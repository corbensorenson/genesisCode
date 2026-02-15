use std::collections::BTreeMap;
use std::fs;
use std::path::{Path, PathBuf};

use base64ct::{Base64, Encoding};
use ed25519_dalek::{Signature, Signer, SigningKey, VerifyingKey};
use gc_coreform::{Term, TermOrdKey, parse_term, print_term};
use rand_core::OsRng;
use serde::{Deserialize, Serialize};
use thiserror::Error;
use zeroize::Zeroizing;

use crate::store::EvidenceStore;

#[derive(Debug, Error)]
pub enum SigningError {
    #[error("io error: {0}")]
    Io(#[from] std::io::Error),

    #[error("key parse error: {0}")]
    KeyParse(String),

    #[error("signature parse error: {0}")]
    SigParse(String),

    #[error("store error: {0}")]
    Store(String),

    #[error("signature verification failed")]
    VerifyFailed,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct KeyFile {
    pub alg: String,
    pub sk_b64: String,
    pub pk_b64: String,
}

impl KeyFile {
    pub fn generate_ed25519() -> Self {
        let sk = SigningKey::generate(&mut OsRng);
        let pk = sk.verifying_key();
        Self {
            alg: "ed25519".to_string(),
            sk_b64: Base64::encode_string(sk.to_bytes().as_slice()),
            pk_b64: Base64::encode_string(pk.to_bytes().as_slice()),
        }
    }

    pub fn load(path: &Path) -> Result<Self, SigningError> {
        let s = fs::read_to_string(path)?;
        let k: KeyFile = toml::from_str(&s)
            .map_err(|e| SigningError::KeyParse(format!("{}: {e}", path.display())))?;
        if k.alg != "ed25519" {
            return Err(SigningError::KeyParse(format!(
                "{}: unsupported alg {}",
                path.display(),
                k.alg
            )));
        }
        // Validate base64 payloads early.
        let _ = decode_b64_32(&k.sk_b64).map_err(SigningError::KeyParse)?;
        let _ = decode_b64_32(&k.pk_b64).map_err(SigningError::KeyParse)?;
        Ok(k)
    }

    pub fn write_secure(&self, path: &Path) -> Result<(), SigningError> {
        let s = toml::to_string_pretty(self)
            .map_err(|e| SigningError::KeyParse(format!("serialize key: {e}")))?;
        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent)?;
        }
        fs::write(path, s.as_bytes())?;
        // Best-effort permission hardening on Unix.
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            let perm = fs::Permissions::from_mode(0o600);
            let _ = fs::set_permissions(path, perm);
        }
        Ok(())
    }

    pub fn signing_key(&self) -> Result<SigningKey, SigningError> {
        let sk = Zeroizing::new(decode_b64_32(&self.sk_b64).map_err(SigningError::KeyParse)?);
        Ok(SigningKey::from_bytes(&sk))
    }

    pub fn verifying_key(&self) -> Result<VerifyingKey, SigningError> {
        let pk = decode_b64_32(&self.pk_b64).map_err(SigningError::KeyParse)?;
        VerifyingKey::from_bytes(&pk).map_err(|e| SigningError::KeyParse(format!("bad pk: {e}")))
    }
}

#[derive(Debug, Clone)]
pub struct AcceptanceSignature {
    pub acceptance_hash: [u8; 32],
    pub pk: [u8; 32],
    pub sig: [u8; 64],
}

impl AcceptanceSignature {
    pub fn to_term(&self) -> Term {
        Term::Map(
            [
                (
                    TermOrdKey(Term::symbol(":kind")),
                    Term::Str("genesis/acceptance-signature-v0.2".to_string()),
                ),
                (
                    TermOrdKey(Term::symbol(":alg")),
                    Term::Str("ed25519".to_string()),
                ),
                (
                    TermOrdKey(Term::symbol(":acceptance-h")),
                    Term::Bytes(self.acceptance_hash.to_vec()),
                ),
                (
                    TermOrdKey(Term::symbol(":pk")),
                    Term::Bytes(self.pk.to_vec()),
                ),
                (
                    TermOrdKey(Term::symbol(":sig")),
                    Term::Bytes(self.sig.to_vec()),
                ),
            ]
            .into_iter()
            .collect(),
        )
    }

    pub fn from_term(t: &Term) -> Result<Self, SigningError> {
        let Term::Map(m) = t else {
            return Err(SigningError::SigParse(
                "signature artifact must be a map".to_string(),
            ));
        };
        let kind = m.get(&TermOrdKey(Term::symbol(":kind")));
        if !matches!(kind, Some(Term::Str(s)) if s == "genesis/acceptance-signature-v0.2") {
            return Err(SigningError::SigParse(format!(
                "wrong :kind (expected genesis/acceptance-signature-v0.2, got {})",
                kind.map(print_term).unwrap_or_else(|| "nil".to_string())
            )));
        }
        let alg = m.get(&TermOrdKey(Term::symbol(":alg")));
        if !matches!(alg, Some(Term::Str(s)) if s == "ed25519") {
            return Err(SigningError::SigParse("unsupported :alg".to_string()));
        }
        let acceptance_hash = bytes32_field(m, ":acceptance-h")?;
        let pk = bytes32_field(m, ":pk")?;
        let sig = bytes64_field(m, ":sig")?;
        Ok(Self {
            acceptance_hash,
            pk,
            sig,
        })
    }

    pub fn verify(&self, allowed_pks: &[VerifyingKey]) -> Result<(), SigningError> {
        let msg = acceptance_message(&self.acceptance_hash);
        let sig = Signature::from_bytes(&self.sig);
        let pk = VerifyingKey::from_bytes(&self.pk)
            .map_err(|e| SigningError::SigParse(format!("bad pk bytes: {e}")))?;
        if !allowed_pks.iter().any(|k| k == &pk) {
            return Err(SigningError::VerifyFailed);
        }
        pk.verify_strict(&msg, &sig)
            .map_err(|_| SigningError::VerifyFailed)
    }
}

pub fn acceptance_message(acceptance_hash: &[u8; 32]) -> Vec<u8> {
    let mut msg = Vec::with_capacity(16 + 32);
    msg.extend_from_slice(b"GCv0.2\0acceptance\0");
    msg.extend_from_slice(acceptance_hash);
    msg
}

pub fn sign_acceptance_hash(
    store: &EvidenceStore,
    acceptance_hex: &str,
    key: &SigningKey,
) -> Result<(String, AcceptanceSignature), SigningError> {
    let acceptance_hash = hex32_to_bytes(acceptance_hex)?;
    let msg = acceptance_message(&acceptance_hash);
    let sig = key.sign(&msg);
    let pk = key.verifying_key().to_bytes();
    let rec = AcceptanceSignature {
        acceptance_hash,
        pk,
        sig: sig.to_bytes(),
    };
    let artifact = store
        .put_term(&rec.to_term())
        .map_err(|e| SigningError::Store(format!("{e}")))?;
    Ok((artifact, rec))
}

pub fn read_acceptance_hash_from_last(pkg_dir: &Path) -> Result<String, SigningError> {
    let p = pkg_dir.join(".genesis").join("last_acceptance");
    let s = fs::read_to_string(&p)?;
    let t = s.trim();
    if t.len() == 64 && t.chars().all(|c| c.is_ascii_hexdigit()) {
        Ok(t.to_string())
    } else {
        Err(SigningError::SigParse(format!(
            "{}: invalid acceptance hash",
            p.display()
        )))
    }
}

pub fn signatures_file_path(pkg_dir: &Path) -> PathBuf {
    pkg_dir.join(".genesis").join("signatures.gc")
}

pub fn load_signature_set(path: &Path) -> Result<Vec<String>, SigningError> {
    let s = fs::read_to_string(path)?;
    let t = parse_term(&s).map_err(|e| SigningError::SigParse(format!("{e}")))?;
    let Term::Vector(xs) = t else {
        return Err(SigningError::SigParse(
            "signatures file must be a vector".to_string(),
        ));
    };
    let mut out = Vec::new();
    for x in xs {
        match x {
            Term::Str(s) if s.len() == 64 && s.chars().all(|c| c.is_ascii_hexdigit()) => {
                out.push(s)
            }
            _ => {
                return Err(SigningError::SigParse(
                    "signatures file entries must be 64-hex strings".to_string(),
                ));
            }
        }
    }
    Ok(out)
}

pub fn write_signature_set(path: &Path, sigs: &[String]) -> Result<(), SigningError> {
    let mut v = sigs.to_vec();
    v.sort();
    v.dedup();
    let t = Term::Vector(v.into_iter().map(Term::Str).collect());
    let out = gc_coreform::print_term(&t);
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)?;
    }
    fs::write(path, out.as_bytes())?;
    Ok(())
}

fn decode_b64_32(s: &str) -> Result<[u8; 32], String> {
    let mut out = [0u8; 32];
    Base64::decode(s, &mut out).map_err(|e| format!("invalid base64: {e}"))?;
    Ok(out)
}

fn hex32_to_bytes(s: &str) -> Result<[u8; 32], SigningError> {
    let t = s.trim();
    if t.len() != 64 || !t.chars().all(|c| c.is_ascii_hexdigit()) {
        return Err(SigningError::SigParse("invalid hex hash".to_string()));
    }
    let mut out = [0u8; 32];
    for (i, b) in out.iter_mut().enumerate() {
        let hi = hex_val(t.as_bytes()[2 * i])
            .ok_or_else(|| SigningError::SigParse("invalid hex".to_string()))?;
        let lo = hex_val(t.as_bytes()[2 * i + 1])
            .ok_or_else(|| SigningError::SigParse("invalid hex".to_string()))?;
        *b = (hi << 4) | lo;
    }
    Ok(out)
}

fn hex_val(b: u8) -> Option<u8> {
    match b {
        b'0'..=b'9' => Some(b - b'0'),
        b'a'..=b'f' => Some(10 + (b - b'a')),
        b'A'..=b'F' => Some(10 + (b - b'A')),
        _ => None,
    }
}

fn bytes32_field(m: &BTreeMap<TermOrdKey, Term>, key: &str) -> Result<[u8; 32], SigningError> {
    let Some(Term::Bytes(b)) = m.get(&TermOrdKey(Term::symbol(key))) else {
        return Err(SigningError::SigParse(format!("missing {key}")));
    };
    if b.len() != 32 {
        return Err(SigningError::SigParse(format!("{key} must be 32 bytes")));
    }
    let mut out = [0u8; 32];
    out.copy_from_slice(b);
    Ok(out)
}

fn bytes64_field(m: &BTreeMap<TermOrdKey, Term>, key: &str) -> Result<[u8; 64], SigningError> {
    let Some(Term::Bytes(b)) = m.get(&TermOrdKey(Term::symbol(key))) else {
        return Err(SigningError::SigParse(format!("missing {key}")));
    };
    if b.len() != 64 {
        return Err(SigningError::SigParse(format!("{key} must be 64 bytes")));
    }
    let mut out = [0u8; 64];
    out.copy_from_slice(b);
    Ok(out)
}
