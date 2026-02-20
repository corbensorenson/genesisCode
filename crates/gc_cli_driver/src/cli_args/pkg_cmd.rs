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

        /// Lock path.
        #[arg(long, default_value = "genesis.lock")]
        lock: PathBuf,

        /// Workspace descriptor path.
        #[arg(long, default_value = "genesis.workspace.toml")]
        workspace_file: PathBuf,

        /// Environment output root.
        #[arg(long, default_value = ".genesis/env")]
        out_dir: PathBuf,
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
}
