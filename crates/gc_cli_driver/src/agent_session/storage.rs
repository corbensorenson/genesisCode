use std::collections::BTreeSet;
use std::fs;
use std::path::{Component, Path, PathBuf};

use serde_json::json;

use super::model::{
    SESSION_SCHEMA, SNAPSHOT_SCHEMA, SessionRecord, SnapshotFile, WorkspaceSnapshot,
};
use super::*;

const MAX_SNAPSHOT_FILES: usize = 4096;
const MAX_SNAPSHOT_BYTES: u64 = 256 * 1024 * 1024;

pub(super) struct SessionPaths {
    pub(super) live_root: PathBuf,
    pub(super) store_root: PathBuf,
    pub(super) transaction_root: PathBuf,
    pub(super) workspace_root: PathBuf,
    pub(super) state_path: PathBuf,
}

fn session_error(code: &'static str, message: impl Into<String>, session: &str) -> CliError {
    cli_err_with_context(
        EX_IO,
        code,
        message,
        json!({"operation": "agent-session", "session": session}),
    )
}

fn manifest_error_detail(error: gc_pkg::ManifestError) -> String {
    match error {
        gc_pkg::ManifestError::Io(error) => {
            format!("package manifest I/O failed with {:?}", error.kind())
        }
        gc_pkg::ManifestError::Parse { msg, .. } => {
            format!("package manifest parsing failed: {msg}")
        }
        gc_pkg::ManifestError::Invalid { msg, .. } => {
            format!("package manifest validation failed: {msg}")
        }
    }
}

pub(super) fn validate_session_id(session: &str) -> Result<(), CliError> {
    let valid = !session.is_empty()
        && session.len() <= 64
        && session
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'-' | b'_'));
    if valid {
        Ok(())
    } else {
        Err(session_error(
            "session/invalid-id",
            "session ID must use 1..64 ASCII letters, digits, '-' or '_'",
            session,
        ))
    }
}

pub(super) fn resolve_paths(pkg: &Path, session: &str) -> Result<SessionPaths, CliError> {
    validate_session_id(session)?;
    let (_manifest, package_root) = PackageManifest::load(pkg).map_err(|error| {
        session_error(
            "session/package-invalid",
            manifest_error_detail(error),
            session,
        )
    })?;
    let package_root = if package_root.as_os_str().is_empty() {
        PathBuf::from(".")
    } else {
        package_root
    };
    let live_root = package_root.canonicalize().map_err(|error| {
        session_error(
            "session/package-unavailable",
            format!("package root is unavailable: {error}"),
            session,
        )
    })?;
    let store_root = live_root.join(".genesis/agent-sessions");
    let transaction_root = store_root.join("transactions").join(session);
    Ok(SessionPaths {
        workspace_root: transaction_root.join("workspace"),
        state_path: transaction_root.join("session.json"),
        live_root,
        store_root,
        transaction_root,
    })
}

pub(super) fn package_manifest_name(pkg: &Path) -> Result<String, CliError> {
    pkg.file_name()
        .and_then(|name| name.to_str())
        .filter(|name| !name.is_empty())
        .map(str::to_string)
        .ok_or_else(|| {
            cli_err(
                EX_PARSE,
                "session/package-path",
                "package path has no file name",
            )
        })
}

fn relative_path(root: &Path, path: &Path, session: &str) -> Result<String, CliError> {
    let relative = path.strip_prefix(root).map_err(|_| {
        session_error(
            "session/snapshot-escape",
            "snapshot input escapes the package root",
            session,
        )
    })?;
    let mut parts = Vec::new();
    for component in relative.components() {
        match component {
            Component::Normal(value) => {
                let value = value.to_str().ok_or_else(|| {
                    session_error(
                        "session/snapshot-path",
                        "snapshot paths must be valid UTF-8",
                        session,
                    )
                })?;
                parts.push(value.to_string());
            }
            _ => {
                return Err(session_error(
                    "session/snapshot-path",
                    "snapshot paths must be normalized and base-relative",
                    session,
                ));
            }
        }
    }
    if parts.is_empty() {
        return Err(session_error(
            "session/snapshot-path",
            "snapshot paths must name a file",
            session,
        ));
    }
    let relative = parts.join("/");
    if valid_relative_path_material(&relative) {
        Ok(relative)
    } else {
        Err(session_error(
            "session/snapshot-path",
            "snapshot paths must use canonical base-relative path material",
            session,
        ))
    }
}

pub(super) fn valid_relative_path_material(path: &str) -> bool {
    !path.is_empty()
        && !path.starts_with('/')
        && !path.contains('\\')
        && !path.chars().any(char::is_control)
        && path
            .split('/')
            .all(|segment| !segment.is_empty() && segment != "." && segment != "..")
}

fn validated_snapshot_input(
    root: &Path,
    input: &Path,
    session: &str,
) -> Result<(String, PathBuf), CliError> {
    let relative = relative_path(root, input, session)?;
    let mut candidate = root.to_path_buf();
    for segment in relative.split('/') {
        candidate.push(segment);
        let metadata = fs::symlink_metadata(&candidate).map_err(|error| {
            session_error(
                "session/snapshot-read",
                format!("cannot inspect snapshot input: {error}"),
                session,
            )
        })?;
        if metadata.file_type().is_symlink() {
            return Err(session_error(
                "session/snapshot-file",
                "snapshot inputs and their parent paths must not be symlinks",
                session,
            ));
        }
    }
    let canonical = candidate.canonicalize().map_err(|error| {
        session_error(
            "session/snapshot-read",
            format!("cannot resolve snapshot input: {error}"),
            session,
        )
    })?;
    if canonical != candidate || !canonical.starts_with(root) {
        return Err(session_error(
            "session/snapshot-escape",
            "snapshot input does not resolve canonically beneath the package root",
            session,
        ));
    }
    Ok((relative, canonical))
}

fn add_manifest_closure(
    manifest_path: &Path,
    root: &Path,
    session: &str,
    visited: &mut BTreeSet<PathBuf>,
    files: &mut BTreeSet<PathBuf>,
) -> Result<(), CliError> {
    let canonical_manifest = manifest_path.canonicalize().map_err(|error| {
        session_error(
            "session/snapshot-read",
            format!("cannot resolve snapshot manifest: {error}"),
            session,
        )
    })?;
    if !canonical_manifest.starts_with(root) {
        return Err(session_error(
            "session/snapshot-escape",
            "package dependency escapes the transaction root",
            session,
        ));
    }
    if !visited.insert(canonical_manifest.clone()) {
        return Ok(());
    }
    let (manifest, package_root) = PackageManifest::load(&canonical_manifest).map_err(|error| {
        session_error(
            "session/package-invalid",
            manifest_error_detail(error),
            session,
        )
    })?;
    files.insert(canonical_manifest);
    for module in manifest.modules {
        files.insert(package_root.join(module.path));
    }
    if let Some(caps) = manifest.caps_policy {
        files.insert(package_root.join(caps));
    }
    for dependency in manifest.dependencies {
        let candidate = package_root.join(dependency.path);
        let dependency_manifest = if candidate.is_dir() {
            candidate.join("package.toml")
        } else {
            candidate
        };
        add_manifest_closure(&dependency_manifest, root, session, visited, files)?;
    }
    Ok(())
}

fn blob_identity(bytes: &[u8]) -> String {
    let mut hasher = blake3::Hasher::new();
    hasher.update(b"GCv0.2\0agent-session-blob-v0.1\0");
    hasher.update(bytes);
    hasher.finalize().to_hex().to_string()
}

fn snapshot_identity(files: &[SnapshotFile]) -> String {
    let payload = json!({"schema": SNAPSHOT_SCHEMA, "files": files});
    let mut hasher = blake3::Hasher::new();
    hasher.update(b"GCv0.2\0workspace-snapshot-v0.1\0");
    hasher.update(json_canonical_string(&payload).as_bytes());
    hasher.finalize().to_hex().to_string()
}

pub(super) fn read_object(
    paths: &SessionPaths,
    blob: &str,
    expected_bytes: u64,
    session: &str,
) -> Result<Vec<u8>, CliError> {
    let bytes = fs::read(paths.store_root.join("objects").join(blob)).map_err(|error| {
        session_error(
            "session/snapshot-missing",
            format!("cannot read snapshot object: {error}"),
            session,
        )
    })?;
    if blob_identity(&bytes) != blob || bytes.len() as u64 != expected_bytes {
        return Err(session_error(
            "session/snapshot-mismatch",
            "snapshot object identity verification failed",
            session,
        ));
    }
    Ok(bytes)
}

pub(super) fn write_atomic(path: &Path, bytes: &[u8]) -> std::io::Result<()> {
    let parent = path.parent().ok_or_else(|| {
        std::io::Error::new(std::io::ErrorKind::InvalidInput, "path has no parent")
    })?;
    fs::create_dir_all(parent)?;
    let tmp = parent.join(format!(
        ".{}.genesis-tmp",
        path.file_name()
            .and_then(|name| name.to_str())
            .unwrap_or("file")
    ));
    fs::write(&tmp, bytes)?;
    fs::rename(tmp, path)
}

pub(super) fn capture_snapshot(
    paths: &SessionPaths,
    source_root: &Path,
    package_manifest: &str,
    session: &str,
) -> Result<WorkspaceSnapshot, CliError> {
    let source_root = source_root.canonicalize().map_err(|error| {
        session_error(
            "session/snapshot-read",
            format!("cannot resolve snapshot root: {error}"),
            session,
        )
    })?;
    let mut inputs = BTreeSet::new();
    add_manifest_closure(
        &source_root.join(package_manifest),
        &source_root,
        session,
        &mut BTreeSet::new(),
        &mut inputs,
    )?;
    if inputs.len() > MAX_SNAPSHOT_FILES {
        return Err(session_error(
            "session/snapshot-limit",
            format!("snapshot exceeds {MAX_SNAPSHOT_FILES} files"),
            session,
        ));
    }
    let mut total = 0_u64;
    let mut records = Vec::with_capacity(inputs.len());
    for input in inputs {
        let (relative, input) = validated_snapshot_input(&source_root, &input, session)?;
        let metadata = fs::symlink_metadata(&input).map_err(|error| {
            session_error(
                "session/snapshot-read",
                format!("cannot inspect snapshot input: {error}"),
                session,
            )
        })?;
        if metadata.file_type().is_symlink() || !metadata.is_file() {
            return Err(session_error(
                "session/snapshot-file",
                "snapshot inputs must be regular, non-symlink files",
                session,
            ));
        }
        let bytes = fs::read(&input).map_err(|error| {
            session_error(
                "session/snapshot-read",
                format!("cannot read snapshot input: {error}"),
                session,
            )
        })?;
        total = total.saturating_add(bytes.len() as u64);
        if total > MAX_SNAPSHOT_BYTES {
            return Err(session_error(
                "session/snapshot-limit",
                format!("snapshot exceeds {MAX_SNAPSHOT_BYTES} bytes"),
                session,
            ));
        }
        let blob = blob_identity(&bytes);
        let object = paths.store_root.join("objects").join(&blob);
        if object.exists() {
            read_object(paths, &blob, bytes.len() as u64, session)?;
        } else {
            write_atomic(&object, &bytes).map_err(|error| {
                session_error(
                    "session/store-write",
                    format!("cannot persist snapshot object: {error}"),
                    session,
                )
            })?;
        }
        records.push(SnapshotFile {
            path: relative,
            blob,
            bytes: bytes.len() as u64,
        });
    }
    records.sort_by(|left, right| left.path.cmp(&right.path));
    let identity = snapshot_identity(&records);
    let snapshot = WorkspaceSnapshot {
        schema: SNAPSHOT_SCHEMA.to_string(),
        identity: identity.clone(),
        files: records,
    };
    let encoded = format!(
        "{}\n",
        json_canonical_string(&serde_json::to_value(&snapshot).map_err(|error| {
            session_error(
                "session/state-invalid",
                format!("cannot encode snapshot: {error}"),
                session,
            )
        })?)
    );
    write_atomic(
        &paths
            .store_root
            .join("snapshots")
            .join(format!("{identity}.json")),
        encoded.as_bytes(),
    )
    .map_err(|error| {
        session_error(
            "session/store-write",
            format!("cannot persist snapshot manifest: {error}"),
            session,
        )
    })?;
    Ok(snapshot)
}

pub(super) fn load_snapshot(
    paths: &SessionPaths,
    identity: &str,
    session: &str,
) -> Result<WorkspaceSnapshot, CliError> {
    let bytes = fs::read(
        paths
            .store_root
            .join("snapshots")
            .join(format!("{identity}.json")),
    )
    .map_err(|error| {
        session_error(
            "session/snapshot-missing",
            format!("cannot read snapshot manifest: {error}"),
            session,
        )
    })?;
    let snapshot: WorkspaceSnapshot = serde_json::from_slice(&bytes).map_err(|error| {
        session_error(
            "session/state-invalid",
            format!("snapshot manifest is invalid: {error}"),
            session,
        )
    })?;
    if snapshot.schema != SNAPSHOT_SCHEMA
        || snapshot.identity != identity
        || snapshot_identity(&snapshot.files) != identity
    {
        return Err(session_error(
            "session/snapshot-mismatch",
            "snapshot identity verification failed",
            session,
        ));
    }
    if snapshot.files.len() > MAX_SNAPSHOT_FILES
        || snapshot
            .files
            .windows(2)
            .any(|pair| pair[0].path >= pair[1].path)
        || snapshot
            .files
            .iter()
            .map(|file| file.bytes)
            .fold(0_u64, u64::saturating_add)
            > MAX_SNAPSHOT_BYTES
        || snapshot.files.iter().any(|file| {
            !valid_relative_path_material(&file.path)
                || !is_hex64(&file.blob)
                || file.bytes > MAX_SNAPSHOT_BYTES
        })
    {
        return Err(session_error(
            "session/snapshot-mismatch",
            "snapshot ordering, uniqueness, or bounds verification failed",
            session,
        ));
    }
    Ok(snapshot)
}

pub(super) fn materialize_snapshot(
    paths: &SessionPaths,
    snapshot: &WorkspaceSnapshot,
    destination: &Path,
    session: &str,
) -> Result<(), CliError> {
    if destination.exists() {
        fs::remove_dir_all(destination).map_err(|error| {
            session_error(
                "session/workspace-write",
                format!("cannot clear transaction workspace: {error}"),
                session,
            )
        })?;
    }
    fs::create_dir_all(destination).map_err(|error| {
        session_error(
            "session/workspace-write",
            format!("cannot create transaction workspace: {error}"),
            session,
        )
    })?;
    for file in &snapshot.files {
        if !valid_relative_path_material(&file.path) {
            return Err(session_error(
                "session/snapshot-mismatch",
                "snapshot path material verification failed",
                session,
            ));
        }
        let bytes = read_object(paths, &file.blob, file.bytes, session)?;
        write_atomic(&destination.join(&file.path), &bytes).map_err(|error| {
            session_error(
                "session/workspace-write",
                format!("cannot materialize transaction workspace: {error}"),
                session,
            )
        })?;
    }
    Ok(())
}

pub(super) fn save_state(paths: &SessionPaths, state: &SessionRecord) -> Result<(), CliError> {
    let encoded = format!(
        "{}\n",
        json_canonical_string(&serde_json::to_value(state).map_err(|error| {
            session_error(
                "session/state-invalid",
                format!("cannot encode transaction state: {error}"),
                &state.session,
            )
        })?)
    );
    write_atomic(&paths.state_path, encoded.as_bytes()).map_err(|error| {
        session_error(
            "session/state-write",
            format!("cannot persist transaction state: {error}"),
            &state.session,
        )
    })
}

pub(super) fn load_state(paths: &SessionPaths, session: &str) -> Result<SessionRecord, CliError> {
    let bytes = fs::read(&paths.state_path).map_err(|error| {
        session_error(
            "session/not-found",
            format!("transaction does not exist: {error}"),
            session,
        )
    })?;
    let state: SessionRecord = serde_json::from_slice(&bytes).map_err(|error| {
        session_error(
            "session/state-invalid",
            format!("transaction state is invalid: {error}"),
            session,
        )
    })?;
    if state.schema != SESSION_SCHEMA || state.session != session {
        return Err(session_error(
            "session/state-invalid",
            "transaction identity does not match its state record",
            session,
        ));
    }
    let identities_valid = is_hex64(&state.base_snapshot)
        && is_hex64(&state.current_snapshot)
        && state.patches.iter().all(|patch| {
            is_hex64(&patch.patch)
                && is_hex64(&patch.before_snapshot)
                && is_hex64(&patch.after_snapshot)
                && patch.acceptance.as_deref().is_none_or(is_hex64)
        })
        && state.verification.as_ref().is_none_or(|verification| {
            is_hex64(&verification.snapshot) && is_hex64(&verification.acceptance)
        });
    let mut expected = state.base_snapshot.as_str();
    let chain_valid = state.patches.iter().all(|patch| {
        let valid = patch.before_snapshot == expected;
        expected = patch.after_snapshot.as_str();
        valid
    }) && expected == state.current_snapshot;
    let verification_valid = state
        .verification
        .as_ref()
        .is_none_or(|verification| verification.snapshot == state.current_snapshot);
    let applied_valid = state.status != super::model::SessionStatus::Applied
        || state
            .verification
            .as_ref()
            .is_some_and(|verification| verification.obligations_ok);
    if state.patches.len() > MAX_SNAPSHOT_FILES
        || !identities_valid
        || !chain_valid
        || !verification_valid
        || !applied_valid
    {
        return Err(session_error(
            "session/state-invalid",
            "transaction patch chain or verification identity is invalid",
            session,
        ));
    }
    Ok(state)
}

pub(super) fn snapshot_contains(snapshot: &WorkspaceSnapshot, relative: &str) -> bool {
    snapshot.files.iter().any(|file| file.path == relative)
}
