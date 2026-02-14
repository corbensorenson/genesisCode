use std::fs::OpenOptions;
use std::io::Write;
use std::path::{Path, PathBuf};

use blake3::Hasher;

use crate::error::ObligationError;
use gc_coreform::{Term, print_term};

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

    pub fn root_dir(&self) -> &Path {
        &self.root
    }

    fn hash_bytes(bytes: &[u8]) -> String {
        let mut h = Hasher::new();
        h.update(bytes);
        h.finalize().to_hex().to_string()
    }

    fn verify_existing(&self, hex: &str) -> Result<(), ObligationError> {
        let path = self.path_for(hex);
        let bytes = std::fs::read(&path)?;
        let got = Self::hash_bytes(&bytes);
        if got != hex {
            return Err(ObligationError::Store(format!(
                "evidence store corruption: {} expected hash {}, got {}",
                path.display(),
                hex,
                got
            )));
        }
        Ok(())
    }

    pub fn verify_hex(&self, hex: &str) -> Result<(), ObligationError> {
        self.verify_existing(hex)
    }

    pub fn put_bytes(&self, bytes: &[u8]) -> Result<String, ObligationError> {
        let hex = Self::hash_bytes(bytes);
        let path = self.path_for(&hex);

        // If already present, verify integrity and return.
        if path.exists() {
            self.verify_existing(&hex)?;
            return Ok(hex);
        }

        // Write-once, content-addressed storage:
        // - use a create_new temp file to avoid clobbering other writers
        // - rename into place (atomic on same filesystem)
        // - tolerate races where another writer wins
        let mut tmp_i: u64 = 0;
        let tmp_path = loop {
            let cand = self
                .root
                .join(format!(".tmp-{}-{}-{}", hex, std::process::id(), tmp_i));
            tmp_i = tmp_i.saturating_add(1);
            match OpenOptions::new().write(true).create_new(true).open(&cand) {
                Ok(mut f) => {
                    f.write_all(bytes)?;
                    f.sync_all()?;
                    break cand;
                }
                Err(e) if e.kind() == std::io::ErrorKind::AlreadyExists => continue,
                Err(e) => return Err(e.into()),
            }
        };

        match std::fs::rename(&tmp_path, &path) {
            Ok(()) => {
                // Best-effort directory fsync for durability on platforms that support it.
                #[cfg(unix)]
                {
                    let dir = std::fs::File::open(&self.root)?;
                    dir.sync_all()?;
                }
            }
            Err(e) if e.kind() == std::io::ErrorKind::AlreadyExists => {
                // Another writer won the race; discard our temp and verify existing contents.
                let _ = std::fs::remove_file(&tmp_path);
                self.verify_existing(&hex)?;
            }
            Err(e) => {
                let _ = std::fs::remove_file(&tmp_path);
                return Err(e.into());
            }
        }

        Ok(hex)
    }

    pub fn put_term(&self, t: &Term) -> Result<String, ObligationError> {
        let s = print_term(t);
        self.put_bytes(s.as_bytes())
    }
}

#[cfg(test)]
mod tests {
    use super::EvidenceStore;

    #[test]
    fn concurrent_put_is_race_safe() {
        let td = tempfile::tempdir().unwrap();
        let store = EvidenceStore::open(td.path()).unwrap();
        let bytes = b"hello evidence";

        let mut ths = Vec::new();
        for _ in 0..16 {
            let s = store.clone();
            ths.push(std::thread::spawn(move || s.put_bytes(bytes).unwrap()));
        }
        let hs: Vec<String> = ths.into_iter().map(|t| t.join().unwrap()).collect();
        for h in &hs[1..] {
            assert_eq!(h, &hs[0]);
        }
        assert!(store.path_for(&hs[0]).exists());
    }

    #[test]
    fn detects_corruption_if_hash_path_exists_with_wrong_contents() {
        let td = tempfile::tempdir().unwrap();
        let store = EvidenceStore::open(td.path()).unwrap();

        let good = b"good bytes";
        let bad = b"bad bytes";

        let hex = EvidenceStore::hash_bytes(good);
        std::fs::write(store.path_for(&hex), bad).unwrap();

        let e = store.put_bytes(good).unwrap_err();
        let msg = format!("{e}");
        assert!(msg.contains("corruption"), "{msg}");
    }
}
