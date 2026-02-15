use std::collections::BTreeSet;
use std::io::{Read, Write};

use thiserror::Error;

use crate::schema::{bytes32_to_hex, hex_to_bytes32};

const MAGIC: &[u8; 4] = b"GPK\0";
const VERSION: u32 = 1;
const KIND_RAW_CANONICAL: u8 = 0;
const INDEX_ENTRY_BYTES: usize = 32 + 1 + 7 + 8 + 8;

#[derive(Debug, Error)]
pub enum GpkError {
    #[error("gpk: invalid magic")]
    BadMagic,
    #[error("gpk: unsupported version {0}")]
    BadVersion(u32),
    #[error("gpk: truncated or corrupt input")]
    Truncated,
    #[error("gpk: duplicate entry {0}")]
    Duplicate(String),
    #[error("gpk: invalid hash: {0}")]
    Hash(String),
    #[error("gpk: invalid index layout: {0}")]
    BadIndex(String),
    #[error("gpk: io error: {0}")]
    Io(#[from] std::io::Error),
}

#[derive(Debug, Clone)]
pub struct GpkEntry {
    pub hash: [u8; 32],
    pub kind: u8,
    pub bytes: Vec<u8>,
}

#[derive(Debug, Clone)]
pub struct GpkBundle {
    pub version: u32,
    pub root: [u8; 32],
    pub entries: Vec<GpkEntry>,
}

pub fn write_bundle<W: Write>(
    mut w: W,
    root: [u8; 32],
    entries: &[(String, Vec<u8>)],
) -> Result<(), GpkError> {
    w.write_all(MAGIC)?;
    w.write_all(&VERSION.to_le_bytes())?;
    w.write_all(&root)?;

    let mut seen: BTreeSet<String> = BTreeSet::new();
    for (h, _) in entries {
        if !seen.insert(h.clone()) {
            return Err(GpkError::Duplicate(h.clone()));
        }
        crate::schema::validate_hex_hash(h).map_err(GpkError::Hash)?;
    }

    let cnt = entries.len() as u64;
    w.write_all(&cnt.to_le_bytes())?;

    // Index: fixed-size entries so a reader can validate offsets/lengths deterministically.
    let header_len = MAGIC.len() + 4 + 32 + 8;
    let index_len = (INDEX_ENTRY_BYTES as u64)
        .checked_mul(cnt)
        .ok_or_else(|| GpkError::BadIndex("index too large".to_string()))?;
    let payload_start = (header_len as u64)
        .checked_add(index_len)
        .ok_or_else(|| GpkError::BadIndex("payload offset overflow".to_string()))?;

    let mut cur_off = payload_start;
    let mut lens: Vec<u64> = Vec::with_capacity(entries.len());
    for (_, bytes) in entries {
        lens.push(bytes.len() as u64);
    }

    for ((hex, _), len) in entries.iter().zip(lens.iter().copied()) {
        let h = hex_to_bytes32(hex).map_err(GpkError::Hash)?;
        w.write_all(&h)?;
        w.write_all(&[KIND_RAW_CANONICAL])?;
        w.write_all(&[0u8; 7])?; // reserved/padding
        w.write_all(&cur_off.to_le_bytes())?;
        w.write_all(&len.to_le_bytes())?;
        cur_off = cur_off
            .checked_add(len)
            .ok_or_else(|| GpkError::BadIndex("payload length overflow".to_string()))?;
    }

    // Payload.
    for (_, bytes) in entries {
        w.write_all(bytes)?;
    }
    Ok(())
}

pub fn read_bundle<R: Read>(mut r: R) -> Result<GpkBundle, GpkError> {
    let mut magic = [0u8; 4];
    r.read_exact(&mut magic).map_err(|_| GpkError::Truncated)?;
    if &magic != MAGIC {
        return Err(GpkError::BadMagic);
    }
    let mut ver = [0u8; 4];
    r.read_exact(&mut ver).map_err(|_| GpkError::Truncated)?;
    let version = u32::from_le_bytes(ver);
    if version != VERSION {
        return Err(GpkError::BadVersion(version));
    }
    let mut root = [0u8; 32];
    r.read_exact(&mut root).map_err(|_| GpkError::Truncated)?;

    let mut cntb = [0u8; 8];
    r.read_exact(&mut cntb).map_err(|_| GpkError::Truncated)?;
    let cnt = u64::from_le_bytes(cntb);

    let header_len = MAGIC.len() + 4 + 32 + 8;
    let index_len = (INDEX_ENTRY_BYTES as u64)
        .checked_mul(cnt)
        .ok_or_else(|| GpkError::BadIndex("index too large".to_string()))?;
    let payload_start = (header_len as u64)
        .checked_add(index_len)
        .ok_or_else(|| GpkError::BadIndex("payload offset overflow".to_string()))?;

    let mut seen: BTreeSet<String> = BTreeSet::new();
    let mut index: Vec<([u8; 32], u8, u64, u64)> = Vec::with_capacity(cnt as usize);
    for _ in 0..cnt {
        let mut h = [0u8; 32];
        r.read_exact(&mut h).map_err(|_| GpkError::Truncated)?;
        let hex = bytes32_to_hex(&h);
        if !seen.insert(hex.clone()) {
            return Err(GpkError::Duplicate(hex));
        }

        let mut kindb = [0u8; 1];
        r.read_exact(&mut kindb).map_err(|_| GpkError::Truncated)?;
        let kind = kindb[0];
        let mut _pad = [0u8; 7];
        r.read_exact(&mut _pad).map_err(|_| GpkError::Truncated)?;

        let mut offb = [0u8; 8];
        r.read_exact(&mut offb).map_err(|_| GpkError::Truncated)?;
        let off = u64::from_le_bytes(offb);

        let mut lenb = [0u8; 8];
        r.read_exact(&mut lenb).map_err(|_| GpkError::Truncated)?;
        let len = u64::from_le_bytes(lenb);

        index.push((h, kind, off, len));
    }

    let mut entries = Vec::with_capacity(index.len());
    let mut expected_off = payload_start;
    for (h, kind, off, len) in index {
        if off != expected_off {
            return Err(GpkError::BadIndex(format!(
                "non-canonical offset (expected {expected_off}, got {off})"
            )));
        }
        if len > (usize::MAX as u64) {
            return Err(GpkError::Truncated);
        }
        let mut bytes = vec![0u8; len as usize];
        r.read_exact(&mut bytes).map_err(|_| GpkError::Truncated)?;
        expected_off = expected_off
            .checked_add(len)
            .ok_or_else(|| GpkError::BadIndex("payload length overflow".to_string()))?;
        entries.push(GpkEntry {
            hash: h,
            kind,
            bytes,
        });
    }

    // v1 has no extra sections; trailing bytes are treated as corruption.
    let mut extra = [0u8; 1];
    match r.read(&mut extra) {
        Ok(0) => {}
        Ok(_) => return Err(GpkError::BadIndex("trailing bytes".to_string())),
        Err(e) => return Err(GpkError::Io(e)),
    }

    Ok(GpkBundle {
        version,
        root,
        entries,
    })
}
