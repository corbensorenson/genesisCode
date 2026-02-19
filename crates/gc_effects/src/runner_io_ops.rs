use std::io::Read;
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
                Term::Str(path_to_slash(base_dir)),
            ),
            (
                TermOrdKey(Term::symbol(":path")),
                Term::Str(path_to_slash(path)),
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
        return Ok(base.clone());
    }
    Ok(std::env::current_dir()?)
}

pub(crate) fn sandbox_path_read(base_dir: &Path, input: &str) -> Result<PathBuf, EffectsError> {
    let candidate = Path::new(input);
    let full = if candidate.is_absolute() {
        candidate.to_path_buf()
    } else {
        base_dir.join(candidate)
    };
    let canon = std::fs::canonicalize(&full)
        .map_err(|e| EffectsError::Log(format!("read path invalid `{}`: {e}", full.display())))?;
    if !canon.starts_with(base_dir) {
        return Err(EffectsError::Log(format!(
            "read path escapes base_dir: {}",
            canon.display()
        )));
    }
    Ok(canon)
}

pub(crate) fn sandbox_path_write(
    base_dir: &Path,
    input: &str,
    create_dirs: bool,
) -> Result<PathBuf, EffectsError> {
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

pub(crate) fn atomic_write_text(path: &Path, bytes: &[u8]) -> Result<(), std::io::Error> {
    let parent = path
        .parent()
        .map(Path::to_path_buf)
        .unwrap_or_else(|| PathBuf::from("."));
    std::fs::create_dir_all(&parent)?;
    let tmp = parent.join(format!(
        ".{}.{}.tmp",
        path.file_name().and_then(|s| s.to_str()).unwrap_or("out"),
        std::process::id()
    ));
    std::fs::write(&tmp, bytes)?;
    std::fs::rename(&tmp, path)?;
    Ok(())
}

pub(crate) fn sandbox_path_allow_missing(
    base_dir: &Path,
    input: &str,
    create_dirs: bool,
) -> Result<PathBuf, EffectsError> {
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
