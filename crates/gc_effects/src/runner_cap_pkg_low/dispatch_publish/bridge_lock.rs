use super::*;
use crate::pkg_lock_read_authority::{PkgBridgeLockFacts, PkgLockOpsDecision};

pub(super) struct BridgeLockUpdate<'a> {
    pub(super) lock: &'a str,
    pub(super) dep: &'a str,
    pub(super) registry: Option<&'a str>,
    pub(super) commit: &'a str,
    pub(super) snapshot: &'a str,
    pub(super) provenance_root: &'a str,
    pub(super) conversion_evidence: &'a str,
    pub(super) attestation: &'a str,
}

pub(super) fn authority_unavailable() -> EffectsError {
    EffectsError::Log(
        "core/pkg-low::bridge lock mutation requires the artifact-loaded GenesisCode lock ops authority"
            .to_string(),
    )
}

pub(super) fn update_lock(
    update: BridgeLockUpdate<'_>,
    policy: Option<&OpPolicy>,
    authority: Option<&mut PkgLockReadAuthority>,
    error_tok: SealId,
    op: &str,
) -> Result<Result<String, Value>, EffectsError> {
    let Some(authority) = authority else {
        return Err(authority_unavailable());
    };
    let base_dir = effective_base_dir(policy)?;
    let read_path = match sandbox_path_read(&base_dir, update.lock) {
        Ok(path) => path,
        Err(error) => {
            return Ok(Err(mk_error(
                error_tok,
                "core/pkg/missing-lock",
                error.to_string(),
                Some(op),
            )));
        }
    };
    let bytes = match read_bounded_lock(&read_path) {
        Ok(bytes) => bytes,
        Err(message) => {
            return Ok(Err(mk_error(
                error_tok,
                "core/pkg/bad-lock",
                message,
                Some(op),
            )));
        }
    };
    let write_path = match sandbox_path_write(&base_dir, update.lock, false) {
        Ok(path) => path,
        Err(error) => {
            return Ok(Err(mk_error(
                error_tok,
                "core/caps/path-escape",
                error.to_string(),
                Some(op),
            )));
        }
    };
    let facts = PkgBridgeLockFacts {
        dep: update.dep,
        registry: update.registry,
        commit: update.commit,
        snapshot: update.snapshot,
        provenance_root: update.provenance_root,
        conversion_evidence: update.conversion_evidence,
        attestation: update.attestation,
    };
    match authority.bridge_lock_toml(&bytes, facts)? {
        PkgLockOpsDecision::Write { bytes, lock_hash } => {
            if let Err(error) = atomic_write_text(&write_path, &bytes) {
                return Ok(Err(mk_error(
                    error_tok,
                    "core/pkg/io-error",
                    error.to_string(),
                    Some(op),
                )));
            }
            Ok(Ok(lock_hash))
        }
        PkgLockOpsDecision::Error { code, message } => {
            Ok(Err(mk_error(error_tok, &code, message, Some(op))))
        }
        PkgLockOpsDecision::List { .. } => Err(EffectsError::Log(
            "selfhost package lock ops authority returned list for bridge lock mutation"
                .to_string(),
        )),
    }
}
