use std::collections::BTreeMap;
use std::path::Path;

use gc_coreform::{Term, TermOrdKey, hash_term, print_term};
use gc_kernel::{Apply, EvalCtx, MemLimits, Value};
use gc_prelude::{
    SelfhostBootstrapMode, build_prelude, load_selfhost_coreform_toolchain_v1_with_mode,
};
use thiserror::Error;

const BINDING: &str = "core/security::evidence-verify-authority";
const REQUEST_KIND: &str = "genesis/evidence-verification-authority-request-v0.1";
const RESULT_KIND: &str = "genesis/evidence-verification-authority-result-v0.1";
const STEP_LIMIT: u64 = 20_000_000;
const ALLOC_LIMIT: u64 = 64_000_000;

#[derive(Debug, Error)]
pub enum EvidenceVerifyError {
    #[error("selfhost evidence-verification authority error: {0}")]
    Authority(String),
}

#[derive(Debug, Clone)]
pub struct EvidenceFact {
    pub class: &'static str,
    pub code: String,
    pub mechanism_ok: bool,
    pub observed: Term,
    pub required: Term,
}

#[derive(Debug, Clone)]
pub struct TransparencyEntryObservation {
    pub hash: [u8; 32],
    pub store_valid: bool,
    pub load_error: Option<String>,
    pub term: Term,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct EvidenceDecision {
    pub verified: bool,
    pub errors: Vec<String>,
    pub checked: usize,
}

#[derive(Debug, Clone)]
pub struct DsseVerificationFacts<'a> {
    pub envelope_closed: bool,
    pub expected_key_id: &'a str,
    pub expected_payload_type: &'a str,
    pub key_id: &'a str,
    pub key_valid: bool,
    pub payload_hash: [u8; 32],
    pub payload_type: &'a str,
    pub signature_count: usize,
    pub signature_fields_closed: bool,
    pub signature_key_id: &'a str,
    pub signature_valid: bool,
}

pub struct EvidenceVerifyAuthority {
    context: EvalCtx,
    authority: Value,
}

impl EvidenceVerifyAuthority {
    pub fn load(artifact: &Path) -> Result<Self, EvidenceVerifyError> {
        let mut context = EvalCtx::with_step_limit(None);
        context.set_mem_limits(MemLimits {
            max_alloc_units: Some(ALLOC_LIMIT),
            max_bytes_len: Some(16 * 1024 * 1024 + 1024),
            max_map_len: Some(32),
            max_string_len: Some(16 * 1024 * 1024 + 1024),
            max_vec_len: Some(16_384),
            ..MemLimits::default()
        });
        let prelude = build_prelude(&mut context);
        let mut environment = prelude.env;
        load_selfhost_coreform_toolchain_v1_with_mode(
            &mut context,
            &mut environment,
            SelfhostBootstrapMode::ArtifactOnly,
            Some(artifact),
        )
        .map_err(|error| authority_error(format!("artifact bootstrap failed: {error}")))?;
        let authority = environment
            .get(BINDING)
            .ok_or_else(|| authority_error(format!("missing binding {BINDING}")))?;
        context.reset_counters();
        context.step_limit = Some(STEP_LIMIT);
        Ok(Self { context, authority })
    }

    pub fn package(
        &mut self,
        facts: Vec<EvidenceFact>,
    ) -> Result<EvidenceDecision, EvidenceVerifyError> {
        let checked = facts.len();
        let facts = Term::Vector(facts.into_iter().map(fact_term).collect());
        let data = self.decide(request(":package", [(":facts", facts)]))?;
        decode_decision(data, checked)
    }

    pub fn transparency(
        &mut self,
        head: Option<[u8; 32]>,
        head_error: Option<String>,
        entries: Vec<TransparencyEntryObservation>,
    ) -> Result<EvidenceDecision, EvidenceVerifyError> {
        let data = self.decide(request(
            ":transparency",
            [
                (
                    ":entries",
                    Term::Vector(entries.into_iter().map(transparency_entry_term).collect()),
                ),
                (":head", head.map(bytes32).unwrap_or(Term::Nil)),
                (
                    ":head-error",
                    head_error.map(Term::Str).unwrap_or(Term::Nil),
                ),
            ],
        ))?;
        let fields = exact_map(
            &data,
            "transparency decision",
            &[":entries", ":errors", ":head", ":verified"],
        )?;
        let checked = required_usize(fields, ":entries", "transparency decision")?;
        Ok(EvidenceDecision {
            verified: required_bool(fields, ":verified", "transparency decision")?,
            errors: required_strings(fields, ":errors", "transparency decision")?,
            checked,
        })
    }

    pub fn dsse(
        &mut self,
        facts: DsseVerificationFacts<'_>,
    ) -> Result<EvidenceDecision, EvidenceVerifyError> {
        let data = self.decide(request(
            ":dsse",
            [
                (":envelope-closed", Term::Bool(facts.envelope_closed)),
                (
                    ":expected-key-id",
                    Term::Str(facts.expected_key_id.to_string()),
                ),
                (
                    ":expected-payload-type",
                    Term::Str(facts.expected_payload_type.to_string()),
                ),
                (":key-id", Term::Str(facts.key_id.to_string())),
                (":key-valid", Term::Bool(facts.key_valid)),
                (":payload-h", bytes32(facts.payload_hash)),
                (":payload-type", Term::Str(facts.payload_type.to_string())),
                (":signature-count", Term::Int(facts.signature_count.into())),
                (
                    ":signature-fields-closed",
                    Term::Bool(facts.signature_fields_closed),
                ),
                (
                    ":signature-key-id",
                    Term::Str(facts.signature_key_id.to_string()),
                ),
                (":signature-valid", Term::Bool(facts.signature_valid)),
            ],
        ))?;
        decode_decision(data, 1)
    }

    fn decide(&mut self, request: Term) -> Result<Term, EvidenceVerifyError> {
        let request_hash = hash_term(&request);
        let value = self
            .authority
            .clone()
            .apply(&mut self.context, Value::data(request))
            .map_err(|error| authority_error(format!("apply failed: {error}")))?;
        let term = match &value {
            Value::Sealed { token, payload }
                if self
                    .context
                    .protocol
                    .is_some_and(|protocol| *token == protocol.error) =>
            {
                let detail = payload
                    .to_plain_term()
                    .map(|term| print_term(&term))
                    .unwrap_or_else(|| "<opaque-error-payload>".to_string());
                return Err(authority_error(format!("returned sealed ERROR {detail}")));
            }
            _ => value
                .to_plain_term()
                .ok_or_else(|| authority_error(format!("returned opaque value: {value:?}")))?,
        };
        decode_result(term, request_hash)
    }
}

fn authority_error(message: impl Into<String>) -> EvidenceVerifyError {
    EvidenceVerifyError::Authority(message.into())
}

fn key(name: &str) -> TermOrdKey {
    TermOrdKey(Term::symbol(name))
}

fn map(entries: impl IntoIterator<Item = (&'static str, Term)>) -> Term {
    Term::Map(
        entries
            .into_iter()
            .map(|(name, value)| (key(name), value))
            .collect(),
    )
}

fn bytes32(value: [u8; 32]) -> Term {
    Term::Bytes(value.to_vec().into())
}

fn request(phase: &'static str, entries: impl IntoIterator<Item = (&'static str, Term)>) -> Term {
    let mut fields = vec![
        (":kind", Term::Str(REQUEST_KIND.to_string())),
        (":phase", Term::symbol(phase)),
        (":v", Term::Int(1.into())),
    ];
    fields.extend(entries);
    map(fields)
}

fn fact_term(fact: EvidenceFact) -> Term {
    map([
        (":class", Term::symbol(fact.class)),
        (":code", Term::Str(fact.code)),
        (":mechanism-ok", Term::Bool(fact.mechanism_ok)),
        (":observed", fact.observed),
        (":required", fact.required),
    ])
}

fn transparency_entry_term(entry: TransparencyEntryObservation) -> Term {
    map([
        (":hash", bytes32(entry.hash)),
        (
            ":load-error",
            entry.load_error.map(Term::Str).unwrap_or(Term::Nil),
        ),
        (":store-valid", Term::Bool(entry.store_valid)),
        (":term", entry.term),
    ])
}

fn decode_result(term: Term, request_hash: [u8; 32]) -> Result<Term, EvidenceVerifyError> {
    let fields = exact_map(
        &term,
        "authority result",
        &[
            ":code",
            ":data",
            ":kind",
            ":message",
            ":ok",
            ":request-h",
            ":v",
        ],
    )?;
    require_string(fields, ":kind", "authority result", RESULT_KIND)?;
    require_int_one(fields, ":v", "authority result")?;
    require_string(
        fields,
        ":request-h",
        "authority result",
        &hex32(request_hash),
    )?;
    if !required_bool(fields, ":ok", "authority result")? {
        let code =
            optional_string(fields, ":code", "authority result")?.unwrap_or("evidence/rejected");
        let message = optional_string(fields, ":message", "authority result")?
            .unwrap_or("authority rejected request");
        return Err(authority_error(format!("{code}: {message}")));
    }
    if !matches!(fields.get(&key(":code")), Some(Term::Nil))
        || !matches!(fields.get(&key(":message")), Some(Term::Nil))
    {
        return Err(authority_error(
            "accepted result must have nil code and message",
        ));
    }
    fields
        .get(&key(":data"))
        .cloned()
        .ok_or_else(|| authority_error("result missing :data"))
}

fn decode_decision(data: Term, checked: usize) -> Result<EvidenceDecision, EvidenceVerifyError> {
    let fields = exact_map(
        &data,
        "evidence decision",
        &[":checked", ":errors", ":verified"],
    )
    .or_else(|_| {
        exact_map(
            &data,
            "evidence decision",
            &[
                ":errors",
                ":key-id",
                ":payload-h",
                ":payload-type",
                ":verified",
            ],
        )
    })?;
    if let Some(value) = fields.get(&key(":checked")) {
        if integer_usize(value) != Some(checked) {
            return Err(authority_error("authority checked-count mismatch"));
        }
    }
    let errors = required_strings(fields, ":errors", "evidence decision")?;
    let verified = required_bool(fields, ":verified", "evidence decision")?;
    if verified != errors.is_empty() {
        return Err(authority_error("authority verdict/error inconsistency"));
    }
    Ok(EvidenceDecision {
        verified,
        errors,
        checked,
    })
}

fn exact_map<'a>(
    term: &'a Term,
    context: &str,
    names: &[&str],
) -> Result<&'a BTreeMap<TermOrdKey, Term>, EvidenceVerifyError> {
    let Term::Map(fields) = term else {
        return Err(authority_error(format!("{context} must be a map")));
    };
    let expected = names
        .iter()
        .map(|name| key(name))
        .collect::<std::collections::BTreeSet<_>>();
    let actual = fields
        .keys()
        .cloned()
        .collect::<std::collections::BTreeSet<_>>();
    if actual != expected {
        return Err(authority_error(format!("{context} field set mismatch")));
    }
    Ok(fields)
}

fn required_bool(
    fields: &BTreeMap<TermOrdKey, Term>,
    name: &str,
    context: &str,
) -> Result<bool, EvidenceVerifyError> {
    match fields.get(&key(name)) {
        Some(Term::Bool(value)) => Ok(*value),
        _ => Err(authority_error(format!("{context} {name} must be bool"))),
    }
}

fn require_string(
    fields: &BTreeMap<TermOrdKey, Term>,
    name: &str,
    context: &str,
    expected: &str,
) -> Result<(), EvidenceVerifyError> {
    match fields.get(&key(name)) {
        Some(Term::Str(value)) if value == expected => Ok(()),
        _ => Err(authority_error(format!("{context} {name} mismatch"))),
    }
}

fn optional_string<'a>(
    fields: &'a BTreeMap<TermOrdKey, Term>,
    name: &str,
    context: &str,
) -> Result<Option<&'a str>, EvidenceVerifyError> {
    match fields.get(&key(name)) {
        Some(Term::Nil) => Ok(None),
        Some(Term::Str(value)) => Ok(Some(value)),
        _ => Err(authority_error(format!(
            "{context} {name} must be string or nil"
        ))),
    }
}

fn require_int_one(
    fields: &BTreeMap<TermOrdKey, Term>,
    name: &str,
    context: &str,
) -> Result<(), EvidenceVerifyError> {
    match fields.get(&key(name)) {
        Some(value) if integer_usize(value) == Some(1) => Ok(()),
        _ => Err(authority_error(format!("{context} {name} must be 1"))),
    }
}

fn required_usize(
    fields: &BTreeMap<TermOrdKey, Term>,
    name: &str,
    context: &str,
) -> Result<usize, EvidenceVerifyError> {
    fields
        .get(&key(name))
        .and_then(integer_usize)
        .ok_or_else(|| authority_error(format!("{context} {name} must be a nonnegative usize")))
}

fn integer_usize(value: &Term) -> Option<usize> {
    let Term::Int(value) = value else {
        return None;
    };
    usize::try_from(value.clone()).ok()
}

fn required_strings(
    fields: &BTreeMap<TermOrdKey, Term>,
    name: &str,
    context: &str,
) -> Result<Vec<String>, EvidenceVerifyError> {
    let Some(Term::Vector(values)) = fields.get(&key(name)) else {
        return Err(authority_error(format!("{context} {name} must be vector")));
    };
    values
        .iter()
        .map(|value| match value {
            Term::Str(value) => Ok(value.clone()),
            _ => Err(authority_error(format!(
                "{context} {name} entries must be strings"
            ))),
        })
        .collect()
}

fn hex32(value: [u8; 32]) -> String {
    value
        .into_iter()
        .map(|byte| format!("{byte:02x}"))
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::PathBuf;

    fn fixture_authority() -> EvidenceVerifyAuthority {
        let artifact = std::env::var_os("GENESIS_TEST_SELFHOST_ARTIFACT")
            .map(PathBuf::from)
            .unwrap_or_else(|| {
                PathBuf::from(env!("CARGO_MANIFEST_DIR"))
                    .join("../..")
                    .join("selfhost/toolchain.gc")
            });
        EvidenceVerifyAuthority::load(&artifact).expect("load evidence authority")
    }

    fn transparency_term(previous: [u8; 32]) -> Term {
        map([
            (":acceptance-artifact", Term::Str("aa".repeat(32))),
            (
                ":kind",
                Term::Str("genesis/transparency-entry-v0.2".to_string()),
            ),
            (":package-artifact", Term::Str("bb".repeat(32))),
            (":prev-h", bytes32(previous)),
            (":signature-artifact", Term::Str("cc".repeat(32))),
            (":signer-pk-b64", Term::Str("fixture-key".to_string())),
        ])
    }

    #[test]
    fn package_identity_and_mechanism_facts_control_consumed_verdict() {
        let mut authority = fixture_authority();
        let accepted = authority
            .package(vec![EvidenceFact {
                class: ":identity",
                code: "fixture/identity".to_string(),
                mechanism_ok: true,
                observed: Term::Str("same".to_string()),
                required: Term::Str("same".to_string()),
            }])
            .expect("valid fact request");
        assert!(accepted.verified);

        let rejected = authority
            .package(vec![EvidenceFact {
                class: ":identity",
                code: "fixture/identity".to_string(),
                mechanism_ok: true,
                observed: Term::Str("changed".to_string()),
                required: Term::Str("same".to_string()),
            }])
            .expect("semantic denial is a valid authority result");
        assert_eq!(rejected.errors, vec!["fixture/identity"]);
        assert!(!rejected.verified);
    }

    #[test]
    fn transparency_authority_rejects_cycles_even_when_mechanisms_report_valid() {
        let mut authority = fixture_authority();
        let hash = [7; 32];
        let observation = TransparencyEntryObservation {
            hash,
            store_valid: true,
            load_error: None,
            term: transparency_term(hash),
        };
        let decision = authority
            .transparency(Some(hash), None, vec![observation.clone(), observation])
            .expect("cycle request");
        assert!(!decision.verified);
        assert_eq!(decision.errors, vec!["transparency/cycle"]);
    }

    #[test]
    fn dsse_authority_requires_every_closed_identity_and_crypto_fact() {
        let mut authority = fixture_authority();
        let valid = DsseVerificationFacts {
            envelope_closed: true,
            expected_key_id: "sha256:key",
            expected_payload_type: "fixture/type",
            key_id: "sha256:key",
            key_valid: true,
            payload_hash: [9; 32],
            payload_type: "fixture/type",
            signature_count: 1,
            signature_fields_closed: true,
            signature_key_id: "sha256:key",
            signature_valid: true,
        };
        assert!(
            authority
                .dsse(valid.clone())
                .expect("valid DSSE facts")
                .verified
        );
        let mut invalid = valid;
        invalid.signature_valid = false;
        assert!(
            !authority
                .dsse(invalid)
                .expect("invalid DSSE facts")
                .verified
        );
    }
}
