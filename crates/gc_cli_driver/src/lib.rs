use std::path::{Path, PathBuf};

use anyhow::Context;
use clap::{Parser, Subcommand, ValueEnum};
use serde::{Deserialize, Serialize};

use gc_coreform::{
    Term, TermOrdKey, canonicalize_module, hash_module, parse_module, parse_term, print_module,
    print_term,
};
use gc_effects::{CapsPolicy, Decision, EffectLog};
use gc_kernel::{Apply, EvalCtx, MemLimits, SealId, StepLimit, Value, eval_module, eval_term};
use gc_obligations::PackageManifest;
use gc_prelude::{
    SelfhostBootstrapMode, build_prelude, embedded_bootstrap_available,
    load_selfhost_coreform_toolchain_v1_with_mode, selfhost_coreform_toolchain_v1_sources,
};

const EX_OK: u8 = 0;
const EX_INTERNAL: u8 = 1;
const EX_PARSE: u8 = 10;
const EX_FMT: u8 = 11;
const EX_EVAL: u8 = 20;
const EX_OBLIGATIONS: u8 = 30;
const EX_REPLAY_MISMATCH: u8 = 40;
const EX_CAPS_DENIED: u8 = 41;
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

    /// CoreForm frontend for command groups that do not expose `--engine`.
    /// Defaults to `selfhost` in the fast-path profile.
    #[arg(long, global = true, value_enum)]
    coreform_frontend: Option<CoreformFrontendArg>,

    #[command(subcommand)]
    cmd: Cmd,
}

#[derive(Debug, Clone, Copy, ValueEnum)]
enum SelfhostBootstrapArg {
    ArtifactOnly,
    ArtifactPreferred,
    Embedded,
}

#[derive(Debug, Clone, Copy, ValueEnum, PartialEq, Eq)]
enum CoreformFrontendArg {
    Rust,
    Selfhost,
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

    /// Run an effect program with a capability policy and emit a deterministic log.
    Run {
        file: PathBuf,
        /// Frontend engine. `rust` uses the Rust CoreForm frontend; `selfhost` runs the
        /// self-hosted CoreForm parser+canonicalizer inside the kernel.
        #[arg(long, value_enum)]
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
        /// Frontend engine. `rust` uses the Rust CoreForm frontend; `selfhost` runs the
        /// self-hosted CoreForm parser+canonicalizer inside the kernel.
        #[arg(long, value_enum)]
        engine: Option<FmtEngine>,
        /// Input effect log path (.gclog).
        #[arg(long)]
        log: PathBuf,
        /// Artifact store directory for logs that externalize large responses.
        #[arg(long)]
        store: Option<PathBuf>,
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
        /// Frontend engine. `rust` uses the Rust CoreForm frontend; `selfhost` runs the
        /// self-hosted CoreForm parser+canonicalizer inside the kernel.
        #[arg(long, value_enum)]
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

    /// GenesisPkg operations (snapshot + bundle export/import).
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

#[derive(Copy, Clone, Debug, Eq, PartialEq, ValueEnum)]
enum FmtEngine {
    Rust,
    Selfhost,
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

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Flavor {
    Native,
    Wasi,
}

pub fn run(flavor: Flavor) -> std::process::ExitCode {
    let cli = Cli::parse();
    match dispatch(&cli, flavor) {
        Ok(out) => {
            if cli.json {
                // JSON mode: exactly one JSON object on stdout.
                println!(
                    "{}",
                    serde_json::to_string(&out.json).expect("json serialization")
                );
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
                    .expect("json serialization")
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

fn dispatch(cli: &Cli, flavor: Flavor) -> Result<CmdOut, CliError> {
    enforce_selfhost_only_cmd(cli, flavor)?;
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
        Cmd::Run {
            file,
            engine,
            caps,
            log,
        } => cmd_run(cli, flavor, file, *engine, caps, log.as_deref()),
        Cmd::Replay {
            file,
            engine,
            log,
            store,
        } => cmd_replay(cli, file, *engine, log, store.as_deref()),
        Cmd::Test { pkg, caps } => cmd_test(cli, pkg, caps.as_deref()),
        Cmd::Pack { pkg } => cmd_pack(cli, pkg),
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
        Cmd::Keygen { out } => cmd_keygen(cli, out),
        Cmd::Sign {
            pkg,
            key,
            acceptance,
            signatures,
        } => cmd_sign(cli, pkg, key, acceptance.as_deref(), signatures.as_deref()),
        Cmd::TransparencyVerify { pkg } => cmd_transparency_verify(cli, pkg),
        Cmd::Typecheck { pkg } => cmd_typecheck(cli, pkg),
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
        Cmd::Verify {
            pkg,
            acceptance,
            policy,
            signatures,
            scan_store,
        } => cmd_verify(
            cli,
            pkg,
            acceptance.as_deref(),
            policy.as_deref(),
            signatures.as_deref(),
            *scan_store,
        ),
        Cmd::Store { caps, log, cmd } => cmd_store(cli, caps, log.as_deref(), cmd),
        Cmd::Refs { caps, log, cmd } => cmd_refs(cli, caps, log.as_deref(), cmd),
        Cmd::Pkg { caps, log, cmd } => cmd_pkg(cli, caps, log.as_deref(), cmd),
        Cmd::Policy { cmd } => cmd_policy(cli, cmd),
        Cmd::Sync { caps, log, cmd } => cmd_sync(cli, caps, log.as_deref(), cmd),
        Cmd::Gc { caps, log, cmd } => cmd_gc(cli, caps, log.as_deref(), cmd),
        Cmd::Vcs { caps, log, cmd } => cmd_vcs(cli, caps.as_deref(), log.as_deref(), cmd),
    }
}

fn resolved_step_limit(cli: &Cli) -> StepLimit {
    if cli.no_step_limit {
        return StepLimit::Unlimited;
    }
    if let Some(n) = cli.step_limit {
        return StepLimit::Limit(n);
    }
    StepLimit::Default
}

fn resolved_mem_limits(cli: &Cli) -> MemLimits {
    MemLimits {
        max_pair_cells: cli.max_pair_cells,
        max_vec_len: cli.max_vec_len,
        max_map_len: cli.max_map_len,
        max_bytes_len: cli.max_bytes_len,
        max_string_len: cli.max_string_len,
    }
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

fn resolved_coreform_frontend(cli: &Cli) -> Result<gc_obligations::CoreformFrontend, CliError> {
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
                return Err(cli_err(
                    EX_VERIFY,
                    "engine/rust-disabled",
                    format!(
                        "`--coreform-frontend rust` is disabled in the default selfhost profile; set {ALLOW_RUST_ENGINE_ENV}=1 to enable compatibility mode"
                    ),
                ));
            }
            Ok(gc_obligations::CoreformFrontend::Rust)
        }
        CoreformFrontendArg::Selfhost => {
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
    }
}

fn coreform_frontend_json(frontend: &gc_obligations::CoreformFrontend) -> serde_json::Value {
    match frontend {
        gc_obligations::CoreformFrontend::Rust => serde_json::json!({
            "name": "rust"
        }),
        gc_obligations::CoreformFrontend::Selfhost(cfg) => serde_json::json!({
            "name": "selfhost",
            "bootstrap_mode": match cfg.bootstrap_mode {
                SelfhostBootstrapMode::ArtifactOnly => "artifact-only",
                SelfhostBootstrapMode::ArtifactPreferred => "artifact-preferred",
                SelfhostBootstrapMode::Embedded => "embedded",
            },
            "artifact": cfg.artifact.as_ref().map(|p| p.display().to_string()),
        }),
    }
}

fn coreform_frontend_for_engine(cli: &Cli, engine: FmtEngine) -> gc_obligations::CoreformFrontend {
    match engine {
        FmtEngine::Rust => gc_obligations::CoreformFrontend::Rust,
        FmtEngine::Selfhost => {
            gc_obligations::CoreformFrontend::Selfhost(gc_obligations::SelfhostFrontendConfig {
                bootstrap_mode: resolved_selfhost_bootstrap_mode(cli),
                artifact: resolved_selfhost_artifact_for_frontend(cli),
            })
        }
    }
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

fn enforce_selfhost_only_cmd(cli: &Cli, _flavor: Flavor) -> Result<(), CliError> {
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
        Cmd::Pack { .. } => Ok(()),
        Cmd::Store { .. } => Ok(()),
        Cmd::Refs { .. } => Ok(()),
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
        Cmd::Vcs {
            cmd: VcsCmd::Hash { engine, .. },
            ..
        } => enforce_selfhost_engine(cli, "vcs hash", *engine),
        Cmd::Vcs { .. } => Ok(()),
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
        gc_obligations::ObligationError::Test(s) => cli_err(EX_EVAL, "test/error", s),
        gc_obligations::ObligationError::Typecheck(s) => cli_err(EX_EVAL, "typecheck/error", s),
        gc_obligations::ObligationError::Opt(s) => cli_err(EX_INTERNAL, "opt/error", s),
        gc_obligations::ObligationError::Store(s) => cli_err(EX_INTERNAL, "store/error", s),
        gc_obligations::ObligationError::Io(e) => cli_err(EX_IO, "io/error", format!("{e}")),
    }
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

            let f = env.get("core/cli::fmt-module").ok_or_else(|| {
                cli_err(
                    EX_INTERNAL,
                    "selfhost/missing",
                    "missing binding core/cli::fmt-module",
                )
            })?;

            // Now apply the user-configured step limit to the formatting work itself.
            ctx.steps = 0;
            ctx.step_limit = resolved_step_limit(cli).resolve();
            let r = f
                .apply(&mut ctx, Value::Data(Term::Str(src.clone())))
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

            let Some(Term::Str(s)) = r.as_data() else {
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
        Term::Map(m) => {
            let code = m
                .get(&gc_coreform::TermOrdKey(Term::Symbol(
                    ":error/code".to_string(),
                )))
                .and_then(|t| match t {
                    Term::Str(s) => Some(s.clone()),
                    _ => None,
                })
                .unwrap_or_else(|| "core/error".to_string());
            let msg = m
                .get(&gc_coreform::TermOrdKey(Term::Symbol(
                    ":error/message".to_string(),
                )))
                .and_then(|t| match t {
                    Term::Str(s) => Some(s.clone()),
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
) -> Result<Vec<Term>, CliError> {
    if let Some(canon_src_fn) = env.get("core/cli::canonicalize-module-src") {
        let canon = canon_src_fn
            .apply(ctx, Value::Data(Term::Str(src.to_string())))
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

        let Some(Term::Vector(forms)) = canon.as_data() else {
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
        .apply(ctx, Value::Data(Term::Str(src.to_string())))
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

    let Some(Term::Vector(parsed_forms)) = parsed.as_data() else {
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
        .apply(ctx, Value::Data(Term::Vector(parsed_forms.clone())))
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

    let Some(Term::Vector(forms)) = canon.as_data() else {
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

fn parse_hex32_for_cli(hex: &str, context: &str) -> Result<[u8; 32], CliError> {
    let s = hex.trim();
    if s.len() != 64 {
        return Err(cli_err(
            EX_PARSE,
            "selfhost/hash",
            format!("{context} returned non-64-byte hex hash"),
        ));
    }
    let mut out = [0u8; 32];
    for (i, pair) in s.as_bytes().chunks_exact(2).enumerate() {
        let hi = (pair[0] as char).to_digit(16).ok_or_else(|| {
            cli_err(
                EX_PARSE,
                "selfhost/hash",
                format!("{context} returned invalid hex hash"),
            )
        })?;
        let lo = (pair[1] as char).to_digit(16).ok_or_else(|| {
            cli_err(
                EX_PARSE,
                "selfhost/hash",
                format!("{context} returned invalid hex hash"),
            )
        })?;
        out[i] = ((hi << 4) | lo) as u8;
    }
    Ok(out)
}

fn selfhost_hash_module_forms(
    ctx: &mut EvalCtx,
    env: &gc_kernel::Env,
    forms: &[Term],
) -> Result<[u8; 32], CliError> {
    if let Some(hash_forms_fn) = env.get("core/cli::hash-module-forms") {
        let out = hash_forms_fn
            .apply(ctx, Value::Data(Term::Vector(forms.to_vec())))
            .map_err(|e| cli_err(EX_EVAL, "eval/error", format!("selfhost hash failed: {e}")))?;
        if let Some((code, message, payload)) = extract_protocol_error(ctx, &out) {
            return Err(CliError {
                exit_code: EX_PARSE,
                json: JsonError {
                    code: "selfhost/error",
                    message: format!("{code}: {message}"),
                    context: payload.map(serde_json::Value::String),
                },
            });
        }
        let Some(Term::Str(hex)) = out.as_data() else {
            return Err(cli_err(
                EX_INTERNAL,
                "selfhost/bad-return",
                format!(
                    "core/cli hash-module-forms returned non-string: {}",
                    out.debug_repr()
                ),
            ));
        };
        return parse_hex32_for_cli(hex, "core/cli hash-module-forms");
    }

    let hash_fn = env.get("selfhost/hash::hash-module").ok_or_else(|| {
        cli_err(
            EX_INTERNAL,
            "selfhost/missing",
            "missing binding selfhost/hash::hash-module",
        )
    })?;
    let out = hash_fn
        .apply(ctx, Value::Data(Term::Vector(forms.to_vec())))
        .map_err(|e| cli_err(EX_EVAL, "eval/error", format!("selfhost hash failed: {e}")))?;
    if let Some((code, message, payload)) = extract_protocol_error(ctx, &out) {
        return Err(CliError {
            exit_code: EX_PARSE,
            json: JsonError {
                code: "selfhost/error",
                message: format!("{code}: {message}"),
                context: payload.map(serde_json::Value::String),
            },
        });
    }
    let Some(Term::Str(hex)) = out.as_data() else {
        return Err(cli_err(
            EX_INTERNAL,
            "selfhost/bad-return",
            format!(
                "selfhost hash-module returned non-string: {}",
                out.debug_repr()
            ),
        ));
    };
    parse_hex32_for_cli(hex, "selfhost hash-module")
}

fn selfhost_stage1_transform_module(
    ctx: &mut EvalCtx,
    env: &gc_kernel::Env,
    forms: &[Term],
) -> Result<Vec<Term>, CliError> {
    let stage1_fn = env
        .get("core/cli::stage1-transform-module")
        .ok_or_else(|| {
            cli_err(
                EX_INTERNAL,
                "selfhost/missing",
                "missing binding core/cli::stage1-transform-module",
            )
        })?;
    let out = stage1_fn
        .apply(ctx, Value::Data(Term::Vector(forms.to_vec())))
        .map_err(|e| {
            cli_err(
                EX_EVAL,
                "eval/error",
                format!("selfhost stage1 failed: {e}"),
            )
        })?;
    if let Some((code, message, payload)) = extract_protocol_error(ctx, &out) {
        return Err(CliError {
            exit_code: EX_INTERNAL,
            json: JsonError {
                code: "selfhost/error",
                message: format!("{code}: {message}"),
                context: payload.map(serde_json::Value::String),
            },
        });
    }
    let Some(Term::Vector(transformed)) = out.as_data() else {
        return Err(cli_err(
            EX_INTERNAL,
            "selfhost/bad-return",
            format!(
                "core/cli stage1-transform-module returned non-vector: {}",
                out.debug_repr()
            ),
        ));
    };
    Ok(transformed.clone())
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

fn selfhost_plan_request_map(
    cli: &Cli,
    binding: &str,
    req: Term,
    cmd_name: &str,
) -> Result<std::collections::BTreeMap<TermOrdKey, Term>, CliError> {
    let mut ctx = mk_ctx(cli);
    let prelude = build_prelude(&mut ctx);
    let mut env = prelude.env;
    load_selfhost_toolchain(cli, &mut ctx, &mut env)?;

    let f = env.get(binding).ok_or_else(|| {
        cli_err(
            EX_INTERNAL,
            "selfhost/missing",
            format!("missing binding {binding}"),
        )
    })?;
    let out = f.apply(&mut ctx, Value::Data(req)).map_err(|e| {
        cli_err(
            EX_EVAL,
            "eval/error",
            format!("{binding} failed for {cmd_name}: {e}"),
        )
    })?;

    if let Some((code, message, payload)) = extract_protocol_error(&ctx, &out) {
        return Err(CliError {
            exit_code: EX_PARSE,
            json: JsonError {
                code: "selfhost/error",
                message: format!("{cmd_name}: {code}: {message}"),
                context: payload.map(serde_json::Value::String),
            },
        });
    }

    if let Some(Term::Map(m)) = out.as_data() {
        return Ok(m.clone());
    }
    let fallback = out.to_term_for_log(ctx.protocol.map(|p| p.error));
    if let Term::Map(m) = fallback {
        return Ok(m);
    }
    Err(cli_err(
        EX_INTERNAL,
        "selfhost/bad-return",
        format!(
            "{binding} returned non-map for {cmd_name}: {}",
            out.debug_repr()
        ),
    ))
}

fn planned_required_str(
    m: &std::collections::BTreeMap<TermOrdKey, Term>,
    key: &str,
    cmd_name: &str,
) -> Result<String, CliError> {
    match m.get(&TermOrdKey(Term::symbol(key))) {
        Some(Term::Str(s)) => Ok(s.clone()),
        _ => Err(cli_err(
            EX_PARSE,
            "selfhost/plan",
            format!("{cmd_name}: planner returned invalid {key}"),
        )),
    }
}

fn planned_optional_str(
    m: &std::collections::BTreeMap<TermOrdKey, Term>,
    key: &str,
    cmd_name: &str,
) -> Result<Option<String>, CliError> {
    match m.get(&TermOrdKey(Term::symbol(key))) {
        Some(Term::Str(s)) => Ok(Some(s.clone())),
        Some(Term::Nil) | None => Ok(None),
        _ => Err(cli_err(
            EX_PARSE,
            "selfhost/plan",
            format!("{cmd_name}: planner returned invalid {key}"),
        )),
    }
}

fn planned_required_bool(
    m: &std::collections::BTreeMap<TermOrdKey, Term>,
    key: &str,
    cmd_name: &str,
) -> Result<bool, CliError> {
    match m.get(&TermOrdKey(Term::symbol(key))) {
        Some(Term::Bool(b)) => Ok(*b),
        _ => Err(cli_err(
            EX_PARSE,
            "selfhost/plan",
            format!("{cmd_name}: planner returned invalid {key}"),
        )),
    }
}

fn planned_required_u64(
    m: &std::collections::BTreeMap<TermOrdKey, Term>,
    key: &str,
    cmd_name: &str,
) -> Result<u64, CliError> {
    let Some(Term::Int(i)) = m.get(&TermOrdKey(Term::symbol(key))) else {
        return Err(cli_err(
            EX_PARSE,
            "selfhost/plan",
            format!("{cmd_name}: planner returned invalid {key}"),
        ));
    };
    i.to_string().parse::<u64>().map_err(|_| {
        cli_err(
            EX_PARSE,
            "selfhost/plan",
            format!("{cmd_name}: planner returned out-of-range {key}"),
        )
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
                    context: Some(gc_opt::stage1_pipeline_json(&out)),
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
                    context: Some(gc_opt::stage2_report_json(s2)),
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
            "stage1": stage1.as_ref().map(gc_opt::stage1_pipeline_json),
            "stage2": stage2.as_ref().map(gc_opt::stage2_report_json),
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
            // Parse/canonicalize with selfhost bindings loaded, then evaluate in a fresh
            // prelude-only env so contract closure hashing matches the rust frontend path.
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

fn cmd_run(
    cli: &Cli,
    flavor: Flavor,
    file: &Path,
    engine: Option<FmtEngine>,
    caps: &Path,
    log: Option<&Path>,
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
    let program_hash = hash_module(&forms);

    let policy = CapsPolicy::load(caps)
        .with_context(|| format!("read {}", caps.display()))
        .map_err(|e| cli_err(EX_PARSE, "caps/parse", format!("{e}")))?;

    let prog = eval_module(&mut ctx, &mut env, &forms)
        .map_err(|e| cli_err(EX_EVAL, "eval/error", format!("{e}")))?;

    let toolchain = match flavor {
        Flavor::Native => format!("genesis/{} (native)", env!("CARGO_PKG_VERSION")),
        Flavor::Wasi => format!("genesis_wasi/{} (wasi)", env!("CARGO_PKG_VERSION")),
    };
    let r = gc_effects::run(&mut ctx, &policy, prog, program_hash, toolchain)
        .map_err(|e| cli_err(EX_EVAL, "effects/run", format!("{e}")))?;

    let log_path = log
        .map(PathBuf::from)
        .unwrap_or_else(|| file.with_extension("gclog"));
    std::fs::write(&log_path, r.log.to_string_canonical() + "\n")
        .with_context(|| format!("write {}", log_path.display()))
        .map_err(|e| cli_err(EX_IO, "io/write", format!("{e}")))?;

    let denied = r.log.entries.iter().any(|e| e.decision == Decision::Deny);
    let exit_code = if denied { EX_CAPS_DENIED } else { EX_OK };

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

fn default_log_path(op: &str) -> PathBuf {
    let dir = PathBuf::from(".genesis").join("logs");
    let _ = std::fs::create_dir_all(&dir);
    let stamp = format!(
        "{}-{}",
        std::process::id(),
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap_or_default()
            .as_millis()
    );
    dir.join(format!("{op}-{stamp}.gclog"))
}

fn cmd_store(
    cli: &Cli,
    caps: &Path,
    log: Option<&Path>,
    cmd: &StoreCmd,
) -> Result<CmdOut, CliError> {
    let frontend = resolved_coreform_frontend(cli)?;
    let frontend_info = coreform_frontend_json(&frontend);

    let policy = CapsPolicy::load(caps)
        .with_context(|| format!("read {}", caps.display()))
        .map_err(|e| cli_err(EX_PARSE, "caps/parse", format!("{e}")))?;

    let mut ctx = mk_ctx(cli);
    let prelude = build_prelude(&mut ctx);
    let mut env = prelude.env;

    let (prog, kind, log_op, program_hash) = match &frontend {
        gc_obligations::CoreformFrontend::Rust => {
            let (forms, kind, log_op) = match cmd {
                StoreCmd::Put { input } => {
                    let src = std::fs::read_to_string(input)
                        .with_context(|| format!("read {}", input.display()))
                        .map_err(|e| cli_err(EX_IO, "io/read", format!("{e}")))?;
                    let art = parse_term(&src)
                        .map_err(|e| cli_err(EX_PARSE, "parse/term", e.to_string()))?;
                    (
                        mk_store_put_program(&art),
                        "genesis/store-put-v0.2",
                        "store-put",
                    )
                }
                StoreCmd::Get { hash, .. } => (
                    mk_store_get_program(hash),
                    "genesis/store-get-v0.2",
                    "store-get",
                ),
                StoreCmd::Has { hash } => (
                    mk_store_has_program(hash),
                    "genesis/store-has-v0.2",
                    "store-has",
                ),
            };
            let forms = canonicalize_module(forms)
                .map_err(|e| cli_err(EX_PARSE, "canon/coreform", e.to_string()))?;
            let program_hash = hash_module(&forms);
            let prog = eval_module(&mut ctx, &mut env, &forms)
                .map_err(|e| cli_err(EX_EVAL, "eval/error", format!("{e}")))?;
            (prog, kind, log_op, program_hash)
        }
        gc_obligations::CoreformFrontend::Selfhost(_) => {
            load_selfhost_toolchain(cli, &mut ctx, &mut env)?;
            let (prog, kind, log_op, desc) = match cmd {
                StoreCmd::Put { input } => {
                    let src = std::fs::read_to_string(input)
                        .with_context(|| format!("read {}", input.display()))
                        .map_err(|e| cli_err(EX_IO, "io/read", format!("{e}")))?;
                    let art = selfhost_parse_term(&mut ctx, &env, &src, "store put input")?;
                    let f = env.get("core/cli::store-put-program").ok_or_else(|| {
                        cli_err(
                            EX_INTERNAL,
                            "selfhost/missing",
                            "missing binding core/cli::store-put-program",
                        )
                    })?;
                    let prog = f.apply(&mut ctx, Value::Data(art.clone())).map_err(|e| {
                        cli_err(
                            EX_EVAL,
                            "eval/error",
                            format!("core/cli store-put-program failed: {e}"),
                        )
                    })?;
                    let desc = Term::Map(
                        [
                            (
                                TermOrdKey(Term::symbol(":cmd")),
                                Term::Str("store/put".to_string()),
                            ),
                            (
                                TermOrdKey(Term::symbol(":artifact-h")),
                                Term::Bytes(gc_coreform::hash_term(&art).to_vec().into()),
                            ),
                        ]
                        .into_iter()
                        .collect(),
                    );
                    (prog, "genesis/store-put-v0.2", "store-put", desc)
                }
                StoreCmd::Get { hash, .. } => {
                    let f = env.get("core/cli::store-get-program").ok_or_else(|| {
                        cli_err(
                            EX_INTERNAL,
                            "selfhost/missing",
                            "missing binding core/cli::store-get-program",
                        )
                    })?;
                    let prog = f
                        .apply(&mut ctx, Value::Data(Term::Str(hash.to_string())))
                        .map_err(|e| {
                            cli_err(
                                EX_EVAL,
                                "eval/error",
                                format!("core/cli store-get-program failed: {e}"),
                            )
                        })?;
                    let desc = Term::Map(
                        [
                            (
                                TermOrdKey(Term::symbol(":cmd")),
                                Term::Str("store/get".to_string()),
                            ),
                            (
                                TermOrdKey(Term::symbol(":hash")),
                                Term::Str(hash.to_string()),
                            ),
                        ]
                        .into_iter()
                        .collect(),
                    );
                    (prog, "genesis/store-get-v0.2", "store-get", desc)
                }
                StoreCmd::Has { hash } => {
                    let f = env.get("core/cli::store-has-program").ok_or_else(|| {
                        cli_err(
                            EX_INTERNAL,
                            "selfhost/missing",
                            "missing binding core/cli::store-has-program",
                        )
                    })?;
                    let prog = f
                        .apply(&mut ctx, Value::Data(Term::Str(hash.to_string())))
                        .map_err(|e| {
                            cli_err(
                                EX_EVAL,
                                "eval/error",
                                format!("core/cli store-has-program failed: {e}"),
                            )
                        })?;
                    let desc = Term::Map(
                        [
                            (
                                TermOrdKey(Term::symbol(":cmd")),
                                Term::Str("store/has".to_string()),
                            ),
                            (
                                TermOrdKey(Term::symbol(":hash")),
                                Term::Str(hash.to_string()),
                            ),
                        ]
                        .into_iter()
                        .collect(),
                    );
                    (prog, "genesis/store-has-v0.2", "store-has", desc)
                }
            };
            let program_hash = gc_coreform::hash_term(&desc);
            (prog, kind, log_op, program_hash)
        }
    };

    let toolchain = format!("genesis {}", env!("CARGO_PKG_VERSION"));
    let r = gc_effects::run(&mut ctx, &policy, prog, program_hash, toolchain)
        .map_err(|e| cli_err(EX_EVAL, "effects/run", format!("{e}")))?;

    let log_path = log
        .map(PathBuf::from)
        .unwrap_or_else(|| default_log_path(log_op));
    std::fs::write(&log_path, r.log.to_string_canonical() + "\n")
        .with_context(|| format!("write {}", log_path.display()))
        .map_err(|e| cli_err(EX_IO, "io/write", format!("{e}")))?;

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
                m.get(&gc_coreform::TermOrdKey(Term::symbol(":error/code"))),
                Some(Term::Str(s)) if s == "core/caps/denied"
            )
        {
            exit_code = EX_CAPS_DENIED;
        }
    }

    // Extract a stable stdout payload.
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
            StoreCmd::Get { out, .. } => {
                if !ok {
                    format!("{value}\n")
                } else if let Some(p) = out {
                    let art = extract_store_get_artifact(&r.value).ok_or_else(|| {
                        cli_err(
                            EX_INTERNAL,
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
            "coreform_frontend": frontend_info,
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

fn cmd_refs(cli: &Cli, caps: &Path, log: Option<&Path>, cmd: &RefsCmd) -> Result<CmdOut, CliError> {
    let frontend = resolved_coreform_frontend(cli)?;
    let frontend_info = coreform_frontend_json(&frontend);

    let policy = CapsPolicy::load(caps)
        .with_context(|| format!("read {}", caps.display()))
        .map_err(|e| cli_err(EX_PARSE, "caps/parse", format!("{e}")))?;

    let mut ctx = mk_ctx(cli);
    let prelude = build_prelude(&mut ctx);
    let mut env = prelude.env;

    let (prog, kind, log_op, program_hash) = match frontend {
        gc_obligations::CoreformFrontend::Rust => {
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
            let prog = eval_module(&mut ctx, &mut env, &forms)
                .map_err(|e| cli_err(EX_EVAL, "eval/error", format!("{e}")))?;
            (prog, kind, log_op, program_hash)
        }
        gc_obligations::CoreformFrontend::Selfhost(_) => {
            load_selfhost_toolchain(cli, &mut ctx, &mut env)?;

            let (prog, kind, log_op, desc) = match cmd {
                RefsCmd::Get { name } => {
                    let f = env.get("core/cli::refs-get-program").ok_or_else(|| {
                        cli_err(
                            EX_INTERNAL,
                            "selfhost/missing",
                            "missing binding core/cli::refs-get-program",
                        )
                    })?;
                    let prog = f
                        .apply(&mut ctx, Value::Data(Term::Str(name.to_string())))
                        .map_err(|e| {
                            cli_err(
                                EX_EVAL,
                                "eval/error",
                                format!("core/cli refs-get-program failed: {e}"),
                            )
                        })?;
                    let desc = Term::Map(
                        [
                            (
                                TermOrdKey(Term::symbol(":cmd")),
                                Term::Str("refs/get".to_string()),
                            ),
                            (
                                TermOrdKey(Term::symbol(":name")),
                                Term::Str(name.to_string()),
                            ),
                        ]
                        .into_iter()
                        .collect(),
                    );
                    (prog, "genesis/refs-get-v0.1", "refs-get", desc)
                }
                RefsCmd::List { prefix } => {
                    let f = env.get("core/cli::refs-list-program").ok_or_else(|| {
                        cli_err(
                            EX_INTERNAL,
                            "selfhost/missing",
                            "missing binding core/cli::refs-list-program",
                        )
                    })?;
                    let prefix_term = prefix
                        .as_deref()
                        .map(|s| Term::Str(s.to_string()))
                        .unwrap_or(Term::Nil);
                    let prog = f
                        .apply(&mut ctx, Value::Data(prefix_term.clone()))
                        .map_err(|e| {
                            cli_err(
                                EX_EVAL,
                                "eval/error",
                                format!("core/cli refs-list-program failed: {e}"),
                            )
                        })?;
                    let desc = Term::Map(
                        [
                            (
                                TermOrdKey(Term::symbol(":cmd")),
                                Term::Str("refs/list".to_string()),
                            ),
                            (TermOrdKey(Term::symbol(":prefix")), prefix_term),
                        ]
                        .into_iter()
                        .collect(),
                    );
                    (prog, "genesis/refs-list-v0.1", "refs-list", desc)
                }
                RefsCmd::Set {
                    name,
                    hash,
                    policy,
                    expected_old,
                } => {
                    let f = env.get("core/cli::refs-set-program").ok_or_else(|| {
                        cli_err(
                            EX_INTERNAL,
                            "selfhost/missing",
                            "missing binding core/cli::refs-set-program",
                        )
                    })?;

                    let (present, expected_old_term) = match expected_old.as_deref() {
                        None => (false, Term::Nil),
                        Some("nil") => (true, Term::Nil),
                        Some(s) => (true, Term::Str(s.to_string())),
                    };
                    let req = Term::Map(
                        [
                            (
                                TermOrdKey(Term::symbol(":name")),
                                Term::Str(name.to_string()),
                            ),
                            (
                                TermOrdKey(Term::symbol(":hash")),
                                Term::Str(hash.to_string()),
                            ),
                            (
                                TermOrdKey(Term::symbol(":policy")),
                                Term::Str(policy.to_string()),
                            ),
                            (
                                TermOrdKey(Term::symbol(":expected-old-present")),
                                Term::Bool(present),
                            ),
                            (TermOrdKey(Term::symbol(":expected-old")), expected_old_term),
                        ]
                        .into_iter()
                        .collect(),
                    );

                    let prog = f.apply(&mut ctx, Value::Data(req)).map_err(|e| {
                        cli_err(
                            EX_EVAL,
                            "eval/error",
                            format!("core/cli refs-set-program failed: {e}"),
                        )
                    })?;
                    let desc = Term::Map(
                        [
                            (
                                TermOrdKey(Term::symbol(":cmd")),
                                Term::Str("refs/set".to_string()),
                            ),
                            (
                                TermOrdKey(Term::symbol(":name")),
                                Term::Str(name.to_string()),
                            ),
                            (
                                TermOrdKey(Term::symbol(":hash")),
                                Term::Str(hash.to_string()),
                            ),
                            (
                                TermOrdKey(Term::symbol(":policy")),
                                Term::Str(policy.to_string()),
                            ),
                            (
                                TermOrdKey(Term::symbol(":expected-old")),
                                expected_old
                                    .as_deref()
                                    .map(|s| Term::Str(s.to_string()))
                                    .unwrap_or(Term::Nil),
                            ),
                        ]
                        .into_iter()
                        .collect(),
                    );
                    (prog, "genesis/refs-set-v0.1", "refs-set", desc)
                }
                RefsCmd::Delete {
                    name,
                    policy,
                    expected_old,
                } => {
                    let f = env.get("core/cli::refs-delete-program").ok_or_else(|| {
                        cli_err(
                            EX_INTERNAL,
                            "selfhost/missing",
                            "missing binding core/cli::refs-delete-program",
                        )
                    })?;

                    let (present, expected_old_term) = match expected_old.as_deref() {
                        None => (false, Term::Nil),
                        Some("nil") => (true, Term::Nil),
                        Some(s) => (true, Term::Str(s.to_string())),
                    };
                    let req = Term::Map(
                        [
                            (
                                TermOrdKey(Term::symbol(":name")),
                                Term::Str(name.to_string()),
                            ),
                            (
                                TermOrdKey(Term::symbol(":policy")),
                                Term::Str(policy.to_string()),
                            ),
                            (
                                TermOrdKey(Term::symbol(":expected-old-present")),
                                Term::Bool(present),
                            ),
                            (TermOrdKey(Term::symbol(":expected-old")), expected_old_term),
                        ]
                        .into_iter()
                        .collect(),
                    );

                    let prog = f.apply(&mut ctx, Value::Data(req)).map_err(|e| {
                        cli_err(
                            EX_EVAL,
                            "eval/error",
                            format!("core/cli refs-delete-program failed: {e}"),
                        )
                    })?;
                    let desc = Term::Map(
                        [
                            (
                                TermOrdKey(Term::symbol(":cmd")),
                                Term::Str("refs/delete".to_string()),
                            ),
                            (
                                TermOrdKey(Term::symbol(":name")),
                                Term::Str(name.to_string()),
                            ),
                            (
                                TermOrdKey(Term::symbol(":policy")),
                                Term::Str(policy.to_string()),
                            ),
                            (
                                TermOrdKey(Term::symbol(":expected-old")),
                                expected_old
                                    .as_deref()
                                    .map(|s| Term::Str(s.to_string()))
                                    .unwrap_or(Term::Nil),
                            ),
                        ]
                        .into_iter()
                        .collect(),
                    );
                    (prog, "genesis/refs-delete-v0.1", "refs-delete", desc)
                }
            };
            let program_hash = gc_coreform::hash_term(&desc);
            (prog, kind, log_op, program_hash)
        }
    };

    let toolchain = format!("genesis {}", env!("CARGO_PKG_VERSION"));
    let r = gc_effects::run(&mut ctx, &policy, prog, program_hash, toolchain)
        .map_err(|e| cli_err(EX_EVAL, "effects/run", format!("{e}")))?;

    let log_path = log
        .map(PathBuf::from)
        .unwrap_or_else(|| default_log_path(log_op));
    std::fs::write(&log_path, r.log.to_string_canonical() + "\n")
        .with_context(|| format!("write {}", log_path.display()))
        .map_err(|e| cli_err(EX_IO, "io/write", format!("{e}")))?;

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
                m.get(&gc_coreform::TermOrdKey(Term::symbol(":error/code"))),
                Some(Term::Str(s)) if s == "core/caps/denied"
            )
        {
            exit_code = EX_CAPS_DENIED;
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
            "coreform_frontend": frontend_info,
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

fn cmd_pkg(cli: &Cli, caps: &Path, log: Option<&Path>, cmd: &PkgCmd) -> Result<CmdOut, CliError> {
    let frontend = resolved_coreform_frontend(cli)?;
    let frontend_info = coreform_frontend_json(&frontend);

    let policy = CapsPolicy::load(caps)
        .with_context(|| format!("read {}", caps.display()))
        .map_err(|e| cli_err(EX_PARSE, "caps/parse", format!("{e}")))?;

    let mut ctx = mk_ctx(cli);
    let prelude = build_prelude(&mut ctx);
    let mut env = prelude.env;
    let (prog, kind, log_op, program_hash) = match frontend {
        gc_obligations::CoreformFrontend::Rust => {
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
                    let (name, selector) = parse_pkg_spec(spec)
                        .map_err(|e| cli_err(EX_PARSE, "pkg/spec", e.to_string()))?;
                    (
                        mk_pkg_add_program(
                            lock,
                            &name,
                            &selector,
                            update_policy,
                            registry.as_deref(),
                        ),
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
            let prog = eval_module(&mut ctx, &mut env, &forms)
                .map_err(|e| cli_err(EX_EVAL, "eval/error", format!("{e}")))?;
            Ok::<_, CliError>((prog, kind, log_op, program_hash))
        }
        gc_obligations::CoreformFrontend::Selfhost(_) => {
            load_selfhost_toolchain(cli, &mut ctx, &mut env)?;

            let (prog, kind, log_op, desc) = match cmd {
                PkgCmd::Init {
                    workspace,
                    lock,
                    policy,
                    registry_default,
                } => {
                    let f = env.get("core/cli::pkg-init-program").ok_or_else(|| {
                        cli_err(
                            EX_INTERNAL,
                            "selfhost/missing",
                            "missing binding core/cli::pkg-init-program",
                        )
                    })?;
                    let mut mm = std::collections::BTreeMap::new();
                    mm.insert(
                        TermOrdKey(Term::symbol(":workspace")),
                        Term::Str(workspace.to_string()),
                    );
                    mm.insert(
                        TermOrdKey(Term::symbol(":lock")),
                        Term::Str(lock.display().to_string()),
                    );
                    mm.insert(
                        TermOrdKey(Term::symbol(":policy")),
                        Term::Str(policy.to_string()),
                    );
                    mm.insert(
                        TermOrdKey(Term::symbol(":registry-default")),
                        registry_default
                            .as_deref()
                            .map(|s| Term::Str(s.to_string()))
                            .unwrap_or(Term::Nil),
                    );
                    let req = Term::Map(mm);
                    let prog = f.apply(&mut ctx, Value::Data(req)).map_err(|e| {
                        cli_err(
                            EX_EVAL,
                            "eval/error",
                            format!("core/cli pkg-init-program failed: {e}"),
                        )
                    })?;
                    let desc = Term::Map(
                        [
                            (
                                TermOrdKey(Term::symbol(":cmd")),
                                Term::Str("pkg/init".to_string()),
                            ),
                            (
                                TermOrdKey(Term::symbol(":workspace")),
                                Term::Str(workspace.to_string()),
                            ),
                            (
                                TermOrdKey(Term::symbol(":lock")),
                                Term::Str(lock.display().to_string()),
                            ),
                            (
                                TermOrdKey(Term::symbol(":policy")),
                                Term::Str(policy.to_string()),
                            ),
                        ]
                        .into_iter()
                        .collect(),
                    );
                    (prog, "genesis/pkg-init-v0.1", "pkg-init", desc)
                }
                PkgCmd::Add {
                    spec,
                    lock,
                    update_policy,
                    registry,
                } => {
                    let (name, selector) = parse_pkg_spec(spec)
                        .map_err(|e| cli_err(EX_PARSE, "pkg/spec", e.to_string()))?;
                    let f = env.get("core/cli::pkg-add-program").ok_or_else(|| {
                        cli_err(
                            EX_INTERNAL,
                            "selfhost/missing",
                            "missing binding core/cli::pkg-add-program",
                        )
                    })?;
                    let mut mm = std::collections::BTreeMap::new();
                    mm.insert(
                        TermOrdKey(Term::symbol(":lock")),
                        Term::Str(lock.display().to_string()),
                    );
                    mm.insert(TermOrdKey(Term::symbol(":name")), Term::Str(name.clone()));
                    mm.insert(
                        TermOrdKey(Term::symbol(":selector")),
                        Term::Str(selector.clone()),
                    );
                    mm.insert(
                        TermOrdKey(Term::symbol(":update-policy")),
                        Term::Str(update_policy.to_string()),
                    );
                    mm.insert(
                        TermOrdKey(Term::symbol(":registry")),
                        registry
                            .as_deref()
                            .map(|s| Term::Str(s.to_string()))
                            .unwrap_or(Term::Nil),
                    );
                    let req = Term::Map(mm);
                    let prog = f.apply(&mut ctx, Value::Data(req)).map_err(|e| {
                        cli_err(
                            EX_EVAL,
                            "eval/error",
                            format!("core/cli pkg-add-program failed: {e}"),
                        )
                    })?;
                    let desc = Term::Map(
                        [
                            (
                                TermOrdKey(Term::symbol(":cmd")),
                                Term::Str("pkg/add".to_string()),
                            ),
                            (
                                TermOrdKey(Term::symbol(":lock")),
                                Term::Str(lock.display().to_string()),
                            ),
                            (TermOrdKey(Term::symbol(":name")), Term::Str(name)),
                            (TermOrdKey(Term::symbol(":selector")), Term::Str(selector)),
                            (
                                TermOrdKey(Term::symbol(":update-policy")),
                                Term::Str(update_policy.to_string()),
                            ),
                        ]
                        .into_iter()
                        .collect(),
                    );
                    (prog, "genesis/pkg-add-v0.1", "pkg-add", desc)
                }
                PkgCmd::Lock { lock, strict } => {
                    let f = env.get("core/cli::pkg-lock-program").ok_or_else(|| {
                        cli_err(
                            EX_INTERNAL,
                            "selfhost/missing",
                            "missing binding core/cli::pkg-lock-program",
                        )
                    })?;
                    let req = Term::Map(
                        [
                            (
                                TermOrdKey(Term::symbol(":lock")),
                                Term::Str(lock.display().to_string()),
                            ),
                            (TermOrdKey(Term::symbol(":strict")), Term::Bool(*strict)),
                        ]
                        .into_iter()
                        .collect(),
                    );
                    let prog = f.apply(&mut ctx, Value::Data(req)).map_err(|e| {
                        cli_err(
                            EX_EVAL,
                            "eval/error",
                            format!("core/cli pkg-lock-program failed: {e}"),
                        )
                    })?;
                    let desc = Term::Map(
                        [
                            (
                                TermOrdKey(Term::symbol(":cmd")),
                                Term::Str("pkg/lock".to_string()),
                            ),
                            (
                                TermOrdKey(Term::symbol(":lock")),
                                Term::Str(lock.display().to_string()),
                            ),
                            (TermOrdKey(Term::symbol(":strict")), Term::Bool(*strict)),
                        ]
                        .into_iter()
                        .collect(),
                    );
                    (prog, "genesis/pkg-lock-v0.1", "pkg-lock", desc)
                }
                PkgCmd::Update { lock } => {
                    let f = env.get("core/cli::pkg-update-program").ok_or_else(|| {
                        cli_err(
                            EX_INTERNAL,
                            "selfhost/missing",
                            "missing binding core/cli::pkg-update-program",
                        )
                    })?;
                    let req = Term::Map(
                        [(
                            TermOrdKey(Term::symbol(":lock")),
                            Term::Str(lock.display().to_string()),
                        )]
                        .into_iter()
                        .collect(),
                    );
                    let prog = f.apply(&mut ctx, Value::Data(req)).map_err(|e| {
                        cli_err(
                            EX_EVAL,
                            "eval/error",
                            format!("core/cli pkg-update-program failed: {e}"),
                        )
                    })?;
                    let desc = Term::Map(
                        [
                            (
                                TermOrdKey(Term::symbol(":cmd")),
                                Term::Str("pkg/update".to_string()),
                            ),
                            (
                                TermOrdKey(Term::symbol(":lock")),
                                Term::Str(lock.display().to_string()),
                            ),
                        ]
                        .into_iter()
                        .collect(),
                    );
                    (prog, "genesis/pkg-update-v0.1", "pkg-update", desc)
                }
                PkgCmd::Install {
                    lock,
                    frozen,
                    strict,
                } => {
                    let f = env.get("core/cli::pkg-install-program").ok_or_else(|| {
                        cli_err(
                            EX_INTERNAL,
                            "selfhost/missing",
                            "missing binding core/cli::pkg-install-program",
                        )
                    })?;
                    let req = Term::Map(
                        [
                            (
                                TermOrdKey(Term::symbol(":lock")),
                                Term::Str(lock.display().to_string()),
                            ),
                            (TermOrdKey(Term::symbol(":frozen")), Term::Bool(*frozen)),
                            (TermOrdKey(Term::symbol(":strict")), Term::Bool(*strict)),
                        ]
                        .into_iter()
                        .collect(),
                    );
                    let prog = f.apply(&mut ctx, Value::Data(req)).map_err(|e| {
                        cli_err(
                            EX_EVAL,
                            "eval/error",
                            format!("core/cli pkg-install-program failed: {e}"),
                        )
                    })?;
                    let desc = Term::Map(
                        [
                            (
                                TermOrdKey(Term::symbol(":cmd")),
                                Term::Str("pkg/install".to_string()),
                            ),
                            (
                                TermOrdKey(Term::symbol(":lock")),
                                Term::Str(lock.display().to_string()),
                            ),
                            (TermOrdKey(Term::symbol(":frozen")), Term::Bool(*frozen)),
                            (TermOrdKey(Term::symbol(":strict")), Term::Bool(*strict)),
                        ]
                        .into_iter()
                        .collect(),
                    );
                    (prog, "genesis/pkg-install-v0.1", "pkg-install", desc)
                }
                PkgCmd::Verify { lock } => {
                    let f = env.get("core/cli::pkg-verify-program").ok_or_else(|| {
                        cli_err(
                            EX_INTERNAL,
                            "selfhost/missing",
                            "missing binding core/cli::pkg-verify-program",
                        )
                    })?;
                    let req = Term::Map(
                        [(
                            TermOrdKey(Term::symbol(":lock")),
                            Term::Str(lock.display().to_string()),
                        )]
                        .into_iter()
                        .collect(),
                    );
                    let prog = f.apply(&mut ctx, Value::Data(req)).map_err(|e| {
                        cli_err(
                            EX_EVAL,
                            "eval/error",
                            format!("core/cli pkg-verify-program failed: {e}"),
                        )
                    })?;
                    let desc = Term::Map(
                        [
                            (
                                TermOrdKey(Term::symbol(":cmd")),
                                Term::Str("pkg/verify".to_string()),
                            ),
                            (
                                TermOrdKey(Term::symbol(":lock")),
                                Term::Str(lock.display().to_string()),
                            ),
                        ]
                        .into_iter()
                        .collect(),
                    );
                    (prog, "genesis/pkg-verify-v0.1", "pkg-verify", desc)
                }
                PkgCmd::List { lock } => {
                    let f = env.get("core/cli::pkg-list-program").ok_or_else(|| {
                        cli_err(
                            EX_INTERNAL,
                            "selfhost/missing",
                            "missing binding core/cli::pkg-list-program",
                        )
                    })?;
                    let req = Term::Map(
                        [(
                            TermOrdKey(Term::symbol(":lock")),
                            Term::Str(lock.display().to_string()),
                        )]
                        .into_iter()
                        .collect(),
                    );
                    let prog = f.apply(&mut ctx, Value::Data(req)).map_err(|e| {
                        cli_err(
                            EX_EVAL,
                            "eval/error",
                            format!("core/cli pkg-list-program failed: {e}"),
                        )
                    })?;
                    let desc = Term::Map(
                        [
                            (
                                TermOrdKey(Term::symbol(":cmd")),
                                Term::Str("pkg/list".to_string()),
                            ),
                            (
                                TermOrdKey(Term::symbol(":lock")),
                                Term::Str(lock.display().to_string()),
                            ),
                        ]
                        .into_iter()
                        .collect(),
                    );
                    (prog, "genesis/pkg-list-v0.1", "pkg-list", desc)
                }
                PkgCmd::Info { name, lock } => {
                    let f = env.get("core/cli::pkg-info-program").ok_or_else(|| {
                        cli_err(
                            EX_INTERNAL,
                            "selfhost/missing",
                            "missing binding core/cli::pkg-info-program",
                        )
                    })?;
                    let req = Term::Map(
                        [
                            (
                                TermOrdKey(Term::symbol(":lock")),
                                Term::Str(lock.display().to_string()),
                            ),
                            (
                                TermOrdKey(Term::symbol(":name")),
                                Term::Str(name.to_string()),
                            ),
                        ]
                        .into_iter()
                        .collect(),
                    );
                    let prog = f.apply(&mut ctx, Value::Data(req)).map_err(|e| {
                        cli_err(
                            EX_EVAL,
                            "eval/error",
                            format!("core/cli pkg-info-program failed: {e}"),
                        )
                    })?;
                    let desc = Term::Map(
                        [
                            (
                                TermOrdKey(Term::symbol(":cmd")),
                                Term::Str("pkg/info".to_string()),
                            ),
                            (
                                TermOrdKey(Term::symbol(":lock")),
                                Term::Str(lock.display().to_string()),
                            ),
                            (
                                TermOrdKey(Term::symbol(":name")),
                                Term::Str(name.to_string()),
                            ),
                        ]
                        .into_iter()
                        .collect(),
                    );
                    (prog, "genesis/pkg-info-v0.1", "pkg-info", desc)
                }
                PkgCmd::Snapshot { pkg } => {
                    let f = env.get("core/cli::pkg-snapshot-program").ok_or_else(|| {
                        cli_err(
                            EX_INTERNAL,
                            "selfhost/missing",
                            "missing binding core/cli::pkg-snapshot-program",
                        )
                    })?;
                    let req = Term::Map(
                        [(
                            TermOrdKey(Term::symbol(":pkg")),
                            Term::Str(pkg.display().to_string()),
                        )]
                        .into_iter()
                        .collect(),
                    );
                    let prog = f.apply(&mut ctx, Value::Data(req)).map_err(|e| {
                        cli_err(
                            EX_EVAL,
                            "eval/error",
                            format!("core/cli pkg-snapshot-program failed: {e}"),
                        )
                    })?;
                    let desc = Term::Map(
                        [
                            (
                                TermOrdKey(Term::symbol(":cmd")),
                                Term::Str("pkg/snapshot".to_string()),
                            ),
                            (
                                TermOrdKey(Term::symbol(":pkg")),
                                Term::Str(pkg.display().to_string()),
                            ),
                        ]
                        .into_iter()
                        .collect(),
                    );
                    (prog, "genesis/pkg-snapshot-v0.1", "pkg-snapshot", desc)
                }
                PkgCmd::Export {
                    root,
                    out,
                    full,
                    depth,
                    include_evidence,
                    include_deps,
                    include_refs,
                } => {
                    let f = env.get("core/cli::pkg-export-program").ok_or_else(|| {
                        cli_err(
                            EX_INTERNAL,
                            "selfhost/missing",
                            "missing binding core/cli::pkg-export-program",
                        )
                    })?;
                    let req = Term::Map(
                        [
                            (
                                TermOrdKey(Term::symbol(":root")),
                                Term::Str(root.to_string()),
                            ),
                            (
                                TermOrdKey(Term::symbol(":out")),
                                Term::Str(out.display().to_string()),
                            ),
                            (
                                TermOrdKey(Term::symbol(":mode")),
                                Term::Str(if *full {
                                    ":full".to_string()
                                } else {
                                    ":shallow".to_string()
                                }),
                            ),
                            (
                                TermOrdKey(Term::symbol(":depth")),
                                Term::Int((*depth as i64).into()),
                            ),
                            (
                                TermOrdKey(Term::symbol(":include-evidence")),
                                Term::Str(include_evidence.to_string()),
                            ),
                            (
                                TermOrdKey(Term::symbol(":include-deps")),
                                Term::Str(include_deps.to_string()),
                            ),
                            (
                                TermOrdKey(Term::symbol(":refs")),
                                Term::Vector(include_refs.iter().cloned().map(Term::Str).collect()),
                            ),
                        ]
                        .into_iter()
                        .collect(),
                    );
                    let prog = f.apply(&mut ctx, Value::Data(req)).map_err(|e| {
                        cli_err(
                            EX_EVAL,
                            "eval/error",
                            format!("core/cli pkg-export-program failed: {e}"),
                        )
                    })?;
                    let desc = Term::Map(
                        [
                            (
                                TermOrdKey(Term::symbol(":cmd")),
                                Term::Str("pkg/export".to_string()),
                            ),
                            (
                                TermOrdKey(Term::symbol(":root")),
                                Term::Str(root.to_string()),
                            ),
                            (
                                TermOrdKey(Term::symbol(":out")),
                                Term::Str(out.display().to_string()),
                            ),
                            (TermOrdKey(Term::symbol(":full")), Term::Bool(*full)),
                            (
                                TermOrdKey(Term::symbol(":depth")),
                                Term::Int((*depth as i64).into()),
                            ),
                            (
                                TermOrdKey(Term::symbol(":include-evidence")),
                                Term::Str(include_evidence.to_string()),
                            ),
                            (
                                TermOrdKey(Term::symbol(":include-deps")),
                                Term::Str(include_deps.to_string()),
                            ),
                            (
                                TermOrdKey(Term::symbol(":refs")),
                                Term::Vector(include_refs.iter().cloned().map(Term::Str).collect()),
                            ),
                        ]
                        .into_iter()
                        .collect(),
                    );
                    (prog, "genesis/pkg-export-v0.1", "pkg-export", desc)
                }
                PkgCmd::Import {
                    input,
                    set_refs,
                    policy,
                } => {
                    let f = env.get("core/cli::pkg-import-program").ok_or_else(|| {
                        cli_err(
                            EX_INTERNAL,
                            "selfhost/missing",
                            "missing binding core/cli::pkg-import-program",
                        )
                    })?;
                    let parsed = parse_local_set_refs(set_refs, policy.as_deref())?;

                    let mut set_refs_term: Vec<Term> = Vec::new();
                    for sr in &parsed {
                        let mut mm = std::collections::BTreeMap::new();
                        mm.insert(
                            TermOrdKey(Term::symbol(":name")),
                            Term::Str(sr.name.clone()),
                        );
                        mm.insert(
                            TermOrdKey(Term::symbol(":hash")),
                            if sr.hash == "nil" {
                                Term::Nil
                            } else {
                                Term::Str(sr.hash.clone())
                            },
                        );
                        mm.insert(
                            TermOrdKey(Term::symbol(":policy")),
                            Term::Str(sr.policy.clone()),
                        );
                        if let Some(exp) = &sr.expected_old {
                            mm.insert(
                                TermOrdKey(Term::symbol(":expected-old")),
                                if exp == "nil" {
                                    Term::Nil
                                } else {
                                    Term::Str(exp.clone())
                                },
                            );
                        }
                        set_refs_term.push(Term::Map(mm));
                    }

                    let req = Term::Map(
                        [
                            (
                                TermOrdKey(Term::symbol(":in")),
                                Term::Str(input.display().to_string()),
                            ),
                            (
                                TermOrdKey(Term::symbol(":set-refs")),
                                Term::Vector(set_refs_term),
                            ),
                        ]
                        .into_iter()
                        .collect(),
                    );
                    let prog = f.apply(&mut ctx, Value::Data(req)).map_err(|e| {
                        cli_err(
                            EX_EVAL,
                            "eval/error",
                            format!("core/cli pkg-import-program failed: {e}"),
                        )
                    })?;
                    let desc = Term::Map(
                        [
                            (
                                TermOrdKey(Term::symbol(":cmd")),
                                Term::Str("pkg/import".to_string()),
                            ),
                            (
                                TermOrdKey(Term::symbol(":in")),
                                Term::Str(input.display().to_string()),
                            ),
                            (
                                TermOrdKey(Term::symbol(":set-refs-len")),
                                Term::Int((parsed.len() as i64).into()),
                            ),
                        ]
                        .into_iter()
                        .collect(),
                    );
                    (prog, "genesis/pkg-import-v0.1", "pkg-import", desc)
                }
                PkgCmd::Publish {
                    remote,
                    refname,
                    policy: policy_h,
                    expected_old,
                    depth,
                    commit,
                } => {
                    let f = env.get("core/cli::pkg-publish-program").ok_or_else(|| {
                        cli_err(
                            EX_INTERNAL,
                            "selfhost/missing",
                            "missing binding core/cli::pkg-publish-program",
                        )
                    })?;
                    let (present, expected) = match expected_old.as_deref() {
                        None => (false, Term::Nil),
                        Some(e) => {
                            if e == "nil" {
                                (true, Term::Nil)
                            } else {
                                (true, Term::Str(e.to_string()))
                            }
                        }
                    };
                    let req = Term::Map(
                        [
                            (
                                TermOrdKey(Term::symbol(":remote")),
                                Term::Str(remote.to_string()),
                            ),
                            (
                                TermOrdKey(Term::symbol(":ref")),
                                Term::Str(refname.to_string()),
                            ),
                            (
                                TermOrdKey(Term::symbol(":policy")),
                                Term::Str(policy_h.to_string()),
                            ),
                            (
                                TermOrdKey(Term::symbol(":expected-old-present")),
                                Term::Bool(present),
                            ),
                            (TermOrdKey(Term::symbol(":expected-old")), expected),
                            (
                                TermOrdKey(Term::symbol(":depth")),
                                Term::Int((*depth as i64).into()),
                            ),
                            (
                                TermOrdKey(Term::symbol(":commit")),
                                commit
                                    .as_deref()
                                    .map(|s| Term::Str(s.to_string()))
                                    .unwrap_or(Term::Nil),
                            ),
                        ]
                        .into_iter()
                        .collect(),
                    );
                    let prog = f.apply(&mut ctx, Value::Data(req)).map_err(|e| {
                        cli_err(
                            EX_EVAL,
                            "eval/error",
                            format!("core/cli pkg-publish-program failed: {e}"),
                        )
                    })?;
                    let desc = Term::Map(
                        [
                            (
                                TermOrdKey(Term::symbol(":cmd")),
                                Term::Str("pkg/publish".to_string()),
                            ),
                            (
                                TermOrdKey(Term::symbol(":remote")),
                                Term::Str(remote.to_string()),
                            ),
                            (
                                TermOrdKey(Term::symbol(":ref")),
                                Term::Str(refname.to_string()),
                            ),
                            (
                                TermOrdKey(Term::symbol(":policy")),
                                Term::Str(policy_h.to_string()),
                            ),
                            (
                                TermOrdKey(Term::symbol(":expected-old-present")),
                                Term::Bool(present),
                            ),
                            (
                                TermOrdKey(Term::symbol(":depth")),
                                Term::Int((*depth as i64).into()),
                            ),
                            (
                                TermOrdKey(Term::symbol(":commit-present")),
                                Term::Bool(commit.is_some()),
                            ),
                        ]
                        .into_iter()
                        .collect(),
                    );
                    (prog, "genesis/pkg-publish-v0.1", "pkg-publish", desc)
                }
            };
            let program_hash = gc_coreform::hash_term(&desc);
            Ok((prog, kind, log_op, program_hash))
        }
    }?;

    let toolchain = format!("genesis {}", env!("CARGO_PKG_VERSION"));
    let r = gc_effects::run(&mut ctx, &policy, prog, program_hash, toolchain)
        .map_err(|e| cli_err(EX_EVAL, "effects/run", format!("{e}")))?;

    let log_path = log
        .map(PathBuf::from)
        .unwrap_or_else(|| default_log_path(log_op));
    std::fs::write(&log_path, r.log.to_string_canonical() + "\n")
        .with_context(|| format!("write {}", log_path.display()))
        .map_err(|e| cli_err(EX_IO, "io/write", format!("{e}")))?;

    let mut ok = true;
    let mut exit_code = EX_OK;
    if let Some(proto) = ctx.protocol
        && let Value::Sealed { token, payload } = &r.value
        && *token == proto.error
    {
        ok = false;
        exit_code = EX_EVAL;
        if let Value::Data(Term::Map(m)) = payload.as_ref()
            && let Some(Term::Str(code)) =
                m.get(&gc_coreform::TermOrdKey(Term::symbol(":error/code")))
        {
            if code == "core/caps/denied" {
                exit_code = EX_CAPS_DENIED;
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
            "coreform_frontend": frontend_info,
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

fn cmd_sync(cli: &Cli, caps: &Path, log: Option<&Path>, cmd: &SyncCmd) -> Result<CmdOut, CliError> {
    let frontend = resolved_coreform_frontend(cli)?;
    let frontend_info = coreform_frontend_json(&frontend);

    let policy = CapsPolicy::load(caps)
        .with_context(|| format!("read {}", caps.display()))
        .map_err(|e| cli_err(EX_PARSE, "caps/parse", format!("{e}")))?;

    let mut ctx = mk_ctx(cli);
    let prelude = build_prelude(&mut ctx);
    let mut env = prelude.env;

    let (prog, kind, log_op, program_hash) = match frontend {
        gc_obligations::CoreformFrontend::Rust => {
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
            let prog = eval_module(&mut ctx, &mut env, &forms)
                .map_err(|e| cli_err(EX_EVAL, "eval/error", format!("{e}")))?;
            (prog, kind, log_op, program_hash)
        }
        gc_obligations::CoreformFrontend::Selfhost(_) => {
            load_selfhost_toolchain(cli, &mut ctx, &mut env)?;

            let (prog, kind, log_op, desc) = match cmd {
                SyncCmd::Pull {
                    remote,
                    refs,
                    roots,
                    depth,
                    force,
                } => {
                    let f = env.get("core/cli::sync-pull-program").ok_or_else(|| {
                        cli_err(
                            EX_INTERNAL,
                            "selfhost/missing",
                            "missing binding core/cli::sync-pull-program",
                        )
                    })?;
                    let req = Term::Map(
                        [
                            (
                                TermOrdKey(Term::symbol(":remote")),
                                Term::Str(remote.to_string()),
                            ),
                            (
                                TermOrdKey(Term::symbol(":refs")),
                                Term::Vector(refs.iter().cloned().map(Term::Str).collect()),
                            ),
                            (
                                TermOrdKey(Term::symbol(":roots")),
                                Term::Vector(roots.iter().cloned().map(Term::Str).collect()),
                            ),
                            (
                                TermOrdKey(Term::symbol(":depth")),
                                Term::Int((*depth as i64).into()),
                            ),
                            (TermOrdKey(Term::symbol(":force")), Term::Bool(*force)),
                        ]
                        .into_iter()
                        .collect(),
                    );
                    let prog = f.apply(&mut ctx, Value::Data(req)).map_err(|e| {
                        cli_err(
                            EX_EVAL,
                            "eval/error",
                            format!("core/cli sync-pull-program failed: {e}"),
                        )
                    })?;
                    let desc = Term::Map(
                        [
                            (
                                TermOrdKey(Term::symbol(":cmd")),
                                Term::Str("sync/pull".to_string()),
                            ),
                            (
                                TermOrdKey(Term::symbol(":remote")),
                                Term::Str(remote.to_string()),
                            ),
                            (
                                TermOrdKey(Term::symbol(":refs")),
                                Term::Vector(refs.iter().cloned().map(Term::Str).collect()),
                            ),
                            (
                                TermOrdKey(Term::symbol(":roots")),
                                Term::Vector(roots.iter().cloned().map(Term::Str).collect()),
                            ),
                            (
                                TermOrdKey(Term::symbol(":depth")),
                                Term::Int((*depth as i64).into()),
                            ),
                            (TermOrdKey(Term::symbol(":force")), Term::Bool(*force)),
                        ]
                        .into_iter()
                        .collect(),
                    );
                    (prog, "genesis/sync-pull-v0.1", "sync-pull", desc)
                }
                SyncCmd::Push {
                    remote,
                    roots,
                    depth,
                    set_refs,
                } => {
                    let f = env.get("core/cli::sync-push-program").ok_or_else(|| {
                        cli_err(
                            EX_INTERNAL,
                            "selfhost/missing",
                            "missing binding core/cli::sync-push-program",
                        )
                    })?;
                    let parsed = parse_sync_set_refs(set_refs)?;

                    let mut set_refs_term: Vec<Term> = Vec::new();
                    for sr in &parsed {
                        let mut mm = std::collections::BTreeMap::new();
                        mm.insert(
                            TermOrdKey(Term::symbol(":name")),
                            Term::Str(sr.name.clone()),
                        );
                        mm.insert(
                            TermOrdKey(Term::symbol(":hash")),
                            Term::Str(sr.hash.clone()),
                        );
                        mm.insert(
                            TermOrdKey(Term::symbol(":policy")),
                            Term::Str(sr.policy.clone()),
                        );
                        if let Some(e) = &sr.expected_old {
                            let v = if e == "nil" {
                                Term::Nil
                            } else {
                                Term::Str(e.clone())
                            };
                            mm.insert(TermOrdKey(Term::symbol(":expected-old")), v);
                        }
                        set_refs_term.push(Term::Map(mm));
                    }

                    let req = Term::Map(
                        [
                            (
                                TermOrdKey(Term::symbol(":remote")),
                                Term::Str(remote.to_string()),
                            ),
                            (
                                TermOrdKey(Term::symbol(":roots")),
                                Term::Vector(roots.iter().cloned().map(Term::Str).collect()),
                            ),
                            (
                                TermOrdKey(Term::symbol(":depth")),
                                Term::Int((*depth as i64).into()),
                            ),
                            (
                                TermOrdKey(Term::symbol(":set-refs")),
                                Term::Vector(set_refs_term),
                            ),
                        ]
                        .into_iter()
                        .collect(),
                    );
                    let prog = f.apply(&mut ctx, Value::Data(req)).map_err(|e| {
                        cli_err(
                            EX_EVAL,
                            "eval/error",
                            format!("core/cli sync-push-program failed: {e}"),
                        )
                    })?;
                    let desc = Term::Map(
                        [
                            (
                                TermOrdKey(Term::symbol(":cmd")),
                                Term::Str("sync/push".to_string()),
                            ),
                            (
                                TermOrdKey(Term::symbol(":remote")),
                                Term::Str(remote.to_string()),
                            ),
                            (
                                TermOrdKey(Term::symbol(":roots")),
                                Term::Vector(roots.iter().cloned().map(Term::Str).collect()),
                            ),
                            (
                                TermOrdKey(Term::symbol(":depth")),
                                Term::Int((*depth as i64).into()),
                            ),
                            (
                                TermOrdKey(Term::symbol(":set-refs-len")),
                                Term::Int((parsed.len() as i64).into()),
                            ),
                        ]
                        .into_iter()
                        .collect(),
                    );
                    (prog, "genesis/sync-push-v0.1", "sync-push", desc)
                }
            };
            let program_hash = gc_coreform::hash_term(&desc);
            (prog, kind, log_op, program_hash)
        }
    };

    let toolchain = format!("genesis {}", env!("CARGO_PKG_VERSION"));
    let r = gc_effects::run(&mut ctx, &policy, prog, program_hash, toolchain)
        .map_err(|e| cli_err(EX_EVAL, "effects/run", format!("{e}")))?;

    let log_path = log
        .map(PathBuf::from)
        .unwrap_or_else(|| default_log_path(log_op));
    std::fs::write(&log_path, r.log.to_string_canonical() + "\n")
        .with_context(|| format!("write {}", log_path.display()))
        .map_err(|e| cli_err(EX_IO, "io/write", format!("{e}")))?;

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
                m.get(&gc_coreform::TermOrdKey(Term::symbol(":error/code"))),
                Some(Term::Str(s)) if s == "core/caps/denied"
            )
        {
            exit_code = EX_CAPS_DENIED;
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
            "coreform_frontend": frontend_info,
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

fn cmd_gc(cli: &Cli, caps: &Path, log: Option<&Path>, cmd: &GcCmd) -> Result<CmdOut, CliError> {
    let frontend = resolved_coreform_frontend(cli)?;
    let frontend_info = coreform_frontend_json(&frontend);

    let policy = CapsPolicy::load(caps)
        .with_context(|| format!("read {}", caps.display()))
        .map_err(|e| cli_err(EX_PARSE, "caps/parse", format!("{e}")))?;

    let mut ctx = mk_ctx(cli);
    let prelude = build_prelude(&mut ctx);
    let mut env = prelude.env;

    let (prog, kind, log_op, program_hash) = match frontend {
        gc_obligations::CoreformFrontend::Rust => {
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
            let prog = eval_module(&mut ctx, &mut env, &forms)
                .map_err(|e| cli_err(EX_EVAL, "eval/error", format!("{e}")))?;
            (prog, kind, log_op, program_hash)
        }
        gc_obligations::CoreformFrontend::Selfhost(_) => {
            load_selfhost_toolchain(cli, &mut ctx, &mut env)?;

            let (prog, kind, log_op, desc) = match cmd {
                GcCmd::Plan {
                    lock,
                    pins,
                    depth,
                    no_lock,
                    no_refs,
                } => {
                    let f = env.get("core/cli::gc-plan-program").ok_or_else(|| {
                        cli_err(
                            EX_INTERNAL,
                            "selfhost/missing",
                            "missing binding core/cli::gc-plan-program",
                        )
                    })?;
                    let req = Term::Map(
                        [
                            (
                                TermOrdKey(Term::symbol(":lock")),
                                Term::Str(lock.display().to_string()),
                            ),
                            (
                                TermOrdKey(Term::symbol(":pins")),
                                Term::Str(pins.display().to_string()),
                            ),
                            (
                                TermOrdKey(Term::symbol(":depth")),
                                Term::Int((*depth as i64).into()),
                            ),
                            (
                                TermOrdKey(Term::symbol(":include-lock")),
                                Term::Bool(!*no_lock),
                            ),
                            (
                                TermOrdKey(Term::symbol(":include-refs")),
                                Term::Bool(!*no_refs),
                            ),
                        ]
                        .into_iter()
                        .collect(),
                    );
                    let prog = f.apply(&mut ctx, Value::Data(req)).map_err(|e| {
                        cli_err(
                            EX_EVAL,
                            "eval/error",
                            format!("core/cli gc-plan-program failed: {e}"),
                        )
                    })?;
                    let desc = Term::Map(
                        [
                            (
                                TermOrdKey(Term::symbol(":cmd")),
                                Term::Str("gc/plan".to_string()),
                            ),
                            (
                                TermOrdKey(Term::symbol(":lock")),
                                Term::Str(lock.display().to_string()),
                            ),
                            (
                                TermOrdKey(Term::symbol(":pins")),
                                Term::Str(pins.display().to_string()),
                            ),
                            (
                                TermOrdKey(Term::symbol(":depth")),
                                Term::Int((*depth as i64).into()),
                            ),
                            (
                                TermOrdKey(Term::symbol(":include-lock")),
                                Term::Bool(!*no_lock),
                            ),
                            (
                                TermOrdKey(Term::symbol(":include-refs")),
                                Term::Bool(!*no_refs),
                            ),
                        ]
                        .into_iter()
                        .collect(),
                    );
                    (prog, "genesis/gc-plan-v0.1", "gc-plan", desc)
                }
                GcCmd::Run {
                    lock,
                    pins,
                    depth,
                    no_lock,
                    no_refs,
                    quarantine,
                    quarantine_dir,
                } => {
                    let f = env.get("core/cli::gc-run-program").ok_or_else(|| {
                        cli_err(
                            EX_INTERNAL,
                            "selfhost/missing",
                            "missing binding core/cli::gc-run-program",
                        )
                    })?;
                    let req = Term::Map(
                        [
                            (
                                TermOrdKey(Term::symbol(":lock")),
                                Term::Str(lock.display().to_string()),
                            ),
                            (
                                TermOrdKey(Term::symbol(":pins")),
                                Term::Str(pins.display().to_string()),
                            ),
                            (
                                TermOrdKey(Term::symbol(":depth")),
                                Term::Int((*depth as i64).into()),
                            ),
                            (
                                TermOrdKey(Term::symbol(":include-lock")),
                                Term::Bool(!*no_lock),
                            ),
                            (
                                TermOrdKey(Term::symbol(":include-refs")),
                                Term::Bool(!*no_refs),
                            ),
                            (
                                TermOrdKey(Term::symbol(":quarantine")),
                                Term::Bool(*quarantine),
                            ),
                            (
                                TermOrdKey(Term::symbol(":quarantine-dir")),
                                quarantine_dir
                                    .as_deref()
                                    .map(|p| Term::Str(p.display().to_string()))
                                    .unwrap_or(Term::Nil),
                            ),
                        ]
                        .into_iter()
                        .collect(),
                    );
                    let prog = f.apply(&mut ctx, Value::Data(req)).map_err(|e| {
                        cli_err(
                            EX_EVAL,
                            "eval/error",
                            format!("core/cli gc-run-program failed: {e}"),
                        )
                    })?;
                    let desc = Term::Map(
                        [
                            (
                                TermOrdKey(Term::symbol(":cmd")),
                                Term::Str("gc/run".to_string()),
                            ),
                            (
                                TermOrdKey(Term::symbol(":lock")),
                                Term::Str(lock.display().to_string()),
                            ),
                            (
                                TermOrdKey(Term::symbol(":pins")),
                                Term::Str(pins.display().to_string()),
                            ),
                            (
                                TermOrdKey(Term::symbol(":depth")),
                                Term::Int((*depth as i64).into()),
                            ),
                            (
                                TermOrdKey(Term::symbol(":include-lock")),
                                Term::Bool(!*no_lock),
                            ),
                            (
                                TermOrdKey(Term::symbol(":include-refs")),
                                Term::Bool(!*no_refs),
                            ),
                            (
                                TermOrdKey(Term::symbol(":quarantine")),
                                Term::Bool(*quarantine),
                            ),
                            (
                                TermOrdKey(Term::symbol(":quarantine-dir")),
                                quarantine_dir
                                    .as_deref()
                                    .map(|p| Term::Str(p.display().to_string()))
                                    .unwrap_or(Term::Nil),
                            ),
                        ]
                        .into_iter()
                        .collect(),
                    );
                    (prog, "genesis/gc-run-v0.1", "gc-run", desc)
                }
                GcCmd::Pin { target, pins } => {
                    let f = env.get("core/cli::gc-pin-program").ok_or_else(|| {
                        cli_err(
                            EX_INTERNAL,
                            "selfhost/missing",
                            "missing binding core/cli::gc-pin-program",
                        )
                    })?;
                    let req = Term::Map(
                        [
                            (
                                TermOrdKey(Term::symbol(":target")),
                                Term::Str(target.to_string()),
                            ),
                            (
                                TermOrdKey(Term::symbol(":pins")),
                                Term::Str(pins.display().to_string()),
                            ),
                        ]
                        .into_iter()
                        .collect(),
                    );
                    let prog = f.apply(&mut ctx, Value::Data(req)).map_err(|e| {
                        cli_err(
                            EX_EVAL,
                            "eval/error",
                            format!("core/cli gc-pin-program failed: {e}"),
                        )
                    })?;
                    let desc = Term::Map(
                        [
                            (
                                TermOrdKey(Term::symbol(":cmd")),
                                Term::Str("gc/pin".to_string()),
                            ),
                            (
                                TermOrdKey(Term::symbol(":target")),
                                Term::Str(target.to_string()),
                            ),
                        ]
                        .into_iter()
                        .collect(),
                    );
                    (prog, "genesis/gc-pin-v0.1", "gc-pin", desc)
                }
                GcCmd::Unpin { target, pins } => {
                    let f = env.get("core/cli::gc-unpin-program").ok_or_else(|| {
                        cli_err(
                            EX_INTERNAL,
                            "selfhost/missing",
                            "missing binding core/cli::gc-unpin-program",
                        )
                    })?;
                    let req = Term::Map(
                        [
                            (
                                TermOrdKey(Term::symbol(":target")),
                                Term::Str(target.to_string()),
                            ),
                            (
                                TermOrdKey(Term::symbol(":pins")),
                                Term::Str(pins.display().to_string()),
                            ),
                        ]
                        .into_iter()
                        .collect(),
                    );
                    let prog = f.apply(&mut ctx, Value::Data(req)).map_err(|e| {
                        cli_err(
                            EX_EVAL,
                            "eval/error",
                            format!("core/cli gc-unpin-program failed: {e}"),
                        )
                    })?;
                    let desc = Term::Map(
                        [
                            (
                                TermOrdKey(Term::symbol(":cmd")),
                                Term::Str("gc/unpin".to_string()),
                            ),
                            (
                                TermOrdKey(Term::symbol(":target")),
                                Term::Str(target.to_string()),
                            ),
                        ]
                        .into_iter()
                        .collect(),
                    );
                    (prog, "genesis/gc-unpin-v0.1", "gc-unpin", desc)
                }
                GcCmd::Purge {
                    ttl_days,
                    quarantine_dir,
                } => {
                    let f = env.get("core/cli::gc-purge-program").ok_or_else(|| {
                        cli_err(
                            EX_INTERNAL,
                            "selfhost/missing",
                            "missing binding core/cli::gc-purge-program",
                        )
                    })?;
                    let req = Term::Map(
                        [
                            (
                                TermOrdKey(Term::symbol(":ttl-days")),
                                Term::Int((*ttl_days as i64).into()),
                            ),
                            (
                                TermOrdKey(Term::symbol(":quarantine-dir")),
                                quarantine_dir
                                    .as_deref()
                                    .map(|p| Term::Str(p.display().to_string()))
                                    .unwrap_or(Term::Nil),
                            ),
                        ]
                        .into_iter()
                        .collect(),
                    );
                    let prog = f.apply(&mut ctx, Value::Data(req)).map_err(|e| {
                        cli_err(
                            EX_EVAL,
                            "eval/error",
                            format!("core/cli gc-purge-program failed: {e}"),
                        )
                    })?;
                    let desc = Term::Map(
                        [
                            (
                                TermOrdKey(Term::symbol(":cmd")),
                                Term::Str("gc/purge".to_string()),
                            ),
                            (
                                TermOrdKey(Term::symbol(":ttl-days")),
                                Term::Int((*ttl_days as i64).into()),
                            ),
                        ]
                        .into_iter()
                        .collect(),
                    );
                    (prog, "genesis/gc-purge-v0.1", "gc-purge", desc)
                }
            };
            let program_hash = gc_coreform::hash_term(&desc);
            (prog, kind, log_op, program_hash)
        }
    };

    let toolchain = format!("genesis {}", env!("CARGO_PKG_VERSION"));
    let r = gc_effects::run(&mut ctx, &policy, prog, program_hash, toolchain)
        .map_err(|e| cli_err(EX_EVAL, "effects/run", format!("{e}")))?;

    let log_path = log
        .map(PathBuf::from)
        .unwrap_or_else(|| default_log_path(log_op));
    std::fs::write(&log_path, r.log.to_string_canonical() + "\n")
        .with_context(|| format!("write {}", log_path.display()))
        .map_err(|e| cli_err(EX_IO, "io/write", format!("{e}")))?;

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
                m.get(&gc_coreform::TermOrdKey(Term::symbol(":error/code"))),
                Some(Term::Str(s)) if s == "core/caps/denied"
            )
        {
            exit_code = EX_CAPS_DENIED;
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
            "coreform_frontend": frontend_info,
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

fn cmd_vcs(
    cli: &Cli,
    caps: Option<&Path>,
    log: Option<&Path>,
    cmd: &VcsCmd,
) -> Result<CmdOut, CliError> {
    if let VcsCmd::Hash { input, engine } = cmd {
        let engine = resolved_engine(cli, "vcs hash", *engine)?;
        let src = std::fs::read_to_string(input)
            .with_context(|| format!("read {}", input.display()))
            .map_err(|e| cli_err(EX_IO, "io/read", format!("{e}")))?;
        let (hash_hex, hk) = if engine == FmtEngine::Selfhost {
            let mut ctx = EvalCtx::with_step_limit(None);
            ctx.set_mem_limits(resolved_mem_limits(cli));
            let prelude = build_prelude(&mut ctx);
            let mut env = prelude.env;
            load_selfhost_toolchain(cli, &mut ctx, &mut env)?;

            let f = env.get("core/cli::hash-src-with-kind").ok_or_else(|| {
                cli_err(
                    EX_INTERNAL,
                    "selfhost/missing",
                    "missing binding core/cli::hash-src-with-kind",
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
            let (hash_hex, hk) = match r {
                Value::Data(Term::Map(m)) => {
                    let hash_hex = match m.get(&TermOrdKey(Term::symbol(":hash"))) {
                        Some(Term::Str(s)) => s.clone(),
                        _ => {
                            return Err(cli_err(
                                EX_INTERNAL,
                                "selfhost/bad-return",
                                "selfhost vcs hash return missing :hash string",
                            ));
                        }
                    };
                    let hk = match m.get(&TermOrdKey(Term::symbol(":kind"))) {
                        Some(Term::Str(s)) if s == "term" || s == "module" => s.clone(),
                        _ => {
                            return Err(cli_err(
                                EX_INTERNAL,
                                "selfhost/bad-return",
                                "selfhost vcs hash return missing :kind string",
                            ));
                        }
                    };
                    (hash_hex, hk)
                }
                Value::Map(m) => {
                    let hash_hex = match m.get(&TermOrdKey(Term::symbol(":hash"))) {
                        Some(Value::Data(Term::Str(s))) => s.clone(),
                        _ => {
                            return Err(cli_err(
                                EX_INTERNAL,
                                "selfhost/bad-return",
                                "selfhost vcs hash return missing :hash string",
                            ));
                        }
                    };
                    let hk = match m.get(&TermOrdKey(Term::symbol(":kind"))) {
                        Some(Value::Data(Term::Str(s))) if s == "term" || s == "module" => {
                            s.clone()
                        }
                        _ => {
                            return Err(cli_err(
                                EX_INTERNAL,
                                "selfhost/bad-return",
                                "selfhost vcs hash return missing :kind string",
                            ));
                        }
                    };
                    (hash_hex, hk)
                }
                _ => {
                    return Err(cli_err(
                        EX_INTERNAL,
                        "selfhost/bad-return",
                        format!("selfhost vcs hash returned non-map: {}", r.debug_repr()),
                    ));
                }
            };
            (hash_hex, hk)
        } else {
            let (h, hk) = match parse_term(&src) {
                Ok(t) => (gc_coreform::hash_term(&t), "term"),
                Err(_) => {
                    let forms = parse_module(&src)
                        .map_err(|e| cli_err(EX_PARSE, "parse/coreform", e.to_string()))?;
                    let forms = canonicalize_module(forms)
                        .map_err(|e| cli_err(EX_PARSE, "canon/coreform", e.to_string()))?;
                    (hash_module(&forms), "module")
                }
            };
            (gc_vcs::bytes32_to_hex(&h), hk.to_string())
        };

        let env = JsonEnvelope {
            ok: true,
            kind: "genesis/vcs-hash-v0.2",
            data: Some(serde_json::json!({
                "in": input.display().to_string(),
                // Keep legacy field for backward-compat while standardizing on `in`.
                "input": input.display().to_string(),
                "hash": hash_hex,
                "hash_kind": hk,
                "hash_format": "hex",
                "engine": if engine == FmtEngine::Selfhost { "selfhost" } else { "rust" },
            })),
            error: None,
        };
        return Ok(CmdOut {
            exit_code: EX_OK,
            stdout: if cli.json {
                String::new()
            } else {
                format!("{hash_hex}\n")
            },
            json: serde_json::to_value(env).expect("json"),
        });
    }

    let frontend = resolved_coreform_frontend(cli)?;
    let frontend_info = coreform_frontend_json(&frontend);

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

    let mut ctx = mk_ctx(cli);
    let prelude = build_prelude(&mut ctx);
    let mut env = prelude.env;
    let (prog, kind, log_op, program_hash) = match frontend {
        gc_obligations::CoreformFrontend::Rust => {
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
            let prog = eval_module(&mut ctx, &mut env, &forms)
                .map_err(|e| cli_err(EX_EVAL, "eval/error", format!("{e}")))?;
            Ok::<_, CliError>((prog, kind, log_op, program_hash))
        }
        gc_obligations::CoreformFrontend::Selfhost(_) => {
            load_selfhost_toolchain(cli, &mut ctx, &mut env)?;

            let (prog, kind, log_op, desc) = match cmd {
                VcsCmd::Hash { .. } => unreachable!("handled above"),
                VcsCmd::Log { root, max } => {
                    let f = env.get("core/cli::vcs-log-program").ok_or_else(|| {
                        cli_err(
                            EX_INTERNAL,
                            "selfhost/missing",
                            "missing binding core/cli::vcs-log-program",
                        )
                    })?;
                    let req = Term::Map(
                        [
                            (
                                TermOrdKey(Term::symbol(":root")),
                                Term::Str(root.to_string()),
                            ),
                            (
                                TermOrdKey(Term::symbol(":max")),
                                Term::Int((*max as i64).into()),
                            ),
                        ]
                        .into_iter()
                        .collect(),
                    );
                    let prog = f.apply(&mut ctx, Value::Data(req)).map_err(|e| {
                        cli_err(
                            EX_EVAL,
                            "eval/error",
                            format!("core/cli vcs-log-program failed: {e}"),
                        )
                    })?;
                    let desc = Term::Map(
                        [
                            (
                                TermOrdKey(Term::symbol(":cmd")),
                                Term::Str("vcs/log".to_string()),
                            ),
                            (
                                TermOrdKey(Term::symbol(":root")),
                                Term::Str(root.to_string()),
                            ),
                            (
                                TermOrdKey(Term::symbol(":max")),
                                Term::Int((*max as i64).into()),
                            ),
                        ]
                        .into_iter()
                        .collect(),
                    );
                    (prog, "genesis/vcs-log-v0.1", "vcs-log", desc)
                }
                VcsCmd::Blame {
                    snapshot,
                    sym,
                    path,
                } => {
                    gc_vcs::validate_hex_hash(snapshot).map_err(|e| {
                        cli_err(EX_PARSE, "vcs/blame", format!("invalid --snapshot: {e}"))
                    })?;
                    if sym.trim().is_empty() {
                        return Err(cli_err(EX_PARSE, "vcs/blame", "invalid --sym: empty value"));
                    }

                    let f = env.get("core/cli::vcs-blame-program").ok_or_else(|| {
                        cli_err(
                            EX_INTERNAL,
                            "selfhost/missing",
                            "missing binding core/cli::vcs-blame-program",
                        )
                    })?;
                    let mut mm = std::collections::BTreeMap::new();
                    mm.insert(
                        TermOrdKey(Term::symbol(":snapshot")),
                        Term::Str(snapshot.to_string()),
                    );
                    mm.insert(TermOrdKey(Term::symbol(":sym")), Term::Str(sym.to_string()));
                    if let Some(p) = path.as_deref() {
                        mm.insert(TermOrdKey(Term::symbol(":path")), Term::Str(p.to_string()));
                    }
                    let req = Term::Map(mm);
                    let prog = f.apply(&mut ctx, Value::Data(req)).map_err(|e| {
                        cli_err(
                            EX_EVAL,
                            "eval/error",
                            format!("core/cli vcs-blame-program failed: {e}"),
                        )
                    })?;

                    let mut dm = std::collections::BTreeMap::new();
                    dm.insert(
                        TermOrdKey(Term::symbol(":cmd")),
                        Term::Str("vcs/blame".to_string()),
                    );
                    dm.insert(
                        TermOrdKey(Term::symbol(":snapshot")),
                        Term::Str(snapshot.to_string()),
                    );
                    dm.insert(TermOrdKey(Term::symbol(":sym")), Term::Str(sym.to_string()));
                    if let Some(p) = path.as_deref() {
                        dm.insert(TermOrdKey(Term::symbol(":path")), Term::Str(p.to_string()));
                    }
                    let desc = Term::Map(dm);
                    (prog, "genesis/vcs-blame-v0.1", "vcs-blame", desc)
                }
                VcsCmd::Why { snapshot, sym, op } => {
                    gc_vcs::validate_hex_hash(snapshot).map_err(|e| {
                        cli_err(EX_PARSE, "vcs/why", format!("invalid --snapshot: {e}"))
                    })?;
                    if sym.trim().is_empty() {
                        return Err(cli_err(EX_PARSE, "vcs/why", "invalid --sym: empty value"));
                    }
                    if let Some(op) = op.as_deref()
                        && op.trim().is_empty()
                    {
                        return Err(cli_err(EX_PARSE, "vcs/why", "invalid --op: empty value"));
                    }

                    let f = env.get("core/cli::vcs-why-program").ok_or_else(|| {
                        cli_err(
                            EX_INTERNAL,
                            "selfhost/missing",
                            "missing binding core/cli::vcs-why-program",
                        )
                    })?;
                    let mut mm = std::collections::BTreeMap::new();
                    mm.insert(
                        TermOrdKey(Term::symbol(":snapshot")),
                        Term::Str(snapshot.to_string()),
                    );
                    mm.insert(TermOrdKey(Term::symbol(":sym")), Term::Str(sym.to_string()));
                    if let Some(o) = op.as_deref() {
                        mm.insert(TermOrdKey(Term::symbol(":op")), Term::Str(o.to_string()));
                    }
                    let req = Term::Map(mm);
                    let prog = f.apply(&mut ctx, Value::Data(req)).map_err(|e| {
                        cli_err(
                            EX_EVAL,
                            "eval/error",
                            format!("core/cli vcs-why-program failed: {e}"),
                        )
                    })?;

                    let mut dm = std::collections::BTreeMap::new();
                    dm.insert(
                        TermOrdKey(Term::symbol(":cmd")),
                        Term::Str("vcs/why".to_string()),
                    );
                    dm.insert(
                        TermOrdKey(Term::symbol(":snapshot")),
                        Term::Str(snapshot.to_string()),
                    );
                    dm.insert(TermOrdKey(Term::symbol(":sym")), Term::Str(sym.to_string()));
                    if let Some(o) = op.as_deref() {
                        dm.insert(TermOrdKey(Term::symbol(":op")), Term::Str(o.to_string()));
                    }
                    let desc = Term::Map(dm);
                    (prog, "genesis/vcs-why-v0.1", "vcs-why", desc)
                }
                VcsCmd::Diff {
                    base,
                    to,
                    out,
                    no_store,
                } => {
                    let f = env.get("core/cli::vcs-diff-program").ok_or_else(|| {
                        cli_err(
                            EX_INTERNAL,
                            "selfhost/missing",
                            "missing binding core/cli::vcs-diff-program",
                        )
                    })?;
                    let mut mm = std::collections::BTreeMap::new();
                    mm.insert(
                        TermOrdKey(Term::symbol(":base")),
                        Term::Str(base.to_string()),
                    );
                    mm.insert(TermOrdKey(Term::symbol(":to")), Term::Str(to.to_string()));
                    mm.insert(TermOrdKey(Term::symbol(":store")), Term::Bool(!*no_store));
                    if let Some(o) = out.as_deref() {
                        mm.insert(
                            TermOrdKey(Term::symbol(":out")),
                            Term::Str(o.display().to_string()),
                        );
                    }
                    let req = Term::Map(mm);
                    let prog = f.apply(&mut ctx, Value::Data(req)).map_err(|e| {
                        cli_err(
                            EX_EVAL,
                            "eval/error",
                            format!("core/cli vcs-diff-program failed: {e}"),
                        )
                    })?;
                    let mut dm = std::collections::BTreeMap::new();
                    dm.insert(
                        TermOrdKey(Term::symbol(":cmd")),
                        Term::Str("vcs/diff".to_string()),
                    );
                    dm.insert(
                        TermOrdKey(Term::symbol(":base")),
                        Term::Str(base.to_string()),
                    );
                    dm.insert(TermOrdKey(Term::symbol(":to")), Term::Str(to.to_string()));
                    dm.insert(TermOrdKey(Term::symbol(":store")), Term::Bool(!*no_store));
                    if let Some(o) = out.as_deref() {
                        dm.insert(
                            TermOrdKey(Term::symbol(":out")),
                            Term::Str(o.display().to_string()),
                        );
                    }
                    let desc = Term::Map(dm);
                    (prog, "genesis/vcs-diff-v0.1", "vcs-diff", desc)
                }
                VcsCmd::Apply {
                    base,
                    patch,
                    out,
                    no_store,
                } => {
                    let f = env.get("core/cli::vcs-apply-program").ok_or_else(|| {
                        cli_err(
                            EX_INTERNAL,
                            "selfhost/missing",
                            "missing binding core/cli::vcs-apply-program",
                        )
                    })?;
                    let mut mm = std::collections::BTreeMap::new();
                    mm.insert(
                        TermOrdKey(Term::symbol(":base")),
                        Term::Str(base.to_string()),
                    );
                    mm.insert(
                        TermOrdKey(Term::symbol(":patch")),
                        Term::Str(patch.to_string()),
                    );
                    mm.insert(TermOrdKey(Term::symbol(":store")), Term::Bool(!*no_store));
                    if let Some(o) = out.as_deref() {
                        mm.insert(
                            TermOrdKey(Term::symbol(":out")),
                            Term::Str(o.display().to_string()),
                        );
                    }
                    let req = Term::Map(mm);
                    let prog = f.apply(&mut ctx, Value::Data(req)).map_err(|e| {
                        cli_err(
                            EX_EVAL,
                            "eval/error",
                            format!("core/cli vcs-apply-program failed: {e}"),
                        )
                    })?;
                    let mut dm = std::collections::BTreeMap::new();
                    dm.insert(
                        TermOrdKey(Term::symbol(":cmd")),
                        Term::Str("vcs/apply".to_string()),
                    );
                    dm.insert(
                        TermOrdKey(Term::symbol(":base")),
                        Term::Str(base.to_string()),
                    );
                    dm.insert(
                        TermOrdKey(Term::symbol(":patch")),
                        Term::Str(patch.to_string()),
                    );
                    dm.insert(TermOrdKey(Term::symbol(":store")), Term::Bool(!*no_store));
                    if let Some(o) = out.as_deref() {
                        dm.insert(
                            TermOrdKey(Term::symbol(":out")),
                            Term::Str(o.display().to_string()),
                        );
                    }
                    let desc = Term::Map(dm);
                    (prog, "genesis/vcs-apply-v0.1", "vcs-apply", desc)
                }
                VcsCmd::Merge3 {
                    base,
                    left,
                    right,
                    out,
                } => {
                    let f = env.get("core/cli::vcs-merge3-program").ok_or_else(|| {
                        cli_err(
                            EX_INTERNAL,
                            "selfhost/missing",
                            "missing binding core/cli::vcs-merge3-program",
                        )
                    })?;
                    let mut mm = std::collections::BTreeMap::new();
                    mm.insert(
                        TermOrdKey(Term::symbol(":base")),
                        Term::Str(base.to_string()),
                    );
                    mm.insert(
                        TermOrdKey(Term::symbol(":left")),
                        Term::Str(left.to_string()),
                    );
                    mm.insert(
                        TermOrdKey(Term::symbol(":right")),
                        Term::Str(right.to_string()),
                    );
                    if let Some(o) = out.as_deref() {
                        mm.insert(
                            TermOrdKey(Term::symbol(":out")),
                            Term::Str(o.display().to_string()),
                        );
                    }
                    let req = Term::Map(mm);
                    let prog = f.apply(&mut ctx, Value::Data(req)).map_err(|e| {
                        cli_err(
                            EX_EVAL,
                            "eval/error",
                            format!("core/cli vcs-merge3-program failed: {e}"),
                        )
                    })?;
                    let mut dm = std::collections::BTreeMap::new();
                    dm.insert(
                        TermOrdKey(Term::symbol(":cmd")),
                        Term::Str("vcs/merge3".to_string()),
                    );
                    dm.insert(
                        TermOrdKey(Term::symbol(":base")),
                        Term::Str(base.to_string()),
                    );
                    dm.insert(
                        TermOrdKey(Term::symbol(":left")),
                        Term::Str(left.to_string()),
                    );
                    dm.insert(
                        TermOrdKey(Term::symbol(":right")),
                        Term::Str(right.to_string()),
                    );
                    if let Some(o) = out.as_deref() {
                        dm.insert(
                            TermOrdKey(Term::symbol(":out")),
                            Term::Str(o.display().to_string()),
                        );
                    }
                    let desc = Term::Map(dm);
                    (prog, "genesis/vcs-merge3-v0.1", "vcs-merge3", desc)
                }
                VcsCmd::ResolveConflict {
                    conflict,
                    strategy,
                    picks,
                    sets,
                    out,
                } => {
                    if strategy.is_none() && picks.is_empty() && sets.is_empty() {
                        return Err(cli_err(
                            EX_PARSE,
                            "vcs/resolve-conflict",
                            "must provide --strategy and/or --pick/--set overrides",
                        ));
                    }

                    let f = env
                        .get("core/cli::vcs-resolve-conflict-program")
                        .ok_or_else(|| {
                            cli_err(
                                EX_INTERNAL,
                                "selfhost/missing",
                                "missing binding core/cli::vcs-resolve-conflict-program",
                            )
                        })?;

                    let mut payload: std::collections::BTreeMap<TermOrdKey, Term> =
                        std::collections::BTreeMap::new();
                    payload.insert(
                        TermOrdKey(Term::symbol(":conflict")),
                        Term::Str(conflict.to_string()),
                    );
                    if let Some(s) = strategy.as_deref() {
                        let s = s.trim();
                        let sym = match s {
                            "left" | ":left" => ":left",
                            "right" | ":right" => ":right",
                            "base" | ":base" => ":base",
                            other => {
                                return Err(cli_err(
                                    EX_PARSE,
                                    "vcs/resolve-conflict",
                                    format!(
                                        "unsupported --strategy {other} (expected left|right|base)"
                                    ),
                                ));
                            }
                        };
                        payload.insert(
                            TermOrdKey(Term::symbol(":strategy")),
                            Term::Str(sym.to_string()),
                        );
                    }
                    if let Some(out) = out.as_deref() {
                        payload.insert(
                            TermOrdKey(Term::symbol(":out")),
                            Term::Str(out.display().to_string()),
                        );
                    }

                    let mut res: std::collections::BTreeMap<String, Term> =
                        std::collections::BTreeMap::new();
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
                        gc_vcs::validate_hex_hash(hv).map_err(|e| {
                            cli_err(EX_PARSE, "vcs/resolve-conflict", e.to_string())
                        })?;
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

                    let req = Term::Map(payload.clone());
                    let prog = f.apply(&mut ctx, Value::Data(req)).map_err(|e| {
                        cli_err(
                            EX_EVAL,
                            "eval/error",
                            format!("core/cli vcs-resolve-conflict-program failed: {e}"),
                        )
                    })?;

                    let mut dm: std::collections::BTreeMap<TermOrdKey, Term> =
                        std::collections::BTreeMap::new();
                    dm.insert(
                        TermOrdKey(Term::symbol(":cmd")),
                        Term::Str("vcs/resolve-conflict".to_string()),
                    );
                    dm.insert(
                        TermOrdKey(Term::symbol(":conflict")),
                        Term::Str(conflict.to_string()),
                    );
                    if let Some(s) = strategy.as_deref() {
                        dm.insert(
                            TermOrdKey(Term::symbol(":strategy")),
                            Term::Str(s.to_string()),
                        );
                    }
                    dm.insert(
                        TermOrdKey(Term::symbol(":picks-len")),
                        Term::Int((picks.len() as i64).into()),
                    );
                    dm.insert(
                        TermOrdKey(Term::symbol(":sets-len")),
                        Term::Int((sets.len() as i64).into()),
                    );
                    if let Some(out) = out.as_deref() {
                        dm.insert(
                            TermOrdKey(Term::symbol(":out")),
                            Term::Str(out.display().to_string()),
                        );
                    }
                    if let Some(Term::Map(rm)) =
                        payload.get(&TermOrdKey(Term::symbol(":resolutions")))
                    {
                        dm.insert(
                            TermOrdKey(Term::symbol(":resolutions")),
                            Term::Map(rm.clone()),
                        );
                    }
                    let desc = Term::Map(dm);
                    (
                        prog,
                        "genesis/vcs-resolve-conflict-v0.1",
                        "vcs-resolve-conflict",
                        desc,
                    )
                }
            };
            let program_hash = gc_coreform::hash_term(&desc);
            Ok((prog, kind, log_op, program_hash))
        }
    }?;

    let toolchain = format!("genesis {}", env!("CARGO_PKG_VERSION"));
    let r = gc_effects::run(&mut ctx, &policy, prog, program_hash, toolchain)
        .map_err(|e| cli_err(EX_EVAL, "effects/run", format!("{e}")))?;

    let log_path = log
        .map(PathBuf::from)
        .unwrap_or_else(|| default_log_path(log_op));
    std::fs::write(&log_path, r.log.to_string_canonical() + "\n")
        .with_context(|| format!("write {}", log_path.display()))
        .map_err(|e| cli_err(EX_IO, "io/write", format!("{e}")))?;

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
                m.get(&gc_coreform::TermOrdKey(Term::symbol(":error/code"))),
                Some(Term::Str(s)) if s == "core/caps/denied"
            )
        {
            exit_code = EX_CAPS_DENIED;
        }
    }

    let (value, value_format) = render_value_for_cli(&ctx, &r.value);

    // Detect conflict artifact and use stable exit semantics for merge.
    if matches!(cmd, VcsCmd::Merge3 { .. } | VcsCmd::ResolveConflict { .. })
        && let Value::Data(Term::Map(m)) = &r.value
        && matches!(
            m.get(&gc_coreform::TermOrdKey(Term::symbol(":ok"))),
            Some(Term::Bool(false))
        )
        && m.contains_key(&gc_coreform::TermOrdKey(Term::symbol(":conflict")))
    {
        ok = false;
        exit_code = 3; // conflict
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
            "coreform_frontend": frontend_info,
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

fn mk_store_put_program(artifact: &Term) -> Vec<Term> {
    // (def prog (core/effect::perform 'core/store::put {:artifact (quote <artifact>)} (fn (r) (core/effect::pure r)))) prog
    let op = Term::list(vec![Term::symbol("quote"), Term::symbol("core/store::put")]);
    let payload = Term::Map(
        [(
            gc_coreform::TermOrdKey(Term::symbol(":artifact")),
            Term::list(vec![Term::symbol("quote"), artifact.clone()]),
        )]
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

fn mk_store_get_program(hash: &str) -> Vec<Term> {
    let op = Term::list(vec![Term::symbol("quote"), Term::symbol("core/store::get")]);
    let payload = Term::Map(
        [(
            gc_coreform::TermOrdKey(Term::symbol(":hash")),
            Term::Str(hash.to_string()),
        )]
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

fn mk_store_has_program(hash: &str) -> Vec<Term> {
    let op = Term::list(vec![Term::symbol("quote"), Term::symbol("core/store::has")]);
    let payload = Term::Map(
        [(
            gc_coreform::TermOrdKey(Term::symbol(":hash")),
            Term::Str(hash.to_string()),
        )]
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

fn extract_store_put_hash(v: &Value) -> Option<String> {
    let t = v.to_term_for_log(None);
    let Term::Map(m) = t else { return None };
    match m.get(&gc_coreform::TermOrdKey(Term::symbol(":hash"))) {
        Some(Term::Str(s)) => Some(s.clone()),
        _ => None,
    }
}

fn extract_store_has_present(v: &Value) -> Option<bool> {
    let t = v.to_term_for_log(None);
    let Term::Map(m) = t else { return None };
    match m.get(&gc_coreform::TermOrdKey(Term::symbol(":present"))) {
        Some(Term::Bool(b)) => Some(*b),
        _ => None,
    }
}

fn extract_store_get_artifact(v: &Value) -> Option<Term> {
    let t = v.to_term_for_log(None);
    let Term::Map(m) = t else { return None };
    m.get(&gc_coreform::TermOrdKey(Term::symbol(":artifact")))
        .cloned()
}

fn mk_refs_get_program(name: &str) -> Vec<Term> {
    let op = Term::list(vec![Term::symbol("quote"), Term::symbol("core/refs::get")]);
    let payload = Term::Map(
        [(
            gc_coreform::TermOrdKey(Term::symbol(":name")),
            Term::Str(name.to_string()),
        )]
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

fn mk_refs_list_program(prefix: Option<&str>) -> Vec<Term> {
    let op = Term::list(vec![Term::symbol("quote"), Term::symbol("core/refs::list")]);
    let mut m = std::collections::BTreeMap::new();
    if let Some(p) = prefix {
        m.insert(
            gc_coreform::TermOrdKey(Term::symbol(":prefix")),
            Term::Str(p.to_string()),
        );
    } else {
        m.insert(gc_coreform::TermOrdKey(Term::symbol(":prefix")), Term::Nil);
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

fn mk_refs_set_program(
    name: &str,
    hash: &str,
    policy: &str,
    expected_old: Option<&str>,
) -> Vec<Term> {
    let op = Term::list(vec![Term::symbol("quote"), Term::symbol("core/refs::set")]);
    let mut m = std::collections::BTreeMap::new();
    m.insert(
        gc_coreform::TermOrdKey(Term::symbol(":name")),
        Term::Str(name.to_string()),
    );
    m.insert(
        gc_coreform::TermOrdKey(Term::symbol(":hash")),
        Term::Str(hash.to_string()),
    );
    m.insert(
        gc_coreform::TermOrdKey(Term::symbol(":policy")),
        Term::Str(policy.to_string()),
    );
    if let Some(e) = expected_old {
        let v = if e == "nil" {
            Term::Nil
        } else {
            Term::Str(e.to_string())
        };
        m.insert(gc_coreform::TermOrdKey(Term::symbol(":expected-old")), v);
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

fn mk_refs_delete_program(name: &str, policy: &str, expected_old: Option<&str>) -> Vec<Term> {
    let op = Term::list(vec![
        Term::symbol("quote"),
        Term::symbol("core/refs::delete"),
    ]);
    let mut m = std::collections::BTreeMap::new();
    m.insert(
        gc_coreform::TermOrdKey(Term::symbol(":name")),
        Term::Str(name.to_string()),
    );
    m.insert(
        gc_coreform::TermOrdKey(Term::symbol(":policy")),
        Term::Str(policy.to_string()),
    );
    if let Some(e) = expected_old {
        let v = if e == "nil" {
            Term::Nil
        } else {
            Term::Str(e.to_string())
        };
        m.insert(gc_coreform::TermOrdKey(Term::symbol(":expected-old")), v);
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

fn extract_refs_get_hash(v: &Value) -> Option<String> {
    let t = v.to_term_for_log(None);
    let Term::Map(m) = t else { return None };
    match m.get(&gc_coreform::TermOrdKey(Term::symbol(":hash"))) {
        Some(Term::Str(s)) => Some(s.clone()),
        Some(Term::Nil) => Some("nil".to_string()),
        _ => None,
    }
}

fn extract_refs_set_hash(v: &Value) -> Option<String> {
    let t = v.to_term_for_log(None);
    let Term::Map(m) = t else { return None };
    match m.get(&gc_coreform::TermOrdKey(Term::symbol(":hash"))) {
        Some(Term::Str(s)) => Some(s.clone()),
        Some(Term::Nil) => Some("nil".to_string()),
        _ => None,
    }
}

fn extract_refs_list_pairs(v: &Value) -> Option<Vec<(String, String)>> {
    let t = v.to_term_for_log(None);
    let Term::Map(m) = t else { return None };
    let Term::Vector(xs) = m.get(&gc_coreform::TermOrdKey(Term::symbol(":refs")))? else {
        return None;
    };
    let mut out = Vec::new();
    for x in xs {
        let Term::Map(em) = x else { return None };
        let name = match em.get(&gc_coreform::TermOrdKey(Term::symbol(":name"))) {
            Some(Term::Str(s)) => s.clone(),
            _ => return None,
        };
        let hash = match em.get(&gc_coreform::TermOrdKey(Term::symbol(":hash"))) {
            Some(Term::Str(s)) => s.clone(),
            Some(Term::Nil) => "nil".to_string(),
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
) -> Vec<Term> {
    let op = Term::list(vec![Term::symbol("quote"), Term::symbol("core/pkg::init")]);
    let mut m = std::collections::BTreeMap::new();
    m.insert(
        gc_coreform::TermOrdKey(Term::symbol(":workspace")),
        Term::Str(workspace.to_string()),
    );
    m.insert(
        gc_coreform::TermOrdKey(Term::symbol(":lock")),
        Term::Str(lock.display().to_string()),
    );
    m.insert(
        gc_coreform::TermOrdKey(Term::symbol(":policy")),
        Term::Str(policy.to_string()),
    );
    if let Some(rd) = registry_default {
        m.insert(
            gc_coreform::TermOrdKey(Term::symbol(":registry-default")),
            Term::Str(rd.to_string()),
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

fn mk_pkg_add_program(
    lock: &Path,
    name: &str,
    selector: &str,
    update_policy: &str,
    registry: Option<&str>,
) -> Vec<Term> {
    let op = Term::list(vec![Term::symbol("quote"), Term::symbol("core/pkg::add")]);
    let mut m = std::collections::BTreeMap::new();
    m.insert(
        gc_coreform::TermOrdKey(Term::symbol(":lock")),
        Term::Str(lock.display().to_string()),
    );
    m.insert(
        gc_coreform::TermOrdKey(Term::symbol(":name")),
        Term::Str(name.to_string()),
    );
    m.insert(
        gc_coreform::TermOrdKey(Term::symbol(":selector")),
        Term::Str(selector.to_string()),
    );
    m.insert(
        gc_coreform::TermOrdKey(Term::symbol(":update-policy")),
        Term::Str(update_policy.to_string()),
    );
    if let Some(r) = registry {
        m.insert(
            gc_coreform::TermOrdKey(Term::symbol(":registry")),
            Term::Str(r.to_string()),
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

fn mk_pkg_lock_program(lock: &Path, strict: bool) -> Vec<Term> {
    let op = Term::list(vec![Term::symbol("quote"), Term::symbol("core/pkg::lock")]);
    let payload = Term::Map(
        [
            (
                gc_coreform::TermOrdKey(Term::symbol(":lock")),
                Term::Str(lock.display().to_string()),
            ),
            (
                gc_coreform::TermOrdKey(Term::symbol(":strict")),
                Term::Bool(strict),
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

fn mk_pkg_update_program(lock: &Path) -> Vec<Term> {
    let op = Term::list(vec![
        Term::symbol("quote"),
        Term::symbol("core/pkg::update"),
    ]);
    let payload = Term::Map(
        [(
            gc_coreform::TermOrdKey(Term::symbol(":lock")),
            Term::Str(lock.display().to_string()),
        )]
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

fn mk_pkg_install_program(lock: &Path, frozen: bool, strict: bool) -> Vec<Term> {
    let op = Term::list(vec![
        Term::symbol("quote"),
        Term::symbol("core/pkg::install"),
    ]);
    let payload = Term::Map(
        [
            (
                gc_coreform::TermOrdKey(Term::symbol(":lock")),
                Term::Str(lock.display().to_string()),
            ),
            (
                gc_coreform::TermOrdKey(Term::symbol(":frozen")),
                Term::Bool(frozen),
            ),
            (
                gc_coreform::TermOrdKey(Term::symbol(":strict")),
                Term::Bool(strict),
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

fn mk_pkg_verify_program(lock: &Path) -> Vec<Term> {
    let op = Term::list(vec![
        Term::symbol("quote"),
        Term::symbol("core/pkg::verify"),
    ]);
    let payload = Term::Map(
        [(
            gc_coreform::TermOrdKey(Term::symbol(":lock")),
            Term::Str(lock.display().to_string()),
        )]
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

fn mk_pkg_list_program(lock: &Path) -> Vec<Term> {
    let op = Term::list(vec![Term::symbol("quote"), Term::symbol("core/pkg::list")]);
    let payload = Term::Map(
        [(
            gc_coreform::TermOrdKey(Term::symbol(":lock")),
            Term::Str(lock.display().to_string()),
        )]
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

fn mk_pkg_info_program(lock: &Path, name: &str) -> Vec<Term> {
    let op = Term::list(vec![Term::symbol("quote"), Term::symbol("core/pkg::info")]);
    let payload = Term::Map(
        [
            (
                gc_coreform::TermOrdKey(Term::symbol(":lock")),
                Term::Str(lock.display().to_string()),
            ),
            (
                gc_coreform::TermOrdKey(Term::symbol(":name")),
                Term::Str(name.to_string()),
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

fn mk_pkg_snapshot_program(pkg: &Path) -> Vec<Term> {
    let op = Term::list(vec![
        Term::symbol("quote"),
        Term::symbol("core/pkg::snapshot"),
    ]);
    let payload = Term::Map(
        [(
            gc_coreform::TermOrdKey(Term::symbol(":pkg")),
            Term::Str(pkg.display().to_string()),
        )]
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

fn mk_pkg_publish_program(
    remote: &str,
    refname: &str,
    policy_h: &str,
    expected_old: Option<&str>,
    depth: u64,
    commit: Option<&str>,
) -> Vec<Term> {
    let op = Term::list(vec![
        Term::symbol("quote"),
        Term::symbol("core/pkg::publish"),
    ]);
    let mut m = std::collections::BTreeMap::new();
    m.insert(
        gc_coreform::TermOrdKey(Term::symbol(":remote")),
        Term::Str(remote.to_string()),
    );
    m.insert(
        gc_coreform::TermOrdKey(Term::symbol(":ref")),
        Term::Str(refname.to_string()),
    );
    m.insert(
        gc_coreform::TermOrdKey(Term::symbol(":policy")),
        Term::Str(policy_h.to_string()),
    );
    if let Some(e) = expected_old {
        let v = if e == "nil" {
            Term::Nil
        } else {
            Term::Str(e.to_string())
        };
        m.insert(gc_coreform::TermOrdKey(Term::symbol(":expected-old")), v);
    }
    if depth > 0 {
        m.insert(
            gc_coreform::TermOrdKey(Term::symbol(":depth")),
            Term::Int((depth as i64).into()),
        );
    }
    if let Some(h) = commit {
        m.insert(
            gc_coreform::TermOrdKey(Term::symbol(":commit")),
            Term::Str(h.to_string()),
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

fn mk_gpk_export_program(
    root: &str,
    out: &Path,
    full: bool,
    depth: u64,
    include_evidence: &str,
    include_deps: &str,
    include_refs: &[String],
) -> Vec<Term> {
    let op = Term::list(vec![
        Term::symbol("quote"),
        Term::symbol("core/gpk::export"),
    ]);
    let mut m = std::collections::BTreeMap::new();
    m.insert(
        gc_coreform::TermOrdKey(Term::symbol(":root")),
        Term::Str(root.to_string()),
    );
    m.insert(
        gc_coreform::TermOrdKey(Term::symbol(":out")),
        Term::Str(out.display().to_string()),
    );
    if full {
        m.insert(
            gc_coreform::TermOrdKey(Term::symbol(":mode")),
            Term::Str(":full".to_string()),
        );
    } else {
        m.insert(
            gc_coreform::TermOrdKey(Term::symbol(":mode")),
            Term::Str(":shallow".to_string()),
        );
    }
    if depth > 0 {
        m.insert(
            gc_coreform::TermOrdKey(Term::symbol(":depth")),
            Term::Int((depth as i64).into()),
        );
    }
    m.insert(
        gc_coreform::TermOrdKey(Term::symbol(":include-evidence")),
        Term::Str(include_evidence.to_string()),
    );
    m.insert(
        gc_coreform::TermOrdKey(Term::symbol(":include-deps")),
        Term::Str(include_deps.to_string()),
    );
    if !include_refs.is_empty() {
        m.insert(
            gc_coreform::TermOrdKey(Term::symbol(":refs")),
            Term::Vector(include_refs.iter().cloned().map(Term::Str).collect()),
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

fn mk_gpk_import_program(input: &Path, set_refs: &[SetRefSpec]) -> Vec<Term> {
    let op = Term::list(vec![
        Term::symbol("quote"),
        Term::symbol("core/gpk::import"),
    ]);
    let mut payload_m = std::collections::BTreeMap::new();
    payload_m.insert(
        gc_coreform::TermOrdKey(Term::symbol(":in")),
        Term::Str(input.display().to_string()),
    );
    if !set_refs.is_empty() {
        let mut entries = Vec::with_capacity(set_refs.len());
        for sr in set_refs {
            let mut em = std::collections::BTreeMap::new();
            em.insert(
                gc_coreform::TermOrdKey(Term::symbol(":name")),
                Term::Str(sr.name.clone()),
            );
            em.insert(
                gc_coreform::TermOrdKey(Term::symbol(":hash")),
                if sr.hash == "nil" {
                    Term::Nil
                } else {
                    Term::Str(sr.hash.clone())
                },
            );
            em.insert(
                gc_coreform::TermOrdKey(Term::symbol(":policy")),
                Term::Str(sr.policy.clone()),
            );
            if let Some(exp) = &sr.expected_old {
                em.insert(
                    gc_coreform::TermOrdKey(Term::symbol(":expected-old")),
                    if exp == "nil" {
                        Term::Nil
                    } else {
                        Term::Str(exp.clone())
                    },
                );
            }
            entries.push(Term::Map(em));
        }
        payload_m.insert(
            gc_coreform::TermOrdKey(Term::symbol(":set-refs")),
            Term::Vector(entries),
        );
    }
    let payload = Term::Map(payload_m);
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

fn mk_sync_pull_program(
    remote: &str,
    refs: &[String],
    roots: &[String],
    depth: u64,
    force: bool,
) -> Vec<Term> {
    let op = Term::list(vec![Term::symbol("quote"), Term::symbol("core/sync::pull")]);
    let mut m = std::collections::BTreeMap::new();
    m.insert(
        gc_coreform::TermOrdKey(Term::symbol(":remote")),
        Term::Str(remote.to_string()),
    );
    if !refs.is_empty() {
        m.insert(
            gc_coreform::TermOrdKey(Term::symbol(":refs")),
            Term::Vector(refs.iter().cloned().map(Term::Str).collect()),
        );
    }
    if !roots.is_empty() {
        m.insert(
            gc_coreform::TermOrdKey(Term::symbol(":roots")),
            Term::Vector(roots.iter().cloned().map(Term::Str).collect()),
        );
    }
    if depth > 0 {
        m.insert(
            gc_coreform::TermOrdKey(Term::symbol(":depth")),
            Term::Int((depth as i64).into()),
        );
    }
    if force {
        m.insert(
            gc_coreform::TermOrdKey(Term::symbol(":force")),
            Term::Bool(true),
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

fn mk_sync_push_program(
    remote: &str,
    roots: &[String],
    depth: u64,
    set_refs: &[SetRefSpec],
) -> Vec<Term> {
    let op = Term::list(vec![Term::symbol("quote"), Term::symbol("core/sync::push")]);
    let mut m = std::collections::BTreeMap::new();
    m.insert(
        gc_coreform::TermOrdKey(Term::symbol(":remote")),
        Term::Str(remote.to_string()),
    );
    m.insert(
        gc_coreform::TermOrdKey(Term::symbol(":roots")),
        Term::Vector(roots.iter().cloned().map(Term::Str).collect()),
    );
    if depth > 0 {
        m.insert(
            gc_coreform::TermOrdKey(Term::symbol(":depth")),
            Term::Int((depth as i64).into()),
        );
    }
    if !set_refs.is_empty() {
        let mut out = Vec::new();
        for sr in set_refs {
            let mut mm = std::collections::BTreeMap::new();
            mm.insert(
                gc_coreform::TermOrdKey(Term::symbol(":name")),
                Term::Str(sr.name.clone()),
            );
            mm.insert(
                gc_coreform::TermOrdKey(Term::symbol(":hash")),
                Term::Str(sr.hash.clone()),
            );
            mm.insert(
                gc_coreform::TermOrdKey(Term::symbol(":policy")),
                Term::Str(sr.policy.clone()),
            );
            if let Some(e) = &sr.expected_old {
                let v = if e == "nil" {
                    Term::Nil
                } else {
                    Term::Str(e.clone())
                };
                mm.insert(gc_coreform::TermOrdKey(Term::symbol(":expected-old")), v);
            }
            out.push(Term::Map(mm));
        }
        m.insert(
            gc_coreform::TermOrdKey(Term::symbol(":set-refs")),
            Term::Vector(out),
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

fn mk_gc_plan_program(
    lock: &Path,
    pins: &Path,
    depth: u64,
    include_lock: bool,
    include_refs: bool,
) -> Vec<Term> {
    let op = Term::list(vec![Term::symbol("quote"), Term::symbol("core/gc::plan")]);
    let mut m = std::collections::BTreeMap::new();
    m.insert(
        gc_coreform::TermOrdKey(Term::symbol(":lock")),
        Term::Str(lock.display().to_string()),
    );
    m.insert(
        gc_coreform::TermOrdKey(Term::symbol(":pins")),
        Term::Str(pins.display().to_string()),
    );
    m.insert(
        gc_coreform::TermOrdKey(Term::symbol(":depth")),
        Term::Int((depth as i64).into()),
    );
    m.insert(
        gc_coreform::TermOrdKey(Term::symbol(":include-lock")),
        Term::Bool(include_lock),
    );
    m.insert(
        gc_coreform::TermOrdKey(Term::symbol(":include-refs")),
        Term::Bool(include_refs),
    );
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

fn mk_gc_run_program(
    lock: &Path,
    pins: &Path,
    depth: u64,
    include_lock: bool,
    include_refs: bool,
    quarantine: bool,
    quarantine_dir: Option<&Path>,
) -> Vec<Term> {
    let op = Term::list(vec![Term::symbol("quote"), Term::symbol("core/gc::run")]);
    let mut m = std::collections::BTreeMap::new();
    m.insert(
        gc_coreform::TermOrdKey(Term::symbol(":lock")),
        Term::Str(lock.display().to_string()),
    );
    m.insert(
        gc_coreform::TermOrdKey(Term::symbol(":pins")),
        Term::Str(pins.display().to_string()),
    );
    m.insert(
        gc_coreform::TermOrdKey(Term::symbol(":depth")),
        Term::Int((depth as i64).into()),
    );
    m.insert(
        gc_coreform::TermOrdKey(Term::symbol(":include-lock")),
        Term::Bool(include_lock),
    );
    m.insert(
        gc_coreform::TermOrdKey(Term::symbol(":include-refs")),
        Term::Bool(include_refs),
    );
    m.insert(
        gc_coreform::TermOrdKey(Term::symbol(":quarantine")),
        Term::Bool(quarantine),
    );
    if let Some(qd) = quarantine_dir {
        m.insert(
            gc_coreform::TermOrdKey(Term::symbol(":quarantine-dir")),
            Term::Str(qd.display().to_string()),
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

fn mk_gc_pin_program(target: &str, pins: &Path) -> Vec<Term> {
    let op = Term::list(vec![Term::symbol("quote"), Term::symbol("core/gc::pin")]);
    let payload = Term::Map(
        [
            (
                gc_coreform::TermOrdKey(Term::symbol(":target")),
                Term::Str(target.to_string()),
            ),
            (
                gc_coreform::TermOrdKey(Term::symbol(":pins")),
                Term::Str(pins.display().to_string()),
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

fn mk_gc_unpin_program(target: &str, pins: &Path) -> Vec<Term> {
    let op = Term::list(vec![Term::symbol("quote"), Term::symbol("core/gc::unpin")]);
    let payload = Term::Map(
        [
            (
                gc_coreform::TermOrdKey(Term::symbol(":target")),
                Term::Str(target.to_string()),
            ),
            (
                gc_coreform::TermOrdKey(Term::symbol(":pins")),
                Term::Str(pins.display().to_string()),
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

fn mk_gc_purge_program(ttl_days: u64, quarantine_dir: Option<&Path>) -> Vec<Term> {
    let op = Term::list(vec![Term::symbol("quote"), Term::symbol("core/gc::purge")]);
    let mut m = std::collections::BTreeMap::new();
    m.insert(
        gc_coreform::TermOrdKey(Term::symbol(":ttl-days")),
        Term::Int((ttl_days as i64).into()),
    );
    if let Some(qd) = quarantine_dir {
        m.insert(
            gc_coreform::TermOrdKey(Term::symbol(":quarantine-dir")),
            Term::Str(qd.display().to_string()),
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

fn mk_vcs_log_program(root: &str, max: u64) -> Vec<Term> {
    let op = Term::list(vec![Term::symbol("quote"), Term::symbol("core/vcs::log")]);
    let payload = Term::Map(
        [
            (
                gc_coreform::TermOrdKey(Term::symbol(":root")),
                Term::Str(root.to_string()),
            ),
            (
                gc_coreform::TermOrdKey(Term::symbol(":max")),
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
        gc_coreform::TermOrdKey(Term::symbol(":snapshot")),
        Term::Str(snapshot.to_string()),
    );
    m.insert(
        gc_coreform::TermOrdKey(Term::symbol(":sym")),
        Term::Str(sym.to_string()),
    );
    if let Some(path) = path {
        m.insert(
            gc_coreform::TermOrdKey(Term::symbol(":path")),
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
        gc_coreform::TermOrdKey(Term::symbol(":snapshot")),
        Term::Str(snapshot.to_string()),
    );
    m.insert(
        gc_coreform::TermOrdKey(Term::symbol(":sym")),
        Term::Str(sym.to_string()),
    );
    if let Some(op_sym) = op_sym {
        if op_sym.trim().is_empty() {
            return Err(cli_err(EX_PARSE, "vcs/why", "invalid --op: empty value"));
        }
        m.insert(
            gc_coreform::TermOrdKey(Term::symbol(":op")),
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
        gc_coreform::TermOrdKey(Term::symbol(":base")),
        Term::Str(base.to_string()),
    );
    m.insert(
        gc_coreform::TermOrdKey(Term::symbol(":to")),
        Term::Str(to.to_string()),
    );
    if let Some(out) = out {
        m.insert(
            gc_coreform::TermOrdKey(Term::symbol(":out")),
            Term::Str(out.display().to_string()),
        );
    }
    m.insert(
        gc_coreform::TermOrdKey(Term::symbol(":store")),
        Term::Bool(store),
    );
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
        gc_coreform::TermOrdKey(Term::symbol(":base")),
        Term::Str(base.to_string()),
    );
    m.insert(
        gc_coreform::TermOrdKey(Term::symbol(":patch")),
        Term::Str(patch.to_string()),
    );
    if let Some(out) = out {
        m.insert(
            gc_coreform::TermOrdKey(Term::symbol(":out")),
            Term::Str(out.display().to_string()),
        );
    }
    m.insert(
        gc_coreform::TermOrdKey(Term::symbol(":store")),
        Term::Bool(store),
    );
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
        gc_coreform::TermOrdKey(Term::symbol(":base")),
        Term::Str(base.to_string()),
    );
    m.insert(
        gc_coreform::TermOrdKey(Term::symbol(":left")),
        Term::Str(left.to_string()),
    );
    m.insert(
        gc_coreform::TermOrdKey(Term::symbol(":right")),
        Term::Str(right.to_string()),
    );
    if let Some(out) = out {
        m.insert(
            gc_coreform::TermOrdKey(Term::symbol(":out")),
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

    let mut payload: std::collections::BTreeMap<gc_coreform::TermOrdKey, Term> =
        std::collections::BTreeMap::new();
    payload.insert(
        gc_coreform::TermOrdKey(Term::symbol(":conflict")),
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
            gc_coreform::TermOrdKey(Term::symbol(":strategy")),
            Term::Str(sym.to_string()),
        );
    }
    if let Some(out) = out {
        payload.insert(
            gc_coreform::TermOrdKey(Term::symbol(":out")),
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
        let mut rm: std::collections::BTreeMap<gc_coreform::TermOrdKey, Term> =
            std::collections::BTreeMap::new();
        for (k, v) in res {
            rm.insert(gc_coreform::TermOrdKey(Term::Symbol(k)), v);
        }
        payload.insert(
            gc_coreform::TermOrdKey(Term::symbol(":resolutions")),
            Term::Map(rm),
        );
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
    match m.get(&gc_coreform::TermOrdKey(Term::symbol(":patch"))) {
        Some(Term::Str(s)) => Some(s.clone()),
        _ => None,
    }
}

fn extract_vcs_snapshot_hash(v: &Value) -> Option<String> {
    let t = v.to_term_for_log(None);
    let Term::Map(m) = t else { return None };
    match m.get(&gc_coreform::TermOrdKey(Term::symbol(":snapshot"))) {
        Some(Term::Str(s)) => Some(s.clone()),
        _ => None,
    }
}

fn extract_vcs_commit_hash(v: &Value) -> Option<String> {
    let t = v.to_term_for_log(None);
    let Term::Map(m) = t else { return None };
    match m.get(&gc_coreform::TermOrdKey(Term::symbol(":commit"))) {
        Some(Term::Str(s)) => Some(s.clone()),
        _ => None,
    }
}

fn extract_pkg_snapshot_hash(v: &Value) -> Option<String> {
    let t = v.to_term_for_log(None);
    let Term::Map(m) = t else { return None };
    match m.get(&gc_coreform::TermOrdKey(Term::symbol(":snapshot"))) {
        Some(Term::Str(s)) => Some(s.clone()),
        _ => None,
    }
}

fn extract_pkg_export_bundle_hash(v: &Value) -> Option<String> {
    let t = v.to_term_for_log(None);
    let Term::Map(m) = t else { return None };
    match m.get(&gc_coreform::TermOrdKey(Term::symbol(":bundle-h"))) {
        Some(Term::Str(s)) => Some(s.clone()),
        _ => None,
    }
}

fn extract_pkg_import_root(v: &Value) -> Option<String> {
    let t = v.to_term_for_log(None);
    let Term::Map(m) = t else { return None };
    match m.get(&gc_coreform::TermOrdKey(Term::symbol(":root"))) {
        Some(Term::Str(s)) => Some(s.clone()),
        _ => None,
    }
}

fn extract_pkg_publish_commit(v: &Value) -> Option<String> {
    let t = v.to_term_for_log(None);
    let Term::Map(m) = t else { return None };
    match m.get(&gc_coreform::TermOrdKey(Term::symbol(":commit"))) {
        Some(Term::Str(s)) => Some(s.clone()),
        _ => None,
    }
}

fn extract_pkg_lock_hash(v: &Value) -> Option<String> {
    let t = v.to_term_for_log(None);
    let Term::Map(m) = t else { return None };
    match m.get(&gc_coreform::TermOrdKey(Term::symbol(":lock-h"))) {
        Some(Term::Str(s)) => Some(s.clone()),
        _ => None,
    }
}

fn extract_pkg_ok_bool(v: &Value) -> Option<bool> {
    let t = v.to_term_for_log(None);
    let Term::Map(m) = t else { return None };
    match m.get(&gc_coreform::TermOrdKey(Term::symbol(":ok"))) {
        Some(Term::Bool(b)) => Some(*b),
        _ => None,
    }
}

fn cmd_replay(
    cli: &Cli,
    file: &PathBuf,
    engine: Option<FmtEngine>,
    log_path: &PathBuf,
    store_dir: Option<&Path>,
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
        parse_term(&log_src).map_err(|e| cli_err(EX_PARSE, "parse/log", e.to_string()))?;
    let log = EffectLog::from_term(&log_term)
        .map_err(|e| cli_err(EX_PARSE, "parse/log", format!("{e}")))?;
    if log.program_hash != program_hash {
        return Err(cli_err(
            EX_REPLAY_MISMATCH,
            "replay/program-hash-mismatch",
            "program hash mismatch: log is for different program",
        ));
    }

    let prog = eval_module(&mut ctx, &mut env, &forms)
        .map_err(|e| cli_err(EX_EVAL, "eval/error", format!("{e}")))?;
    let store = match store_dir {
        Some(p) => Some(
            gc_effects::ArtifactStore::open(p)
                .map_err(|e| cli_err(EX_IO, "io/store", format!("{e}")))?,
        ),
        None => None,
    };
    let v = gc_effects::replay_with_store(&mut ctx, prog, &log, store.as_ref()).map_err(|e| {
        let code = match e {
            gc_effects::EffectsError::ReplayMismatch(_) => "replay/mismatch",
            _ => "replay/error",
        };
        cli_err(EX_REPLAY_MISMATCH, code, format!("{e}"))
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

fn cmd_test(cli: &Cli, pkg: &Path, caps: Option<&Path>) -> Result<CmdOut, CliError> {
    let frontend = resolved_coreform_frontend(cli)?;
    let frontend_info = coreform_frontend_json(&frontend);

    let r = gc_obligations::test_package_with_step_limit_and_frontend(
        pkg,
        caps,
        resolved_step_limit(cli),
        resolved_mem_limits(cli),
        frontend.clone(),
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
            "coreform_frontend": frontend_info,
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
    let frontend_info = coreform_frontend_json(&frontend);

    let h = gc_obligations::pack_with_frontend(pkg, frontend).map_err(obligation_err)?;
    let env = JsonEnvelope {
        ok: true,
        kind: "genesis/pack-v0.2",
        data: Some(serde_json::json!({
            "pkg": pkg.display().to_string(),
            "coreform_frontend": frontend_info,
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
    fast_path_required: bool,
    selfhost_routed: bool,
    default_selfhost: bool,
}

const SELFHOST_CUTOVER_ROWS: &[SelfhostCutoverRow] = &[
    SelfhostCutoverRow {
        cmd: "fmt",
        fast_path_required: true,
        selfhost_routed: true,
        default_selfhost: true,
    },
    SelfhostCutoverRow {
        cmd: "eval",
        fast_path_required: true,
        selfhost_routed: true,
        default_selfhost: true,
    },
    SelfhostCutoverRow {
        cmd: "explain",
        fast_path_required: true,
        selfhost_routed: true,
        default_selfhost: true,
    },
    SelfhostCutoverRow {
        cmd: "run",
        fast_path_required: true,
        selfhost_routed: true,
        default_selfhost: true,
    },
    SelfhostCutoverRow {
        cmd: "replay",
        fast_path_required: true,
        selfhost_routed: true,
        default_selfhost: true,
    },
    SelfhostCutoverRow {
        cmd: "test",
        fast_path_required: true,
        selfhost_routed: true,
        default_selfhost: true,
    },
    SelfhostCutoverRow {
        cmd: "pack",
        fast_path_required: true,
        selfhost_routed: true,
        default_selfhost: true,
    },
    SelfhostCutoverRow {
        cmd: "selfhost-artifact",
        fast_path_required: false,
        selfhost_routed: true,
        default_selfhost: true,
    },
    SelfhostCutoverRow {
        cmd: "selfhost-dashboard",
        fast_path_required: false,
        selfhost_routed: true,
        default_selfhost: true,
    },
    SelfhostCutoverRow {
        cmd: "keygen",
        fast_path_required: false,
        selfhost_routed: false,
        default_selfhost: false,
    },
    SelfhostCutoverRow {
        cmd: "sign",
        fast_path_required: false,
        selfhost_routed: false,
        default_selfhost: false,
    },
    SelfhostCutoverRow {
        cmd: "transparency-verify",
        fast_path_required: false,
        selfhost_routed: false,
        default_selfhost: false,
    },
    SelfhostCutoverRow {
        cmd: "typecheck",
        fast_path_required: true,
        selfhost_routed: true,
        default_selfhost: true,
    },
    SelfhostCutoverRow {
        cmd: "optimize",
        fast_path_required: true,
        selfhost_routed: true,
        default_selfhost: true,
    },
    SelfhostCutoverRow {
        cmd: "apply-patch",
        fast_path_required: true,
        selfhost_routed: true,
        default_selfhost: true,
    },
    SelfhostCutoverRow {
        cmd: "verify",
        fast_path_required: false,
        selfhost_routed: false,
        default_selfhost: false,
    },
    SelfhostCutoverRow {
        cmd: "store/*",
        fast_path_required: true,
        selfhost_routed: true,
        default_selfhost: true,
    },
    SelfhostCutoverRow {
        cmd: "refs/*",
        fast_path_required: true,
        selfhost_routed: true,
        default_selfhost: true,
    },
    SelfhostCutoverRow {
        cmd: "pkg/*",
        fast_path_required: true,
        selfhost_routed: true,
        default_selfhost: true,
    },
    SelfhostCutoverRow {
        cmd: "policy/*",
        fast_path_required: false,
        selfhost_routed: false,
        default_selfhost: false,
    },
    SelfhostCutoverRow {
        cmd: "sync/*",
        fast_path_required: true,
        selfhost_routed: true,
        default_selfhost: true,
    },
    SelfhostCutoverRow {
        cmd: "gc/*",
        fast_path_required: true,
        selfhost_routed: true,
        default_selfhost: true,
    },
    SelfhostCutoverRow {
        cmd: "vcs/*",
        fast_path_required: true,
        selfhost_routed: true,
        default_selfhost: true,
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
        .filter(|r| r.default_selfhost)
        .count();
    let fast_path_total = SELFHOST_CUTOVER_ROWS
        .iter()
        .filter(|r| r.fast_path_required)
        .count();
    let fast_path_default_ok = SELFHOST_CUTOVER_ROWS
        .iter()
        .filter(|r| r.fast_path_required)
        .all(|r| r.default_selfhost && r.selfhost_routed);
    let routed_bps = percent_basis_points(routed_count, total_commands);
    let default_bps = percent_basis_points(default_selfhost_count, total_commands);

    let rows_term: Vec<Term> = SELFHOST_CUTOVER_ROWS
        .iter()
        .map(|row| {
            Term::Map(
                [
                    (
                        TermOrdKey(Term::symbol(":cmd")),
                        Term::Str(row.cmd.to_string()),
                    ),
                    (
                        TermOrdKey(Term::symbol(":fast-path-required")),
                        Term::Bool(row.fast_path_required),
                    ),
                    (
                        TermOrdKey(Term::symbol(":selfhost-routed")),
                        Term::Bool(row.selfhost_routed),
                    ),
                    (
                        TermOrdKey(Term::symbol(":default-selfhost")),
                        Term::Bool(row.default_selfhost),
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
                            TermOrdKey(Term::symbol(":fast-path-required-commands")),
                            Term::Int((fast_path_total as i64).into()),
                        ),
                        (
                            TermOrdKey(Term::symbol(":fast-path-default-ok")),
                            Term::Bool(fast_path_default_ok),
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
            format!("| Fast-path default OK | {} |", fast_path_default_ok),
            "".to_string(),
            "## Command Coverage".to_string(),
            "".to_string(),
            "| Command | Fast Path | Selfhost Routed | Default Selfhost |".to_string(),
            "| --- | --- | --- | --- |".to_string(),
        ];
        for row in SELFHOST_CUTOVER_ROWS {
            lines.push(format!(
                "| `{}` | {} | {} | {} |",
                row.cmd, row.fast_path_required, row.selfhost_routed, row.default_selfhost
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
                "fast_path_required_commands": fast_path_total,
                "fast_path_default_ok": fast_path_default_ok,
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

fn cmd_selfhost_artifact(
    cli: &Cli,
    out: &Path,
    min_stage2_supported_modules: u64,
    min_stage2_validated_modules: u64,
) -> Result<CmdOut, CliError> {
    #[derive(Debug, Clone)]
    struct Stage2Seed {
        source: String,
        forms: Vec<Term>,
        module_hash: [u8; 32],
        stage2_module_hash: [u8; 32],
        stage1_ok: bool,
        stage1_errors: Vec<String>,
        supported: bool,
        ok: bool,
        errors: Vec<String>,
        wasm_hash: Option<[u8; 32]>,
        wasm_bytes_len: Option<usize>,
    }

    #[derive(Debug, Clone)]
    struct Stage2Summary {
        module_hash: [u8; 32],
        supported: bool,
        ok: bool,
        errors: Vec<String>,
        wasm_hash: Option<[u8; 32]>,
        wasm_bytes_len: Option<usize>,
    }

    #[derive(Debug, Clone)]
    struct Stage2SeedIndex {
        generated_by: Option<String>,
        modules: std::collections::BTreeMap<String, Stage2Seed>,
    }

    fn load_stage2_seed_index(path: &Path) -> Option<Stage2SeedIndex> {
        let src = std::fs::read_to_string(path).ok()?;
        let term = parse_term(&src).ok()?;
        let Term::Map(root) = term else { return None };
        match root.get(&TermOrdKey(Term::symbol(":kind"))) {
            Some(Term::Str(s)) if s == "genesis/selfhost-toolchain-artifact-v0.2" => {}
            _ => return None,
        }
        let generated_by = match root.get(&TermOrdKey(Term::symbol(":generated-by"))) {
            Some(Term::Str(s)) => Some(s.clone()),
            _ => None,
        };
        let modules = match root.get(&TermOrdKey(Term::symbol(":modules"))) {
            Some(Term::Vector(v)) => v,
            _ => return None,
        };
        let mut out = std::collections::BTreeMap::new();
        for m in modules {
            let Term::Map(mm) = m else { continue };
            let path = match mm.get(&TermOrdKey(Term::symbol(":path"))) {
                Some(Term::Str(s)) => s.clone(),
                _ => continue,
            };
            let source = match mm.get(&TermOrdKey(Term::symbol(":source"))) {
                Some(Term::Str(s)) => s.clone(),
                _ => continue,
            };
            let forms = match mm.get(&TermOrdKey(Term::symbol(":forms"))) {
                Some(Term::Vector(v)) => v.clone(),
                _ => continue,
            };
            let module_hash = match mm.get(&TermOrdKey(Term::symbol(":module-h"))) {
                Some(Term::Bytes(b)) if b.len() == 32 => {
                    let mut h = [0u8; 32];
                    h.copy_from_slice(b.as_ref());
                    h
                }
                _ => continue,
            };
            let stage2_module_hash = match mm.get(&TermOrdKey(Term::symbol(":stage2-module-h"))) {
                Some(Term::Bytes(b)) if b.len() == 32 => {
                    let mut h = [0u8; 32];
                    h.copy_from_slice(b.as_ref());
                    h
                }
                _ => module_hash,
            };
            let stage1_ok = matches!(
                mm.get(&TermOrdKey(Term::symbol(":stage1-ok"))),
                Some(Term::Bool(true))
            );
            let stage1_errors = match mm.get(&TermOrdKey(Term::symbol(":stage1-errors"))) {
                Some(Term::Vector(v)) => v
                    .iter()
                    .filter_map(|t| match t {
                        Term::Str(s) => Some(s.clone()),
                        _ => None,
                    })
                    .collect(),
                _ => Vec::new(),
            };
            let supported = matches!(
                mm.get(&TermOrdKey(Term::symbol(":stage2-supported"))),
                Some(Term::Bool(true))
            );
            let ok = matches!(
                mm.get(&TermOrdKey(Term::symbol(":stage2-ok"))),
                Some(Term::Bool(true))
            );
            let errors = match mm.get(&TermOrdKey(Term::symbol(":stage2-errors"))) {
                Some(Term::Vector(v)) => v
                    .iter()
                    .filter_map(|t| match t {
                        Term::Str(s) => Some(s.clone()),
                        _ => None,
                    })
                    .collect(),
                _ => Vec::new(),
            };
            let wasm_hash = match mm.get(&TermOrdKey(Term::symbol(":stage2-wasm-h"))) {
                Some(Term::Bytes(b)) if b.len() == 32 => {
                    let mut h = [0u8; 32];
                    h.copy_from_slice(b.as_ref());
                    Some(h)
                }
                _ => None,
            };
            let wasm_bytes_len = match mm.get(&TermOrdKey(Term::symbol(":stage2-wasm-bytes"))) {
                Some(Term::Int(i)) => i.to_string().parse::<usize>().ok(),
                _ => None,
            };
            out.insert(
                path,
                Stage2Seed {
                    source,
                    forms,
                    module_hash,
                    stage2_module_hash,
                    stage1_ok,
                    stage1_errors,
                    supported,
                    ok,
                    errors,
                    wasm_hash,
                    wasm_bytes_len,
                },
            );
        }
        Some(Stage2SeedIndex {
            generated_by,
            modules: out,
        })
    }

    let frontend = resolved_coreform_frontend(cli)?;
    let (out_buf, min_stage2_supported_modules, min_stage2_validated_modules) = match frontend {
        gc_obligations::CoreformFrontend::Rust => (
            out.to_path_buf(),
            min_stage2_supported_modules,
            min_stage2_validated_modules,
        ),
        gc_obligations::CoreformFrontend::Selfhost(_) => {
            let req = Term::Map(
                [
                    (
                        TermOrdKey(Term::symbol(":out")),
                        Term::Str(out.display().to_string()),
                    ),
                    (
                        TermOrdKey(Term::symbol(":min-stage2-supported-modules")),
                        Term::Int((min_stage2_supported_modules as i64).into()),
                    ),
                    (
                        TermOrdKey(Term::symbol(":min-stage2-validated-modules")),
                        Term::Int((min_stage2_validated_modules as i64).into()),
                    ),
                ]
                .into_iter()
                .collect(),
            );
            let planned = selfhost_plan_request_map(
                cli,
                "core/cli::selfhost-artifact-request",
                req,
                "selfhost-artifact",
            )?;
            (
                PathBuf::from(planned_required_str(&planned, ":out", "selfhost-artifact")?),
                planned_required_u64(
                    &planned,
                    ":min-stage2-supported-modules",
                    "selfhost-artifact",
                )?,
                planned_required_u64(
                    &planned,
                    ":min-stage2-validated-modules",
                    "selfhost-artifact",
                )?,
            )
        }
    };
    let out = out_buf.as_path();

    // Artifact rebuild uses trusted bundled sources; do not charge user step limits here.
    let step_limit = StepLimit::Unlimited;
    let mem_limits = resolved_mem_limits(cli);
    let bootstrap_mode = if embedded_bootstrap_available() {
        SelfhostBootstrapMode::Embedded
    } else {
        SelfhostBootstrapMode::ArtifactOnly
    };
    let bootstrap_artifact = if bootstrap_mode == SelfhostBootstrapMode::ArtifactOnly {
        resolved_selfhost_artifact_for_frontend(cli)
    } else {
        None
    };
    if bootstrap_mode == SelfhostBootstrapMode::ArtifactOnly && bootstrap_artifact.is_none() {
        return Err(cli_err(
            EX_PARSE,
            "selfhost/bootstrap",
            "selfhost-artifact requires an existing toolchain artifact when embedded bootstrap is unavailable; pass --selfhost-artifact or set GENESIS_SELFHOST_TOOLCHAIN_ARTIFACT",
        ));
    }
    let mut ctx = EvalCtx::with_step_limit(None);
    ctx.set_mem_limits(mem_limits);
    let prelude = build_prelude(&mut ctx);
    let mut env = prelude.env;
    load_selfhost_coreform_toolchain_v1_with_mode(
        &mut ctx,
        &mut env,
        bootstrap_mode,
        bootstrap_artifact.as_deref(),
    )
    .map_err(|e| cli_err(EX_PARSE, "selfhost/bootstrap", format!("{e}")))?;
    ctx.steps = 0;
    ctx.step_limit = step_limit.resolve();

    let stage2_seed_index = bootstrap_artifact.as_deref().and_then(load_stage2_seed_index);
    let reuse_seed_results = stage2_seed_index.as_ref().is_some_and(|idx| {
        idx.generated_by.as_deref() == Some(&format!("genesis {}", env!("CARGO_PKG_VERSION")))
    });
    let mut stage2_seed_hits = 0u64;
    let mut stage2_computed = 0u64;

    let mut modules = Vec::new();
    let mut all_ok = true;
    let mut stage2_supported = 0u64;
    let mut stage2_validated = 0u64;
    let mut gate_errors: Vec<String> = Vec::new();

    for (path, src) in selfhost_coreform_toolchain_v1_sources() {
        let seed = if reuse_seed_results {
            stage2_seed_index
                .as_ref()
                .and_then(|idx| idx.modules.get(&path))
                .filter(|s| s.source == src)
                .cloned()
        } else {
            None
        };

        let (forms, module_h, stage1_ok, stage1_errors, stage2) = if let Some(seed) = seed {
            stage2_seed_hits = stage2_seed_hits.saturating_add(1);
            (
                seed.forms,
                seed.module_hash,
                seed.stage1_ok,
                seed.stage1_errors,
                Stage2Summary {
                    module_hash: seed.stage2_module_hash,
                    supported: seed.supported,
                    ok: seed.ok,
                    errors: seed.errors,
                    wasm_hash: seed.wasm_hash,
                    wasm_bytes_len: seed.wasm_bytes_len,
                },
            )
        } else {
            let forms = selfhost_parse_canonicalize_module(&mut ctx, &env, &src).map_err(|e| {
                cli_err(
                    e.exit_code,
                    "selfhost/canon",
                    format!("{path}: {}", e.json.message),
                )
            })?;
            let module_h = selfhost_hash_module_forms(&mut ctx, &env, &forms).map_err(|e| {
                cli_err(
                    e.exit_code,
                    "selfhost/hash",
                    format!("{path}: {}", e.json.message),
                )
            })?;
            let stage1_forms =
                selfhost_stage1_transform_module(&mut ctx, &env, &forms).map_err(|e| {
                    cli_err(
                        e.exit_code,
                        "selfhost/stage1",
                        format!("{path}: {}", e.json.message),
                    )
                })?;
            let gate_report = gc_opt::stage1_validation_report(&forms, &stage1_forms);
            stage2_computed = stage2_computed.saturating_add(1);
            let report = gc_opt::stage2_validation_report(&stage1_forms);
            (
                forms,
                module_h,
                gate_report.ok,
                gate_report.errors,
                Stage2Summary {
                    module_hash: report.module_hash,
                    supported: report.supported,
                    ok: report.ok,
                    errors: report.errors,
                    wasm_hash: report.wasm_hash,
                    wasm_bytes_len: report.wasm_bytes_len,
                },
            )
        };

        if !stage1_ok || (stage2.supported && !stage2.ok) {
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
                    Term::Str(path.clone()),
                ),
                (
                    TermOrdKey(Term::symbol(":source")),
                    Term::Str(src.clone()),
                ),
                (
                    TermOrdKey(Term::symbol(":forms")),
                    Term::Vector(forms.clone()),
                ),
                (
                    TermOrdKey(Term::symbol(":module-h")),
                    Term::Bytes(module_h.to_vec().into()),
                ),
                (
                    TermOrdKey(Term::symbol(":stage1-ok")),
                    Term::Bool(stage1_ok),
                ),
                (
                    TermOrdKey(Term::symbol(":stage1-errors")),
                    Term::Vector(stage1_errors.into_iter().map(Term::Str).collect()),
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
                    Term::Vector(stage2.errors.iter().cloned().map(Term::Str).collect()),
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
                Term::Str(format!("genesis {}", env!("CARGO_PKG_VERSION"))),
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
            "stage2_cache_hits": stage2_seed_hits,
            "stage2_computed_modules": stage2_computed,
            "stage2_seed_artifact": bootstrap_artifact.as_ref().map(|p| p.display().to_string()),
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

fn cmd_keygen(cli: &Cli, out: &Path) -> Result<CmdOut, CliError> {
    let frontend = resolved_coreform_frontend(cli)?;
    let out_buf = match frontend {
        gc_obligations::CoreformFrontend::Rust => out.to_path_buf(),
        gc_obligations::CoreformFrontend::Selfhost(_) => {
            let req = Term::Map(
                [(
                    TermOrdKey(Term::symbol(":out")),
                    Term::Str(out.display().to_string()),
                )]
                .into_iter()
                .collect(),
            );
            let planned = selfhost_plan_request_map(cli, "core/cli::keygen-request", req, "keygen")?;
            PathBuf::from(planned_required_str(&planned, ":out", "keygen")?)
        }
    };
    let out = out_buf.as_path();

    let k = gc_obligations::KeyFile::generate_ed25519();
    k.write_secure(out)
        .map_err(|e| cli_err(EX_IO, "io/write", format!("{e}")))?;

    let env = JsonEnvelope {
        ok: true,
        kind: "genesis/keygen-v0.2",
        data: Some(serde_json::json!({
            "out": out.display().to_string(),
            "alg": k.alg,
            "pk_b64": k.pk_b64,
        })),
        error: None,
    };
    Ok(CmdOut {
        exit_code: EX_OK,
        stdout: if cli.json {
            String::new()
        } else {
            format!("{}\n", out.display())
        },
        json: serde_json::to_value(env).expect("json"),
    })
}

fn cmd_sign(
    cli: &Cli,
    pkg: &Path,
    key_path: &Path,
    acceptance: Option<&str>,
    signatures: Option<&Path>,
) -> Result<CmdOut, CliError> {
    let frontend = resolved_coreform_frontend(cli)?;
    let (pkg_buf, key_path_buf, acceptance_buf, signatures_buf) = match frontend {
        gc_obligations::CoreformFrontend::Rust => (
            pkg.to_path_buf(),
            key_path.to_path_buf(),
            acceptance.map(|s| s.to_string()),
            signatures.map(Path::to_path_buf),
        ),
        gc_obligations::CoreformFrontend::Selfhost(_) => {
            let req = Term::Map(
                [
                    (
                        TermOrdKey(Term::symbol(":pkg")),
                        Term::Str(pkg.display().to_string()),
                    ),
                    (
                        TermOrdKey(Term::symbol(":key")),
                        Term::Str(key_path.display().to_string()),
                    ),
                    (
                        TermOrdKey(Term::symbol(":acceptance")),
                        acceptance
                            .map(|s| Term::Str(s.to_string()))
                            .unwrap_or(Term::Nil),
                    ),
                    (
                        TermOrdKey(Term::symbol(":signatures")),
                        signatures
                            .map(|p| Term::Str(p.display().to_string()))
                            .unwrap_or(Term::Nil),
                    ),
                ]
                .into_iter()
                .collect(),
            );
            let planned = selfhost_plan_request_map(cli, "core/cli::sign-request", req, "sign")?;
            (
                PathBuf::from(planned_required_str(&planned, ":pkg", "sign")?),
                PathBuf::from(planned_required_str(&planned, ":key", "sign")?),
                planned_optional_str(&planned, ":acceptance", "sign")?,
                planned_optional_str(&planned, ":signatures", "sign")?.map(PathBuf::from),
            )
        }
    };
    let pkg = pkg_buf.as_path();
    let key_path = key_path_buf.as_path();
    let acceptance = acceptance_buf.as_deref();
    let signatures = signatures_buf.as_deref();

    let (_manifest, pkg_dir) = PackageManifest::load(pkg)
        .map_err(|e| cli_err(EX_PARSE, "manifest/parse", format!("{e}")))?;
    let store = gc_obligations::EvidenceStore::open(&pkg_dir).map_err(obligation_err)?;

    let acc_hex = match acceptance {
        Some(s) => s.trim().to_string(),
        None => gc_obligations::read_acceptance_hash_from_last(&pkg_dir).map_err(|e| match e {
            gc_obligations::SigningError::Io(_) => cli_err(EX_IO, "io/read", format!("{e}")),
            _ => cli_err(EX_PARSE, "sign/acceptance", format!("{e}")),
        })?,
    };

    let k = gc_obligations::KeyFile::load(key_path)
        .map_err(|e| cli_err(EX_PARSE, "sign/key", format!("{e}")))?;
    let sk = k
        .signing_key()
        .map_err(|e| cli_err(EX_PARSE, "sign/key", format!("{e}")))?;

    let (sig_artifact, _rec) = gc_obligations::sign_acceptance_hash(&store, &acc_hex, &sk)
        .map_err(|e| cli_err(EX_INTERNAL, "sign/error", format!("{e}")))?;

    // Update .genesis/last_signature and the signature set.
    let genesis_dir = pkg_dir.join(".genesis");
    std::fs::create_dir_all(&genesis_dir)
        .map_err(|e| cli_err(EX_IO, "io/write", format!("{e}")))?;
    std::fs::write(
        genesis_dir.join("last_signature"),
        format!("{sig_artifact}\n"),
    )
    .map_err(|e| cli_err(EX_IO, "io/write", format!("{e}")))?;

    let sigset_path = signatures
        .map(|p| p.to_path_buf())
        .unwrap_or_else(|| gc_obligations::signatures_file_path(&pkg_dir));
    let mut set = gc_obligations::load_signature_set(&sigset_path).unwrap_or_default();
    set.push(sig_artifact.clone());
    gc_obligations::write_signature_set(&sigset_path, &set)
        .map_err(|e| cli_err(EX_IO, "io/write", format!("{e}")))?;

    // Append a transparency log entry (best-effort deterministic format; if this fails, treat as error).
    let pkg_artifact = gc_obligations::package_artifact_hash(pkg).map_err(obligation_err)?;
    let transparency_entry = gc_obligations::append_transparency_entry(
        &store,
        &pkg_dir,
        &pkg_artifact,
        &acc_hex,
        &sig_artifact,
        &k.pk_b64,
    )
    .map_err(|e| cli_err(EX_INTERNAL, "transparency/error", format!("{e}")))?;

    let env = JsonEnvelope {
        ok: true,
        kind: "genesis/sign-v0.2",
        data: Some(serde_json::json!({
            "pkg": pkg.display().to_string(),
            "key": key_path.display().to_string(),
            "package_artifact": pkg_artifact,
            "acceptance_artifact": acc_hex,
            "signature_artifact": sig_artifact,
            "sigset": sigset_path.display().to_string(),
            "transparency_entry": transparency_entry,
            "pk_b64": k.pk_b64,
        })),
        error: None,
    };
    Ok(CmdOut {
        exit_code: EX_OK,
        stdout: if cli.json {
            String::new()
        } else {
            format!("{sig_artifact}\n")
        },
        json: serde_json::to_value(env).expect("json"),
    })
}

fn cmd_transparency_verify(cli: &Cli, pkg: &Path) -> Result<CmdOut, CliError> {
    let frontend = resolved_coreform_frontend(cli)?;
    let pkg_buf = match frontend {
        gc_obligations::CoreformFrontend::Rust => pkg.to_path_buf(),
        gc_obligations::CoreformFrontend::Selfhost(_) => {
            let req = Term::Map(
                [(
                    TermOrdKey(Term::symbol(":pkg")),
                    Term::Str(pkg.display().to_string()),
                )]
                .into_iter()
                .collect(),
            );
            let planned = selfhost_plan_request_map(
                cli,
                "core/cli::transparency-verify-request",
                req,
                "transparency-verify",
            )?;
            PathBuf::from(planned_required_str(
                &planned,
                ":pkg",
                "transparency-verify",
            )?)
        }
    };
    let pkg = pkg_buf.as_path();

    let (_manifest, pkg_dir) = PackageManifest::load(pkg)
        .map_err(|e| cli_err(EX_PARSE, "manifest/parse", format!("{e}")))?;
    let store = gc_obligations::EvidenceStore::open(&pkg_dir).map_err(obligation_err)?;
    let r = gc_obligations::verify_transparency_log(&store, &pkg_dir)
        .map_err(|e| cli_err(EX_INTERNAL, "transparency/error", format!("{e}")))?;
    let exit_code = if r.ok { EX_OK } else { EX_VERIFY };
    let env = JsonEnvelope {
        ok: r.ok,
        kind: "genesis/transparency-verify-v0.2",
        data: Some(serde_json::json!({
            "pkg": pkg.display().to_string(),
            "head": r.head,
            "entries": r.entries,
            "errors": r.errors,
        })),
        error: None,
    };
    let mut stdout = String::new();
    if !cli.json {
        stdout.push_str(if r.ok { "ok\n" } else { "not ok\n" });
    }
    Ok(CmdOut {
        exit_code,
        stdout,
        json: serde_json::to_value(env).expect("json"),
    })
}

fn cmd_typecheck(cli: &Cli, pkg: &Path) -> Result<CmdOut, CliError> {
    let frontend = resolved_coreform_frontend(cli)?;
    let frontend_info = coreform_frontend_json(&frontend);
    let result = gc_obligations::typecheck_package_with_step_limit_and_frontend(
        pkg,
        resolved_step_limit(cli),
        resolved_mem_limits(cli),
        frontend,
    )
    .map_err(obligation_err)?;
    let report_s = result.report_coreform;

    let exit_code = if result.ok { EX_OK } else { EX_OBLIGATIONS };
    let env = JsonEnvelope {
        ok: result.ok,
        kind: "genesis/typecheck-v0.2",
        data: Some(serde_json::json!({
            "pkg": pkg.display().to_string(),
            "coreform_frontend": frontend_info,
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
    let frontend_info = coreform_frontend_json(&coreform_frontend_for_engine(cli, engine));
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
    let pipeline =
        gc_opt::optimize_command_pipeline(&forms, stage1_gate, stage2_gate, emit_wasm.is_some())
            .map_err(|e| match e {
                gc_opt::OptimizeCommandError::Stage1Build(msg) => {
                    cli_err(EX_INTERNAL, "stage1/error", msg)
                }
                gc_opt::OptimizeCommandError::Stage1Gate(out) => CliError {
                    exit_code: EX_OBLIGATIONS,
                    json: JsonError {
                        code: "obligation/stage1-validation",
                        message: "core/obligation::stage1-validation failed".to_string(),
                        context: Some(gc_opt::stage1_pipeline_json(&out)),
                    },
                },
                gc_opt::OptimizeCommandError::Stage2Gate(s2) => CliError {
                    exit_code: EX_OBLIGATIONS,
                    json: JsonError {
                        code: "obligation/translation-validation",
                        message:
                            "core/obligation::translation-validation (stage2 CoreForm->WASM) failed"
                                .to_string(),
                        context: Some(gc_opt::stage2_report_json(&s2)),
                    },
                },
                gc_opt::OptimizeCommandError::Stage2Compile(e) => match e {
                    gc_opt::Stage2CompileError::Unsupported(msg) => {
                        cli_err(EX_OBLIGATIONS, "stage2/unsupported", msg)
                    }
                    gc_opt::Stage2CompileError::Internal(msg) => {
                        cli_err(EX_INTERNAL, "stage2/error", msg)
                    }
                },
            })?;

    if let Some(p) = emit_wasm {
        let art = pipeline.wasm_artifact.as_ref().ok_or_else(|| {
            cli_err(
                EX_INTERNAL,
                "stage2/error",
                "missing wasm artifact from optimize pipeline",
            )
        })?;
        std::fs::write(p, &art.wasm_bytes)
            .with_context(|| format!("write {}", p.display()))
            .map_err(|e| cli_err(EX_IO, "io/write", format!("{e}")))?;
    }

    let out_s = print_module(&pipeline.optimized_forms);

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
            "coreform_frontend": frontend_info,
            "stage1": gc_opt::stage1_pipeline_json(&pipeline.stage1),
            "stage2": pipeline.stage2.as_ref().map(gc_opt::stage2_report_json),
            "changed": pipeline.changed,
            "original_hash": hex32(pipeline.original_hash),
            "optimized_hash": hex32(pipeline.optimized_hash),
            "egg_runs": pipeline.stage1.optimize_report.stats.egg_runs,
            "egg_iterations": pipeline.stage1.optimize_report.stats.iterations,
            "egg_eclasses": pipeline.stage1.optimize_report.stats.eclasses,
            "egg_enodes": pipeline.stage1.optimize_report.stats.enodes,
            "egg_rewrites_applied": pipeline.stage1.optimize_report.stats.rewrites_applied,
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
    let frontend_info = coreform_frontend_json(&frontend);

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
            "coreform_frontend": frontend_info,
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

fn cmd_verify(
    cli: &Cli,
    pkg: &Path,
    acceptance: Option<&str>,
    policy: Option<&Path>,
    signatures: Option<&Path>,
    scan_store: bool,
) -> Result<CmdOut, CliError> {
    let frontend = resolved_coreform_frontend(cli)?;
    let (pkg_buf, acceptance_buf, policy_buf, signatures_buf, scan_store) = match frontend {
        gc_obligations::CoreformFrontend::Rust => (
            pkg.to_path_buf(),
            acceptance.map(|s| s.to_string()),
            policy.map(Path::to_path_buf),
            signatures.map(Path::to_path_buf),
            scan_store,
        ),
        gc_obligations::CoreformFrontend::Selfhost(_) => {
            let req = Term::Map(
                [
                    (
                        TermOrdKey(Term::symbol(":pkg")),
                        Term::Str(pkg.display().to_string()),
                    ),
                    (
                        TermOrdKey(Term::symbol(":acceptance")),
                        acceptance
                            .map(|s| Term::Str(s.to_string()))
                            .unwrap_or(Term::Nil),
                    ),
                    (
                        TermOrdKey(Term::symbol(":policy")),
                        policy
                            .map(|p| Term::Str(p.display().to_string()))
                            .unwrap_or(Term::Nil),
                    ),
                    (
                        TermOrdKey(Term::symbol(":signatures")),
                        signatures
                            .map(|p| Term::Str(p.display().to_string()))
                            .unwrap_or(Term::Nil),
                    ),
                    (TermOrdKey(Term::symbol(":scan-store")), Term::Bool(scan_store)),
                ]
                .into_iter()
                .collect(),
            );
            let planned =
                selfhost_plan_request_map(cli, "core/cli::verify-request", req, "verify")?;
            (
                PathBuf::from(planned_required_str(&planned, ":pkg", "verify")?),
                planned_optional_str(&planned, ":acceptance", "verify")?,
                planned_optional_str(&planned, ":policy", "verify")?.map(PathBuf::from),
                planned_optional_str(&planned, ":signatures", "verify")?.map(PathBuf::from),
                planned_required_bool(&planned, ":scan-store", "verify")?,
            )
        }
    };
    let pkg = pkg_buf.as_path();
    let acceptance = acceptance_buf.as_deref();
    let policy = policy_buf.as_deref();
    let signatures = signatures_buf.as_deref();

    let r =
        gc_obligations::verify_package_with_policy(pkg, acceptance, scan_store, policy, signatures)
            .map_err(obligation_err)?;
    let exit_code = if r.ok { EX_OK } else { EX_VERIFY };

    let env = JsonEnvelope {
        ok: r.ok,
        kind: "genesis/verify-v0.2",
        data: Some(serde_json::json!({
            "pkg": pkg.display().to_string(),
            "acceptance_artifact": r.acceptance_artifact,
            "policy": policy.map(|p| p.display().to_string()),
            "signatures": signatures.map(|p| p.display().to_string()),
            "policy_min_signatures": r.policy_min_signatures,
            "checked_signatures": r.checked_signatures,
            "valid_signatures": r.valid_signatures,
            "store_scanned": r.store_scanned,
            "checked_modules": r.checked_modules,
            "checked_deps": r.checked_deps,
            "checked_artifacts": r.checked_artifacts,
            "errors": r.errors,
        })),
        error: None,
    };

    let mut stdout = String::new();
    if !cli.json {
        stdout.push_str(if r.ok { "ok\n" } else { "not ok\n" });
    }
    Ok(CmdOut {
        exit_code,
        stdout,
        json: serde_json::to_value(env).expect("json"),
    })
}

fn normalize_newlines(s: &str) -> String {
    s.replace("\r\n", "\n")
}

fn render_value_for_cli(ctx: &EvalCtx, v: &Value) -> (String, &'static str) {
    // Prefer a stable CoreForm-ish representation. For sealed protocol errors we unwrap the
    // payload for readability.
    let protocol_error: Option<SealId> = ctx.protocol.map(|p| p.error);
    let t = v.to_term_for_log(protocol_error);
    (gc_coreform::print_term(&t), "coreform")
}

fn hex32(h: [u8; 32]) -> String {
    const HEX: &[u8; 16] = b"0123456789abcdef";
    let mut out = String::new();
    for b in h {
        out.push(HEX[(b >> 4) as usize] as char);
        out.push(HEX[(b & 0x0f) as usize] as char);
    }
    out
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
