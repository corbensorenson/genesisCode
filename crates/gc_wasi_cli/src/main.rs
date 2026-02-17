use std::path::{Path, PathBuf};

use anyhow::Context;
use clap::{Parser, Subcommand, ValueEnum};
use serde::{Deserialize, Serialize};

use gc_coreform::{
    Term, TermOrdKey, canonicalize_module, hash_module, parse_module, parse_term, print_module,
    print_term,
};
use gc_effects::{
    ArtifactStore, CapsPolicy, Decision, EffectLog, EffectsError, replay_with_store, run,
};
use gc_kernel::{Apply, EvalCtx, MemLimits, StepLimit, Value, eval_module, eval_term};
use gc_obligations::PackageManifest;
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

    /// Explain contract dispatch path for a given message.
    Explain {
        file: PathBuf,
        /// Frontend engine. `rust` uses the Rust CoreForm frontend; `selfhost` runs the
        /// self-hosted CoreForm parser+canonicalizer inside the kernel.
        #[arg(long, value_enum)]
        engine: Option<FmtEngine>,
        /// Contract expression or symbol (CoreForm).
        #[arg(long)]
        contract: String,
        /// Message datum (CoreForm term, usually (msg op payload)).
        #[arg(long)]
        msg: String,
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

    /// Run the (gradual) type/effect checker for a package.
    Typecheck {
        /// Path to package.toml
        #[arg(long)]
        pkg: PathBuf,
    },

    /// Optimize a CoreForm module/program (pure subset only).
    Optimize {
        file: PathBuf,
        /// Write optimized CoreForm output to this file. If omitted, prints to stdout.
        #[arg(long)]
        out: Option<PathBuf>,
        /// Emit stage-2 compiled WASM artifact to this path.
        #[arg(long)]
        emit_wasm: Option<PathBuf>,
        /// Frontend engine. `rust` uses the Rust CoreForm frontend; `selfhost` runs the
        /// self-hosted CoreForm parser+canonicalizer inside the kernel.
        #[arg(long, value_enum)]
        engine: Option<FmtEngine>,
        /// Require `core/obligation::stage1-validation` to pass.
        #[arg(long)]
        stage1_gate: bool,
        /// Require stage-2 translation validation to pass when supported.
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

    /// Emit a selfhost cutover dashboard artifact and markdown status mirror.
    SelfhostDashboard {
        /// Output markdown path (defaults to docs/status/SELFHOST_CUTOVER.md).
        #[arg(long)]
        markdown: Option<PathBuf>,
        /// Store dir for content-addressed dashboard artifact
        /// (defaults to .genesis/store).
        #[arg(long)]
        store: Option<PathBuf>,
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

        /// Output effect log path (.gclog). Defaults to ./.genesis/logs/<op>-<pid>-<seq>.gclog
        #[arg(long)]
        log: Option<PathBuf>,

        #[command(subcommand)]
        cmd: SyncCmd,
    },

    /// Local artifact-store garbage collection workflows (policy-gated).
    Gc {
        /// Capability policy TOML (deny-by-default allowlist).
        #[arg(long)]
        caps: PathBuf,

        /// Output effect log path (.gclog). Defaults to ./.genesis/logs/<op>-<pid>-<seq>.gclog
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

        /// Output effect log path (.gclog). Defaults to ./.genesis/logs/<op>-<pid>-<seq>.gclog
        #[arg(long)]
        log: Option<PathBuf>,

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
}

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
const DASHBOARD_MARKDOWN_DEFAULT_REL: &str = "docs/status/SELFHOST_CUTOVER.md";
const DASHBOARD_STORE_DEFAULT_REL: &str = ".genesis/store";

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
        Cmd::Explain { engine, .. } => enforce_selfhost_engine(cli, "explain", *engine),
        Cmd::Optimize { engine, .. } => enforce_selfhost_engine(cli, "optimize", *engine),
        Cmd::Run { engine, .. } => enforce_selfhost_engine(cli, "run", *engine),
        Cmd::Replay { engine, .. } => enforce_selfhost_engine(cli, "replay", *engine),
        Cmd::Test { .. } => Ok(()),
        Cmd::Pack { .. } => Ok(()),
        Cmd::Typecheck { .. } => Ok(()),
        Cmd::ApplyPatch { .. } => Ok(()),
        Cmd::SelfhostDashboard { .. } => Ok(()),
        Cmd::Store { .. } => Ok(()),
        Cmd::Refs { .. } => Ok(()),
        Cmd::Pkg { .. } => Ok(()),
        Cmd::Policy { .. } => Ok(()),
        Cmd::Sync { .. } => Ok(()),
        Cmd::Gc { .. } => Ok(()),
        Cmd::Vcs {
            cmd: VcsCmd::Hash { engine, .. },
            ..
        } => enforce_selfhost_engine(cli, "vcs hash", *engine),
        Cmd::Vcs { .. } => Ok(()),
        other => {
            let cmd = match other {
                Cmd::Test { .. } | Cmd::Pack { .. } => unreachable!(),
                Cmd::SelfhostArtifact { .. } => "selfhost-artifact",
                Cmd::SelfhostDashboard { .. } => unreachable!(),
                Cmd::Fmt { .. }
                | Cmd::Eval { .. }
                | Cmd::Explain { .. }
                | Cmd::Optimize { .. }
                | Cmd::Run { .. }
                | Cmd::Replay { .. }
                | Cmd::Typecheck { .. }
                | Cmd::ApplyPatch { .. }
                | Cmd::Store { .. }
                | Cmd::Refs { .. }
                | Cmd::Pkg { .. }
                | Cmd::Policy { .. }
                | Cmd::Sync { .. }
                | Cmd::Gc { .. }
                | Cmd::Vcs { .. } => unreachable!(),
            };
            Err(cli_err(
                EX_VERIFY,
                "selfhost-only/unsupported-cmd",
                format!(
                    "selfhost-only mode currently supports only `fmt`, `eval`, `explain`, `optimize`, `run`, `replay`, `test`, `pack`, `typecheck`, `apply-patch`, `selfhost-dashboard`, `store`, `refs`, `pkg`, `policy`, `sync`, `gc`, and `vcs/*`; `{cmd}` is not yet selfhost-routed"
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

fn extract_meta_static(forms: &[Term]) -> Option<Term> {
    for f in forms {
        let Some(items) = f.as_proper_list() else {
            continue;
        };
        if items.len() != 3 {
            continue;
        }
        if !matches!(items[0], Term::Symbol(s) if s == "def") {
            continue;
        }
        if !matches!(items[1], Term::Symbol(s) if s == "::meta") {
            continue;
        }
        let Some(q) = items[2].as_proper_list() else {
            continue;
        };
        if q.len() == 2
            && matches!(q[0], Term::Symbol(s) if s == "quote")
            && let Term::Map(m) = q[1]
        {
            return Some(Term::Map(m.clone()));
        }
    }
    None
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

            let f = env
                .get("core/cli::fmt-module")
                .or_else(|| env.get("selfhost/tool::fmt-module"))
                .ok_or_else(|| {
                    cli_err(
                        EX_INTERNAL,
                        "selfhost/missing",
                        "missing binding core/cli::fmt-module (or fallback selfhost/tool::fmt-module)",
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
    if let Some(canon_src_fn) = env.get("core/cli::canonicalize-module-src") {
        let canon = canon_src_fn
            .apply(ctx, Value::Data(gc_coreform::Term::Str(src.to_string())))
            .map_err(|e| {
                cli_err(
                    EX_EVAL,
                    "eval/error",
                    format!("core/cli canonicalize-module-src failed: {e}"),
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
                    "core/cli canonicalize-module-src returned non-vector: {}",
                    canon.debug_repr()
                ),
            ));
        };
        return Ok(forms.clone());
    }

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

fn selfhost_parse_term(
    ctx: &mut EvalCtx,
    env: &gc_kernel::Env,
    src: &str,
    arg_name: &str,
) -> Result<Term, CliError> {
    let parse_fn = env.get("selfhost/parse::parse-term").ok_or_else(|| {
        cli_err(
            EX_INTERNAL,
            "selfhost/missing",
            "missing binding selfhost/parse::parse-term",
        )
    })?;
    let parsed = parse_fn
        .apply(ctx, Value::Data(Term::Str(src.to_string())))
        .map_err(|e| {
            cli_err(
                EX_EVAL,
                "eval/error",
                format!("selfhost parse-term failed for {arg_name}: {e}"),
            )
        })?;

    if let Some((code, message, payload)) = extract_protocol_error(ctx, &parsed) {
        return Err(CliError {
            exit_code: EX_PARSE,
            json: JsonError {
                code: "selfhost/error",
                message: format!("{arg_name}: {code}: {message}"),
                context: payload.map(serde_json::Value::String),
            },
        });
    }

    let Some(term) = parsed.as_data() else {
        return Err(cli_err(
            EX_INTERNAL,
            "selfhost/bad-return",
            format!(
                "selfhost parse-term returned non-data for {arg_name}: {}",
                parsed.debug_repr()
            ),
        ));
    };
    Ok(term.clone())
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

fn cmd_explain(
    cli: &Cli,
    file: &PathBuf,
    engine: Option<FmtEngine>,
    contract_src: &str,
    msg_src: &str,
) -> Result<CmdOut, CliError> {
    let engine = resolved_engine(cli, "explain", engine)?;
    let src = std::fs::read_to_string(file)
        .with_context(|| format!("read {}", file.display()))
        .map_err(|e| cli_err(EX_IO, "io/read", format!("{e}")))?;
    let (mut ctx, mut env, forms, contract_term, msg_term) = match engine {
        FmtEngine::Rust => {
            let forms = parse_module(&src)
                .map_err(|e| cli_err(EX_PARSE, "parse/coreform", e.to_string()))?;
            let forms = canonicalize_module(forms)
                .map_err(|e| cli_err(EX_PARSE, "canon/coreform", e.to_string()))?;
            let contract_term = parse_term(contract_src)
                .map_err(|e| cli_err(EX_PARSE, "parse/term", format!("--contract: {e}")))?;
            let msg_term = parse_term(msg_src)
                .map_err(|e| cli_err(EX_PARSE, "parse/term", format!("--msg: {e}")))?;
            let mut ctx = mk_ctx(cli);
            let prelude = build_prelude(&mut ctx);
            (ctx, prelude.env, forms, contract_term, msg_term)
        }
        FmtEngine::Selfhost => {
            let mut parse_ctx = EvalCtx::with_step_limit(None);
            parse_ctx.set_mem_limits(resolved_mem_limits(cli));
            let prelude = build_prelude(&mut parse_ctx);
            let mut parse_env = prelude.env;
            load_selfhost_toolchain(cli, &mut parse_ctx, &mut parse_env)?;

            parse_ctx.steps = 0;
            parse_ctx.step_limit = None;
            let forms = selfhost_parse_canonicalize_module(&mut parse_ctx, &parse_env, &src)?;
            let contract_term =
                selfhost_parse_term(&mut parse_ctx, &parse_env, contract_src, "--contract")?;
            let msg_term = selfhost_parse_term(&mut parse_ctx, &parse_env, msg_src, "--msg")?;

            let mut eval_ctx = mk_ctx(cli);
            let prelude = build_prelude(&mut eval_ctx);
            (eval_ctx, prelude.env, forms, contract_term, msg_term)
        }
    };

    eval_module(&mut ctx, &mut env, &forms)
        .map_err(|e| cli_err(EX_EVAL, "eval/error", format!("{e}")))?;

    let contract = eval_term(&mut ctx, &env, &contract_term)
        .map_err(|e| cli_err(EX_EVAL, "eval/error", format!("--contract: {e}")))?;
    let msg_val = Value::Data(msg_term);

    let explain = env.get("core/contract::explain").ok_or_else(|| {
        cli_err(
            EX_INTERNAL,
            "prelude/missing",
            "missing prelude binding core/contract::explain",
        )
    })?;
    let r = explain
        .apply(&mut ctx, contract)
        .map_err(|e| cli_err(EX_EVAL, "eval/error", format!("apply contract: {e}")))?
        .apply(&mut ctx, msg_val)
        .map_err(|e| cli_err(EX_EVAL, "eval/error", format!("explain failed: {e}")))?;

    let (value, value_format) = render_value_for_cli(&ctx, &r);
    let env = JsonEnvelope {
        ok: true,
        kind: "genesis/explain-v0.2",
        data: Some(serde_json::json!({
            "file": file.display().to_string(),
            "engine": match engine {
                FmtEngine::Rust => "rust",
                FmtEngine::Selfhost => "selfhost",
            },
            "contract": contract_src,
            "msg": msg_src,
            "trace": value,
            "trace_format": value_format,
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

#[derive(Clone, Copy)]
struct SelfhostCutoverRow {
    cmd: &'static str,
    selfhost_routed: bool,
}

const SELFHOST_CUTOVER_ROWS: &[SelfhostCutoverRow] = &[
    SelfhostCutoverRow {
        cmd: "fmt",
        selfhost_routed: true,
    },
    SelfhostCutoverRow {
        cmd: "eval",
        selfhost_routed: true,
    },
    SelfhostCutoverRow {
        cmd: "explain",
        selfhost_routed: true,
    },
    SelfhostCutoverRow {
        cmd: "run",
        selfhost_routed: true,
    },
    SelfhostCutoverRow {
        cmd: "replay",
        selfhost_routed: true,
    },
    SelfhostCutoverRow {
        cmd: "test",
        selfhost_routed: true,
    },
    SelfhostCutoverRow {
        cmd: "pack",
        selfhost_routed: true,
    },
    SelfhostCutoverRow {
        cmd: "selfhost-artifact",
        selfhost_routed: false,
    },
    SelfhostCutoverRow {
        cmd: "selfhost-dashboard",
        selfhost_routed: true,
    },
    SelfhostCutoverRow {
        cmd: "typecheck",
        selfhost_routed: true,
    },
    SelfhostCutoverRow {
        cmd: "optimize",
        selfhost_routed: true,
    },
    SelfhostCutoverRow {
        cmd: "apply-patch",
        selfhost_routed: true,
    },
    SelfhostCutoverRow {
        cmd: "store/*",
        selfhost_routed: false,
    },
    SelfhostCutoverRow {
        cmd: "refs/*",
        selfhost_routed: false,
    },
    SelfhostCutoverRow {
        cmd: "pkg/*",
        selfhost_routed: false,
    },
    SelfhostCutoverRow {
        cmd: "policy/*",
        selfhost_routed: false,
    },
    SelfhostCutoverRow {
        cmd: "sync/*",
        selfhost_routed: false,
    },
    SelfhostCutoverRow {
        cmd: "gc/*",
        selfhost_routed: false,
    },
    SelfhostCutoverRow {
        cmd: "vcs/*",
        selfhost_routed: false,
    },
];

fn percent_basis_points(part: usize, total: usize) -> u64 {
    if total == 0 {
        return 0;
    }
    ((part as u128 * 10_000u128) / total as u128) as u64
}

fn percent_string_from_bps(bps: u64) -> String {
    format!("{}.{:02}%", bps / 100, bps % 100)
}

fn write_content_addressed_artifact(
    store_dir: &Path,
    bytes: &[u8],
) -> Result<(String, PathBuf), CliError> {
    std::fs::create_dir_all(store_dir)
        .with_context(|| format!("create {}", store_dir.display()))
        .map_err(|e| cli_err(EX_IO, "io/mkdir", format!("{e}")))?;

    let hex = blake3::hash(bytes).to_hex().to_string();
    let path = store_dir.join(&hex);
    if !path.is_file() {
        std::fs::write(&path, bytes)
            .with_context(|| format!("write {}", path.display()))
            .map_err(|e| cli_err(EX_IO, "io/write", format!("{e}")))?;
    }
    Ok((hex, path))
}

fn cmd_selfhost_dashboard(
    cli: &Cli,
    markdown: Option<&Path>,
    store: Option<&Path>,
) -> Result<CmdOut, CliError> {
    let artifact = resolved_selfhost_artifact_for_frontend(cli);
    let artifact_path = artifact.as_ref().map(|p| p.display().to_string());
    let artifact_exists = artifact.as_ref().is_some_and(|p| p.is_file());
    let strict = selfhost_only_enabled(cli);

    let total_commands = SELFHOST_CUTOVER_ROWS.len();
    let routed_count = SELFHOST_CUTOVER_ROWS
        .iter()
        .filter(|r| r.selfhost_routed)
        .count();
    let default_selfhost_count = SELFHOST_CUTOVER_ROWS
        .iter()
        .filter(|r| r.selfhost_routed)
        .count();
    let routed_bps = percent_basis_points(routed_count, total_commands);
    let default_bps = percent_basis_points(default_selfhost_count, total_commands);

    let rows_term: Vec<Term> = SELFHOST_CUTOVER_ROWS
        .iter()
        .map(|row| {
            let default_selfhost = row.selfhost_routed;
            Term::Map(
                [
                    (
                        TermOrdKey(Term::symbol(":cmd")),
                        Term::Str(row.cmd.to_string()),
                    ),
                    (
                        TermOrdKey(Term::symbol(":selfhost-routed")),
                        Term::Bool(row.selfhost_routed),
                    ),
                    (
                        TermOrdKey(Term::symbol(":default-selfhost")),
                        Term::Bool(default_selfhost),
                    ),
                ]
                .into_iter()
                .collect(),
            )
        })
        .collect();

    let dashboard_term = Term::Map(
        [
            (
                TermOrdKey(Term::symbol(":kind")),
                Term::Str("genesis/selfhost-cutover-dashboard-v0.2".to_string()),
            ),
            (TermOrdKey(Term::symbol(":v")), Term::Int(1.into())),
            (TermOrdKey(Term::symbol(":strict")), Term::Bool(strict)),
            (
                TermOrdKey(Term::symbol(":artifact-configured")),
                Term::Bool(artifact.is_some()),
            ),
            (
                TermOrdKey(Term::symbol(":artifact-exists")),
                Term::Bool(artifact_exists),
            ),
            (
                TermOrdKey(Term::symbol(":artifact-path")),
                artifact
                    .as_ref()
                    .map(|p| Term::Str(p.display().to_string()))
                    .unwrap_or(Term::Nil),
            ),
            (
                TermOrdKey(Term::symbol(":summary")),
                Term::Map(
                    [
                        (
                            TermOrdKey(Term::symbol(":total-commands")),
                            Term::Int((total_commands as i64).into()),
                        ),
                        (
                            TermOrdKey(Term::symbol(":selfhost-routed-commands")),
                            Term::Int((routed_count as i64).into()),
                        ),
                        (
                            TermOrdKey(Term::symbol(":selfhost-default-commands")),
                            Term::Int((default_selfhost_count as i64).into()),
                        ),
                        (
                            TermOrdKey(Term::symbol(":selfhost-routed-bps")),
                            Term::Int((routed_bps as i64).into()),
                        ),
                        (
                            TermOrdKey(Term::symbol(":selfhost-default-bps")),
                            Term::Int((default_bps as i64).into()),
                        ),
                    ]
                    .into_iter()
                    .collect(),
                ),
            ),
            (
                TermOrdKey(Term::symbol(":commands")),
                Term::Vector(rows_term),
            ),
        ]
        .into_iter()
        .collect(),
    );
    let artifact_bytes = print_term(&dashboard_term);

    let store_dir = store
        .map(Path::to_path_buf)
        .unwrap_or_else(|| PathBuf::from(DASHBOARD_STORE_DEFAULT_REL));
    let (artifact_hash, artifact_path_fs) =
        write_content_addressed_artifact(&store_dir, artifact_bytes.as_bytes())?;

    let markdown_path = markdown
        .map(Path::to_path_buf)
        .unwrap_or_else(|| PathBuf::from(DASHBOARD_MARKDOWN_DEFAULT_REL));
    let markdown_body = {
        let mut lines = vec![
            "# Selfhost Cutover Dashboard (v0.2)".to_string(),
            "".to_string(),
            format!("- Artifact hash: `{artifact_hash}`"),
            format!("- Store artifact: `{}`", artifact_path_fs.display()),
            format!(
                "- Selfhost toolchain artifact configured: `{}`",
                artifact_path.as_deref().unwrap_or("none")
            ),
            format!("- Selfhost toolchain artifact exists: `{artifact_exists}`"),
            "".to_string(),
            "## Summary".to_string(),
            "".to_string(),
            "| Metric | Value |".to_string(),
            "| --- | --- |".to_string(),
            format!("| Total command groups | {} |", total_commands),
            format!("| Selfhost-routed command groups | {} |", routed_count),
            format!(
                "| Selfhost-routed coverage | {} |",
                percent_string_from_bps(routed_bps)
            ),
            format!(
                "| Default selfhost coverage | {} |",
                percent_string_from_bps(default_bps)
            ),
            "".to_string(),
            "## Command Coverage".to_string(),
            "".to_string(),
            "| Command | Selfhost Routed | Default Selfhost |".to_string(),
            "| --- | --- | --- |".to_string(),
        ];
        for row in SELFHOST_CUTOVER_ROWS {
            let default_selfhost = row.selfhost_routed;
            lines.push(format!(
                "| `{}` | {} | {} |",
                row.cmd, row.selfhost_routed, default_selfhost
            ));
        }
        lines.push(String::new());
        lines.join("\n")
    };
    if let Some(parent) = markdown_path.parent()
        && !parent.as_os_str().is_empty()
    {
        std::fs::create_dir_all(parent)
            .with_context(|| format!("create {}", parent.display()))
            .map_err(|e| cli_err(EX_IO, "io/mkdir", format!("{e}")))?;
    }
    std::fs::write(&markdown_path, markdown_body.as_bytes())
        .with_context(|| format!("write {}", markdown_path.display()))
        .map_err(|e| cli_err(EX_IO, "io/write", format!("{e}")))?;

    let env = JsonEnvelope {
        ok: true,
        kind: "genesis/selfhost-dashboard-v0.2",
        data: Some(serde_json::json!({
            "artifact_hash": artifact_hash,
            "store_artifact": artifact_path_fs.display().to_string(),
            "store_dir": store_dir.display().to_string(),
            "markdown": markdown_path.display().to_string(),
            "artifact_configured": artifact.is_some(),
            "artifact_exists": artifact_exists,
            "artifact_path": artifact_path,
            "summary": {
                "total_commands": total_commands,
                "selfhost_routed_commands": routed_count,
                "selfhost_default_commands": default_selfhost_count,
                "selfhost_routed_percent": percent_string_from_bps(routed_bps),
                "selfhost_default_percent": percent_string_from_bps(default_bps),
            }
        })),
        error: None,
    };
    Ok(CmdOut {
        exit_code: EX_OK,
        stdout: if cli.json {
            String::new()
        } else {
            format!(
                "{}\n{}\n",
                artifact_path_fs.display(),
                markdown_path.display()
            )
        },
        json: serde_json::to_value(env).expect("json"),
    })
}

fn cmd_typecheck(cli: &Cli, pkg: &Path) -> Result<CmdOut, CliError> {
    let (manifest, pkg_dir) = PackageManifest::load(pkg)
        .map_err(|e| cli_err(EX_PARSE, "manifest/parse", format!("{e}")))?;
    let frontend = resolved_coreform_frontend(cli)?;

    let mut mods = Vec::new();
    if matches!(frontend, gc_obligations::CoreformFrontend::Selfhost(_)) {
        // Toolchain bootstrap is trusted; do not charge it against parse/canonicalize limits.
        let mut ctx = EvalCtx::with_step_limit(None);
        ctx.set_mem_limits(resolved_mem_limits(cli));
        let prelude = build_prelude(&mut ctx);
        let mut env = prelude.env;
        load_selfhost_toolchain(cli, &mut ctx, &mut env)?;

        // Charge user-configured limits to module parse/canonicalize work.
        ctx.steps = 0;
        ctx.step_limit = resolved_step_limit(cli).resolve();
        for m in &manifest.modules {
            let abs = pkg_dir.join(&m.path);
            let src = std::fs::read_to_string(&abs)
                .with_context(|| format!("read {}", abs.display()))
                .map_err(|e| cli_err(EX_IO, "io/read", format!("{e}")))?;
            let forms = selfhost_parse_canonicalize_module(&mut ctx, &env, &src)?;
            let meta = extract_meta_static(&forms);
            mods.push(gc_types::ModuleForTypecheck {
                path: m.path.clone(),
                forms,
                meta,
            });
        }
    } else {
        for m in &manifest.modules {
            let abs = pkg_dir.join(&m.path);
            let src = std::fs::read_to_string(&abs)
                .with_context(|| format!("read {}", abs.display()))
                .map_err(|e| cli_err(EX_IO, "io/read", format!("{e}")))?;
            let forms = parse_module(&src)
                .map_err(|e| cli_err(EX_PARSE, "parse/coreform", e.to_string()))?;
            let forms = canonicalize_module(forms)
                .map_err(|e| cli_err(EX_PARSE, "canon/coreform", e.to_string()))?;
            let meta = extract_meta_static(&forms);
            mods.push(gc_types::ModuleForTypecheck {
                path: m.path.clone(),
                forms,
                meta,
            });
        }
    }

    let report = gc_types::typecheck_package(&mods);
    let report_term = report.to_term();
    let report_s = gc_coreform::print_term(&report_term);

    let exit_code = if report.ok { EX_OK } else { EX_OBLIGATIONS };
    let env = JsonEnvelope {
        ok: report.ok,
        kind: "genesis/typecheck-v0.2",
        data: Some(serde_json::json!({
            "pkg": pkg.display().to_string(),
            "report_coreform": report_s,
        })),
        error: None,
    };
    Ok(CmdOut {
        exit_code,
        stdout: if cli.json {
            String::new()
        } else {
            format!("{report_s}\n")
        },
        json: serde_json::to_value(env).expect("json"),
    })
}

fn cmd_optimize(
    cli: &Cli,
    file: &PathBuf,
    out: Option<&PathBuf>,
    emit_wasm: Option<&PathBuf>,
    engine: Option<FmtEngine>,
    stage1_gate: bool,
    stage2_gate: bool,
) -> Result<CmdOut, CliError> {
    let engine = resolved_engine(cli, "optimize", engine)?;
    let src = std::fs::read_to_string(file)
        .with_context(|| format!("read {}", file.display()))
        .map_err(|e| cli_err(EX_IO, "io/read", format!("{e}")))?;

    let forms = match engine {
        FmtEngine::Rust => {
            let forms = parse_module(&src)
                .map_err(|e| cli_err(EX_PARSE, "parse/coreform", e.to_string()))?;
            canonicalize_module(forms)
                .map_err(|e| cli_err(EX_PARSE, "canon/coreform", e.to_string()))?
        }
        FmtEngine::Selfhost => {
            let mut ctx = EvalCtx::with_step_limit(None);
            ctx.set_mem_limits(resolved_mem_limits(cli));
            let prelude = build_prelude(&mut ctx);
            let mut env = prelude.env;
            load_selfhost_toolchain(cli, &mut ctx, &mut env)?;
            ctx.steps = 0;
            ctx.step_limit = None;
            selfhost_parse_canonicalize_module(&mut ctx, &env, &src)?
        }
    };
    let orig_h = hash_module(&forms);

    let stage1 = gc_opt::stage1_pipeline(&forms)
        .map_err(|e| cli_err(EX_INTERNAL, "stage1/error", format!("{e}")))?;
    if stage1_gate && !stage1.gate_report.ok {
        return Err(CliError {
            exit_code: EX_OBLIGATIONS,
            json: JsonError {
                code: "obligation/stage1-validation",
                message: "core/obligation::stage1-validation failed".to_string(),
                context: Some(stage1_pipeline_json(&stage1)),
            },
        });
    }
    let opt = stage1.transformed_forms.clone();
    let opt_report = stage1.optimize_report.clone();
    let opt_h = hash_module(&opt);
    let stage2 = if stage2_gate || emit_wasm.is_some() {
        Some(gc_opt::stage2_validation_report(&opt))
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

    if let Some(p) = emit_wasm {
        let art = gc_opt::stage2_compile_module(&opt).map_err(|e| match e {
            gc_opt::Stage2CompileError::Unsupported(msg) => {
                cli_err(EX_OBLIGATIONS, "stage2/unsupported", msg)
            }
            gc_opt::Stage2CompileError::Internal(msg) => cli_err(EX_INTERNAL, "stage2/error", msg),
        })?;
        std::fs::write(p, &art.wasm_bytes)
            .with_context(|| format!("write {}", p.display()))
            .map_err(|e| cli_err(EX_IO, "io/write", format!("{e}")))?;
    }

    let out_s = print_module(&opt);
    let changed = orig_h != opt_h;

    if let Some(p) = out {
        std::fs::write(p, out_s.as_bytes())
            .with_context(|| format!("write {}", p.display()))
            .map_err(|e| cli_err(EX_IO, "io/write", format!("{e}")))?;
    }

    let stdout = if cli.json || out.is_some() {
        String::new()
    } else {
        out_s.clone()
    };

    let env = JsonEnvelope {
        ok: true,
        kind: "genesis/optimize-v0.2",
        data: Some(serde_json::json!({
            "file": file.display().to_string(),
            "out": out.map(|p| p.display().to_string()),
            "wasm_out": emit_wasm.map(|p| p.display().to_string()),
            "engine": match engine {
                FmtEngine::Rust => "rust",
                FmtEngine::Selfhost => "selfhost",
            },
            "stage1": stage1_pipeline_json(&stage1),
            "stage2": stage2.as_ref().map(stage2_report_json),
            "changed": changed,
            "original_hash": hex32(orig_h),
            "optimized_hash": hex32(opt_h),
            "egg_runs": opt_report.stats.egg_runs,
            "egg_iterations": opt_report.stats.iterations,
            "egg_eclasses": opt_report.stats.eclasses,
            "egg_enodes": opt_report.stats.enodes,
            "egg_rewrites_applied": opt_report.stats.rewrites_applied,
            "optimized_coreform": out_s,
        })),
        error: None,
    };
    Ok(CmdOut {
        exit_code: EX_OK,
        stdout,
        json: serde_json::to_value(env).expect("json"),
    })
}

fn cmd_apply_patch(
    cli: &Cli,
    patch: &Path,
    pkg: &Path,
    caps: Option<&Path>,
) -> Result<CmdOut, CliError> {
    let frontend = resolved_coreform_frontend(cli)?;

    let r = gc_patches::apply_patch_with_step_limit_and_frontend(
        patch,
        pkg,
        caps,
        resolved_step_limit(cli),
        resolved_mem_limits(cli),
        frontend,
    )
    .map_err(|e| match e {
        gc_patches::PatchError::Parse(_) | gc_patches::PatchError::Validate(_) => {
            cli_err(EX_PARSE, "patch/invalid", format!("{e}"))
        }
        gc_patches::PatchError::Io(_) => cli_err(EX_IO, "io/error", format!("{e}")),
        gc_patches::PatchError::Obligations(inner) => obligation_err(inner),
    })?;

    let exit_code = if r.ok { EX_OK } else { EX_OBLIGATIONS };
    let env = JsonEnvelope {
        ok: r.ok,
        kind: "genesis/apply-patch-v0.2",
        data: Some(serde_json::json!({
            "patch": patch.display().to_string(),
            "pkg": pkg.display().to_string(),
            "caps": caps.map(|p| p.display().to_string()),
            "patch_artifact": r.patch_artifact,
            "report_artifact": r.report_artifact,
            "acceptance_artifact": r.acceptance_artifact,
            "package_artifact": r.package_artifact,
        })),
        error: None,
    };
    Ok(CmdOut {
        exit_code,
        stdout: if cli.json {
            String::new()
        } else {
            format!("{}\n", r.report_artifact)
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

fn parse_set_ref_spec(spec: &str) -> Result<SetRefSpec, CliError> {
    let (base, expected_old_raw) = match spec.split_once('@') {
        None => (spec, None),
        Some((lhs, rhs)) => (lhs, Some(rhs)),
    };

    let mut it = base.rsplitn(3, ':');
    let Some(policy) = it.next() else {
        return Err(cli_err(
            EX_PARSE,
            "sync/set-ref",
            "set-ref must be <refname>:<commit-hash>:<policy-hash>[@<expected-old-hash|nil>]"
                .to_string(),
        ));
    };
    let Some(hash) = it.next() else {
        return Err(cli_err(
            EX_PARSE,
            "sync/set-ref",
            "set-ref must be <refname>:<commit-hash>:<policy-hash>[@<expected-old-hash|nil>]"
                .to_string(),
        ));
    };
    let Some(name) = it.next() else {
        return Err(cli_err(
            EX_PARSE,
            "sync/set-ref",
            "set-ref must be <refname>:<commit-hash>:<policy-hash>[@<expected-old-hash|nil>]"
                .to_string(),
        ));
    };
    let name = name.trim();
    let hash = hash.trim();
    let policy = policy.trim();

    if name.is_empty() || hash.is_empty() || policy.is_empty() {
        return Err(cli_err(
            EX_PARSE,
            "sync/set-ref",
            "set-ref fields must be non-empty".to_string(),
        ));
    }
    if !is_hex64(hash) {
        return Err(cli_err(
            EX_PARSE,
            "sync/set-ref",
            "set-ref commit hash must be 64-hex".to_string(),
        ));
    }
    if !is_hex64(policy) {
        return Err(cli_err(
            EX_PARSE,
            "sync/set-ref",
            "set-ref policy hash must be 64-hex".to_string(),
        ));
    }
    let expected_old = match expected_old_raw.map(str::trim) {
        None => None,
        Some("") => {
            return Err(cli_err(
                EX_PARSE,
                "sync/set-ref",
                "set-ref expected-old must be non-empty when provided".to_string(),
            ));
        }
        Some(s) => {
            if s != "nil" && !is_hex64(s) {
                return Err(cli_err(
                    EX_PARSE,
                    "sync/set-ref",
                    "set-ref expected-old must be 64-hex or `nil`".to_string(),
                ));
            }
            Some(if s == "nil" {
                "nil".to_string()
            } else {
                s.to_ascii_lowercase()
            })
        }
    };

    Ok(SetRefSpec {
        name: name.to_string(),
        hash: hash.to_ascii_lowercase(),
        policy: policy.to_ascii_lowercase(),
        expected_old,
    })
}

fn parse_sync_set_refs(specs: &[String]) -> Result<Vec<SetRefSpec>, CliError> {
    let mut seen: std::collections::BTreeSet<String> = std::collections::BTreeSet::new();
    let mut out = Vec::with_capacity(specs.len());
    for spec in specs {
        let parsed = parse_set_ref_spec(spec)?;
        if !seen.insert(parsed.name.clone()) {
            return Err(cli_err(
                EX_PARSE,
                "sync/set-ref",
                format!("duplicate set-ref target: {}", parsed.name),
            ));
        }
        out.push(parsed);
    }
    Ok(out)
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

    let mut seen: std::collections::BTreeSet<String> = std::collections::BTreeSet::new();
    let mut out = Vec::new();
    for s in specs {
        let (name, rhs) = s.split_once('=').ok_or_else(|| {
            cli_err(
                EX_PARSE,
                "pkg/import",
                "set-ref must be <refname>=<commit-hash|nil>[@<expected-old-hash|nil>]".to_string(),
            )
        })?;
        let name = name.trim();
        let rhs = rhs.trim();
        if name.is_empty() || rhs.is_empty() {
            return Err(cli_err(
                EX_PARSE,
                "pkg/import",
                "set-ref fields must be non-empty".to_string(),
            ));
        }
        if !seen.insert(name.to_string()) {
            return Err(cli_err(
                EX_PARSE,
                "pkg/import",
                format!("duplicate set-ref target: {name}"),
            ));
        }
        let (hash, expected_old) = match rhs.split_once('@') {
            None => (rhs, None),
            Some((h, eo)) => {
                let eo = eo.trim();
                if eo.is_empty() {
                    return Err(cli_err(
                        EX_PARSE,
                        "pkg/import",
                        "set-ref expected-old must be non-empty when @ is used".to_string(),
                    ));
                }
                (h.trim(), Some(eo))
            }
        };
        if hash != "nil" && !is_hex64(hash) {
            return Err(cli_err(
                EX_PARSE,
                "pkg/import",
                "set-ref hash must be 64-hex or `nil`".to_string(),
            ));
        }
        let expected_old = match expected_old {
            None => None,
            Some(eo) => {
                if eo != "nil" && !is_hex64(eo) {
                    return Err(cli_err(
                        EX_PARSE,
                        "pkg/import",
                        "set-ref expected-old must be 64-hex or `nil`".to_string(),
                    ));
                }
                Some(eo.to_string())
            }
        };
        out.push(SetRefSpec {
            name: name.to_string(),
            hash: hash.to_string(),
            policy: pol.to_string(),
            expected_old,
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

fn mk_pkg_publish_program(
    remote: &str,
    refname: &str,
    policy_h: &str,
    expected_old: Option<&str>,
    depth: u64,
    commit: Option<&str>,
) -> Vec<gc_coreform::Term> {
    let op = gc_coreform::Term::list(vec![
        gc_coreform::Term::symbol("quote"),
        gc_coreform::Term::symbol("core/pkg::publish"),
    ]);
    let mut m = std::collections::BTreeMap::new();
    m.insert(
        gc_coreform::TermOrdKey(gc_coreform::Term::symbol(":remote")),
        gc_coreform::Term::Str(remote.to_string()),
    );
    m.insert(
        gc_coreform::TermOrdKey(gc_coreform::Term::symbol(":ref")),
        gc_coreform::Term::Str(refname.to_string()),
    );
    m.insert(
        gc_coreform::TermOrdKey(gc_coreform::Term::symbol(":policy")),
        gc_coreform::Term::Str(policy_h.to_string()),
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
    if depth > 0 {
        m.insert(
            gc_coreform::TermOrdKey(gc_coreform::Term::symbol(":depth")),
            gc_coreform::Term::Int((depth as i64).into()),
        );
    }
    if let Some(h) = commit {
        m.insert(
            gc_coreform::TermOrdKey(gc_coreform::Term::symbol(":commit")),
            gc_coreform::Term::Str(h.to_string()),
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

fn mk_sync_pull_program(
    remote: &str,
    refs: &[String],
    roots: &[String],
    depth: u64,
    force: bool,
) -> Vec<gc_coreform::Term> {
    let op = gc_coreform::Term::list(vec![
        gc_coreform::Term::symbol("quote"),
        gc_coreform::Term::symbol("core/sync::pull"),
    ]);
    let mut m = std::collections::BTreeMap::new();
    m.insert(
        gc_coreform::TermOrdKey(gc_coreform::Term::symbol(":remote")),
        gc_coreform::Term::Str(remote.to_string()),
    );
    m.insert(
        gc_coreform::TermOrdKey(gc_coreform::Term::symbol(":refs")),
        gc_coreform::Term::Vector(refs.iter().cloned().map(gc_coreform::Term::Str).collect()),
    );
    m.insert(
        gc_coreform::TermOrdKey(gc_coreform::Term::symbol(":roots")),
        gc_coreform::Term::Vector(roots.iter().cloned().map(gc_coreform::Term::Str).collect()),
    );
    m.insert(
        gc_coreform::TermOrdKey(gc_coreform::Term::symbol(":depth")),
        gc_coreform::Term::Int((depth as i64).into()),
    );
    m.insert(
        gc_coreform::TermOrdKey(gc_coreform::Term::symbol(":force")),
        gc_coreform::Term::Bool(force),
    );
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

fn mk_sync_push_program(
    remote: &str,
    roots: &[String],
    depth: u64,
    set_refs: &[SetRefSpec],
) -> Vec<gc_coreform::Term> {
    let op = gc_coreform::Term::list(vec![
        gc_coreform::Term::symbol("quote"),
        gc_coreform::Term::symbol("core/sync::push"),
    ]);
    let mut m = std::collections::BTreeMap::new();
    m.insert(
        gc_coreform::TermOrdKey(gc_coreform::Term::symbol(":remote")),
        gc_coreform::Term::Str(remote.to_string()),
    );
    m.insert(
        gc_coreform::TermOrdKey(gc_coreform::Term::symbol(":roots")),
        gc_coreform::Term::Vector(roots.iter().cloned().map(gc_coreform::Term::Str).collect()),
    );
    m.insert(
        gc_coreform::TermOrdKey(gc_coreform::Term::symbol(":depth")),
        gc_coreform::Term::Int((depth as i64).into()),
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
                gc_coreform::Term::Str(sr.hash.clone()),
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
        m.insert(
            gc_coreform::TermOrdKey(gc_coreform::Term::symbol(":set-refs")),
            gc_coreform::Term::Vector(entries),
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

fn mk_gc_plan_program(
    lock: &Path,
    pins: &Path,
    depth: u64,
    include_lock: bool,
    include_refs: bool,
) -> Vec<gc_coreform::Term> {
    let op = gc_coreform::Term::list(vec![
        gc_coreform::Term::symbol("quote"),
        gc_coreform::Term::symbol("core/gc::plan"),
    ]);
    let mut m = std::collections::BTreeMap::new();
    m.insert(
        gc_coreform::TermOrdKey(gc_coreform::Term::symbol(":lock")),
        gc_coreform::Term::Str(lock.display().to_string()),
    );
    m.insert(
        gc_coreform::TermOrdKey(gc_coreform::Term::symbol(":pins")),
        gc_coreform::Term::Str(pins.display().to_string()),
    );
    m.insert(
        gc_coreform::TermOrdKey(gc_coreform::Term::symbol(":depth")),
        gc_coreform::Term::Int((depth as i64).into()),
    );
    m.insert(
        gc_coreform::TermOrdKey(gc_coreform::Term::symbol(":include-lock")),
        gc_coreform::Term::Bool(include_lock),
    );
    m.insert(
        gc_coreform::TermOrdKey(gc_coreform::Term::symbol(":include-refs")),
        gc_coreform::Term::Bool(include_refs),
    );
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

fn mk_gc_run_program(
    lock: &Path,
    pins: &Path,
    depth: u64,
    include_lock: bool,
    include_refs: bool,
    quarantine: bool,
    quarantine_dir: Option<&Path>,
) -> Vec<gc_coreform::Term> {
    let op = gc_coreform::Term::list(vec![
        gc_coreform::Term::symbol("quote"),
        gc_coreform::Term::symbol("core/gc::run"),
    ]);
    let mut m = std::collections::BTreeMap::new();
    m.insert(
        gc_coreform::TermOrdKey(gc_coreform::Term::symbol(":lock")),
        gc_coreform::Term::Str(lock.display().to_string()),
    );
    m.insert(
        gc_coreform::TermOrdKey(gc_coreform::Term::symbol(":pins")),
        gc_coreform::Term::Str(pins.display().to_string()),
    );
    m.insert(
        gc_coreform::TermOrdKey(gc_coreform::Term::symbol(":depth")),
        gc_coreform::Term::Int((depth as i64).into()),
    );
    m.insert(
        gc_coreform::TermOrdKey(gc_coreform::Term::symbol(":include-lock")),
        gc_coreform::Term::Bool(include_lock),
    );
    m.insert(
        gc_coreform::TermOrdKey(gc_coreform::Term::symbol(":include-refs")),
        gc_coreform::Term::Bool(include_refs),
    );
    m.insert(
        gc_coreform::TermOrdKey(gc_coreform::Term::symbol(":quarantine")),
        gc_coreform::Term::Bool(quarantine),
    );
    if let Some(qd) = quarantine_dir {
        m.insert(
            gc_coreform::TermOrdKey(gc_coreform::Term::symbol(":quarantine-dir")),
            gc_coreform::Term::Str(qd.display().to_string()),
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

fn mk_gc_pin_program(target: &str, pins: &Path) -> Vec<gc_coreform::Term> {
    let op = gc_coreform::Term::list(vec![
        gc_coreform::Term::symbol("quote"),
        gc_coreform::Term::symbol("core/gc::pin"),
    ]);
    let payload = gc_coreform::Term::Map(
        [
            (
                gc_coreform::TermOrdKey(gc_coreform::Term::symbol(":target")),
                gc_coreform::Term::Str(target.to_string()),
            ),
            (
                gc_coreform::TermOrdKey(gc_coreform::Term::symbol(":pins")),
                gc_coreform::Term::Str(pins.display().to_string()),
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

fn mk_gc_unpin_program(target: &str, pins: &Path) -> Vec<gc_coreform::Term> {
    let op = gc_coreform::Term::list(vec![
        gc_coreform::Term::symbol("quote"),
        gc_coreform::Term::symbol("core/gc::unpin"),
    ]);
    let payload = gc_coreform::Term::Map(
        [
            (
                gc_coreform::TermOrdKey(gc_coreform::Term::symbol(":target")),
                gc_coreform::Term::Str(target.to_string()),
            ),
            (
                gc_coreform::TermOrdKey(gc_coreform::Term::symbol(":pins")),
                gc_coreform::Term::Str(pins.display().to_string()),
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

fn mk_gc_purge_program(ttl_days: u64, quarantine_dir: Option<&Path>) -> Vec<gc_coreform::Term> {
    let op = gc_coreform::Term::list(vec![
        gc_coreform::Term::symbol("quote"),
        gc_coreform::Term::symbol("core/gc::purge"),
    ]);
    let mut m = std::collections::BTreeMap::new();
    m.insert(
        gc_coreform::TermOrdKey(gc_coreform::Term::symbol(":ttl-days")),
        gc_coreform::Term::Int((ttl_days as i64).into()),
    );
    if let Some(qd) = quarantine_dir {
        m.insert(
            gc_coreform::TermOrdKey(gc_coreform::Term::symbol(":quarantine-dir")),
            gc_coreform::Term::Str(qd.display().to_string()),
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

fn mk_vcs_log_program(root: &str, max: u64) -> Vec<Term> {
    let op = Term::list(vec![Term::symbol("quote"), Term::symbol("core/vcs::log")]);
    let payload = Term::Map(
        [
            (
                TermOrdKey(Term::symbol(":root")),
                Term::Str(root.to_string()),
            ),
            (
                TermOrdKey(Term::symbol(":max")),
                Term::Int((max as i64).into()),
            ),
        ]
        .into_iter()
        .collect(),
    );
    let k = Term::list(vec![
        Term::symbol("fn"),
        Term::list(vec![Term::symbol("r")]),
        Term::list(vec![Term::symbol("core/effect::pure"), Term::symbol("r")]),
    ]);
    let perform = Term::list(vec![Term::symbol("core/effect::perform"), op, payload, k]);
    vec![
        Term::list(vec![Term::symbol("def"), Term::symbol("prog"), perform]),
        Term::symbol("prog"),
    ]
}

fn mk_vcs_blame_program(
    snapshot: &str,
    sym: &str,
    path: Option<&str>,
) -> Result<Vec<Term>, CliError> {
    gc_vcs::validate_hex_hash(snapshot)
        .map_err(|e| cli_err(EX_PARSE, "vcs/blame", format!("invalid --snapshot: {e}")))?;
    if sym.trim().is_empty() {
        return Err(cli_err(EX_PARSE, "vcs/blame", "invalid --sym: empty value"));
    }
    let op = Term::list(vec![Term::symbol("quote"), Term::symbol("core/vcs::blame")]);
    let mut m = std::collections::BTreeMap::new();
    m.insert(
        TermOrdKey(Term::symbol(":snapshot")),
        Term::Str(snapshot.to_string()),
    );
    m.insert(TermOrdKey(Term::symbol(":sym")), Term::Str(sym.to_string()));
    if let Some(path) = path {
        m.insert(
            TermOrdKey(Term::symbol(":path")),
            Term::Str(path.to_string()),
        );
    }
    let payload = Term::Map(m);
    let k = Term::list(vec![
        Term::symbol("fn"),
        Term::list(vec![Term::symbol("r")]),
        Term::list(vec![Term::symbol("core/effect::pure"), Term::symbol("r")]),
    ]);
    let perform = Term::list(vec![Term::symbol("core/effect::perform"), op, payload, k]);
    Ok(vec![
        Term::list(vec![Term::symbol("def"), Term::symbol("prog"), perform]),
        Term::symbol("prog"),
    ])
}

fn mk_vcs_why_program(
    snapshot: &str,
    sym: &str,
    op_sym: Option<&str>,
) -> Result<Vec<Term>, CliError> {
    gc_vcs::validate_hex_hash(snapshot)
        .map_err(|e| cli_err(EX_PARSE, "vcs/why", format!("invalid --snapshot: {e}")))?;
    if sym.trim().is_empty() {
        return Err(cli_err(EX_PARSE, "vcs/why", "invalid --sym: empty value"));
    }

    let op = Term::list(vec![Term::symbol("quote"), Term::symbol("core/vcs::why")]);
    let mut m = std::collections::BTreeMap::new();
    m.insert(
        TermOrdKey(Term::symbol(":snapshot")),
        Term::Str(snapshot.to_string()),
    );
    m.insert(TermOrdKey(Term::symbol(":sym")), Term::Str(sym.to_string()));
    if let Some(op_sym) = op_sym {
        if op_sym.trim().is_empty() {
            return Err(cli_err(EX_PARSE, "vcs/why", "invalid --op: empty value"));
        }
        m.insert(
            TermOrdKey(Term::symbol(":op")),
            Term::Str(op_sym.to_string()),
        );
    }
    let payload = Term::Map(m);
    let k = Term::list(vec![
        Term::symbol("fn"),
        Term::list(vec![Term::symbol("r")]),
        Term::list(vec![Term::symbol("core/effect::pure"), Term::symbol("r")]),
    ]);
    let perform = Term::list(vec![Term::symbol("core/effect::perform"), op, payload, k]);
    Ok(vec![
        Term::list(vec![Term::symbol("def"), Term::symbol("prog"), perform]),
        Term::symbol("prog"),
    ])
}

fn mk_vcs_diff_program(base: &str, to: &str, out: Option<&Path>, store: bool) -> Vec<Term> {
    let op = Term::list(vec![Term::symbol("quote"), Term::symbol("core/vcs::diff")]);
    let mut m = std::collections::BTreeMap::new();
    m.insert(
        TermOrdKey(Term::symbol(":base")),
        Term::Str(base.to_string()),
    );
    m.insert(TermOrdKey(Term::symbol(":to")), Term::Str(to.to_string()));
    if let Some(out) = out {
        m.insert(
            TermOrdKey(Term::symbol(":out")),
            Term::Str(out.display().to_string()),
        );
    }
    m.insert(TermOrdKey(Term::symbol(":store")), Term::Bool(store));
    let payload = Term::Map(m);
    let k = Term::list(vec![
        Term::symbol("fn"),
        Term::list(vec![Term::symbol("r")]),
        Term::list(vec![Term::symbol("core/effect::pure"), Term::symbol("r")]),
    ]);
    let perform = Term::list(vec![Term::symbol("core/effect::perform"), op, payload, k]);
    vec![
        Term::list(vec![Term::symbol("def"), Term::symbol("prog"), perform]),
        Term::symbol("prog"),
    ]
}

fn mk_vcs_apply_program(base: &str, patch: &str, out: Option<&Path>, store: bool) -> Vec<Term> {
    let op = Term::list(vec![Term::symbol("quote"), Term::symbol("core/vcs::apply")]);
    let mut m = std::collections::BTreeMap::new();
    m.insert(
        TermOrdKey(Term::symbol(":base")),
        Term::Str(base.to_string()),
    );
    m.insert(
        TermOrdKey(Term::symbol(":patch")),
        Term::Str(patch.to_string()),
    );
    if let Some(out) = out {
        m.insert(
            TermOrdKey(Term::symbol(":out")),
            Term::Str(out.display().to_string()),
        );
    }
    m.insert(TermOrdKey(Term::symbol(":store")), Term::Bool(store));
    let payload = Term::Map(m);
    let k = Term::list(vec![
        Term::symbol("fn"),
        Term::list(vec![Term::symbol("r")]),
        Term::list(vec![Term::symbol("core/effect::pure"), Term::symbol("r")]),
    ]);
    let perform = Term::list(vec![Term::symbol("core/effect::perform"), op, payload, k]);
    vec![
        Term::list(vec![Term::symbol("def"), Term::symbol("prog"), perform]),
        Term::symbol("prog"),
    ]
}

fn mk_vcs_merge3_program(base: &str, left: &str, right: &str, out: Option<&Path>) -> Vec<Term> {
    let op = Term::list(vec![
        Term::symbol("quote"),
        Term::symbol("core/vcs::merge3"),
    ]);
    let mut m = std::collections::BTreeMap::new();
    m.insert(
        TermOrdKey(Term::symbol(":base")),
        Term::Str(base.to_string()),
    );
    m.insert(
        TermOrdKey(Term::symbol(":left")),
        Term::Str(left.to_string()),
    );
    m.insert(
        TermOrdKey(Term::symbol(":right")),
        Term::Str(right.to_string()),
    );
    if let Some(out) = out {
        m.insert(
            TermOrdKey(Term::symbol(":out")),
            Term::Str(out.display().to_string()),
        );
    }
    let payload = Term::Map(m);
    let k = Term::list(vec![
        Term::symbol("fn"),
        Term::list(vec![Term::symbol("r")]),
        Term::list(vec![Term::symbol("core/effect::pure"), Term::symbol("r")]),
    ]);
    let perform = Term::list(vec![Term::symbol("core/effect::perform"), op, payload, k]);
    vec![
        Term::list(vec![Term::symbol("def"), Term::symbol("prog"), perform]),
        Term::symbol("prog"),
    ]
}

fn mk_vcs_resolve_conflict_program(
    conflict: &str,
    strategy: Option<&str>,
    picks: &[String],
    sets: &[String],
    out: Option<&Path>,
) -> Result<Vec<Term>, CliError> {
    if strategy.is_none() && picks.is_empty() && sets.is_empty() {
        return Err(cli_err(
            EX_PARSE,
            "vcs/resolve-conflict",
            "must provide --strategy and/or --pick/--set overrides",
        ));
    }

    let op = Term::list(vec![
        Term::symbol("quote"),
        Term::symbol("core/vcs::resolve-conflict"),
    ]);
    let mut payload: std::collections::BTreeMap<TermOrdKey, Term> =
        std::collections::BTreeMap::new();
    payload.insert(
        TermOrdKey(Term::symbol(":conflict")),
        Term::Str(conflict.to_string()),
    );
    if let Some(s) = strategy {
        let s = s.trim();
        let sym = match s {
            "left" | ":left" => ":left",
            "right" | ":right" => ":right",
            "base" | ":base" => ":base",
            other => {
                return Err(cli_err(
                    EX_PARSE,
                    "vcs/resolve-conflict",
                    format!("unsupported --strategy {other} (expected left|right|base)"),
                ));
            }
        };
        payload.insert(
            TermOrdKey(Term::symbol(":strategy")),
            Term::Str(sym.to_string()),
        );
    }
    if let Some(out) = out {
        payload.insert(
            TermOrdKey(Term::symbol(":out")),
            Term::Str(out.display().to_string()),
        );
    }

    let mut res: std::collections::BTreeMap<String, Term> = std::collections::BTreeMap::new();
    for p in picks {
        let (opk, side) = p.split_once('=').ok_or_else(|| {
            cli_err(
                EX_PARSE,
                "vcs/resolve-conflict",
                format!("bad --pick {p}; expected op=left|right|base"),
            )
        })?;
        let opk = opk.trim();
        if opk.is_empty() {
            return Err(cli_err(
                EX_PARSE,
                "vcs/resolve-conflict",
                "bad --pick: empty op",
            ));
        }
        if res.contains_key(opk) {
            return Err(cli_err(
                EX_PARSE,
                "vcs/resolve-conflict",
                format!("duplicate resolution for op {opk}"),
            ));
        }
        let side = side.trim();
        let sym = match side {
            "left" | ":left" => ":left",
            "right" | ":right" => ":right",
            "base" | ":base" => ":base",
            other => {
                return Err(cli_err(
                    EX_PARSE,
                    "vcs/resolve-conflict",
                    format!("bad --pick {p}; unsupported side {other}"),
                ));
            }
        };
        res.insert(opk.to_string(), Term::Str(sym.to_string()));
    }
    for s in sets {
        let (opk, hv) = s.split_once('=').ok_or_else(|| {
            cli_err(
                EX_PARSE,
                "vcs/resolve-conflict",
                format!("bad --set {s}; expected op=<64-hex>"),
            )
        })?;
        let opk = opk.trim();
        if opk.is_empty() {
            return Err(cli_err(
                EX_PARSE,
                "vcs/resolve-conflict",
                "bad --set: empty op",
            ));
        }
        if res.contains_key(opk) {
            return Err(cli_err(
                EX_PARSE,
                "vcs/resolve-conflict",
                format!("duplicate resolution for op {opk}"),
            ));
        }
        let hv = hv.trim();
        gc_vcs::validate_hex_hash(hv)
            .map_err(|e| cli_err(EX_PARSE, "vcs/resolve-conflict", e.to_string()))?;
        res.insert(opk.to_string(), Term::Str(hv.to_string()));
    }
    if !res.is_empty() {
        let mut rm: std::collections::BTreeMap<TermOrdKey, Term> =
            std::collections::BTreeMap::new();
        for (k, v) in res {
            rm.insert(TermOrdKey(Term::Symbol(k)), v);
        }
        payload.insert(TermOrdKey(Term::symbol(":resolutions")), Term::Map(rm));
    }

    let payload = Term::Map(payload);
    let k = Term::list(vec![
        Term::symbol("fn"),
        Term::list(vec![Term::symbol("r")]),
        Term::list(vec![Term::symbol("core/effect::pure"), Term::symbol("r")]),
    ]);
    let perform = Term::list(vec![Term::symbol("core/effect::perform"), op, payload, k]);
    Ok(vec![
        Term::list(vec![Term::symbol("def"), Term::symbol("prog"), perform]),
        Term::symbol("prog"),
    ])
}

fn extract_vcs_patch_hash(v: &Value) -> Option<String> {
    let t = v.to_term_for_log(None);
    let Term::Map(m) = t else { return None };
    match m.get(&TermOrdKey(Term::symbol(":patch"))) {
        Some(Term::Str(s)) => Some(s.clone()),
        _ => None,
    }
}

fn extract_vcs_snapshot_hash(v: &Value) -> Option<String> {
    let t = v.to_term_for_log(None);
    let Term::Map(m) = t else { return None };
    match m.get(&TermOrdKey(Term::symbol(":snapshot"))) {
        Some(Term::Str(s)) => Some(s.clone()),
        _ => None,
    }
}

fn extract_vcs_commit_hash(v: &Value) -> Option<String> {
    let t = v.to_term_for_log(None);
    let Term::Map(m) = t else { return None };
    match m.get(&TermOrdKey(Term::symbol(":commit"))) {
        Some(Term::Str(s)) => Some(s.clone()),
        _ => None,
    }
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

fn extract_pkg_publish_commit(v: &Value) -> Option<String> {
    let t = v.to_term_for_log(None);
    let gc_coreform::Term::Map(m) = t else {
        return None;
    };
    match m.get(&gc_coreform::TermOrdKey(gc_coreform::Term::symbol(
        ":commit",
    ))) {
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
        PkgCmd::Publish {
            remote,
            refname,
            policy: policy_h,
            expected_old,
            depth,
            commit,
        } => (
            mk_pkg_publish_program(
                remote,
                refname,
                policy_h,
                expected_old.as_deref(),
                *depth,
                commit.as_deref(),
            ),
            "genesis/pkg-publish-v0.1",
            "pkg-publish",
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
            && let Some(gc_coreform::Term::Str(code)) = m.get(&gc_coreform::TermOrdKey(
                gc_coreform::Term::symbol(":error/code"),
            ))
        {
            if code == "core/caps/denied" {
                exit_code = EX_CAPS_DENY;
            } else if matches!(cmd, PkgCmd::Publish { .. })
                && (code.starts_with("core/pkg/")
                    || code.starts_with("core/refs/")
                    || code == "core/store/not-found")
            {
                exit_code = EX_OBLIGATIONS;
            }
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
            PkgCmd::Publish { .. } => extract_pkg_publish_commit(&r.value)
                .map(|h| format!("{h}\n"))
                .unwrap_or_else(|| {
                    if ok {
                        "ok\n".to_string()
                    } else {
                        format!("{value}\n")
                    }
                }),
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

fn cmd_sync(
    cli: &Cli,
    caps: &Path,
    log: &Option<PathBuf>,
    cmd: &SyncCmd,
) -> Result<CmdOut, CliError> {
    let policy = CapsPolicy::load(caps)
        .with_context(|| format!("read {}", caps.display()))
        .map_err(|e| cli_err(EX_PARSE, "caps/parse", format!("{e}")))?;

    let (forms, kind, log_op) = match cmd {
        SyncCmd::Pull {
            remote,
            refs,
            roots,
            depth,
            force,
        } => (
            mk_sync_pull_program(remote, refs, roots, *depth, *force),
            "genesis/sync-pull-v0.1",
            "sync-pull",
        ),
        SyncCmd::Push {
            remote,
            roots,
            depth,
            set_refs,
        } => {
            let parsed = parse_sync_set_refs(set_refs)?;
            (
                mk_sync_push_program(remote, roots, *depth, &parsed),
                "genesis/sync-push-v0.1",
                "sync-push",
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
        if let Value::Data(Term::Map(m)) = payload.as_ref()
            && matches!(
                m.get(&TermOrdKey(Term::symbol(":error/code"))),
                Some(Term::Str(s)) if s == "core/caps/denied"
            )
        {
            exit_code = EX_CAPS_DENY;
        }
    }

    let (value, value_format) = render_value_for_cli(&ctx, &r.value);
    let stdout = if cli.json {
        String::new()
    } else {
        format!("{value}\n")
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
                code: "sync/error",
                message: "sync operation failed".to_string(),
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

fn cmd_gc(cli: &Cli, caps: &Path, log: &Option<PathBuf>, cmd: &GcCmd) -> Result<CmdOut, CliError> {
    let policy = CapsPolicy::load(caps)
        .with_context(|| format!("read {}", caps.display()))
        .map_err(|e| cli_err(EX_PARSE, "caps/parse", format!("{e}")))?;

    let (forms, kind, log_op) = match cmd {
        GcCmd::Plan {
            lock,
            pins,
            depth,
            no_lock,
            no_refs,
        } => (
            mk_gc_plan_program(lock, pins, *depth, !*no_lock, !*no_refs),
            "genesis/gc-plan-v0.1",
            "gc-plan",
        ),
        GcCmd::Run {
            lock,
            pins,
            depth,
            no_lock,
            no_refs,
            quarantine,
            quarantine_dir,
        } => (
            mk_gc_run_program(
                lock,
                pins,
                *depth,
                !*no_lock,
                !*no_refs,
                *quarantine,
                quarantine_dir.as_deref(),
            ),
            "genesis/gc-run-v0.1",
            "gc-run",
        ),
        GcCmd::Pin { target, pins } => (
            mk_gc_pin_program(target, pins),
            "genesis/gc-pin-v0.1",
            "gc-pin",
        ),
        GcCmd::Unpin { target, pins } => (
            mk_gc_unpin_program(target, pins),
            "genesis/gc-unpin-v0.1",
            "gc-unpin",
        ),
        GcCmd::Purge {
            ttl_days,
            quarantine_dir,
        } => (
            mk_gc_purge_program(*ttl_days, quarantine_dir.as_deref()),
            "genesis/gc-purge-v0.1",
            "gc-purge",
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
        format!("{value}\n")
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
                code: "gc/error",
                message: "gc operation failed".to_string(),
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

#[derive(Debug, Clone, Serialize, Deserialize)]
struct PoliciesConfig {
    #[serde(default = "policy_config_version_one")]
    version: u64,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    default: Option<String>,
    #[serde(default)]
    aliases: std::collections::BTreeMap<String, String>,
}

fn policy_config_version_one() -> u64 {
    1
}

impl Default for PoliciesConfig {
    fn default() -> Self {
        Self {
            version: 1,
            default: None,
            aliases: std::collections::BTreeMap::new(),
        }
    }
}

fn normalize_policies_config(mut cfg: PoliciesConfig) -> Result<PoliciesConfig, String> {
    if cfg.version != 1 {
        return Err(format!(
            "unsupported policies config version {} (expected 1)",
            cfg.version
        ));
    }
    let mut aliases = std::collections::BTreeMap::new();
    for (name_raw, hash_raw) in cfg.aliases {
        let name = name_raw.trim();
        if name.is_empty() {
            return Err("policy alias names must be non-empty".to_string());
        }
        let hash = hash_raw.trim();
        if !is_hex64(hash) {
            return Err(format!("policy alias `{name}` must map to a 64-hex hash"));
        }
        if aliases
            .insert(name.to_string(), hash.to_ascii_lowercase())
            .is_some()
        {
            return Err(format!("duplicate policy alias `{name}`"));
        }
    }
    cfg.aliases = aliases;
    if let Some(default_raw) = cfg.default.take() {
        let d = default_raw.trim();
        if d.is_empty() {
            return Err("default policy selector must be non-empty".to_string());
        }
        cfg.default = Some(if is_hex64(d) {
            d.to_ascii_lowercase()
        } else {
            d.to_string()
        });
    } else {
        cfg.default = None;
    }
    Ok(cfg)
}

fn load_policies_config(path: &Path) -> Result<PoliciesConfig, CliError> {
    if !path.exists() {
        return Ok(PoliciesConfig::default());
    }
    let s = std::fs::read_to_string(path)
        .with_context(|| format!("read {}", path.display()))
        .map_err(|e| cli_err(EX_IO, "io/read", format!("{e}")))?;
    let cfg: PoliciesConfig =
        toml::from_str(&s).map_err(|e| cli_err(EX_PARSE, "policy/parse", format!("{e}")))?;
    normalize_policies_config(cfg).map_err(|e| cli_err(EX_PARSE, "policy/parse", e))
}

fn save_policies_config(path: &Path, cfg: &PoliciesConfig) -> Result<(), CliError> {
    if let Some(parent) = path.parent()
        && !parent.as_os_str().is_empty()
    {
        std::fs::create_dir_all(parent)
            .with_context(|| format!("create {}", parent.display()))
            .map_err(|e| cli_err(EX_IO, "io/write", format!("{e}")))?;
    }
    let s = toml::to_string_pretty(cfg)
        .map_err(|e| cli_err(EX_INTERNAL, "policy/serialize", format!("{e}")))?;
    std::fs::write(path, s)
        .with_context(|| format!("write {}", path.display()))
        .map_err(|e| cli_err(EX_IO, "io/write", format!("{e}")))
}

fn resolve_policy_selector(query: &str, cfg: &PoliciesConfig) -> Result<(String, String), String> {
    let q = query.trim();
    if q.is_empty() {
        return Err("policy selector must be non-empty".to_string());
    }
    if q == "default" {
        let Some(def) = cfg.default.as_deref() else {
            return Err("no default policy configured".to_string());
        };
        return resolve_policy_selector(def, cfg);
    }
    if is_hex64(q) {
        let h = q.to_ascii_lowercase();
        return Ok((h.clone(), h));
    }
    let h = cfg
        .aliases
        .get(q)
        .ok_or_else(|| format!("unknown policy alias `{q}`"))?;
    Ok((q.to_string(), h.clone()))
}

fn cmd_policy(cli: &Cli, cmd: &PolicyCmd) -> Result<CmdOut, CliError> {
    match cmd {
        PolicyCmd::List { policies } => {
            let cfg = load_policies_config(policies)?;
            let default_resolved = cfg
                .default
                .as_deref()
                .and_then(|d| resolve_policy_selector(d, &cfg).ok().map(|(_, h)| h));
            let stdout = if cli.json {
                String::new()
            } else {
                let mut s = String::new();
                s.push_str("default ");
                match cfg.default.as_deref() {
                    Some(d) => s.push_str(d),
                    None => s.push_str("nil"),
                }
                s.push('\n');
                if let Some(h) = &default_resolved {
                    s.push_str("default-resolved ");
                    s.push_str(h);
                    s.push('\n');
                }
                for (name, hash) in &cfg.aliases {
                    s.push_str(name);
                    s.push(' ');
                    s.push_str(hash);
                    s.push('\n');
                }
                s
            };
            let env = JsonEnvelope {
                ok: true,
                kind: "genesis/policy-list-v0.1",
                data: Some(serde_json::json!({
                    "policies": policies.display().to_string(),
                    "default": cfg.default,
                    "default_resolved": default_resolved,
                    "aliases": cfg.aliases.iter().map(|(k, v)| serde_json::json!({"name": k, "hash": v})).collect::<Vec<_>>(),
                })),
                error: None,
            };
            Ok(CmdOut {
                exit_code: EX_OK,
                stdout,
                json: serde_json::to_value(env).expect("json"),
            })
        }
        PolicyCmd::Show {
            name_or_hash,
            policies,
            store,
        } => {
            let cfg = load_policies_config(policies)?;
            let (resolved, hash) = resolve_policy_selector(name_or_hash, &cfg)
                .map_err(|e| cli_err(EX_VERIFY, "policy/resolve", e))?;
            let p = store.join(&hash);
            let bytes = std::fs::read(&p)
                .with_context(|| format!("read {}", p.display()))
                .map_err(|e| cli_err(EX_IO, "io/read", format!("{e}")))?;
            let src = String::from_utf8(bytes).map_err(|e| {
                cli_err(
                    EX_PARSE,
                    "policy/parse",
                    format!("policy artifact {} is not utf-8: {e}", p.display()),
                )
            })?;
            let t =
                parse_term(&src).map_err(|e| cli_err(EX_PARSE, "policy/parse", format!("{e}")))?;
            let pol = gc_vcs::Policy::from_term(&t)
                .map_err(|e| cli_err(EX_PARSE, "policy/schema", format!("{e}")))?;
            let printed = print_term(&t);
            let stdout = if cli.json {
                String::new()
            } else {
                format!("{printed}\n")
            };
            let env = JsonEnvelope {
                ok: true,
                kind: "genesis/policy-show-v0.1",
                data: Some(serde_json::json!({
                    "query": name_or_hash,
                    "resolved": resolved,
                    "hash": hash,
                    "store": store.display().to_string(),
                    "term": printed,
                    "name": pol.name,
                    "frozen_prefixes": pol.frozen_prefixes,
                    "classes": {
                        "dev": pol.dev.is_some(),
                        "main": pol.main.is_some(),
                        "tags": pol.tags.is_some(),
                    }
                })),
                error: None,
            };
            Ok(CmdOut {
                exit_code: EX_OK,
                stdout,
                json: serde_json::to_value(env).expect("json"),
            })
        }
        PolicyCmd::SetDefault {
            name_or_hash,
            policies,
        } => {
            let mut cfg = load_policies_config(policies)?;
            if is_hex64(name_or_hash) {
                cfg.default = Some(name_or_hash.to_ascii_lowercase());
            } else {
                let alias = name_or_hash.trim();
                if alias.is_empty() {
                    return Err(cli_err(
                        EX_PARSE,
                        "policy/set-default",
                        "policy selector must be non-empty",
                    ));
                }
                if !cfg.aliases.contains_key(alias) {
                    return Err(cli_err(
                        EX_VERIFY,
                        "policy/set-default",
                        format!("unknown policy alias `{alias}`"),
                    ));
                }
                cfg.default = Some(alias.to_string());
            }
            save_policies_config(policies, &cfg)?;
            let (_, resolved_hash) =
                resolve_policy_selector(cfg.default.as_deref().unwrap_or(""), &cfg)
                    .map_err(|e| cli_err(EX_VERIFY, "policy/set-default", e))?;
            let stdout = if cli.json {
                String::new()
            } else {
                format!(
                    "default {}\ndefault-resolved {}\n",
                    cfg.default.as_deref().unwrap_or("nil"),
                    resolved_hash
                )
            };
            let env = JsonEnvelope {
                ok: true,
                kind: "genesis/policy-set-default-v0.1",
                data: Some(serde_json::json!({
                    "policies": policies.display().to_string(),
                    "default": cfg.default,
                    "default_resolved": resolved_hash,
                })),
                error: None,
            };
            Ok(CmdOut {
                exit_code: EX_OK,
                stdout,
                json: serde_json::to_value(env).expect("json"),
            })
        }
    }
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

fn cmd_vcs(
    cli: &Cli,
    caps: Option<&Path>,
    log: &Option<PathBuf>,
    cmd: &VcsCmd,
) -> Result<CmdOut, CliError> {
    if let VcsCmd::Hash { input, engine } = cmd {
        return cmd_vcs_hash(cli, input, *engine);
    }

    let caps = caps.ok_or_else(|| {
        cli_err(
            EX_PARSE,
            "caps/missing",
            "missing --caps (required for effectful vcs operations)",
        )
    })?;
    let policy = CapsPolicy::load(caps)
        .with_context(|| format!("read {}", caps.display()))
        .map_err(|e| cli_err(EX_PARSE, "caps/parse", format!("{e}")))?;

    let (forms, kind, log_op) = match cmd {
        VcsCmd::Hash { .. } => unreachable!("handled above"),
        VcsCmd::Diff {
            base,
            to,
            out,
            no_store,
        } => (
            mk_vcs_diff_program(base, to, out.as_deref(), !*no_store),
            "genesis/vcs-diff-v0.1",
            "vcs-diff",
        ),
        VcsCmd::Apply {
            base,
            patch,
            out,
            no_store,
        } => (
            mk_vcs_apply_program(base, patch, out.as_deref(), !*no_store),
            "genesis/vcs-apply-v0.1",
            "vcs-apply",
        ),
        VcsCmd::Log { root, max } => (
            mk_vcs_log_program(root, *max),
            "genesis/vcs-log-v0.1",
            "vcs-log",
        ),
        VcsCmd::Blame {
            snapshot,
            sym,
            path,
        } => (
            mk_vcs_blame_program(snapshot, sym, path.as_deref())?,
            "genesis/vcs-blame-v0.1",
            "vcs-blame",
        ),
        VcsCmd::Why { snapshot, sym, op } => (
            mk_vcs_why_program(snapshot, sym, op.as_deref())?,
            "genesis/vcs-why-v0.1",
            "vcs-why",
        ),
        VcsCmd::Merge3 {
            base,
            left,
            right,
            out,
        } => (
            mk_vcs_merge3_program(base, left, right, out.as_deref()),
            "genesis/vcs-merge3-v0.1",
            "vcs-merge3",
        ),
        VcsCmd::ResolveConflict {
            conflict,
            strategy,
            picks,
            sets,
            out,
        } => (
            mk_vcs_resolve_conflict_program(
                conflict,
                strategy.as_deref(),
                picks,
                sets,
                out.as_deref(),
            )?,
            "genesis/vcs-resolve-conflict-v0.1",
            "vcs-resolve-conflict",
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
        if let Value::Data(Term::Map(m)) = payload.as_ref()
            && matches!(
                m.get(&TermOrdKey(Term::symbol(":error/code"))),
                Some(Term::Str(s)) if s == "core/caps/denied"
            )
        {
            exit_code = EX_CAPS_DENY;
        }
    }

    let (value, value_format) = render_value_for_cli(&ctx, &r.value);

    if matches!(cmd, VcsCmd::Merge3 { .. } | VcsCmd::ResolveConflict { .. })
        && let Value::Data(Term::Map(m)) = &r.value
        && matches!(
            m.get(&TermOrdKey(Term::symbol(":ok"))),
            Some(Term::Bool(false))
        )
        && m.contains_key(&TermOrdKey(Term::symbol(":conflict")))
    {
        ok = false;
        exit_code = 3;
    }

    let stdout = if cli.json {
        String::new()
    } else {
        match cmd {
            VcsCmd::Diff { .. } => extract_vcs_patch_hash(&r.value)
                .map(|h| format!("{h}\n"))
                .unwrap_or_else(|| format!("{value}\n")),
            VcsCmd::Apply { .. } => extract_vcs_snapshot_hash(&r.value)
                .map(|h| format!("{h}\n"))
                .unwrap_or_else(|| format!("{value}\n")),
            VcsCmd::Blame { .. } => extract_vcs_commit_hash(&r.value)
                .map(|h| format!("{h}\n"))
                .unwrap_or_else(|| format!("{value}\n")),
            _ => format!("{value}\n"),
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
                code: "vcs/error",
                message: "vcs operation failed".to_string(),
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
        Cmd::Explain {
            file,
            engine,
            contract,
            msg,
        } => cmd_explain(cli, file, *engine, contract, msg),
        Cmd::Test { pkg, caps } => cmd_test(cli, pkg.as_path(), caps.as_deref()),
        Cmd::Pack { pkg } => cmd_pack(cli, pkg.as_path()),
        Cmd::Typecheck { pkg } => cmd_typecheck(cli, pkg.as_path()),
        Cmd::Optimize {
            file,
            out,
            emit_wasm,
            engine,
            stage1_gate,
            stage2_gate,
        } => cmd_optimize(
            cli,
            file,
            out.as_ref(),
            emit_wasm.as_ref(),
            *engine,
            *stage1_gate,
            *stage2_gate,
        ),
        Cmd::ApplyPatch { patch, pkg, caps } => cmd_apply_patch(cli, patch, pkg, caps.as_deref()),
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
        Cmd::SelfhostDashboard { markdown, store } => {
            cmd_selfhost_dashboard(cli, markdown.as_deref(), store.as_deref())
        }
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
        Cmd::Policy { cmd } => cmd_policy(cli, cmd),
        Cmd::Sync { caps, log, cmd } => cmd_sync(cli, caps.as_path(), log, cmd),
        Cmd::Gc { caps, log, cmd } => cmd_gc(cli, caps.as_path(), log, cmd),
        Cmd::Vcs { caps, log, cmd } => cmd_vcs(cli, caps.as_deref(), log, cmd),
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

#[cfg(test)]
mod tests {
    use super::{EX_PARSE, parse_set_ref_spec, parse_sync_set_refs};

    #[test]
    fn parse_set_ref_spec_supports_contract_refs_with_colons() {
        let commit = "a".repeat(64);
        let policy = "b".repeat(64);
        let expected_old = "c".repeat(64);
        let spec = format!(
            "refs/contracts/my-lib/counter::Counter/heads/dev:{commit}:{policy}@{expected_old}"
        );
        let parsed = parse_set_ref_spec(&spec).expect("parse");
        assert_eq!(
            parsed.name,
            "refs/contracts/my-lib/counter::Counter/heads/dev"
        );
        assert_eq!(parsed.hash, commit);
        assert_eq!(parsed.policy, policy);
        assert_eq!(parsed.expected_old.as_deref(), Some(expected_old.as_str()));
    }

    #[test]
    fn parse_set_ref_spec_rejects_invalid_hashes() {
        let err = parse_set_ref_spec("refs/heads/main:nothex:alsonothex").expect_err("must fail");
        assert_eq!(err.exit_code, EX_PARSE);
    }

    #[test]
    fn parse_set_ref_spec_accepts_expected_old_nil() {
        let commit = "a".repeat(64);
        let policy = "b".repeat(64);
        let spec = format!("refs/heads/main:{commit}:{policy}@nil");
        let parsed = parse_set_ref_spec(&spec).expect("parse");
        assert_eq!(parsed.expected_old.as_deref(), Some("nil"));
    }

    #[test]
    fn parse_set_ref_spec_supports_contract_refs_without_expected_old() {
        let commit = "a".repeat(64);
        let policy = "b".repeat(64);
        let spec = format!("refs/contracts/p::q/heads/dev:{commit}:{policy}");
        let parsed = parse_set_ref_spec(&spec).expect("parse");
        assert_eq!(parsed.name, "refs/contracts/p::q/heads/dev");
        assert_eq!(parsed.hash, commit);
        assert_eq!(parsed.policy, policy);
        assert_eq!(parsed.expected_old, None);
    }

    #[test]
    fn parse_sync_set_refs_rejects_duplicate_targets() {
        let commit = "a".repeat(64);
        let policy = "b".repeat(64);
        let specs = vec![
            format!("refs/heads/main:{commit}:{policy}"),
            format!("refs/heads/main:{commit}:{policy}@nil"),
        ];
        let err = parse_sync_set_refs(&specs).expect_err("must fail");
        assert_eq!(err.exit_code, EX_PARSE);
    }
}
