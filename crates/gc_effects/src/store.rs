use std::fs::OpenOptions;
use std::io::Write;
use std::path::{Path, PathBuf};

use blake3::Hasher;

use crate::error::EffectsError;

/// Content-addressed artifact store for effect logs.
///
/// This intentionally mirrors the evidence store semantics:
/// - write-once by hash
/// - concurrent-writer tolerant
/// - verifies existing contents match the filename hash
#[derive(Debug, Clone)]
pub struct ArtifactStore {
    root: PathBuf,
}

impl ArtifactStore {
    pub fn open(store_dir: &Path) -> Result<Self, EffectsError> {
        std::fs::create_dir_all(store_dir)?;
        Ok(Self {
            root: store_dir.to_path_buf(),
        })
    }

    pub fn root_dir(&self) -> &Path {
        &self.root
    }

    pub fn path_for(&self, hex: &str) -> PathBuf {
        self.root.join(hex)
    }

    fn hash_bytes(bytes: &[u8]) -> String {
        let mut h = Hasher::new();
        h.update(bytes);
        h.finalize().to_hex().to_string()
    }

    pub fn verify_hex(&self, hex: &str) -> Result<(), EffectsError> {
        let p = self.path_for(hex);
        let bytes = std::fs::read(&p)?;
        let got = Self::hash_bytes(&bytes);
        if got != hex {
            return Err(EffectsError::Log(format!(
                "artifact store corruption: expected hash {hex}, got {got}"
            )));
        }
        Ok(())
    }

    pub fn put_bytes(&self, bytes: &[u8]) -> Result<String, EffectsError> {
        let hex = Self::hash_bytes(bytes);
        let path = self.path_for(&hex);

        if path.exists() {
            self.verify_hex(&hex)?;
            return Ok(hex);
        }

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
                #[cfg(unix)]
                {
                    let dir = std::fs::File::open(&self.root)?;
                    dir.sync_all()?;
                }
            }
            Err(e) if e.kind() == std::io::ErrorKind::AlreadyExists => {
                let _ = std::fs::remove_file(&tmp_path);
                self.verify_hex(&hex)?;
            }
            Err(e) => {
                let _ = std::fs::remove_file(&tmp_path);
                return Err(e.into());
            }
        }

        Ok(hex)
    }

    pub fn get_bytes(&self, hex: &str) -> Result<Vec<u8>, EffectsError> {
        self.verify_hex(hex)?;
        Ok(std::fs::read(self.path_for(hex))?)
    }
}
