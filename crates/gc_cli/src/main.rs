use std::path::{Path, PathBuf};

use anyhow::Context;
use clap::{Parser, Subcommand};
use serde::Serialize;

use gc_coreform::{Term, canonicalize_module, hash_module, parse_module, parse_term, print_module};
use gc_effects::{CapsPolicy, Decision, EffectLog};
use gc_kernel::{Apply, EvalCtx, MemLimits, SealId, StepLimit, Value, eval_module, eval_term};
use gc_obligations::PackageManifest;
use gc_prelude::build_prelude;

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

    #[command(subcommand)]
    cmd: Cmd,
}

#[derive(Subcommand)]
enum Cmd {
    /// Canonical formatting for CoreForm (.gc) files.
    Fmt {
        file: PathBuf,
        /// Fail if the file is not already canonically formatted.
        #[arg(long)]
        check: bool,
    },

    /// Evaluate a CoreForm program/module (pure).
    Eval { file: PathBuf },

    /// Explain contract dispatch path for a given message.
    Explain {
        file: PathBuf,
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
}

#[derive(Subcommand)]
enum StoreCmd {
    /// Store a CoreForm artifact datum and return its content hash.
    Put {
        /// Input file containing a single CoreForm term.
        #[arg(long)]
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
        /// Snapshot hash (hex).
        #[arg(long)]
        snapshot: String,
        /// Output bundle path (relative to capability base_dir).
        #[arg(long)]
        out: PathBuf,
    },

    /// Import a `.gpk` bundle into the local store.
    Import {
        /// Input bundle path (relative to capability base_dir).
        #[arg(long)]
        input: PathBuf,
    },
}

fn main() -> std::process::ExitCode {
    let cli = Cli::parse();
    match dispatch(&cli) {
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

fn dispatch(cli: &Cli) -> Result<CmdOut, CliError> {
    match &cli.cmd {
        Cmd::Fmt { file, check } => cmd_fmt(cli, file, *check),
        Cmd::Eval { file } => cmd_eval(cli, file),
        Cmd::Explain {
            file,
            contract,
            msg,
        } => cmd_explain(cli, file, contract, msg),
        Cmd::Run { file, caps, log } => cmd_run(cli, file, caps, log.as_deref()),
        Cmd::Replay { file, log, store } => cmd_replay(cli, file, log, store.as_deref()),
        Cmd::Test { pkg, caps } => cmd_test(cli, pkg, caps.as_deref()),
        Cmd::Pack { pkg } => cmd_pack(cli, pkg),
        Cmd::Keygen { out } => cmd_keygen(cli, out),
        Cmd::Sign {
            pkg,
            key,
            acceptance,
            signatures,
        } => cmd_sign(cli, pkg, key, acceptance.as_deref(), signatures.as_deref()),
        Cmd::TransparencyVerify { pkg } => cmd_transparency_verify(cli, pkg),
        Cmd::Typecheck { pkg } => cmd_typecheck(cli, pkg),
        Cmd::Optimize { file, out } => cmd_optimize(cli, file, out.as_ref()),
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

fn cmd_fmt(_cli: &Cli, file: &PathBuf, check: bool) -> Result<CmdOut, CliError> {
    let src = std::fs::read_to_string(file)
        .with_context(|| format!("read {}", file.display()))
        .map_err(|e| cli_err(EX_IO, "io/read", format!("{e}")))?;
    let forms =
        parse_module(&src).map_err(|e| cli_err(EX_PARSE, "parse/coreform", e.to_string()))?;
    let canon = canonicalize_module(forms)
        .map_err(|e| cli_err(EX_PARSE, "canon/coreform", e.to_string()))?;
    let out = print_module(&canon);

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

fn cmd_eval(cli: &Cli, file: &PathBuf) -> Result<CmdOut, CliError> {
    let src = std::fs::read_to_string(file)
        .with_context(|| format!("read {}", file.display()))
        .map_err(|e| cli_err(EX_IO, "io/read", format!("{e}")))?;
    let forms =
        parse_module(&src).map_err(|e| cli_err(EX_PARSE, "parse/coreform", e.to_string()))?;
    let forms = canonicalize_module(forms)
        .map_err(|e| cli_err(EX_PARSE, "canon/coreform", e.to_string()))?;

    let mut ctx = mk_ctx(cli);
    let prelude = build_prelude(&mut ctx);
    let mut env = prelude.env;

    let v = eval_module(&mut ctx, &mut env, &forms)
        .map_err(|e| cli_err(EX_EVAL, "eval/error", format!("{e}")))?;

    let (value, value_format) = render_value_for_cli(&ctx, &v);
    let env = JsonEnvelope {
        ok: true,
        kind: "genesis/eval-v0.2",
        data: Some(serde_json::json!({
            "file": file.display().to_string(),
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
    contract_src: &str,
    msg_src: &str,
) -> Result<CmdOut, CliError> {
    let src = std::fs::read_to_string(file)
        .with_context(|| format!("read {}", file.display()))
        .map_err(|e| cli_err(EX_IO, "io/read", format!("{e}")))?;
    let forms =
        parse_module(&src).map_err(|e| cli_err(EX_PARSE, "parse/coreform", e.to_string()))?;
    let forms = canonicalize_module(forms)
        .map_err(|e| cli_err(EX_PARSE, "canon/coreform", e.to_string()))?;

    let mut ctx = mk_ctx(cli);
    let prelude = build_prelude(&mut ctx);
    let mut env = prelude.env;

    eval_module(&mut ctx, &mut env, &forms)
        .map_err(|e| cli_err(EX_EVAL, "eval/error", format!("{e}")))?;

    let contract_term = parse_term(contract_src)
        .map_err(|e| cli_err(EX_PARSE, "parse/term", format!("--contract: {e}")))?;
    let contract = eval_term(&mut ctx, &env, &contract_term)
        .map_err(|e| cli_err(EX_EVAL, "eval/error", format!("--contract: {e}")))?;

    let msg_term =
        parse_term(msg_src).map_err(|e| cli_err(EX_PARSE, "parse/term", format!("--msg: {e}")))?;
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

fn cmd_run(cli: &Cli, file: &Path, caps: &Path, log: Option<&Path>) -> Result<CmdOut, CliError> {
    let src = std::fs::read_to_string(file)
        .with_context(|| format!("read {}", file.display()))
        .map_err(|e| cli_err(EX_IO, "io/read", format!("{e}")))?;
    let forms =
        parse_module(&src).map_err(|e| cli_err(EX_PARSE, "parse/coreform", e.to_string()))?;
    let forms = canonicalize_module(forms)
        .map_err(|e| cli_err(EX_PARSE, "canon/coreform", e.to_string()))?;
    let program_hash = hash_module(&forms);

    let policy = CapsPolicy::load(caps)
        .with_context(|| format!("read {}", caps.display()))
        .map_err(|e| cli_err(EX_PARSE, "caps/parse", format!("{e}")))?;

    let mut ctx = mk_ctx(cli);
    let prelude = build_prelude(&mut ctx);
    let mut env = prelude.env;

    let prog = eval_module(&mut ctx, &mut env, &forms)
        .map_err(|e| cli_err(EX_EVAL, "eval/error", format!("{e}")))?;

    let toolchain = format!("genesis {}", env!("CARGO_PKG_VERSION"));
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
            "caps": caps.display().to_string(),
            "log": log_path.display().to_string(),
            "denied": denied,
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
    let policy = CapsPolicy::load(caps)
        .with_context(|| format!("read {}", caps.display()))
        .map_err(|e| cli_err(EX_PARSE, "caps/parse", format!("{e}")))?;

    let (forms, kind, log_op) = match cmd {
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

    let mut ctx = mk_ctx(cli);
    let prelude = build_prelude(&mut ctx);
    let mut env = prelude.env;
    let prog = eval_module(&mut ctx, &mut env, &forms)
        .map_err(|e| cli_err(EX_EVAL, "eval/error", format!("{e}")))?;

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
        PkgCmd::Lock { lock } => (
            mk_pkg_lock_program(lock),
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
        PkgCmd::Export { snapshot, out } => (
            mk_gpk_export_program(snapshot, out),
            "genesis/pkg-export-v0.1",
            "pkg-export",
        ),
        PkgCmd::Import { input } => (
            mk_gpk_import_program(input),
            "genesis/pkg-import-v0.1",
            "pkg-import",
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

fn mk_pkg_lock_program(lock: &Path) -> Vec<Term> {
    let op = Term::list(vec![Term::symbol("quote"), Term::symbol("core/pkg::lock")]);
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

fn mk_gpk_export_program(snapshot: &str, out: &Path) -> Vec<Term> {
    let op = Term::list(vec![
        Term::symbol("quote"),
        Term::symbol("core/gpk::export"),
    ]);
    let payload = Term::Map(
        [
            (
                gc_coreform::TermOrdKey(Term::symbol(":root")),
                Term::Str(snapshot.to_string()),
            ),
            (
                gc_coreform::TermOrdKey(Term::symbol(":out")),
                Term::Str(out.display().to_string()),
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

fn mk_gpk_import_program(input: &Path) -> Vec<Term> {
    let op = Term::list(vec![
        Term::symbol("quote"),
        Term::symbol("core/gpk::import"),
    ]);
    let payload = Term::Map(
        [(
            gc_coreform::TermOrdKey(Term::symbol(":in")),
            Term::Str(input.display().to_string()),
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
    log_path: &PathBuf,
    store_dir: Option<&Path>,
) -> Result<CmdOut, CliError> {
    let src = std::fs::read_to_string(file)
        .with_context(|| format!("read {}", file.display()))
        .map_err(|e| cli_err(EX_IO, "io/read", format!("{e}")))?;
    let forms =
        parse_module(&src).map_err(|e| cli_err(EX_PARSE, "parse/coreform", e.to_string()))?;
    let forms = canonicalize_module(forms)
        .map_err(|e| cli_err(EX_PARSE, "canon/coreform", e.to_string()))?;
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

    let mut ctx = mk_ctx(cli);
    let prelude = build_prelude(&mut ctx);
    let mut env = prelude.env;

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
    let r = gc_obligations::test_package_with_step_limit(
        pkg,
        caps,
        resolved_step_limit(cli),
        resolved_mem_limits(cli),
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
    let h = gc_obligations::pack(pkg).map_err(obligation_err)?;
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

fn cmd_keygen(cli: &Cli, out: &Path) -> Result<CmdOut, CliError> {
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
    let (manifest, pkg_dir) = PackageManifest::load(pkg)
        .map_err(|e| cli_err(EX_PARSE, "manifest/parse", format!("{e}")))?;

    let mut mods = Vec::new();
    for m in &manifest.modules {
        let abs = pkg_dir.join(&m.path);
        let src = std::fs::read_to_string(&abs)
            .with_context(|| format!("read {}", abs.display()))
            .map_err(|e| cli_err(EX_IO, "io/read", format!("{e}")))?;
        let forms =
            parse_module(&src).map_err(|e| cli_err(EX_PARSE, "parse/coreform", e.to_string()))?;
        let forms = canonicalize_module(forms)
            .map_err(|e| cli_err(EX_PARSE, "canon/coreform", e.to_string()))?;
        let meta = extract_meta_static(&forms);
        mods.push(gc_types::ModuleForTypecheck {
            path: m.path.clone(),
            forms,
            meta,
        });
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

fn cmd_optimize(cli: &Cli, file: &PathBuf, out: Option<&PathBuf>) -> Result<CmdOut, CliError> {
    let src = std::fs::read_to_string(file)
        .with_context(|| format!("read {}", file.display()))
        .map_err(|e| cli_err(EX_IO, "io/read", format!("{e}")))?;
    let forms =
        parse_module(&src).map_err(|e| cli_err(EX_PARSE, "parse/coreform", e.to_string()))?;
    let forms = canonicalize_module(forms)
        .map_err(|e| cli_err(EX_PARSE, "canon/coreform", e.to_string()))?;
    let orig_h = hash_module(&forms);

    let (opt, opt_report) = gc_opt::optimize_module_with_report(&forms);
    let opt =
        canonicalize_module(opt).map_err(|e| cli_err(EX_PARSE, "canon/coreform", e.to_string()))?;
    let opt_h = hash_module(&opt);
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
    let r = gc_patches::apply_patch_with_step_limit(
        patch,
        pkg,
        caps,
        resolved_step_limit(cli),
        resolved_mem_limits(cli),
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

fn cmd_verify(
    cli: &Cli,
    pkg: &Path,
    acceptance: Option<&str>,
    policy: Option<&Path>,
    signatures: Option<&Path>,
    scan_store: bool,
) -> Result<CmdOut, CliError> {
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

fn hex32(h: [u8; 32]) -> String {
    const HEX: &[u8; 16] = b"0123456789abcdef";
    let mut out = String::new();
    for b in h {
        out.push(HEX[(b >> 4) as usize] as char);
        out.push(HEX[(b & 0x0f) as usize] as char);
    }
    out
}
