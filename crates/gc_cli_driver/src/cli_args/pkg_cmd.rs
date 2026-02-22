#[derive(Subcommand)]
enum PkgCmd {
    /// Create a workspace descriptor + lock file in one deterministic step.
    New {
        /// Workspace name.
        #[arg(long)]
        workspace: String,

        /// Lock path.
        #[arg(long, default_value = "genesis.lock")]
        lock: PathBuf,

        /// Workspace descriptor path.
        #[arg(long, default_value = "genesis.workspace.toml")]
        workspace_file: PathBuf,

        /// Workspace policy alias.
        #[arg(long, default_value = "policy:default-v0.1")]
        policy: String,

        /// Default registry remote spec.
        #[arg(long)]
        registry_default: Option<String>,

        /// Optional member specs (`name=path` or `path`), repeatable.
        #[arg(long = "member")]
        members: Vec<String>,
    },

    /// Initialize a `genesis.lock` workspace lock file.
    Init {
        /// Workspace name.
        #[arg(long)]
        workspace: String,

        /// Lock path (relative to the capability base_dir).
        #[arg(long, default_value = "genesis.lock")]
        lock: PathBuf,

        /// Workspace policy alias (stored in lock; not resolved in v0.1).
        #[arg(long, default_value = "policy:default-v0.1")]
        policy: String,

        /// Default registry remote spec (stored in lock).
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

        /// Deterministic resolver strategy for this dependency.
        #[arg(long, value_parser = ["pinned", "track-ref", "tag-policy"])]
        strategy: Option<String>,

        /// Tag policy label when `--strategy tag-policy` is selected.
        #[arg(long)]
        tag_policy: Option<String>,
    },

    /// Remove a dependency requirement (and its locked entry) from `genesis.lock`.
    Remove {
        name: String,

        /// Lock path.
        #[arg(long, default_value = "genesis.lock")]
        lock: PathBuf,
    },

    /// Resolve requirements into pinned commits/snapshots in `genesis.lock` (local-only v0.1).
    Lock {
        /// Lock path (relative to the capability base_dir).
        #[arg(long, default_value = "genesis.lock")]
        lock: PathBuf,

        /// Perform strict checks while resolving locks: validate commit/snapshot/evidence integrity.
        #[arg(long)]
        strict: bool,
    },

    /// Update locked entries for tracked refs (`update_policy=auto`) (local-only v0.1).
    Update {
        /// Lock path (relative to the capability base_dir).
        #[arg(long, default_value = "genesis.lock")]
        lock: PathBuf,
    },

    /// Run a named workspace task from `genesis.workspace.toml` as canonical command data.
    Run {
        /// Task name.
        task: String,

        /// Workspace descriptor path.
        #[arg(long, default_value = "genesis.workspace.toml")]
        workspace_file: PathBuf,
    },

    /// Build a deterministic deployment bundle target from a package manifest.
    Build {
        /// Path to package.toml.
        #[arg(long, default_value = "package.toml")]
        pkg: PathBuf,

        /// Deployment target contract.
        #[arg(long, value_parser = ["web", "desktop", "service"])]
        target: String,

        /// Bundle output root directory.
        #[arg(long, default_value = ".genesis/build")]
        out_dir: PathBuf,
    },

    /// Run package obligations (gcpm alias for `genesis test`).
    Test {
        /// Path to package.toml.
        #[arg(long, default_value = "package.toml")]
        pkg: PathBuf,

        /// Optional capability policy override for effectful tests.
        #[arg(long)]
        caps: Option<PathBuf>,
    },

    /// Closed-loop module self-optimization gated by translation validation + obligations.
    SelfOptimize {
        /// Path to package.toml.
        #[arg(long, default_value = "package.toml")]
        pkg: PathBuf,

        /// Optional capability policy override for effectful validation tests.
        #[arg(long)]
        caps: Option<PathBuf>,

        /// Evaluate candidate rewrite, emit proof artifacts, but do not promote file changes.
        #[arg(long)]
        dry_run: bool,
    },

    /// Build a deterministic requirements-trace evidence artifact for regulated assurance workflows.
    Trace {
        /// Path to package.toml.
        #[arg(long, default_value = "package.toml")]
        pkg: PathBuf,

        /// Requirements graph CoreForm term path.
        #[arg(long, default_value = "requirements.gc")]
        requirements: PathBuf,

        /// Optional release commit hash this trace binds to.
        /// When omitted, trace verification is snapshot+policy anchored and avoids commit/evidence hash cycles.
        #[arg(long)]
        commit: Option<String>,

        /// Release snapshot hash this trace binds to.
        #[arg(long)]
        snapshot: String,

        /// Policy artifact hash this trace was verified against.
        #[arg(long)]
        policy: Option<String>,

        /// Output path for the generated evidence term.
        #[arg(long, default_value = ".genesis/assurance/requirements_trace.gc")]
        out: PathBuf,

        /// Do not import generated artifacts into `.genesis/store`.
        #[arg(long)]
        no_store: bool,
    },

    /// Build a deterministic tool-qualification evidence artifact for protected releases.
    Qualify {
        /// Optional release commit hash this qualification binds to.
        /// When omitted, qualification is policy anchored and can be attached pre-commit.
        #[arg(long)]
        commit: Option<String>,

        /// Policy artifact hash this qualification was validated against.
        #[arg(long)]
        policy: Option<String>,

        /// Qualification profile label (e.g. release-full, ci, dal-a).
        #[arg(long, default_value = "release-full")]
        profile: String,

        /// Qualification requirement identifier (repeatable).
        #[arg(long = "requirement")]
        requirements: Vec<String>,

        /// Qualification test artifact link in the form `id=<64-hex>`, repeatable.
        #[arg(long = "test-artifact")]
        test_artifacts: Vec<String>,

        /// Tool descriptor in the form `name=/absolute/or/relative/path`, repeatable.
        #[arg(long = "tool")]
        tools: Vec<String>,

        /// Output path for the generated evidence term.
        #[arg(long, default_value = ".genesis/assurance/tool_qualification.gc")]
        out: PathBuf,

        /// Do not import generated artifacts into `.genesis/store`.
        #[arg(long)]
        no_store: bool,
    },

    /// Build a deterministic certification-oriented assurance pack artifact and optional audit bundle directory.
    AssurancePack {
        /// Path to package.toml used for obligation-link validation.
        #[arg(long, default_value = "package.toml")]
        pkg: PathBuf,

        /// Target assurance profile contract.
        #[arg(
            long = "assurance-profile",
            default_value = "custom",
            value_parser = [
                "custom",
                "do178c-dal-a",
                "do178c-dal-b",
                "nasa-class-a",
                "nasa-class-b",
                "iec62304-class-c"
            ]
        )]
        assurance_profile: String,

        /// Optional release commit hash this pack binds to.
        #[arg(long)]
        commit: Option<String>,

        /// Release snapshot hash this pack binds to.
        #[arg(long)]
        snapshot: String,

        /// Optional policy hash this pack was verified against.
        #[arg(long)]
        policy: Option<String>,

        /// Requirements-trace artifact path or store hash.
        #[arg(long, default_value = ".genesis/assurance/requirements_trace.gc")]
        trace: String,

        /// Tool-qualification artifact path or store hash.
        #[arg(long, default_value = ".genesis/assurance/tool_qualification.gc")]
        qualification: String,

        /// Coverage report artifact path or store hash (repeatable).
        #[arg(long = "coverage")]
        coverage: Vec<String>,

        /// Independence attestation in `<left-role>:<right-role>@<attestor>` format (repeatable).
        #[arg(long = "independence-attestation")]
        independence_attestations: Vec<String>,

        /// Output path for the generated assurance pack term.
        #[arg(long, default_value = ".genesis/assurance/assurance_pack.gc")]
        out: PathBuf,

        /// Optional output directory for a deterministic audit bundle mirror.
        #[arg(long)]
        bundle_dir: Option<PathBuf>,

        /// Do not import generated assurance pack into `.genesis/store`.
        #[arg(long)]
        no_store: bool,
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

    /// Diagnose workspace/package lock and capability configuration with deterministic fix hints.
    Doctor {
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

    /// Export deterministic package ABI/introspection index for agent planning.
    Abi {
        /// Path to package.toml (relative to the capability base_dir).
        #[arg(long, default_value = "package.toml")]
        pkg: PathBuf,
    },

    /// Build and store a `:vcs/snapshot` for a `package.toml`.
    Snapshot {
        /// Path to package.toml (relative to the capability base_dir).
        #[arg(long)]
        pkg: PathBuf,
    },

    /// Export a shallow `.gpk` bundle from a snapshot hash.
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
        /// Format: `<refname>=<commit-hash|nil>[@<expected-old-hash|nil>]`.
        #[arg(long = "set-ref")]
        set_refs: Vec<String>,

        /// Policy artifact hash (hex) used by the local refs/set gate (required when using --set-ref).
        #[arg(long)]
        policy: Option<String>,
    },

    /// Publish a commit to a remote registry and advance a remote ref (policy-gated).
    ///
    /// This is the "pip publish" equivalent: upload reachable artifacts and set the remote ref.
    Publish {
        /// Remote spec (e.g. gen://example.com/registry or https://...).
        #[arg(long)]
        remote: String,

        /// Remote ref to advance (e.g. refs/heads/main, refs/tags/v1.0.0).
        #[arg(long = "ref")]
        refname: String,

        /// Policy artifact hash (hex) used by the remote refs/set gate.
        #[arg(long)]
        policy: String,

        /// Optional optimistic concurrency check for the remote ref.
        /// Pass a hex hash, or the literal string `nil` to require the ref to be unset.
        #[arg(long)]
        expected_old: Option<String>,

        /// Commit parent depth to include when publishing (0 = no parents).
        #[arg(long, default_value_t = 0)]
        depth: u64,

        /// Commit hash to publish. If omitted, resolves from the local `refname` in the refs db.
        #[arg(long)]
        commit: Option<String>,
    },

    /// Realize a deterministic workspace environment profile under `.genesis/env/<profile-hash>/`.
    Env {
        /// Profile name (e.g. dev|ci|release).
        #[arg(long, default_value = "dev")]
        profile: String,

        /// Runtime backend profile contract for this realized environment.
        ///
        /// Accepted values: `headless|gpu|gfx|backend` and `profile-*` aliases.
        /// When omitted, resolves from workspace profile/defaults.
        #[arg(long = "runtime-backend")]
        runtime_backend: Option<String>,

        /// Lock path.
        #[arg(long, default_value = "genesis.lock")]
        lock: PathBuf,

        /// Workspace descriptor path.
        #[arg(long, default_value = "genesis.workspace.toml")]
        workspace_file: PathBuf,

        /// Environment output root.
        #[arg(long, default_value = ".genesis/env")]
        out_dir: PathBuf,

        /// Hydrate missing locked artifacts via policy-gated `core/store::get` before materialization.
        #[arg(long)]
        hydrate: bool,
    },

    /// Migrate a package-only repo into workspace+gcpm mode.
    Migrate {
        /// Path to package.toml.
        #[arg(long, default_value = "package.toml")]
        pkg: PathBuf,

        /// Lock path.
        #[arg(long, default_value = "genesis.lock")]
        lock: PathBuf,

        /// Workspace descriptor path.
        #[arg(long, default_value = "genesis.workspace.toml")]
        workspace_file: PathBuf,

        /// Optional workspace name override.
        #[arg(long)]
        workspace: Option<String>,

        /// Default registry remote spec.
        #[arg(long)]
        registry_default: Option<String>,
    },

    /// Emit deterministic non-gfx runtime profile artifacts and enforce SLO regressions.
    ProfileRuntime {
        /// Output runtime profile artifact path.
        #[arg(long, default_value = ".genesis/perf/runtime_profile.gc")]
        out: PathBuf,

        /// Runtime profile history JSONL path.
        #[arg(long, default_value = ".genesis/perf/runtime_profile_history.jsonl")]
        history: PathBuf,

        /// Minimum history samples required before regression checks are enforced.
        #[arg(long, default_value_t = 5)]
        min_history: u64,

        /// Allowed regression percentage over history p95 (for example `25` means +25%).
        #[arg(long, default_value_t = 100)]
        max_regression_percent: u64,

        /// Skip appending this run to history.
        #[arg(long)]
        no_history_append: bool,

        /// Absolute task scheduler budget in microseconds.
        #[arg(long, default_value_t = 10_000_000)]
        task_budget_us: u64,

        /// Absolute IO store-cycle budget in microseconds.
        #[arg(long, default_value_t = 5_000_000)]
        io_budget_us: u64,

        /// Absolute memory-pressure probe budget in microseconds.
        #[arg(long, default_value_t = 5_000_000)]
        memory_budget_us: u64,
    },
}
