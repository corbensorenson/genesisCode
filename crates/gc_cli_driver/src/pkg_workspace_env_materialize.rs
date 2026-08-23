use std::collections::BTreeSet;
use std::path::{Path, PathBuf};

use super::pkg_workspace_env_authority::{AuthorizedEnvironment, FileScope};

pub(super) fn commit(
    authorized: &AuthorizedEnvironment,
    backend: Option<&super::pkg_workspace_ops_backend::BackendEnvPlan>,
) -> Result<(), String> {
    preflight(authorized, backend)?;
    if let Some(plan) = backend {
        super::pkg_workspace_ops_backend::materialize_backend_bridge(plan)?;
    }
    materialize_external(authorized)?;
    materialize_environment_root(authorized)
}

fn preflight(
    authorized: &AuthorizedEnvironment,
    backend: Option<&super::pkg_workspace_ops_backend::BackendEnvPlan>,
) -> Result<(), String> {
    let env_files = authorized
        .files
        .iter()
        .filter(|file| file.scope == FileScope::Environment)
        .collect::<Vec<_>>();
    let expected_names = env_files
        .iter()
        .map(|file| file.path.clone())
        .collect::<BTreeSet<_>>();
    crate::pkg_scaffold::preflight_directory_chain(
        authorized
            .env_root
            .parent()
            .unwrap_or_else(|| Path::new(".")),
    )?;
    match std::fs::symlink_metadata(&authorized.env_root) {
        Ok(metadata) if metadata.file_type().is_symlink() || !metadata.is_dir() => {
            return Err(format!(
                "workspace environment root is not a regular directory: {}",
                authorized.env_root.display()
            ));
        }
        Ok(_) => {
            let actual = std::fs::read_dir(&authorized.env_root)
                .map_err(|error| error.to_string())?
                .map(|entry| entry.map(|entry| PathBuf::from(entry.file_name())))
                .collect::<Result<BTreeSet<_>, _>>()
                .map_err(|error| error.to_string())?;
            if actual != expected_names {
                return Err("existing workspace environment file inventory mismatch".to_string());
            }
            for file in env_files {
                require_existing_file(&authorized.env_root.join(&file.path), &file.body)?;
            }
        }
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
        Err(error) => return Err(error.to_string()),
    }
    for directory in &authorized.mkdirs {
        crate::pkg_scaffold::preflight_directory_chain(directory)?;
    }
    for file in authorized
        .files
        .iter()
        .filter(|file| file.scope == FileScope::External)
    {
        preflight_mutable_file(&file.path)?;
    }
    if let Some(backend) = backend {
        preflight_mutable_file(&backend.bridge_cmd)?;
    }
    Ok(())
}

fn preflight_mutable_file(path: &Path) -> Result<(), String> {
    if let Some(parent) = path.parent() {
        crate::pkg_scaffold::preflight_directory_chain(parent)?;
    }
    match std::fs::symlink_metadata(path) {
        Ok(metadata) if metadata.file_type().is_symlink() || !metadata.is_file() => Err(format!(
            "refusing workspace environment non-regular destination: {}",
            path.display()
        )),
        Ok(_) => Ok(()),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(()),
        Err(error) => Err(format!("inspect destination `{}`: {error}", path.display())),
    }
}

fn require_existing_file(path: &Path, expected: &[u8]) -> Result<(), String> {
    let metadata = std::fs::symlink_metadata(path).map_err(|error| error.to_string())?;
    if metadata.file_type().is_symlink() || !metadata.is_file() {
        return Err(format!(
            "immutable environment artifact is not a file: {}",
            path.display()
        ));
    }
    let actual = std::fs::read(path).map_err(|error| error.to_string())?;
    if actual == expected {
        Ok(())
    } else {
        Err(format!(
            "immutable environment artifact differs: {}",
            path.display()
        ))
    }
}

fn materialize_external(authorized: &AuthorizedEnvironment) -> Result<(), String> {
    for directory in &authorized.mkdirs {
        std::fs::create_dir_all(directory).map_err(|error| {
            format!(
                "create workspace environment directory `{}`: {error}",
                directory.display()
            )
        })?;
    }
    for file in authorized
        .files
        .iter()
        .filter(|file| file.scope == FileScope::External)
    {
        crate::pkg_scaffold::atomic_write_text(&file.path, &file.body)
            .map_err(|error| format!("write `{}`: {error}", file.path.display()))?;
    }
    Ok(())
}

fn materialize_environment_root(authorized: &AuthorizedEnvironment) -> Result<(), String> {
    if authorized.env_root.is_dir() {
        return Ok(());
    }
    let out_dir = authorized
        .env_root
        .parent()
        .ok_or_else(|| "workspace environment root has no parent".to_string())?;
    std::fs::create_dir_all(out_dir).map_err(|error| error.to_string())?;
    let mut sequence = 0u64;
    let staging = loop {
        let candidate = out_dir.join(format!(
            ".{}.tmp-{}-{sequence}",
            authorized
                .env_root
                .file_name()
                .and_then(|name| name.to_str())
                .unwrap_or("env"),
            crate::platform_process_id()
        ));
        sequence = sequence.saturating_add(1);
        match std::fs::create_dir(&candidate) {
            Ok(()) => break candidate,
            Err(error) if error.kind() == std::io::ErrorKind::AlreadyExists => continue,
            Err(error) => return Err(error.to_string()),
        }
    };
    let write_result = (|| {
        for file in authorized
            .files
            .iter()
            .filter(|file| file.scope == FileScope::Environment)
        {
            std::fs::write(staging.join(&file.path), &file.body)
                .map_err(|error| format!("stage `{}`: {error}", file.path.display()))?;
        }
        std::fs::rename(&staging, &authorized.env_root).map_err(|error| {
            format!(
                "publish workspace environment `{}`: {error}",
                authorized.env_root.display()
            )
        })
    })();
    if write_result.is_err() {
        let _ = std::fs::remove_dir_all(&staging);
    }
    write_result
}
