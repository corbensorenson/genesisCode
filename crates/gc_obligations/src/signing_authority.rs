use std::collections::BTreeSet;
use std::path::Path;

use gc_coreform::{Term, TermOrdKey, hash_term, print_term};
use gc_kernel::{Apply, EvalCtx, MemLimits, Value};
use gc_prelude::{
    SelfhostBootstrapMode, build_prelude, load_selfhost_coreform_toolchain_v1_with_mode,
};

use crate::SigningError;

const BINDING: &str = "core/security::signing-authority";
const REQUEST_KIND: &str = "genesis/signing-authority-request-v0.1";
const RESULT_KIND: &str = "genesis/signing-authority-result-v0.1";
const STEP_LIMIT: u64 = 20_000_000;
const ALLOC_LIMIT: u64 = 64_000_000;

fn authority_error(message: impl Into<String>) -> SigningError {
    SigningError::Authority(message.into())
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

fn hash_bytes(value: [u8; 32]) -> Term {
    Term::Bytes(value.to_vec().into())
}

fn hash_hex(value: [u8; 32]) -> String {
    const HEX: &[u8; 16] = b"0123456789abcdef";
    let mut out = String::with_capacity(64);
    for byte in value {
        out.push(HEX[(byte >> 4) as usize] as char);
        out.push(HEX[(byte & 0x0f) as usize] as char);
    }
    out
}

fn base_request(
    phase: &'static str,
    entries: impl IntoIterator<Item = (&'static str, Term)>,
) -> Term {
    let mut fields = vec![
        (":kind", Term::Str(REQUEST_KIND.to_string())),
        (":phase", Term::symbol(phase)),
        (":v", Term::Int(1.into())),
    ];
    fields.extend(entries);
    map(fields)
}

pub struct SigningAuthority {
    context: EvalCtx,
    authority: Value,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SigningCommit {
    pub signature_set: Vec<String>,
    pub transparency_entry: Term,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DsseSigningArtifact {
    pub key_id: String,
    pub kind: String,
    pub payload: Vec<u8>,
    pub payload_sha256: String,
    pub payload_type: String,
    pub public_key: [u8; 32],
    pub signature: [u8; 64],
    pub version: String,
}

impl SigningAuthority {
    pub fn load(
        bootstrap_mode: SelfhostBootstrapMode,
        artifact: Option<&Path>,
    ) -> Result<Self, SigningError> {
        if bootstrap_mode != SelfhostBootstrapMode::ArtifactOnly {
            return Err(authority_error(
                "production signing requires artifact-only bootstrap",
            ));
        }
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
            bootstrap_mode,
            artifact,
        )
        .map_err(|error| authority_error(format!("artifact bootstrap failed: {error}")))?;
        let authority = environment
            .get(BINDING)
            .ok_or_else(|| authority_error(format!("missing binding {BINDING}")))?;
        context.reset_counters();
        context.step_limit = Some(STEP_LIMIT);
        Ok(Self { context, authority })
    }

    pub fn keygen(
        &mut self,
        public_key: [u8; 32],
        keypair_valid: bool,
    ) -> Result<(), SigningError> {
        let data = self.decide(base_request(
            ":keygen",
            [
                (":keypair-valid", Term::Bool(keypair_valid)),
                (":public-key", hash_bytes(public_key)),
            ],
        ))?;
        let fields = exact_map(&data, "keygen data", &[":alg", ":public-key"])?;
        require_string(fields, ":alg", "keygen data", "ed25519")?;
        require_bytes32(fields, ":public-key", "keygen data", public_key)?;
        Ok(())
    }

    pub fn acceptance_message(
        &mut self,
        acceptance_hash: [u8; 32],
    ) -> Result<Vec<u8>, SigningError> {
        let data = self.decide(base_request(
            ":acceptance-plan",
            [(":acceptance-h", hash_bytes(acceptance_hash))],
        ))?;
        let fields = exact_map(&data, "acceptance plan", &[":message"])?;
        required_bytes(fields, ":message", "acceptance plan")
    }

    pub fn acceptance_artifact(
        &mut self,
        acceptance_hash: [u8; 32],
        public_key: [u8; 32],
        signature: [u8; 64],
        signature_valid: bool,
    ) -> Result<Term, SigningError> {
        self.decide(base_request(
            ":acceptance-finalize",
            [
                (":acceptance-h", hash_bytes(acceptance_hash)),
                (":public-key", hash_bytes(public_key)),
                (":signature", Term::Bytes(signature.to_vec().into())),
                (":signature-valid", Term::Bool(signature_valid)),
            ],
        ))
    }

    #[allow(clippy::too_many_arguments)]
    pub fn commit(
        &mut self,
        package_artifact: &str,
        acceptance_artifact: &str,
        signature_artifact: &str,
        public_key_base64: &str,
        prior_signatures: &[String],
        previous_head: Option<[u8; 32]>,
    ) -> Result<SigningCommit, SigningError> {
        let data = self.decide(base_request(
            ":commit",
            [
                (
                    ":acceptance-artifact",
                    Term::Str(acceptance_artifact.to_string()),
                ),
                (":package-artifact", Term::Str(package_artifact.to_string())),
                (
                    ":previous-head",
                    previous_head.map(hash_bytes).unwrap_or(Term::Nil),
                ),
                (
                    ":prior-signatures",
                    Term::Vector(prior_signatures.iter().cloned().map(Term::Str).collect()),
                ),
                (
                    ":public-key-base64",
                    Term::Str(public_key_base64.to_string()),
                ),
                (
                    ":signature-artifact",
                    Term::Str(signature_artifact.to_string()),
                ),
            ],
        ))?;
        let fields = exact_map(
            &data,
            "commit data",
            &[":signature-set", ":transparency-entry"],
        )?;
        let signature_set = required_hash_vector(fields, ":signature-set", "commit data")?;
        let transparency_entry = fields
            .get(&key(":transparency-entry"))
            .cloned()
            .ok_or_else(|| authority_error("commit data missing :transparency-entry"))?;
        validate_transparency_entry(
            &transparency_entry,
            package_artifact,
            acceptance_artifact,
            signature_artifact,
            public_key_base64,
            previous_head,
        )?;
        Ok(SigningCommit {
            signature_set,
            transparency_entry,
        })
    }

    pub fn dsse_message(
        &mut self,
        payload_type: &str,
        payload: &[u8],
    ) -> Result<Vec<u8>, SigningError> {
        let data = self.decide(base_request(
            ":dsse-plan",
            [
                (":payload", Term::Bytes(payload.to_vec().into())),
                (":payload-type", Term::Str(payload_type.to_string())),
            ],
        ))?;
        let fields = exact_map(&data, "DSSE plan", &[":message"])?;
        required_bytes(fields, ":message", "DSSE plan")
    }

    #[allow(clippy::too_many_arguments)]
    pub fn dsse_artifact(
        &mut self,
        payload_type: &str,
        payload: &[u8],
        payload_hash: [u8; 32],
        public_key: [u8; 32],
        key_hash: [u8; 32],
        signature: [u8; 64],
        signature_valid: bool,
    ) -> Result<DsseSigningArtifact, SigningError> {
        let data = self.decide(base_request(
            ":dsse-finalize",
            [
                (":key-h", hash_bytes(key_hash)),
                (":payload", Term::Bytes(payload.to_vec().into())),
                (":payload-h", hash_bytes(payload_hash)),
                (":payload-type", Term::Str(payload_type.to_string())),
                (":public-key", hash_bytes(public_key)),
                (":signature", Term::Bytes(signature.to_vec().into())),
                (":signature-valid", Term::Bool(signature_valid)),
            ],
        ))?;
        decode_dsse_artifact(
            data,
            payload_type,
            payload,
            payload_hash,
            public_key,
            key_hash,
            signature,
        )
    }

    fn decide(&mut self, request: Term) -> Result<Term, SigningError> {
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
                .ok_or_else(|| authority_error(format!("returned an opaque value: {value:?}")))?,
        };
        decode_result(term, request_hash)
    }
}

include!("signing_authority_decode.rs");
