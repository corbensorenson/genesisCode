use ed25519_dalek::{Signature, VerifyingKey};
use gc_coreform::HASH_DOMAIN_PREFIX;
use thiserror::Error;

use crate::schema::Attestation;

#[derive(Debug, Error)]
pub enum CommitAttestationError {
    #[error("attestation invalid: {0}")]
    Invalid(String),
    #[error("signature verification failed")]
    VerifyFailed,
}

pub fn commit_attestation_message(commit_hash: &[u8; 32]) -> Vec<u8> {
    let mut msg = Vec::with_capacity(32 + 24);
    msg.extend_from_slice(HASH_DOMAIN_PREFIX);
    msg.extend_from_slice(b"vcs\0commit-sign\0");
    msg.extend_from_slice(commit_hash);
    msg
}

pub fn verify_commit_attestation(
    att: &Attestation,
    expected_signing_hash: &[u8; 32],
    allowed: &[VerifyingKey],
) -> Result<(), CommitAttestationError> {
    if att.alg != "ed25519" {
        return Err(CommitAttestationError::Invalid(format!(
            "unsupported alg {}",
            att.alg
        )));
    }
    if &att.signing_hash != expected_signing_hash {
        return Err(CommitAttestationError::Invalid(
            "attestation signing hash mismatch".to_string(),
        ));
    }
    let msg = commit_attestation_message(expected_signing_hash);

    let sig = Signature::from_bytes(&att.sig);
    let pk = VerifyingKey::from_bytes(&att.pk)
        .map_err(|e| CommitAttestationError::Invalid(format!("bad pk: {e}")))?;
    if !allowed.iter().any(|k| k == &pk) {
        return Err(CommitAttestationError::VerifyFailed);
    }
    pk.verify_strict(&msg, &sig)
        .map_err(|_| CommitAttestationError::VerifyFailed)
}
