use std::collections::BTreeMap;
use std::fs::OpenOptions;
use std::io::{Read, Write};
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

#[derive(Debug)]
pub(crate) enum ArtifactObservation {
    Missing,
    TooLarge { observed: usize },
    Bytes(Vec<u8>),
}

#[derive(Debug)]
pub(crate) struct StoreInventoryEntry {
    pub(crate) kind: &'static str,
    pub(crate) name: Vec<u8>,
}

#[derive(Debug)]
pub(crate) enum StoreInventoryObservation {
    Entries(Vec<StoreInventoryEntry>),
    ResourceLimit,
}

#[derive(Debug)]
pub(crate) enum ArtifactHashObservation {
    Missing,
    TooLarge,
    Hash { bytes: usize, hash: String },
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
        Err(EffectsError::Log(
            "artifact store read instability".to_string(),
        ))
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
            let cand = self.root.join(format!(
                ".tmp-{}-{}-{}",
                hex,
                crate::platform_process_id(),
                tmp_i
            ));
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

    pub fn get_bytes_limited(&self, hex: &str, max_bytes: usize) -> Result<Vec<u8>, EffectsError> {
        let path = self.path_for(hex);
        let mut f = std::fs::File::open(&path)?;
        let mut out = Vec::new();
        let mut buf = [0u8; 8 * 1024];
        loop {
            let n = f.read(&mut buf)?;
            if n == 0 {
                break;
            }
            if out.len().saturating_add(n) > max_bytes {
                return Err(EffectsError::Log(format!(
                    "artifact size exceeds configured max bytes ({max_bytes}) for {hex}"
                )));
            }
            out.extend_from_slice(&buf[..n]);
        }
        Self::verify_named_bytes(hex, &out)?;
        if self.integrity_cache.is_some() {
            let sig = Self::file_sig(&std::fs::metadata(&path)?);
            self.cache_mark_verified(hex, sig);
        }
        Ok(out)
    }

    pub(crate) fn observe_bytes_limited(
        &self,
        hex: &str,
        max_bytes: usize,
    ) -> Result<ArtifactObservation, EffectsError> {
        const STABLE_READ_RETRIES: usize = 3;
        let path = self.path_for(hex);
        for _ in 0..STABLE_READ_RETRIES {
            let mut file = match std::fs::File::open(&path) {
                Ok(file) => file,
                Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
                    return Ok(ArtifactObservation::Missing);
                }
                Err(error) => return Err(error.into()),
            };
            let before = Self::file_sig(&file.metadata()?);
            if before.len > max_bytes as u64 {
                return Ok(ArtifactObservation::TooLarge {
                    observed: usize::try_from(before.len).unwrap_or(usize::MAX),
                });
            }
            let mut bytes = Vec::with_capacity(before.len as usize);
            let mut chunk = [0_u8; 8 * 1024];
            loop {
                let count = file.read(&mut chunk)?;
                if count == 0 {
                    break;
                }
                let observed = bytes.len().saturating_add(count);
                if observed > max_bytes {
                    return Ok(ArtifactObservation::TooLarge { observed });
                }
                bytes.extend_from_slice(&chunk[..count]);
            }
            let after = Self::file_sig(&file.metadata()?);
            if before == after {
                return Ok(ArtifactObservation::Bytes(bytes));
            }
        }
        Err(EffectsError::Log(
            "artifact store read instability".to_string(),
        ))
    }

    pub(crate) fn observe_inventory(
        &self,
        max_entries: usize,
        max_name_bytes: usize,
    ) -> Result<StoreInventoryObservation, EffectsError> {
        let mut entries = Vec::new();
        let mut name_bytes = 0_usize;
        for entry in std::fs::read_dir(&self.root)? {
            let entry = entry?;
            if entries.len() >= max_entries {
                return Ok(StoreInventoryObservation::ResourceLimit);
            }
            let name = store_entry_name_bytes(&entry.file_name())?;
            name_bytes = name_bytes.saturating_add(name.len());
            if name_bytes > max_name_bytes {
                return Ok(StoreInventoryObservation::ResourceLimit);
            }
            let file_type = entry.file_type()?;
            let kind = if file_type.is_file() {
                ":file"
            } else if file_type.is_dir() {
                ":directory"
            } else if file_type.is_symlink() {
                ":symlink"
            } else {
                ":other"
            };
            entries.push(StoreInventoryEntry { kind, name });
        }
        entries.sort_by(|left, right| left.name.cmp(&right.name));
        Ok(StoreInventoryObservation::Entries(entries))
    }

    pub(crate) fn observe_hash_limited(
        &self,
        hex: &str,
        max_bytes: usize,
    ) -> Result<ArtifactHashObservation, EffectsError> {
        const STABLE_READ_RETRIES: usize = 3;
        let path = self.path_for(hex);
        for _ in 0..STABLE_READ_RETRIES {
            let mut file = match std::fs::File::open(&path) {
                Ok(file) => file,
                Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
                    return Ok(ArtifactHashObservation::Missing);
                }
                Err(error) => return Err(error.into()),
            };
            let before = Self::file_sig(&file.metadata()?);
            if before.len > max_bytes as u64 {
                return Ok(ArtifactHashObservation::TooLarge);
            }
            let mut observed = 0_usize;
            let mut hasher = Hasher::new();
            let mut chunk = [0_u8; 8 * 1024];
            loop {
                let count = file.read(&mut chunk)?;
                if count == 0 {
                    break;
                }
                observed = observed.saturating_add(count);
                if observed > max_bytes {
                    return Ok(ArtifactHashObservation::TooLarge);
                }
                hasher.update(&chunk[..count]);
            }
            let after = Self::file_sig(&file.metadata()?);
            if before == after {
                return Ok(ArtifactHashObservation::Hash {
                    bytes: observed,
                    hash: hasher.finalize().to_hex().to_string(),
                });
            }
        }
        Err(EffectsError::Log(
            "artifact store read instability".to_string(),
        ))
    }
}

#[cfg(unix)]
fn store_entry_name_bytes(name: &std::ffi::OsStr) -> Result<Vec<u8>, EffectsError> {
    use std::os::unix::ffi::OsStrExt;
    Ok(name.as_bytes().to_vec())
}

#[cfg(not(unix))]
fn store_entry_name_bytes(name: &std::ffi::OsStr) -> Result<Vec<u8>, EffectsError> {
    name.to_str()
        .map(|value| value.as_bytes().to_vec())
        .ok_or_else(|| EffectsError::Log("artifact store entry name is not Unicode".to_string()))
}

#[cfg(test)]
mod tests {
    use super::{ArtifactHashObservation, ArtifactStore, StoreInventoryObservation};

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

    #[test]
    fn verify_observations_are_sorted_classified_and_bounded() {
        let td = tempfile::tempdir().expect("tempdir");
        let store = ArtifactStore::open(td.path()).expect("open");
        std::fs::write(td.path().join("z-file"), b"z").expect("write file");
        std::fs::create_dir(td.path().join("a-dir")).expect("create directory");

        let StoreInventoryObservation::Entries(entries) =
            store.observe_inventory(2, 32).expect("observe inventory")
        else {
            panic!("expected bounded inventory");
        };
        assert_eq!(entries[0].name, b"a-dir");
        assert_eq!(entries[0].kind, ":directory");
        assert_eq!(entries[1].name, b"z-file");
        assert_eq!(entries[1].kind, ":file");
        assert!(matches!(
            store.observe_inventory(1, 32).expect("entry bound"),
            StoreInventoryObservation::ResourceLimit
        ));
        assert!(matches!(
            store.observe_inventory(2, 4).expect("name-byte bound"),
            StoreInventoryObservation::ResourceLimit
        ));
    }

    #[test]
    fn verify_hash_observation_streams_identity_under_limit() {
        let td = tempfile::tempdir().expect("tempdir");
        let store = ArtifactStore::open(td.path()).expect("open");
        let bytes = b"streamed artifact";
        let hash = blake3::hash(bytes).to_hex().to_string();
        std::fs::write(td.path().join(&hash), bytes).expect("write artifact");

        assert!(matches!(
            store
                .observe_hash_limited(&hash, bytes.len() - 1)
                .expect("bounded hash"),
            ArtifactHashObservation::TooLarge
        ));
        match store
            .observe_hash_limited(&hash, bytes.len())
            .expect("hash observation")
        {
            ArtifactHashObservation::Hash {
                bytes: observed,
                hash: observed_hash,
            } => {
                assert_eq!(observed, bytes.len());
                assert_eq!(observed_hash, hash);
            }
            other => panic!("unexpected observation: {other:?}"),
        }
    }
}
