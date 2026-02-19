use std::collections::{BTreeMap, BTreeSet};
use std::io::{self, BufRead, Write};
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

mod cmd_gc;
mod cmd_pkg;
mod cmd_policy;
mod cmd_refs;
mod cmd_store;
mod cmd_sync;
mod cmd_vcs;
mod diagnostics;
mod kernel_exec;
mod pkg_abi;
mod pkg_contract;
mod pkg_doctor;
mod pkg_reports;
mod pkg_task_runner;
mod pkg_telemetry;
mod pkg_workspace_ops;
mod program_builders;

use cmd_gc::cmd_gc;
use cmd_pkg::cmd_pkg;
use cmd_policy::cmd_policy;
use cmd_refs::cmd_refs;
use cmd_store::cmd_store;
use cmd_sync::cmd_sync;
pub(crate) use cmd_vcs::SetRefSpec;
use cmd_vcs::{
    cmd_vcs, extract_pkg_export_bundle_hash, extract_pkg_import_root, extract_pkg_lock_hash,
    extract_pkg_ok_bool, extract_pkg_publish_commit, extract_pkg_snapshot_hash,
    extract_refs_get_hash, extract_refs_list_pairs, extract_refs_set_hash,
    extract_store_get_artifact, extract_store_has_present, extract_store_put_hash, is_hex64,
    mk_store_get_program, mk_store_has_program, mk_store_put_program, normalize_pkg_add_strategy,
    parse_local_set_refs, parse_pkg_spec, parse_sync_set_refs,
};
use diagnostics::annotate_envelope;
use kernel_exec::eval_module_default;
use program_builders::*;

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
                println!("{}", json_canonical_string(&out.json));
            } else if !out.stdout.is_empty() {
                print!("{}", out.stdout);
            }
            std::process::ExitCode::from(out.exit_code)
        }
        Err(e) => {
            if cli.json {
                let out = serde_json::to_value(JsonEnvelope::<serde_json::Value> {
                    ok: false,
                    kind: "genesis/error-v0.2",
                    data: None,
                    error: Some(e.json),
                })
                .expect("json serialization");
                let out = annotate_envelope(out, e.exit_code);
                println!("{}", json_canonical_string(&out));
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

fn canonicalize_json(v: &serde_json::Value) -> serde_json::Value {
    match v {
        serde_json::Value::Object(m) => {
            let mut sorted: BTreeMap<String, serde_json::Value> = BTreeMap::new();
            for (k, vv) in m {
                sorted.insert(k.clone(), canonicalize_json(vv));
            }
            let mut out = serde_json::Map::new();
            for (k, vv) in sorted {
                out.insert(k, vv);
            }
            serde_json::Value::Object(out)
        }
        serde_json::Value::Array(xs) => {
            serde_json::Value::Array(xs.iter().map(canonicalize_json).collect())
        }
        _ => v.clone(),
    }
}

fn json_canonical_string(v: &serde_json::Value) -> String {
    serde_json::to_string(&canonicalize_json(v)).expect("json serialization")
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

#[derive(Debug, Deserialize)]
struct WarmRequest {
    argv: Vec<String>,
}

#[derive(Debug)]
struct CmdOut {
    exit_code: u8,
    stdout: String,
    json: serde_json::Value,
}

fn dispatch(cli: &Cli, flavor: Flavor) -> Result<CmdOut, CliError> {
    enforce_selfhost_only_cmd(cli, flavor)?;
    let mut out = match &cli.cmd {
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
        Cmd::Warm { prime_selfhost } => cmd_warm(cli, flavor, *prime_selfhost),
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
    }?;
    out.json = annotate_envelope(out.json, out.exit_code);
    Ok(out)
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
    cfg!(debug_assertions)
        && std::env::var(ALLOW_RUST_ENGINE_ENV)
            .map(|v| parse_truthy_env_flag(&v))
            .unwrap_or(false)
}

fn non_artifact_bootstrap_modes_allowed() -> bool {
    cfg!(debug_assertions)
}

fn bootstrap_mode_label(mode: SelfhostBootstrapMode) -> &'static str {
    match mode {
        SelfhostBootstrapMode::ArtifactOnly => "artifact-only",
        SelfhostBootstrapMode::ArtifactPreferred => "artifact-preferred",
        SelfhostBootstrapMode::Embedded => "embedded",
    }
}

fn enforce_bootstrap_mode_allowed_with_flag(
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

fn enforce_bootstrap_mode_allowed(
    mode: SelfhostBootstrapMode,
    context: &str,
) -> Result<(), CliError> {
    enforce_bootstrap_mode_allowed_with_flag(mode, context, non_artifact_bootstrap_modes_allowed())
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
                let msg = if cfg!(debug_assertions) {
                    format!(
                        "`--coreform-frontend rust` is disabled in the default selfhost profile; set {ALLOW_RUST_ENGINE_ENV}=1 to enable compatibility mode"
                    )
                } else {
                    "`--coreform-frontend rust` is disabled in release profile; rust compatibility is development-only for offline parity harnesses".to_string()
                };
                return Err(cli_err(EX_VERIFY, "engine/rust-disabled", msg));
            }
            Ok(gc_obligations::CoreformFrontend::Rust)
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

fn coreform_frontend_for_engine(
    cli: &Cli,
    engine: FmtEngine,
) -> Result<gc_obligations::CoreformFrontend, CliError> {
    match engine {
        FmtEngine::Rust => Ok(gc_obligations::CoreformFrontend::Rust),
        FmtEngine::Selfhost => {
            let mode = resolved_selfhost_bootstrap_mode(cli);
            enforce_bootstrap_mode_allowed(mode, "engine frontend")?;
            Ok(gc_obligations::CoreformFrontend::Selfhost(
                gc_obligations::SelfhostFrontendConfig {
                    bootstrap_mode: mode,
                    artifact: resolved_selfhost_artifact_for_frontend(cli),
                },
            ))
        }
    }
}

fn rust_engine_disabled_message(cmd_name: &str) -> String {
    if cfg!(debug_assertions) {
        format!(
            "`--engine rust` is disabled in the default selfhost profile for `{cmd_name}`; set {ALLOW_RUST_ENGINE_ENV}=1 to enable compatibility mode"
        )
    } else {
        format!(
            "`--engine rust` is disabled in release profile for `{cmd_name}`; rust compatibility is development-only for offline parity harnesses"
        )
    }
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
                rust_engine_disabled_message(cmd_name),
            ));
        }
        if e == FmtEngine::Selfhost {
            enforce_bootstrap_mode_allowed(resolved_selfhost_bootstrap_mode(cli), cmd_name)?;
        }
        return Ok(e);
    }
    Ok(FmtEngine::Selfhost)
}

fn load_selfhost_toolchain(
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
    let artifact = resolved_selfhost_artifact_for_frontend(cli);
    load_selfhost_coreform_toolchain_v1_with_mode(ctx, env, mode, artifact.as_deref())
        .map_err(|e| cli_err(EX_INTERNAL, "selfhost/init", format!("{e}")))
}

fn maybe_embedded_bootstrap_mode() -> SelfhostBootstrapMode {
    if embedded_bootstrap_available() && non_artifact_bootstrap_modes_allowed() {
        SelfhostBootstrapMode::Embedded
    } else {
        SelfhostBootstrapMode::ArtifactOnly
    }
}

fn coreform_frontend_for_engine_json(
    cli: &Cli,
    engine: FmtEngine,
) -> Result<serde_json::Value, CliError> {
    Ok(coreform_frontend_json(&coreform_frontend_for_engine(
        cli, engine,
    )?))
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

fn is_legacy_high_level_semantic_op(op: &str) -> bool {
    // Selfhost cutover is complete for pkg/vcs/gc/gpk command semantics. In selfhost-only mode
    // these high-level semantic ops must not execute at runtime.
    op.starts_with("core/pkg::")
        || op.starts_with("core/vcs::")
        || op.starts_with("core/gc::")
        || op.starts_with("core/gpk::")
}

fn collect_legacy_high_level_semantic_ops(log: &EffectLog) -> Vec<String> {
    let mut out: BTreeSet<String> = BTreeSet::new();
    for entry in &log.entries {
        if is_legacy_high_level_semantic_op(&entry.op) {
            out.insert(entry.op.clone());
        }
    }
    out.into_iter().collect()
}

fn enforce_no_legacy_semantic_fallback_in_selfhost_only(
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
        Cmd::Warm { .. } => Ok(()),
        Cmd::Vcs {
            cmd: VcsCmd::Hash { engine, .. },
            ..
        } => enforce_selfhost_engine(cli, "vcs hash", *engine),
        Cmd::Vcs { .. } => Ok(()),
    }
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

fn inherited_global_args(cli: &Cli) -> Vec<String> {
    let mut out = Vec::new();
    if cli.json {
        out.push("--json".to_string());
    }
    if let Some(n) = cli.step_limit {
        out.push("--step-limit".to_string());
        out.push(n.to_string());
    }
    if cli.no_step_limit {
        out.push("--no-step-limit".to_string());
    }
    if let Some(n) = cli.max_pair_cells {
        out.push("--max-pair-cells".to_string());
        out.push(n.to_string());
    }
    if let Some(n) = cli.max_vec_len {
        out.push("--max-vec-len".to_string());
        out.push(n.to_string());
    }
    if let Some(n) = cli.max_map_len {
        out.push("--max-map-len".to_string());
        out.push(n.to_string());
    }
    if let Some(n) = cli.max_bytes_len {
        out.push("--max-bytes-len".to_string());
        out.push(n.to_string());
    }
    if let Some(n) = cli.max_string_len {
        out.push("--max-string-len".to_string());
        out.push(n.to_string());
    }
    if let Some(p) = &cli.selfhost_artifact {
        out.push("--selfhost-artifact".to_string());
        out.push(p.display().to_string());
    }
    out.push("--selfhost-bootstrap".to_string());
    out.push(
        match cli.selfhost_bootstrap {
            SelfhostBootstrapArg::ArtifactOnly => "artifact-only",
            SelfhostBootstrapArg::ArtifactPreferred => "artifact-preferred",
            SelfhostBootstrapArg::Embedded => "embedded",
        }
        .to_string(),
    );
    if cli.selfhost_only {
        out.push("--selfhost-only".to_string());
    }
    if let Some(frontend) = cli.coreform_frontend {
        out.push("--coreform-frontend".to_string());
        out.push(
            match frontend {
                CoreformFrontendArg::Rust => "rust",
                CoreformFrontendArg::Selfhost => "selfhost",
            }
            .to_string(),
        );
    }
    out
}

fn emit_warm_line(v: &serde_json::Value) -> Result<(), CliError> {
    let mut out = io::stdout().lock();
    writeln!(out, "{}", json_canonical_string(v))
        .map_err(|e| cli_err(EX_IO, "io/error", format!("{e}")))?;
    out.flush()
        .map_err(|e| cli_err(EX_IO, "io/error", format!("{e}")))?;
    Ok(())
}

fn flavor_token(flavor: Flavor) -> &'static str {
    match flavor {
        Flavor::Native => "native",
        Flavor::Wasi => "wasi",
    }
}

fn warm_session_cache_key(
    cli: &Cli,
    flavor: Flavor,
    prime_selfhost: bool,
    inherited: &[String],
) -> String {
    let cwd = std::env::current_dir()
        .ok()
        .and_then(|p| p.to_str().map(|s| s.to_string()))
        .unwrap_or_else(|| ".".to_string());
    let payload = serde_json::json!({
        "kind": "genesis/warm-cache-key-v0.1",
        "flavor": flavor_token(flavor),
        "prime_selfhost": prime_selfhost,
        "selfhost_only": cli.selfhost_only,
            "selfhost_bootstrap": match cli.selfhost_bootstrap {
            SelfhostBootstrapArg::ArtifactOnly => "artifact-only",
            SelfhostBootstrapArg::ArtifactPreferred => "artifact-preferred",
            SelfhostBootstrapArg::Embedded => "embedded",
        },
        "coreform_frontend": cli.coreform_frontend.map(|v| match v {
            CoreformFrontendArg::Rust => "rust",
            CoreformFrontendArg::Selfhost => "selfhost",
        }),
        "selfhost_artifact": cli.selfhost_artifact.as_ref().map(|p| p.display().to_string()),
        "cwd": cwd,
        "inherited": inherited,
    });
    let canon = json_canonical_string(&payload);
    let mut h = blake3::Hasher::new();
    h.update(b"GCv0.2\0warm-cache-key\0");
    h.update(canon.as_bytes());
    h.finalize().to_hex().to_string()
}

fn cmd_warm(cli: &Cli, flavor: Flavor, prime_selfhost: bool) -> Result<CmdOut, CliError> {
    if prime_selfhost {
        let frontend = resolved_coreform_frontend(cli)?;
        if matches!(frontend, gc_obligations::CoreformFrontend::Selfhost(_)) {
            let mut ctx = mk_ctx(cli);
            let prelude = build_prelude(&mut ctx);
            let mut env = prelude.env;
            load_selfhost_toolchain(cli, &mut ctx, &mut env)?;
        }
    }

    let inherited = inherited_global_args(cli);
    let session_cache_key = warm_session_cache_key(cli, flavor, prime_selfhost, &inherited);
    let mut handled: u64 = 0;
    for line in io::stdin().lock().lines() {
        let line = line.map_err(|e| cli_err(EX_IO, "io/error", format!("{e}")))?;
        let trimmed = line.trim();
        if trimmed.is_empty() {
            continue;
        }

        let req: WarmRequest = match serde_json::from_str(trimmed) {
            Ok(r) => r,
            Err(e) => {
                emit_warm_line(&serde_json::json!({
                    "ok": false,
                    "kind": "genesis/warm-response-v0.1",
                    "error": { "code": "warm/request-parse", "message": format!("{e}") },
                    "data": {
                        "request_index": handled,
                        "session_cache_key": session_cache_key
                    }
                }))?;
                handled = handled.saturating_add(1);
                continue;
            }
        };

        if req.argv.len() == 1 && matches!(req.argv[0].as_str(), "exit" | "quit" | "stop") {
            break;
        }
        if req.argv.first().map(|s| s.as_str()) == Some("warm") {
            emit_warm_line(&serde_json::json!({
                "ok": false,
                "kind": "genesis/warm-response-v0.1",
                "error": { "code": "warm/nested", "message": "nested warm command is not allowed" },
                "data": {
                    "request_index": handled,
                    "session_cache_key": session_cache_key
                }
            }))?;
            handled = handled.saturating_add(1);
            continue;
        }

        let argv: Vec<String> = std::iter::once("genesis".to_string())
            .chain(inherited.iter().cloned())
            .chain(req.argv.iter().cloned())
            .collect();
        let sub_cli = match Cli::try_parse_from(argv) {
            Ok(c) => c,
            Err(e) => {
                emit_warm_line(&serde_json::json!({
                    "ok": false,
                    "kind": "genesis/warm-response-v0.1",
                    "error": { "code": "warm/request-argv-parse", "message": e.to_string() },
                    "data": {
                        "request_index": handled,
                        "session_cache_key": session_cache_key
                    }
                }))?;
                handled = handled.saturating_add(1);
                continue;
            }
        };

        match dispatch(&sub_cli, flavor) {
            Ok(out) => {
                emit_warm_line(&serde_json::json!({
                    "ok": true,
                    "kind": "genesis/warm-response-v0.1",
                    "data": {
                        "request_index": handled,
                        "session_cache_key": session_cache_key,
                        "exit_code": out.exit_code,
                        "result": out.json
                    }
                }))?;
            }
            Err(e) => {
                emit_warm_line(&serde_json::json!({
                    "ok": false,
                    "kind": "genesis/warm-response-v0.1",
                    "error": e.json,
                    "data": {
                        "request_index": handled,
                        "session_cache_key": session_cache_key,
                        "exit_code": e.exit_code
                    }
                }))?;
            }
        }
        handled = handled.saturating_add(1);
    }

    let env = JsonEnvelope {
        ok: true,
        kind: "genesis/warm-session-v0.1",
        data: Some(serde_json::json!({
            "requests_handled": handled,
            "prime_selfhost": prime_selfhost,
            "session_cache_key": session_cache_key
        })),
        error: None,
    };
    Ok(CmdOut {
        exit_code: EX_OK,
        stdout: String::new(),
        json: serde_json::to_value(env).expect("json"),
    })
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

    let (v, eval_backend) = eval_module_default(&mut ctx, &mut env, &forms)
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
            "kernel_eval_backend": eval_backend.as_str(),
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

    let (_, eval_backend) = eval_module_default(&mut ctx, &mut env, &forms)
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
            "kernel_eval_backend": eval_backend.as_str(),
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

    let (prog, eval_backend) = eval_module_default(&mut ctx, &mut env, &forms)
        .map_err(|e| cli_err(EX_EVAL, "eval/error", format!("{e}")))?;

    let toolchain = match flavor {
        Flavor::Native => format!("genesis/{} (native)", env!("CARGO_PKG_VERSION")),
        Flavor::Wasi => format!("genesis_wasi/{} (wasi)", env!("CARGO_PKG_VERSION")),
    };
    let r = gc_effects::run(&mut ctx, &policy, prog, program_hash, toolchain)
        .map_err(|e| cli_err(EX_EVAL, "effects/run", format!("{e}")))?;
    enforce_no_legacy_semantic_fallback_in_selfhost_only(cli, "run", &r.log)?;

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
            "kernel_eval_backend": eval_backend.as_str(),
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

    let (prog, eval_backend) = eval_module_default(&mut ctx, &mut env, &forms)
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
            "kernel_eval_backend": eval_backend.as_str(),
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
            "kernel_eval_backend_default": "compiled-with-treewalk-fallback",
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
        selfhost_routed: true,
        default_selfhost: true,
    },
    SelfhostCutoverRow {
        cmd: "sign",
        fast_path_required: false,
        selfhost_routed: true,
        default_selfhost: true,
    },
    SelfhostCutoverRow {
        cmd: "transparency-verify",
        fast_path_required: false,
        selfhost_routed: true,
        default_selfhost: true,
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
        selfhost_routed: true,
        default_selfhost: true,
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
        selfhost_routed: true,
        default_selfhost: true,
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
    let bootstrap_mode = maybe_embedded_bootstrap_mode();
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

    let stage2_seed_index = bootstrap_artifact
        .as_deref()
        .and_then(load_stage2_seed_index);
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

    let toolchain_sources = selfhost_coreform_toolchain_v1_sources()
        .map_err(|e| cli_err(EX_INTERNAL, "selfhost/sources", format!("{e}")))?;
    for (path, src) in toolchain_sources {
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
                (TermOrdKey(Term::symbol(":path")), Term::Str(path.clone())),
                (TermOrdKey(Term::symbol(":source")), Term::Str(src.clone())),
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
            let planned =
                selfhost_plan_request_map(cli, "core/cli::keygen-request", req, "keygen")?;
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
    let frontend_info = coreform_frontend_for_engine_json(cli, engine)?;
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
                    (
                        TermOrdKey(Term::symbol(":scan-store")),
                        Term::Bool(scan_store),
                    ),
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
    use super::{
        EX_PARSE, EX_VERIFY, SelfhostBootstrapMode, enforce_bootstrap_mode_allowed_with_flag,
        json_canonical_string, parse_sync_set_refs,
    };
    use crate::cmd_vcs::parse_set_ref_spec;

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
    fn json_canonical_string_sorts_object_keys_recursively() {
        let value = serde_json::json!({
            "z": 1,
            "a": {
                "y": 2,
                "x": [{"b": 1, "a": 2}]
            }
        });
        let s = json_canonical_string(&value);
        assert_eq!(s, r#"{"a":{"x":[{"a":2,"b":1}],"y":2},"z":1}"#);
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

    #[test]
    fn non_artifact_bootstrap_mode_is_dev_only() {
        let err = enforce_bootstrap_mode_allowed_with_flag(
            SelfhostBootstrapMode::Embedded,
            "test",
            false,
        )
        .expect_err("embedded bootstrap should be rejected outside development mode");
        assert_eq!(err.exit_code, EX_VERIFY);
        assert!(err.json.message.contains("development-only"));
        enforce_bootstrap_mode_allowed_with_flag(SelfhostBootstrapMode::Embedded, "test", true)
            .expect("embedded bootstrap should be allowed in development mode");
    }
}
