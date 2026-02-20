use std::collections::{BTreeMap, BTreeSet};
use std::io::{self, BufRead, Write};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU8, Ordering};

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

mod cli_json;
mod cmd_commit;
mod cmd_core;
mod cmd_gc;
mod cmd_pkg;
mod cmd_policy;
mod cmd_refs;
mod cmd_security_ops;
mod cmd_selfhost;
mod cmd_store;
mod cmd_sync;
mod cmd_vcs;
mod commit_contract;
mod diagnostics;
mod gc_contract;
mod kernel_exec;
mod pkg_abi;
mod pkg_contract;
mod pkg_doctor;
mod pkg_reports;
mod pkg_self_opt;
mod pkg_task_runner;
mod pkg_telemetry;
mod pkg_workspace_ops;
mod policy_config;
mod program_builders;
mod refs_contract;
mod selfhost_bridge;
mod selfhost_frontend;
mod sync_contract;
mod vcs_contract;

use cli_json::*;
use cmd_commit::cmd_commit;
use cmd_core::*;
use cmd_gc::cmd_gc;
use cmd_pkg::cmd_pkg;
use cmd_policy::cmd_policy;
use cmd_refs::cmd_refs;
use cmd_security_ops::*;
use cmd_selfhost::*;
use cmd_store::cmd_store;
use cmd_sync::cmd_sync;
pub(crate) use cmd_vcs::SetRefSpec;
use cmd_vcs::{
    cmd_vcs, extract_pkg_export_bundle_hash, extract_pkg_import_root, extract_pkg_lock_hash,
    extract_pkg_ok_bool, extract_pkg_publish_commit, extract_pkg_snapshot_hash,
    extract_refs_get_hash, extract_refs_list_pairs, extract_refs_set_hash, is_hex64,
    normalize_pkg_add_strategy, parse_local_set_refs, parse_pkg_spec, parse_sync_set_refs,
};
use diagnostics::annotate_envelope;
use kernel_exec::eval_module_default;
use policy_config::*;
use program_builders::*;
use selfhost_bridge::*;
use selfhost_frontend::*;

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

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RuntimeProfile {
    Production,
    ParityHarness,
}

static RUNTIME_PROFILE: AtomicU8 = AtomicU8::new(0);

pub(crate) fn runtime_profile() -> RuntimeProfile {
    match RUNTIME_PROFILE.load(Ordering::Relaxed) {
        1 => RuntimeProfile::ParityHarness,
        _ => RuntimeProfile::Production,
    }
}

fn set_runtime_profile(profile: RuntimeProfile) {
    let encoded = match profile {
        RuntimeProfile::Production => 0,
        RuntimeProfile::ParityHarness => 1,
    };
    RUNTIME_PROFILE.store(encoded, Ordering::Relaxed);
}

pub fn run(flavor: Flavor) -> std::process::ExitCode {
    run_with_profile(flavor, RuntimeProfile::Production)
}

pub fn run_with_profile(flavor: Flavor, profile: RuntimeProfile) -> std::process::ExitCode {
    set_runtime_profile(profile);
    let parity = matches!(profile, RuntimeProfile::ParityHarness);
    gc_prelude::set_bootstrap_runtime_profile_parity_harness(parity);
    gc_obligations::set_frontend_runtime_profile_parity_harness(parity);
    gc_effects::set_force_wasi_remote_profile(matches!(flavor, Flavor::Wasi));
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
                let out = match json_envelope_value(JsonEnvelope::<serde_json::Value> {
                    ok: false,
                    kind: "genesis/error-v0.2",
                    data: None,
                    error: Some(e.json),
                }) {
                    Ok(v) => v,
                    Err(serr) => serde_json::json!({
                        "ok": false,
                        "kind": "genesis/error-v0.2",
                        "error": {
                            "code": serr.json.code,
                            "message": serr.json.message,
                            "context": serr.json.context,
                        },
                    }),
                };
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
        Cmd::SemanticEdit { cmd } => cmd_semantic_edit(cli, cmd),
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
        Cmd::Commit { caps, log, cmd } => cmd_commit(cli, caps, log.as_deref(), cmd),
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

fn cli_err_anyhow(exit_code: u8, code: &'static str, err: anyhow::Error) -> CliError {
    // Preserve the full anyhow chain so JSON diagnostics show the real root cause.
    cli_err(exit_code, code, format!("{err:#}"))
}

fn caps_parse_cli_err(err: anyhow::Error) -> CliError {
    cli_err_anyhow(EX_PARSE, "caps/parse", err)
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
            load_runtime_selfhost_toolchain(cli, &mut ctx, &mut env)?;
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
        json: json_envelope_value(env)?,
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
            "kernel_eval_backend_default": "compiled",
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
        json: json_envelope_value(env)?,
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
        json: json_envelope_value(env)?,
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
        cmd: "semantic-edit",
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
