use std::collections::{BTreeMap, BTreeSet};
use std::path::Path;

use gc_coreform::{Term, TermOrdKey, hash_term};
use gc_kernel::{Apply, EvalCtx, MemLimits, Value};
use gc_prelude::{
    SelfhostBootstrapMode, build_prelude, load_selfhost_coreform_toolchain_v1_with_mode,
};
use thiserror::Error;

use crate::policy::{CapsPolicy, SelfhostAuthorityConfig};

const BINDING: &str = "core/commit::authority";
const REQUEST_KIND: &str = "genesis/commit-authority-request-v0.1";
const RESULT_KIND: &str = "genesis/commit-authority-result-v0.1";
const STEP_LIMIT: u64 = 20_000_000;
const ALLOC_LIMIT: u64 = 80_000_000;
const MAX_ITEMS: usize = 65_536;

#[derive(Debug, Error)]
pub enum CommitAuthorityError {
    #[error("commit authority bootstrap failed: {0}")]
    Bootstrap(String),
    #[error("commit authority evaluation failed: {0}")]
    Evaluation(String),
    #[error("commit authority protocol error: {0}")]
    Protocol(String),
    #[error("commit authority rejected [{code}]: {message}")]
    Rejected { code: String, message: String },
}

pub struct CommitAuthority {
    context: EvalCtx,
    authority: Value,
}

#[derive(Debug)]
pub(crate) struct ValidatedCommit {
    pub(crate) parents: Vec<String>,
    pub(crate) base: Option<String>,
    pub(crate) patch: String,
    pub(crate) result: String,
    pub(crate) obligations: Vec<Term>,
    pub(crate) evidence: Vec<String>,
    pub(crate) attestations: Vec<String>,
    pub(crate) message: String,
    pub(crate) target: Term,
    pub(crate) author: Term,
    pub(crate) why: Term,
}

impl CommitAuthority {
    pub fn load(
        bootstrap_mode: SelfhostBootstrapMode,
        artifact: Option<&Path>,
    ) -> Result<Self, CommitAuthorityError> {
        let mut context = EvalCtx::with_step_limit(None);
        context.set_mem_limits(MemLimits {
            max_alloc_units: Some(ALLOC_LIMIT),
            max_bytes_len: Some(4 * 1024 * 1024),
            max_map_len: Some(MAX_ITEMS as u64),
            max_string_len: Some(4 * 1024 * 1024),
            max_vec_len: Some(MAX_ITEMS as u64),
            ..MemLimits::default()
        });
        let prelude = build_prelude(&mut context);
        let mut environment = prelude.env;
        load_selfhost_coreform_toolchain_v1_with_mode(
            &mut context,
            &mut environment,
            bootstrap_mode,
            artifact,
        )
        .map_err(|error| CommitAuthorityError::Bootstrap(format!("{error:#}")))?;
        let authority = environment
            .get(BINDING)
            .ok_or_else(|| CommitAuthorityError::Bootstrap(format!("missing binding {BINDING}")))?;
        context.reset_counters();
        context.step_limit = Some(STEP_LIMIT);
        Ok(Self { context, authority })
    }

    pub(crate) fn load_config(
        config: &SelfhostAuthorityConfig,
    ) -> Result<Self, CommitAuthorityError> {
        Self::load(config.bootstrap_mode, config.artifact.as_deref())
    }

    pub(crate) fn load_policy(policy: &CapsPolicy) -> Result<Self, CommitAuthorityError> {
        let config = policy.selfhost_authority_config().ok_or_else(|| {
            CommitAuthorityError::Bootstrap(
                "operation requires the artifact-loaded GenesisCode commit authority".to_string(),
            )
        })?;
        Self::load_config(config)
    }

    pub(crate) fn validate_typed_commit(
        policy: &CapsPolicy,
        authority: &mut Option<Self>,
        artifact: &Term,
    ) -> Result<Option<ValidatedCommit>, CommitAuthorityError> {
        if !is_typed_commit(artifact) {
            return Ok(None);
        }
        if authority.is_none() {
            *authority = Some(Self::load_policy(policy)?);
        }
        let Some(authority) = authority.as_mut() else {
            return Err(protocol("commit authority cache remained uninitialized"));
        };
        authority.validate_commit(artifact.clone()).map(Some)
    }

    pub fn make(&mut self, payload: Term) -> Result<Term, CommitAuthorityError> {
        self.evaluate(":make", payload, None)
    }

    pub fn validate(&mut self, artifact: Term) -> Result<Term, CommitAuthorityError> {
        self.evaluate(
            ":validate",
            map([(":artifact", artifact.clone())]),
            Some(&artifact),
        )
    }

    pub(crate) fn validate_commit(
        &mut self,
        artifact: Term,
    ) -> Result<ValidatedCommit, CommitAuthorityError> {
        let artifact = self.validate(artifact)?;
        reify_commit(&artifact)
    }

    fn evaluate(
        &mut self,
        op: &str,
        payload: Term,
        expected_artifact: Option<&Term>,
    ) -> Result<Term, CommitAuthorityError> {
        let request = map([
            (":kind", Term::Str(REQUEST_KIND.to_string())),
            (":op", Term::symbol(op)),
            (":payload", payload),
            (":v", Term::Int(1.into())),
        ]);
        let request_hash = hex32(hash_term(&request));
        self.context.reset_counters();
        self.context.step_limit = Some(STEP_LIMIT);
        let value = self
            .authority
            .clone()
            .apply(&mut self.context, Value::data(request))
            .map_err(|error| CommitAuthorityError::Evaluation(error.to_string()))?;
        decode_result(value, &request_hash, expected_artifact)
    }
}

fn is_typed_commit(term: &Term) -> bool {
    let Term::Map(fields) = term else {
        return false;
    };
    matches!(
        fields.get(&TermOrdKey(Term::symbol(":type"))),
        Some(Term::Symbol(kind)) if kind == ":vcs/commit"
    )
}

fn decode_result(
    value: Value,
    request_hash: &str,
    expected_artifact: Option<&Term>,
) -> Result<Term, CommitAuthorityError> {
    let Some(Term::Map(fields)) = value.to_plain_term() else {
        return Err(protocol(format!(
            "{BINDING} returned non-map: {}",
            value.debug_repr()
        )));
    };
    let expected = [
        ":artifact",
        ":code",
        ":kind",
        ":message",
        ":ok",
        ":request-h",
        ":v",
    ]
    .into_iter()
    .map(|name| TermOrdKey(Term::symbol(name)))
    .collect::<BTreeSet<_>>();
    if fields.keys().cloned().collect::<BTreeSet<_>>() != expected {
        return Err(protocol("result field set mismatch"));
    }
    require_string(&fields, ":kind", RESULT_KIND)?;
    require_int(&fields, ":v", 1)?;
    require_string(&fields, ":request-h", request_hash)?;
    if !required_bool(&fields, ":ok")? {
        require_nil(&fields, ":artifact")?;
        return Err(CommitAuthorityError::Rejected {
            code: required_string(&fields, ":code")?.to_string(),
            message: required_string(&fields, ":message")?.to_string(),
        });
    }
    require_nil(&fields, ":code")?;
    require_nil(&fields, ":message")?;
    let artifact = match field(&fields, ":artifact")? {
        artifact @ Term::Map(_) => artifact.clone(),
        _ => return Err(protocol("successful result artifact must be a map")),
    };
    if expected_artifact.is_some_and(|expected| expected != &artifact) {
        return Err(protocol(
            "validation result substituted the submitted artifact",
        ));
    }
    Ok(artifact)
}

fn reify_commit(term: &Term) -> Result<ValidatedCommit, CommitAuthorityError> {
    let Term::Map(fields) = term else {
        return Err(protocol("validated commit must be a map"));
    };
    let allowed = [
        ":attestations",
        ":author",
        ":base",
        ":evidence",
        ":message",
        ":obligations",
        ":parents",
        ":patch",
        ":result",
        ":target",
        ":type",
        ":v",
        ":why",
    ]
    .into_iter()
    .map(|name| TermOrdKey(Term::symbol(name)))
    .collect::<BTreeSet<_>>();
    if fields.keys().any(|key| !allowed.contains(key)) {
        return Err(protocol("validated commit contains an unknown field"));
    }
    require_symbol(fields, ":type", ":vcs/commit")?;
    require_int(fields, ":v", 1)?;
    Ok(ValidatedCommit {
        parents: required_hash_vector(fields, ":parents")?,
        base: optional_hash(fields, ":base")?,
        patch: required_hash(fields, ":patch")?,
        result: required_hash(fields, ":result")?,
        obligations: required_vector(fields, ":obligations")?.to_vec(),
        evidence: required_hash_vector(fields, ":evidence")?,
        attestations: required_hash_vector(fields, ":attestations")?,
        message: required_string(fields, ":message")?.to_string(),
        target: optional_field(fields, ":target"),
        author: optional_field(fields, ":author"),
        why: optional_field(fields, ":why"),
    })
}

fn map<const N: usize>(entries: [(&str, Term); N]) -> Term {
    Term::Map(
        entries
            .into_iter()
            .map(|(name, value)| (TermOrdKey(Term::symbol(name)), value))
            .collect(),
    )
}

fn field<'a>(
    fields: &'a BTreeMap<TermOrdKey, Term>,
    name: &str,
) -> Result<&'a Term, CommitAuthorityError> {
    fields
        .get(&TermOrdKey(Term::symbol(name)))
        .ok_or_else(|| protocol(format!("missing field {name}")))
}

fn optional_field(fields: &BTreeMap<TermOrdKey, Term>, name: &str) -> Term {
    fields
        .get(&TermOrdKey(Term::symbol(name)))
        .cloned()
        .unwrap_or(Term::Nil)
}

fn require_string(
    fields: &BTreeMap<TermOrdKey, Term>,
    name: &str,
    expected: &str,
) -> Result<(), CommitAuthorityError> {
    match field(fields, name)? {
        Term::Str(value) if value == expected => Ok(()),
        _ => Err(protocol(format!("field {name} mismatch"))),
    }
}

fn required_string<'a>(
    fields: &'a BTreeMap<TermOrdKey, Term>,
    name: &str,
) -> Result<&'a str, CommitAuthorityError> {
    match field(fields, name)? {
        Term::Str(value) => Ok(value),
        _ => Err(protocol(format!("field {name} must be a string"))),
    }
}

fn require_symbol(
    fields: &BTreeMap<TermOrdKey, Term>,
    name: &str,
    expected: &str,
) -> Result<(), CommitAuthorityError> {
    match field(fields, name)? {
        Term::Symbol(value) if value == expected => Ok(()),
        _ => Err(protocol(format!("field {name} mismatch"))),
    }
}

fn require_int(
    fields: &BTreeMap<TermOrdKey, Term>,
    name: &str,
    expected: i64,
) -> Result<(), CommitAuthorityError> {
    match field(fields, name)? {
        Term::Int(value) if value.to_string() == expected.to_string() => Ok(()),
        _ => Err(protocol(format!("field {name} mismatch"))),
    }
}

fn required_bool(
    fields: &BTreeMap<TermOrdKey, Term>,
    name: &str,
) -> Result<bool, CommitAuthorityError> {
    match field(fields, name)? {
        Term::Bool(value) => Ok(*value),
        _ => Err(protocol(format!("field {name} must be a bool"))),
    }
}

fn require_nil(
    fields: &BTreeMap<TermOrdKey, Term>,
    name: &str,
) -> Result<(), CommitAuthorityError> {
    match field(fields, name)? {
        Term::Nil => Ok(()),
        _ => Err(protocol(format!("field {name} must be nil"))),
    }
}

fn required_vector<'a>(
    fields: &'a BTreeMap<TermOrdKey, Term>,
    name: &str,
) -> Result<&'a [Term], CommitAuthorityError> {
    match field(fields, name)? {
        Term::Vector(values) if values.len() <= MAX_ITEMS => Ok(values),
        Term::Vector(_) => Err(protocol(format!("field {name} exceeds item bound"))),
        _ => Err(protocol(format!("field {name} must be a vector"))),
    }
}

fn required_hash(
    fields: &BTreeMap<TermOrdKey, Term>,
    name: &str,
) -> Result<String, CommitAuthorityError> {
    match field(fields, name)? {
        Term::Str(value) if is_hash(value) => Ok(value.clone()),
        _ => Err(protocol(format!("field {name} must be a lowercase hash"))),
    }
}

fn optional_hash(
    fields: &BTreeMap<TermOrdKey, Term>,
    name: &str,
) -> Result<Option<String>, CommitAuthorityError> {
    match field(fields, name)? {
        Term::Nil => Ok(None),
        Term::Str(value) if is_hash(value) => Ok(Some(value.clone())),
        _ => Err(protocol(format!(
            "field {name} must be nil or a lowercase hash"
        ))),
    }
}

fn required_hash_vector(
    fields: &BTreeMap<TermOrdKey, Term>,
    name: &str,
) -> Result<Vec<String>, CommitAuthorityError> {
    required_vector(fields, name)?
        .iter()
        .map(|value| match value {
            Term::Str(value) if is_hash(value) => Ok(value.clone()),
            _ => Err(protocol(format!(
                "field {name} entries must be lowercase hashes"
            ))),
        })
        .collect()
}

fn is_hash(value: &str) -> bool {
    value.len() == 64
        && value
            .as_bytes()
            .iter()
            .all(|byte| byte.is_ascii_digit() || matches!(byte, b'a'..=b'f'))
}

fn protocol(message: impl Into<String>) -> CommitAuthorityError {
    CommitAuthorityError::Protocol(message.into())
}

fn hex32(bytes: [u8; 32]) -> String {
    bytes.iter().map(|byte| format!("{byte:02x}")).collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use gc_kernel::ValueMap;

    fn accepted(artifact: Term, request_hash: &str) -> Value {
        Value::data(map([
            (":artifact", artifact),
            (":code", Term::Nil),
            (":kind", Term::Str(RESULT_KIND.to_string())),
            (":message", Term::Nil),
            (":ok", Term::Bool(true)),
            (":request-h", Term::Str(request_hash.to_string())),
            (":v", Term::Int(1.into())),
        ]))
    }

    #[test]
    fn strict_decoder_rejects_open_unbound_and_substituted_results() {
        let artifact = Term::Map(BTreeMap::new());
        let mut open = match accepted(artifact.clone(), &"0".repeat(64)).to_plain_term() {
            Some(Term::Map(fields)) => fields,
            _ => panic!("accepted fixture must be a map"),
        };
        open.insert(TermOrdKey(Term::symbol(":extra")), Term::Nil);
        assert!(decode_result(Value::data(Term::Map(open)), &"0".repeat(64), None).is_err());
        assert!(
            decode_result(
                accepted(artifact.clone(), &"0".repeat(64)),
                &"1".repeat(64),
                None,
            )
            .is_err()
        );
        let submitted = map([(":submitted", Term::Bool(true))]);
        assert!(
            decode_result(
                accepted(artifact, &"0".repeat(64)),
                &"0".repeat(64),
                Some(&submitted),
            )
            .is_err()
        );
    }

    #[test]
    fn strict_decoder_accepts_runtime_map_results() {
        let mut value = ValueMap::new();
        value.insert_mut(
            TermOrdKey(Term::symbol(":artifact")),
            Value::data(Term::Map(BTreeMap::new())),
        );
        value.insert_mut(TermOrdKey(Term::symbol(":code")), Value::data(Term::Nil));
        value.insert_mut(
            TermOrdKey(Term::symbol(":kind")),
            Value::data(Term::Str(RESULT_KIND.to_string())),
        );
        value.insert_mut(TermOrdKey(Term::symbol(":message")), Value::data(Term::Nil));
        value.insert_mut(
            TermOrdKey(Term::symbol(":ok")),
            Value::data(Term::Bool(true)),
        );
        value.insert_mut(
            TermOrdKey(Term::symbol(":request-h")),
            Value::data(Term::Str("0".repeat(64))),
        );
        value.insert_mut(TermOrdKey(Term::symbol(":v")), Value::int(1));
        assert!(matches!(
            decode_result(Value::map(value), &"0".repeat(64), None),
            Ok(Term::Map(_))
        ));
    }

    #[test]
    fn structural_view_preserves_symbol_obligations_and_rejects_unknown_fields() {
        let hash = "a".repeat(64);
        let commit = map([
            (":attestations", Term::Vector(Vec::new())),
            (":base", Term::Nil),
            (":evidence", Term::Vector(Vec::new())),
            (":message", Term::Str("message".to_string())),
            (
                ":obligations",
                Term::Vector(vec![Term::symbol(":proof/required")]),
            ),
            (":parents", Term::Vector(Vec::new())),
            (":patch", Term::Str(hash.clone())),
            (":result", Term::Str(hash)),
            (":type", Term::symbol(":vcs/commit")),
            (":v", Term::Int(1.into())),
        ]);
        let view = reify_commit(&commit).expect("closed authority artifact should reify");
        assert_eq!(view.obligations, vec![Term::symbol(":proof/required")]);

        let Term::Map(mut open) = commit else {
            panic!("commit fixture must be a map");
        };
        open.insert(TermOrdKey(Term::symbol(":extra")), Term::Nil);
        assert!(reify_commit(&Term::Map(open)).is_err());
    }

    #[test]
    fn typed_commit_classifier_is_exact_and_does_not_capture_other_objects() {
        assert!(is_typed_commit(&map([(
            ":type",
            Term::symbol(":vcs/commit")
        )])));
        assert!(!is_typed_commit(&map([(
            ":type",
            Term::symbol(":vcs/snapshot")
        )])));
        assert!(!is_typed_commit(&map([(
            ":type",
            Term::Str(":vcs/commit".to_string())
        )])));
        assert!(!is_typed_commit(&Term::Nil));
    }
}
