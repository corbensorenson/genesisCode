use std::io::Read;
#[cfg(unix)]
use std::io::Write;
#[cfg(target_os = "wasi")]
use std::path::Component;
use std::path::{Path, PathBuf};

use gc_coreform::{Term, TermOrdKey};

use crate::EffectsError;
use crate::policy::OpPolicy;
use crate::runner_timeout::TimeoutCancelToken;

#[derive(Debug)]
pub(crate) enum FsReadError {
    Io(std::io::Error),
    LimitExceeded { observed: usize, limit: usize },
    Cancelled,
}

pub(crate) fn read_file_with_optional_limit(
    path: &Path,
    max_bytes: Option<usize>,
    cancel: Option<&TimeoutCancelToken>,
) -> Result<Vec<u8>, FsReadError> {
    let Some(limit) = max_bytes else {
        let mut f = std::fs::File::open(path).map_err(FsReadError::Io)?;
        let mut out = Vec::new();
        let mut buf = [0u8; 8 * 1024];
        loop {
            if cancel.is_some_and(|t| t.is_cancelled()) {
                return Err(FsReadError::Cancelled);
            }
            let n = f.read(&mut buf).map_err(FsReadError::Io)?;
            if n == 0 {
                break;
            }
            out.extend_from_slice(&buf[..n]);
        }
        return Ok(out);
    };
    let mut f = std::fs::File::open(path).map_err(FsReadError::Io)?;
    let mut out = Vec::new();
    let mut buf = [0u8; 8 * 1024];
    loop {
        if cancel.is_some_and(|t| t.is_cancelled()) {
            return Err(FsReadError::Cancelled);
        }
        let n = f.read(&mut buf).map_err(FsReadError::Io)?;
        if n == 0 {
            break;
        }
        out.extend_from_slice(&buf[..n]);
        if out.len() > limit {
            return Err(FsReadError::LimitExceeded {
                observed: out.len(),
                limit,
            });
        }
    }
    Ok(out)
}

pub(crate) fn io_error_payload(op: &str, base_dir: &Path, path: &Path, e: &std::io::Error) -> Term {
    Term::Map(
        [
            (
                TermOrdKey(Term::symbol(":op")),
                Term::Symbol(op.to_string()),
            ),
            (
                TermOrdKey(Term::symbol(":base-dir")),
                Term::Str(".".to_string()),
            ),
            (
                TermOrdKey(Term::symbol(":path")),
                Term::Str(base_relative_error_path(base_dir, path)),
            ),
            (
                TermOrdKey(Term::symbol(":io-kind")),
                Term::Str(format!("{:?}", e.kind())),
            ),
        ]
        .into_iter()
        .collect(),
    )
}

fn base_relative_error_path(base_dir: &Path, path: &Path) -> String {
    match path.strip_prefix(base_dir) {
        Ok(rel) => {
            let rel_s = path_to_slash(rel);
            if rel_s.is_empty() {
                ".".to_string()
            } else {
                rel_s
            }
        }
        Err(_) => "<outside-base>".to_string(),
    }
}

pub(crate) fn path_to_slash(p: &Path) -> String {
    let s = p.to_string_lossy().to_string();
    if cfg!(windows) {
        s.replace('\\', "/")
    } else {
        s
    }
}

pub(crate) fn payload_path(payload: &Term) -> Result<String, EffectsError> {
    let Term::Map(m) = payload else {
        return Err(EffectsError::Log("payload must be a map".to_string()));
    };
    match m.get(&TermOrdKey(Term::symbol(":path"))) {
        Some(Term::Str(s)) => Ok(s.clone()),
        _ => Err(EffectsError::Log(
            "payload missing :path string".to_string(),
        )),
    }
}

pub(crate) fn payload_pkg_path(payload: &Term) -> Result<String, EffectsError> {
    let Term::Map(m) = payload else {
        return Err(EffectsError::Log("payload must be a map".to_string()));
    };
    match m.get(&TermOrdKey(Term::symbol(":pkg"))) {
        Some(Term::Str(s)) => Ok(s.clone()),
        _ => Err(EffectsError::Log("payload missing :pkg string".to_string())),
    }
}

pub(crate) fn effective_base_dir(pol: Option<&OpPolicy>) -> Result<PathBuf, EffectsError> {
    if let Some(pol) = pol
        && let Some(base) = &pol.base_dir
    {
        #[cfg(target_os = "wasi")]
        return lexical_normalize(base);
        #[cfg(not(target_os = "wasi"))]
        return Ok(std::fs::canonicalize(base).unwrap_or_else(|_| base.clone()));
    }
    let cwd = std::env::current_dir()?;
    #[cfg(target_os = "wasi")]
    return lexical_normalize(&cwd);
    #[cfg(not(target_os = "wasi"))]
    Ok(std::fs::canonicalize(&cwd).unwrap_or(cwd))
}

#[cfg(target_os = "wasi")]
fn lexical_normalize(path: &Path) -> Result<PathBuf, EffectsError> {
    let mut normalized = PathBuf::new();
    for component in path.components() {
        match component {
            Component::Prefix(prefix) => normalized.push(prefix.as_os_str()),
            Component::RootDir => normalized.push(component.as_os_str()),
            Component::CurDir => {}
            Component::ParentDir => {
                if !normalized.pop() {
                    return Err(EffectsError::Log(format!(
                        "path escapes lexical root: {}",
                        path.display()
                    )));
                }
            }
            Component::Normal(part) => normalized.push(part),
        }
    }
    Ok(normalized)
}

#[cfg(target_os = "wasi")]
fn wasi_sandbox_path(
    base_dir: &Path,
    input: &str,
    allow_missing: bool,
) -> Result<PathBuf, EffectsError> {
    let base = lexical_normalize(base_dir)?;
    let candidate = Path::new(input);
    let full = if candidate.is_absolute() {
        if !base.is_absolute() {
            return Err(EffectsError::Log(format!(
                "absolute path requires an absolute base_dir: {}",
                candidate.display()
            )));
        }
        lexical_normalize(candidate)?
    } else {
        if candidate
            .components()
            .any(|component| matches!(component, Component::ParentDir))
        {
            return Err(EffectsError::Log(format!(
                "path contains forbidden parent traversal: {}",
                candidate.display()
            )));
        }
        lexical_normalize(&base.join(candidate))?
    };
    if !full.starts_with(&base) {
        return Err(EffectsError::Log(format!(
            "path escapes base_dir: {}",
            full.display()
        )));
    }

    let relative = full
        .strip_prefix(&base)
        .map_err(|_| EffectsError::Log(format!("path escapes base_dir: {}", full.display())))?;
    let mut current = base.clone();
    for component in relative.components() {
        current.push(component.as_os_str());
        match std::fs::symlink_metadata(&current) {
            Ok(metadata) if metadata.file_type().is_symlink() => {
                return Err(EffectsError::Log(format!(
                    "path traverses a forbidden symlink: {}",
                    current.display()
                )));
            }
            Ok(_) => {}
            Err(error) if allow_missing && error.kind() == std::io::ErrorKind::NotFound => break,
            Err(error) => {
                return Err(EffectsError::Log(format!(
                    "path metadata invalid `{}`: {error}",
                    current.display()
                )));
            }
        }
    }
    Ok(full)
}

pub(crate) fn sandbox_path_read(base_dir: &Path, input: &str) -> Result<PathBuf, EffectsError> {
    #[cfg(target_os = "wasi")]
    return wasi_sandbox_path(base_dir, input, false);

    #[cfg(not(target_os = "wasi"))]
    {
        let candidate = Path::new(input);
        let full = if candidate.is_absolute() {
            candidate.to_path_buf()
        } else {
            base_dir.join(candidate)
        };
        let canon = std::fs::canonicalize(&full).map_err(|e| {
            EffectsError::Log(format!("read path invalid `{}`: {e}", full.display()))
        })?;
        if !canon.starts_with(base_dir) {
            return Err(EffectsError::Log(format!(
                "read path escapes base_dir: {}",
                canon.display()
            )));
        }
        Ok(canon)
    }
}

pub(crate) fn sandbox_path_write(
    base_dir: &Path,
    input: &str,
    create_dirs: bool,
) -> Result<PathBuf, EffectsError> {
    #[cfg(target_os = "wasi")]
    {
        let joined = wasi_sandbox_path(base_dir, input, true)?;
        if let Some(parent) = joined.parent()
            && create_dirs
        {
            std::fs::create_dir_all(parent).map_err(|e| {
                EffectsError::Log(format!("create dir `{}` failed: {e}", parent.display()))
            })?;
            wasi_sandbox_path(base_dir, input, true)?;
        }
        return Ok(joined);
    }

    #[cfg(not(target_os = "wasi"))]
    {
        let candidate = Path::new(input);
        let joined = if candidate.is_absolute() {
            candidate.to_path_buf()
        } else {
            base_dir.join(candidate)
        };

        if let Some(parent) = joined.parent() {
            if create_dirs {
                std::fs::create_dir_all(parent).map_err(|e| {
                    EffectsError::Log(format!("create dir `{}` failed: {e}", parent.display()))
                })?;
            }
            let parent_canon = std::fs::canonicalize(parent).map_err(|e| {
                EffectsError::Log(format!("write parent invalid `{}`: {e}", parent.display()))
            })?;
            if !parent_canon.starts_with(base_dir) {
                return Err(EffectsError::Log(format!(
                    "write path escapes base_dir: {}",
                    parent_canon.display()
                )));
            }
        }
        Ok(joined)
    }
}

pub(crate) fn atomic_write_text(path: &Path, bytes: &[u8]) -> Result<(), std::io::Error> {
    let parent = path
        .parent()
        .map(Path::to_path_buf)
        .unwrap_or_else(|| PathBuf::from("."));
    std::fs::create_dir_all(&parent)?;
    let mut sequence = 0u64;
    let tmp = loop {
        let candidate = parent.join(format!(
            ".{}.{}.{}.tmp",
            path.file_name().and_then(|s| s.to_str()).unwrap_or("out"),
            crate::platform_process_id(),
            sequence
        ));
        sequence = sequence.saturating_add(1);
        match std::fs::OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(&candidate)
        {
            Ok(mut file) => {
                use std::io::Write as _;
                file.write_all(bytes)?;
                break candidate;
            }
            Err(error) if error.kind() == std::io::ErrorKind::AlreadyExists => continue,
            Err(error) => return Err(error),
        }
    };
    std::fs::rename(&tmp, path)?;
    Ok(())
}

pub(crate) fn write_file_no_follow(path: &Path, bytes: &[u8]) -> Result<(), std::io::Error> {
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;

        let mut f = std::fs::OpenOptions::new()
            .write(true)
            .create(true)
            .truncate(true)
            .custom_flags(libc::O_NOFOLLOW)
            .open(path)?;
        f.write_all(bytes)?;
        f.sync_all()?;
        Ok(())
    }
    #[cfg(not(unix))]
    {
        if path.exists() {
            let md = std::fs::symlink_metadata(path)?;
            if md.file_type().is_symlink() {
                return Err(std::io::Error::other("refusing to write through symlink"));
            }
        }
        std::fs::write(path, bytes)
    }
}

pub(crate) fn sandbox_path_allow_missing(
    base_dir: &Path,
    input: &str,
    create_dirs: bool,
) -> Result<PathBuf, EffectsError> {
    #[cfg(target_os = "wasi")]
    {
        let full = wasi_sandbox_path(base_dir, input, true)?;
        if let Some(parent) = full.parent()
            && create_dirs
        {
            std::fs::create_dir_all(parent).map_err(|e| {
                EffectsError::Log(format!("create dir `{}` failed: {e}", parent.display()))
            })?;
            wasi_sandbox_path(base_dir, input, true)?;
        }
        return Ok(full);
    }

    #[cfg(not(target_os = "wasi"))]
    {
        let candidate = Path::new(input);
        let full = if candidate.is_absolute() {
            candidate.to_path_buf()
        } else {
            base_dir.join(candidate)
        };
        if full.exists() {
            return sandbox_path_read(base_dir, &full.to_string_lossy());
        }
        if let Some(parent) = full.parent() {
            if create_dirs {
                std::fs::create_dir_all(parent).map_err(|e| {
                    EffectsError::Log(format!("create dir `{}` failed: {e}", parent.display()))
                })?;
            }
            let canon_parent = std::fs::canonicalize(parent).map_err(|e| {
                EffectsError::Log(format!("path parent invalid `{}`: {e}", parent.display()))
            })?;
            if !canon_parent.starts_with(base_dir) {
                return Err(EffectsError::Log(format!(
                    "path escapes base_dir: {}",
                    canon_parent.display()
                )));
            }
        }
        Ok(full)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn map_value_str<'a>(payload: &'a Term, key: &str) -> Option<&'a str> {
        let Term::Map(m) = payload else {
            return None;
        };
        match m.get(&TermOrdKey(Term::symbol(key))) {
            Some(Term::Str(s)) => Some(s.as_str()),
            _ => None,
        }
    }

    #[test]
    fn io_error_payload_uses_base_relative_path() {
        let base_dir = PathBuf::from("workspace");
        let path = base_dir.join("nested/file.txt");
        let io_err = std::io::Error::new(std::io::ErrorKind::NotFound, "missing");
        let payload = io_error_payload("io/fs::read", &base_dir, &path, &io_err);
        assert_eq!(map_value_str(&payload, ":base-dir"), Some("."));
        assert_eq!(map_value_str(&payload, ":path"), Some("nested/file.txt"));
    }

    #[test]
    fn io_error_payload_sanitizes_outside_path() {
        let base_dir = PathBuf::from("workspace");
        let path = PathBuf::from("other/place.txt");
        let io_err = std::io::Error::new(std::io::ErrorKind::PermissionDenied, "denied");
        let payload = io_error_payload("io/fs::read", &base_dir, &path, &io_err);
        assert_eq!(map_value_str(&payload, ":base-dir"), Some("."));
        assert_eq!(map_value_str(&payload, ":path"), Some("<outside-base>"));
    }
}
