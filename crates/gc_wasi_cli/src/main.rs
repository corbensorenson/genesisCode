use std::path::{Path, PathBuf};

use anyhow::Context;
use clap::{Parser, Subcommand, ValueEnum};
use serde::Serialize;

use gc_coreform::{
    Term, TermOrdKey, canonicalize_module, hash_module, parse_module, parse_term, print_module,
    print_term,
};
use gc_effects::{
    ArtifactStore, CapsPolicy, Decision, EffectLog, EffectsError, replay_with_store, run,
};
use gc_kernel::{Apply, EvalCtx, MemLimits, StepLimit, Value, eval_module};
use gc_prelude::{
    SelfhostBootstrapMode, build_prelude, load_selfhost_coreform_toolchain_v1_with_mode,
    selfhost_coreform_toolchain_v1_sources,
};

const EX_OK: u8 = 0;
const EX_INTERNAL: u8 = 1;
const EX_PARSE: u8 = 10;
const EX_FMT: u8 = 11;
const EX_EVAL: u8 = 20;
const EX_OBLIGATIONS: u8 = 30;
const EX_REPLAY: u8 = 40;
const EX_CAPS_DENY: u8 = 41;
const EX_VERIFY: u8 = 50;
const EX_IO: u8 = 70;

#[derive(Parser)]
#[command(name = "genesis", version)]
struct Cli {
    /// Emit machine-readable JSON on stdout.
    #[arg(long, global = true)]
    json: bool,

    /// Kernel evaluation step limit (default is toolchain-defined).
    #[arg(long, global = true, value_name = "N")]
    step_limit: Option<u64>,

    /// Disable the kernel evaluation step limit.
    #[arg(long, global = true, conflicts_with = "step_limit")]
    no_step_limit: bool,

    /// Maximum total number of `pair/cons` cells allocated during evaluation.
    #[arg(long, global = true, value_name = "N")]
    max_pair_cells: Option<u64>,

    /// Maximum observed vector length (vector literals and `vec/push`).
    #[arg(long, global = true, value_name = "N")]
    max_vec_len: Option<u64>,

    /// Maximum observed map length (map literals, `map/put`, `map/merge`).
    #[arg(long, global = true, value_name = "N")]
    max_map_len: Option<u64>,

    /// Maximum observed bytes length (bytes literals and `bytes/concat`).
    #[arg(long, global = true, value_name = "N")]
    max_bytes_len: Option<u64>,

    /// Maximum observed string length in UTF-8 bytes (string literals and `str/concat`).
    #[arg(long, global = true, value_name = "N")]
    max_string_len: Option<u64>,

    /// Path to selfhost toolchain artifact used when `--engine selfhost` is selected.
    /// Defaults to `./.genesis/selfhost/toolchain.gc` when bootstrap mode allows artifacts.
    #[arg(long, global = true, value_name = "FILE")]
    selfhost_artifact: Option<PathBuf>,

    /// Selfhost bootstrap mode for `--engine selfhost`.
    /// `artifact-only` is production mode; `embedded` is for local bootstrap/development.
    #[arg(long, global = true, value_enum, default_value_t = SelfhostBootstrapArg::ArtifactOnly)]
    selfhost_bootstrap: SelfhostBootstrapArg,

    /// Enforce selfhost-only execution for frontend paths.
    ///
    /// In this mode, commands that accept `--engine` must use `--engine selfhost`, and
    /// selfhost bootstrap mode must be `artifact-only` (no embedded fallback).
    /// This can also be enabled via `GENESIS_SELFHOST_ONLY=1`.
    #[arg(long, global = true, default_value_t = false)]
    selfhost_only: bool,

    #[command(subcommand)]
    cmd: Cmd,
}

#[derive(Debug, Clone, Copy, ValueEnum)]
enum SelfhostBootstrapArg {
    ArtifactOnly,
    ArtifactPreferred,
    Embedded,
}

#[derive(Subcommand)]
enum Cmd {
    /// Canonical formatting for CoreForm (.gc) files.
    Fmt {
        file: PathBuf,
        /// Fail if the file is not already canonically formatted.
        #[arg(long)]
        check: bool,
        /// Formatting engine. `rust` uses the Rust CoreForm frontend; `selfhost` runs the
        /// self-hosted CoreForm toolchain inside the kernel.
        #[arg(long, value_enum)]
        engine: Option<FmtEngine>,
    },

    /// Evaluate a CoreForm program/module (pure).
    Eval {
        file: PathBuf,
        /// Frontend engine. `rust` uses the Rust CoreForm frontend; `selfhost` runs the
        /// self-hosted CoreForm parser+canonicalizer inside the kernel.
        #[arg(long, value_enum)]
        engine: Option<FmtEngine>,
        /// Run the Stage-1 compiler pipeline (CoreForm -> CoreForm validated transforms)
        /// before evaluation.
        #[arg(long)]
        stage1_pipeline: bool,
        /// Require `core/obligation::stage1-validation` to pass for the Stage-1 pipeline.
        /// Implies `--stage1-pipeline`.
        #[arg(long)]
        stage1_gate: bool,
        /// Require stage-2 CoreForm->WASM translation validation to pass when this module is
        /// supported by stage-2 lowering.
        #[arg(long)]
        stage2_gate: bool,
    },

    /// Run package obligations (unit tests, determinism, replay checks, etc.) and write evidence into `.genesis/store`.
    Test {
        /// Path to package.toml
        #[arg(long)]
        pkg: PathBuf,

        /// Override capability policy TOML for effectful tests.
        #[arg(long)]
        caps: Option<PathBuf>,
    },

    /// Compute and store a content-addressed package artifact record.
    Pack {
        /// Path to package.toml
        #[arg(long)]
        pkg: PathBuf,
    },

    /// Build a selfhost CoreForm toolchain artifact for bootstrap cutover.
    SelfhostArtifact {
        /// Output artifact path (CoreForm term file).
        #[arg(long)]
        out: PathBuf,
        /// Minimum number of modules that must be Stage-2 supported.
        #[arg(long, default_value_t = 0)]
        min_stage2_supported_modules: u64,
        /// Minimum number of modules that must be Stage-2 validated (`supported && ok`).
        #[arg(long, default_value_t = 0)]
        min_stage2_validated_modules: u64,
    },

    /// Run an effect program with a deny-by-default capability policy.
    Run {
        file: PathBuf,
        /// Frontend engine. `rust` uses the Rust CoreForm frontend; `selfhost` runs the
        /// self-hosted CoreForm toolchain inside the kernel.
        #[arg(long, value_enum)]
        engine: Option<FmtEngine>,
        /// Capability policy file (caps.toml).
        #[arg(long)]
        caps: PathBuf,
        /// Write the deterministic effect log (.gclog) to this path.
        #[arg(long)]
        log: Option<PathBuf>,
    },

    /// Replay an effect program against an existing effect log (.gclog).
    Replay {
        file: PathBuf,
        /// Frontend engine. `rust` uses the Rust CoreForm frontend; `selfhost` runs the
        /// self-hosted CoreForm toolchain inside the kernel.
        #[arg(long, value_enum)]
        engine: Option<FmtEngine>,
        #[arg(long)]
        log: PathBuf,
        /// Optional artifact store dir for logs that externalize large responses.
        #[arg(long)]
        store: Option<PathBuf>,
    },

    /// Content-addressed store operations (effectful; policy-gated).
    Store {
        /// Capability policy TOML (deny-by-default allowlist).
        #[arg(long)]
        caps: PathBuf,

        /// Output effect log path (.gclog). Defaults to ./.genesis/logs/<op>-<pid>-<seq>.gclog
        #[arg(long)]
        log: Option<PathBuf>,

        #[command(subcommand)]
        cmd: StoreCmd,
    },

    /// Manage refs (branches/tags) as effectful operations (policy-gated).
    Refs {
        /// Capability policy TOML (deny-by-default allowlist).
        #[arg(long)]
        caps: PathBuf,

        /// Output effect log path (.gclog). Defaults to ./.genesis/logs/<op>-<pid>-<seq>.gclog
        #[arg(long)]
        log: Option<PathBuf>,

        #[command(subcommand)]
        cmd: RefsCmd,
    },

    /// Package workflows (GenesisPkg) as effectful operations (policy-gated).
    ///
    /// Under the WASI bootstrap, networking is denied; these commands support local-only flows
    /// (lockfiles, local store/refs, `.gpk` import/export).
    Pkg {
        /// Capability policy TOML (deny-by-default allowlist).
        #[arg(long)]
        caps: PathBuf,

        /// Output effect log path (.gclog). Defaults to ./.genesis/logs/<op>-<pid>-<seq>.gclog
        #[arg(long)]
        log: Option<PathBuf>,

        #[command(subcommand)]
        cmd: PkgCmd,
    },

    /// GenesisGraph pure ops subset.
    Vcs {
        #[command(subcommand)]
        cmd: VcsCmd,
    },
}

#[derive(Subcommand)]
enum VcsCmd {
    /// Hash a CoreForm term (or module) without mutating the store.
    Hash {
        /// Input file containing a single CoreForm term or a CoreForm module.
        #[arg(long = "in", alias = "input")]
        input: PathBuf,
        /// Frontend engine. `rust` uses the Rust CoreForm frontend; `selfhost` runs the
        /// self-hosted CoreForm toolchain inside the kernel.
        #[arg(long, value_enum)]
        engine: Option<FmtEngine>,
    },
}

#[derive(Subcommand)]
enum StoreCmd {
    /// Store a CoreForm artifact datum and return its content hash.
    Put {
        /// Input file containing a single CoreForm term.
        #[arg(long = "in", alias = "input")]
        input: PathBuf,
    },
    /// Fetch an artifact datum by hash.
    Get {
        /// Content hash (hex).
        hash: String,
        /// Optional output path (writes the canonical CoreForm term bytes).
        #[arg(long)]
        out: Option<PathBuf>,
    },
    /// Check presence of an artifact hash.
    Has {
        /// Content hash (hex).
        hash: String,
    },
}

#[derive(Subcommand)]
enum RefsCmd {
    /// Get a ref value.
    Get {
        /// Ref name (e.g. refs/heads/main).
        name: String,
    },
    /// List refs (optionally filtered by prefix).
    List {
        /// Prefix filter (e.g. refs/heads/).
        #[arg(long)]
        prefix: Option<String>,
    },
    /// Advance a ref to a commit hash (policy-gated).
    Set {
        /// Ref name.
        name: String,
        /// Commit hash (hex).
        hash: String,
        /// Policy artifact hash (hex).
        #[arg(long)]
        policy: String,
        /// Optional optimistic concurrency check. Pass a hex hash, or the literal string `nil`
        /// to require the ref to be unset.
        #[arg(long)]
        expected_old: Option<String>,
    },
    /// Delete a ref (policy-gated).
    Delete {
        /// Ref name.
        name: String,
        /// Policy artifact hash (hex).
        #[arg(long)]
        policy: String,
        /// Optional optimistic concurrency check. Pass a hex hash, or the literal string `nil`
        /// to require the ref to be unset.
        #[arg(long)]
        expected_old: Option<String>,
    },
}

#[derive(Subcommand)]
enum PkgCmd {
    /// Initialize a `genesis.lock` workspace lock file.
    Init {
        /// Workspace name.
        #[arg(long)]
        workspace: String,

        /// Lock path (relative to the capability base_dir).
        #[arg(long, default_value = "genesis.lock")]
        lock: PathBuf,

        /// Workspace policy alias (stored in lock; not resolved here).
        #[arg(long, default_value = "policy:default-v0.1")]
        policy: String,

        /// Default registry remote spec (stored in lock; unused under WASI without sync).
        #[arg(long)]
        registry_default: Option<String>,
    },

    /// Add or update a dependency requirement in `genesis.lock`.
    ///
    /// Spec format: `<name>@<selector>` where selector is `commit:<hex>`, `snapshot:<hex>`,
    /// or `refs/...` (or `ref:refs/...`).
    Add {
        spec: String,

        /// Lock path (relative to the capability base_dir).
        #[arg(long, default_value = "genesis.lock")]
        lock: PathBuf,

        /// Update policy for ref-tracking dependencies.
        #[arg(long, default_value = "manual", value_parser = ["manual", "auto"])]
        update_policy: String,

        /// Registry name from `[registries]` (default is `default`).
        #[arg(long)]
        registry: Option<String>,
    },

    /// Resolve requirements into pinned commits/snapshots in `genesis.lock` (local-only).
    Lock {
        /// Lock path (relative to the capability base_dir).
        #[arg(long, default_value = "genesis.lock")]
        lock: PathBuf,

        /// Perform strict checks while resolving locks: validate commit/snapshot/evidence integrity.
        #[arg(long)]
        strict: bool,
    },

    /// Update locked entries for tracked refs (`update_policy=auto`) (local-only).
    Update {
        /// Lock path (relative to the capability base_dir).
        #[arg(long, default_value = "genesis.lock")]
        lock: PathBuf,
    },

    /// Verify that all locked snapshots are present in the local store, and optionally verify commit evidence.
    Install {
        /// Lock path (relative to the capability base_dir).
        #[arg(long, default_value = "genesis.lock")]
        lock: PathBuf,

        /// Fail if any requirement is missing a locked entry.
        #[arg(long)]
        frozen: bool,

        /// Perform strict checks: validate commit/evidence artifacts when present.
        #[arg(long)]
        strict: bool,
    },

    /// Verify locked entries and referenced artifacts (strict checks).
    Verify {
        /// Lock path (relative to the capability base_dir).
        #[arg(long, default_value = "genesis.lock")]
        lock: PathBuf,
    },

    /// List requirements and locked entries from `genesis.lock`.
    List {
        /// Lock path (relative to the capability base_dir).
        #[arg(long, default_value = "genesis.lock")]
        lock: PathBuf,
    },

    /// Show info for a single dependency from `genesis.lock`.
    Info {
        name: String,

        /// Lock path (relative to the capability base_dir).
        #[arg(long, default_value = "genesis.lock")]
        lock: PathBuf,
    },

    /// Build and store a `:vcs/snapshot` for a `package.toml`.
    Snapshot {
        /// Path to package.toml (relative to the capability base_dir).
        #[arg(long)]
        pkg: PathBuf,
    },

    /// Export a `.gpk` bundle from a root hash.
    Export {
        /// Root identifier. Accepts a hash, `refs/...`, or `ref:refs/...`.
        ///
        /// For shallow bundles this must resolve to a snapshot hash.
        /// For full bundles this is usually a commit hash.
        #[arg(long = "snapshot", visible_alias = "root")]
        root: String,

        /// Output bundle path (relative to capability base_dir).
        #[arg(long)]
        out: PathBuf,

        /// Export a full-history bundle from the root hash (commit closure + snapshots + patches + evidence).
        #[arg(long)]
        full: bool,

        /// Parent depth when the root is a commit hash (0 = no parents).
        #[arg(long, default_value_t = 0)]
        depth: u64,

        /// Evidence inclusion policy for full bundles: `required`, `all`, or `none`.
        #[arg(long, default_value = "required")]
        include_evidence: String,

        /// Dependency inclusion policy for snapshot deps: `none`, `locked`, or `all`.
        #[arg(long, default_value = "locked")]
        include_deps: String,

        /// Include named refs in the bundle (requires `.gpk` v2).
        #[arg(long = "include-ref")]
        include_refs: Vec<String>,
    },

    /// Import a `.gpk` bundle into the local store.
    Import {
        /// Input bundle path (relative to capability base_dir).
        #[arg(long)]
        input: PathBuf,

        /// Update local refs after import.
        ///
        /// Format: `<refname>=<commit-hash>` (hash may be `nil` to delete).
        #[arg(long = "set-ref")]
        set_refs: Vec<String>,

        /// Policy artifact hash (hex) used by the local refs/set gate (required when using --set-ref).
        #[arg(long)]
        policy: Option<String>,
    },
}

#[derive(Copy, Clone, Debug, Eq, PartialEq, ValueEnum)]
enum FmtEngine {
    Rust,
    Selfhost,
}

fn resolved_step_limit(cli: &Cli) -> StepLimit {
    if cli.no_step_limit {
        StepLimit::Unlimited
    } else if let Some(n) = cli.step_limit {
        StepLimit::Limit(n)
    } else {
        StepLimit::Default
    }
}

fn resolved_mem_limits(cli: &Cli) -> MemLimits {
    let mut m = MemLimits::default();
    if let Some(n) = cli.max_pair_cells {
        m.max_pair_cells = Some(n);
    }
    if let Some(n) = cli.max_vec_len {
        m.max_vec_len = Some(n);
    }
    if let Some(n) = cli.max_map_len {
        m.max_map_len = Some(n);
    }
    if let Some(n) = cli.max_bytes_len {
        m.max_bytes_len = Some(n);
    }
    if let Some(n) = cli.max_string_len {
        m.max_string_len = Some(n);
    }
    m
}

fn mk_ctx(cli: &Cli) -> EvalCtx {
    let mut ctx = EvalCtx::with_step_limit(resolved_step_limit(cli).resolve());
    ctx.set_mem_limits(resolved_mem_limits(cli));
    ctx
}

fn resolved_selfhost_bootstrap_mode(cli: &Cli) -> SelfhostBootstrapMode {
    match cli.selfhost_bootstrap {
        SelfhostBootstrapArg::ArtifactOnly => SelfhostBootstrapMode::ArtifactOnly,
        SelfhostBootstrapArg::ArtifactPreferred => SelfhostBootstrapMode::ArtifactPreferred,
        SelfhostBootstrapArg::Embedded => SelfhostBootstrapMode::Embedded,
    }
}

const SELFHOST_TOOLCHAIN_ARTIFACT_ENV: &str = "GENESIS_SELFHOST_TOOLCHAIN_ARTIFACT";
const ALLOW_RUST_ENGINE_ENV: &str = "GENESIS_ALLOW_RUST_ENGINE";
const DEFAULT_SELFHOST_TOOLCHAIN_ARTIFACT_REL: &str = ".genesis/selfhost/toolchain.gc";
const WORKSPACE_SELFHOST_TOOLCHAIN_ARTIFACT_REL: &str = "selfhost/toolchain.gc";

fn parse_truthy_env_flag(s: &str) -> bool {
    matches!(
        s.trim().to_ascii_lowercase().as_str(),
        "1" | "true" | "yes" | "on"
    )
}

fn default_selfhost_artifact_path() -> PathBuf {
    std::env::current_dir()
        .unwrap_or_else(|_| PathBuf::from("."))
        .join(DEFAULT_SELFHOST_TOOLCHAIN_ARTIFACT_REL)
}

fn workspace_selfhost_artifact_path() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("../..")
        .join(WORKSPACE_SELFHOST_TOOLCHAIN_ARTIFACT_REL)
}

fn resolved_selfhost_artifact_for_frontend(cli: &Cli) -> Option<PathBuf> {
    if let Some(p) = cli.selfhost_artifact.clone() {
        return Some(p);
    }
    if let Ok(raw) = std::env::var(SELFHOST_TOOLCHAIN_ARTIFACT_ENV) {
        let trimmed = raw.trim();
        if !trimmed.is_empty() {
            return Some(PathBuf::from(trimmed));
        }
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

fn selfhost_only_enabled(cli: &Cli) -> bool {
    cli.selfhost_only
        || std::env::var("GENESIS_SELFHOST_ONLY")
            .map(|v| parse_truthy_env_flag(&v))
            .unwrap_or(false)
}

fn rust_engine_compat_enabled() -> bool {
    std::env::var(ALLOW_RUST_ENGINE_ENV)
        .map(|v| parse_truthy_env_flag(&v))
        .unwrap_or(false)
}

fn resolved_coreform_frontend(cli: &Cli) -> Result<gc_obligations::CoreformFrontend, CliError> {
    let strict = selfhost_only_enabled(cli);
    let mode = resolved_selfhost_bootstrap_mode(cli);
    if strict && mode != SelfhostBootstrapMode::ArtifactOnly {
        return Err(cli_err(
            EX_VERIFY,
            "selfhost-only/bootstrap",
            "selfhost-only mode requires --selfhost-bootstrap artifact-only",
        ));
    }
    let artifact = resolved_selfhost_artifact_for_frontend(cli);
    Ok(gc_obligations::CoreformFrontend::Selfhost(
        gc_obligations::SelfhostFrontendConfig {
            bootstrap_mode: mode,
            artifact,
        },
    ))
}

fn enforce_selfhost_engine(
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

fn resolved_engine(
    cli: &Cli,
    cmd_name: &str,
    engine: Option<FmtEngine>,
) -> Result<FmtEngine, CliError> {
    enforce_selfhost_engine(cli, cmd_name, engine)?;
    if let Some(e) = engine {
        if e == FmtEngine::Rust && !rust_engine_compat_enabled() {
            return Err(cli_err(
                EX_VERIFY,
                "compat/rust-engine-disabled",
                format!(
                    "`--engine rust` is disabled in the default selfhost profile for `{cmd_name}`; set {ALLOW_RUST_ENGINE_ENV}=1 to enable compatibility mode"
                ),
            ));
        }
        return Ok(e);
    }
    Ok(FmtEngine::Selfhost)
}

fn enforce_selfhost_only_cmd(cli: &Cli) -> Result<(), CliError> {
    if !selfhost_only_enabled(cli) {
        return Ok(());
    }
    match &cli.cmd {
        Cmd::Fmt { engine, .. } => enforce_selfhost_engine(cli, "fmt", *engine),
        Cmd::Eval { engine, .. } => enforce_selfhost_engine(cli, "eval", *engine),
        Cmd::Run { engine, .. } => enforce_selfhost_engine(cli, "run", *engine),
        Cmd::Replay { engine, .. } => enforce_selfhost_engine(cli, "replay", *engine),
        Cmd::Test { .. } => Ok(()),
        Cmd::Pack { .. } => Ok(()),
        Cmd::Vcs {
            cmd: VcsCmd::Hash { engine, .. },
        } => enforce_selfhost_engine(cli, "vcs hash", *engine),
        other => {
            let cmd = match other {
                Cmd::Test { .. } | Cmd::Pack { .. } => unreachable!(),
                Cmd::SelfhostArtifact { .. } => "selfhost-artifact",
                Cmd::Store { .. } => "store",
                Cmd::Refs { .. } => "refs",
                Cmd::Pkg { .. } => "pkg",
                Cmd::Vcs { .. } => "vcs (non-hash)",
                Cmd::Fmt { .. } | Cmd::Eval { .. } | Cmd::Run { .. } | Cmd::Replay { .. } => {
                    unreachable!()
                }
            };
            Err(cli_err(
                EX_VERIFY,
                "selfhost-only/unsupported-cmd",
                format!(
                    "selfhost-only mode currently supports only `fmt`, `eval`, `run`, `replay`, `test`, `pack`, and `vcs hash`; `{cmd}` is not yet selfhost-routed"
                ),
            ))
        }
    }
}

fn load_selfhost_toolchain(
    cli: &Cli,
    ctx: &mut EvalCtx,
    env: &mut gc_kernel::Env,
) -> Result<(), CliError> {
    let mode = resolved_selfhost_bootstrap_mode(cli);
    if selfhost_only_enabled(cli) && mode != SelfhostBootstrapMode::ArtifactOnly {
        return Err(cli_err(
            EX_VERIFY,
            "selfhost-only/bootstrap",
            "selfhost-only mode requires --selfhost-bootstrap artifact-only",
        ));
    }
    let artifact = resolved_selfhost_artifact_for_frontend(cli);
    load_selfhost_coreform_toolchain_v1_with_mode(ctx, env, mode, artifact.as_deref())
        .map_err(|e| cli_err(EX_INTERNAL, "selfhost/init", format!("{e}")))
}

#[derive(Debug)]
struct CliError {
    exit_code: u8,
    json: JsonError,
}

#[derive(Debug, Serialize)]
struct JsonError {
    code: &'static str,
    message: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    context: Option<serde_json::Value>,
}

#[derive(Debug, Serialize)]
struct JsonEnvelope<T> {
    ok: bool,
    kind: &'static str,
    #[serde(skip_serializing_if = "Option::is_none")]
    data: Option<T>,
    #[serde(skip_serializing_if = "Option::is_none")]
    error: Option<JsonError>,
}

#[derive(Debug)]
struct CmdOut {
    exit_code: u8,
    stdout: String,
    json: serde_json::Value,
}

fn cli_err(exit_code: u8, code: &'static str, message: impl Into<String>) -> CliError {
    CliError {
        exit_code,
        json: JsonError {
            code,
            message: message.into(),
            context: None,
        },
    }
}

fn obligation_err(e: gc_obligations::ObligationError) -> CliError {
    match e {
        gc_obligations::ObligationError::Manifest(s) => cli_err(EX_PARSE, "manifest/error", s),
        gc_obligations::ObligationError::Module(s) => cli_err(EX_PARSE, "module/error", s),
        gc_obligations::ObligationError::Store(s) => cli_err(EX_IO, "store/error", s),
        gc_obligations::ObligationError::Io(e) => cli_err(EX_IO, "io/error", e.to_string()),
        other => cli_err(EX_EVAL, "obligation/error", other.to_string()),
    }
}

fn normalize_newlines(s: &str) -> String {
    s.replace("\r\n", "\n")
}

fn hex32(h: [u8; 32]) -> String {
    const LUT: &[u8; 16] = b"0123456789abcdef";
    let mut out = String::with_capacity(64);
    for b in h {
        out.push(LUT[(b >> 4) as usize] as char);
        out.push(LUT[(b & 0x0f) as usize] as char);
    }
    out
}

fn render_value_for_cli(ctx: &EvalCtx, v: &Value) -> (String, &'static str) {
    let protocol_error = ctx.protocol.map(|p| p.error);
    let t = v.to_term_for_log(protocol_error);
    (gc_coreform::print_term(&t), "coreform/term")
}

fn default_effect_log_path(op: &str) -> Result<PathBuf, CliError> {
    use std::fs::OpenOptions;
    use std::io::Write;

    let dir = PathBuf::from(".genesis").join("logs");
    std::fs::create_dir_all(&dir).map_err(|e| cli_err(EX_IO, "io/mkdir", format!("{e}")))?;
    for i in 0u64..1_000_000 {
        let cand = dir.join(format!("{op}-{}-{i}.gclog", std::process::id()));
        match OpenOptions::new().write(true).create_new(true).open(&cand) {
            Ok(mut f) => {
                let _ = f.write_all(b"; genesis effect log\n");
                return Ok(cand);
            }
            Err(e) if e.kind() == std::io::ErrorKind::AlreadyExists => continue,
            Err(e) => return Err(cli_err(EX_IO, "io/write", format!("{e}"))),
        }
    }
    Err(cli_err(
        EX_IO,
        "io/write",
        "failed to allocate effect log path after 1,000,000 attempts",
    ))
}

fn write_effect_log(path: &PathBuf, log: &EffectLog) -> Result<(), CliError> {
    std::fs::write(path, log.to_string_canonical() + "\n")
        .with_context(|| format!("write {}", path.display()))
        .map_err(|e| cli_err(EX_IO, "io/write", format!("{e}")))
}

fn cmd_fmt(
    cli: &Cli,
    file: &PathBuf,
    check: bool,
    engine: Option<FmtEngine>,
) -> Result<CmdOut, CliError> {
    let engine = resolved_engine(cli, "fmt", engine)?;
    let src = std::fs::read_to_string(file)
        .with_context(|| format!("read {}", file.display()))
        .map_err(|e| cli_err(EX_IO, "io/read", format!("{e}")))?;

    let out = match engine {
        FmtEngine::Rust => {
            let forms = parse_module(&src)
                .map_err(|e| cli_err(EX_PARSE, "parse/coreform", e.to_string()))?;
            let canon = canonicalize_module(forms)
                .map_err(|e| cli_err(EX_PARSE, "canon/coreform", e.to_string()))?;
            print_module(&canon)
        }
        FmtEngine::Selfhost => {
            // Toolchain bootstrap is trusted; do not charge it against the step limit for the file being formatted.
            // Memory limits still apply deterministically.
            let mut ctx = EvalCtx::with_step_limit(None);
            ctx.set_mem_limits(resolved_mem_limits(cli));
            let prelude = build_prelude(&mut ctx);
            let mut env = prelude.env;

            load_selfhost_toolchain(cli, &mut ctx, &mut env)?;

            let f = env.get("selfhost/tool::fmt-module").ok_or_else(|| {
                cli_err(
                    EX_INTERNAL,
                    "selfhost/missing",
                    "missing binding selfhost/tool::fmt-module",
                )
            })?;
            // Now apply the user-configured step limit to the formatting work itself.
            ctx.steps = 0;
            ctx.step_limit = resolved_step_limit(cli).resolve();
            let r = f
                .apply(&mut ctx, Value::Data(gc_coreform::Term::Str(src.clone())))
                .map_err(|e| cli_err(EX_EVAL, "eval/error", format!("selfhost fmt failed: {e}")))?;

            if let Some((code, message, payload)) = extract_protocol_error(&ctx, &r) {
                return Err(CliError {
                    exit_code: EX_PARSE,
                    json: JsonError {
                        code: "selfhost/error",
                        message: format!("{code}: {message}"),
                        context: payload.map(serde_json::Value::String),
                    },
                });
            }

            let Some(gc_coreform::Term::Str(s)) = r.as_data() else {
                return Err(cli_err(
                    EX_INTERNAL,
                    "selfhost/bad-return",
                    format!("selfhost fmt returned non-string: {}", r.debug_repr()),
                ));
            };
            s.clone()
        }
    };

    let changed = normalize_newlines(&src) != normalize_newlines(&out);
    let ok = if check { !changed } else { true };
    let exit_code = if ok { EX_OK } else { EX_FMT };

    if !check && changed {
        std::fs::write(file, out)
            .with_context(|| format!("write {}", file.display()))
            .map_err(|e| cli_err(EX_IO, "io/write", format!("{e}")))?;
    }

    let env = JsonEnvelope {
        ok,
        kind: "genesis/fmt-v0.2",
        data: Some(serde_json::json!({
            "file": file.display().to_string(),
            "check": check,
            "changed": changed,
            "engine": match engine {
                FmtEngine::Rust => "rust",
                FmtEngine::Selfhost => "selfhost",
            },
        })),
        error: if ok {
            None
        } else {
            Some(JsonError {
                code: "fmt/not-canonical",
                message: format!("{} is not canonically formatted", file.display()),
                context: None,
            })
        },
    };

    Ok(CmdOut {
        exit_code,
        stdout: String::new(),
        json: serde_json::to_value(env).expect("json"),
    })
}

fn extract_protocol_error(ctx: &EvalCtx, v: &Value) -> Option<(String, String, Option<String>)> {
    let tok = ctx.protocol?.error;
    let Value::Sealed { token, payload } = v else {
        return None;
    };
    if *token != tok {
        return None;
    }

    let payload_term = payload.to_term_for_log(Some(tok));
    let (code, msg) = match &payload_term {
        gc_coreform::Term::Map(m) => {
            let code = m
                .get(&gc_coreform::TermOrdKey(gc_coreform::Term::Symbol(
                    ":error/code".to_string(),
                )))
                .and_then(|t| match t {
                    gc_coreform::Term::Str(s) => Some(s.clone()),
                    _ => None,
                })
                .unwrap_or_else(|| "core/error".to_string());
            let msg = m
                .get(&gc_coreform::TermOrdKey(gc_coreform::Term::Symbol(
                    ":error/message".to_string(),
                )))
                .and_then(|t| match t {
                    gc_coreform::Term::Str(s) => Some(s.clone()),
                    _ => None,
                })
                .unwrap_or_else(|| "error".to_string());
            (code, msg)
        }
        _ => ("core/error".to_string(), "error".to_string()),
    };
    Some((code, msg, Some(gc_coreform::print_term(&payload_term))))
}

fn selfhost_parse_canonicalize_module(
    ctx: &mut EvalCtx,
    env: &gc_kernel::Env,
    src: &str,
) -> Result<Vec<gc_coreform::Term>, CliError> {
    let parse_fn = env.get("selfhost/parse::parse-module").ok_or_else(|| {
        cli_err(
            EX_INTERNAL,
            "selfhost/missing",
            "missing binding selfhost/parse::parse-module",
        )
    })?;
    let parsed = parse_fn
        .apply(ctx, Value::Data(gc_coreform::Term::Str(src.to_string())))
        .map_err(|e| {
            cli_err(
                EX_EVAL,
                "eval/error",
                format!("selfhost parse-module failed: {e}"),
            )
        })?;

    if let Some((code, message, payload)) = extract_protocol_error(ctx, &parsed) {
        return Err(CliError {
            exit_code: EX_PARSE,
            json: JsonError {
                code: "selfhost/error",
                message: format!("{code}: {message}"),
                context: payload.map(serde_json::Value::String),
            },
        });
    }

    let Some(gc_coreform::Term::Vector(parsed_forms)) = parsed.as_data() else {
        return Err(cli_err(
            EX_INTERNAL,
            "selfhost/bad-return",
            format!(
                "selfhost parse-module returned non-vector: {}",
                parsed.debug_repr()
            ),
        ));
    };

    let canon_fn = env
        .get("selfhost/canon::canonicalize-module")
        .ok_or_else(|| {
            cli_err(
                EX_INTERNAL,
                "selfhost/missing",
                "missing binding selfhost/canon::canonicalize-module",
            )
        })?;
    let canon = canon_fn
        .apply(
            ctx,
            Value::Data(gc_coreform::Term::Vector(parsed_forms.clone())),
        )
        .map_err(|e| {
            cli_err(
                EX_EVAL,
                "eval/error",
                format!("selfhost canonicalize-module failed: {e}"),
            )
        })?;

    if let Some((code, message, payload)) = extract_protocol_error(ctx, &canon) {
        return Err(CliError {
            exit_code: EX_PARSE,
            json: JsonError {
                code: "selfhost/error",
                message: format!("{code}: {message}"),
                context: payload.map(serde_json::Value::String),
            },
        });
    }

    let Some(gc_coreform::Term::Vector(forms)) = canon.as_data() else {
        return Err(cli_err(
            EX_INTERNAL,
            "selfhost/bad-return",
            format!(
                "selfhost canonicalize-module returned non-vector: {}",
                canon.debug_repr()
            ),
        ));
    };
    Ok(forms.clone())
}

fn stage1_pipeline_json(out: &gc_opt::Stage1PipelineOutcome) -> serde_json::Value {
    serde_json::json!({
        "obligation": out.gate_report.obligation,
        "ok": out.gate_report.ok,
        "errors": out.gate_report.errors,
        "original_module_hash": hex32(out.gate_report.original_module_hash),
        "transformed_module_hash": hex32(out.gate_report.transformed_module_hash),
        "original_value_hash": out.gate_report.original_value_hash.map(hex32),
        "transformed_value_hash": out.gate_report.transformed_value_hash.map(hex32),
        "egg_runs": out.optimize_report.stats.egg_runs,
        "egg_iterations": out.optimize_report.stats.iterations,
        "egg_eclasses": out.optimize_report.stats.eclasses,
        "egg_enodes": out.optimize_report.stats.enodes,
        "egg_rewrites_applied": out.optimize_report.stats.rewrites_applied,
    })
}

fn stage2_report_json(r: &gc_opt::Stage2ValidationReport) -> serde_json::Value {
    serde_json::json!({
        "obligation": r.obligation,
        "supported": r.supported,
        "ok": r.ok,
        "module_hash": hex32(r.module_hash),
        "wasm_hash": r.wasm_hash.map(hex32),
        "value_kind": r.value_kind.map(|k| match k {
            gc_opt::Stage2ValueKind::Int => "int",
            gc_opt::Stage2ValueKind::Bool => "bool",
            gc_opt::Stage2ValueKind::Nil => "nil",
            gc_opt::Stage2ValueKind::Sym => "sym",
            gc_opt::Stage2ValueKind::Str => "str",
            gc_opt::Stage2ValueKind::Bytes => "bytes",
        }),
        "original_value_hash": r.original_value_hash.map(hex32),
        "wasm_value_hash": r.wasm_value_hash.map(hex32),
        "wasm_bytes_len": r.wasm_bytes_len,
        "errors": r.errors,
    })
}

fn cmd_eval(
    cli: &Cli,
    file: &PathBuf,
    engine: Option<FmtEngine>,
    stage1_pipeline: bool,
    stage1_gate: bool,
    stage2_gate: bool,
) -> Result<CmdOut, CliError> {
    let engine = resolved_engine(cli, "eval", engine)?;
    let src = std::fs::read_to_string(file)
        .with_context(|| format!("read {}", file.display()))
        .map_err(|e| cli_err(EX_IO, "io/read", format!("{e}")))?;

    let (mut ctx, mut env, mut forms) = match engine {
        FmtEngine::Rust => {
            let forms = parse_module(&src)
                .map_err(|e| cli_err(EX_PARSE, "parse/coreform", e.to_string()))?;
            let forms = canonicalize_module(forms)
                .map_err(|e| cli_err(EX_PARSE, "canon/coreform", e.to_string()))?;

            let mut ctx = mk_ctx(cli);
            let prelude = build_prelude(&mut ctx);
            (ctx, prelude.env, forms)
        }
        FmtEngine::Selfhost => {
            // Parse/canonicalize with selfhost bindings loaded, then evaluate in a fresh
            // prelude-only env so closure/request hashing matches the rust frontend path.
            let mut parse_ctx = EvalCtx::with_step_limit(None);
            parse_ctx.set_mem_limits(resolved_mem_limits(cli));
            let prelude = build_prelude(&mut parse_ctx);
            let mut parse_env = prelude.env;
            load_selfhost_toolchain(cli, &mut parse_ctx, &mut parse_env)?;

            parse_ctx.steps = 0;
            parse_ctx.step_limit = None;
            let forms = selfhost_parse_canonicalize_module(&mut parse_ctx, &parse_env, &src)?;

            let mut eval_ctx = mk_ctx(cli);
            let prelude = build_prelude(&mut eval_ctx);
            (eval_ctx, prelude.env, forms)
        }
    };

    let stage1 = if stage1_pipeline || stage1_gate {
        let out = gc_opt::stage1_pipeline(&forms)
            .map_err(|e| cli_err(EX_INTERNAL, "stage1/error", format!("{e}")))?;
        if stage1_gate && !out.gate_report.ok {
            return Err(CliError {
                exit_code: EX_OBLIGATIONS,
                json: JsonError {
                    code: "obligation/stage1-validation",
                    message: "core/obligation::stage1-validation failed".to_string(),
                    context: Some(stage1_pipeline_json(&out)),
                },
            });
        }
        forms = out.transformed_forms.clone();
        Some(out)
    } else {
        None
    };

    let stage1_for_stage2 = if stage2_gate && stage1.is_none() {
        Some(
            gc_opt::stage1_pipeline(&forms)
                .map_err(|e| cli_err(EX_INTERNAL, "stage1/error", format!("{e}")))?,
        )
    } else {
        None
    };
    let stage2_input: &[Term] = if let Some(out) = stage1.as_ref() {
        &out.transformed_forms
    } else if let Some(out) = stage1_for_stage2.as_ref() {
        &out.transformed_forms
    } else {
        &forms
    };
    let stage2 = if stage2_gate {
        Some(gc_opt::stage2_validation_report(stage2_input))
    } else {
        None
    };
    if stage2_gate {
        let s2 = stage2
            .as_ref()
            .expect("stage2 report must exist when stage2 gate is enabled");
        if s2.supported && !s2.ok {
            return Err(CliError {
                exit_code: EX_OBLIGATIONS,
                json: JsonError {
                    code: "obligation/translation-validation",
                    message:
                        "core/obligation::translation-validation (stage2 CoreForm->WASM) failed"
                            .to_string(),
                    context: Some(stage2_report_json(s2)),
                },
            });
        }
    }

    let v = eval_module(&mut ctx, &mut env, &forms)
        .map_err(|e| cli_err(EX_EVAL, "eval/error", format!("{e}")))?;
    let (value, value_format) = render_value_for_cli(&ctx, &v);

    let env = JsonEnvelope {
        ok: true,
        kind: "genesis/eval-v0.2",
        data: Some(serde_json::json!({
            "file": file.display().to_string(),
            "engine": match engine {
                FmtEngine::Rust => "rust",
                FmtEngine::Selfhost => "selfhost",
            },
            "stage1": stage1.as_ref().map(stage1_pipeline_json),
            "stage2": stage2.as_ref().map(stage2_report_json),
            "value": value,
            "value_format": value_format,
        })),
        error: None,
    };
    Ok(CmdOut {
        exit_code: EX_OK,
        stdout: if cli.json {
            String::new()
        } else {
            format!("{value}\n")
        },
        json: serde_json::to_value(env).expect("json"),
    })
}

fn cmd_test(cli: &Cli, pkg: &Path, caps: Option<&Path>) -> Result<CmdOut, CliError> {
    let frontend = resolved_coreform_frontend(cli)?;
    let r = gc_obligations::test_package_with_step_limit_and_frontend(
        pkg,
        caps,
        resolved_step_limit(cli),
        resolved_mem_limits(cli),
        frontend,
    )
    .map_err(obligation_err)?;
    let exit_code = if r.ok { EX_OK } else { EX_OBLIGATIONS };

    let obligations: Vec<serde_json::Value> = r
        .obligation_results
        .iter()
        .map(|o| {
            serde_json::json!({
                "name": o.name,
                "ok": o.ok,
                "artifact": o.artifact,
                "errors": o.errors,
            })
        })
        .collect();

    let env = JsonEnvelope {
        ok: r.ok,
        kind: "genesis/test-v0.2",
        data: Some(serde_json::json!({
            "pkg": pkg.display().to_string(),
            "caps": caps.map(|p| p.display().to_string()),
            "acceptance_artifact": r.acceptance_artifact,
            "obligations": obligations,
        })),
        error: None,
    };

    Ok(CmdOut {
        exit_code,
        stdout: if cli.json {
            String::new()
        } else {
            format!("{}\n", r.acceptance_artifact)
        },
        json: serde_json::to_value(env).expect("json"),
    })
}

fn cmd_pack(cli: &Cli, pkg: &Path) -> Result<CmdOut, CliError> {
    let frontend = resolved_coreform_frontend(cli)?;
    let h = gc_obligations::pack_with_frontend(pkg, frontend).map_err(obligation_err)?;
    let env = JsonEnvelope {
        ok: true,
        kind: "genesis/pack-v0.2",
        data: Some(serde_json::json!({
            "pkg": pkg.display().to_string(),
            "package_artifact": h,
        })),
        error: None,
    };
    Ok(CmdOut {
        exit_code: EX_OK,
        stdout: if cli.json {
            String::new()
        } else {
            format!("{h}\n")
        },
        json: serde_json::to_value(env).expect("json"),
    })
}

fn cmd_selfhost_artifact(
    cli: &Cli,
    out: &Path,
    min_stage2_supported_modules: u64,
    min_stage2_validated_modules: u64,
) -> Result<CmdOut, CliError> {
    let mut modules = Vec::new();
    let mut all_ok = true;
    let mut stage2_supported = 0u64;
    let mut stage2_validated = 0u64;
    let mut gate_errors: Vec<String> = Vec::new();

    for (path, src) in selfhost_coreform_toolchain_v1_sources() {
        let forms = parse_module(src)
            .map_err(|e| cli_err(EX_PARSE, "selfhost/parse", format!("{path}: {e}")))?;
        let forms = canonicalize_module(forms)
            .map_err(|e| cli_err(EX_PARSE, "selfhost/canon", format!("{path}: {e}")))?;
        let module_h = hash_module(&forms);

        let stage1 = gc_opt::stage1_pipeline(&forms)
            .map_err(|e| cli_err(EX_INTERNAL, "stage1/error", format!("{path}: {e}")))?;
        let stage2 = gc_opt::stage2_validation_report(&stage1.transformed_forms);
        if !stage1.gate_report.ok || (stage2.supported && !stage2.ok) {
            all_ok = false;
        }
        if stage2.supported {
            stage2_supported = stage2_supported.saturating_add(1);
            if stage2.ok {
                stage2_validated = stage2_validated.saturating_add(1);
            }
        }

        modules.push(Term::Map(
            [
                (
                    TermOrdKey(Term::symbol(":path")),
                    Term::Str((*path).to_string()),
                ),
                (
                    TermOrdKey(Term::symbol(":source")),
                    Term::Str((*src).to_string()),
                ),
                (
                    TermOrdKey(Term::symbol(":module-h")),
                    Term::Bytes(module_h.to_vec().into()),
                ),
                (
                    TermOrdKey(Term::symbol(":stage1-ok")),
                    Term::Bool(stage1.gate_report.ok),
                ),
                (
                    TermOrdKey(Term::symbol(":stage1-errors")),
                    Term::Vector(
                        stage1
                            .gate_report
                            .errors
                            .into_iter()
                            .map(Term::Str)
                            .collect(),
                    ),
                ),
                (
                    TermOrdKey(Term::symbol(":stage2-supported")),
                    Term::Bool(stage2.supported),
                ),
                (
                    TermOrdKey(Term::symbol(":stage2-ok")),
                    Term::Bool(stage2.ok),
                ),
                (
                    TermOrdKey(Term::symbol(":stage2-errors")),
                    Term::Vector(stage2.errors.into_iter().map(Term::Str).collect()),
                ),
                (
                    TermOrdKey(Term::symbol(":stage2-module-h")),
                    Term::Bytes(stage2.module_hash.to_vec().into()),
                ),
                (
                    TermOrdKey(Term::symbol(":stage2-wasm-h")),
                    stage2
                        .wasm_hash
                        .map(|h| Term::Bytes(h.to_vec().into()))
                        .unwrap_or(Term::Nil),
                ),
                (
                    TermOrdKey(Term::symbol(":stage2-wasm-bytes")),
                    stage2
                        .wasm_bytes_len
                        .map(|n| Term::Int((n as i64).into()))
                        .unwrap_or(Term::Nil),
                ),
            ]
            .into_iter()
            .collect(),
        ));
    }

    if stage2_supported < min_stage2_supported_modules {
        all_ok = false;
        gate_errors.push(format!(
            "stage2 supported modules {} is below required minimum {}",
            stage2_supported, min_stage2_supported_modules
        ));
    }
    if stage2_validated < min_stage2_validated_modules {
        all_ok = false;
        gate_errors.push(format!(
            "stage2 validated modules {} is below required minimum {}",
            stage2_validated, min_stage2_validated_modules
        ));
    }

    let artifact = Term::Map(
        [
            (
                TermOrdKey(Term::symbol(":kind")),
                Term::Str("genesis/selfhost-toolchain-artifact-v0.2".to_string()),
            ),
            (TermOrdKey(Term::symbol(":v")), Term::Int(1.into())),
            (TermOrdKey(Term::symbol(":ok")), Term::Bool(all_ok)),
            (
                TermOrdKey(Term::symbol(":generated-by")),
                Term::Str(format!("genesis_wasi {}", env!("CARGO_PKG_VERSION"))),
            ),
            (
                TermOrdKey(Term::symbol(":stage2-summary")),
                Term::Map(
                    [
                        (
                            TermOrdKey(Term::symbol(":supported-modules")),
                            Term::Int((stage2_supported as i64).into()),
                        ),
                        (
                            TermOrdKey(Term::symbol(":validated-modules")),
                            Term::Int((stage2_validated as i64).into()),
                        ),
                    ]
                    .into_iter()
                    .collect(),
                ),
            ),
            (
                TermOrdKey(Term::symbol(":stage2-requirements")),
                Term::Map(
                    [
                        (
                            TermOrdKey(Term::symbol(":min-supported-modules")),
                            Term::Int((min_stage2_supported_modules as i64).into()),
                        ),
                        (
                            TermOrdKey(Term::symbol(":min-validated-modules")),
                            Term::Int((min_stage2_validated_modules as i64).into()),
                        ),
                        (
                            TermOrdKey(Term::symbol(":ok")),
                            Term::Bool(gate_errors.is_empty()),
                        ),
                        (
                            TermOrdKey(Term::symbol(":errors")),
                            Term::Vector(gate_errors.iter().cloned().map(Term::Str).collect()),
                        ),
                    ]
                    .into_iter()
                    .collect(),
                ),
            ),
            (TermOrdKey(Term::symbol(":modules")), Term::Vector(modules)),
        ]
        .into_iter()
        .collect(),
    );
    let artifact_s = print_term(&artifact);
    std::fs::write(out, artifact_s.as_bytes())
        .with_context(|| format!("write {}", out.display()))
        .map_err(|e| cli_err(EX_IO, "io/write", format!("{e}")))?;

    let artifact_hash = *blake3::hash(artifact_s.as_bytes()).as_bytes();
    let exit_code = if all_ok { EX_OK } else { EX_OBLIGATIONS };
    let env = JsonEnvelope {
        ok: all_ok,
        kind: "genesis/selfhost-artifact-v0.2",
        data: Some(serde_json::json!({
            "out": out.display().to_string(),
            "ok": all_ok,
            "artifact_hash": hex32(artifact_hash),
            "stage2_supported_modules": stage2_supported,
            "stage2_validated_modules": stage2_validated,
            "min_stage2_supported_modules": min_stage2_supported_modules,
            "min_stage2_validated_modules": min_stage2_validated_modules,
            "stage2_requirements_ok": gate_errors.is_empty(),
            "stage2_requirement_errors": gate_errors,
        })),
        error: None,
    };
    Ok(CmdOut {
        exit_code,
        stdout: if cli.json {
            String::new()
        } else {
            format!("{}\n", out.display())
        },
        json: serde_json::to_value(env).expect("json"),
    })
}

fn cmd_run(
    cli: &Cli,
    file: &PathBuf,
    engine: Option<FmtEngine>,
    caps: &Path,
    log_path: &Option<PathBuf>,
) -> Result<CmdOut, CliError> {
    let engine = resolved_engine(cli, "run", engine)?;
    let src = std::fs::read_to_string(file)
        .with_context(|| format!("read {}", file.display()))
        .map_err(|e| cli_err(EX_IO, "io/read", format!("{e}")))?;
    let (mut ctx, mut env, forms) = match engine {
        FmtEngine::Rust => {
            let forms = parse_module(&src)
                .map_err(|e| cli_err(EX_PARSE, "parse/coreform", e.to_string()))?;
            let forms = canonicalize_module(forms)
                .map_err(|e| cli_err(EX_PARSE, "canon/coreform", e.to_string()))?;
            let mut ctx = mk_ctx(cli);
            let prelude = build_prelude(&mut ctx);
            (ctx, prelude.env, forms)
        }
        FmtEngine::Selfhost => {
            // Parse/canonicalize with selfhost bindings loaded, then evaluate in a fresh
            // prelude-only env so closure/request hashing matches the rust frontend path.
            let mut parse_ctx = EvalCtx::with_step_limit(None);
            parse_ctx.set_mem_limits(resolved_mem_limits(cli));
            let prelude = build_prelude(&mut parse_ctx);
            let mut parse_env = prelude.env;
            load_selfhost_toolchain(cli, &mut parse_ctx, &mut parse_env)?;

            parse_ctx.steps = 0;
            parse_ctx.step_limit = None;
            let forms = selfhost_parse_canonicalize_module(&mut parse_ctx, &parse_env, &src)?;

            let mut eval_ctx = mk_ctx(cli);
            let prelude = build_prelude(&mut eval_ctx);
            (eval_ctx, prelude.env, forms)
        }
    };

    let policy = CapsPolicy::load(caps)
        .with_context(|| format!("read {}", caps.display()))
        .map_err(|e| cli_err(EX_PARSE, "caps/parse", format!("{e}")))?;

    let program_hash = hash_module(&forms);
    let prog = eval_module(&mut ctx, &mut env, &forms)
        .map_err(|e| cli_err(EX_EVAL, "eval/error", format!("{e}")))?;

    let toolchain = format!("genesis_wasi/{} (wasi)", env!("CARGO_PKG_VERSION"));
    let r = run(&mut ctx, &policy, prog, program_hash, toolchain)
        .map_err(|e| cli_err(EX_EVAL, "run/error", format!("{e}")))?;

    let denied = r.log.entries.iter().any(|e| e.decision == Decision::Deny);
    let exit_code = if denied { EX_CAPS_DENY } else { EX_OK };

    let log_path = log_path
        .clone()
        .unwrap_or_else(|| file.with_extension("gclog"));
    write_effect_log(&log_path, &r.log)?;

    let (value, value_format) = render_value_for_cli(&ctx, &r.value);
    let env = JsonEnvelope {
        ok: !denied,
        kind: "genesis/run-v0.2",
        data: Some(serde_json::json!({
            "file": file.display().to_string(),
            "engine": match engine {
                FmtEngine::Rust => "rust",
                FmtEngine::Selfhost => "selfhost",
            },
            "caps": caps.display().to_string(),
            "log": log_path.display().to_string(),
            "program_hash_hex": hex32(program_hash),
            "denied": denied,
            "entries": r.log.entries.len(),
            "value": value,
            "value_format": value_format,
        })),
        error: None,
    };
    Ok(CmdOut {
        exit_code,
        stdout: if cli.json {
            String::new()
        } else {
            format!("{value}\n")
        },
        json: serde_json::to_value(env).expect("json"),
    })
}

fn cmd_replay(
    cli: &Cli,
    file: &PathBuf,
    engine: Option<FmtEngine>,
    log_path: &PathBuf,
    store: &Option<PathBuf>,
) -> Result<CmdOut, CliError> {
    let engine = resolved_engine(cli, "replay", engine)?;
    let src = std::fs::read_to_string(file)
        .with_context(|| format!("read {}", file.display()))
        .map_err(|e| cli_err(EX_IO, "io/read", format!("{e}")))?;
    let (mut ctx, mut env, forms) = match engine {
        FmtEngine::Rust => {
            let forms = parse_module(&src)
                .map_err(|e| cli_err(EX_PARSE, "parse/coreform", e.to_string()))?;
            let forms = canonicalize_module(forms)
                .map_err(|e| cli_err(EX_PARSE, "canon/coreform", e.to_string()))?;
            let mut ctx = mk_ctx(cli);
            let prelude = build_prelude(&mut ctx);
            (ctx, prelude.env, forms)
        }
        FmtEngine::Selfhost => {
            // Parse/canonicalize with selfhost bindings loaded, then evaluate in a fresh
            // prelude-only env so closure/request hashing matches the rust frontend path.
            let mut parse_ctx = EvalCtx::with_step_limit(None);
            parse_ctx.set_mem_limits(resolved_mem_limits(cli));
            let prelude = build_prelude(&mut parse_ctx);
            let mut parse_env = prelude.env;
            load_selfhost_toolchain(cli, &mut parse_ctx, &mut parse_env)?;

            parse_ctx.steps = 0;
            parse_ctx.step_limit = None;
            let forms = selfhost_parse_canonicalize_module(&mut parse_ctx, &parse_env, &src)?;

            let mut eval_ctx = mk_ctx(cli);
            let prelude = build_prelude(&mut eval_ctx);
            (eval_ctx, prelude.env, forms)
        }
    };
    let program_hash = hash_module(&forms);

    let log_src = std::fs::read_to_string(log_path)
        .with_context(|| format!("read {}", log_path.display()))
        .map_err(|e| cli_err(EX_IO, "io/read", format!("{e}")))?;
    let log_term =
        parse_term(&log_src).map_err(|e| cli_err(EX_PARSE, "parse/gclog", e.to_string()))?;
    let log = EffectLog::from_term(&log_term)
        .map_err(|e| cli_err(EX_PARSE, "parse/gclog", format!("{e}")))?;
    if log.program_hash != program_hash {
        return Err(cli_err(
            EX_REPLAY,
            "replay/program-hash-mismatch",
            "program hash mismatch: log is for different program",
        ));
    }

    let prog = eval_module(&mut ctx, &mut env, &forms)
        .map_err(|e| cli_err(EX_EVAL, "eval/error", format!("{e}")))?;

    let store_dir = store.clone();
    let store_obj = store_dir
        .as_deref()
        .map(|sd| ArtifactStore::open(sd).map_err(|e| cli_err(EX_IO, "io/store", format!("{e}"))))
        .transpose()?;
    let v = replay_with_store(&mut ctx, prog, &log, store_obj.as_ref()).map_err(|e| {
        let (exit, code) = match &e {
            EffectsError::ReplayMismatch(_) => (EX_REPLAY, "replay/mismatch"),
            _ => (EX_EVAL, "replay/error"),
        };
        cli_err(exit, code, format!("{e}"))
    })?;

    let (value, value_format) = render_value_for_cli(&ctx, &v);
    let env = JsonEnvelope {
        ok: true,
        kind: "genesis/replay-v0.2",
        data: Some(serde_json::json!({
            "file": file.display().to_string(),
            "engine": match engine {
                FmtEngine::Rust => "rust",
                FmtEngine::Selfhost => "selfhost",
            },
            "log": log_path.display().to_string(),
            "store": store_dir.map(|p| p.display().to_string()),
            "value": value,
            "value_format": value_format,
        })),
        error: None,
    };
    Ok(CmdOut {
        exit_code: EX_OK,
        stdout: if cli.json {
            String::new()
        } else {
            format!("{value}\n")
        },
        json: serde_json::to_value(env).expect("json"),
    })
}

fn mk_store_put_program(artifact: &gc_coreform::Term) -> Vec<gc_coreform::Term> {
    // (def prog (core/effect::perform 'core/store::put {:artifact (quote <artifact>)} (fn (r) (core/effect::pure r)))) prog
    let op = gc_coreform::Term::list(vec![
        gc_coreform::Term::symbol("quote"),
        gc_coreform::Term::symbol("core/store::put"),
    ]);
    let payload = gc_coreform::Term::Map(
        [(
            gc_coreform::TermOrdKey(gc_coreform::Term::symbol(":artifact")),
            gc_coreform::Term::list(vec![gc_coreform::Term::symbol("quote"), artifact.clone()]),
        )]
        .into_iter()
        .collect(),
    );
    let k = gc_coreform::Term::list(vec![
        gc_coreform::Term::symbol("fn"),
        gc_coreform::Term::list(vec![gc_coreform::Term::symbol("r")]),
        gc_coreform::Term::list(vec![
            gc_coreform::Term::symbol("core/effect::pure"),
            gc_coreform::Term::symbol("r"),
        ]),
    ]);
    let perform = gc_coreform::Term::list(vec![
        gc_coreform::Term::symbol("core/effect::perform"),
        op,
        payload,
        k,
    ]);
    vec![
        gc_coreform::Term::list(vec![
            gc_coreform::Term::symbol("def"),
            gc_coreform::Term::symbol("prog"),
            perform,
        ]),
        gc_coreform::Term::symbol("prog"),
    ]
}

fn mk_store_get_program(hash: &str) -> Vec<gc_coreform::Term> {
    let op = gc_coreform::Term::list(vec![
        gc_coreform::Term::symbol("quote"),
        gc_coreform::Term::symbol("core/store::get"),
    ]);
    let payload = gc_coreform::Term::Map(
        [(
            gc_coreform::TermOrdKey(gc_coreform::Term::symbol(":hash")),
            gc_coreform::Term::Str(hash.to_string()),
        )]
        .into_iter()
        .collect(),
    );
    let k = gc_coreform::Term::list(vec![
        gc_coreform::Term::symbol("fn"),
        gc_coreform::Term::list(vec![gc_coreform::Term::symbol("r")]),
        gc_coreform::Term::list(vec![
            gc_coreform::Term::symbol("core/effect::pure"),
            gc_coreform::Term::symbol("r"),
        ]),
    ]);
    let perform = gc_coreform::Term::list(vec![
        gc_coreform::Term::symbol("core/effect::perform"),
        op,
        payload,
        k,
    ]);
    vec![
        gc_coreform::Term::list(vec![
            gc_coreform::Term::symbol("def"),
            gc_coreform::Term::symbol("prog"),
            perform,
        ]),
        gc_coreform::Term::symbol("prog"),
    ]
}

fn mk_store_has_program(hash: &str) -> Vec<gc_coreform::Term> {
    let op = gc_coreform::Term::list(vec![
        gc_coreform::Term::symbol("quote"),
        gc_coreform::Term::symbol("core/store::has"),
    ]);
    let payload = gc_coreform::Term::Map(
        [(
            gc_coreform::TermOrdKey(gc_coreform::Term::symbol(":hash")),
            gc_coreform::Term::Str(hash.to_string()),
        )]
        .into_iter()
        .collect(),
    );
    let k = gc_coreform::Term::list(vec![
        gc_coreform::Term::symbol("fn"),
        gc_coreform::Term::list(vec![gc_coreform::Term::symbol("r")]),
        gc_coreform::Term::list(vec![
            gc_coreform::Term::symbol("core/effect::pure"),
            gc_coreform::Term::symbol("r"),
        ]),
    ]);
    let perform = gc_coreform::Term::list(vec![
        gc_coreform::Term::symbol("core/effect::perform"),
        op,
        payload,
        k,
    ]);
    vec![
        gc_coreform::Term::list(vec![
            gc_coreform::Term::symbol("def"),
            gc_coreform::Term::symbol("prog"),
            perform,
        ]),
        gc_coreform::Term::symbol("prog"),
    ]
}

fn extract_store_put_hash(v: &Value) -> Option<String> {
    let t = v.to_term_for_log(None);
    let gc_coreform::Term::Map(m) = t else {
        return None;
    };
    match m.get(&gc_coreform::TermOrdKey(gc_coreform::Term::symbol(":hash"))) {
        Some(gc_coreform::Term::Str(s)) => Some(s.clone()),
        _ => None,
    }
}

fn extract_store_has_present(v: &Value) -> Option<bool> {
    let t = v.to_term_for_log(None);
    let gc_coreform::Term::Map(m) = t else {
        return None;
    };
    match m.get(&gc_coreform::TermOrdKey(gc_coreform::Term::symbol(
        ":present",
    ))) {
        Some(gc_coreform::Term::Bool(b)) => Some(*b),
        _ => None,
    }
}

fn extract_store_get_artifact(v: &Value) -> Option<gc_coreform::Term> {
    let t = v.to_term_for_log(None);
    let gc_coreform::Term::Map(m) = t else {
        return None;
    };
    m.get(&gc_coreform::TermOrdKey(gc_coreform::Term::symbol(
        ":artifact",
    )))
    .cloned()
}

fn mk_refs_get_program(name: &str) -> Vec<gc_coreform::Term> {
    let op = gc_coreform::Term::list(vec![
        gc_coreform::Term::symbol("quote"),
        gc_coreform::Term::symbol("core/refs::get"),
    ]);
    let payload = gc_coreform::Term::Map(
        [(
            gc_coreform::TermOrdKey(gc_coreform::Term::symbol(":name")),
            gc_coreform::Term::Str(name.to_string()),
        )]
        .into_iter()
        .collect(),
    );
    let k = gc_coreform::Term::list(vec![
        gc_coreform::Term::symbol("fn"),
        gc_coreform::Term::list(vec![gc_coreform::Term::symbol("r")]),
        gc_coreform::Term::list(vec![
            gc_coreform::Term::symbol("core/effect::pure"),
            gc_coreform::Term::symbol("r"),
        ]),
    ]);
    let perform = gc_coreform::Term::list(vec![
        gc_coreform::Term::symbol("core/effect::perform"),
        op,
        payload,
        k,
    ]);
    vec![
        gc_coreform::Term::list(vec![
            gc_coreform::Term::symbol("def"),
            gc_coreform::Term::symbol("prog"),
            perform,
        ]),
        gc_coreform::Term::symbol("prog"),
    ]
}

fn mk_refs_list_program(prefix: Option<&str>) -> Vec<gc_coreform::Term> {
    let op = gc_coreform::Term::list(vec![
        gc_coreform::Term::symbol("quote"),
        gc_coreform::Term::symbol("core/refs::list"),
    ]);
    let mut m = std::collections::BTreeMap::new();
    if let Some(p) = prefix {
        m.insert(
            gc_coreform::TermOrdKey(gc_coreform::Term::symbol(":prefix")),
            gc_coreform::Term::Str(p.to_string()),
        );
    } else {
        m.insert(
            gc_coreform::TermOrdKey(gc_coreform::Term::symbol(":prefix")),
            gc_coreform::Term::Nil,
        );
    }
    let payload = gc_coreform::Term::Map(m);
    let k = gc_coreform::Term::list(vec![
        gc_coreform::Term::symbol("fn"),
        gc_coreform::Term::list(vec![gc_coreform::Term::symbol("r")]),
        gc_coreform::Term::list(vec![
            gc_coreform::Term::symbol("core/effect::pure"),
            gc_coreform::Term::symbol("r"),
        ]),
    ]);
    let perform = gc_coreform::Term::list(vec![
        gc_coreform::Term::symbol("core/effect::perform"),
        op,
        payload,
        k,
    ]);
    vec![
        gc_coreform::Term::list(vec![
            gc_coreform::Term::symbol("def"),
            gc_coreform::Term::symbol("prog"),
            perform,
        ]),
        gc_coreform::Term::symbol("prog"),
    ]
}

fn mk_refs_set_program(
    name: &str,
    hash: &str,
    policy: &str,
    expected_old: Option<&str>,
) -> Vec<gc_coreform::Term> {
    let op = gc_coreform::Term::list(vec![
        gc_coreform::Term::symbol("quote"),
        gc_coreform::Term::symbol("core/refs::set"),
    ]);
    let mut m = std::collections::BTreeMap::new();
    m.insert(
        gc_coreform::TermOrdKey(gc_coreform::Term::symbol(":name")),
        gc_coreform::Term::Str(name.to_string()),
    );
    m.insert(
        gc_coreform::TermOrdKey(gc_coreform::Term::symbol(":hash")),
        gc_coreform::Term::Str(hash.to_string()),
    );
    m.insert(
        gc_coreform::TermOrdKey(gc_coreform::Term::symbol(":policy")),
        gc_coreform::Term::Str(policy.to_string()),
    );
    if let Some(e) = expected_old {
        let v = if e == "nil" {
            gc_coreform::Term::Nil
        } else {
            gc_coreform::Term::Str(e.to_string())
        };
        m.insert(
            gc_coreform::TermOrdKey(gc_coreform::Term::symbol(":expected-old")),
            v,
        );
    }
    let payload = gc_coreform::Term::Map(m);
    let k = gc_coreform::Term::list(vec![
        gc_coreform::Term::symbol("fn"),
        gc_coreform::Term::list(vec![gc_coreform::Term::symbol("r")]),
        gc_coreform::Term::list(vec![
            gc_coreform::Term::symbol("core/effect::pure"),
            gc_coreform::Term::symbol("r"),
        ]),
    ]);
    let perform = gc_coreform::Term::list(vec![
        gc_coreform::Term::symbol("core/effect::perform"),
        op,
        payload,
        k,
    ]);
    vec![
        gc_coreform::Term::list(vec![
            gc_coreform::Term::symbol("def"),
            gc_coreform::Term::symbol("prog"),
            perform,
        ]),
        gc_coreform::Term::symbol("prog"),
    ]
}

fn mk_refs_delete_program(
    name: &str,
    policy: &str,
    expected_old: Option<&str>,
) -> Vec<gc_coreform::Term> {
    let op = gc_coreform::Term::list(vec![
        gc_coreform::Term::symbol("quote"),
        gc_coreform::Term::symbol("core/refs::delete"),
    ]);
    let mut m = std::collections::BTreeMap::new();
    m.insert(
        gc_coreform::TermOrdKey(gc_coreform::Term::symbol(":name")),
        gc_coreform::Term::Str(name.to_string()),
    );
    m.insert(
        gc_coreform::TermOrdKey(gc_coreform::Term::symbol(":policy")),
        gc_coreform::Term::Str(policy.to_string()),
    );
    if let Some(e) = expected_old {
        let v = if e == "nil" {
            gc_coreform::Term::Nil
        } else {
            gc_coreform::Term::Str(e.to_string())
        };
        m.insert(
            gc_coreform::TermOrdKey(gc_coreform::Term::symbol(":expected-old")),
            v,
        );
    }
    let payload = gc_coreform::Term::Map(m);
    let k = gc_coreform::Term::list(vec![
        gc_coreform::Term::symbol("fn"),
        gc_coreform::Term::list(vec![gc_coreform::Term::symbol("r")]),
        gc_coreform::Term::list(vec![
            gc_coreform::Term::symbol("core/effect::pure"),
            gc_coreform::Term::symbol("r"),
        ]),
    ]);
    let perform = gc_coreform::Term::list(vec![
        gc_coreform::Term::symbol("core/effect::perform"),
        op,
        payload,
        k,
    ]);
    vec![
        gc_coreform::Term::list(vec![
            gc_coreform::Term::symbol("def"),
            gc_coreform::Term::symbol("prog"),
            perform,
        ]),
        gc_coreform::Term::symbol("prog"),
    ]
}

fn extract_refs_get_hash(v: &Value) -> Option<String> {
    let t = v.to_term_for_log(None);
    let gc_coreform::Term::Map(m) = t else {
        return None;
    };
    match m.get(&gc_coreform::TermOrdKey(gc_coreform::Term::symbol(":hash"))) {
        Some(gc_coreform::Term::Str(s)) => Some(s.clone()),
        Some(gc_coreform::Term::Nil) => Some("nil".to_string()),
        _ => None,
    }
}

fn extract_refs_set_hash(v: &Value) -> Option<String> {
    extract_refs_get_hash(v)
}

fn extract_refs_list_pairs(v: &Value) -> Option<Vec<(String, String)>> {
    let t = v.to_term_for_log(None);
    let gc_coreform::Term::Map(m) = t else {
        return None;
    };
    let gc_coreform::Term::Vector(xs) =
        m.get(&gc_coreform::TermOrdKey(gc_coreform::Term::symbol(":refs")))?
    else {
        return None;
    };
    let mut out = Vec::new();
    for x in xs {
        let gc_coreform::Term::Map(em) = x else {
            return None;
        };
        let name = match em.get(&gc_coreform::TermOrdKey(gc_coreform::Term::symbol(":name"))) {
            Some(gc_coreform::Term::Str(s)) => s.clone(),
            _ => return None,
        };
        let hash = match em.get(&gc_coreform::TermOrdKey(gc_coreform::Term::symbol(":hash"))) {
            Some(gc_coreform::Term::Str(s)) => s.clone(),
            Some(gc_coreform::Term::Nil) => "nil".to_string(),
            _ => return None,
        };
        out.push((name, hash));
    }
    Some(out)
}

fn parse_pkg_spec(spec: &str) -> Result<(String, String), String> {
    let (name, sel) = spec
        .split_once('@')
        .ok_or_else(|| "spec must be <name>@<selector>".to_string())?;
    let name = name.trim();
    let sel = sel.trim();
    if name.is_empty() || sel.is_empty() {
        return Err("spec must be <name>@<selector> (both non-empty)".to_string());
    }
    Ok((name.to_string(), sel.to_string()))
}

#[derive(Debug, Clone)]
struct SetRefSpec {
    name: String,
    hash: String,
    policy: String,
    expected_old: Option<String>,
}

fn is_hex64(s: &str) -> bool {
    if s.len() != 64 {
        return false;
    }
    s.as_bytes().iter().all(|b| b.is_ascii_hexdigit())
}

fn parse_local_set_refs(
    specs: &[String],
    policy: Option<&str>,
) -> Result<Vec<SetRefSpec>, CliError> {
    if specs.is_empty() {
        return Ok(Vec::new());
    }
    let Some(pol) = policy else {
        return Err(cli_err(
            EX_PARSE,
            "pkg/import",
            "--set-ref requires --policy <policy-hash>".to_string(),
        ));
    };
    if !is_hex64(pol) {
        return Err(cli_err(
            EX_PARSE,
            "pkg/import",
            "--policy must be 64-hex".to_string(),
        ));
    }

    let mut out = Vec::new();
    for s in specs {
        let (name, hash) = s.split_once('=').ok_or_else(|| {
            cli_err(
                EX_PARSE,
                "pkg/import",
                "set-ref must be <refname>=<commit-hash|nil>".to_string(),
            )
        })?;
        let name = name.trim();
        let hash = hash.trim();
        if name.is_empty() || hash.is_empty() {
            return Err(cli_err(
                EX_PARSE,
                "pkg/import",
                "set-ref fields must be non-empty".to_string(),
            ));
        }
        if hash != "nil" && !is_hex64(hash) {
            return Err(cli_err(
                EX_PARSE,
                "pkg/import",
                "set-ref hash must be 64-hex or `nil`".to_string(),
            ));
        }
        out.push(SetRefSpec {
            name: name.to_string(),
            hash: hash.to_string(),
            policy: pol.to_string(),
            expected_old: None,
        });
    }
    Ok(out)
}

fn mk_pkg_init_program(
    workspace: &str,
    lock: &Path,
    policy: &str,
    registry_default: Option<&str>,
) -> Vec<gc_coreform::Term> {
    let op = gc_coreform::Term::list(vec![
        gc_coreform::Term::symbol("quote"),
        gc_coreform::Term::symbol("core/pkg::init"),
    ]);
    let mut m = std::collections::BTreeMap::new();
    m.insert(
        gc_coreform::TermOrdKey(gc_coreform::Term::symbol(":workspace")),
        gc_coreform::Term::Str(workspace.to_string()),
    );
    m.insert(
        gc_coreform::TermOrdKey(gc_coreform::Term::symbol(":lock")),
        gc_coreform::Term::Str(lock.display().to_string()),
    );
    m.insert(
        gc_coreform::TermOrdKey(gc_coreform::Term::symbol(":policy")),
        gc_coreform::Term::Str(policy.to_string()),
    );
    if let Some(rd) = registry_default {
        m.insert(
            gc_coreform::TermOrdKey(gc_coreform::Term::symbol(":registry-default")),
            gc_coreform::Term::Str(rd.to_string()),
        );
    }
    let payload = gc_coreform::Term::Map(m);
    let k = gc_coreform::Term::list(vec![
        gc_coreform::Term::symbol("fn"),
        gc_coreform::Term::list(vec![gc_coreform::Term::symbol("r")]),
        gc_coreform::Term::list(vec![
            gc_coreform::Term::symbol("core/effect::pure"),
            gc_coreform::Term::symbol("r"),
        ]),
    ]);
    let perform = gc_coreform::Term::list(vec![
        gc_coreform::Term::symbol("core/effect::perform"),
        op,
        payload,
        k,
    ]);
    vec![
        gc_coreform::Term::list(vec![
            gc_coreform::Term::symbol("def"),
            gc_coreform::Term::symbol("prog"),
            perform,
        ]),
        gc_coreform::Term::symbol("prog"),
    ]
}

fn mk_pkg_add_program(
    lock: &Path,
    name: &str,
    selector: &str,
    update_policy: &str,
    registry: Option<&str>,
) -> Vec<gc_coreform::Term> {
    let op = gc_coreform::Term::list(vec![
        gc_coreform::Term::symbol("quote"),
        gc_coreform::Term::symbol("core/pkg::add"),
    ]);
    let mut m = std::collections::BTreeMap::new();
    m.insert(
        gc_coreform::TermOrdKey(gc_coreform::Term::symbol(":lock")),
        gc_coreform::Term::Str(lock.display().to_string()),
    );
    m.insert(
        gc_coreform::TermOrdKey(gc_coreform::Term::symbol(":name")),
        gc_coreform::Term::Str(name.to_string()),
    );
    m.insert(
        gc_coreform::TermOrdKey(gc_coreform::Term::symbol(":selector")),
        gc_coreform::Term::Str(selector.to_string()),
    );
    m.insert(
        gc_coreform::TermOrdKey(gc_coreform::Term::symbol(":update-policy")),
        gc_coreform::Term::Str(update_policy.to_string()),
    );
    if let Some(r) = registry {
        m.insert(
            gc_coreform::TermOrdKey(gc_coreform::Term::symbol(":registry")),
            gc_coreform::Term::Str(r.to_string()),
        );
    }
    let payload = gc_coreform::Term::Map(m);
    let k = gc_coreform::Term::list(vec![
        gc_coreform::Term::symbol("fn"),
        gc_coreform::Term::list(vec![gc_coreform::Term::symbol("r")]),
        gc_coreform::Term::list(vec![
            gc_coreform::Term::symbol("core/effect::pure"),
            gc_coreform::Term::symbol("r"),
        ]),
    ]);
    let perform = gc_coreform::Term::list(vec![
        gc_coreform::Term::symbol("core/effect::perform"),
        op,
        payload,
        k,
    ]);
    vec![
        gc_coreform::Term::list(vec![
            gc_coreform::Term::symbol("def"),
            gc_coreform::Term::symbol("prog"),
            perform,
        ]),
        gc_coreform::Term::symbol("prog"),
    ]
}

fn mk_pkg_lock_program(lock: &Path, strict: bool) -> Vec<gc_coreform::Term> {
    let op = gc_coreform::Term::list(vec![
        gc_coreform::Term::symbol("quote"),
        gc_coreform::Term::symbol("core/pkg::lock"),
    ]);
    let payload = gc_coreform::Term::Map(
        [
            (
                gc_coreform::TermOrdKey(gc_coreform::Term::symbol(":lock")),
                gc_coreform::Term::Str(lock.display().to_string()),
            ),
            (
                gc_coreform::TermOrdKey(gc_coreform::Term::symbol(":strict")),
                gc_coreform::Term::Bool(strict),
            ),
        ]
        .into_iter()
        .collect(),
    );
    let k = gc_coreform::Term::list(vec![
        gc_coreform::Term::symbol("fn"),
        gc_coreform::Term::list(vec![gc_coreform::Term::symbol("r")]),
        gc_coreform::Term::list(vec![
            gc_coreform::Term::symbol("core/effect::pure"),
            gc_coreform::Term::symbol("r"),
        ]),
    ]);
    let perform = gc_coreform::Term::list(vec![
        gc_coreform::Term::symbol("core/effect::perform"),
        op,
        payload,
        k,
    ]);
    vec![
        gc_coreform::Term::list(vec![
            gc_coreform::Term::symbol("def"),
            gc_coreform::Term::symbol("prog"),
            perform,
        ]),
        gc_coreform::Term::symbol("prog"),
    ]
}

fn mk_pkg_update_program(lock: &Path) -> Vec<gc_coreform::Term> {
    let op = gc_coreform::Term::list(vec![
        gc_coreform::Term::symbol("quote"),
        gc_coreform::Term::symbol("core/pkg::update"),
    ]);
    let payload = gc_coreform::Term::Map(
        [(
            gc_coreform::TermOrdKey(gc_coreform::Term::symbol(":lock")),
            gc_coreform::Term::Str(lock.display().to_string()),
        )]
        .into_iter()
        .collect(),
    );
    let k = gc_coreform::Term::list(vec![
        gc_coreform::Term::symbol("fn"),
        gc_coreform::Term::list(vec![gc_coreform::Term::symbol("r")]),
        gc_coreform::Term::list(vec![
            gc_coreform::Term::symbol("core/effect::pure"),
            gc_coreform::Term::symbol("r"),
        ]),
    ]);
    let perform = gc_coreform::Term::list(vec![
        gc_coreform::Term::symbol("core/effect::perform"),
        op,
        payload,
        k,
    ]);
    vec![
        gc_coreform::Term::list(vec![
            gc_coreform::Term::symbol("def"),
            gc_coreform::Term::symbol("prog"),
            perform,
        ]),
        gc_coreform::Term::symbol("prog"),
    ]
}

fn mk_pkg_install_program(lock: &Path, frozen: bool, strict: bool) -> Vec<gc_coreform::Term> {
    let op = gc_coreform::Term::list(vec![
        gc_coreform::Term::symbol("quote"),
        gc_coreform::Term::symbol("core/pkg::install"),
    ]);
    let payload = gc_coreform::Term::Map(
        [
            (
                gc_coreform::TermOrdKey(gc_coreform::Term::symbol(":lock")),
                gc_coreform::Term::Str(lock.display().to_string()),
            ),
            (
                gc_coreform::TermOrdKey(gc_coreform::Term::symbol(":frozen")),
                gc_coreform::Term::Bool(frozen),
            ),
            (
                gc_coreform::TermOrdKey(gc_coreform::Term::symbol(":strict")),
                gc_coreform::Term::Bool(strict),
            ),
        ]
        .into_iter()
        .collect(),
    );
    let k = gc_coreform::Term::list(vec![
        gc_coreform::Term::symbol("fn"),
        gc_coreform::Term::list(vec![gc_coreform::Term::symbol("r")]),
        gc_coreform::Term::list(vec![
            gc_coreform::Term::symbol("core/effect::pure"),
            gc_coreform::Term::symbol("r"),
        ]),
    ]);
    let perform = gc_coreform::Term::list(vec![
        gc_coreform::Term::symbol("core/effect::perform"),
        op,
        payload,
        k,
    ]);
    vec![
        gc_coreform::Term::list(vec![
            gc_coreform::Term::symbol("def"),
            gc_coreform::Term::symbol("prog"),
            perform,
        ]),
        gc_coreform::Term::symbol("prog"),
    ]
}

fn mk_pkg_verify_program(lock: &Path) -> Vec<gc_coreform::Term> {
    let op = gc_coreform::Term::list(vec![
        gc_coreform::Term::symbol("quote"),
        gc_coreform::Term::symbol("core/pkg::verify"),
    ]);
    let payload = gc_coreform::Term::Map(
        [(
            gc_coreform::TermOrdKey(gc_coreform::Term::symbol(":lock")),
            gc_coreform::Term::Str(lock.display().to_string()),
        )]
        .into_iter()
        .collect(),
    );
    let k = gc_coreform::Term::list(vec![
        gc_coreform::Term::symbol("fn"),
        gc_coreform::Term::list(vec![gc_coreform::Term::symbol("r")]),
        gc_coreform::Term::list(vec![
            gc_coreform::Term::symbol("core/effect::pure"),
            gc_coreform::Term::symbol("r"),
        ]),
    ]);
    let perform = gc_coreform::Term::list(vec![
        gc_coreform::Term::symbol("core/effect::perform"),
        op,
        payload,
        k,
    ]);
    vec![
        gc_coreform::Term::list(vec![
            gc_coreform::Term::symbol("def"),
            gc_coreform::Term::symbol("prog"),
            perform,
        ]),
        gc_coreform::Term::symbol("prog"),
    ]
}

fn mk_pkg_list_program(lock: &Path) -> Vec<gc_coreform::Term> {
    let op = gc_coreform::Term::list(vec![
        gc_coreform::Term::symbol("quote"),
        gc_coreform::Term::symbol("core/pkg::list"),
    ]);
    let payload = gc_coreform::Term::Map(
        [(
            gc_coreform::TermOrdKey(gc_coreform::Term::symbol(":lock")),
            gc_coreform::Term::Str(lock.display().to_string()),
        )]
        .into_iter()
        .collect(),
    );
    let k = gc_coreform::Term::list(vec![
        gc_coreform::Term::symbol("fn"),
        gc_coreform::Term::list(vec![gc_coreform::Term::symbol("r")]),
        gc_coreform::Term::list(vec![
            gc_coreform::Term::symbol("core/effect::pure"),
            gc_coreform::Term::symbol("r"),
        ]),
    ]);
    let perform = gc_coreform::Term::list(vec![
        gc_coreform::Term::symbol("core/effect::perform"),
        op,
        payload,
        k,
    ]);
    vec![
        gc_coreform::Term::list(vec![
            gc_coreform::Term::symbol("def"),
            gc_coreform::Term::symbol("prog"),
            perform,
        ]),
        gc_coreform::Term::symbol("prog"),
    ]
}

fn mk_pkg_info_program(lock: &Path, name: &str) -> Vec<gc_coreform::Term> {
    let op = gc_coreform::Term::list(vec![
        gc_coreform::Term::symbol("quote"),
        gc_coreform::Term::symbol("core/pkg::info"),
    ]);
    let payload = gc_coreform::Term::Map(
        [
            (
                gc_coreform::TermOrdKey(gc_coreform::Term::symbol(":lock")),
                gc_coreform::Term::Str(lock.display().to_string()),
            ),
            (
                gc_coreform::TermOrdKey(gc_coreform::Term::symbol(":name")),
                gc_coreform::Term::Str(name.to_string()),
            ),
        ]
        .into_iter()
        .collect(),
    );
    let k = gc_coreform::Term::list(vec![
        gc_coreform::Term::symbol("fn"),
        gc_coreform::Term::list(vec![gc_coreform::Term::symbol("r")]),
        gc_coreform::Term::list(vec![
            gc_coreform::Term::symbol("core/effect::pure"),
            gc_coreform::Term::symbol("r"),
        ]),
    ]);
    let perform = gc_coreform::Term::list(vec![
        gc_coreform::Term::symbol("core/effect::perform"),
        op,
        payload,
        k,
    ]);
    vec![
        gc_coreform::Term::list(vec![
            gc_coreform::Term::symbol("def"),
            gc_coreform::Term::symbol("prog"),
            perform,
        ]),
        gc_coreform::Term::symbol("prog"),
    ]
}

fn mk_pkg_snapshot_program(pkg: &Path) -> Vec<gc_coreform::Term> {
    let op = gc_coreform::Term::list(vec![
        gc_coreform::Term::symbol("quote"),
        gc_coreform::Term::symbol("core/pkg::snapshot"),
    ]);
    let payload = gc_coreform::Term::Map(
        [(
            gc_coreform::TermOrdKey(gc_coreform::Term::symbol(":pkg")),
            gc_coreform::Term::Str(pkg.display().to_string()),
        )]
        .into_iter()
        .collect(),
    );
    let k = gc_coreform::Term::list(vec![
        gc_coreform::Term::symbol("fn"),
        gc_coreform::Term::list(vec![gc_coreform::Term::symbol("r")]),
        gc_coreform::Term::list(vec![
            gc_coreform::Term::symbol("core/effect::pure"),
            gc_coreform::Term::symbol("r"),
        ]),
    ]);
    let perform = gc_coreform::Term::list(vec![
        gc_coreform::Term::symbol("core/effect::perform"),
        op,
        payload,
        k,
    ]);
    vec![
        gc_coreform::Term::list(vec![
            gc_coreform::Term::symbol("def"),
            gc_coreform::Term::symbol("prog"),
            perform,
        ]),
        gc_coreform::Term::symbol("prog"),
    ]
}

fn mk_gpk_export_program(
    root: &str,
    out: &Path,
    full: bool,
    depth: u64,
    include_evidence: &str,
    include_deps: &str,
    include_refs: &[String],
) -> Vec<gc_coreform::Term> {
    let op = gc_coreform::Term::list(vec![
        gc_coreform::Term::symbol("quote"),
        gc_coreform::Term::symbol("core/gpk::export"),
    ]);
    let mut m = std::collections::BTreeMap::new();
    m.insert(
        gc_coreform::TermOrdKey(gc_coreform::Term::symbol(":root")),
        gc_coreform::Term::Str(root.to_string()),
    );
    m.insert(
        gc_coreform::TermOrdKey(gc_coreform::Term::symbol(":out")),
        gc_coreform::Term::Str(out.display().to_string()),
    );
    m.insert(
        gc_coreform::TermOrdKey(gc_coreform::Term::symbol(":mode")),
        gc_coreform::Term::Str(if full { ":full" } else { ":shallow" }.to_string()),
    );
    if depth > 0 {
        m.insert(
            gc_coreform::TermOrdKey(gc_coreform::Term::symbol(":depth")),
            gc_coreform::Term::Int((depth as i64).into()),
        );
    }
    m.insert(
        gc_coreform::TermOrdKey(gc_coreform::Term::symbol(":include-evidence")),
        gc_coreform::Term::Str(include_evidence.to_string()),
    );
    m.insert(
        gc_coreform::TermOrdKey(gc_coreform::Term::symbol(":include-deps")),
        gc_coreform::Term::Str(include_deps.to_string()),
    );
    if !include_refs.is_empty() {
        m.insert(
            gc_coreform::TermOrdKey(gc_coreform::Term::symbol(":refs")),
            gc_coreform::Term::Vector(
                include_refs
                    .iter()
                    .cloned()
                    .map(gc_coreform::Term::Str)
                    .collect(),
            ),
        );
    }
    let payload = gc_coreform::Term::Map(m);
    let k = gc_coreform::Term::list(vec![
        gc_coreform::Term::symbol("fn"),
        gc_coreform::Term::list(vec![gc_coreform::Term::symbol("r")]),
        gc_coreform::Term::list(vec![
            gc_coreform::Term::symbol("core/effect::pure"),
            gc_coreform::Term::symbol("r"),
        ]),
    ]);
    let perform = gc_coreform::Term::list(vec![
        gc_coreform::Term::symbol("core/effect::perform"),
        op,
        payload,
        k,
    ]);
    vec![
        gc_coreform::Term::list(vec![
            gc_coreform::Term::symbol("def"),
            gc_coreform::Term::symbol("prog"),
            perform,
        ]),
        gc_coreform::Term::symbol("prog"),
    ]
}

fn mk_gpk_import_program(input: &Path, set_refs: &[SetRefSpec]) -> Vec<gc_coreform::Term> {
    let op = gc_coreform::Term::list(vec![
        gc_coreform::Term::symbol("quote"),
        gc_coreform::Term::symbol("core/gpk::import"),
    ]);
    let mut payload_m = std::collections::BTreeMap::new();
    payload_m.insert(
        gc_coreform::TermOrdKey(gc_coreform::Term::symbol(":in")),
        gc_coreform::Term::Str(input.display().to_string()),
    );
    if !set_refs.is_empty() {
        let mut entries = Vec::with_capacity(set_refs.len());
        for sr in set_refs {
            let mut em = std::collections::BTreeMap::new();
            em.insert(
                gc_coreform::TermOrdKey(gc_coreform::Term::symbol(":name")),
                gc_coreform::Term::Str(sr.name.clone()),
            );
            em.insert(
                gc_coreform::TermOrdKey(gc_coreform::Term::symbol(":hash")),
                if sr.hash == "nil" {
                    gc_coreform::Term::Nil
                } else {
                    gc_coreform::Term::Str(sr.hash.clone())
                },
            );
            em.insert(
                gc_coreform::TermOrdKey(gc_coreform::Term::symbol(":policy")),
                gc_coreform::Term::Str(sr.policy.clone()),
            );
            if let Some(exp) = &sr.expected_old {
                em.insert(
                    gc_coreform::TermOrdKey(gc_coreform::Term::symbol(":expected-old")),
                    if exp == "nil" {
                        gc_coreform::Term::Nil
                    } else {
                        gc_coreform::Term::Str(exp.clone())
                    },
                );
            }
            entries.push(gc_coreform::Term::Map(em));
        }
        payload_m.insert(
            gc_coreform::TermOrdKey(gc_coreform::Term::symbol(":set-refs")),
            gc_coreform::Term::Vector(entries),
        );
    }
    let payload = gc_coreform::Term::Map(payload_m);
    let k = gc_coreform::Term::list(vec![
        gc_coreform::Term::symbol("fn"),
        gc_coreform::Term::list(vec![gc_coreform::Term::symbol("r")]),
        gc_coreform::Term::list(vec![
            gc_coreform::Term::symbol("core/effect::pure"),
            gc_coreform::Term::symbol("r"),
        ]),
    ]);
    let perform = gc_coreform::Term::list(vec![
        gc_coreform::Term::symbol("core/effect::perform"),
        op,
        payload,
        k,
    ]);
    vec![
        gc_coreform::Term::list(vec![
            gc_coreform::Term::symbol("def"),
            gc_coreform::Term::symbol("prog"),
            perform,
        ]),
        gc_coreform::Term::symbol("prog"),
    ]
}

fn extract_pkg_snapshot_hash(v: &Value) -> Option<String> {
    let t = v.to_term_for_log(None);
    let gc_coreform::Term::Map(m) = t else {
        return None;
    };
    match m.get(&gc_coreform::TermOrdKey(gc_coreform::Term::symbol(
        ":snapshot",
    ))) {
        Some(gc_coreform::Term::Str(s)) => Some(s.clone()),
        _ => None,
    }
}

fn extract_pkg_export_bundle_hash(v: &Value) -> Option<String> {
    let t = v.to_term_for_log(None);
    let gc_coreform::Term::Map(m) = t else {
        return None;
    };
    match m.get(&gc_coreform::TermOrdKey(gc_coreform::Term::symbol(
        ":bundle-h",
    ))) {
        Some(gc_coreform::Term::Str(s)) => Some(s.clone()),
        _ => None,
    }
}

fn extract_pkg_import_root(v: &Value) -> Option<String> {
    let t = v.to_term_for_log(None);
    let gc_coreform::Term::Map(m) = t else {
        return None;
    };
    match m.get(&gc_coreform::TermOrdKey(gc_coreform::Term::symbol(":root"))) {
        Some(gc_coreform::Term::Str(s)) => Some(s.clone()),
        _ => None,
    }
}

fn extract_pkg_lock_hash(v: &Value) -> Option<String> {
    let t = v.to_term_for_log(None);
    let gc_coreform::Term::Map(m) = t else {
        return None;
    };
    match m.get(&gc_coreform::TermOrdKey(gc_coreform::Term::symbol(
        ":lock-h",
    ))) {
        Some(gc_coreform::Term::Str(s)) => Some(s.clone()),
        _ => None,
    }
}

fn extract_pkg_ok_bool(v: &Value) -> Option<bool> {
    let t = v.to_term_for_log(None);
    let gc_coreform::Term::Map(m) = t else {
        return None;
    };
    match m.get(&gc_coreform::TermOrdKey(gc_coreform::Term::symbol(":ok"))) {
        Some(gc_coreform::Term::Bool(b)) => Some(*b),
        _ => None,
    }
}

fn cmd_pkg(
    cli: &Cli,
    caps: &Path,
    log: &Option<PathBuf>,
    cmd: &PkgCmd,
) -> Result<CmdOut, CliError> {
    let policy = CapsPolicy::load(caps)
        .with_context(|| format!("read {}", caps.display()))
        .map_err(|e| cli_err(EX_PARSE, "caps/parse", format!("{e}")))?;

    let (forms, kind, log_op) = match cmd {
        PkgCmd::Init {
            workspace,
            lock,
            policy,
            registry_default,
        } => (
            mk_pkg_init_program(workspace, lock, policy, registry_default.as_deref()),
            "genesis/pkg-init-v0.1",
            "pkg-init",
        ),
        PkgCmd::Add {
            spec,
            lock,
            update_policy,
            registry,
        } => {
            let (name, selector) =
                parse_pkg_spec(spec).map_err(|e| cli_err(EX_PARSE, "pkg/spec", e.to_string()))?;
            (
                mk_pkg_add_program(lock, &name, &selector, update_policy, registry.as_deref()),
                "genesis/pkg-add-v0.1",
                "pkg-add",
            )
        }
        PkgCmd::Lock { lock, strict } => (
            mk_pkg_lock_program(lock, *strict),
            "genesis/pkg-lock-v0.1",
            "pkg-lock",
        ),
        PkgCmd::Update { lock } => (
            mk_pkg_update_program(lock),
            "genesis/pkg-update-v0.1",
            "pkg-update",
        ),
        PkgCmd::Install {
            lock,
            frozen,
            strict,
        } => (
            mk_pkg_install_program(lock, *frozen, *strict),
            "genesis/pkg-install-v0.1",
            "pkg-install",
        ),
        PkgCmd::Verify { lock } => (
            mk_pkg_verify_program(lock),
            "genesis/pkg-verify-v0.1",
            "pkg-verify",
        ),
        PkgCmd::List { lock } => (
            mk_pkg_list_program(lock),
            "genesis/pkg-list-v0.1",
            "pkg-list",
        ),
        PkgCmd::Info { name, lock } => (
            mk_pkg_info_program(lock, name),
            "genesis/pkg-info-v0.1",
            "pkg-info",
        ),
        PkgCmd::Snapshot { pkg } => (
            mk_pkg_snapshot_program(pkg),
            "genesis/pkg-snapshot-v0.1",
            "pkg-snapshot",
        ),
        PkgCmd::Export {
            root,
            out,
            full,
            depth,
            include_evidence,
            include_deps,
            include_refs,
        } => (
            mk_gpk_export_program(
                root,
                out,
                *full,
                *depth,
                include_evidence,
                include_deps,
                include_refs,
            ),
            "genesis/pkg-export-v0.1",
            "pkg-export",
        ),
        PkgCmd::Import {
            input,
            set_refs,
            policy,
        } => {
            let parsed = parse_local_set_refs(set_refs, policy.as_deref())?;
            (
                mk_gpk_import_program(input, &parsed),
                "genesis/pkg-import-v0.1",
                "pkg-import",
            )
        }
    };

    let forms = canonicalize_module(forms)
        .map_err(|e| cli_err(EX_PARSE, "canon/coreform", e.to_string()))?;
    let program_hash = hash_module(&forms);

    let mut ctx = mk_ctx(cli);
    let prelude = build_prelude(&mut ctx);
    let mut env = prelude.env;
    let prog = eval_module(&mut ctx, &mut env, &forms)
        .map_err(|e| cli_err(EX_EVAL, "eval/error", format!("{e}")))?;

    let toolchain = format!("genesis_wasi/{} (wasi)", env!("CARGO_PKG_VERSION"));
    let r = run(&mut ctx, &policy, prog, program_hash, toolchain)
        .map_err(|e| cli_err(EX_EVAL, "effects/run", format!("{e}")))?;

    let log_path = match log {
        Some(p) => p.clone(),
        None => default_effect_log_path(log_op)?,
    };
    write_effect_log(&log_path, &r.log)?;

    let mut ok = true;
    let mut exit_code = EX_OK;
    if let Some(proto) = ctx.protocol
        && let Value::Sealed { token, payload } = &r.value
        && *token == proto.error
    {
        ok = false;
        exit_code = EX_EVAL;
        if let Value::Data(gc_coreform::Term::Map(m)) = payload.as_ref()
            && matches!(
                m.get(&gc_coreform::TermOrdKey(gc_coreform::Term::symbol(":error/code"))),
                Some(gc_coreform::Term::Str(s)) if s == "core/caps/denied"
            )
        {
            exit_code = EX_CAPS_DENY;
        }
    } else if matches!(cmd, PkgCmd::Install { .. } | PkgCmd::Verify { .. })
        && let Some(false) = extract_pkg_ok_bool(&r.value)
    {
        ok = false;
        exit_code = EX_VERIFY;
    }

    let (value, value_format) = render_value_for_cli(&ctx, &r.value);
    let stdout = if cli.json {
        String::new()
    } else {
        match cmd {
            PkgCmd::Init { .. }
            | PkgCmd::Add { .. }
            | PkgCmd::Lock { .. }
            | PkgCmd::Update { .. } => extract_pkg_lock_hash(&r.value)
                .map(|h| format!("{h}\n"))
                .unwrap_or_else(|| format!("{value}\n")),
            PkgCmd::Install { .. } | PkgCmd::Verify { .. } => {
                if ok {
                    "ok\n".to_string()
                } else {
                    format!("{value}\n")
                }
            }
            PkgCmd::List { .. } | PkgCmd::Info { .. } => format!("{value}\n"),
            PkgCmd::Snapshot { .. } => extract_pkg_snapshot_hash(&r.value)
                .map(|h| format!("{h}\n"))
                .unwrap_or_else(|| format!("{value}\n")),
            PkgCmd::Export { .. } => extract_pkg_export_bundle_hash(&r.value)
                .map(|h| format!("{h}\n"))
                .unwrap_or_else(|| format!("{value}\n")),
            PkgCmd::Import { .. } => extract_pkg_import_root(&r.value)
                .map(|h| format!("{h}\n"))
                .unwrap_or_else(|| format!("{value}\n")),
        }
    };

    let env = JsonEnvelope {
        ok,
        kind,
        data: Some(serde_json::json!({
            "caps": caps.display().to_string(),
            "log": log_path.display().to_string(),
            "value": value,
            "value_format": value_format,
        })),
        error: if ok {
            None
        } else {
            Some(JsonError {
                code: "pkg/error",
                message: "pkg operation failed".to_string(),
                context: None,
            })
        },
    };

    Ok(CmdOut {
        exit_code,
        stdout,
        json: serde_json::to_value(env).expect("json"),
    })
}

fn cmd_store(
    cli: &Cli,
    caps: &Path,
    log: &Option<PathBuf>,
    cmd: &StoreCmd,
) -> Result<CmdOut, CliError> {
    let policy = CapsPolicy::load(caps)
        .with_context(|| format!("read {}", caps.display()))
        .map_err(|e| cli_err(EX_PARSE, "caps/parse", format!("{e}")))?;

    let (forms, kind, log_op, out_path) = match cmd {
        StoreCmd::Put { input } => {
            let src = std::fs::read_to_string(input)
                .with_context(|| format!("read {}", input.display()))
                .map_err(|e| cli_err(EX_IO, "io/read", format!("{e}")))?;
            let art =
                parse_term(&src).map_err(|e| cli_err(EX_PARSE, "parse/term", e.to_string()))?;
            (
                mk_store_put_program(&art),
                "genesis/store-put-v0.2",
                "store-put",
                None,
            )
        }
        StoreCmd::Get { hash, out } => (
            mk_store_get_program(hash),
            "genesis/store-get-v0.2",
            "store-get",
            out.clone(),
        ),
        StoreCmd::Has { hash } => (
            mk_store_has_program(hash),
            "genesis/store-has-v0.2",
            "store-has",
            None,
        ),
    };

    let forms = canonicalize_module(forms)
        .map_err(|e| cli_err(EX_PARSE, "canon/coreform", e.to_string()))?;
    let program_hash = hash_module(&forms);

    let mut ctx = mk_ctx(cli);
    let prelude = build_prelude(&mut ctx);
    let mut env = prelude.env;
    let prog = eval_module(&mut ctx, &mut env, &forms)
        .map_err(|e| cli_err(EX_EVAL, "eval/error", format!("{e}")))?;

    let toolchain = format!("genesis_wasi/{} (wasi)", env!("CARGO_PKG_VERSION"));
    let r = run(&mut ctx, &policy, prog, program_hash, toolchain)
        .map_err(|e| cli_err(EX_EVAL, "effects/run", format!("{e}")))?;

    let log_path = match log {
        Some(p) => p.clone(),
        None => default_effect_log_path(log_op)?,
    };
    write_effect_log(&log_path, &r.log)?;

    let mut ok = true;
    let mut exit_code = EX_OK;
    if let Some(proto) = ctx.protocol
        && let Value::Sealed { token, payload } = &r.value
        && *token == proto.error
    {
        ok = false;
        exit_code = EX_EVAL;
        if let Value::Data(gc_coreform::Term::Map(m)) = payload.as_ref()
            && matches!(
                m.get(&gc_coreform::TermOrdKey(gc_coreform::Term::symbol(":error/code"))),
                Some(gc_coreform::Term::Str(s)) if s == "core/caps/denied"
            )
        {
            exit_code = EX_CAPS_DENY;
        }
    }

    let (value, value_format) = render_value_for_cli(&ctx, &r.value);
    let stdout = if cli.json {
        String::new()
    } else {
        match cmd {
            StoreCmd::Put { .. } => extract_store_put_hash(&r.value)
                .map(|h| format!("{h}\n"))
                .unwrap_or_else(|| format!("{value}\n")),
            StoreCmd::Has { .. } => extract_store_has_present(&r.value)
                .map(|b| format!("{}\n", if b { "true" } else { "false" }))
                .unwrap_or_else(|| format!("{value}\n")),
            StoreCmd::Get { .. } => {
                if !ok {
                    format!("{value}\n")
                } else if let Some(p) = &out_path {
                    let art = extract_store_get_artifact(&r.value).ok_or_else(|| {
                        cli_err(
                            EX_EVAL,
                            "store/error",
                            "store get returned unexpected value",
                        )
                    })?;
                    std::fs::write(p, gc_coreform::print_term(&art) + "\n")
                        .with_context(|| format!("write {}", p.display()))
                        .map_err(|e| cli_err(EX_IO, "io/write", format!("{e}")))?;
                    String::new()
                } else {
                    extract_store_get_artifact(&r.value)
                        .map(|t| format!("{}\n", gc_coreform::print_term(&t)))
                        .unwrap_or_else(|| format!("{value}\n"))
                }
            }
        }
    };

    let env = JsonEnvelope {
        ok,
        kind,
        data: Some(serde_json::json!({
            "caps": caps.display().to_string(),
            "log": log_path.display().to_string(),
            "value": value,
            "value_format": value_format,
        })),
        error: if ok {
            None
        } else {
            Some(JsonError {
                code: "store/error",
                message: "store operation failed".to_string(),
                context: None,
            })
        },
    };
    Ok(CmdOut {
        exit_code,
        stdout,
        json: serde_json::to_value(env).expect("json"),
    })
}

fn cmd_refs(
    cli: &Cli,
    caps: &Path,
    log: &Option<PathBuf>,
    cmd: &RefsCmd,
) -> Result<CmdOut, CliError> {
    let policy = CapsPolicy::load(caps)
        .with_context(|| format!("read {}", caps.display()))
        .map_err(|e| cli_err(EX_PARSE, "caps/parse", format!("{e}")))?;

    let (forms, kind, log_op) = match cmd {
        RefsCmd::Get { name } => (
            mk_refs_get_program(name),
            "genesis/refs-get-v0.1",
            "refs-get",
        ),
        RefsCmd::List { prefix } => (
            mk_refs_list_program(prefix.as_deref()),
            "genesis/refs-list-v0.1",
            "refs-list",
        ),
        RefsCmd::Set {
            name,
            hash,
            policy,
            expected_old,
        } => (
            mk_refs_set_program(name, hash, policy, expected_old.as_deref()),
            "genesis/refs-set-v0.1",
            "refs-set",
        ),
        RefsCmd::Delete {
            name,
            policy,
            expected_old,
        } => (
            mk_refs_delete_program(name, policy, expected_old.as_deref()),
            "genesis/refs-delete-v0.1",
            "refs-delete",
        ),
    };

    let forms = canonicalize_module(forms)
        .map_err(|e| cli_err(EX_PARSE, "canon/coreform", e.to_string()))?;
    let program_hash = hash_module(&forms);

    let mut ctx = mk_ctx(cli);
    let prelude = build_prelude(&mut ctx);
    let mut env = prelude.env;
    let prog = eval_module(&mut ctx, &mut env, &forms)
        .map_err(|e| cli_err(EX_EVAL, "eval/error", format!("{e}")))?;

    let toolchain = format!("genesis_wasi/{} (wasi)", env!("CARGO_PKG_VERSION"));
    let r = run(&mut ctx, &policy, prog, program_hash, toolchain)
        .map_err(|e| cli_err(EX_EVAL, "effects/run", format!("{e}")))?;

    let log_path = match log {
        Some(p) => p.clone(),
        None => default_effect_log_path(log_op)?,
    };
    write_effect_log(&log_path, &r.log)?;

    let mut ok = true;
    let mut exit_code = EX_OK;
    if let Some(proto) = ctx.protocol
        && let Value::Sealed { token, payload } = &r.value
        && *token == proto.error
    {
        ok = false;
        exit_code = EX_EVAL;
        if let Value::Data(gc_coreform::Term::Map(m)) = payload.as_ref()
            && matches!(
                m.get(&gc_coreform::TermOrdKey(gc_coreform::Term::symbol(":error/code"))),
                Some(gc_coreform::Term::Str(s)) if s == "core/caps/denied"
            )
        {
            exit_code = EX_CAPS_DENY;
        }
    }

    let (value, value_format) = render_value_for_cli(&ctx, &r.value);
    let stdout = if cli.json {
        String::new()
    } else {
        match cmd {
            RefsCmd::Get { .. } => extract_refs_get_hash(&r.value)
                .map(|h| format!("{h}\n"))
                .unwrap_or_else(|| format!("{value}\n")),
            RefsCmd::List { .. } => extract_refs_list_pairs(&r.value)
                .map(|pairs| {
                    let mut s = String::new();
                    for (n, h) in pairs {
                        s.push_str(&n);
                        s.push(' ');
                        s.push_str(&h);
                        s.push('\n');
                    }
                    s
                })
                .unwrap_or_else(|| format!("{value}\n")),
            RefsCmd::Set { .. } => {
                if ok {
                    extract_refs_set_hash(&r.value)
                        .map(|h| format!("{h}\n"))
                        .unwrap_or_else(|| "ok\n".to_string())
                } else {
                    format!("{value}\n")
                }
            }
            RefsCmd::Delete { .. } => {
                if ok {
                    "ok\n".to_string()
                } else {
                    format!("{value}\n")
                }
            }
        }
    };

    let env = JsonEnvelope {
        ok,
        kind,
        data: Some(serde_json::json!({
            "caps": caps.display().to_string(),
            "log": log_path.display().to_string(),
            "value": value,
            "value_format": value_format,
        })),
        error: if ok {
            None
        } else {
            Some(JsonError {
                code: "refs/error",
                message: "refs operation failed".to_string(),
                context: None,
            })
        },
    };
    Ok(CmdOut {
        exit_code,
        stdout,
        json: serde_json::to_value(env).expect("json"),
    })
}

fn cmd_vcs_hash(cli: &Cli, input: &PathBuf, engine: Option<FmtEngine>) -> Result<CmdOut, CliError> {
    let engine = resolved_engine(cli, "vcs hash", engine)?;
    let src = std::fs::read_to_string(input)
        .with_context(|| format!("read {}", input.display()))
        .map_err(|e| cli_err(EX_IO, "io/read", format!("{e}")))?;
    let (hex, kind) = if engine == FmtEngine::Selfhost {
        let mut ctx = EvalCtx::with_step_limit(None);
        ctx.set_mem_limits(resolved_mem_limits(cli));
        let prelude = build_prelude(&mut ctx);
        let mut env = prelude.env;
        load_selfhost_toolchain(cli, &mut ctx, &mut env)?;

        let f = env
            .get("selfhost/tool::hash-src-with-kind")
            .ok_or_else(|| {
                cli_err(
                    EX_INTERNAL,
                    "selfhost/missing",
                    "missing binding selfhost/tool::hash-src-with-kind",
                )
            })?;

        ctx.steps = 0;
        ctx.step_limit = resolved_step_limit(cli).resolve();
        let r = f
            .apply(&mut ctx, Value::Data(Term::Str(src.clone())))
            .map_err(|e| {
                cli_err(
                    EX_EVAL,
                    "eval/error",
                    format!("selfhost vcs hash failed: {e}"),
                )
            })?;
        if let Some((code, message, payload)) = extract_protocol_error(&ctx, &r) {
            return Err(CliError {
                exit_code: EX_PARSE,
                json: JsonError {
                    code: "selfhost/error",
                    message: format!("{code}: {message}"),
                    context: payload.map(serde_json::Value::String),
                },
            });
        }
        let (hex, kind) = match r {
            Value::Data(Term::Map(m)) => {
                let hex = match m.get(&TermOrdKey(Term::symbol(":hash"))) {
                    Some(Term::Str(s)) => s.clone(),
                    _ => {
                        return Err(cli_err(
                            EX_INTERNAL,
                            "selfhost/bad-return",
                            "selfhost vcs hash return missing :hash string",
                        ));
                    }
                };
                let kind = match m.get(&TermOrdKey(Term::symbol(":kind"))) {
                    Some(Term::Str(s)) if s == "term" || s == "module" => s.clone(),
                    _ => {
                        return Err(cli_err(
                            EX_INTERNAL,
                            "selfhost/bad-return",
                            "selfhost vcs hash return missing :kind string",
                        ));
                    }
                };
                (hex, kind)
            }
            Value::Map(m) => {
                let hex = match m.get(&TermOrdKey(Term::symbol(":hash"))) {
                    Some(Value::Data(Term::Str(s))) => s.clone(),
                    _ => {
                        return Err(cli_err(
                            EX_INTERNAL,
                            "selfhost/bad-return",
                            "selfhost vcs hash return missing :hash string",
                        ));
                    }
                };
                let kind = match m.get(&TermOrdKey(Term::symbol(":kind"))) {
                    Some(Value::Data(Term::Str(s))) if s == "term" || s == "module" => s.clone(),
                    _ => {
                        return Err(cli_err(
                            EX_INTERNAL,
                            "selfhost/bad-return",
                            "selfhost vcs hash return missing :kind string",
                        ));
                    }
                };
                (hex, kind)
            }
            _ => {
                return Err(cli_err(
                    EX_INTERNAL,
                    "selfhost/bad-return",
                    format!("selfhost vcs hash returned non-map: {}", r.debug_repr()),
                ));
            }
        };
        (hex, kind)
    } else {
        // Keep precedence aligned with native CLI and selfhost handler: try term first,
        // then fall back to module hashing when term parsing fails.
        let (h, kind) = match parse_term(&src) {
            Ok(t) => (gc_coreform::hash_term(&t), "term"),
            Err(_) => {
                let forms = parse_module(&src)
                    .map_err(|e| cli_err(EX_PARSE, "parse/coreform", e.to_string()))?;
                let canon = canonicalize_module(forms)
                    .map_err(|e| cli_err(EX_PARSE, "canon/coreform", e.to_string()))?;
                (hash_module(&canon), "module")
            }
        };
        (hex32(h), kind.to_string())
    };

    let env = JsonEnvelope {
        ok: true,
        kind: "genesis/vcs-hash-v0.2",
        data: Some(serde_json::json!({
            "in": input.display().to_string(),
            "hash": hex,
            "hash_kind": kind,
            "hash_format": "hex",
            "engine": if engine == FmtEngine::Selfhost { "selfhost" } else { "rust" },
        })),
        error: None,
    };

    Ok(CmdOut {
        exit_code: EX_OK,
        stdout: if cli.json {
            String::new()
        } else {
            format!("{}\n", env.data.as_ref().unwrap()["hash"].as_str().unwrap())
        },
        json: serde_json::to_value(env).expect("json"),
    })
}

fn dispatch(cli: &Cli) -> Result<CmdOut, CliError> {
    enforce_selfhost_only_cmd(cli)?;
    match &cli.cmd {
        Cmd::Fmt {
            file,
            check,
            engine,
        } => cmd_fmt(cli, file, *check, *engine),
        Cmd::Eval {
            file,
            engine,
            stage1_pipeline,
            stage1_gate,
            stage2_gate,
        } => cmd_eval(
            cli,
            file,
            *engine,
            *stage1_pipeline,
            *stage1_gate,
            *stage2_gate,
        ),
        Cmd::Test { pkg, caps } => cmd_test(cli, pkg.as_path(), caps.as_deref()),
        Cmd::Pack { pkg } => cmd_pack(cli, pkg.as_path()),
        Cmd::SelfhostArtifact {
            out,
            min_stage2_supported_modules,
            min_stage2_validated_modules,
        } => cmd_selfhost_artifact(
            cli,
            out,
            *min_stage2_supported_modules,
            *min_stage2_validated_modules,
        ),
        Cmd::Run {
            file,
            engine,
            caps,
            log,
        } => cmd_run(cli, file, *engine, caps.as_path(), log),
        Cmd::Replay {
            file,
            engine,
            log,
            store,
        } => cmd_replay(cli, file, *engine, log, store),
        Cmd::Store { caps, log, cmd } => cmd_store(cli, caps.as_path(), log, cmd),
        Cmd::Refs { caps, log, cmd } => cmd_refs(cli, caps.as_path(), log, cmd),
        Cmd::Pkg { caps, log, cmd } => cmd_pkg(cli, caps.as_path(), log, cmd),
        Cmd::Vcs { cmd } => match cmd {
            VcsCmd::Hash { input, engine } => cmd_vcs_hash(cli, input, *engine),
        },
    }
}

fn main() -> std::process::ExitCode {
    let cli = Cli::parse();
    match dispatch(&cli) {
        Ok(out) => {
            if cli.json {
                println!("{}", serde_json::to_string(&out.json).expect("json"));
            } else if !out.stdout.is_empty() {
                print!("{}", out.stdout);
            }
            std::process::ExitCode::from(out.exit_code)
        }
        Err(e) => {
            if cli.json {
                println!(
                    "{}",
                    serde_json::to_string(&JsonEnvelope::<serde_json::Value> {
                        ok: false,
                        kind: "genesis/error-v0.2",
                        data: None,
                        error: Some(e.json),
                    })
                    .expect("json")
                );
            } else {
                eprintln!("{}", e.json.message);
                if let Some(ctx) = e.json.context
                    && let Some(s) = ctx.as_str()
                    && !s.is_empty()
                {
                    eprintln!("{s}");
                }
            }
            std::process::ExitCode::from(e.exit_code)
        }
    }
}
