use gc_coreform::{Term, canonicalize_module, hash_module, parse_module};
use thiserror::Error;

#[derive(Debug, Clone)]
pub struct CanonicalModule {
    pub forms: Vec<Term>,
    pub module_hash: [u8; 32],
}

#[derive(Debug, Clone, Error)]
pub enum ModuleSemanticError {
    #[error("{message}")]
    Parse { message: String },

    #[error("{message}")]
    Canon { message: String },

    #[error("manifest module hash is not 64-hex: {path}")]
    BadPinnedHash { path: String },

    #[error("module hash mismatch: {path}")]
    HashMismatch { path: String },
}

pub fn parse_canonical_module_source(
    src: &str,
    module_path: &str,
    pinned_hash_hex: Option<&str>,
) -> Result<CanonicalModule, ModuleSemanticError> {
    let forms = parse_module(src).map_err(|e| ModuleSemanticError::Parse {
        message: e.to_string(),
    })?;
    let forms = canonicalize_module(forms).map_err(|e| ModuleSemanticError::Canon {
        message: e.to_string(),
    })?;
    let module_hash = hash_module(&forms);

    if let Some(want_hex) = pinned_hash_hex {
        if want_hex.len() != 64 || !want_hex.chars().all(|c| c.is_ascii_hexdigit()) {
            return Err(ModuleSemanticError::BadPinnedHash {
                path: module_path.to_string(),
            });
        }
        let got_hex = blake3::Hash::from_bytes(module_hash).to_hex().to_string();
        if got_hex != want_hex {
            return Err(ModuleSemanticError::HashMismatch {
                path: module_path.to_string(),
            });
        }
    }

    Ok(CanonicalModule { forms, module_hash })
}

#[cfg(test)]
mod tests {
    use super::{ModuleSemanticError, parse_canonical_module_source};

    #[test]
    fn rejects_non_hex_pinned_hash() {
        let err = parse_canonical_module_source("(def x 1)\n", "x.gc", Some("not-hex"))
            .expect_err("must fail on malformed hash");
        assert!(matches!(err, ModuleSemanticError::BadPinnedHash { .. }));
    }

    #[test]
    fn rejects_hash_mismatch() {
        let err = parse_canonical_module_source(
            "(def x 1)\n",
            "x.gc",
            Some("0000000000000000000000000000000000000000000000000000000000000000"),
        )
        .expect_err("must fail on hash mismatch");
        assert!(matches!(err, ModuleSemanticError::HashMismatch { .. }));
    }
}
