use std::io::Read;
#[cfg(target_os = "wasi")]
use std::path::Component;
use std::path::{Path, PathBuf};

use gc_coreform::{Term, TermOrdKey};
use gc_kernel::text_profile::normalize_nfc;

use crate::EffectsError;
use crate::policy::OpPolicy;
use crate::runner_timeout::TimeoutCancelToken;

#[derive(Debug)]
pub(crate) enum FsReadError {
    Io(std::io::Error),
    LimitExceeded { observed: usize, limit: usize },
    Cancelled,
}

pub(crate) fn read_open_file_with_optional_limit(
    mut file: std::fs::File,
    max_bytes: Option<usize>,
    cancel: Option<&TimeoutCancelToken>,
) -> Result<Vec<u8>, FsReadError> {
    let Some(limit) = max_bytes else {
        let mut out = Vec::new();
        let mut buf = [0u8; 8 * 1024];
        loop {
            if cancel.is_some_and(|t| t.is_cancelled()) {
                return Err(FsReadError::Cancelled);
            }
            let n = file.read(&mut buf).map_err(FsReadError::Io)?;
            if n == 0 {
                break;
            }
            out.extend_from_slice(&buf[..n]);
        }
        return Ok(out);
    };
    let mut out = Vec::new();
    let mut buf = [0u8; 8 * 1024];
    loop {
        if cancel.is_some_and(|t| t.is_cancelled()) {
            return Err(FsReadError::Cancelled);
        }
        let n = file.read(&mut buf).map_err(FsReadError::Io)?;
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

pub(crate) fn base_relative_error_path(base_dir: &Path, path: &Path) -> String {
    match path.strip_prefix(base_dir) {
        Ok(rel) => canonical_path_material(rel).unwrap_or_else(|| "<invalid-path>".to_string()),
        Err(_) => "<outside-base>".to_string(),
    }
}

pub(crate) fn canonical_path_material(path: &Path) -> Option<String> {
    let raw = path.to_str()?;
    let slash = if cfg!(windows) {
        raw.replace('\\', "/")
    } else {
        if raw.contains('\\') {
            return None;
        }
        raw.to_string()
    };
    let material = if slash.is_empty() {
        ".".to_string()
    } else {
        slash
    };
    normalize_nfc(&material).ok()
}

pub(crate) fn validate_portable_effect_path(path: &str) -> Result<(), EffectsError> {
    if path == "." {
        return Ok(());
    }
    if path.is_empty()
        || path.starts_with('/')
        || path.starts_with('\\')
        || path.contains('\\')
        || path.as_bytes().get(1) == Some(&b':')
    {
        return Err(EffectsError::Log(
            "filesystem path must be base-relative and use `/` separators".to_string(),
        ));
    }
    if path
        .split('/')
        .any(|component| component.is_empty() || component == "." || component == "..")
    {
        return Err(EffectsError::Log(
            "filesystem path contains an empty, `.` or `..` component".to_string(),
        ));
    }
    let normalized = normalize_nfc(path).map_err(|error| {
        EffectsError::Log(format!("filesystem path normalization failed: {error}"))
    })?;
    if normalized != path {
        return Err(EffectsError::Log(
            "filesystem path must use Unicode 17 NFC".to_string(),
        ));
    }
    Ok(())
}

pub(crate) fn path_to_slash(p: &Path) -> String {
    let Some(raw) = p.to_str() else {
        return "<invalid-path>".to_string();
    };
    let slash = if cfg!(windows) {
        raw.replace('\\', "/")
    } else {
        raw.to_string()
    };
    normalize_nfc(&slash).unwrap_or_else(|_| "<invalid-path>".to_string())
}

pub(crate) fn payload_path(payload: &Term) -> Result<String, EffectsError> {
    let Term::Map(m) = payload else {
        return Err(EffectsError::Log("payload must be a map".to_string()));
    };
    match m.get(&TermOrdKey(Term::symbol(":path"))) {
        Some(Term::Str(s)) => {
            validate_portable_effect_path(s)?;
            Ok(s.clone())
        }
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
        Ok(crate::rooted_fs::FsRoot::open(base_dir)?.legacy_path(input, true, false, false)?)
    }
}

/// Authority is the held file, never the diagnostic input name. Readers cannot
/// convert this grant back into an ambient pathname to reopen it.
pub(crate) struct DocumentRead {
    file: std::fs::File,
    #[cfg(any(test, feature = "parity-oracle"))]
    description: String,
}

impl DocumentRead {
    /// Consume the admitted handle. A bounded read probes at most one byte
    /// beyond the limit and grows its result only with fallible allocation.
    pub(crate) fn read_bytes(self, max_bytes: Option<usize>) -> Result<Vec<u8>, FsReadError> {
        let mut file = self.file;
        let mut out = Vec::new();
        let mut buffer = [0u8; 8 * 1024];
        loop {
            let requested = max_bytes.map_or(buffer.len(), |limit| {
                limit
                    .saturating_sub(out.len())
                    .saturating_add(1)
                    .min(buffer.len())
            });
            let read = file
                .read(&mut buffer[..requested])
                .map_err(FsReadError::Io)?;
            if read == 0 {
                return Ok(out);
            }
            let observed = out.len().checked_add(read).ok_or_else(|| {
                FsReadError::Io(std::io::Error::new(
                    std::io::ErrorKind::InvalidData,
                    "document size overflow",
                ))
            })?;
            if let Some(limit) = max_bytes
                && observed > limit
            {
                return Err(FsReadError::LimitExceeded { observed, limit });
            }
            out.try_reserve(read).map_err(|_| {
                FsReadError::Io(std::io::Error::new(
                    std::io::ErrorKind::OutOfMemory,
                    "document allocation failed",
                ))
            })?;
            out.extend_from_slice(&buffer[..read]);
        }
    }

    pub(crate) fn reader(&self) -> &std::fs::File {
        &self.file
    }

    #[cfg(any(test, feature = "parity-oracle"))]
    pub(crate) fn description(&self) -> &str {
        &self.description
    }
}

pub(crate) fn sandbox_document_read(
    base_dir: &Path,
    input: &str,
) -> Result<DocumentRead, EffectsError> {
    sandbox_optional_document_read(base_dir, input)?.ok_or_else(|| {
        std::io::Error::new(
            std::io::ErrorKind::NotFound,
            "document source does not exist",
        )
        .into()
    })
}

pub(crate) fn sandbox_optional_document_read(
    base_dir: &Path,
    input: &str,
) -> Result<Option<DocumentRead>, EffectsError> {
    // Root admission is never optional. Only absence below the opened root is.
    let root = crate::rooted_fs::FsRoot::open(base_dir)?;
    let file = match root.open_document_read(input) {
        Ok(file) => file,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(None),
        Err(error) => return Err(error.into()),
    };
    Ok(Some(DocumentRead {
        file,
        #[cfg(any(test, feature = "parity-oracle"))]
        description: input.to_owned(),
    }))
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
        Ok(crate::rooted_fs::FsRoot::open(base_dir)?.legacy_path(
            input,
            false,
            true,
            create_dirs,
        )?)
    }
}

pub(crate) fn sandbox_atomic_write_target(
    base_dir: &Path,
    input: &str,
    create_dirs: bool,
) -> Result<crate::rooted_fs::AtomicWriteTarget, EffectsError> {
    Ok(crate::rooted_fs::FsRoot::open(base_dir)?.prepare_atomic_write(input, create_dirs)?)
}

pub(crate) fn atomic_write_text(
    target: &crate::rooted_fs::AtomicWriteTarget,
    bytes: &[u8],
) -> Result<(), std::io::Error> {
    target.write(bytes)
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
        Ok(
            crate::rooted_fs::FsRoot::open(base_dir)?.legacy_path(
                input,
                true,
                true,
                create_dirs,
            )?,
        )
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn document_read_bounds_probe_and_accepts_exact_empty_and_legacy_inputs() {
        let fixture = tempfile::tempdir().unwrap();
        let path = fixture.path().join("document");
        std::fs::write(&path, b"1234").unwrap();
        assert_eq!(
            sandbox_document_read(fixture.path(), "document")
                .unwrap()
                .read_bytes(Some(4))
                .unwrap(),
            b"1234"
        );
        let error = sandbox_document_read(fixture.path(), "document")
            .unwrap()
            .read_bytes(Some(2))
            .unwrap_err();
        assert!(matches!(
            error,
            FsReadError::LimitExceeded {
                observed: 3,
                limit: 2
            }
        ));
        assert!(matches!(
            sandbox_document_read(fixture.path(), "document")
                .unwrap()
                .read_bytes(Some(0)),
            Err(FsReadError::LimitExceeded {
                observed: 1,
                limit: 0
            })
        ));
        assert_eq!(
            sandbox_document_read(fixture.path(), path.to_str().unwrap())
                .unwrap()
                .read_bytes(None)
                .unwrap(),
            b"1234"
        );
        std::fs::File::create(&path).unwrap();
        assert!(
            sandbox_document_read(fixture.path(), "document")
                .unwrap()
                .read_bytes(Some(0))
                .unwrap()
                .is_empty()
        );
        std::fs::File::create(&path)
            .unwrap()
            .set_len(64 * 1024 * 1024)
            .unwrap();
        assert!(matches!(
            sandbox_document_read(fixture.path(), "document")
                .unwrap()
                .read_bytes(Some(32)),
            Err(FsReadError::LimitExceeded {
                observed: 33,
                limit: 32
            })
        ));
    }

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

    #[test]
    fn portable_effect_paths_are_relative_slash_nfc_and_case_exact() {
        for accepted in [".", "src/Main.gc", "café/data.bin"] {
            validate_portable_effect_path(accepted).expect("portable path");
        }
        for rejected in [
            "",
            "/tmp/data",
            "C:/data",
            "src\\Main.gc",
            "src//Main.gc",
            "src/./Main.gc",
            "src/../Main.gc",
            "cafe\u{301}/data.bin",
        ] {
            assert!(
                validate_portable_effect_path(rejected).is_err(),
                "path must fail closed: {rejected:?}"
            );
        }
        assert_ne!(
            canonical_path_material(Path::new("src/Main.gc")),
            canonical_path_material(Path::new("src/main.gc"))
        );
    }

    #[test]
    fn canonical_path_material_normalizes_unicode_without_host_prefixes() {
        assert_eq!(
            canonical_path_material(Path::new("cafe\u{301}/data.bin")),
            Some("café/data.bin".to_string())
        );
        assert_eq!(
            canonical_path_material(Path::new("")),
            Some(".".to_string())
        );
    }

    #[cfg(unix)]
    #[test]
    fn non_utf8_paths_never_enter_language_payloads_lossily() {
        use std::ffi::OsString;
        use std::os::unix::ffi::OsStringExt;

        let invalid = PathBuf::from(OsString::from_vec(vec![b'a', 0xff, b'b']));
        assert_eq!(canonical_path_material(&invalid), None);

        let base = PathBuf::from("workspace");
        let invalid_path = base.join(invalid);
        let io_err = std::io::Error::new(std::io::ErrorKind::NotFound, "missing");
        let payload = io_error_payload("io/fs::read", &base, &invalid_path, &io_err);
        assert_eq!(map_value_str(&payload, ":path"), Some("<invalid-path>"));
        assert!(!format!("{payload:?}").contains('�'));
    }
}
