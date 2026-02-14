use std::path::{Path, PathBuf};

use blake3::Hasher;

use crate::error::ObligationError;
use gc_coreform::{print_term, Term};

#[derive(Debug, Clone)]
pub struct EvidenceStore {
    root: PathBuf,
}

impl EvidenceStore {
    pub fn open(package_root: &Path) -> Result<Self, ObligationError> {
        let root = package_root.join(".genesis").join("store");
        std::fs::create_dir_all(&root)?;
        Ok(Self { root })
    }

    pub fn path_for(&self, hex: &str) -> PathBuf {
        self.root.join(hex)
    }

    pub fn put_bytes(&self, bytes: &[u8]) -> Result<String, ObligationError> {
        let mut h = Hasher::new();
        h.update(bytes);
        let hex = h.finalize().to_hex().to_string();
        let path = self.path_for(&hex);
        if path.exists() {
            return Ok(hex);
        }
        let tmp = self.root.join(format!(".tmp-{}-{}", hex, std::process::id()));
        std::fs::write(&tmp, bytes)?;
        std::fs::rename(&tmp, &path)?;
        Ok(hex)
    }

    pub fn put_term(&self, t: &Term) -> Result<String, ObligationError> {
        let s = print_term(t);
        self.put_bytes(s.as_bytes())
    }
}

