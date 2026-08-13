use std::collections::BTreeSet;
use std::fs;
use std::path::{Path, PathBuf};

use gc_coreform::{Term, TermOrdKey, parse_term};
use thiserror::Error;

use crate::evidence_verify_authority::{EvidenceVerifyAuthority, TransparencyEntryObservation};
use crate::store::EvidenceStore;

const MAX_TRANSPARENCY_ENTRIES: usize = 16_384;

#[derive(Debug, Error)]
pub enum TransparencyError {
    #[error("io error: {0}")]
    Io(#[from] std::io::Error),

    #[error("log error: {0}")]
    Log(String),
}

#[derive(Debug, Clone)]
pub struct TransparencyVerifyResult {
    pub ok: bool,
    pub head: Option<String>,
    pub entries: usize,
    pub errors: Vec<String>,
}

pub fn transparency_head_path(pkg_dir: &Path) -> PathBuf {
    pkg_dir.join(".genesis").join("transparency_head")
}

#[cfg(feature = "parity-oracle")]
pub fn append_transparency_entry(
    store: &EvidenceStore,
    pkg_dir: &Path,
    package_artifact: &str,
    acceptance_artifact: &str,
    signature_artifact: &str,
    signer_pk_b64: &str,
) -> Result<String, TransparencyError> {
    let genesis_dir = pkg_dir.join(".genesis");
    fs::create_dir_all(&genesis_dir)?;

    let head_path = transparency_head_path(pkg_dir);
    let prev_hex = fs::read_to_string(&head_path)
        .ok()
        .map(|s| s.trim().to_string());
    let prev_hex = prev_hex.filter(|s| looks_like_hex32(s));

    let prev_bytes = match prev_hex.as_deref() {
        None => Term::Nil,
        Some(h) => Term::Bytes(
            hex32_to_bytes(h)
                .map_err(TransparencyError::Log)?
                .to_vec()
                .into(),
        ),
    };

    let entry = Term::Map(
        [
            (
                TermOrdKey(Term::symbol(":kind")),
                Term::Str("genesis/transparency-entry-v0.2".to_string()),
            ),
            (TermOrdKey(Term::symbol(":prev-h")), prev_bytes),
            (
                TermOrdKey(Term::symbol(":package-artifact")),
                Term::Str(package_artifact.to_string()),
            ),
            (
                TermOrdKey(Term::symbol(":acceptance-artifact")),
                Term::Str(acceptance_artifact.to_string()),
            ),
            (
                TermOrdKey(Term::symbol(":signature-artifact")),
                Term::Str(signature_artifact.to_string()),
            ),
            (
                TermOrdKey(Term::symbol(":signer-pk-b64")),
                Term::Str(signer_pk_b64.to_string()),
            ),
        ]
        .into_iter()
        .collect(),
    );

    let entry_hex = store
        .put_term(&entry)
        .map_err(|e| TransparencyError::Log(format!("{e}")))?;

    fs::write(&head_path, format!("{entry_hex}\n"))?;
    Ok(entry_hex)
}

pub fn verify_transparency_log(
    store: &EvidenceStore,
    pkg_dir: &Path,
    authority_artifact: &Path,
) -> Result<TransparencyVerifyResult, TransparencyError> {
    let head_path = transparency_head_path(pkg_dir);
    let (head, head_bytes, head_error) = read_head_observation(&head_path);
    let mut observations = Vec::new();
    let mut cur = head.clone();
    let mut seen = BTreeSet::new();
    while let Some(hex) = cur.as_deref() {
        if observations.len() >= MAX_TRANSPARENCY_ENTRIES {
            observations.push(TransparencyEntryObservation {
                hash: hex32_to_bytes(hex).unwrap_or([0; 32]),
                observed_hash: None,
                load_error: Some("transparency chain exceeds finite entry limit".to_string()),
                term: Term::Nil,
            });
            break;
        }
        let hash = match hex32_to_bytes(hex) {
            Ok(hash) => hash,
            Err(error) => {
                observations.push(TransparencyEntryObservation {
                    hash: [0; 32],
                    observed_hash: None,
                    load_error: Some(error),
                    term: Term::Nil,
                });
                break;
            }
        };
        let (observed_hash, term, load_error) = match store.observe_bytes(hex) {
            Ok((bytes, observed)) => {
                let observed_hash = match hex32_to_bytes(&observed) {
                    Ok(hash) => Some(hash),
                    Err(error) => {
                        observations.push(TransparencyEntryObservation {
                            hash,
                            observed_hash: None,
                            load_error: Some(error),
                            term: Term::Nil,
                        });
                        break;
                    }
                };
                match parse_observed_term(&bytes, &store.path_for(hex)) {
                    Ok(term) => (observed_hash, term, None),
                    Err(error) => (observed_hash, Term::Nil, Some(error.to_string())),
                }
            }
            Err(error) => (None, Term::Nil, Some(error.to_string())),
        };
        observations.push(TransparencyEntryObservation {
            hash,
            observed_hash,
            load_error,
            term: term.clone(),
        });
        if !seen.insert(hex.to_string()) {
            break;
        }
        cur = proposed_previous_hash(&term);
    }

    let mut authority = EvidenceVerifyAuthority::load(authority_artifact)
        .map_err(|error| TransparencyError::Log(error.to_string()))?;
    let decision = authority
        .transparency(head_bytes, head_error, observations)
        .map_err(|error| TransparencyError::Log(error.to_string()))?;
    Ok(TransparencyVerifyResult {
        ok: decision.verified,
        head,
        entries: decision.checked,
        errors: decision.errors,
    })
}

fn read_head_observation(path: &Path) -> (Option<String>, Option<[u8; 32]>, Option<String>) {
    match fs::read_to_string(path) {
        Ok(source) => {
            let value = source.trim().to_string();
            match hex32_to_bytes(&value) {
                Ok(bytes) => (Some(value), Some(bytes), None),
                Err(_) => (
                    Some(value),
                    None,
                    Some("malformed transparency head".to_string()),
                ),
            }
        }
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => (None, None, None),
        Err(error) => (
            None,
            None,
            Some(format!("cannot read transparency head: {error}")),
        ),
    }
}

fn proposed_previous_hash(term: &Term) -> Option<String> {
    let Term::Map(fields) = term else { return None };
    match fields.get(&TermOrdKey(Term::symbol(":prev-h"))) {
        Some(Term::Bytes(bytes)) if bytes.len() == 32 => Some(bytes_to_hex32(bytes)),
        _ => None,
    }
}

fn parse_observed_term(bytes: &[u8], path: &Path) -> Result<Term, TransparencyError> {
    let source = std::str::from_utf8(bytes).map_err(|error| {
        TransparencyError::Log(format!("bad artifact {}: {error}", path.display()))
    })?;
    parse_term(source).map_err(|error| {
        TransparencyError::Log(format!("bad artifact {}: {error}", path.display()))
    })
}

#[cfg(feature = "parity-oracle")]
fn looks_like_hex32(s: &str) -> bool {
    s.len() == 64 && s.chars().all(|c| c.is_ascii_hexdigit())
}

fn hex32_to_bytes(s: &str) -> Result<[u8; 32], String> {
    let t = s.trim();
    if t.len() != 64 || !t.chars().all(|c| c.is_ascii_hexdigit()) {
        return Err("invalid hex hash".to_string());
    }
    let mut out = [0u8; 32];
    for (i, b) in out.iter_mut().enumerate() {
        let hi = hex_val(t.as_bytes()[2 * i]).ok_or_else(|| "invalid hex".to_string())?;
        let lo = hex_val(t.as_bytes()[2 * i + 1]).ok_or_else(|| "invalid hex".to_string())?;
        *b = (hi << 4) | lo;
    }
    Ok(out)
}

fn bytes_to_hex32(b: &[u8]) -> String {
    const HEX: &[u8; 16] = b"0123456789abcdef";
    let mut out = String::new();
    for &x in b {
        out.push(HEX[(x >> 4) as usize] as char);
        out.push(HEX[(x & 0x0f) as usize] as char);
    }
    out
}

fn hex_val(b: u8) -> Option<u8> {
    match b {
        b'0'..=b'9' => Some(b - b'0'),
        b'a'..=b'f' => Some(10 + (b - b'a')),
        b'A'..=b'F' => Some(10 + (b - b'A')),
        _ => None,
    }
}
