use super::*;

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
    let wasi_http_bridge_plan = build_wasi_http_bridge_plan(workspace_file, &ws, prof)?;

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
            (
                TermOrdKey(Term::symbol(":wasi-http-bridge-root")),
                Term::Str(wasi_http_bridge_plan.root.display().to_string()),
            ),
            (
                TermOrdKey(Term::symbol(":wasi-http-bridge-remote")),
                wasi_http_bridge_plan
                    .remote
                    .as_ref()
                    .map(|s| Term::Str(s.clone()))
                    .unwrap_or_else(|| Term::symbol(":none")),
            ),
            (
                TermOrdKey(Term::symbol(":wasi-http-bridge-remote-root")),
                wasi_http_bridge_plan
                    .remote_root
                    .as_ref()
                    .map(|p| Term::Str(p.display().to_string()))
                    .unwrap_or_else(|| Term::symbol(":none")),
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
    materialize_wasi_http_bridge_plan(
        &wasi_http_bridge_plan,
        &env_root,
        profile,
        &selected_runtime_backend,
    )?;

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
        &wasi_http_bridge_plan,
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
                TermOrdKey(Term::symbol(":wasi-http-bridge-root")),
                Term::Str(wasi_http_bridge_plan.root.display().to_string()),
            ),
            (
                TermOrdKey(Term::symbol(":wasi-http-bridge-remote")),
                wasi_http_bridge_plan
                    .remote
                    .as_ref()
                    .map(|s| Term::Str(s.clone()))
                    .unwrap_or_else(|| Term::symbol(":none")),
            ),
            (
                TermOrdKey(Term::symbol(":wasi-http-bridge-remote-root")),
                wasi_http_bridge_plan
                    .remote_root
                    .as_ref()
                    .map(|p| Term::Str(p.display().to_string()))
                    .unwrap_or_else(|| Term::symbol(":none")),
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
    wasi_http_bridge_plan: &WasiHttpBridgePlan,
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
                TermOrdKey(Term::symbol(":wasi-http-bridge-root")),
                Term::Str(wasi_http_bridge_plan.root.display().to_string()),
            ),
            (
                TermOrdKey(Term::symbol(":wasi-http-bridge-remote")),
                wasi_http_bridge_plan
                    .remote
                    .as_ref()
                    .map(|s| Term::Str(s.clone()))
                    .unwrap_or_else(|| Term::symbol(":none")),
            ),
            (
                TermOrdKey(Term::symbol(":wasi-http-bridge-remote-root")),
                wasi_http_bridge_plan
                    .remote_root
                    .as_ref()
                    .map(|p| Term::Str(p.display().to_string()))
                    .unwrap_or_else(|| Term::symbol(":none")),
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

struct WasiHttpBridgePlan {
    root: PathBuf,
    remote: Option<String>,
    remote_root: Option<PathBuf>,
}

fn build_wasi_http_bridge_plan(
    workspace_file: &Path,
    ws: &WorkspaceConfig,
    prof: &gc_pkg::WorkspaceProfile,
) -> Result<WasiHttpBridgePlan, String> {
    let workspace_root = workspace_file
        .parent()
        .unwrap_or_else(|| Path::new("."))
        .to_path_buf();
    let root = workspace_root
        .join(".genesis")
        .join("runtime")
        .join("wasi-http-bridge");
    let remote = prof
        .registry
        .clone()
        .or_else(|| ws.defaults.registry.clone());

    let remote_root = match remote.as_deref() {
        Some(r) if r.starts_with("http://") || r.starts_with("https://") => Some(
            gc_registry::wasi_http_bridge_resolve_remote_root(&root, r)
                .map_err(|e| format!("resolve wasi http bridge root for registry `{r}`: {e}"))?,
        ),
        _ => None,
    };

    Ok(WasiHttpBridgePlan {
        root,
        remote,
        remote_root,
    })
}

fn materialize_wasi_http_bridge_plan(
    plan: &WasiHttpBridgePlan,
    env_root: &Path,
    profile: &str,
    runtime_backend: &str,
) -> Result<(), String> {
    std::fs::create_dir_all(plan.root.join("http")).map_err(|e| e.to_string())?;
    std::fs::create_dir_all(plan.root.join("https")).map_err(|e| e.to_string())?;
    if let Some(remote_root) = &plan.remote_root {
        std::fs::create_dir_all(remote_root).map_err(|e| e.to_string())?;
    }

    let descriptor = Term::Map(
        [
            (
                TermOrdKey(Term::symbol(":type")),
                Term::symbol(":gcpm/wasi-http-bridge-runtime"),
            ),
            (TermOrdKey(Term::symbol(":v")), Term::Int(1.into())),
            (
                TermOrdKey(Term::symbol(":profile")),
                Term::Str(profile.to_string()),
            ),
            (
                TermOrdKey(Term::symbol(":runtime-backend-profile")),
                Term::Str(runtime_backend.to_string()),
            ),
            (
                TermOrdKey(Term::symbol(":root")),
                Term::Str(plan.root.display().to_string()),
            ),
            (
                TermOrdKey(Term::symbol(":remote")),
                plan.remote
                    .as_ref()
                    .map(|s| Term::Str(s.clone()))
                    .unwrap_or_else(|| Term::symbol(":none")),
            ),
            (
                TermOrdKey(Term::symbol(":remote-root")),
                plan.remote_root
                    .as_ref()
                    .map(|p| Term::Str(p.display().to_string()))
                    .unwrap_or_else(|| Term::symbol(":none")),
            ),
        ]
        .into_iter()
        .collect(),
    );
    let descriptor_body = gc_coreform::print_term(&descriptor) + "\n";
    write_if_same_or_new(
        &env_root.join("wasi-http-bridge.gc"),
        descriptor_body.as_bytes(),
    )
    .map_err(|e| e.to_string())?;
    atomic_write_text(&plan.root.join("runtime.gc"), descriptor_body.as_bytes())
        .map_err(|e| e.to_string())?;
    Ok(())
}

pub(super) fn resolve_env_runtime_backend_profile(
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

pub(super) fn write_if_same_or_new(path: &Path, bytes: &[u8]) -> std::io::Result<()> {
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
