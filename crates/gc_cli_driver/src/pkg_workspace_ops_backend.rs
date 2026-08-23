use std::path::{Path, PathBuf};

use sha2::{Digest, Sha256};

use crate::pkg_caps_templates::render_backend_caps_policy;

pub(crate) struct BackendEnvPlan {
    pub(crate) effective_caps_body: Vec<u8>,
    pub(crate) effective_caps_hash: String,
    pub(crate) bridge_cmd: PathBuf,
    pub(crate) bridge_sha256: String,
    launcher_body: Option<Vec<u8>>,
}

pub(crate) fn plan_backend_env_bundle(workspace_file: &Path) -> Result<BackendEnvPlan, String> {
    let workspace_root = workspace_file
        .parent()
        .unwrap_or_else(|| Path::new("."))
        .to_path_buf();
    let workspace_root = absolutize(&workspace_root)?;
    let (bridge_cmd, bridge_bytes, launcher_body) = plan_backend_bridge_cmd(&workspace_root)?;
    let bridge_sha256 = format!("{:x}", Sha256::digest(&bridge_bytes));
    let caps_body = render_backend_caps_policy(Some(&bridge_cmd), Some(&bridge_sha256));
    Ok(BackendEnvPlan {
        effective_caps_body: caps_body.as_bytes().to_vec(),
        effective_caps_hash: blake3::hash(caps_body.as_bytes()).to_hex().to_string(),
        bridge_cmd,
        bridge_sha256,
        launcher_body,
    })
}

pub(crate) fn materialize_backend_bridge(plan: &BackendEnvPlan) -> Result<(), String> {
    let Some(body) = &plan.launcher_body else {
        return Ok(());
    };
    let parent = plan
        .bridge_cmd
        .parent()
        .ok_or_else(|| "backend bridge destination has no parent".to_string())?;
    crate::pkg_scaffold::preflight_directory_chain(parent)?;
    std::fs::create_dir_all(parent)
        .map_err(|error| format!("create backend runtime dir `{}`: {error}", parent.display()))?;
    write_text_if_changed(&plan.bridge_cmd, body)?;
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let mut permissions = std::fs::metadata(&plan.bridge_cmd)
            .map_err(|error| format!("read permissions `{}`: {error}", plan.bridge_cmd.display()))?
            .permissions();
        permissions.set_mode(0o755);
        std::fs::set_permissions(&plan.bridge_cmd, permissions).map_err(|error| {
            format!(
                "set executable permissions `{}`: {error}",
                plan.bridge_cmd.display()
            )
        })?;
    }
    Ok(())
}

fn absolutize(path: &Path) -> Result<PathBuf, String> {
    if path.is_absolute() {
        return Ok(path.to_path_buf());
    }
    std::env::current_dir()
        .map(|cwd| cwd.join(path))
        .map_err(|e| {
            format!(
                "resolve absolute workspace root from `{}`: {e}",
                path.display()
            )
        })
}

fn plan_backend_bridge_cmd(
    workspace_root: &Path,
) -> Result<(PathBuf, Vec<u8>, Option<Vec<u8>>), String> {
    if let Some(path) = detect_backend_bridge_cmd(workspace_root) {
        let bytes = std::fs::read(&path)
            .map_err(|error| format!("read backend bridge cmd `{}`: {error}", path.display()))?;
        return Ok((path, bytes, None));
    }
    plan_provisioned_backend_bridge_cmd(workspace_root)
}

fn detect_backend_bridge_cmd(workspace_root: &Path) -> Option<PathBuf> {
    let candidates = [
        workspace_root
            .join(".genesis")
            .join("runtime")
            .join("backend")
            .join("host_bridge"),
        workspace_root
            .join(".genesis")
            .join("runtime")
            .join("backend")
            .join("host_bridge.exe"),
        workspace_root
            .join(".genesis")
            .join("runtime")
            .join("backend")
            .join("host_bridge.sh"),
        workspace_root.join("tools").join("host_bridge.sh"),
        workspace_root
            .join(".genesis")
            .join("runtime")
            .join("backend")
            .join("host_bridge.cmd"),
        workspace_root.join("tools").join("host_bridge.cmd"),
    ];
    candidates.into_iter().find(|p| p.is_file())
}

fn plan_provisioned_backend_bridge_cmd(
    workspace_root: &Path,
) -> Result<(PathBuf, Vec<u8>, Option<Vec<u8>>), String> {
    let current = std::env::current_exe()
        .map_err(|e| format!("resolve current genesis binary for backend bridge: {e}"))?;
    if !current.is_file() {
        return Err(format!(
            "current genesis binary not found at {}",
            current.display()
        ));
    }

    let runtime_dir = workspace_root
        .join(".genesis")
        .join("runtime")
        .join("backend");
    if cfg!(windows) {
        let launcher = runtime_dir.join("host_bridge.cmd");
        let body = format!("@echo off\r\n\"{}\" %*\r\n", current.display()).into_bytes();
        return Ok((launcher, body.clone(), Some(body)));
    }

    let launcher = runtime_dir.join("host_bridge.sh");
    let body = format!(
        "#!/usr/bin/env bash\nset -euo pipefail\nexec \"{}\" \"$@\"\n",
        current.display()
    )
    .into_bytes();
    Ok((launcher, body.clone(), Some(body)))
}

fn write_text_if_changed(path: &Path, body: &[u8]) -> Result<(), String> {
    let rewrite = match std::fs::read(path) {
        Ok(existing) => existing != body,
        Err(_) => true,
    };
    if rewrite {
        crate::pkg_scaffold::atomic_write_text(path, body)
            .map_err(|e| format!("write `{}`: {e}", path.display()))?;
    }
    Ok(())
}
