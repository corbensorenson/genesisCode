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
    pub observed_hash: Option<[u8; 32]>,
    pub load_error: Option<String>,
    pub term: Term,
}

#[derive(Debug, Clone)]
pub struct StoreHashObservation {
    pub role: &'static str,
    pub required_hash: String,
    pub observed_hash: Option<String>,
    pub load_error: Option<String>,
}

#[derive(Debug, Clone)]
pub struct PolicyKeyObservation {
    pub encoded: String,
    pub decoded: Option<[u8; 32]>,
    pub decode_error: Option<String>,
    pub key_valid: bool,
}

#[derive(Debug, Clone)]
pub struct RegistryPolicyObservation {
    pub version: u64,
    pub min_signatures: u64,
    pub allowed_keys: Vec<PolicyKeyObservation>,
}

#[derive(Debug, Clone)]
pub struct SignatureObservation {
    pub artifact_hash: String,
    pub crypto_valid: bool,
    pub term: Term,
}

#[derive(Debug, Clone)]
pub struct PackageVerificationRequest {
    pub facts: Vec<EvidenceFact>,
    pub acceptance_hash: Option<[u8; 32]>,
    pub acceptance: Term,
    pub store: Vec<StoreHashObservation>,
    pub policy: Option<RegistryPolicyObservation>,
    pub signature_set: Term,
    pub signatures: Vec<SignatureObservation>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct EvidenceDecision {
    pub verified: bool,
    pub errors: Vec<String>,
    pub checked: usize,
    pub valid_signatures: usize,
}

#[derive(Debug, Clone)]
pub struct DsseVerificationFacts<'a> {
    pub envelope_fields: &'a [String],
    pub expected_key_id: &'a str,
    pub expected_payload_type: &'a str,
    pub key_id: &'a str,
    pub key_valid: bool,
    pub payload_hash: [u8; 32],
    pub payload_type: &'a str,
    pub signature_count: usize,
    pub signature_fields: &'a [String],
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
        package: PackageVerificationRequest,
    ) -> Result<EvidenceDecision, EvidenceVerifyError> {
        let data = self.decide(request(
            ":package",
            [
                (":acceptance", package.acceptance),
                (
                    ":acceptance-h",
                    package.acceptance_hash.map(bytes32).unwrap_or(Term::Nil),
                ),
                (
                    ":facts",
                    Term::Vector(package.facts.into_iter().map(fact_term).collect()),
                ),
                (
                    ":policy",
                    package.policy.map(policy_term).unwrap_or(Term::Nil),
                ),
                (":signature-set", package.signature_set),
                (
                    ":signatures",
                    Term::Vector(
                        package
                            .signatures
                            .into_iter()
                            .map(signature_observation_term)
                            .collect(),
                    ),
                ),
                (
                    ":store",
                    Term::Vector(
                        package
                            .store
                            .into_iter()
                            .map(store_observation_term)
                            .collect(),
                    ),
                ),
            ],
        ))?;
        decode_package_decision(data)
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
            valid_signatures: 0,
        })
    }

    pub fn dsse(
        &mut self,
        facts: DsseVerificationFacts<'_>,
    ) -> Result<EvidenceDecision, EvidenceVerifyError> {
        let data = self.decide(request(
            ":dsse",
            [
                (
                    ":envelope-fields",
                    string_vector(facts.envelope_fields.iter().cloned()),
                ),
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
                    ":signature-fields",
                    string_vector(facts.signature_fields.iter().cloned()),
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

fn string_vector(values: impl IntoIterator<Item = String>) -> Term {
    Term::Vector(values.into_iter().map(Term::Str).collect())
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
        (
            ":observed-h",
            entry.observed_hash.map(bytes32).unwrap_or(Term::Nil),
        ),
        (":term", entry.term),
    ])
}

fn store_observation_term(observation: StoreHashObservation) -> Term {
    map([
        (
            ":load-error",
            observation.load_error.map(Term::Str).unwrap_or(Term::Nil),
        ),
        (
            ":observed-h",
            observation
                .observed_hash
                .map(Term::Str)
                .unwrap_or(Term::Nil),
        ),
        (":required-h", Term::Str(observation.required_hash)),
        (":role", Term::symbol(observation.role)),
    ])
}

fn policy_term(policy: RegistryPolicyObservation) -> Term {
    map([
        (
            ":allowed-keys",
            Term::Vector(
                policy
                    .allowed_keys
                    .into_iter()
                    .map(policy_key_term)
                    .collect(),
            ),
        ),
        (":min-signatures", Term::Int(policy.min_signatures.into())),
        (":version", Term::Int(policy.version.into())),
    ])
}

fn policy_key_term(observation: PolicyKeyObservation) -> Term {
    map([
        (
            ":decode-error",
            observation.decode_error.map(Term::Str).unwrap_or(Term::Nil),
        ),
        (
            ":decoded",
            observation.decoded.map(bytes32).unwrap_or(Term::Nil),
        ),
        (":encoded", Term::Str(observation.encoded)),
        (":key-valid", Term::Bool(observation.key_valid)),
    ])
}

fn signature_observation_term(observation: SignatureObservation) -> Term {
    map([
        (":artifact-h", Term::Str(observation.artifact_hash)),
        (":crypto-valid", Term::Bool(observation.crypto_valid)),
        (":term", observation.term),
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
        valid_signatures: 0,
    })
}

fn decode_package_decision(data: Term) -> Result<EvidenceDecision, EvidenceVerifyError> {
    let fields = exact_map(
        &data,
        "package evidence decision",
        &[":checked", ":errors", ":valid-signatures", ":verified"],
    )?;
    let checked = required_usize(fields, ":checked", "package evidence decision")?;
    let valid_signatures =
        required_usize(fields, ":valid-signatures", "package evidence decision")?;
    let errors = required_strings(fields, ":errors", "package evidence decision")?;
    let verified = required_bool(fields, ":verified", "package evidence decision")?;
    if verified != errors.is_empty() {
        return Err(authority_error("authority verdict/error inconsistency"));
    }
    Ok(EvidenceDecision {
        verified,
        errors,
        checked,
        valid_signatures,
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
