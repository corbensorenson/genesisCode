use super::*;

#[path = "pkg_publish_authority_crypto.rs"]
mod crypto;
use crypto::{mechanical_signing_hash, verify_crypto_request};
#[cfg(test)]
#[path = "pkg_publish_authority_adapter_tests.rs"]
mod adapter_tests;

pub(super) const PUBLISH_BINDING: &str = "core/pkg::publish-authority";
const REQUEST_KIND: &str = "genesis/pkg-publish-authority-request-v0.1";
const RESULT_KIND: &str = "genesis/pkg-publish-authority-result-v0.1";
const PUBLISH_STEP_LIMIT: u64 = 50_000_000;

#[derive(Debug, Clone)]
pub(crate) struct PkgPublishObject {
    pub(crate) hash: String,
    pub(crate) bytes: Vec<u8>,
    pub(crate) term: Term,
}

impl PkgPublishObject {
    fn envelope(&self) -> Term {
        map([
            (":bytes", Term::Bytes(self.bytes.clone().into())),
            (":h", Term::Str(self.hash.clone())),
            (":term", self.term.clone()),
        ])
    }
}

#[derive(Debug)]
pub(crate) enum PkgPublishInspection {
    Accept {
        attestation_hashes: Vec<String>,
        evidence_hashes: Vec<String>,
        inspect_hash: String,
    },
    Error {
        code: String,
        message: String,
    },
}

#[derive(Debug)]
pub(crate) enum PkgPublishPreparation {
    Accept {
        crypto_facts: Vec<Term>,
        prepare_hash: String,
    },
    Error {
        code: String,
        message: String,
    },
}

#[derive(Debug)]
pub(crate) enum PkgPublishDecision {
    Accept {
        commit: String,
        provenance: Term,
        refname: String,
        sync: Term,
    },
    Error {
        code: String,
        message: String,
    },
}

enum PhaseResult {
    Accept(Term),
    Error { code: String, message: String },
}

impl PkgLockReadAuthority {
    pub(crate) fn inspect_publish(
        &mut self,
        facts: &Term,
    ) -> Result<PkgPublishInspection, EffectsError> {
        let request = publish_request(":inspect", facts.clone(), Term::Nil);
        match self.evaluate_publish(request)? {
            PhaseResult::Error { code, message } => {
                Ok(PkgPublishInspection::Error { code, message })
            }
            PhaseResult::Accept(value) => {
                let fields = publish_exact_map(
                    &value,
                    &[":attestation-hashes", ":evidence-hashes", ":inspect-h"],
                    "inspect value",
                )?;
                let attestation_hashes = hash_vector(
                    publish_field(fields, ":attestation-hashes", "inspect value")?,
                    ":attestation-hashes",
                )?;
                let evidence_hashes = hash_vector(
                    publish_field(fields, ":evidence-hashes", "inspect value")?,
                    ":evidence-hashes",
                )?;
                let inspect_hash = publish_hash_string(
                    publish_field(fields, ":inspect-h", "inspect value")?,
                    ":inspect-h",
                )?;
                require_embedded_hash(&value, ":inspect-h", &inspect_hash, "inspect value")?;
                Ok(PkgPublishInspection::Accept {
                    attestation_hashes,
                    evidence_hashes,
                    inspect_hash,
                })
            }
        }
    }

    pub(crate) fn prepare_publish(
        &mut self,
        facts: &Term,
        inspect_hash: &str,
        evidence: &[PkgPublishObject],
        attestations: &[PkgPublishObject],
    ) -> Result<PkgPublishPreparation, EffectsError> {
        let mechanism = map([
            (
                ":attestations",
                Term::Vector(
                    attestations
                        .iter()
                        .map(PkgPublishObject::envelope)
                        .collect(),
                ),
            ),
            (
                ":evidence",
                Term::Vector(evidence.iter().map(PkgPublishObject::envelope).collect()),
            ),
            (":inspect-h", Term::Str(inspect_hash.to_string())),
        ]);
        let request = publish_request(":prepare", facts.clone(), mechanism);
        match self.evaluate_publish(request)? {
            PhaseResult::Error { code, message } => {
                Ok(PkgPublishPreparation::Error { code, message })
            }
            PhaseResult::Accept(value) => {
                let fields = publish_exact_map(
                    &value,
                    &[":crypto-requests", ":prepare-h"],
                    "prepare value",
                )?;
                let prepare_hash = publish_hash_string(
                    publish_field(fields, ":prepare-h", "prepare value")?,
                    ":prepare-h",
                )?;
                require_embedded_hash(&value, ":prepare-h", &prepare_hash, "prepare value")?;
                let Term::Vector(requests) =
                    publish_field(fields, ":crypto-requests", "prepare value")?
                else {
                    return Err(publish_error("prepare :crypto-requests must be a vector"));
                };
                let commit = facts_field(facts, ":commit")?;
                let signing_hash = mechanical_signing_hash(commit)?;
                let mut crypto_facts = Vec::with_capacity(requests.len());
                for request in requests {
                    let (request_hash, valid) = verify_crypto_request(request, &signing_hash)?;
                    crypto_facts.push(map([
                        (":request-h", Term::Str(request_hash)),
                        (":signature-valid", Term::Bool(valid)),
                    ]));
                }
                Ok(PkgPublishPreparation::Accept {
                    crypto_facts,
                    prepare_hash,
                })
            }
        }
    }

    #[expect(
        clippy::too_many_arguments,
        reason = "the closed finalize protocol keeps every bound phase input explicit"
    )]
    pub(crate) fn finalize_publish(
        &mut self,
        facts: &Term,
        inspect_hash: &str,
        prepare_hash: &str,
        evidence: &[PkgPublishObject],
        attestations: &[PkgPublishObject],
        crypto_facts: Vec<Term>,
    ) -> Result<PkgPublishDecision, EffectsError> {
        let mechanism = map([
            (
                ":attestations",
                Term::Vector(
                    attestations
                        .iter()
                        .map(PkgPublishObject::envelope)
                        .collect(),
                ),
            ),
            (":crypto-facts", Term::Vector(crypto_facts)),
            (
                ":evidence",
                Term::Vector(evidence.iter().map(PkgPublishObject::envelope).collect()),
            ),
            (":inspect-h", Term::Str(inspect_hash.to_string())),
            (":prepare-h", Term::Str(prepare_hash.to_string())),
        ]);
        let request = publish_request(":finalize", facts.clone(), mechanism);
        match self.evaluate_publish(request)? {
            PhaseResult::Error { code, message } => Ok(PkgPublishDecision::Error { code, message }),
            PhaseResult::Accept(value) => decode_finalize_value(value, facts),
        }
    }

    fn evaluate_publish(&mut self, request: Term) -> Result<PhaseResult, EffectsError> {
        let authority = self
            .publish_authority
            .clone()
            .ok_or_else(|| publish_error(format!("missing artifact binding {PUBLISH_BINDING}")))?;
        let request_hash = hash_term(&request);
        self.context.reset_counters();
        self.context.step_limit = Some(PUBLISH_STEP_LIMIT);
        let value = authority
            .apply(&mut self.context, Value::data(request))
            .map_err(|error| publish_error(format!("apply failed: {error}")))?;
        let term = plain_publish_result(value, &self.context)?;
        decode_phase_result(term, request_hash)
    }
}

fn publish_request(phase: &str, facts: Term, mechanism: Term) -> Term {
    map([
        (":facts", facts),
        (":kind", Term::Str(REQUEST_KIND.to_string())),
        (":mechanism", mechanism),
        (":phase", Term::symbol(phase)),
        (":v", Term::Int(1.into())),
    ])
}

fn decode_phase_result(term: Term, request_hash: [u8; 32]) -> Result<PhaseResult, EffectsError> {
    let fields = publish_exact_map(
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
    require_publish_string(fields, ":kind", RESULT_KIND, "authority result")?;
    require_publish_int(fields, ":v", 1, "authority result")?;
    require_publish_string(
        fields,
        ":request-h",
        &hex32(request_hash),
        "authority result",
    )?;
    match publish_field(fields, ":ok", "authority result")? {
        Term::Bool(true) => {
            require_publish_nil(fields, ":code", "authority result")?;
            require_publish_nil(fields, ":message", "authority result")?;
            let value = publish_field(fields, ":value", "authority result")?.clone();
            if matches!(value, Term::Nil) {
                return Err(publish_error("accepted authority result has nil :value"));
            }
            Ok(PhaseResult::Accept(value))
        }
        Term::Bool(false) => {
            require_publish_nil(fields, ":value", "authority result")?;
            let code =
                publish_string(publish_field(fields, ":code", "authority result")?, ":code")?;
            if !publish_diagnostic(&code) {
                return Err(publish_error(format!(
                    "authority rejection used undeclared diagnostic {code}"
                )));
            }
            let message = publish_string(
                publish_field(fields, ":message", "authority result")?,
                ":message",
            )?;
            Ok(PhaseResult::Error { code, message })
        }
        _ => Err(publish_error("authority result :ok must be boolean")),
    }
}

fn decode_finalize_value(value: Term, facts: &Term) -> Result<PkgPublishDecision, EffectsError> {
    let fields = publish_exact_map(
        &value,
        &[":commit", ":provenance", ":ref", ":sync"],
        "finalize value",
    )?;
    let commit = publish_hash_string(
        publish_field(fields, ":commit", "finalize value")?,
        ":commit",
    )?;
    let refname = publish_string(publish_field(fields, ":ref", "finalize value")?, ":ref")?;
    require_fact_string(facts, ":commit-h", &commit)?;
    require_fact_string(facts, ":ref", &refname)?;
    let provenance = publish_field(fields, ":provenance", "finalize value")?.clone();
    let expected_provenance = expected_provenance(facts_field(facts, ":commit")?)?;
    if provenance != expected_provenance {
        return Err(publish_error(
            "finalize provenance contradicts bound commit",
        ));
    }
    let sync = publish_field(fields, ":sync", "finalize value")?.clone();
    let expected_sync = expected_sync(facts)?;
    if sync != expected_sync {
        return Err(publish_error("finalize sync plan contradicts bound facts"));
    }
    Ok(PkgPublishDecision::Accept {
        commit,
        provenance,
        refname,
        sync,
    })
}

fn expected_provenance(commit: &Term) -> Result<Term, EffectsError> {
    let fields = publish_exact_map(
        commit,
        &[
            ":attestations",
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
        ],
        "bound commit",
    )?;
    Ok(map([
        (
            ":attestations",
            publish_field(fields, ":attestations", "bound commit")?.clone(),
        ),
        (
            ":base",
            publish_field(fields, ":base", "bound commit")?.clone(),
        ),
        (
            ":evidence",
            publish_field(fields, ":evidence", "bound commit")?.clone(),
        ),
        (
            ":obligations",
            publish_field(fields, ":obligations", "bound commit")?.clone(),
        ),
        (
            ":parents",
            publish_field(fields, ":parents", "bound commit")?.clone(),
        ),
        (
            ":patch",
            publish_field(fields, ":patch", "bound commit")?.clone(),
        ),
        (
            ":result",
            publish_field(fields, ":result", "bound commit")?.clone(),
        ),
    ]))
}

fn expected_sync(facts: &Term) -> Result<Term, EffectsError> {
    let commit = facts_field(facts, ":commit-h")?.clone();
    let policy = facts_field(facts, ":policy-h")?.clone();
    let refname = facts_field(facts, ":ref")?.clone();
    let remote = facts_field(facts, ":remote")?.clone();
    let expected_old = facts_field(facts, ":expected-old")?.clone();
    let depth = facts_field(facts, ":depth")?.clone();
    let mut set_ref = BTreeMap::from([
        (TermOrdKey(Term::symbol(":hash")), commit.clone()),
        (TermOrdKey(Term::symbol(":name")), refname),
        (TermOrdKey(Term::symbol(":policy")), policy.clone()),
    ]);
    if !matches!(expected_old, Term::Nil) {
        set_ref.insert(TermOrdKey(Term::symbol(":expected-old")), expected_old);
    }
    let mut sync = BTreeMap::from([
        (TermOrdKey(Term::symbol(":remote")), remote),
        (
            TermOrdKey(Term::symbol(":roots")),
            Term::Vector(vec![commit, policy]),
        ),
        (
            TermOrdKey(Term::symbol(":set-refs")),
            Term::Vector(vec![Term::Map(set_ref)]),
        ),
    ]);
    match depth {
        Term::Int(value) if value > 0.into() => {
            sync.insert(TermOrdKey(Term::symbol(":depth")), Term::Int(value));
        }
        Term::Int(_) => {}
        _ => return Err(publish_error("bound publish depth must be an integer")),
    }
    Ok(Term::Map(sync))
}

fn require_embedded_hash(
    term: &Term,
    field_name: &str,
    observed: &str,
    context: &str,
) -> Result<(), EffectsError> {
    let Term::Map(fields) = term else {
        return Err(publish_error(format!("{context} must be a map")));
    };
    let mut unhashed = fields.clone();
    unhashed.remove(&TermOrdKey(Term::symbol(field_name)));
    let expected = hex32(hash_term(&Term::Map(unhashed)));
    if observed == expected {
        Ok(())
    } else {
        Err(publish_error(format!("{context} {field_name} mismatch")))
    }
}

fn facts_field<'a>(facts: &'a Term, name: &str) -> Result<&'a Term, EffectsError> {
    let fields = publish_exact_map(
        facts,
        &[
            ":commit",
            ":commit-h",
            ":depth",
            ":expected-old",
            ":policy",
            ":policy-h",
            ":ref",
            ":remote",
        ],
        "publish facts",
    )?;
    publish_field(fields, name, "publish facts")
}

fn require_fact_string(facts: &Term, name: &str, expected: &str) -> Result<(), EffectsError> {
    match facts_field(facts, name)? {
        Term::Str(value) if value == expected => Ok(()),
        _ => Err(publish_error(format!(
            "finalize value contradicts bound fact {name}"
        ))),
    }
}

fn hash_vector(term: &Term, name: &str) -> Result<Vec<String>, EffectsError> {
    let Term::Vector(values) = term else {
        return Err(publish_error(format!("{name} must be a vector")));
    };
    values
        .iter()
        .map(|value| publish_hash_string(value, name))
        .collect()
}

fn string_vector(term: &Term, name: &str) -> Result<Vec<String>, EffectsError> {
    let Term::Vector(values) = term else {
        return Err(publish_error(format!("{name} must be a vector")));
    };
    values
        .iter()
        .map(|value| publish_string(value, name))
        .collect()
}

fn publish_hash_string(term: &Term, name: &str) -> Result<String, EffectsError> {
    let value = publish_string(term, name)?;
    if gc_vcs::validate_hex_hash(&value).is_ok()
        && value
            .bytes()
            .all(|byte| !byte.is_ascii_uppercase() && byte.is_ascii_hexdigit())
    {
        Ok(value)
    } else {
        Err(publish_error(format!("{name} must be lowercase hex64")))
    }
}

fn publish_string(term: &Term, name: &str) -> Result<String, EffectsError> {
    match term {
        Term::Str(value) => Ok(value.clone()),
        _ => Err(publish_error(format!("{name} must be a string"))),
    }
}

fn publish_bytes(term: &Term, name: &str) -> Result<Vec<u8>, EffectsError> {
    match term {
        Term::Bytes(value) => Ok(value.to_vec()),
        _ => Err(publish_error(format!("{name} must be bytes"))),
    }
}

fn publish_exact_map<'a>(
    term: &'a Term,
    expected: &[&str],
    context: &str,
) -> Result<&'a BTreeMap<TermOrdKey, Term>, EffectsError> {
    let Term::Map(fields) = term else {
        return Err(publish_error(format!(
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
        Err(publish_error(format!(
            "{context} field set mismatch: actual={actual:?} expected={wanted:?}"
        )))
    }
}

fn publish_field<'a>(
    fields: &'a BTreeMap<TermOrdKey, Term>,
    name: &str,
    context: &str,
) -> Result<&'a Term, EffectsError> {
    fields
        .get(&TermOrdKey(Term::symbol(name)))
        .ok_or_else(|| publish_error(format!("{context} missing {name}")))
}

fn require_publish_string(
    fields: &BTreeMap<TermOrdKey, Term>,
    name: &str,
    expected: &str,
    context: &str,
) -> Result<(), EffectsError> {
    if publish_string(publish_field(fields, name, context)?, name)? == expected {
        Ok(())
    } else {
        Err(publish_error(format!("{context} {name} mismatch")))
    }
}

fn require_publish_int(
    fields: &BTreeMap<TermOrdKey, Term>,
    name: &str,
    expected: i64,
    context: &str,
) -> Result<(), EffectsError> {
    match publish_field(fields, name, context)? {
        Term::Int(value) if value == &expected.into() => Ok(()),
        _ => Err(publish_error(format!("{context} {name} mismatch"))),
    }
}

fn require_publish_nil(
    fields: &BTreeMap<TermOrdKey, Term>,
    name: &str,
    context: &str,
) -> Result<(), EffectsError> {
    if matches!(publish_field(fields, name, context)?, Term::Nil) {
        Ok(())
    } else {
        Err(publish_error(format!("{context} {name} must be nil")))
    }
}

fn publish_diagnostic(code: &str) -> bool {
    matches!(
        code,
        "core/pkg/bad-authority-request"
            | "core/pkg/bad-payload"
            | "core/pkg/bad-policy"
            | "core/pkg/ref-frozen"
            | "core/pkg/no-policy-class"
            | "core/pkg/bad-commit"
            | "core/pkg/missing-obligation"
            | "core/pkg/missing-evidence"
            | "core/pkg/bad-evidence"
            | "core/pkg/missing-evidence-kind"
            | "core/pkg/missing-requirements-trace"
            | "core/pkg/invalid-requirements-trace"
            | "core/pkg/missing-tool-qualification"
            | "core/pkg/invalid-tool-qualification"
            | "core/pkg/bad-attestation"
            | "core/pkg/missing-signatures"
            | "core/pkg/missing-attestation-role"
            | "core/pkg/missing-attestation-role-signatures"
            | "core/pkg/role-independence-violation"
    )
}

fn plain_publish_result(value: Value, context: &EvalCtx) -> Result<Term, EffectsError> {
    if let Value::Sealed { token, payload } = &value
        && context
            .protocol
            .is_some_and(|protocol| *token == protocol.error)
    {
        let detail = payload
            .to_plain_term()
            .map(|term| print_term(&term))
            .unwrap_or_else(|| "<opaque-error-payload>".to_string());
        return Err(publish_error(format!("returned sealed ERROR {detail}")));
    }
    value
        .to_plain_term()
        .ok_or_else(|| publish_error(format!("returned opaque value: {value:?}")))
}

fn publish_error(message: impl Into<String>) -> EffectsError {
    EffectsError::Log(format!(
        "selfhost package publish authority: {}",
        message.into()
    ))
}
