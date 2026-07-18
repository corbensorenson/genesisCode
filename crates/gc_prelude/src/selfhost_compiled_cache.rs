use std::collections::BTreeMap;
use std::path::PathBuf;
use std::sync::{Mutex, MutexGuard};

use gc_kernel::{decode_compiled_module_blob, encode_compiled_module_blob};

use super::{
    CachedCompiledModules, DEFAULT_SELFHOST_COMPILED_CACHE_REL, SELFHOST_COMPILED_CACHE_DIR_ENV,
    SELFHOST_COMPILED_CACHE_DISABLE_ENV, SELFHOST_COMPILED_CACHE_FILE_MAGIC, ToolchainManifest,
    env_truthy,
};

pub(super) fn lock_artifact_compiled_cache<'a>(
    cache: &'a Mutex<BTreeMap<[u8; 32], CachedCompiledModules>>,
) -> anyhow::Result<MutexGuard<'a, BTreeMap<[u8; 32], CachedCompiledModules>>> {
    cache
        .lock()
        .map_err(|_| anyhow::anyhow!("artifact cache lock poisoned"))
}

fn hex32(h: [u8; 32]) -> String {
    const HEX: &[u8; 16] = b"0123456789abcdef";
    let mut out = String::with_capacity(64);
    for b in h {
        out.push(HEX[(b >> 4) as usize] as char);
        out.push(HEX[(b & 0x0f) as usize] as char);
    }
    out
}

fn push_u32(out: &mut Vec<u8>, n: usize) -> anyhow::Result<()> {
    let n = u32::try_from(n).map_err(|_| anyhow::anyhow!("cache field exceeds u32 range"))?;
    out.extend_from_slice(&n.to_le_bytes());
    Ok(())
}

fn push_bytes(out: &mut Vec<u8>, bytes: &[u8]) -> anyhow::Result<()> {
    push_u32(out, bytes.len())?;
    out.extend_from_slice(bytes);
    Ok(())
}

fn push_str(out: &mut Vec<u8>, s: &str) -> anyhow::Result<()> {
    push_bytes(out, s.as_bytes())
}

struct CacheDecodeCursor<'a> {
    bytes: &'a [u8],
    at: usize,
}

impl<'a> CacheDecodeCursor<'a> {
    fn new(bytes: &'a [u8]) -> Self {
        Self { bytes, at: 0 }
    }

    fn remaining(&self) -> usize {
        self.bytes.len().saturating_sub(self.at)
    }

    fn read_exact(&mut self, n: usize) -> anyhow::Result<&'a [u8]> {
        if self.remaining() < n {
            return Err(anyhow::anyhow!("compiled cache truncated"));
        }
        let start = self.at;
        self.at += n;
        Ok(&self.bytes[start..start + n])
    }

    fn read_u32(&mut self) -> anyhow::Result<u32> {
        let mut buf = [0u8; 4];
        buf.copy_from_slice(self.read_exact(4)?);
        Ok(u32::from_le_bytes(buf))
    }

    fn read_bytes(&mut self) -> anyhow::Result<&'a [u8]> {
        let n = self.read_u32()? as usize;
        self.read_exact(n)
    }

    fn read_str(&mut self) -> anyhow::Result<String> {
        let bytes = self.read_bytes()?;
        let s = std::str::from_utf8(bytes).map_err(|e| anyhow::anyhow!("invalid utf-8: {e}"))?;
        Ok(s.to_string())
    }
}

fn resolve_compiled_cache_dir() -> Option<PathBuf> {
    if env_truthy(SELFHOST_COMPILED_CACHE_DISABLE_ENV) {
        return None;
    }
    if let Ok(raw) = std::env::var(SELFHOST_COMPILED_CACHE_DIR_ENV) {
        let trimmed = raw.trim();
        if !trimmed.is_empty() {
            return Some(PathBuf::from(trimmed));
        }
    }
    Some(
        std::env::current_dir()
            .unwrap_or_else(|_| PathBuf::from("."))
            .join(DEFAULT_SELFHOST_COMPILED_CACHE_REL),
    )
}

fn compiled_cache_file_path(artifact_h: [u8; 32]) -> Option<PathBuf> {
    let dir = resolve_compiled_cache_dir()?;
    Some(dir.join(format!("{}.bin", hex32(artifact_h))))
}

pub(super) fn decode_compiled_cache_blob(
    bytes: &[u8],
    expected_artifact_h: [u8; 32],
    manifest: &ToolchainManifest,
) -> anyhow::Result<CachedCompiledModules> {
    let mut cur = CacheDecodeCursor::new(bytes);
    let magic = cur.read_exact(SELFHOST_COMPILED_CACHE_FILE_MAGIC.len())?;
    if magic != SELFHOST_COMPILED_CACHE_FILE_MAGIC {
        return Err(anyhow::anyhow!("compiled cache magic mismatch"));
    }
    let got_h = cur.read_exact(32)?;
    if got_h != expected_artifact_h {
        return Err(anyhow::anyhow!("compiled cache artifact hash mismatch"));
    }
    let count = cur.read_u32()? as usize;
    if count != manifest.module_paths.len() {
        return Err(anyhow::anyhow!(
            "compiled cache module count mismatch: expected {}, got {}",
            manifest.module_paths.len(),
            count
        ));
    }

    let mut out = Vec::with_capacity(count);
    for expected_path in &manifest.module_paths {
        let path = cur.read_str()?;
        if &path != expected_path {
            return Err(anyhow::anyhow!(
                "compiled cache path order mismatch: expected {}, got {}",
                expected_path,
                path
            ));
        }
        let blob = cur.read_bytes()?;
        let module = decode_compiled_module_blob(blob)
            .map_err(|e| anyhow::anyhow!("decode compiled module {} failed: {}", path, e))?;
        out.push((path, module));
    }
    if cur.remaining() != 0 {
        return Err(anyhow::anyhow!("compiled cache has trailing bytes"));
    }
    Ok(out)
}

pub(super) fn encode_compiled_cache_blob(
    artifact_h: [u8; 32],
    modules: &CachedCompiledModules,
) -> anyhow::Result<Vec<u8>> {
    let mut out = Vec::new();
    out.extend_from_slice(SELFHOST_COMPILED_CACHE_FILE_MAGIC);
    out.extend_from_slice(&artifact_h);
    push_u32(&mut out, modules.len())?;
    for (path, module) in modules {
        push_str(&mut out, path)?;
        let blob = encode_compiled_module_blob(module)
            .map_err(|e| anyhow::anyhow!("encode compiled module {} failed: {}", path, e))?;
        push_bytes(&mut out, &blob)?;
    }
    Ok(out)
}

pub(super) fn try_read_compiled_cache(
    artifact_h: [u8; 32],
    manifest: &ToolchainManifest,
) -> Option<CachedCompiledModules> {
    let path = compiled_cache_file_path(artifact_h)?;
    let bytes = match std::fs::read(&path) {
        Ok(b) => b,
        Err(_) => return None,
    };
    match decode_compiled_cache_blob(&bytes, artifact_h, manifest) {
        Ok(mods) => Some(mods),
        Err(_) => {
            let _ = std::fs::remove_file(&path);
            None
        }
    }
}

pub(super) fn write_compiled_cache(
    artifact_h: [u8; 32],
    modules: &CachedCompiledModules,
) -> anyhow::Result<()> {
    let Some(path) = compiled_cache_file_path(artifact_h) else {
        return Ok(());
    };
    let Some(dir) = path.parent() else {
        return Ok(());
    };
    std::fs::create_dir_all(dir)?;
    let bytes = encode_compiled_cache_blob(artifact_h, modules)?;

    let mut i: u64 = 0;
    let tmp = loop {
        let cand = dir.join(format!(
            ".tmp-{}-{}-{}",
            hex32(artifact_h),
            cache_process_id(),
            i
        ));
        i = i.saturating_add(1);
        match std::fs::OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(&cand)
        {
            Ok(mut f) => {
                use std::io::Write;
                f.write_all(&bytes)?;
                let _ = f.sync_all();
                break cand;
            }
            Err(e) if e.kind() == std::io::ErrorKind::AlreadyExists => continue,
            Err(e) => return Err(e.into()),
        }
    };
    std::fs::rename(&tmp, &path)?;
    #[cfg(unix)]
    {
        let d = std::fs::File::open(dir)?;
        let _ = d.sync_all();
    }
    Ok(())
}

#[cfg(not(target_os = "wasi"))]
fn cache_process_id() -> u32 {
    std::process::id()
}

#[cfg(target_os = "wasi")]
fn cache_process_id() -> u32 {
    0
}

#[cfg(test)]
mod tests {
    use gc_coreform::Term;
    use gc_kernel::compile_module;

    use super::{ToolchainManifest, decode_compiled_cache_blob, encode_compiled_cache_blob};

    #[test]
    fn cache_writer_emits_current_magic() {
        let forms: Vec<Term> = Vec::new();
        let modules = vec![("empty.gc".to_string(), compile_module(&forms).unwrap())];
        let bytes = encode_compiled_cache_blob([9; 32], &modules).unwrap();
        assert!(bytes.starts_with(b"GCSHC1\0"));

        let manifest = ToolchainManifest {
            module_paths: vec!["empty.gc".to_string()],
            required_symbols: Vec::new(),
        };
        let mut obsolete = bytes;
        obsolete[..7].copy_from_slice(b"GCSHC0\0");
        let error = decode_compiled_cache_blob(&obsolete, [9; 32], &manifest)
            .unwrap_err()
            .to_string();
        assert!(error.contains("magic mismatch"), "{error}");
    }
}
