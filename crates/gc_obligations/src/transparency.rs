use std::fs;
use std::path::{Path, PathBuf};

use gc_coreform::{Term, TermOrdKey, parse_term, print_term};
use thiserror::Error;

use crate::store::EvidenceStore;

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
        Some(h) => Term::Bytes(hex32_to_bytes(h).map_err(TransparencyError::Log)?.to_vec()),
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
) -> Result<TransparencyVerifyResult, TransparencyError> {
    let head_path = transparency_head_path(pkg_dir);
    let head = fs::read_to_string(&head_path)
        .ok()
        .map(|s| s.trim().to_string());
    let head = head.filter(|s| looks_like_hex32(s));

    let mut errors: Vec<String> = Vec::new();
    let mut entries = 0usize;

    let mut cur = head.clone();
    while let Some(hex) = cur.as_deref() {
        if let Err(e) = store.verify_hex(hex) {
            errors.push(format!("{e}"));
            break;
        }
        entries = entries.saturating_add(1);

        let t = match read_term_from_store(store, hex) {
            Ok(t) => t,
            Err(e) => {
                errors.push(format!("{e}"));
                break;
            }
        };

        let Term::Map(m) = t else {
            errors.push(format!("transparency entry {hex} must be a map"));
            break;
        };
        let kind = m.get(&TermOrdKey(Term::symbol(":kind")));
        if !matches!(kind, Some(Term::Str(s)) if s == "genesis/transparency-entry-v0.2") {
            errors.push(format!(
                "transparency entry {hex} has wrong :kind: {}",
                kind.map(print_term).unwrap_or_else(|| "nil".to_string())
            ));
            break;
        }

        let prev = m.get(&TermOrdKey(Term::symbol(":prev-h")));
        cur = match prev {
            None | Some(Term::Nil) => None,
            Some(Term::Bytes(b)) => {
                if b.len() != 32 {
                    errors.push(format!("transparency entry {hex} :prev-h must be 32 bytes"));
                    break;
                }
                Some(bytes_to_hex32(b))
            }
            Some(other) => {
                errors.push(format!(
                    "transparency entry {hex} :prev-h must be bytes or nil, got {}",
                    print_term(other)
                ));
                break;
            }
        };
    }

    Ok(TransparencyVerifyResult {
        ok: errors.is_empty(),
        head,
        entries,
        errors,
    })
}

fn read_term_from_store(store: &EvidenceStore, hex: &str) -> Result<Term, TransparencyError> {
    let p = store.path_for(hex);
    let s = fs::read_to_string(&p)?;
    parse_term(&s).map_err(|e| TransparencyError::Log(format!("bad artifact {}: {e}", p.display())))
}

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
