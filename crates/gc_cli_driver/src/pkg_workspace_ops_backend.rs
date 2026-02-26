use std::path::{Path, PathBuf};

use sha2::{Digest, Sha256};

use crate::pkg_caps_templates::render_backend_caps_policy;

pub(crate) struct BackendEnvBundle {
    pub(crate) effective_caps_path: PathBuf,
    pub(crate) effective_caps_hash: String,
    pub(crate) bridge_cmd: Option<PathBuf>,
    pub(crate) bridge_sha256: Option<String>,
    pub(crate) bridge_ready: bool,
}

pub(crate) fn materialize_backend_env_bundle(
    workspace_file: &Path,
    env_root: &Path,
) -> Result<BackendEnvBundle, String> {
    let workspace_root = workspace_file
        .parent()
        .unwrap_or_else(|| Path::new("."))
        .to_path_buf();
    let bridge_cmd = resolve_backend_bridge_cmd(&workspace_root)?;
    let bridge_sha256 = bridge_cmd
        .as_deref()
        .map(sha256_hex_file)
        .transpose()
        .map_err(|e| format!("hash backend bridge cmd: {e}"))?;
    let bridge_ready = bridge_cmd.is_some() && bridge_sha256.is_some();

    let caps_body = render_backend_caps_policy(bridge_cmd.as_deref(), bridge_sha256.as_deref());
    let effective_caps_path = env_root.join("caps-policy.backend.effective.toml");
    super::write_if_same_or_new(&effective_caps_path, caps_body.as_bytes())
        .map_err(|e| e.to_string())?;

    Ok(BackendEnvBundle {
        effective_caps_path,
        effective_caps_hash: blake3::hash(caps_body.as_bytes()).to_hex().to_string(),
        bridge_cmd,
        bridge_sha256,
        bridge_ready,
    })
}

fn resolve_backend_bridge_cmd(workspace_root: &Path) -> Result<Option<PathBuf>, String> {
    if let Some(path) = detect_backend_bridge_cmd(workspace_root) {
        return Ok(Some(path));
    }
    provision_backend_bridge_cmd(workspace_root).map(Some)
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

fn provision_backend_bridge_cmd(workspace_root: &Path) -> Result<PathBuf, String> {
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
    std::fs::create_dir_all(&runtime_dir).map_err(|e| {
        format!(
            "create backend runtime dir `{}`: {e}",
            runtime_dir.display()
        )
    })?;

    if cfg!(windows) {
        let launcher = runtime_dir.join("host_bridge.cmd");
        let body = format!("@echo off\r\n\"{}\" %*\r\n", current.display());
        write_text_if_changed(&launcher, body.as_bytes())?;
        return Ok(launcher);
    }

    let launcher = runtime_dir.join("host_bridge.sh");
    let body = format!(
        "#!/usr/bin/env bash\nset -euo pipefail\nexec \"{}\" \"$@\"\n",
        current.display()
    );
    write_text_if_changed(&launcher, body.as_bytes())?;
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let mut perms = std::fs::metadata(&launcher)
            .map_err(|e| format!("read permissions `{}`: {e}", launcher.display()))?
            .permissions();
        perms.set_mode(0o755);
        std::fs::set_permissions(&launcher, perms)
            .map_err(|e| format!("set executable permissions `{}`: {e}", launcher.display()))?;
    }
    Ok(launcher)
}

fn write_text_if_changed(path: &Path, body: &[u8]) -> Result<(), String> {
    let rewrite = match std::fs::read(path) {
        Ok(existing) => existing != body,
        Err(_) => true,
    };
    if rewrite {
        std::fs::write(path, body).map_err(|e| format!("write `{}`: {e}", path.display()))?;
    }
    Ok(())
}

fn sha256_hex_file(path: &Path) -> std::io::Result<String> {
    let bytes = std::fs::read(path)?;
    Ok(format!("{:x}", Sha256::digest(&bytes)))
}
