use base64ct::{Base64, Encoding};
use ed25519_dalek::{Signature, VerifyingKey};

use super::*;

pub(super) fn verify_crypto_request(
    request: &Term,
    expected_signing_hash: &[u8; 32],
) -> Result<(String, bool), EffectsError> {
    let fields = publish_exact_map(
        request,
        &[
            ":alg",
            ":allowed-public-keys",
            ":attestation-h",
            ":pk",
            ":request-h",
            ":sig",
            ":sign-message",
            ":signing-h",
        ],
        "crypto request",
    )?;
    let request_hash = publish_hash_string(
        publish_field(fields, ":request-h", "crypto request")?,
        ":request-h",
    )?;
    require_embedded_hash(request, ":request-h", &request_hash, "crypto request")?;
    publish_hash_string(
        publish_field(fields, ":attestation-h", "crypto request")?,
        ":attestation-h",
    )?;
    let alg = publish_string(publish_field(fields, ":alg", "crypto request")?, ":alg")?;
    let allowed = string_vector(
        publish_field(fields, ":allowed-public-keys", "crypto request")?,
        ":allowed-public-keys",
    )?;
    let pk = publish_bytes(publish_field(fields, ":pk", "crypto request")?, ":pk")?;
    let sig = publish_bytes(publish_field(fields, ":sig", "crypto request")?, ":sig")?;
    let sign_message = publish_bytes(
        publish_field(fields, ":sign-message", "crypto request")?,
        ":sign-message",
    )?;
    let signing_hash = publish_bytes(
        publish_field(fields, ":signing-h", "crypto request")?,
        ":signing-h",
    )?;
    let valid = verify_ed25519_mechanism(
        &alg,
        &allowed,
        &pk,
        &sig,
        &sign_message,
        &signing_hash,
        expected_signing_hash,
    );
    Ok((request_hash, valid))
}

fn verify_ed25519_mechanism(
    alg: &str,
    allowed: &[String],
    pk: &[u8],
    sig: &[u8],
    sign_message: &[u8],
    signing_hash: &[u8],
    expected_signing_hash: &[u8; 32],
) -> bool {
    if alg != "ed25519" || signing_hash != expected_signing_hash {
        return false;
    }
    let mut expected_message = Vec::with_capacity(54);
    expected_message.extend_from_slice(gc_coreform::HASH_DOMAIN_PREFIX);
    expected_message.extend_from_slice(b"vcs\0commit-sign\0");
    expected_message.extend_from_slice(expected_signing_hash);
    if sign_message != expected_message {
        return false;
    }
    let Ok(pk_bytes) = <[u8; 32]>::try_from(pk) else {
        return false;
    };
    let Ok(sig_bytes) = <[u8; 64]>::try_from(sig) else {
        return false;
    };
    let Ok(key) = VerifyingKey::from_bytes(&pk_bytes) else {
        return false;
    };
    let mut allowed_match = false;
    for encoded in allowed {
        let Ok(decoded) = Base64::decode_vec(encoded) else {
            return false;
        };
        let Ok(decoded) = <[u8; 32]>::try_from(decoded.as_slice()) else {
            return false;
        };
        let Ok(candidate) = VerifyingKey::from_bytes(&decoded) else {
            return false;
        };
        allowed_match |= candidate == key;
    }
    allowed_match
        && key
            .verify_strict(sign_message, &Signature::from_bytes(&sig_bytes))
            .is_ok()
}

pub(super) fn mechanical_signing_hash(commit: &Term) -> Result<[u8; 32], EffectsError> {
    let Term::Map(fields) = commit else {
        return Err(publish_error("bound commit must be a map for signing"));
    };
    let mut unsigned = fields.clone();
    unsigned.insert(
        TermOrdKey(Term::symbol(":attestations")),
        Term::Vector(Vec::new()),
    );
    let mut hasher = blake3::Hasher::new();
    hasher.update(gc_coreform::HASH_DOMAIN_PREFIX);
    hasher.update(b"vcs\0commit-signing-hash\0");
    hasher.update(print_term(&Term::Map(unsigned)).as_bytes());
    Ok(*hasher.finalize().as_bytes())
}
