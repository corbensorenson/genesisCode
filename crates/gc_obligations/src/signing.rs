use std::collections::BTreeMap;
use std::fs::{self, OpenOptions};
use std::io::{Read, Write};
use std::path::{Path, PathBuf};

use base64ct::{Base64, Encoding};
#[cfg(feature = "parity-oracle")]
use ed25519_dalek::Signer;
use ed25519_dalek::{Signature, SigningKey, VerifyingKey};
use gc_coreform::{Term, TermOrdKey, parse_term, print_term};
use rand_core::OsRng;
use serde::{Deserialize, Serialize};
use thiserror::Error;
use zeroize::Zeroizing;

#[cfg(feature = "parity-oracle")]
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

    #[error("selfhost signing authority error: {0}")]
    Authority(String),

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
        let path_metadata = fs::symlink_metadata(path)?;
        if !path_metadata.file_type().is_file() || path_metadata.file_type().is_symlink() {
            return Err(SigningError::KeyParse(format!(
                "{}: signing key must be a regular non-symlink file",
                path.display()
            )));
        }

        let mut options = OpenOptions::new();
        options.read(true);
        #[cfg(unix)]
        {
            use std::os::unix::fs::OpenOptionsExt;
            options.custom_flags(libc::O_NOFOLLOW);
        }
        let mut file = options.open(path)?;
        let metadata = file.metadata()?;
        if !metadata.file_type().is_file() {
            return Err(SigningError::KeyParse(format!(
                "{}: signing key must be a regular non-symlink file",
                path.display()
            )));
        }
        #[cfg(unix)]
        {
            use std::os::unix::fs::{MetadataExt, PermissionsExt};
            if path_metadata.dev() != metadata.dev() || path_metadata.ino() != metadata.ino() {
                return Err(SigningError::KeyParse(format!(
                    "{}: signing key changed while it was being opened",
                    path.display()
                )));
            }
            if metadata.permissions().mode() & 0o077 != 0 {
                return Err(SigningError::KeyParse(format!(
                    "{}: signing key permissions must deny group and other access",
                    path.display()
                )));
            }
        }
        let mut s = String::new();
        file.read_to_string(&mut s)?;
        let k: KeyFile = toml::from_str(&s)
            .map_err(|e| SigningError::KeyParse(format!("{}: {e}", path.display())))?;
        if k.alg != "ed25519" {
            return Err(SigningError::KeyParse(format!(
                "{}: unsupported alg {}",
                path.display(),
                k.alg
            )));
        }
        let signing = k.signing_key()?;
        let verifying = k.verifying_key()?;
        if signing.verifying_key() != verifying {
            return Err(SigningError::KeyParse(format!(
                "{}: signing and public key material do not match",
                path.display()
            )));
        }
        Ok(k)
    }

    pub fn write_secure(&self, path: &Path) -> Result<(), SigningError> {
        let s = toml::to_string_pretty(self)
            .map_err(|e| SigningError::KeyParse(format!("serialize key: {e}")))?;
        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent)?;
        }
        let mut options = OpenOptions::new();
        options.write(true).create_new(true);
        #[cfg(unix)]
        {
            use std::os::unix::fs::OpenOptionsExt;
            options.mode(0o600);
        }
        let mut file = options.open(path)?;
        file.write_all(s.as_bytes())?;
        file.sync_all()?;
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
    #[cfg(feature = "parity-oracle")]
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
                    Term::Bytes(self.acceptance_hash.to_vec().into()),
                ),
                (
                    TermOrdKey(Term::symbol(":pk")),
                    Term::Bytes(self.pk.to_vec().into()),
                ),
                (
                    TermOrdKey(Term::symbol(":sig")),
                    Term::Bytes(self.sig.to_vec().into()),
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

#[cfg(feature = "parity-oracle")]
pub fn sign_acceptance_hash(
    store: &EvidenceStore,
    acceptance_hex: &str,
    key: &SigningKey,
) -> Result<(String, AcceptanceSignature), SigningError> {
    let acceptance_hash = parse_hash32(acceptance_hex)?;
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

#[cfg(feature = "parity-oracle")]
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

pub fn parse_hash32(s: &str) -> Result<[u8; 32], SigningError> {
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn secure_key_write_is_create_only_and_pair_checked() {
        let directory = tempfile::tempdir().expect("tempdir");
        let path = directory.path().join("key.toml");
        let key = KeyFile::generate_ed25519();
        key.write_secure(&path).expect("initial key write");
        key.write_secure(&path)
            .expect_err("key generation must not overwrite existing secret material");
        KeyFile::load(&path).expect("secure matching keypair");

        let other = KeyFile::generate_ed25519();
        let mut mismatched = key.clone();
        mismatched.pk_b64 = other.pk_b64;
        let mismatch_path = directory.path().join("mismatch.toml");
        mismatched
            .write_secure(&mismatch_path)
            .expect("write malformed fixture");
        let error = KeyFile::load(&mismatch_path).expect_err("mismatched keypair must fail");
        assert!(error.to_string().contains("do not match"));
    }

    #[cfg(unix)]
    #[test]
    fn key_load_rejects_permissive_and_symlinked_secret_files() {
        use std::os::unix::fs::{PermissionsExt, symlink};

        let directory = tempfile::tempdir().expect("tempdir");
        let path = directory.path().join("key.toml");
        KeyFile::generate_ed25519()
            .write_secure(&path)
            .expect("secure key");
        fs::set_permissions(&path, fs::Permissions::from_mode(0o644)).expect("permissions");
        let error = KeyFile::load(&path).expect_err("permissive key must fail");
        assert!(error.to_string().contains("deny group and other"));

        fs::set_permissions(&path, fs::Permissions::from_mode(0o600)).expect("permissions");
        let link = directory.path().join("key-link.toml");
        symlink(&path, &link).expect("symlink fixture");
        let error = KeyFile::load(&link).expect_err("symlink key must fail");
        assert!(error.to_string().contains("regular non-symlink"));
    }
}
