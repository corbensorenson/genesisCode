#[derive(Subcommand)]
enum SemanticEditCmd {
    /// Index canonical AST nodes with stable semantic node IDs.
    Index {
        /// Path to package.toml.
        #[arg(long)]
        pkg: PathBuf,

        /// Module path relative to package directory.
        #[arg(long)]
        module_path: String,
    },

    /// Export a deterministic workspace semantic symbol/dependency graph.
    WorkspaceGraph {
        /// Path to package.toml.
        #[arg(long)]
        pkg: PathBuf,
    },

    /// Build a deterministic multi-file refactor patch plan with conflict previews.
    RefactorPlan {
        /// Path to package.toml.
        #[arg(long)]
        pkg: PathBuf,

        /// Refactor operation kind.
        #[arg(long, value_enum)]
        kind: RefactorKind,

        /// Source symbol to rewrite (for example `my/pkg::foo`).
        #[arg(long = "from")]
        from_symbol: String,

        /// Destination symbol to rewrite to (for example `my/pkg::foo_v2`).
        #[arg(long = "to")]
        to_symbol: String,

        /// Required for `move` and `extract`: destination module path (relative to package dir).
        #[arg(long)]
        target_module_path: Option<String>,
    },

    /// Plan and apply a deterministic multi-file refactor patch in one obligation-gated command.
    ApplyPlan {
        /// Path to package.toml.
        #[arg(long)]
        pkg: PathBuf,

        /// Refactor operation kind.
        #[arg(long, value_enum)]
        kind: RefactorKind,

        /// Source symbol to rewrite (for example `my/pkg::foo`).
        #[arg(long = "from")]
        from_symbol: String,

        /// Destination symbol to rewrite to (for example `my/pkg::foo_v2`).
        #[arg(long = "to")]
        to_symbol: String,

        /// Required for `move` and `extract`: destination module path (relative to package dir).
        #[arg(long)]
        target_module_path: Option<String>,

        /// Optional capability policy path used when applying the generated patch.
        #[arg(long)]
        caps: Option<PathBuf>,
    },
}

#[derive(Args, Clone)]
struct DebugTraceArgs {
    /// Path to module file containing the contract definition.
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
    /// Optional output path for canonicalized explain trace artifact.
    #[arg(long)]
    trace_out: Option<PathBuf>,
}

#[derive(Args, Clone)]
struct DebugLayerArtifactArgs {
    /// Optional planner JSON envelope (`genesis/agent-plan-v0.1`) included as timeline layer.
    #[arg(long)]
    planner_json: Option<PathBuf>,
    /// Optional typecheck JSON envelope (`genesis/typecheck-v0.2`) included as timeline layer.
    #[arg(long)]
    typecheck_json: Option<PathBuf>,
    /// Optional optimize JSON envelope (`genesis/optimize-v0.2`) included as timeline layer.
    #[arg(long)]
    optimize_json: Option<PathBuf>,
    /// Optional effect log (`.gclog`) included as timeline effect-boundary layer.
    #[arg(long)]
    effect_log: Option<PathBuf>,
}

#[derive(Subcommand)]
enum DebugCmd {
    /// Advance a deterministic trace cursor by N steps and return the selected frame.
    Step {
        #[command(flatten)]
        trace: DebugTraceArgs,
        /// Trace cursor index (0-based, next frame to execute).
        #[arg(long, default_value_t = 0)]
        cursor: u64,
        /// Number of frames to advance.
        #[arg(long, default_value_t = 1)]
        count: u64,
    },
    /// Find first frame matching a key/value condition starting at index.
    Break {
        #[command(flatten)]
        trace: DebugTraceArgs,
        /// Start index for breakpoint scan (0-based).
        #[arg(long, default_value_t = 0)]
        start: u64,
        /// Step map key to match (for example `:override` or `override`).
        #[arg(long = "match-key")]
        match_key: String,
        /// CoreForm term expected at `--match-key`.
        #[arg(long = "match-value")]
        match_value: String,
    },
    /// Inspect a specific trace frame by index.
    Inspect {
        #[command(flatten)]
        trace: DebugTraceArgs,
        /// Frame index (0-based).
        #[arg(long, default_value_t = 0)]
        index: u64,
    },
    /// Continue from cursor until end or breakpoint condition match.
    Continue {
        #[command(flatten)]
        trace: DebugTraceArgs,
        /// Trace cursor index (0-based, next frame to execute).
        #[arg(long, default_value_t = 0)]
        cursor: u64,
        /// Optional step map key to match (for example `:override`).
        #[arg(long = "match-key")]
        match_key: Option<String>,
        /// Optional CoreForm term expected at `--match-key`.
        #[arg(long = "match-value")]
        match_value: Option<String>,
    },
    /// List trace frames as a deterministic window.
    Frames {
        #[command(flatten)]
        trace: DebugTraceArgs,
        /// Window start index (0-based).
        #[arg(long, default_value_t = 0)]
        start: u64,
        /// Maximum number of frames in the window.
        #[arg(long)]
        limit: Option<u64>,
    },
    /// Build a deterministic cross-layer timeline artifact for agent remediation loops.
    Timeline {
        #[command(flatten)]
        trace: DebugTraceArgs,
        #[command(flatten)]
        layers: DebugLayerArtifactArgs,
        /// Window start index (0-based) over unified timeline frames.
        #[arg(long, default_value_t = 0)]
        start: u64,
        /// Maximum number of timeline frames in the output window.
        #[arg(long)]
        limit: Option<u64>,
        /// Optional output path for canonical timeline artifact.
        #[arg(long)]
        out: Option<PathBuf>,
    },
    /// Deterministically bisect two timeline artifacts and return first mismatch frame.
    Bisect {
        /// Baseline timeline artifact path produced by `debug timeline --out`.
        #[arg(long)]
        baseline: PathBuf,
        /// Candidate timeline artifact path produced by `debug timeline --out`.
        #[arg(long)]
        candidate: PathBuf,
    },
}

#[derive(Copy, Clone, Debug, Eq, PartialEq, ValueEnum)]
enum RefactorKind {
    Rename,
    Move,
    Extract,
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
    /// Verify artifact integrity by hash or scan the whole local store.
    Verify {
        /// Optional content hash (hex). If omitted, scans all store blobs.
        hash: Option<String>,
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

#[derive(Copy, Clone, Debug, Eq, PartialEq, ValueEnum)]
enum CommitTargetKind {
    Package,
    Module,
    Contract,
    Workspace,
}

#[derive(Subcommand)]
enum CommitCmd {
    /// Create a `:vcs/commit` artifact from base+patch and optionally store it.
    New {
        /// Target entity kind.
        #[arg(long, value_enum)]
        target_kind: CommitTargetKind,

        /// Target identifier (name/path/symbol).
        #[arg(long)]
        target_id: String,

        /// Base snapshot hash (64-hex) or ref name (`refs/...`).
        #[arg(long)]
        base: String,

        /// Patch hash (64-hex) or patch artifact file path.
        #[arg(long)]
        patch: String,

        /// Commit message.
        #[arg(long)]
        message: String,

        /// Optional rationale.
        #[arg(long)]
        why: Option<String>,

        /// Required obligation symbol (repeatable).
        #[arg(long = "obligation")]
        obligations: Vec<String>,

        /// Evidence artifact hash (64-hex, repeatable).
        #[arg(long = "evidence")]
        evidence: Vec<String>,

        /// Optional author display name.
        #[arg(long)]
        author: Option<String>,

        /// Optional signer key id hint (stored as author id metadata).
        #[arg(long)]
        sign: Option<String>,

        /// Store the commit artifact in the content-addressed store.
        #[arg(long)]
        store: bool,
    },

    /// Load and validate a `:vcs/commit` artifact by hash.
    Show {
        /// Commit hash (64-hex).
        hash: String,
    },
}

#[derive(Subcommand)]
enum SyncCmd {
    /// Pull artifacts reachable from refs and/or explicit roots, and optionally update local refs.
    Pull {
        /// Remote spec (e.g. gen://example.com/registry or https://...).
        #[arg(long)]
        remote: String,

        /// Refs to pull from the remote (may be repeated).
        #[arg(long = "ref")]
        refs: Vec<String>,

        /// Explicit root hashes to pull (may be repeated).
        #[arg(long = "root")]
        roots: Vec<String>,

        /// Commit parent depth to include when roots are commits (0 = no parents).
        #[arg(long, default_value_t = 0)]
        depth: u64,

        /// Overwrite local refs if they differ from the remote.
        #[arg(long)]
        force: bool,
    },

    /// Push artifacts reachable from explicit roots, and optionally advance remote refs (CAS).
    Push {
        /// Remote spec (e.g. gen://example.com/registry or https://...).
        #[arg(long)]
        remote: String,

        /// Explicit root hashes to push (may be repeated).
        #[arg(long = "root")]
        roots: Vec<String>,

        /// Commit parent depth to include when roots are commits (0 = no parents).
        #[arg(long, default_value_t = 0)]
        depth: u64,

        /// Optionally advance remote refs after uploading artifacts.
        ///
        /// Format: `<refname>:<commit-hash>:<policy-hash>[@<expected-old-hash|nil>]`
        #[arg(long = "set-ref")]
        set_refs: Vec<String>,
    },
}

#[derive(Subcommand)]
enum RegistryCmd {
    /// Serve a policy-gated HTTP registry backed by a local file store.
    Serve {
        /// Bind address (supports :0 for ephemeral local ports).
        #[arg(long, default_value = "127.0.0.1:8080")]
        addr: String,

        /// Registry root directory; server stores data under <root>/v1.
        #[arg(long)]
        root: PathBuf,

        /// Maximum accepted upload chunk size in bytes.
        #[arg(long, default_value_t = 4_194_304)]
        max_chunk_bytes: u64,

        /// Optional request cap (mainly for deterministic tests).
        #[arg(long)]
        max_requests: Option<u64>,
    },
}
