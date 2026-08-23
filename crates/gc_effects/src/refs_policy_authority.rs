use gc_coreform::{Term, TermOrdKey, hash_term, parse_term, print_term};
use gc_kernel::{Apply, Value};

use super::*;
use crate::pkg_lock_read_authority::{
    PkgPublishObject, mechanical_signing_hash, verify_crypto_request,
};
use crate::store::{ArtifactObservation, ArtifactStore};

pub(super) const POLICY_BINDING: &str = "core/refs::policy-authority";
const REQUEST_KIND: &str = "genesis/refs-policy-authority-request-v0.1";
const RESULT_KIND: &str = "genesis/refs-policy-authority-result-v0.1";
const POLICY_STEP_LIMIT: u64 = 50_000_000;
const MAX_OBJECT_BYTES: usize = 4 * 1024 * 1024;
const MAX_TOTAL_BYTES: usize = 64 * 1024 * 1024;
const MAX_OBJECTS: usize = 4096;

#[derive(Debug, Clone, Eq, PartialEq)]
pub(crate) enum RefsPolicyDecision {
    Accept,
    Error { code: String, message: String },
}

enum PhaseResult {
    Accept(Term),
    Error { code: String, message: String },
}

#[derive(Clone, Copy)]
enum ObjectRole {
    Policy,
    Commit,
    Evidence,
    Attestation,
}

impl RefsAuthority {
    pub(crate) fn validate_policy_gate(
        &mut self,
        store: &ArtifactStore,
        name: &str,
        new_hash: Option<&str>,
        policy_hash: &str,
    ) -> Result<RefsPolicyDecision, EffectsError> {
        let mut observed_bytes = 0usize;
        let policy = match load_object(store, policy_hash, ObjectRole::Policy, &mut observed_bytes)?
        {
            Ok(object) => object,
            Err(decision) => return Ok(decision),
        };
        let commit = if let Some(commit_hash) = new_hash {
            match load_object(store, commit_hash, ObjectRole::Commit, &mut observed_bytes)? {
                Ok(object) => Some(object),
                Err(decision) => return Ok(decision),
            }
        } else {
            None
        };
        let facts = policy_facts(name, &policy, commit.as_ref());
        let Some(commit) = commit else {
            return match self.evaluate_policy(":delete", facts, Term::Nil)? {
                PhaseResult::Accept(value) => {
                    decode_admission(value, name, None, policy_hash)?;
                    Ok(RefsPolicyDecision::Accept)
                }
                PhaseResult::Error { code, message } => {
                    Ok(RefsPolicyDecision::Error { code, message })
                }
            };
        };

        let (attestation_hashes, evidence_hashes, inspect_hash) =
            match self.evaluate_policy(":inspect", facts.clone(), Term::Nil)? {
                PhaseResult::Accept(value) => decode_inspection(value)?,
                PhaseResult::Error { code, message } => {
                    return Ok(RefsPolicyDecision::Error { code, message });
                }
            };
        let requested_count = attestation_hashes
            .len()
            .saturating_add(evidence_hashes.len());
        if requested_count > MAX_OBJECTS {
            return Ok(resource_error(
                "refs policy requested objects",
                requested_count,
                MAX_OBJECTS,
            ));
        }
        let evidence = match load_objects(
            store,
            &evidence_hashes,
            ObjectRole::Evidence,
            &mut observed_bytes,
        )? {
            Ok(objects) => objects,
            Err(decision) => return Ok(decision),
        };
        let attestations = match load_objects(
            store,
            &attestation_hashes,
            ObjectRole::Attestation,
            &mut observed_bytes,
        )? {
            Ok(objects) => objects,
            Err(decision) => return Ok(decision),
        };
        let prepare_mechanism = map([
            (
                ":attestations",
                Term::Vector(attestations.iter().map(object_envelope).collect()),
            ),
            (
                ":evidence",
                Term::Vector(evidence.iter().map(object_envelope).collect()),
            ),
            (":inspect-h", Term::Str(inspect_hash.clone())),
        ]);
        let (requests, prepare_hash) =
            match self.evaluate_policy(":prepare", facts.clone(), prepare_mechanism)? {
                PhaseResult::Accept(value) => decode_preparation(value)?,
                PhaseResult::Error { code, message } => {
                    return Ok(RefsPolicyDecision::Error { code, message });
                }
            };
        let signing_hash = mechanical_signing_hash(&commit.term)?;
        let mut crypto_facts = Vec::with_capacity(requests.len());
        for request in &requests {
            let (request_hash, valid) = verify_crypto_request(request, &signing_hash)?;
            crypto_facts.push(map([
                (":request-h", Term::Str(request_hash)),
                (":signature-valid", Term::Bool(valid)),
            ]));
        }
        let finalize_mechanism = map([
            (
                ":attestations",
                Term::Vector(attestations.iter().map(object_envelope).collect()),
            ),
            (":crypto-facts", Term::Vector(crypto_facts)),
            (
                ":evidence",
                Term::Vector(evidence.iter().map(object_envelope).collect()),
            ),
            (":inspect-h", Term::Str(inspect_hash)),
            (":prepare-h", Term::Str(prepare_hash)),
        ]);
        match self.evaluate_policy(":finalize", facts, finalize_mechanism)? {
            PhaseResult::Accept(value) => {
                decode_admission(value, name, Some(&commit.hash), policy_hash)?;
                Ok(RefsPolicyDecision::Accept)
            }
            PhaseResult::Error { code, message } => Ok(RefsPolicyDecision::Error { code, message }),
        }
    }

    fn evaluate_policy(
        &mut self,
        phase: &str,
        facts: Term,
        mechanism: Term,
    ) -> Result<PhaseResult, EffectsError> {
        let request = map([
            (":facts", facts),
            (":kind", Term::Str(REQUEST_KIND.to_string())),
            (":mechanism", mechanism),
            (":phase", Term::symbol(phase)),
            (":v", Term::Int(1.into())),
        ]);
        let request_hash = hash_term(&request);
        self.context.reset_counters();
        self.context.step_limit = Some(POLICY_STEP_LIMIT);
        let value = self
            .policy_authority
            .clone()
            .apply(&mut self.context, Value::data(request))
            .map_err(|error| policy_error(format!("apply failed: {error}")))?;
        let term = plain_result(value, &self.context)?;
        decode_phase_result(term, request_hash)
    }
}

fn policy_facts(name: &str, policy: &PkgPublishObject, commit: Option<&PkgPublishObject>) -> Term {
    map([
        (
            ":commit",
            commit
                .map(|object| object.term.clone())
                .unwrap_or(Term::Nil),
        ),
        (
            ":commit-h",
            commit
                .map(|object| Term::Str(object.hash.clone()))
                .unwrap_or(Term::Nil),
        ),
        (":depth", Term::Int(0.into())),
        (":expected-old", Term::Nil),
        (":policy", policy.term.clone()),
        (":policy-h", Term::Str(policy.hash.clone())),
        (":ref", Term::Str(name.to_string())),
        (":remote", Term::Str("local".to_string())),
    ])
}

fn object_envelope(object: &PkgPublishObject) -> Term {
    map([
        (":bytes", Term::Bytes(object.bytes.clone().into())),
        (":h", Term::Str(object.hash.clone())),
        (":term", object.term.clone()),
    ])
}

fn load_objects(
    store: &ArtifactStore,
    hashes: &[String],
    role: ObjectRole,
    observed_bytes: &mut usize,
) -> Result<Result<Vec<PkgPublishObject>, RefsPolicyDecision>, EffectsError> {
    let mut objects = Vec::with_capacity(hashes.len());
    for hash in hashes {
        match load_object(store, hash, role, observed_bytes)? {
            Ok(object) => objects.push(object),
            Err(decision) => return Ok(Err(decision)),
        }
    }
    Ok(Ok(objects))
}

fn load_object(
    store: &ArtifactStore,
    hash: &str,
    role: ObjectRole,
    observed_bytes: &mut usize,
) -> Result<Result<PkgPublishObject, RefsPolicyDecision>, EffectsError> {
    if !lowercase_hash(hash) {
        return Ok(Err(role_error(
            role,
            format!("artifact hash must be lowercase hex64: {hash}"),
        )));
    }
    let bytes = match store.observe_bytes_limited(hash, MAX_OBJECT_BYTES)? {
        ArtifactObservation::Missing => {
            return Ok(Err(missing_error(role, hash)));
        }
        ArtifactObservation::TooLarge { observed } => {
            return Ok(Err(resource_error(
                "refs policy artifact bytes",
                observed,
                MAX_OBJECT_BYTES,
            )));
        }
        ArtifactObservation::Bytes(bytes) => bytes,
    };
    *observed_bytes = observed_bytes.saturating_add(bytes.len());
    if *observed_bytes > MAX_TOTAL_BYTES {
        return Ok(Err(resource_error(
            "refs policy total artifact bytes",
            *observed_bytes,
            MAX_TOTAL_BYTES,
        )));
    }
    if blake3::hash(&bytes).to_hex().as_str() != hash {
        return Ok(Err(RefsPolicyDecision::Error {
            code: "core/store/corruption".to_string(),
            message: format!("artifact content does not match its hash: {hash}"),
        }));
    }
    let source = match std::str::from_utf8(&bytes) {
        Ok(source) => source,
        Err(_) => {
            return Ok(Err(role_error(
                role,
                format!("artifact is not UTF-8 CoreForm: {hash}"),
            )));
        }
    };
    let term = match parse_term(source) {
        Ok(term) => term,
        Err(error) => {
            return Ok(Err(role_error(
                role,
                format!("artifact is not valid CoreForm: {hash}: {error}"),
            )));
        }
    };
    Ok(Ok(PkgPublishObject {
        hash: hash.to_string(),
        bytes,
        term,
    }))
}

fn missing_error(role: ObjectRole, hash: &str) -> RefsPolicyDecision {
    let (code, label) = match role {
        ObjectRole::Policy => ("core/refs/policy-not-found", "policy"),
        ObjectRole::Commit => ("core/refs/commit-not-found", "commit"),
        ObjectRole::Evidence | ObjectRole::Attestation => ("core/store/not-found", "artifact"),
    };
    RefsPolicyDecision::Error {
        code: code.to_string(),
        message: format!("{label} artifact not found: {hash}"),
    }
}

fn role_error(role: ObjectRole, message: String) -> RefsPolicyDecision {
    let code = match role {
        ObjectRole::Policy => "core/refs/bad-policy",
        ObjectRole::Commit => "core/refs/bad-commit",
        ObjectRole::Evidence => "core/refs/bad-evidence",
        ObjectRole::Attestation => "core/refs/bad-attestation",
    };
    RefsPolicyDecision::Error {
        code: code.to_string(),
        message,
    }
}

fn resource_error(label: &str, observed: usize, limit: usize) -> RefsPolicyDecision {
    RefsPolicyDecision::Error {
        code: "core/caps/resource-limit".to_string(),
        message: format!("{label} exceeds limit: observed={observed} limit={limit}"),
    }
}

fn decode_phase_result(term: Term, request_hash: [u8; 32]) -> Result<PhaseResult, EffectsError> {
    let fields = exact_map(
        &term,
        &[
            ":code",
            ":kind",
            ":message",
            ":ok",
            ":request-h",
            ":v",
            ":value",
        ],
        "authority result",
    )?;
    require_string(fields, ":kind", RESULT_KIND, "authority result")?;
    require_int(fields, ":v", 1, "authority result")?;
    require_string(
        fields,
        ":request-h",
        &hex32(request_hash),
        "authority result",
    )?;
    match field(fields, ":ok", "authority result")? {
        Term::Bool(true) => {
            require_nil(fields, ":code", "authority result")?;
            require_nil(fields, ":message", "authority result")?;
            let value = field(fields, ":value", "authority result")?.clone();
            if matches!(value, Term::Nil) {
                return Err(policy_error("accepted authority result has nil :value"));
            }
            Ok(PhaseResult::Accept(value))
        }
        Term::Bool(false) => {
            require_nil(fields, ":value", "authority result")?;
            let code = string(field(fields, ":code", "authority result")?, ":code")?;
            if !declared_diagnostic(&code) {
                return Err(policy_error(format!(
                    "authority rejection used undeclared diagnostic {code}"
                )));
            }
            let message = string(field(fields, ":message", "authority result")?, ":message")?;
            Ok(PhaseResult::Error { code, message })
        }
        _ => Err(policy_error("authority result :ok must be boolean")),
    }
}

fn decode_inspection(value: Term) -> Result<(Vec<String>, Vec<String>, String), EffectsError> {
    let fields = exact_map(
        &value,
        &[":attestation-hashes", ":evidence-hashes", ":inspect-h"],
        "inspect value",
    )?;
    let attestations = hash_vector(field(fields, ":attestation-hashes", "inspect value")?)?;
    let evidence = hash_vector(field(fields, ":evidence-hashes", "inspect value")?)?;
    let inspect_hash = hash_string(field(fields, ":inspect-h", "inspect value")?, ":inspect-h")?;
    require_embedded_hash(&value, ":inspect-h", &inspect_hash, "inspect value")?;
    Ok((attestations, evidence, inspect_hash))
}

fn decode_preparation(value: Term) -> Result<(Vec<Term>, String), EffectsError> {
    let fields = exact_map(&value, &[":crypto-requests", ":prepare-h"], "prepare value")?;
    let Term::Vector(requests) = field(fields, ":crypto-requests", "prepare value")? else {
        return Err(policy_error("prepare :crypto-requests must be a vector"));
    };
    if requests.len() > MAX_OBJECTS {
        return Err(policy_error("prepare returned too many crypto requests"));
    }
    let prepare_hash = hash_string(field(fields, ":prepare-h", "prepare value")?, ":prepare-h")?;
    require_embedded_hash(&value, ":prepare-h", &prepare_hash, "prepare value")?;
    Ok((requests.clone(), prepare_hash))
}

fn decode_admission(
    value: Term,
    name: &str,
    commit_hash: Option<&str>,
    policy_hash: &str,
) -> Result<(), EffectsError> {
    let fields = exact_map(
        &value,
        &[":admit", ":commit-h", ":policy-h", ":ref"],
        "admission value",
    )?;
    if !matches!(
        field(fields, ":admit", "admission value")?,
        Term::Bool(true)
    ) {
        return Err(policy_error("admission value :admit must be true"));
    }
    match (field(fields, ":commit-h", "admission value")?, commit_hash) {
        (Term::Nil, None) => {}
        (Term::Str(actual), Some(expected)) if actual == expected => {}
        _ => return Err(policy_error("admission value contradicts commit hash")),
    }
    require_string(fields, ":policy-h", policy_hash, "admission value")?;
    require_string(fields, ":ref", name, "admission value")
}

fn require_embedded_hash(
    term: &Term,
    field_name: &str,
    observed: &str,
    context: &str,
) -> Result<(), EffectsError> {
    let Term::Map(fields) = term else {
        return Err(policy_error(format!("{context} must be a map")));
    };
    let mut unhashed = fields.clone();
    unhashed.remove(&TermOrdKey(Term::symbol(field_name)));
    let expected = hex32(hash_term(&Term::Map(unhashed)));
    if observed == expected {
        Ok(())
    } else {
        Err(policy_error(format!("{context} {field_name} mismatch")))
    }
}

fn exact_map<'a>(
    term: &'a Term,
    expected: &[&str],
    context: &str,
) -> Result<&'a BTreeMap<TermOrdKey, Term>, EffectsError> {
    let Term::Map(fields) = term else {
        return Err(policy_error(format!(
            "{context} must be a map, got {}",
            print_term(term)
        )));
    };
    let actual: Vec<String> = fields
        .keys()
        .map(|key| match &key.0 {
            Term::Symbol(value) => value.clone(),
            other => print_term(other),
        })
        .collect();
    let wanted: Vec<String> = expected.iter().map(|value| (*value).to_string()).collect();
    if actual == wanted {
        Ok(fields)
    } else {
        Err(policy_error(format!(
            "{context} field set mismatch: actual={actual:?} expected={wanted:?}"
        )))
    }
}

fn field<'a>(
    fields: &'a BTreeMap<TermOrdKey, Term>,
    name: &str,
    context: &str,
) -> Result<&'a Term, EffectsError> {
    fields
        .get(&TermOrdKey(Term::symbol(name)))
        .ok_or_else(|| policy_error(format!("{context} missing {name}")))
}

fn hash_vector(term: &Term) -> Result<Vec<String>, EffectsError> {
    let Term::Vector(values) = term else {
        return Err(policy_error("hash list must be a vector"));
    };
    if values.len() > MAX_OBJECTS {
        return Err(policy_error("hash list exceeds object limit"));
    }
    values
        .iter()
        .map(|value| hash_string(value, "artifact hash"))
        .collect()
}

fn hash_string(term: &Term, name: &str) -> Result<String, EffectsError> {
    let value = string(term, name)?;
    if lowercase_hash(&value) {
        Ok(value)
    } else {
        Err(policy_error(format!("{name} must be lowercase hex64")))
    }
}

fn lowercase_hash(value: &str) -> bool {
    value.len() == 64
        && value
            .bytes()
            .all(|byte| byte.is_ascii_hexdigit() && !byte.is_ascii_uppercase())
}

fn string(term: &Term, name: &str) -> Result<String, EffectsError> {
    match term {
        Term::Str(value) => Ok(value.clone()),
        _ => Err(policy_error(format!("{name} must be a string"))),
    }
}

fn require_string(
    fields: &BTreeMap<TermOrdKey, Term>,
    name: &str,
    expected: &str,
    context: &str,
) -> Result<(), EffectsError> {
    if string(field(fields, name, context)?, name)? == expected {
        Ok(())
    } else {
        Err(policy_error(format!("{context} {name} mismatch")))
    }
}

fn require_int(
    fields: &BTreeMap<TermOrdKey, Term>,
    name: &str,
    expected: i64,
    context: &str,
) -> Result<(), EffectsError> {
    match field(fields, name, context)? {
        Term::Int(value) if value == &expected.into() => Ok(()),
        _ => Err(policy_error(format!("{context} {name} mismatch"))),
    }
}

fn require_nil(
    fields: &BTreeMap<TermOrdKey, Term>,
    name: &str,
    context: &str,
) -> Result<(), EffectsError> {
    if matches!(field(fields, name, context)?, Term::Nil) {
        Ok(())
    } else {
        Err(policy_error(format!("{context} {name} must be nil")))
    }
}

fn declared_diagnostic(code: &str) -> bool {
    matches!(
        code,
        "core/refs/bad-authority-request"
            | "core/refs/bad-policy"
            | "core/refs/frozen"
            | "core/refs/no-class"
            | "core/refs/bad-commit"
            | "core/refs/missing-obligation"
            | "core/refs/missing-evidence"
            | "core/refs/bad-evidence"
            | "core/refs/missing-evidence-kind"
            | "core/refs/missing-requirements-trace"
            | "core/refs/invalid-requirements-trace"
            | "core/refs/missing-tool-qualification"
            | "core/refs/invalid-tool-qualification"
            | "core/refs/bad-attestation"
            | "core/refs/insufficient-signatures"
            | "core/refs/missing-attestation-role"
            | "core/refs/missing-attestation-role-signatures"
            | "core/refs/role-independence-violation"
    )
}

fn plain_result(value: Value, context: &EvalCtx) -> Result<Term, EffectsError> {
    if let Value::Sealed { token, payload } = &value
        && context
            .protocol
            .is_some_and(|protocol| *token == protocol.error)
    {
        let detail = payload
            .to_plain_term()
            .map(|term| print_term(&term))
            .unwrap_or_else(|| "<opaque-error-payload>".to_string());
        return Err(policy_error(format!("returned sealed ERROR {detail}")));
    }
    value
        .to_plain_term()
        .ok_or_else(|| policy_error(format!("returned opaque value: {value:?}")))
}

fn hex32(bytes: [u8; 32]) -> String {
    bytes.iter().map(|byte| format!("{byte:02x}")).collect()
}

fn policy_error(message: impl Into<String>) -> EffectsError {
    EffectsError::Log(format!(
        "selfhost refs policy authority: {}",
        message.into()
    ))
}

#[cfg(test)]
#[path = "refs_policy_authority_tests.rs"]
mod tests;
