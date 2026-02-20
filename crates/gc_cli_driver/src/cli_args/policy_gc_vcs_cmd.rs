#[derive(Subcommand)]
enum PolicyCmd {
    /// List configured policy aliases and default selection.
    List {
        /// Policy config TOML path.
        #[arg(long, default_value = ".genesis/policies.toml")]
        policies: PathBuf,
    },

    /// Show a policy artifact by alias or hash.
    Show {
        /// Alias name from policies config, `default`, or 64-hex policy hash.
        name_or_hash: String,

        /// Policy config TOML path.
        #[arg(long, default_value = ".genesis/policies.toml")]
        policies: PathBuf,

        /// Artifact store directory containing policy objects.
        #[arg(long, default_value = ".genesis/store")]
        store: PathBuf,
    },

    /// Set the default policy alias/hash.
    SetDefault {
        /// Alias name from policies config or 64-hex policy hash.
        name_or_hash: String,

        /// Policy config TOML path.
        #[arg(long, default_value = ".genesis/policies.toml")]
        policies: PathBuf,
    },
}

#[derive(Subcommand)]
enum GcCmd {
    /// Plan GC: compute live/dead sets and estimate reclaimable bytes.
    Plan {
        /// Lock path (relative to the capability base_dir).
        #[arg(long, default_value = "genesis.lock")]
        lock: PathBuf,

        /// Pins TOML path (relative to the capability base_dir).
        #[arg(long, default_value = ".genesis/pins.toml")]
        pins: PathBuf,

        /// Commit parent depth to include when roots are commits.
        #[arg(long, default_value_t = 200)]
        depth: u64,

        /// Do not include roots from the lock file.
        #[arg(long)]
        no_lock: bool,

        /// Do not include roots from the refs database.
        #[arg(long)]
        no_refs: bool,
    },

    /// Execute GC: delete or quarantine dead artifacts.
    Run {
        /// Lock path (relative to the capability base_dir).
        #[arg(long, default_value = "genesis.lock")]
        lock: PathBuf,

        /// Pins TOML path (relative to the capability base_dir).
        #[arg(long, default_value = ".genesis/pins.toml")]
        pins: PathBuf,

        /// Commit parent depth to include when roots are commits.
        #[arg(long, default_value_t = 200)]
        depth: u64,

        /// Do not include roots from the lock file.
        #[arg(long)]
        no_lock: bool,

        /// Do not include roots from the refs database.
        #[arg(long)]
        no_refs: bool,

        /// Move dead artifacts into quarantine instead of deleting them.
        #[arg(long)]
        quarantine: bool,

        /// Quarantine directory (relative to capability base_dir). Defaults to .genesis/quarantine.
        #[arg(long)]
        quarantine_dir: Option<PathBuf>,
    },

    /// Add a pin (hash or ref) so GC will retain it.
    Pin {
        target: String,

        /// Pins TOML path (relative to the capability base_dir).
        #[arg(long, default_value = ".genesis/pins.toml")]
        pins: PathBuf,
    },

    /// Remove a pin (hash or ref).
    Unpin {
        target: String,

        /// Pins TOML path (relative to the capability base_dir).
        #[arg(long, default_value = ".genesis/pins.toml")]
        pins: PathBuf,
    },

    /// Purge quarantined artifacts older than a TTL (days).
    Purge {
        /// Purge threshold in days. `0` means purge everything present.
        #[arg(long)]
        ttl_days: u64,

        /// Quarantine directory (relative to capability base_dir). Defaults to .genesis/quarantine.
        #[arg(long)]
        quarantine_dir: Option<PathBuf>,
    },
}

#[derive(Subcommand)]
enum VcsCmd {
    /// Hash a CoreForm term (or module) without mutating the store.
    Hash {
        /// Input file containing a single CoreForm term or a CoreForm module.
        #[arg(long = "in", alias = "input")]
        input: PathBuf,
        /// Frontend engine (selfhost by default in production profile).
        #[arg(long, help = fmt_engine_help())]
        engine: Option<FmtEngine>,
    },

    /// Compute a semantic patch between two snapshot hashes.
    Diff {
        /// Base snapshot hash (hex).
        #[arg(long)]
        base: String,

        /// Target snapshot hash (hex).
        #[arg(long)]
        to: String,

        /// Optional output path for the patch term (relative to capability base_dir).
        #[arg(long)]
        out: Option<PathBuf>,

        /// If set, do not store the patch artifact (value artifacts may still be stored).
        #[arg(long)]
        no_store: bool,
    },

    /// Apply a patch to a base snapshot.
    Apply {
        /// Base snapshot hash (hex).
        #[arg(long)]
        base: String,

        /// Patch hash (hex) or a patch file path (relative to capability base_dir).
        #[arg(long)]
        patch: String,

        /// Optional output path for the resulting snapshot term (relative to capability base_dir).
        #[arg(long)]
        out: Option<PathBuf>,

        /// If set, do not store the resulting snapshot artifact.
        #[arg(long)]
        no_store: bool,
    },

    /// Walk the commit DAG starting from a commit hash or ref name and print the visited commits.
    Log {
        /// Commit hash (hex) or ref name (refs/...).
        root: String,

        /// Maximum number of commits to emit before truncating.
        #[arg(long, default_value_t = 1000)]
        max: u64,
    },

    /// Attribute a symbol in a snapshot to the commit that introduced its current artifact hash.
    Blame {
        /// Snapshot hash (hex).
        #[arg(long)]
        snapshot: String,

        /// Qualified symbol to attribute.
        #[arg(long)]
        sym: String,

        /// Optional structural path hint (forwarded to the capability payload).
        #[arg(long)]
        path: Option<String>,
    },

    /// Explain symbol provenance and evidence context for a snapshot.
    Why {
        /// Snapshot hash (hex).
        #[arg(long)]
        snapshot: String,

        /// Qualified symbol to explain.
        #[arg(long)]
        sym: String,

        /// Optional op symbol for additional context.
        #[arg(long)]
        op: Option<String>,
    },

    /// 3-way semantic merge of contract snapshots (op-table merge; emits conflict artifact on divergence).
    Merge3 {
        /// Base snapshot hash (hex).
        #[arg(long)]
        base: String,

        /// Left snapshot hash (hex).
        #[arg(long)]
        left: String,

        /// Right snapshot hash (hex).
        #[arg(long)]
        right: String,

        /// Optional output path for the merged snapshot or conflict term (relative to capability base_dir).
        #[arg(long)]
        out: Option<PathBuf>,
    },

    /// Resolve a `:vcs/conflict` artifact (currently supports `:kind :contract`) into a merged snapshot and patch.
    ResolveConflict {
        /// Conflict artifact hash (hex).
        #[arg(long)]
        conflict: String,

        /// Default conflict resolution strategy for all ops without an explicit `--pick/--set` override.
        /// One of: left, right, base.
        #[arg(long)]
        strategy: Option<String>,

        /// Per-op side pick in the form `op=left|right|base`. May be repeated.
        #[arg(long = "pick")]
        picks: Vec<String>,

        /// Per-op explicit handler hash in the form `op=<64-hex>`. May be repeated.
        #[arg(long = "set")]
        sets: Vec<String>,

        /// Optional output path for the resulting patch term (relative to capability base_dir).
        #[arg(long)]
        out: Option<PathBuf>,
    },
}
