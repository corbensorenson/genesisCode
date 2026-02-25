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
    #[arg(
        long,
        global = true,
        value_enum,
        default_value_t = SelfhostBootstrapArg::ArtifactOnly,
        help = selfhost_bootstrap_help()
    )]
    selfhost_bootstrap: SelfhostBootstrapArg,

    /// Enforce selfhost-only execution for frontend paths.
    ///
    /// In this mode, commands that accept `--engine` must use `--engine selfhost`, and
    /// selfhost bootstrap mode must be `artifact-only` (no embedded fallback).
    /// This can also be enabled via `GENESIS_SELFHOST_ONLY=1`.
    #[arg(long, global = true, default_value_t = false)]
    selfhost_only: bool,

    /// CoreForm frontend for command groups that do not expose `--engine`.
    /// Defaults to `selfhost` in the fast-path profile.
    #[arg(long, global = true, help = coreform_frontend_help())]
    coreform_frontend: Option<CoreformFrontendArg>,

    #[command(subcommand)]
    cmd: Cmd,
}

#[derive(Debug, Clone, Copy, ValueEnum)]
enum SelfhostBootstrapArg {
    ArtifactOnly,
    #[cfg(feature = "parity-harness")]
    ArtifactPreferred,
    #[cfg(feature = "parity-harness")]
    Embedded,
}

impl SelfhostBootstrapArg {
    fn as_str(self) -> &'static str {
        match self {
            Self::ArtifactOnly => "artifact-only",
            #[cfg(feature = "parity-harness")]
            Self::ArtifactPreferred => "artifact-preferred",
            #[cfg(feature = "parity-harness")]
            Self::Embedded => "embedded",
        }
    }
}

const fn selfhost_bootstrap_help() -> &'static str {
    if cfg!(feature = "parity-harness") {
        "Selfhost bootstrap mode. Production mode is `artifact-only`; parity harness may opt into development bootstrap modes."
    } else {
        "Selfhost bootstrap mode. Accepted value: artifact-only."
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum CoreformFrontendArg {
    #[cfg(feature = "parity-harness")]
    Rust,
    Selfhost,
}

const fn coreform_frontend_help() -> &'static str {
    if cfg!(feature = "parity-harness") {
        "CoreForm frontend. Accepted values: selfhost, rust."
    } else {
        "CoreForm frontend. Accepted value: selfhost."
    }
}

fn coreform_frontend_expected_values() -> &'static str {
    if cfg!(feature = "parity-harness") {
        "`selfhost` or `rust`"
    } else {
        "`selfhost`"
    }
}

impl std::str::FromStr for CoreformFrontendArg {
    type Err = String;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        let expected = coreform_frontend_expected_values();
        match s.trim().to_ascii_lowercase().as_str() {
            "selfhost" => Ok(Self::Selfhost),
            #[cfg(feature = "parity-harness")]
            "rust" => Ok(Self::Rust),
            other => Err(format!("invalid frontend `{other}`; expected {expected}")),
        }
    }
}

impl CoreformFrontendArg {
    fn as_str(self) -> &'static str {
        match self {
            #[cfg(feature = "parity-harness")]
            Self::Rust => "rust",
            Self::Selfhost => "selfhost",
        }
    }
}

#[derive(Subcommand)]
enum Cmd {
    /// Canonical formatting for CoreForm (.gc) files.
    Fmt {
        file: PathBuf,
        /// Fail if the file is not already canonically formatted.
        #[arg(long)]
        check: bool,
        /// Formatting engine (selfhost by default in production profile).
        #[arg(long, help = fmt_engine_help())]
        engine: Option<FmtEngine>,
    },

    /// Evaluate a CoreForm program/module (pure).
    Eval {
        file: PathBuf,
        /// Frontend engine (selfhost by default in production profile).
        #[arg(long, help = fmt_engine_help())]
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

    /// Explain contract dispatch path for a given message.
    Explain {
        file: PathBuf,
        /// Frontend engine (selfhost by default in production profile).
        #[arg(long, help = fmt_engine_help())]
        engine: Option<FmtEngine>,
        /// Contract expression or symbol (CoreForm).
        #[arg(long)]
        contract: String,
        /// Message datum (CoreForm term, usually (msg op payload)).
        #[arg(long)]
        msg: String,
    },

    /// Run an effect program with a capability policy and emit a deterministic log.
    Run {
        file: PathBuf,
        /// Frontend engine (selfhost by default in production profile).
        #[arg(long, help = fmt_engine_help())]
        engine: Option<FmtEngine>,
        /// Capability policy TOML (deny-by-default allowlist).
        #[arg(long)]
        caps: PathBuf,
        /// Output effect log path (.gclog). Defaults to <file>.gclog
        #[arg(long)]
        log: Option<PathBuf>,
    },

    /// Replay an effect program deterministically from a log (hard-fails on mismatch).
    Replay {
        file: PathBuf,
        /// Frontend engine (selfhost by default in production profile).
        #[arg(long, help = fmt_engine_help())]
        engine: Option<FmtEngine>,
        /// Input effect log path (.gclog).
        #[arg(long)]
        log: PathBuf,
        /// Artifact store directory for logs that externalize large responses.
        #[arg(long)]
        store: Option<PathBuf>,
    },

    /// Deterministic debug/trace commands for contract dispatch root-cause loops.
    Debug {
        #[command(subcommand)]
        cmd: DebugCmd,
    },

    /// Run package obligations (unit tests, determinism, replay checks, etc.) and write evidence into .genesis/store.
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
        /// Explicitly recover from a missing/corrupt artifact seed by rebuilding from
        /// `selfhost/toolchain_manifest.gc` module sources via deterministic Rust-side
        /// parse/canonical/hash + stage validation.
        ///
        /// This flag is an auditable emergency path and is only consulted when artifact-only
        /// bootstrap cannot find a usable seed artifact.
        #[arg(long, default_value_t = false)]
        recover_missing_artifact: bool,
    },

    /// Emit a selfhost cutover dashboard artifact and markdown mirror.
    SelfhostDashboard {
        /// Markdown mirror output path.
        #[arg(long)]
        markdown: Option<PathBuf>,

        /// Content-addressed store directory (default: ./.genesis/store).
        #[arg(long)]
        store: Option<PathBuf>,
    },

    /// Warm startup mode: process newline-delimited JSON requests in one long-lived process.
    ///
    /// Input format (one JSON object per line on stdin):
    ///   {"argv":["--json","eval","file.gc"]}
    ///
    /// Output format (one JSON object per line on stdout):
    ///   { "ok": true|false, "kind": "genesis/warm-response-v0.1", ... }
    Warm {
        /// Preload selfhost toolchain once before request handling.
        #[arg(long, default_value_t = true, action = clap::ArgAction::Set)]
        prime_selfhost: bool,
    },

    /// Emit machine-readable CLI command/option schema for agent planning.
    CliSchema,

    /// Emit AI-facing planning index (CLI schema + capability indices + workflow pointers).
    AgentIndex,

    /// Generate a new Ed25519 signing key.
    Keygen {
        /// Output key TOML path.
        #[arg(long)]
        out: PathBuf,
    },

    /// Sign a package acceptance artifact and record the signature in the evidence store.
    Sign {
        /// Path to package.toml
        #[arg(long)]
        pkg: PathBuf,

        /// Signing key TOML path (from `genesis keygen`).
        #[arg(long)]
        key: PathBuf,

        /// Acceptance artifact hash to sign (defaults to .genesis/last_acceptance).
        #[arg(long)]
        acceptance: Option<String>,

        /// Signature set file to update (defaults to .genesis/signatures.gc).
        #[arg(long)]
        signatures: Option<PathBuf>,
    },

    /// Verify the local transparency log chain (if present).
    TransparencyVerify {
        /// Path to package.toml
        #[arg(long)]
        pkg: PathBuf,
    },

    /// Run the (gradual) type/effect checker for a package.
    Typecheck {
        /// Path to package.toml
        #[arg(long)]
        pkg: PathBuf,
    },

    /// Optimize a CoreForm module/program (pure subset only).
    Optimize {
        file: PathBuf,
        /// Output path (defaults to stdout).
        #[arg(long)]
        out: Option<PathBuf>,
        /// Optional output path for stage-2 lowered WebAssembly bytes.
        #[arg(long)]
        emit_wasm: Option<PathBuf>,
        /// Frontend engine (selfhost by default in production profile).
        #[arg(long, help = fmt_engine_help())]
        engine: Option<FmtEngine>,
        /// Require `core/obligation::stage1-validation` to pass.
        #[arg(long)]
        stage1_gate: bool,
        /// Require stage-2 CoreForm->WASM translation validation to pass for this module.
        #[arg(long)]
        stage2_gate: bool,
    },

    /// Validate and apply a semantic patch, then re-run package obligations.
    ApplyPatch {
        patch: PathBuf,
        /// Path to package.toml
        #[arg(long)]
        pkg: PathBuf,
        /// Override capability policy TOML for effectful tests.
        #[arg(long)]
        caps: Option<PathBuf>,
    },

    /// Semantic edit tooling for agentic patch planning.
    #[command(visible_alias = "semedit")]
    SemanticEdit {
        #[command(subcommand)]
        cmd: SemanticEditCmd,
    },

    /// Verify package hashes and evidence store integrity.
    Verify {
        /// Path to package.toml
        #[arg(long)]
        pkg: PathBuf,

        /// Acceptance artifact hash to verify (defaults to .genesis/last_acceptance if present).
        #[arg(long)]
        acceptance: Option<String>,

        /// Registry policy TOML. When provided, signature policy is enforced.
        #[arg(long)]
        policy: Option<PathBuf>,

        /// Signature set file (CoreForm term vector of signature artifact hashes).
        /// Defaults to .genesis/signatures.gc when --policy is provided.
        #[arg(long)]
        signatures: Option<PathBuf>,

        /// Scan the entire evidence store and verify name->content hashes (can be slow).
        #[arg(long)]
        scan_store: bool,
    },

    /// Content-addressed store operations (effectful; policy-gated).
    Store {
        /// Capability policy TOML (deny-by-default allowlist).
        #[arg(long)]
        caps: PathBuf,

        /// Output effect log path (.gclog). Defaults to ./.genesis/logs/<op>-<stamp>.gclog
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

        /// Output effect log path (.gclog). Defaults to ./.genesis/logs/<op>-<stamp>.gclog
        #[arg(long)]
        log: Option<PathBuf>,

        #[command(subcommand)]
        cmd: RefsCmd,
    },

    /// Create and inspect GenesisGraph commit artifacts.
    Commit {
        /// Capability policy TOML (deny-by-default allowlist).
        #[arg(long)]
        caps: PathBuf,

        /// Output effect log path (.gclog). Defaults to ./.genesis/logs/<op>-<stamp>.gclog
        #[arg(long)]
        log: Option<PathBuf>,

        #[command(subcommand)]
        cmd: CommitCmd,
    },

    /// GenesisPkg/GCPM operations (snapshot + bundle export/import).
    #[command(visible_alias = "gcpm")]
    Pkg {
        /// Capability policy TOML (deny-by-default allowlist).
        #[arg(long)]
        caps: PathBuf,

        /// Output effect log path (.gclog). Defaults to ./.genesis/logs/<op>-<stamp>.gclog
        #[arg(long)]
        log: Option<PathBuf>,

        #[command(subcommand)]
        cmd: PkgCmd,
    },

    /// Local policy alias/default management.
    Policy {
        #[command(subcommand)]
        cmd: PolicyCmd,
    },

    /// Sync artifacts and refs with a remote registry (effectful; policy-gated).
    Sync {
        /// Capability policy TOML (deny-by-default allowlist).
        #[arg(long)]
        caps: PathBuf,

        /// Output effect log path (.gclog). Defaults to ./.genesis/logs/<op>-<stamp>.gclog
        #[arg(long)]
        log: Option<PathBuf>,

        #[command(subcommand)]
        cmd: SyncCmd,
    },

    /// First-party Genesis registry server operations.
    Registry {
        #[command(subcommand)]
        cmd: RegistryCmd,
    },

    /// Garbage-collect the local artifact store using reachability closure from refs + locks + pins.
    Gc {
        /// Capability policy TOML (deny-by-default allowlist).
        #[arg(long)]
        caps: PathBuf,

        /// Output effect log path (.gclog). Defaults to ./.genesis/logs/<op>-<stamp>.gclog
        #[arg(long)]
        log: Option<PathBuf>,

        #[command(subcommand)]
        cmd: GcCmd,
    },

    /// GenesisGraph commit DAG operations.
    Vcs {
        /// Capability policy TOML (deny-by-default allowlist).
        ///
        /// Required for effectful VCS operations; omitted for pure ops like `vcs hash`.
        #[arg(long)]
        caps: Option<PathBuf>,

        /// Output effect log path (.gclog). Defaults to ./.genesis/logs/<op>-<stamp>.gclog
        #[arg(long)]
        log: Option<PathBuf>,

        #[command(subcommand)]
        cmd: VcsCmd,
    },
}

#[derive(Copy, Clone, Debug, Eq, PartialEq)]
enum FmtEngine {
    #[cfg(feature = "parity-harness")]
    Rust,
    Selfhost,
}

fn fmt_engine_expected_values() -> &'static str {
    if cfg!(feature = "parity-harness") {
        "`selfhost` or `rust`"
    } else {
        "`selfhost`"
    }
}

const fn fmt_engine_help() -> &'static str {
    if cfg!(feature = "parity-harness") {
        "Frontend engine. Accepted values: selfhost, rust."
    } else {
        "Frontend engine. Accepted value: selfhost."
    }
}

impl std::str::FromStr for FmtEngine {
    type Err = String;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        let expected = fmt_engine_expected_values();
        match s.trim().to_ascii_lowercase().as_str() {
            "selfhost" => Ok(Self::Selfhost),
            #[cfg(feature = "parity-harness")]
            "rust" => Ok(Self::Rust),
            other => Err(format!("invalid engine `{other}`; expected {expected}")),
        }
    }
}

impl FmtEngine {
    fn as_str(self) -> &'static str {
        match self {
            #[cfg(feature = "parity-harness")]
            Self::Rust => "rust",
            Self::Selfhost => "selfhost",
        }
    }
}

include!("cli_args/command_groups.rs");

include!("cli_args/pkg_cmd.rs");
include!("cli_args/policy_gc_vcs_cmd.rs");
