use super::*;

pub(super) const BRIDGE_BINDING: &str = "core/pkg::bridge-authority";
const BRIDGE_REQUEST_KIND: &str = "genesis/pkg-bridge-authority-request-v0.1";
const BRIDGE_RESULT_KIND: &str = "genesis/pkg-bridge-authority-result-v0.1";

#[derive(Clone, Copy)]
pub(crate) struct PkgBridgeFacts<'a> {
    pub(crate) ecosystem: &'a str,
    pub(crate) name: &'a str,
    pub(crate) source: &'a str,
    pub(crate) source_hash: &'a str,
    pub(crate) version: &'a str,
}

#[derive(Debug, Clone)]
pub(crate) struct PkgBridgeObject {
    pub(crate) bytes: Vec<u8>,
    pub(crate) hash: String,
    pub(crate) term: Term,
}

#[derive(Debug)]
pub(crate) struct PkgBridgePlan {
    pub(crate) conversion_data: PkgBridgeObject,
    pub(crate) conversion_evidence: PkgBridgeObject,
    pub(crate) patch: PkgBridgeObject,
    pub(crate) plan_hash: String,
    pub(crate) provenance: PkgBridgeObject,
    pub(crate) sign_message: Vec<u8>,
    pub(crate) signing_hash: [u8; 32],
    pub(crate) snapshot: PkgBridgeObject,
}

#[derive(Debug)]
pub(crate) struct PkgBridgeFinal {
    pub(crate) attestation: PkgBridgeObject,
    pub(crate) commit: PkgBridgeObject,
}

#[derive(Debug)]
pub(crate) enum PkgBridgeDecision<T> {
    Accept(T),
    Error { code: String, message: String },
}

impl PkgLockReadAuthority {
    pub(crate) fn plan_bridge(
        &mut self,
        facts: PkgBridgeFacts<'_>,
    ) -> Result<PkgBridgeDecision<PkgBridgePlan>, EffectsError> {
        let (term, request_hash) = self.apply_bridge(":plan", facts, Term::Nil)?;
        let value = match decode_bridge_envelope(term, request_hash)? {
            PkgBridgeDecision::Accept(value) => value,
            PkgBridgeDecision::Error { code, message } => {
                return Ok(PkgBridgeDecision::Error { code, message });
            }
        };
        let fields = exact_map(&value, &[":plan", ":plan-h"])?;
        let plan = field(fields, ":plan")?;
        let plan_hash = required_string(fields, ":plan-h")?.to_string();
        if !is_lower_hash(&plan_hash) || hex32(hash_term(plan)) != plan_hash {
            return Err(authority_error(
                "bridge plan value and :plan-h are malformed or contradictory",
            ));
        }
        let plan_fields = exact_map(
            plan,
            &[
                ":conversion-data",
                ":conversion-evidence",
                ":patch",
                ":provenance",
                ":sign-message",
                ":signing-h",
                ":snapshot",
                ":unsigned-commit",
            ],
        )?;
        let unsigned_commit = field(plan_fields, ":unsigned-commit")?.clone();
        let signing_hash = bytes32(field(plan_fields, ":signing-h")?, ":signing-h")?;
        let expected_signing_hash = gc_vcs::commit_signing_hash(&unsigned_commit)
            .map_err(|error| authority_error(format!("bridge unsigned commit invalid: {error}")))?;
        if signing_hash != expected_signing_hash {
            return Err(authority_error(
                "bridge plan :signing-h contradicts the unsigned commit",
            ));
        }
        let sign_message =
            required_bytes_term(field(plan_fields, ":sign-message")?, ":sign-message")?;
        if sign_message != gc_vcs::commit_attestation_message(&signing_hash) {
            return Err(authority_error(
                "bridge plan :sign-message contradicts the VCS attestation domain",
            ));
        }
        Ok(PkgBridgeDecision::Accept(PkgBridgePlan {
            conversion_data: decode_object(field(plan_fields, ":conversion-data")?)?,
            conversion_evidence: decode_object(field(plan_fields, ":conversion-evidence")?)?,
            patch: decode_object(field(plan_fields, ":patch")?)?,
            plan_hash,
            provenance: decode_object(field(plan_fields, ":provenance")?)?,
            sign_message,
            signing_hash,
            snapshot: decode_object(field(plan_fields, ":snapshot")?)?,
        }))
    }

    pub(crate) fn finalize_bridge(
        &mut self,
        facts: PkgBridgeFacts<'_>,
        plan: &PkgBridgePlan,
        public_key: [u8; 32],
        signature: [u8; 64],
        signature_valid: bool,
    ) -> Result<PkgBridgeDecision<PkgBridgeFinal>, EffectsError> {
        let mechanism = map([
            (":plan-h", Term::Str(plan.plan_hash.clone())),
            (":public-key", Term::Bytes(public_key.to_vec().into())),
            (":signature", Term::Bytes(signature.to_vec().into())),
            (":signature-valid", Term::Bool(signature_valid)),
        ]);
        let (term, request_hash) = self.apply_bridge(":finalize", facts, mechanism)?;
        let value = match decode_bridge_envelope(term, request_hash)? {
            PkgBridgeDecision::Accept(value) => value,
            PkgBridgeDecision::Error { code, message } => {
                return Ok(PkgBridgeDecision::Error { code, message });
            }
        };
        let fields = exact_map(&value, &[":attestation", ":commit", ":plan-h"])?;
        require_string(fields, ":plan-h", &plan.plan_hash)?;
        let attestation = decode_object(field(fields, ":attestation")?)?;
        let commit = decode_object(field(fields, ":commit")?)?;

        let parsed_attestation = gc_vcs::Attestation::from_term(&attestation.term)
            .map_err(|error| authority_error(format!("bridge attestation invalid: {error}")))?;
        let verifying_key = ed25519_dalek::VerifyingKey::from_bytes(&public_key)
            .map_err(|error| authority_error(format!("bridge public key invalid: {error}")))?;
        gc_vcs::verify_commit_attestation(
            &parsed_attestation,
            &plan.signing_hash,
            std::slice::from_ref(&verifying_key),
        )
        .map_err(|error| authority_error(format!("bridge attestation contradiction: {error}")))?;
        let parsed_commit = gc_vcs::Commit::from_term(&commit.term)
            .map_err(|error| authority_error(format!("bridge commit invalid: {error}")))?;
        if parsed_commit.attestations != vec![attestation.hash.clone()]
            || gc_vcs::commit_signing_hash(&commit.term)
                .map_err(|error| authority_error(format!("bridge commit invalid: {error}")))?
                != plan.signing_hash
        {
            return Err(authority_error(
                "bridge final commit contradicts its plan or attestation",
            ));
        }
        Ok(PkgBridgeDecision::Accept(PkgBridgeFinal {
            attestation,
            commit,
        }))
    }

    fn apply_bridge(
        &mut self,
        operation: &'static str,
        facts: PkgBridgeFacts<'_>,
        mechanism: Term,
    ) -> Result<(Term, [u8; 32]), EffectsError> {
        let request = map([
            (
                ":facts",
                map([
                    (":ecosystem", Term::Str(facts.ecosystem.to_string())),
                    (":name", Term::Str(facts.name.to_string())),
                    (":source", Term::Str(facts.source.to_string())),
                    (":source-hash", Term::Str(facts.source_hash.to_string())),
                    (":version", Term::Str(facts.version.to_string())),
                ]),
            ),
            (":kind", Term::Str(BRIDGE_REQUEST_KIND.to_string())),
            (":mechanism", mechanism),
            (":op", Term::symbol(operation)),
            (":v", Term::Int(1.into())),
        ]);
        let request_hash = hash_term(&request);
        self.context.reset_counters();
        self.context.step_limit = Some(STEP_LIMIT);
        let authority = self
            .bridge_authority
            .clone()
            .ok_or_else(|| authority_error(format!("missing binding {BRIDGE_BINDING}")))?;
        let value = authority
            .apply(&mut self.context, Value::data(request))
            .map_err(|error| authority_error(format!("bridge apply failed: {error}")))?;
        Ok((plain_result(value, &self.context)?, request_hash))
    }
}

fn decode_bridge_envelope(
    term: Term,
    request_hash: [u8; 32],
) -> Result<PkgBridgeDecision<Term>, EffectsError> {
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
    )?;
    require_string(fields, ":kind", BRIDGE_RESULT_KIND)?;
    require_int(fields, ":v", 1)?;
    require_string(fields, ":request-h", &hex32(request_hash))?;
    if required_bool(fields, ":ok")? {
        require_nil(fields, ":code")?;
        require_nil(fields, ":message")?;
        Ok(PkgBridgeDecision::Accept(field(fields, ":value")?.clone()))
    } else {
        require_nil(fields, ":value")?;
        let code = required_string(fields, ":code")?;
        if !matches!(
            code,
            "core/pkg/bad-authority-request" | "core/pkg/bad-payload" | "core/pkg/bridge-signature"
        ) {
            return Err(authority_error(
                "bridge result :code is outside the closed rejection inventory",
            ));
        }
        Ok(PkgBridgeDecision::Error {
            code: code.to_string(),
            message: required_string(fields, ":message")?.to_string(),
        })
    }
}

fn decode_object(term: &Term) -> Result<PkgBridgeObject, EffectsError> {
    let fields = exact_map(term, &[":bytes", ":h", ":term"])?;
    let bytes = required_bytes_term(field(fields, ":bytes")?, ":bytes")?;
    let hash = required_string(fields, ":h")?.to_string();
    let artifact = field(fields, ":term")?.clone();
    if print_term(&artifact).as_bytes() != bytes.as_slice()
        || !is_lower_hash(&hash)
        || blake3::hash(&bytes).to_hex().as_str() != hash
    {
        return Err(authority_error(
            "bridge object :term, :bytes, and :h are malformed or contradictory",
        ));
    }
    Ok(PkgBridgeObject {
        bytes,
        hash,
        term: artifact,
    })
}

fn required_bytes_term(term: &Term, name: &str) -> Result<Vec<u8>, EffectsError> {
    match term {
        Term::Bytes(value) => Ok(value.to_vec()),
        _ => Err(authority_error(format!(
            "bridge result {name} must be bytes"
        ))),
    }
}

fn bytes32(term: &Term, name: &str) -> Result<[u8; 32], EffectsError> {
    let bytes = required_bytes_term(term, name)?;
    bytes
        .try_into()
        .map_err(|_| authority_error(format!("bridge result {name} must contain 32 bytes")))
}

fn is_lower_hash(value: &str) -> bool {
    value.len() == 64
        && value
            .as_bytes()
            .iter()
            .all(|byte| byte.is_ascii_hexdigit() && !byte.is_ascii_uppercase())
}

#[cfg(test)]
mod tests {
    use ed25519_dalek::{Signer, SigningKey};
    use gc_coreform::parse_term;

    use super::*;

    fn artifact_config() -> SelfhostAuthorityConfig {
        let artifact = std::env::var_os("GENESIS_SELFHOST_TOOLCHAIN_ARTIFACT")
            .map(std::path::PathBuf::from)
            .unwrap_or_else(|| {
                std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../selfhost/toolchain.gc")
            })
            .canonicalize()
            .expect("canonical selfhost artifact path");
        SelfhostAuthorityConfig {
            bootstrap_mode: gc_prelude::SelfhostBootstrapMode::ArtifactOnly,
            artifact: Some(artifact),
        }
    }

    fn facts<'a>() -> PkgBridgeFacts<'a> {
        PkgBridgeFacts {
            ecosystem: "crates",
            name: "serde",
            source: "serde@1.0.217",
            source_hash: "0123456789abcdef0123456789abcdef0123456789abcdef0123456789abcdef",
            version: "1.0.217",
        }
    }

    #[test]
    fn authority_owns_exact_bridge_objects_and_valid_attestation() {
        let mut authority = PkgLockReadAuthority::load(&artifact_config()).unwrap();
        let PkgBridgeDecision::Accept(plan) = authority.plan_bridge(facts()).unwrap() else {
            panic!("bridge plan should be accepted");
        };
        let expected_provenance = parse_term(
            r#"{:type :gcpm/external-provenance :v 1 :ecosystem "crates" :name "serde" :version "1.0.217" :source "serde@1.0.217" :source-hash "0123456789abcdef0123456789abcdef0123456789abcdef0123456789abcdef"}"#,
        )
        .unwrap();
        assert_eq!(plan.provenance.term, expected_provenance);
        assert_eq!(
            plan.provenance.hash,
            blake3::hash(print_term(&expected_provenance).as_bytes())
                .to_hex()
                .as_str()
        );
        assert_eq!(
            plan.sign_message,
            gc_vcs::commit_attestation_message(&plan.signing_hash)
        );

        let signing_key = SigningKey::from_bytes(&[0x42; 32]);
        let signature = signing_key.sign(&plan.sign_message).to_bytes();
        let public_key = signing_key.verifying_key().to_bytes();
        let PkgBridgeDecision::Accept(finalized) = authority
            .finalize_bridge(facts(), &plan, public_key, signature, true)
            .unwrap()
        else {
            panic!("bridge finalize should be accepted");
        };
        let attestation = gc_vcs::Attestation::from_term(&finalized.attestation.term).unwrap();
        gc_vcs::verify_commit_attestation(
            &attestation,
            &plan.signing_hash,
            &[signing_key.verifying_key()],
        )
        .unwrap();
        let commit = gc_vcs::Commit::from_term(&finalized.commit.term).unwrap();
        assert_eq!(commit.attestations, vec![finalized.attestation.hash]);
    }

    #[test]
    fn authority_rejects_false_crypto_fact_and_result_substitution() {
        let mut authority = PkgLockReadAuthority::load(&artifact_config()).unwrap();
        let PkgBridgeDecision::Accept(plan) = authority.plan_bridge(facts()).unwrap() else {
            panic!("bridge plan should be accepted");
        };
        let signing_key = SigningKey::from_bytes(&[0x42; 32]);
        let signature = signing_key.sign(&plan.sign_message).to_bytes();
        assert!(matches!(
            authority
                .finalize_bridge(
                    facts(),
                    &plan,
                    signing_key.verifying_key().to_bytes(),
                    signature,
                    false,
                )
                .unwrap(),
            PkgBridgeDecision::Error { ref code, .. } if code == "core/pkg/bridge-signature"
        ));

        let request_hash = [7; 32];
        let valid = map([
            (":code", Term::Nil),
            (":kind", Term::Str(BRIDGE_RESULT_KIND.to_string())),
            (":message", Term::Nil),
            (":ok", Term::Bool(true)),
            (":request-h", Term::Str(hex32(request_hash))),
            (":v", Term::Int(1.into())),
            (":value", map([("accepted", Term::Bool(true))])),
        ]);
        let mut open = match valid.clone() {
            Term::Map(fields) => fields,
            _ => BTreeMap::new(),
        };
        open.insert(TermOrdKey(Term::symbol(":extra")), Term::Nil);
        assert!(decode_bridge_envelope(Term::Map(open), request_hash).is_err());
        let mut unbound = match valid {
            Term::Map(fields) => fields,
            _ => BTreeMap::new(),
        };
        unbound.insert(
            TermOrdKey(Term::symbol(":request-h")),
            Term::Str("0".repeat(64)),
        );
        assert!(decode_bridge_envelope(Term::Map(unbound), request_hash).is_err());
    }

    #[test]
    fn object_decoder_rejects_bytes_and_hash_substitution() {
        let term = parse_term(r#"{:type :vcs/patch :v 1 :ops []}"#).unwrap();
        let bytes = print_term(&term).into_bytes();
        let hash = blake3::hash(&bytes).to_hex().to_string();
        let valid = map([
            (":bytes", Term::Bytes(bytes.clone().into())),
            (":h", Term::Str(hash)),
            (":term", term),
        ]);
        assert!(decode_object(&valid).is_ok());
        let mut substituted = match valid {
            Term::Map(fields) => fields,
            _ => BTreeMap::new(),
        };
        substituted.insert(
            TermOrdKey(Term::symbol(":bytes")),
            Term::Bytes(b"{}".to_vec().into()),
        );
        assert!(decode_object(&Term::Map(substituted)).is_err());
    }
}
