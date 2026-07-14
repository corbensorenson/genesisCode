use std::collections::BTreeSet;
use std::fs::{self, OpenOptions};
use std::io::Write;
use std::path::PathBuf;

use serde_json::json;

use super::model::WorkspaceSnapshot;
use super::storage::{SessionPaths, read_object, valid_relative_path_material, write_atomic};
use super::*;

pub(super) struct ApplyLock {
    path: PathBuf,
}

impl Drop for ApplyLock {
    fn drop(&mut self) {
        let _ = fs::remove_file(&self.path);
    }
}

fn session_error(code: &'static str, message: impl Into<String>, session: &str) -> CliError {
    cli_err_with_context(
        EX_IO,
        code,
        message,
        json!({"operation": "agent-session", "session": session}),
    )
}

pub(super) fn acquire_apply_lock(
    paths: &SessionPaths,
    session: &str,
) -> Result<ApplyLock, CliError> {
    fs::create_dir_all(&paths.store_root).map_err(|error| {
        session_error(
            "session/lock-failed",
            format!("cannot create transaction store: {error}"),
            session,
        )
    })?;
    let path = paths.store_root.join("apply.lock");
    let mut file = OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(&path)
        .map_err(|error| {
            session_error(
                "session/apply-busy",
                format!("another transaction apply is active: {error}"),
                session,
            )
        })?;
    file.write_all(session.as_bytes()).map_err(|error| {
        session_error(
            "session/lock-failed",
            format!("cannot record transaction lock: {error}"),
            session,
        )
    })?;
    Ok(ApplyLock { path })
}

pub(super) fn apply_snapshot(
    paths: &SessionPaths,
    base: &WorkspaceSnapshot,
    current: &WorkspaceSnapshot,
    session: &str,
) -> Result<(), CliError> {
    for file in current.files.iter().chain(&base.files) {
        if !valid_relative_path_material(&file.path) {
            return Err(session_error(
                "session/snapshot-mismatch",
                "snapshot path material verification failed",
                session,
            ));
        }
        read_object(paths, &file.blob, file.bytes, session)?;
    }
    let base_paths = base
        .files
        .iter()
        .map(|file| file.path.as_str())
        .collect::<BTreeSet<_>>();
    let current_paths = current
        .files
        .iter()
        .map(|file| file.path.as_str())
        .collect::<BTreeSet<_>>();
    let apply_result = (|| -> std::io::Result<()> {
        for file in &current.files {
            let destination = paths.live_root.join(&file.path);
            let bytes = read_object(paths, &file.blob, file.bytes, session)
                .map_err(|_| std::io::Error::other("snapshot object verification failed"))?;
            write_atomic(&destination, &bytes)?;
        }
        for removed in base_paths.difference(&current_paths) {
            let target = paths.live_root.join(removed);
            if target.exists() {
                fs::remove_file(target)?;
            }
        }
        Ok(())
    })();
    if let Err(error) = apply_result {
        let rollback = (|| -> std::io::Result<()> {
            for file in &base.files {
                let bytes = read_object(paths, &file.blob, file.bytes, session)
                    .map_err(|_| std::io::Error::other("snapshot object verification failed"))?;
                write_atomic(&paths.live_root.join(&file.path), &bytes)?;
            }
            for added in current_paths.difference(&base_paths) {
                let target = paths.live_root.join(added);
                if target.exists() {
                    fs::remove_file(target)?;
                }
            }
            Ok(())
        })();
        return match rollback {
            Ok(()) => Err(session_error(
                "session/apply-failed",
                format!("transaction apply failed and was rolled back: {error}"),
                session,
            )),
            Err(rollback_error) => Err(session_error(
                "session/rollback-failed",
                format!("transaction apply failed ({error}); rollback failed ({rollback_error})"),
                session,
            )),
        };
    }
    Ok(())
}
