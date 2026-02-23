use std::path::{Path, PathBuf};

use gc_coreform::{parse_term, print_term};

use super::term_helpers::is_hex64;
use super::types::LoadedTerm;

pub(super) fn load_term_from_spec(
    spec: &str,
    base_dir: &Path,
    store_dir: &Path,
    label: &str,
) -> Result<LoadedTerm, String> {
    let candidate = PathBuf::from(spec);
    let path = if candidate.is_file() || candidate.is_absolute() {
        candidate
    } else {
        let from_base = base_dir.join(spec);
        if from_base.is_file() {
            from_base
        } else if is_hex64(spec) {
            store_dir.join(spec)
        } else {
            from_base
        }
    };
    if !path.is_file() {
        return Err(format!(
            "{label} artifact spec `{spec}` did not resolve to a readable file (tried {})",
            path.display()
        ));
    }
    let src =
        std::fs::read_to_string(&path).map_err(|e| format!("read {}: {e}", path.display()))?;
    let term = parse_term(&src).map_err(|e| format!("parse {}: {e}", path.display()))?;
    let canonical_src = print_term(&term) + "\n";
    let hash = blake3::hash(canonical_src.as_bytes()).to_hex().to_string();
    if is_hex64(spec) && spec != hash {
        return Err(format!(
            "{label} artifact hash mismatch for `{spec}`: canonical hash is {hash}"
        ));
    }
    Ok(LoadedTerm {
        term,
        hash,
        canonical_src,
        source: path.display().to_string(),
    })
}
