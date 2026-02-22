use std::path::{Path, PathBuf};

use gc_coreform::{Term, TermOrdKey, hash_term};
use gc_effects::EffectLog;
use gc_pkg::{
    PackageManifest, RUNTIME_BACKEND_HEADLESS, UpdatePolicy, WorkspaceConfig, WorkspaceMember,
    WorkspaceTask, normalize_runtime_backend_profile, runtime_backend_profile_is_compatible,
};

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
    _frontend: gc_obligations::CoreformFrontend,
) -> Result<LocalPkgResult, String> {
    let target_label = normalize_build_target(target)?;
    let target_profile = build_target_profile(target_label)?;
    let (manifest, _) = PackageManifest::load(pkg).map_err(|e| e.to_string())?;
    let package_src = std::fs::read(pkg).map_err(|e| e.to_string())?;
    let package_h = blake3::hash(&package_src).to_hex().to_string();
    let package_artifact = gc_obligations::package_artifact_hash(pkg).map_err(|e| e.to_string())?;

    let build_manifest = Term::Map(
        [
            (
                TermOrdKey(Term::symbol(":type")),
                Term::symbol(":gcpm/build-manifest"),
            ),
            (TermOrdKey(Term::symbol(":v")), Term::Int(1.into())),
            (
                TermOrdKey(Term::symbol(":target")),
                Term::Str(target_label.to_string()),
            ),
            (
                TermOrdKey(Term::symbol(":target-profile")),
                Term::Map(
                    [
                        (
                            TermOrdKey(Term::symbol(":runtime")),
                            Term::Str(target_profile.runtime.to_string()),
                        ),
                        (
                            TermOrdKey(Term::symbol(":host-profile")),
                            Term::Str(target_profile.host_profile.to_string()),
                        ),
                        (
                            TermOrdKey(Term::symbol(":artifact-format")),
                            Term::Str(target_profile.artifact_format.to_string()),
                        ),
                    ]
                    .into_iter()
                    .collect(),
                ),
            ),
            (
                TermOrdKey(Term::symbol(":package")),
                Term::Map(
                    [
                        (
                            TermOrdKey(Term::symbol(":name")),
                            Term::Str(manifest.name.clone()),
                        ),
                        (
                            TermOrdKey(Term::symbol(":version")),
                            Term::Str(manifest.version.clone()),
                        ),
                        (
                            TermOrdKey(Term::symbol(":package-h")),
                            Term::Str(package_h.clone()),
                        ),
                        (
                            TermOrdKey(Term::symbol(":package-artifact")),
                            Term::Str(package_artifact.clone()),
                        ),
                    ]
                    .into_iter()
                    .collect(),
                ),
            ),
        ]
        .into_iter()
        .collect(),
    );
    let build_manifest_src = gc_coreform::print_term(&build_manifest) + "\n";
    let bundle_h = blake3::hash(build_manifest_src.as_bytes())
        .to_hex()
        .to_string();
    let bundle_root = out_dir.join(target_label).join(&bundle_h);
    std::fs::create_dir_all(&bundle_root).map_err(|e| e.to_string())?;

    write_if_same_or_new(
        &bundle_root.join("build_manifest.gc"),
        build_manifest_src.as_bytes(),
    )
    .map_err(|e| e.to_string())?;
    write_if_same_or_new(&bundle_root.join("package.toml"), &package_src)
        .map_err(|e| e.to_string())?;
    write_if_same_or_new(
        &bundle_root.join("package_artifact.txt"),
        format!("{package_artifact}\n").as_bytes(),
    )
    .map_err(|e| e.to_string())?;

    let provenance = Term::Map(
        [
            (
                TermOrdKey(Term::symbol(":type")),
                Term::symbol(":gcpm/build-provenance"),
            ),
            (TermOrdKey(Term::symbol(":v")), Term::Int(1.into())),
            (
                TermOrdKey(Term::symbol(":target")),
                Term::Str(target_label.to_string()),
            ),
            (
                TermOrdKey(Term::symbol(":bundle-h")),
                Term::Str(bundle_h.clone()),
            ),
            (
                TermOrdKey(Term::symbol(":build-manifest-h")),
                Term::Str(bundle_h.clone()),
            ),
            (
                TermOrdKey(Term::symbol(":package-artifact")),
                Term::Str(package_artifact.clone()),
            ),
            (
                TermOrdKey(Term::symbol(":generated-by")),
                Term::Str(format!("genesis {}", env!("CARGO_PKG_VERSION"))),
            ),
        ]
        .into_iter()
        .collect(),
    );
    let provenance_src = gc_coreform::print_term(&provenance) + "\n";
    write_if_same_or_new(
        &bundle_root.join("provenance.gc"),
        provenance_src.as_bytes(),
    )
    .map_err(|e| e.to_string())?;

    let value = Term::Map(
        [
            (TermOrdKey(Term::symbol(":ok")), Term::Bool(true)),
            (
                TermOrdKey(Term::symbol(":target")),
                Term::Str(target_label.to_string()),
            ),
            (
                TermOrdKey(Term::symbol(":pkg")),
                Term::Str(pkg.display().to_string()),
            ),
            (TermOrdKey(Term::symbol(":bundle-h")), Term::Str(bundle_h)),
            (
                TermOrdKey(Term::symbol(":bundle-root")),
                Term::Str(bundle_root.display().to_string()),
            ),
            (TermOrdKey(Term::symbol(":package-h")), Term::Str(package_h)),
            (
                TermOrdKey(Term::symbol(":package-artifact")),
                Term::Str(package_artifact),
            ),
        ]
        .into_iter()
        .collect(),
    );
    Ok(LocalPkgResult {
        kind: "genesis/pkg-build-v0.1",
        log_op: "pkg-build",
        program_hash: hash_term(&value),
        value,
    })
}

pub(crate) fn handle_env(
    profile: &str,
    runtime_backend_override: Option<&str>,
    lock: &Path,
    workspace_file: &Path,
    out_dir: &Path,
) -> Result<LocalPkgResult, String> {
    let ws = WorkspaceConfig::load(workspace_file).map_err(|e| e.to_string())?;
    let l = gc_pkg::GenesisLock::load(lock).map_err(|e| e.to_string())?;
    let prof = ws
        .profiles
        .get(profile)
        .ok_or_else(|| format!("workspace profile `{profile}` not found"))?;
    let active_runtime_backend = crate::active_runtime_backend_profile().to_string();
    let selected_runtime_backend = resolve_env_runtime_backend_profile(
        profile,
        runtime_backend_override,
        prof.runtime_backend.as_deref(),
        ws.defaults.runtime_backend.as_deref(),
    )?;
    let runtime_backend_compatible =
        runtime_backend_profile_is_compatible(&selected_runtime_backend, &active_runtime_backend);
    if !runtime_backend_compatible {
        return Err(format!(
            "profile `{profile}` runtime_backend `{selected_runtime_backend}` is incompatible with active runtime backend profile `{active_runtime_backend}`"
        ));
    }

    let ws_body = ws.to_toml_canonical();
    let lock_body = l.to_toml_canonical();
    let ws_h = blake3::hash(ws_body.as_bytes()).to_hex().to_string();
    let lock_h = blake3::hash(lock_body.as_bytes()).to_hex().to_string();

    let caps_policy = prof
        .caps_policy
        .clone()
        .unwrap_or_else(|| "caps.toml".to_string());
    let caps_policy_path = if Path::new(&caps_policy).is_absolute() {
        Path::new(&caps_policy).to_path_buf()
    } else {
        workspace_file
            .parent()
            .unwrap_or_else(|| Path::new("."))
            .join(&caps_policy)
    };
    if !caps_policy_path.is_file() {
        return Err(format!(
            "profile `{profile}` caps policy file not found: {}",
            caps_policy_path.display()
        ));
    }

    let toolchain_path = resolve_workspace_path(
        workspace_file,
        prof.toolchain
            .as_deref()
            .or(ws.defaults.toolchain.as_deref()),
    );
    if let Some(tp) = &toolchain_path
        && !tp.is_file()
    {
        return Err(format!(
            "profile `{profile}` toolchain file not found: {}",
            tp.display()
        ));
    }

    let members_term = build_env_members_term(workspace_file, &ws.members)?;
    let members_body = gc_coreform::print_term(&members_term) + "\n";
    let members_h = blake3::hash(members_body.as_bytes()).to_hex().to_string();

    let deps_term = build_env_deps_term(workspace_file, &l)?;
    let deps_body = gc_coreform::print_term(&deps_term) + "\n";
    let deps_h = blake3::hash(deps_body.as_bytes()).to_hex().to_string();

    let env_term = Term::Map(
        [
            (TermOrdKey(Term::symbol(":type")), Term::symbol(":gcpm/env")),
            (TermOrdKey(Term::symbol(":v")), Term::Int(2.into())),
            (
                TermOrdKey(Term::symbol(":workspace")),
                Term::Str(ws.workspace.clone()),
            ),
            (
                TermOrdKey(Term::symbol(":workspace-h")),
                Term::Str(ws_h.clone()),
            ),
            (
                TermOrdKey(Term::symbol(":lock-h")),
                Term::Str(lock_h.clone()),
            ),
            (
                TermOrdKey(Term::symbol(":profile")),
                Term::Str(profile.to_string()),
            ),
            (
                TermOrdKey(Term::symbol(":runtime-backend-profile")),
                Term::Str(selected_runtime_backend.clone()),
            ),
            (
                TermOrdKey(Term::symbol(":active-runtime-backend-profile")),
                Term::Str(active_runtime_backend.clone()),
            ),
            (
                TermOrdKey(Term::symbol(":runtime-backend-compatible")),
                Term::Bool(runtime_backend_compatible),
            ),
            (
                TermOrdKey(Term::symbol(":policy")),
                prof.policy
                    .clone()
                    .map(Term::Str)
                    .unwrap_or(Term::symbol(":none")),
            ),
            (
                TermOrdKey(Term::symbol(":registry")),
                prof.registry
                    .clone()
                    .map(Term::Str)
                    .unwrap_or(Term::symbol(":none")),
            ),
            (
                TermOrdKey(Term::symbol(":toolchain")),
                prof.toolchain
                    .clone()
                    .map(Term::Str)
                    .unwrap_or(Term::symbol(":none")),
            ),
            (
                TermOrdKey(Term::symbol(":caps-policy")),
                Term::Str(caps_policy_path.display().to_string()),
            ),
            (TermOrdKey(Term::symbol(":members-h")), Term::Str(members_h)),
            (TermOrdKey(Term::symbol(":deps-h")), Term::Str(deps_h)),
            (
                TermOrdKey(Term::symbol(":toolchain-h")),
                toolchain_path
                    .as_ref()
                    .map(|p| hash_file_hex(p))
                    .transpose()?
                    .map(Term::Str)
                    .unwrap_or_else(|| Term::symbol(":none")),
            ),
        ]
        .into_iter()
        .collect(),
    );
    let env_body = gc_coreform::print_term(&env_term) + "\n";
    let env_h = blake3::hash(env_body.as_bytes()).to_hex().to_string();
    let env_root = out_dir.join(&env_h);
    std::fs::create_dir_all(&env_root).map_err(|e| e.to_string())?;

    write_if_same_or_new(&env_root.join("env.gcenv"), env_body.as_bytes())
        .map_err(|e| e.to_string())?;
    write_if_same_or_new(&env_root.join("members.gc"), members_body.as_bytes())
        .map_err(|e| e.to_string())?;
    write_if_same_or_new(&env_root.join("deps.gc"), deps_body.as_bytes())
        .map_err(|e| e.to_string())?;
    write_if_same_or_new(&env_root.join("workspace.toml"), ws_body.as_bytes())
        .map_err(|e| e.to_string())?;
    write_if_same_or_new(&env_root.join("genesis.lock"), lock_body.as_bytes())
        .map_err(|e| e.to_string())?;

    let caps_policy_bytes = std::fs::read(&caps_policy_path).map_err(|e| e.to_string())?;
    let caps_policy_h = blake3::hash(&caps_policy_bytes).to_hex().to_string();
    write_if_same_or_new(&env_root.join("caps-policy.toml"), &caps_policy_bytes)
        .map_err(|e| e.to_string())?;

    if let Some(tp) = &toolchain_path {
        let bytes = std::fs::read(tp).map_err(|e| e.to_string())?;
        write_if_same_or_new(&env_root.join("toolchain.gc"), &bytes).map_err(|e| e.to_string())?;
    }

    let runtime_backend_contract = RuntimeBackendContract {
        selected: &selected_runtime_backend,
        active: &active_runtime_backend,
        compatible: runtime_backend_compatible,
    };
    let profile_term = build_env_profile_term(
        profile,
        &ws,
        prof,
        &caps_policy_path,
        &toolchain_path,
        runtime_backend_contract,
    );
    let profile_body = gc_coreform::print_term(&profile_term) + "\n";
    let profile_h = blake3::hash(profile_body.as_bytes()).to_hex().to_string();
    write_if_same_or_new(&env_root.join("profile.gc"), profile_body.as_bytes())
        .map_err(|e| e.to_string())?;

    let provenance_term = Term::Map(
        [
            (
                TermOrdKey(Term::symbol(":type")),
                Term::symbol(":gcpm/env-provenance"),
            ),
            (TermOrdKey(Term::symbol(":v")), Term::Int(1.into())),
            (TermOrdKey(Term::symbol(":env-h")), Term::Str(env_h.clone())),
            (
                TermOrdKey(Term::symbol(":workspace-file")),
                Term::Str(workspace_file.display().to_string()),
            ),
            (
                TermOrdKey(Term::symbol(":lock-file")),
                Term::Str(lock.display().to_string()),
            ),
            (
                TermOrdKey(Term::symbol(":generated-by")),
                Term::Str(format!("genesis {}", env!("CARGO_PKG_VERSION"))),
            ),
        ]
        .into_iter()
        .collect(),
    );
    let provenance_body = gc_coreform::print_term(&provenance_term) + "\n";
    write_if_same_or_new(&env_root.join("provenance.gc"), provenance_body.as_bytes())
        .map_err(|e| e.to_string())?;

    let value = Term::Map(
        [
            (TermOrdKey(Term::symbol(":ok")), Term::Bool(true)),
            (
                TermOrdKey(Term::symbol(":profile")),
                Term::Str(profile.to_string()),
            ),
            (
                TermOrdKey(Term::symbol(":caps-policy")),
                Term::Str(caps_policy_path.display().to_string()),
            ),
            (
                TermOrdKey(Term::symbol(":runtime-backend-profile")),
                Term::Str(selected_runtime_backend),
            ),
            (
                TermOrdKey(Term::symbol(":active-runtime-backend-profile")),
                Term::Str(active_runtime_backend),
            ),
            (
                TermOrdKey(Term::symbol(":runtime-backend-compatible")),
                Term::Bool(runtime_backend_compatible),
            ),
            (
                TermOrdKey(Term::symbol(":caps-policy-h")),
                Term::Str(caps_policy_h),
            ),
            (TermOrdKey(Term::symbol(":env-h")), Term::Str(env_h)),
            (TermOrdKey(Term::symbol(":profile-h")), Term::Str(profile_h)),
            (
                TermOrdKey(Term::symbol(":env-root")),
                Term::Str(env_root.display().to_string()),
            ),
        ]
        .into_iter()
        .collect(),
    );
    Ok(LocalPkgResult {
        kind: "genesis/pkg-env-v0.1",
        log_op: "pkg-env",
        program_hash: hash_term(&value),
        value,
    })
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

#[derive(Clone, Copy)]
struct BuildTargetProfile {
    runtime: &'static str,
    host_profile: &'static str,
    artifact_format: &'static str,
}

fn normalize_build_target(target: &str) -> Result<&'static str, String> {
    match target.trim().to_ascii_lowercase().as_str() {
        "web" => Ok("web"),
        "desktop" => Ok("desktop"),
        "service" => Ok("service"),
        "ios" => Ok("ios"),
        "android" => Ok("android"),
        "edge" => Ok("edge"),
        "service-runtime" => Ok("service-runtime"),
        other => Err(format!(
            "invalid build target `{other}`; expected one of web|desktop|service|ios|android|edge|service-runtime"
        )),
    }
}

fn build_target_profile(target: &str) -> Result<BuildTargetProfile, String> {
    match target {
        "web" => Ok(BuildTargetProfile {
            runtime: "wasm32-unknown-unknown",
            host_profile: "browser",
            artifact_format: "wasm-bundle-v1",
        }),
        "desktop" => Ok(BuildTargetProfile {
            runtime: "native",
            host_profile: "desktop",
            artifact_format: "native-bundle-v1",
        }),
        "service" => Ok(BuildTargetProfile {
            runtime: "native",
            host_profile: "headless",
            artifact_format: "service-bundle-v1",
        }),
        "ios" => Ok(BuildTargetProfile {
            runtime: "native",
            host_profile: "mobile-ios",
            artifact_format: "ios-app-bundle-v1",
        }),
        "android" => Ok(BuildTargetProfile {
            runtime: "native",
            host_profile: "mobile-android",
            artifact_format: "android-app-bundle-v1",
        }),
        "edge" => Ok(BuildTargetProfile {
            runtime: "wasm32-wasi-preview2",
            host_profile: "edge-runtime",
            artifact_format: "edge-wasi-bundle-v1",
        }),
        "service-runtime" => Ok(BuildTargetProfile {
            runtime: "wasm32-wasi-preview2",
            host_profile: "service-runtime",
            artifact_format: "service-runtime-bundle-v1",
        }),
        other => Err(format!(
            "invalid build target `{other}`; expected one of web|desktop|service|ios|android|edge|service-runtime"
        )),
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

fn resolve_workspace_path(workspace_file: &Path, raw: Option<&str>) -> Option<PathBuf> {
    let raw = raw?;
    let p = PathBuf::from(raw);
    if p.is_absolute() {
        Some(p)
    } else {
        Some(
            workspace_file
                .parent()
                .unwrap_or_else(|| Path::new("."))
                .join(p),
        )
    }
}

fn hash_file_hex(path: &Path) -> Result<String, String> {
    let bytes = std::fs::read(path).map_err(|e| e.to_string())?;
    Ok(blake3::hash(&bytes).to_hex().to_string())
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

fn workspace_store_dir(workspace_file: &Path) -> PathBuf {
    workspace_file
        .parent()
        .unwrap_or_else(|| Path::new("."))
        .join(".genesis")
        .join("store")
}

#[derive(Clone, Copy)]
struct RuntimeBackendContract<'a> {
    selected: &'a str,
    active: &'a str,
    compatible: bool,
}

fn build_env_profile_term(
    profile_name: &str,
    ws: &WorkspaceConfig,
    prof: &gc_pkg::WorkspaceProfile,
    caps_policy_path: &Path,
    toolchain_path: &Option<PathBuf>,
    runtime_backend: RuntimeBackendContract<'_>,
) -> Term {
    Term::Map(
        [
            (
                TermOrdKey(Term::symbol(":type")),
                Term::symbol(":gcpm/profile"),
            ),
            (TermOrdKey(Term::symbol(":v")), Term::Int(1.into())),
            (
                TermOrdKey(Term::symbol(":workspace")),
                Term::Str(ws.workspace.clone()),
            ),
            (
                TermOrdKey(Term::symbol(":profile")),
                Term::Str(profile_name.to_string()),
            ),
            (
                TermOrdKey(Term::symbol(":policy")),
                prof.policy
                    .clone()
                    .or(ws.defaults.policy.clone())
                    .map(Term::Str)
                    .unwrap_or_else(|| Term::symbol(":none")),
            ),
            (
                TermOrdKey(Term::symbol(":registry")),
                prof.registry
                    .clone()
                    .or(ws.defaults.registry.clone())
                    .map(Term::Str)
                    .unwrap_or_else(|| Term::symbol(":none")),
            ),
            (
                TermOrdKey(Term::symbol(":caps-policy")),
                Term::Str(caps_policy_path.display().to_string()),
            ),
            (
                TermOrdKey(Term::symbol(":runtime-backend-profile")),
                Term::Str(runtime_backend.selected.to_string()),
            ),
            (
                TermOrdKey(Term::symbol(":active-runtime-backend-profile")),
                Term::Str(runtime_backend.active.to_string()),
            ),
            (
                TermOrdKey(Term::symbol(":runtime-backend-compatible")),
                Term::Bool(runtime_backend.compatible),
            ),
            (
                TermOrdKey(Term::symbol(":toolchain")),
                toolchain_path
                    .as_ref()
                    .map(|p| Term::Str(p.display().to_string()))
                    .unwrap_or_else(|| Term::symbol(":none")),
            ),
        ]
        .into_iter()
        .collect(),
    )
}

fn resolve_env_runtime_backend_profile(
    profile_name: &str,
    runtime_backend_override: Option<&str>,
    profile_runtime_backend: Option<&str>,
    default_runtime_backend: Option<&str>,
) -> Result<String, String> {
    let raw = runtime_backend_override
        .or(profile_runtime_backend)
        .or(default_runtime_backend)
        .unwrap_or(RUNTIME_BACKEND_HEADLESS);
    normalize_runtime_backend_profile(raw).ok_or_else(|| {
        format!(
            "profile `{profile_name}` has invalid runtime_backend `{raw}`; expected one of headless|gpu|gfx|backend (or profile-* aliases)"
        )
    })
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
    if path.is_file() {
        let old = std::fs::read(path)?;
        if old == bytes {
            return Ok(());
        }
        return Err(std::io::Error::new(
            std::io::ErrorKind::AlreadyExists,
            format!(
                "refusing to overwrite immutable env artifact at {}",
                path.display()
            ),
        ));
    }
    atomic_write_text(path, bytes)
}
