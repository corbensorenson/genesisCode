//! A request-owned import overlay. Remote admission never mutates the destination.
use super::ArtifactStore;
use crate::error::EffectsError;
use std::collections::HashMap;
use std::fs::File;
use std::io::{Read, Seek, SeekFrom, Write};

#[derive(Debug, thiserror::Error)]
pub(crate) enum ImportError {
    #[error(transparent)]
    Io(#[from] std::io::Error),
    #[error(transparent)]
    Store(#[from] EffectsError),
    #[error(transparent)]
    Identity(#[from] gc_registry::RegistryError),
    #[error("resource-limit: {0}")]
    ResourceLimit(&'static str),
}

#[derive(Clone, Copy)]
struct Range {
    offset: u64,
    length: usize,
}

pub(crate) struct ArtifactImport<'a> {
    destination: &'a ArtifactStore,
    spool: Option<File>,
    index: HashMap<[u8; 32], Range>,
    order: Vec<[u8; 32]>,
    bytes: usize,
    max_bytes: Option<usize>,
    max_artifact_bytes: usize,
    max_objects: usize,
}

impl<'a> ArtifactImport<'a> {
    pub(crate) fn new(
        destination: &'a ArtifactStore,
        max_artifact_bytes: usize,
        max_bytes: Option<usize>,
        max_objects: usize,
    ) -> Self {
        Self {
            destination,
            spool: None,
            index: HashMap::new(),
            order: Vec::new(),
            bytes: 0,
            max_bytes,
            max_artifact_bytes,
            max_objects,
        }
    }

    fn key(hash: &str) -> Result<[u8; 32], ImportError> {
        gc_registry::validate_store_hash(hash)?;
        blake3::Hash::from_hex(hash)
            .map(|hash| *hash.as_bytes())
            .map_err(|_| ImportError::ResourceLimit("content identity conversion failed"))
    }

    pub(crate) fn staged_bytes(&self) -> usize {
        self.bytes
    }

    pub(crate) fn contains(&self, hash: &str) -> Result<bool, ImportError> {
        Ok(self.index.contains_key(&Self::key(hash)?))
    }

    pub(crate) fn stage(&mut self, hash: &str, bytes: &[u8]) -> Result<(), ImportError> {
        gc_registry::verify_store_object("store/get", hash, bytes)?;
        let key = Self::key(hash)?;
        if self.index.contains_key(&key) {
            return Ok(());
        }
        if bytes.len() > self.max_artifact_bytes {
            return Err(ImportError::ResourceLimit(
                "sync artifact exceeds byte limit",
            ));
        }
        let total = self
            .bytes
            .checked_add(bytes.len())
            .ok_or(ImportError::ResourceLimit(
                "sync import byte accounting overflow",
            ))?;
        if self.max_bytes.is_some_and(|limit| total > limit) {
            return Err(ImportError::ResourceLimit(
                "sync import exceeds remaining store byte budget",
            ));
        }
        if self.index.len() >= self.max_objects {
            return Err(ImportError::ResourceLimit(
                "sync import exceeds object limit",
            ));
        }
        self.index
            .try_reserve(1)
            .map_err(|_| ImportError::ResourceLimit("sync import index allocation failed"))?;
        self.order
            .try_reserve(1)
            .map_err(|_| ImportError::ResourceLimit("sync import order allocation failed"))?;
        if self.spool.is_none() {
            self.spool =
                Some(crate::rooted_fs::FsRoot::open(self.destination.root_dir())?.scratch_file()?);
        }
        let spool = self
            .spool
            .as_mut()
            .ok_or(ImportError::ResourceLimit("sync import spool unavailable"))?;
        let offset = spool.seek(SeekFrom::End(0))?;
        offset
            .checked_add(u64::try_from(bytes.len()).map_err(|_| {
                ImportError::ResourceLimit("sync import offset conversion overflow")
            })?)
            .ok_or(ImportError::ResourceLimit("sync import offset overflow"))?;
        spool.write_all(bytes)?;
        self.index.insert(
            key,
            Range {
                offset,
                length: bytes.len(),
            },
        );
        self.order.push(key);
        self.bytes = total;
        Ok(())
    }

    fn read_range(&mut self, key: [u8; 32], range: Range) -> Result<Vec<u8>, ImportError> {
        let mut bytes = Vec::new();
        bytes
            .try_reserve_exact(range.length)
            .map_err(|_| ImportError::ResourceLimit("sync import read allocation failed"))?;
        bytes.resize(range.length, 0);
        let spool = self
            .spool
            .as_mut()
            .ok_or(ImportError::ResourceLimit("sync import spool unavailable"))?;
        spool.seek(SeekFrom::Start(range.offset))?;
        spool.read_exact(&mut bytes)?;
        let hash = blake3::Hash::from_bytes(key).to_hex();
        gc_registry::verify_store_object("sync/import", hash.as_str(), &bytes)?;
        Ok(bytes)
    }

    pub(crate) fn get_bytes(&mut self, hash: &str) -> Result<Vec<u8>, ImportError> {
        let key = Self::key(hash)?;
        match self.index.get(&key).copied() {
            Some(range) => self.read_range(key, range),
            None => Ok(self.destination.get_bytes(hash)?),
        }
    }

    /// All remote and semantic admission must finish before calling this.
    /// Preflight every spool range before the first destination write. A later
    /// destination I/O failure can retain verified objects; no existing object
    /// is deleted to simulate rollback. Charge each completed logical write.
    pub(crate) fn publish(
        mut self,
        written: &mut usize,
        pulled: &mut u64,
    ) -> Result<(), ImportError> {
        written
            .checked_add(self.bytes)
            .ok_or(ImportError::ResourceLimit(
                "store artifact byte accounting overflow",
            ))?;
        for i in 0..self.order.len() {
            let key = self.order[i];
            let range = *self
                .index
                .get(&key)
                .ok_or(ImportError::ResourceLimit("sync import range unavailable"))?;
            self.read_range(key, range)?;
        }
        for i in 0..self.order.len() {
            let key = self.order[i];
            let range = *self
                .index
                .get(&key)
                .ok_or(ImportError::ResourceLimit("sync import range unavailable"))?;
            let bytes = self.read_range(key, range)?;
            let hash = blake3::Hash::from_bytes(key).to_hex();
            let installed = self.destination.put_bytes(&bytes)?;
            if installed != hash.as_str() {
                return Err(ImportError::ResourceLimit(
                    "store import identity contradiction",
                ));
            }
            *written += range.length;
            *pulled = pulled.saturating_add(1);
        }
        Ok(())
    }
}

#[cfg(test)]
#[path = "import_tests.rs"]
mod tests;
