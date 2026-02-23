use std::path::{Path, PathBuf};

use gc_coreform::{Term, TermOrdKey, hash_term};
use gc_effects::EffectLog;
use gc_pkg::{
    PackageManifest, RUNTIME_BACKEND_HEADLESS, UpdatePolicy, WorkspaceConfig, WorkspaceMember,
    WorkspaceTask, normalize_runtime_backend_profile, runtime_backend_profile_is_compatible,
};

#[path = "pkg_workspace_ops_build.rs"]
mod pkg_workspace_ops_build;
#[path = "pkg_workspace_ops_env.rs"]
mod pkg_workspace_ops_env;
#[path = "pkg_workspace_ops_manifest_helpers.rs"]
mod pkg_workspace_ops_manifest_helpers;

use pkg_workspace_ops_manifest_helpers::{build_env_deps_term, build_env_members_term};

pub(crate) struct LocalPkgResult {
    pub(crate) kind: &'static str,
    pub(crate) log_op: &'static str,
    pub(crate) value: Term,
    pub(crate) program_hash: [u8; 32],
}

pub(crate) fn handle_new(
    workspace: &str,
    lock: &Path,
    workspace_file: &Path,
    policy: &str,
    registry_default: Option<&str>,
    members: &[String],
) -> Result<LocalPkgResult, String> {
    let mut ws = WorkspaceConfig::empty(workspace.to_string());
    let active_runtime_backend = crate::active_runtime_backend_profile().to_string();
    let policy_s = policy.to_string();
    ws.defaults.policy = Some(policy_s.clone());
    ws.defaults.runtime_backend = Some(active_runtime_backend.clone());
    ws.profiles.insert(
        "dev".to_string(),
        gc_pkg::WorkspaceProfile {
            caps_policy: Some("caps.toml".to_string()),
            registry: registry_default.map(|s| s.to_string()),
            policy: Some(policy_s.clone()),
            toolchain: None,
            runtime_backend: Some(active_runtime_backend.clone()),
        },
    );
    ws.profiles.insert(
        "ci".to_string(),
        gc_pkg::WorkspaceProfile {
            caps_policy: Some("caps.ci.toml".to_string()),
            registry: registry_default.map(|s| s.to_string()),
            policy: Some(policy_s.clone()),
            toolchain: None,
            runtime_backend: Some(RUNTIME_BACKEND_HEADLESS.to_string()),
        },
    );
    ws.profiles.insert(
        "release".to_string(),
        gc_pkg::WorkspaceProfile {
            caps_policy: Some("caps.release.toml".to_string()),
            registry: registry_default.map(|s| s.to_string()),
            policy: Some(policy_s),
            toolchain: None,
            runtime_backend: Some(active_runtime_backend),
        },
    );
    ws.defaults.registry = registry_default.map(|s| s.to_string());
    if !members.is_empty() {
        ws.members.clear();
        for m in members {
            let member = parse_member_spec(m)?;
            ws.members.push(member);
        }
    }

    let mut l = gc_pkg::GenesisLock::empty(workspace.to_string());
    l.policy = policy.to_string();
    if let Some(rd) = registry_default {
        l.registries.insert("default".to_string(), rd.to_string());
    }

    let lock_body = l.to_toml_canonical();
    atomic_write_text(lock, lock_body.as_bytes()).map_err(|e| e.to_string())?;
    let lock_h = blake3::hash(lock_body.as_bytes()).to_hex().to_string();

    let ws_body = ws.to_toml_canonical();
    atomic_write_text(workspace_file, ws_body.as_bytes()).map_err(|e| e.to_string())?;
    let ws_h = blake3::hash(ws_body.as_bytes()).to_hex().to_string();

    let value = Term::Map(
        [
            (TermOrdKey(Term::symbol(":ok")), Term::Bool(true)),
            (
                TermOrdKey(Term::symbol(":workspace")),
                Term::Str(workspace.to_string()),
            ),
            (
                TermOrdKey(Term::symbol(":workspace-file")),
                Term::Str(workspace_file.display().to_string()),
            ),
            (TermOrdKey(Term::symbol(":workspace-h")), Term::Str(ws_h)),
            (
                TermOrdKey(Term::symbol(":members")),
                Term::Int((ws.members.len() as i64).into()),
            ),
            (
                TermOrdKey(Term::symbol(":lock")),
                Term::Str(lock.display().to_string()),
            ),
            (TermOrdKey(Term::symbol(":lock-h")), Term::Str(lock_h)),
        ]
        .into_iter()
        .collect(),
    );

    Ok(LocalPkgResult {
        kind: "genesis/pkg-new-v0.1",
        log_op: "pkg-new",
        program_hash: hash_term(&value),
        value,
    })
}

pub(crate) fn handle_remove(name: &str, lock: &Path) -> Result<LocalPkgResult, String> {
    let mut l = gc_pkg::GenesisLock::load(lock).map_err(|e| e.to_string())?;
    let removed_req = l.requirements.remove(name).is_some();
    let removed_locked = l.locked.remove(name).is_some();
    let removed = removed_req || removed_locked;

    let lock_body = l.to_toml_canonical();
    atomic_write_text(lock, lock_body.as_bytes()).map_err(|e| e.to_string())?;
    let lock_h = blake3::hash(lock_body.as_bytes()).to_hex().to_string();

    let value = Term::Map(
        [
            (TermOrdKey(Term::symbol(":ok")), Term::Bool(true)),
            (
                TermOrdKey(Term::symbol(":lock")),
                Term::Str(lock.display().to_string()),
            ),
            (TermOrdKey(Term::symbol(":lock-h")), Term::Str(lock_h)),
            (
                TermOrdKey(Term::symbol(":name")),
                Term::Str(name.to_string()),
            ),
            (TermOrdKey(Term::symbol(":removed")), Term::Bool(removed)),
        ]
        .into_iter()
        .collect(),
    );

    Ok(LocalPkgResult {
        kind: "genesis/pkg-remove-v0.1",
        log_op: "pkg-remove",
        program_hash: hash_term(&value),
        value,
    })
}

pub(crate) fn handle_migrate(
    pkg: &Path,
    lock: &Path,
    workspace_file: &Path,
    workspace_override: Option<&str>,
    registry_default: Option<&str>,
) -> Result<LocalPkgResult, String> {
    let (manifest, pkg_dir) = PackageManifest::load(pkg).map_err(|e| e.to_string())?;
    let workspace_name = workspace_override.unwrap_or(&manifest.name);

    let member_path = relative_to_cwd_or_literal(&pkg_dir);
    let mut ws = WorkspaceConfig::empty(workspace_name.to_string());
    ws.members = vec![WorkspaceMember {
        name: manifest.name.clone(),
        path: member_path,
        role: Some("package".to_string()),
    }];
    ws.defaults.registry = registry_default.map(|s| s.to_string());
    ws.tasks.insert(
        "test".to_string(),
        WorkspaceTask {
            cmd: "test".to_string(),
            file: None,
            pkg: Some(pkg.display().to_string()),
            args: vec![],
        },
    );
    ws.tasks.insert(
        "pack".to_string(),
        WorkspaceTask {
            cmd: "pack".to_string(),
            file: None,
            pkg: Some(pkg.display().to_string()),
            args: vec![],
        },
    );

    let mut l = gc_pkg::GenesisLock::empty(workspace_name.to_string());
    if let Some(rd) = registry_default {
        l.registries.insert("default".to_string(), rd.to_string());
    }
    for dep in &manifest.dependencies {
        if let Some(h) = dep.hash.as_deref()
            && is_hash_hex_64(h)
        {
            l.set_requirement(
                &dep.name,
                &format!("snapshot:{h}"),
                UpdatePolicy::Manual,
                Some("default".to_string()),
            );
        }
    }

    let lock_body = l.to_toml_canonical();
    atomic_write_text(lock, lock_body.as_bytes()).map_err(|e| e.to_string())?;
    let lock_h = blake3::hash(lock_body.as_bytes()).to_hex().to_string();

    let ws_body = ws.to_toml_canonical();
    atomic_write_text(workspace_file, ws_body.as_bytes()).map_err(|e| e.to_string())?;
    let ws_h = blake3::hash(ws_body.as_bytes()).to_hex().to_string();

    let value = Term::Map(
        [
            (TermOrdKey(Term::symbol(":ok")), Term::Bool(true)),
            (
                TermOrdKey(Term::symbol(":workspace")),
                Term::Str(workspace_name.to_string()),
            ),
            (
                TermOrdKey(Term::symbol(":workspace-file")),
                Term::Str(workspace_file.display().to_string()),
            ),
            (TermOrdKey(Term::symbol(":workspace-h")), Term::Str(ws_h)),
            (
                TermOrdKey(Term::symbol(":lock")),
                Term::Str(lock.display().to_string()),
            ),
            (TermOrdKey(Term::symbol(":lock-h")), Term::Str(lock_h)),
            (
                TermOrdKey(Term::symbol(":dep-count")),
                Term::Int((manifest.dependencies.len() as i64).into()),
            ),
        ]
        .into_iter()
        .collect(),
    );

    Ok(LocalPkgResult {
        kind: "genesis/pkg-migrate-v0.1",
        log_op: "pkg-migrate",
        program_hash: hash_term(&value),
        value,
    })
}

pub(crate) fn handle_build(
    pkg: &Path,
    target: &str,
    out_dir: &Path,
    frontend: gc_obligations::CoreformFrontend,
) -> Result<LocalPkgResult, String> {
    pkg_workspace_ops_build::handle_build(pkg, target, out_dir, frontend)
}

pub(crate) fn handle_env(
    profile: &str,
    runtime_backend_override: Option<&str>,
    lock: &Path,
    workspace_file: &Path,
    out_dir: &Path,
) -> Result<LocalPkgResult, String> {
    pkg_workspace_ops_env::handle_env(profile, runtime_backend_override, lock, workspace_file, out_dir)
}

pub(crate) fn empty_log(program_hash: [u8; 32]) -> EffectLog {
    EffectLog {
        version: 3,
        program_hash,
        toolchain: format!("genesis {}", env!("CARGO_PKG_VERSION")),
        entries: Vec::new(),
    }
}

fn parse_member_spec(spec: &str) -> Result<WorkspaceMember, String> {
    if let Some((name, path)) = spec.split_once('=') {
        if name.trim().is_empty() || path.trim().is_empty() {
            return Err(format!("invalid member spec `{spec}` (expected name=path)"));
        }
        Ok(WorkspaceMember {
            name: name.trim().to_string(),
            path: path.trim().to_string(),
            role: Some("package".to_string()),
        })
    } else {
        if spec.trim().is_empty() {
            return Err("member path must be non-empty".to_string());
        }
        let path = spec.trim().to_string();
        let name = path
            .split('/')
            .next_back()
            .filter(|s| !s.is_empty())
            .unwrap_or("member")
            .to_string();
        Ok(WorkspaceMember {
            name,
            path,
            role: Some("package".to_string()),
        })
    }
}

fn relative_to_cwd_or_literal(p: &Path) -> String {
    if let Ok(cwd) = std::env::current_dir()
        && let Ok(rel) = p.strip_prefix(&cwd)
    {
        let s = rel.to_string_lossy().to_string();
        if s.is_empty() {
            return ".".to_string();
        }
        return s;
    }
    let s = p.to_string_lossy().to_string();
    if s.is_empty() { ".".to_string() } else { s }
}

pub(crate) fn collect_missing_locked_hashes(
    workspace_file: &Path,
    lock_file: &Path,
) -> Result<Vec<String>, String> {
    let lock = gc_pkg::GenesisLock::load(lock_file).map_err(|e| e.to_string())?;
    let store_dir = workspace_store_dir(workspace_file);
    let mut missing: Vec<String> = Vec::new();
    for entry in lock.locked.values() {
        if !store_dir.join(&entry.snapshot).is_file() {
            missing.push(entry.snapshot.clone());
        }
        if let Some(commit) = &entry.commit
            && !store_dir.join(commit).is_file()
        {
            missing.push(commit.clone());
        }
    }
    missing.sort();
    missing.dedup();
    Ok(missing)
}

pub(crate) fn validate_workspace_runtime_backend_for_run(
    workspace_file: &Path,
) -> Result<String, String> {
    let ws = WorkspaceConfig::load(workspace_file).map_err(|e| e.to_string())?;
    let dev_profile_runtime_backend = ws
        .profiles
        .get("dev")
        .and_then(|p| p.runtime_backend.as_deref());
    let selected_runtime_backend = resolve_env_runtime_backend_profile(
        "dev",
        None,
        dev_profile_runtime_backend,
        ws.defaults.runtime_backend.as_deref(),
    )?;
    let active_runtime_backend = crate::active_runtime_backend_profile().to_string();
    let compatible =
        runtime_backend_profile_is_compatible(&selected_runtime_backend, &active_runtime_backend);
    if !compatible {
        return Err(format!(
            "workspace runtime_backend `{selected_runtime_backend}` (resolved from profile `dev`/defaults) is incompatible with active runtime backend profile `{active_runtime_backend}`"
        ));
    }
    Ok(selected_runtime_backend)
}

pub(crate) fn resolve_env_runtime_backend_profile(
    profile_name: &str,
    runtime_backend_override: Option<&str>,
    profile_runtime_backend: Option<&str>,
    default_runtime_backend: Option<&str>,
) -> Result<String, String> {
    pkg_workspace_ops_env::resolve_env_runtime_backend_profile(
        profile_name,
        runtime_backend_override,
        profile_runtime_backend,
        default_runtime_backend,
    )
}

fn workspace_store_dir(workspace_file: &Path) -> PathBuf {
    workspace_file
        .parent()
        .unwrap_or_else(|| Path::new("."))
        .join(".genesis")
        .join("store")
}

fn is_hash_hex_64(s: &str) -> bool {
    s.len() == 64 && s.as_bytes().iter().all(|b| b.is_ascii_hexdigit())
}

fn atomic_write_text(path: &Path, bytes: &[u8]) -> std::io::Result<()> {
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)?;
    }
    let tmp = path.with_extension("tmp");
    std::fs::write(&tmp, bytes)?;
    std::fs::rename(tmp, path)
}

fn write_if_same_or_new(path: &Path, bytes: &[u8]) -> std::io::Result<()> {
    pkg_workspace_ops_env::write_if_same_or_new(path, bytes)
}
