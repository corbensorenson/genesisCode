use super::*;

pub(super) fn resolved_selfhost_bootstrap_mode(cli: &Cli) -> SelfhostBootstrapMode {
    match cli.selfhost_bootstrap {
        SelfhostBootstrapArg::ArtifactOnly => SelfhostBootstrapMode::ArtifactOnly,
        SelfhostBootstrapArg::ArtifactPreferred => SelfhostBootstrapMode::ArtifactPreferred,
        SelfhostBootstrapArg::Embedded => SelfhostBootstrapMode::Embedded,
    }
}

pub(super) const SELFHOST_TOOLCHAIN_ARTIFACT_ENV: &str = "GENESIS_SELFHOST_TOOLCHAIN_ARTIFACT";
pub(super) const DEFAULT_SELFHOST_TOOLCHAIN_ARTIFACT_REL: &str = ".genesis/selfhost/toolchain.gc";
pub(super) const WORKSPACE_SELFHOST_TOOLCHAIN_ARTIFACT_REL: &str = "selfhost/toolchain.gc";
pub(super) const DASHBOARD_MARKDOWN_DEFAULT_REL: &str = "docs/status/SELFHOST_CUTOVER.md";
pub(super) const DASHBOARD_STORE_DEFAULT_REL: &str = ".genesis/store";

pub(super) fn parse_truthy_env_flag(s: &str) -> bool {
    matches!(
        s.trim().to_ascii_lowercase().as_str(),
        "1" | "true" | "yes" | "on"
    )
}

pub(super) fn selfhost_only_enabled(cli: &Cli) -> bool {
    cli.selfhost_only
        || std::env::var("GENESIS_SELFHOST_ONLY")
            .map(|v| parse_truthy_env_flag(&v))
            .unwrap_or(false)
}

pub(super) fn rust_engine_compat_enabled() -> bool {
    matches!(runtime_profile(), RuntimeProfile::ParityHarness)
}

pub(super) fn frontend_is_rust(frontend: &gc_obligations::CoreformFrontend) -> bool {
    gc_obligations::coreform_frontend_is_rust(frontend)
}

pub(super) fn non_artifact_bootstrap_modes_allowed() -> bool {
    matches!(runtime_profile(), RuntimeProfile::ParityHarness)
}

pub(super) fn implicit_selfhost_artifact_discovery_allowed() -> bool {
    // Implicit artifact discovery is reserved for explicit parity harness workflows.
    matches!(runtime_profile(), RuntimeProfile::ParityHarness)
}

pub(super) fn bootstrap_mode_label(mode: SelfhostBootstrapMode) -> &'static str {
    match mode {
        SelfhostBootstrapMode::ArtifactOnly => "artifact-only",
        SelfhostBootstrapMode::ArtifactPreferred => "artifact-preferred",
        SelfhostBootstrapMode::Embedded => "embedded",
    }
}

pub(super) fn enforce_bootstrap_mode_allowed_with_flag(
    mode: SelfhostBootstrapMode,
    context: &str,
    allow_non_artifact_bootstrap_modes: bool,
) -> Result<(), CliError> {
    if mode == SelfhostBootstrapMode::ArtifactOnly || allow_non_artifact_bootstrap_modes {
        return Ok(());
    }
    Err(cli_err(
        EX_VERIFY,
        "selfhost/bootstrap-mode",
        format!(
            "{context}: `--selfhost-bootstrap {}` is development-only; release profile requires --selfhost-bootstrap artifact-only",
            bootstrap_mode_label(mode)
        ),
    ))
}

pub(super) fn enforce_bootstrap_mode_allowed(
    mode: SelfhostBootstrapMode,
    context: &str,
) -> Result<(), CliError> {
    enforce_bootstrap_mode_allowed_with_flag(mode, context, non_artifact_bootstrap_modes_allowed())
}

pub(super) fn default_selfhost_artifact_path() -> PathBuf {
    std::env::current_dir()
        .unwrap_or_else(|_| PathBuf::from("."))
        .join(DEFAULT_SELFHOST_TOOLCHAIN_ARTIFACT_REL)
}

pub(super) fn workspace_selfhost_artifact_path() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("../..")
        .join(WORKSPACE_SELFHOST_TOOLCHAIN_ARTIFACT_REL)
}

pub(super) fn resolved_selfhost_artifact_for_frontend(cli: &Cli) -> Option<PathBuf> {
    if let Some(p) = resolved_explicit_selfhost_artifact(cli) {
        return Some(p);
    }
    let p = default_selfhost_artifact_path();
    if p.is_file() {
        return Some(p);
    }
    let wp = workspace_selfhost_artifact_path();
    if wp.is_file() {
        return Some(wp);
    }
    None
}

pub(super) fn resolved_explicit_selfhost_artifact(cli: &Cli) -> Option<PathBuf> {
    if let Some(p) = cli.selfhost_artifact.clone() {
        return Some(p);
    }
    if let Ok(raw) = std::env::var(SELFHOST_TOOLCHAIN_ARTIFACT_ENV) {
        let trimmed = raw.trim();
        if !trimmed.is_empty() {
            return Some(PathBuf::from(trimmed));
        }
    }
    if let Some(p) = resolved_workspace_pinned_selfhost_artifact() {
        return Some(p);
    }
    None
}

fn workspace_descriptor_candidates() -> Vec<PathBuf> {
    vec![
        std::env::current_dir()
            .unwrap_or_else(|_| PathBuf::from("."))
            .join("genesis.workspace.toml"),
        PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .join("../..")
            .join("genesis.workspace.toml"),
    ]
}

fn resolve_workspace_toolchain_path(workspace_file: &Path, toolchain: &str) -> Option<PathBuf> {
    let trimmed = toolchain.trim();
    if trimmed.is_empty() {
        return None;
    }
    let candidate = PathBuf::from(trimmed);
    let path = if candidate.is_absolute() {
        candidate
    } else {
        workspace_file
            .parent()
            .unwrap_or_else(|| Path::new("."))
            .join(candidate)
    };
    canonicalize_if_exists(&path)
}

fn resolved_workspace_pinned_selfhost_artifact() -> Option<PathBuf> {
    let mut seen: BTreeSet<PathBuf> = BTreeSet::new();
    for ws in workspace_descriptor_candidates() {
        let ws = ws.canonicalize().unwrap_or(ws);
        if !seen.insert(ws.clone()) {
            continue;
        }
        if !ws.is_file() {
            continue;
        }
        let Ok(cfg) = gc_pkg::WorkspaceConfig::load(&ws) else {
            continue;
        };
        if let Some(toolchain) = cfg.defaults.toolchain
            && let Some(path) = resolve_workspace_toolchain_path(&ws, &toolchain)
        {
            return Some(path);
        }
    }
    None
}

fn canonicalize_if_exists(path: &Path) -> Option<PathBuf> {
    if !path.is_file() {
        return None;
    }
    Some(
        std::fs::canonicalize(path)
            .ok()
            .unwrap_or_else(|| path.to_path_buf()),
    )
}

pub(super) fn require_explicit_selfhost_artifact(
    cli: &Cli,
    context: &str,
) -> Result<PathBuf, CliError> {
    let Some(path) = resolved_explicit_selfhost_artifact(cli) else {
        return Err(cli_err(
            EX_VERIFY,
            "selfhost/artifact-required",
            format!(
                "{context}: explicit selfhost artifact required; pass --selfhost-artifact <file> or set {SELFHOST_TOOLCHAIN_ARTIFACT_ENV}"
            ),
        ));
    };
    canonicalize_if_exists(&path).ok_or_else(|| {
        cli_err(
            EX_PARSE,
            "selfhost/artifact-missing",
            format!(
                "{context}: selfhost artifact file does not exist: {}",
                path.display()
            ),
        )
    })
}

fn artifact_hash_hex(path: &Path) -> Option<String> {
    let bytes = std::fs::read(path).ok()?;
    Some(blake3::hash(&bytes).to_hex().to_string())
}

pub(super) fn selfhost_artifact_identity_for_engine(
    cli: &Cli,
    engine: FmtEngine,
) -> serde_json::Value {
    if engine != FmtEngine::Selfhost {
        return serde_json::Value::Null;
    }
    let path = require_explicit_selfhost_artifact(cli, "runtime").ok();
    match path {
        Some(path) => serde_json::json!({
            "path": path.display().to_string(),
            "hash": artifact_hash_hex(&path),
            "source": "explicit",
        }),
        None => serde_json::Value::Null,
    }
}

pub(super) fn resolved_coreform_frontend(
    cli: &Cli,
) -> Result<gc_obligations::CoreformFrontend, CliError> {
    let strict = selfhost_only_enabled(cli);
    let selected = cli
        .coreform_frontend
        .unwrap_or(CoreformFrontendArg::Selfhost);
    match selected {
        CoreformFrontendArg::Rust => {
            if strict {
                return Err(cli_err(
                    EX_VERIFY,
                    "selfhost-only/frontend",
                    "selfhost-only mode requires --coreform-frontend selfhost",
                ));
            }
            if !rust_engine_compat_enabled() {
                let msg = if cfg!(debug_assertions) {
                    "`--coreform-frontend rust` is disabled in this binary; use dedicated parity harness binaries (`genesis_parity` / `genesis_wasi_parity`) for Rust frontend comparisons".to_string()
                } else {
                    "`--coreform-frontend rust` is disabled in production binaries; use dedicated parity harness binaries (`genesis_parity` / `genesis_wasi_parity`) for Rust frontend comparisons".to_string()
                };
                return Err(cli_err(EX_VERIFY, "engine/rust-disabled", msg));
            }
            Ok(gc_obligations::rust_coreform_frontend())
        }
        CoreformFrontendArg::Selfhost => {
            let mode = resolved_selfhost_bootstrap_mode(cli);
            enforce_bootstrap_mode_allowed(mode, "coreform frontend")?;
            if strict && mode != SelfhostBootstrapMode::ArtifactOnly {
                return Err(cli_err(
                    EX_VERIFY,
                    "selfhost-only/bootstrap",
                    "selfhost-only mode requires --selfhost-bootstrap artifact-only",
                ));
            }
            let artifact = if implicit_selfhost_artifact_discovery_allowed() {
                resolved_selfhost_artifact_for_frontend(cli)
            } else {
                Some(require_explicit_selfhost_artifact(
                    cli,
                    "coreform frontend",
                )?)
            };
            Ok(gc_obligations::CoreformFrontend::Selfhost(
                gc_obligations::SelfhostFrontendConfig {
                    bootstrap_mode: mode,
                    artifact,
                },
            ))
        }
    }
}

pub(super) fn coreform_frontend_json(
    frontend: &gc_obligations::CoreformFrontend,
) -> serde_json::Value {
    if frontend_is_rust(frontend) {
        serde_json::json!({
            "name": "rust"
        })
    } else {
        let gc_obligations::CoreformFrontend::Selfhost(cfg) = frontend else {
            unreachable!("frontend dispatch drift: expected selfhost variant");
        };
        serde_json::json!({
            "name": "selfhost",
            "bootstrap_mode": match cfg.bootstrap_mode {
                SelfhostBootstrapMode::ArtifactOnly => "artifact-only",
                SelfhostBootstrapMode::ArtifactPreferred => "artifact-preferred",
                SelfhostBootstrapMode::Embedded => "embedded",
            },
            "artifact": cfg.artifact.as_ref().map(|p| p.display().to_string()),
        })
    }
}

pub(super) fn coreform_frontend_for_engine(
    cli: &Cli,
    engine: FmtEngine,
) -> Result<gc_obligations::CoreformFrontend, CliError> {
    match engine {
        FmtEngine::Rust => Ok(gc_obligations::rust_coreform_frontend()),
        FmtEngine::Selfhost => {
            let mode = resolved_selfhost_bootstrap_mode(cli);
            enforce_bootstrap_mode_allowed(mode, "engine frontend")?;
            let artifact = Some(require_explicit_selfhost_artifact(cli, "engine frontend")?);
            Ok(gc_obligations::CoreformFrontend::Selfhost(
                gc_obligations::SelfhostFrontendConfig {
                    bootstrap_mode: mode,
                    artifact,
                },
            ))
        }
    }
}

pub(super) fn rust_engine_disabled_message(cmd_name: &str) -> String {
    if cfg!(debug_assertions) {
        format!(
            "`--engine rust` is disabled in this binary for `{cmd_name}`; use dedicated parity harness binaries (`genesis_parity` / `genesis_wasi_parity`) for Rust engine comparisons"
        )
    } else {
        format!(
            "`--engine rust` is disabled in production binaries for `{cmd_name}`; use dedicated parity harness binaries (`genesis_parity` / `genesis_wasi_parity`) for Rust engine comparisons"
        )
    }
}

pub(super) fn resolved_engine(
    cli: &Cli,
    cmd_name: &str,
    engine: Option<FmtEngine>,
) -> Result<FmtEngine, CliError> {
    if selfhost_only_enabled(cli)
        && resolved_selfhost_bootstrap_mode(cli) != SelfhostBootstrapMode::ArtifactOnly
    {
        return Err(cli_err(
            EX_VERIFY,
            "selfhost-only/bootstrap",
            "selfhost-only mode requires --selfhost-bootstrap artifact-only",
        ));
    }
    enforce_selfhost_engine(cli, cmd_name, engine)?;
    if let Some(e) = engine {
        if e == FmtEngine::Rust && !rust_engine_compat_enabled() {
            return Err(cli_err(
                EX_VERIFY,
                "compat/rust-engine-disabled",
                rust_engine_disabled_message(cmd_name),
            ));
        }
        if e == FmtEngine::Selfhost {
            if resolved_selfhost_bootstrap_mode(cli) != SelfhostBootstrapMode::ArtifactOnly {
                return Err(cli_err(
                    EX_VERIFY,
                    "selfhost/bootstrap",
                    format!(
                        "{cmd_name}: selfhost runtime requires --selfhost-bootstrap artifact-only"
                    ),
                ));
            }
            let _ = require_explicit_selfhost_artifact(cli, cmd_name)?;
        }
        return Ok(e);
    }
    if resolved_selfhost_bootstrap_mode(cli) != SelfhostBootstrapMode::ArtifactOnly {
        return Err(cli_err(
            EX_VERIFY,
            "selfhost/bootstrap",
            format!("{cmd_name}: selfhost runtime requires --selfhost-bootstrap artifact-only"),
        ));
    }
    let _ = require_explicit_selfhost_artifact(cli, cmd_name)?;
    Ok(FmtEngine::Selfhost)
}

pub(super) fn load_selfhost_toolchain(
    cli: &Cli,
    ctx: &mut EvalCtx,
    env: &mut gc_kernel::Env,
) -> Result<(), CliError> {
    let mode = resolved_selfhost_bootstrap_mode(cli);
    enforce_bootstrap_mode_allowed(mode, "selfhost runtime")?;
    if selfhost_only_enabled(cli) && mode != SelfhostBootstrapMode::ArtifactOnly {
        return Err(cli_err(
            EX_VERIFY,
            "selfhost-only/bootstrap",
            "selfhost-only mode requires --selfhost-bootstrap artifact-only",
        ));
    }
    let artifact = if implicit_selfhost_artifact_discovery_allowed() {
        resolved_selfhost_artifact_for_frontend(cli)
    } else {
        Some(require_explicit_selfhost_artifact(cli, "selfhost runtime")?)
    };
    load_selfhost_coreform_toolchain_v1_with_mode(ctx, env, mode, artifact.as_deref())
        .map_err(|e| cli_err(EX_INTERNAL, "selfhost/init", format!("{e}")))
}

pub(super) fn load_runtime_selfhost_toolchain(
    cli: &Cli,
    ctx: &mut EvalCtx,
    env: &mut gc_kernel::Env,
) -> Result<(), CliError> {
    let mode = resolved_selfhost_bootstrap_mode(cli);
    if mode != SelfhostBootstrapMode::ArtifactOnly {
        return Err(cli_err(
            EX_VERIFY,
            "selfhost/bootstrap",
            "selfhost runtime requires --selfhost-bootstrap artifact-only",
        ));
    }
    let artifact = require_explicit_selfhost_artifact(cli, "selfhost runtime")?;
    load_selfhost_coreform_toolchain_v1_with_mode(ctx, env, mode, Some(artifact.as_path()))
        .map_err(|e| cli_err(EX_INTERNAL, "selfhost/init", format!("{e}")))
}

pub(super) fn maybe_embedded_bootstrap_mode() -> SelfhostBootstrapMode {
    if embedded_bootstrap_available() && non_artifact_bootstrap_modes_allowed() {
        SelfhostBootstrapMode::Embedded
    } else {
        SelfhostBootstrapMode::ArtifactOnly
    }
}

pub(super) fn coreform_frontend_for_engine_json(
    cli: &Cli,
    engine: FmtEngine,
) -> Result<serde_json::Value, CliError> {
    Ok(coreform_frontend_json(&coreform_frontend_for_engine(
        cli, engine,
    )?))
}
pub(super) fn enforce_selfhost_engine(
    cli: &Cli,
    cmd_name: &str,
    engine: Option<FmtEngine>,
) -> Result<(), CliError> {
    if !selfhost_only_enabled(cli) {
        return Ok(());
    }
    if engine != Some(FmtEngine::Rust) {
        return Ok(());
    }
    Err(cli_err(
        EX_VERIFY,
        "selfhost-only/engine",
        format!(
            "selfhost-only mode requires --engine selfhost for `{cmd_name}` (got --engine rust)"
        ),
    ))
}

pub(super) fn is_legacy_high_level_semantic_op(op: &str) -> bool {
    // Selfhost cutover is complete for pkg/vcs/gc/gpk command semantics. In selfhost-only mode
    // these high-level semantic ops must not execute at runtime.
    op.starts_with("core/pkg::")
        || op.starts_with("core/vcs::")
        || op.starts_with("core/gc::")
        || op.starts_with("core/gpk::")
}

pub(super) fn collect_legacy_high_level_semantic_ops(log: &EffectLog) -> Vec<String> {
    let mut out: BTreeSet<String> = BTreeSet::new();
    for entry in &log.entries {
        if is_legacy_high_level_semantic_op(&entry.op) {
            out.insert(entry.op.clone());
        }
    }
    out.into_iter().collect()
}

pub(super) fn enforce_no_legacy_semantic_fallback_in_selfhost_only(
    cli: &Cli,
    cmd_name: &str,
    log: &EffectLog,
) -> Result<(), CliError> {
    if !selfhost_only_enabled(cli) {
        return Ok(());
    }
    let found = collect_legacy_high_level_semantic_ops(log);
    if found.is_empty() {
        return Ok(());
    }
    Err(cli_err(
        EX_VERIFY,
        "selfhost-only/legacy-semantic-fallback",
        format!(
            "selfhost-only mode detected legacy semantic fallback while running `{cmd_name}`: {}",
            found.join(", ")
        ),
    ))
}

pub(super) fn enforce_selfhost_only_cmd(cli: &Cli, _flavor: Flavor) -> Result<(), CliError> {
    if !selfhost_only_enabled(cli) {
        return Ok(());
    }

    match &cli.cmd {
        Cmd::Fmt { engine, .. } => enforce_selfhost_engine(cli, "fmt", *engine),
        Cmd::Eval { engine, .. } => enforce_selfhost_engine(cli, "eval", *engine),
        Cmd::Explain { engine, .. } => enforce_selfhost_engine(cli, "explain", *engine),
        Cmd::Run { engine, .. } => enforce_selfhost_engine(cli, "run", *engine),
        Cmd::Replay { engine, .. } => enforce_selfhost_engine(cli, "replay", *engine),
        Cmd::Optimize { engine, .. } => enforce_selfhost_engine(cli, "optimize", *engine),
        Cmd::Typecheck { .. } => Ok(()),
        Cmd::Test { .. } => Ok(()),
        Cmd::ApplyPatch { .. } => Ok(()),
        Cmd::SemanticEdit { .. } => Ok(()),
        Cmd::Pack { .. } => Ok(()),
        Cmd::Store { .. } => Ok(()),
        Cmd::Refs { .. } => Ok(()),
        Cmd::Commit { .. } => Ok(()),
        Cmd::Pkg { .. } => Ok(()),
        Cmd::Policy { .. } => Ok(()),
        Cmd::Sync { .. } => Ok(()),
        Cmd::Gc { .. } => Ok(()),
        Cmd::SelfhostArtifact { .. } => Ok(()),
        Cmd::Keygen { .. } => Ok(()),
        Cmd::Sign { .. } => Ok(()),
        Cmd::TransparencyVerify { .. } => Ok(()),
        Cmd::Verify { .. } => Ok(()),
        Cmd::SelfhostDashboard { .. } => Ok(()),
        Cmd::Warm { .. } => Ok(()),
        Cmd::Vcs {
            cmd: VcsCmd::Hash { engine, .. },
            ..
        } => enforce_selfhost_engine(cli, "vcs hash", *engine),
        Cmd::Vcs { .. } => Ok(()),
    }
}
