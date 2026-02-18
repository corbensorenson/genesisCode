use std::collections::BTreeMap;
use std::fs::OpenOptions;
use std::io::Write;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};
use std::time::UNIX_EPOCH;

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
    integrity_cache: Option<Arc<Mutex<IntegrityCache>>>,
}

#[derive(Debug, Clone, Eq, PartialEq)]
struct FileSig {
    len: u64,
    modified_ns: u128,
    #[cfg(unix)]
    dev: u64,
    #[cfg(unix)]
    ino: u64,
    #[cfg(unix)]
    mtime_ns: i128,
    #[cfg(unix)]
    ctime_ns: i128,
}

#[derive(Debug, Default)]
struct IntegrityCache {
    verified: BTreeMap<String, FileSig>,
}

impl ArtifactStore {
    fn env_truthy(name: &str) -> bool {
        fn is_truthy(value: &str) -> bool {
            matches!(
                value.trim().to_ascii_lowercase().as_str(),
                "1" | "true" | "yes" | "on"
            )
        }
        std::env::var(name)
            .ok()
            .map(|v| is_truthy(&v))
            .unwrap_or(false)
    }

    /// Open store with optional integrity cache mode.
    ///
    /// Set `GENESIS_STORE_INTEGRITY_CACHE=1` to enable metadata-validated memoization of
    /// successful hash checks for this process. Cache misses still perform full hash verification.
    pub fn open(store_dir: &Path) -> Result<Self, EffectsError> {
        let use_cache = Self::env_truthy("GENESIS_STORE_INTEGRITY_CACHE");
        Self::open_with_integrity_cache(store_dir, use_cache)
    }

    pub fn open_with_integrity_cache(
        store_dir: &Path,
        enabled: bool,
    ) -> Result<Self, EffectsError> {
        std::fs::create_dir_all(store_dir)?;
        Ok(Self {
            root: store_dir.to_path_buf(),
            integrity_cache: enabled.then(|| Arc::new(Mutex::new(IntegrityCache::default()))),
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

    fn verify_named_bytes(hex: &str, bytes: &[u8]) -> Result<(), EffectsError> {
        let got = Self::hash_bytes(bytes);
        if got != hex {
            return Err(EffectsError::Log(format!(
                "artifact store corruption: expected hash {hex}, got {got}"
            )));
        }
        Ok(())
    }

    fn file_sig(meta: &std::fs::Metadata) -> FileSig {
        #[cfg(unix)]
        {
            use std::os::unix::fs::MetadataExt;
            let modified_ns = meta
                .modified()
                .ok()
                .and_then(|t| t.duration_since(UNIX_EPOCH).ok())
                .map(|d| d.as_nanos())
                .unwrap_or(0);
            let mtime_ns = i128::from(meta.mtime()) * 1_000_000_000 + i128::from(meta.mtime_nsec());
            let ctime_ns = i128::from(meta.ctime()) * 1_000_000_000 + i128::from(meta.ctime_nsec());
            FileSig {
                len: meta.len(),
                modified_ns,
                dev: meta.dev(),
                ino: meta.ino(),
                mtime_ns,
                ctime_ns,
            }
        }
        #[cfg(not(unix))]
        {
            let modified_ns = meta
                .modified()
                .ok()
                .and_then(|t| t.duration_since(UNIX_EPOCH).ok())
                .map(|d| d.as_nanos())
                .unwrap_or(0);
            FileSig {
                len: meta.len(),
                modified_ns,
            }
        }
    }

    fn read_stable(path: &Path) -> Result<(Vec<u8>, FileSig), EffectsError> {
        const STABLE_READ_RETRIES: usize = 3;
        for _ in 0..STABLE_READ_RETRIES {
            let m1 = std::fs::metadata(path)?;
            let s1 = Self::file_sig(&m1);
            let bytes = std::fs::read(path)?;
            let m2 = std::fs::metadata(path)?;
            let s2 = Self::file_sig(&m2);
            if s1 == s2 {
                return Ok((bytes, s2));
            }
        }
        Err(EffectsError::Log(format!(
            "artifact store read instability for {}",
            path.display()
        )))
    }

    fn cache_is_verified(&self, hex: &str, sig: &FileSig) -> bool {
        let Some(cache) = &self.integrity_cache else {
            return false;
        };
        let Ok(guard) = cache.lock() else {
            return false;
        };
        guard.verified.get(hex).is_some_and(|known| known == sig)
    }

    fn cache_mark_verified(&self, hex: &str, sig: FileSig) {
        let Some(cache) = &self.integrity_cache else {
            return;
        };
        let Ok(mut guard) = cache.lock() else {
            return;
        };
        guard.verified.insert(hex.to_string(), sig);
    }

    pub fn verify_hex(&self, hex: &str) -> Result<(), EffectsError> {
        let p = self.path_for(hex);
        if self.integrity_cache.is_some() {
            let sig = Self::file_sig(&std::fs::metadata(&p)?);
            if self.cache_is_verified(hex, &sig) {
                return Ok(());
            }
        }
        let (bytes, sig) = Self::read_stable(&p)?;
        Self::verify_named_bytes(hex, &bytes).map(|_| self.cache_mark_verified(hex, sig))
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
        if self.integrity_cache.is_some() {
            let sig = Self::file_sig(&std::fs::metadata(&path)?);
            self.cache_mark_verified(&hex, sig);
        }

        Ok(hex)
    }

    pub fn get_bytes(&self, hex: &str) -> Result<Vec<u8>, EffectsError> {
        let path = self.path_for(hex);
        let (bytes, sig) = Self::read_stable(&path)?;
        if self.cache_is_verified(hex, &sig) {
            return Ok(bytes);
        }
        Self::verify_named_bytes(hex, &bytes)?;
        self.cache_mark_verified(hex, sig);
        Ok(bytes)
    }
}

#[cfg(test)]
mod tests {
    use super::ArtifactStore;

    #[test]
    fn integrity_cache_mode_detects_replaced_blob_corruption() {
        let td = tempfile::tempdir().expect("tempdir");
        let store = ArtifactStore::open_with_integrity_cache(td.path(), true).expect("open");
        let hex = store.put_bytes(b"alpha").expect("put");
        store.verify_hex(&hex).expect("verify");

        let path = store.path_for(&hex);
        let replacement = td.path().join("replacement");
        std::fs::write(&replacement, b"bravo").expect("write replacement");
        std::fs::rename(&replacement, &path).expect("replace blob");

        let err = store.verify_hex(&hex).unwrap_err();
        assert!(
            format!("{err}").contains("artifact store corruption"),
            "unexpected error: {err}"
        );
    }

    #[test]
    fn integrity_cache_mode_get_bytes_revalidates_on_metadata_change() {
        let td = tempfile::tempdir().expect("tempdir");
        let store = ArtifactStore::open_with_integrity_cache(td.path(), true).expect("open");
        let hex = store.put_bytes(b"abc").expect("put");
        let got = store.get_bytes(&hex).expect("get");
        assert_eq!(got, b"abc");

        let path = store.path_for(&hex);
        let replacement = td.path().join("replacement");
        std::fs::write(&replacement, b"abd").expect("write replacement");
        std::fs::rename(&replacement, &path).expect("replace blob");

        let err = store.get_bytes(&hex).unwrap_err();
        assert!(
            format!("{err}").contains("artifact store corruption"),
            "unexpected error: {err}"
        );
    }
}
