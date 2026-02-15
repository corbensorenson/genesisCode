use std::path::{Path, PathBuf};

use anyhow::Context;
use clap::{Parser, Subcommand};
use serde::Serialize;

use gc_coreform::{canonicalize_module, hash_module, parse_module, parse_term, print_module};
use gc_effects::{
    ArtifactStore, CapsPolicy, Decision, EffectLog, EffectsError, replay_with_store, run,
};
use gc_kernel::{EvalCtx, MemLimits, StepLimit, Value, eval_module};
use gc_prelude::build_prelude;

const EX_OK: u8 = 0;
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

    /// Run an effect program with a deny-by-default capability policy.
    Run {
        file: PathBuf,
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

    /// GenesisGraph pure ops subset.
    Vcs {
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
        /// Root hash (hex). For shallow bundles this is a snapshot hash; for full bundles this is usually a commit hash.
        #[arg(long)]
        snapshot: String,

        /// Output bundle path (relative to capability base_dir).
        #[arg(long)]
        out: PathBuf,

        /// Export a full-history bundle from the root hash (commit closure + snapshots + patches + evidence).
        #[arg(long)]
        full: bool,

        /// Parent depth when the root is a commit hash (0 = no parents).
        #[arg(long, default_value_t = 0)]
        depth: u64,

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
        /// Format: `<refname>=<commit-hash>` (hash may be `nil` to delete).
        #[arg(long = "set-ref")]
        set_refs: Vec<String>,

        /// Policy artifact hash (hex) used by the local refs/set gate (required when using --set-ref).
        #[arg(long)]
        policy: Option<String>,
    },
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

fn cmd_run(
    cli: &Cli,
    file: &PathBuf,
    caps: &Path,
    log_path: &Option<PathBuf>,
) -> Result<CmdOut, CliError> {
    let src = std::fs::read_to_string(file)
        .with_context(|| format!("read {}", file.display()))
        .map_err(|e| cli_err(EX_IO, "io/read", format!("{e}")))?;
    let forms =
        parse_module(&src).map_err(|e| cli_err(EX_PARSE, "parse/coreform", e.to_string()))?;
    let forms = canonicalize_module(forms)
        .map_err(|e| cli_err(EX_PARSE, "canon/coreform", e.to_string()))?;

    let policy = CapsPolicy::load(caps)
        .with_context(|| format!("read {}", caps.display()))
        .map_err(|e| cli_err(EX_PARSE, "caps/parse", format!("{e}")))?;

    let mut ctx = mk_ctx(cli);
    let prelude = build_prelude(&mut ctx);
    let mut env = prelude.env;

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
    log_path: &PathBuf,
    store: &Option<PathBuf>,
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

    let mut ctx = mk_ctx(cli);
    let prelude = build_prelude(&mut ctx);
    let mut env = prelude.env;
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

    let mut out = Vec::new();
    for s in specs {
        let (name, hash) = s.split_once('=').ok_or_else(|| {
            cli_err(
                EX_PARSE,
                "pkg/import",
                "set-ref must be <refname>=<commit-hash|nil>".to_string(),
            )
        })?;
        let name = name.trim();
        let hash = hash.trim();
        if name.is_empty() || hash.is_empty() {
            return Err(cli_err(
                EX_PARSE,
                "pkg/import",
                "set-ref fields must be non-empty".to_string(),
            ));
        }
        if hash != "nil" && !is_hex64(hash) {
            return Err(cli_err(
                EX_PARSE,
                "pkg/import",
                "set-ref hash must be 64-hex or `nil`".to_string(),
            ));
        }
        out.push(SetRefSpec {
            name: name.to_string(),
            hash: hash.to_string(),
            policy: pol.to_string(),
            expected_old: None,
        });
    }
    Ok(out)
}

fn mk_local_set_refs_chain(set_refs: &[SetRefSpec], imp: gc_coreform::Term) -> gc_coreform::Term {
    let mut body = gc_coreform::Term::list(vec![
        gc_coreform::Term::symbol("core/effect::pure"),
        imp.clone(),
    ]);
    for sr in set_refs.iter().rev() {
        let op = gc_coreform::Term::list(vec![
            gc_coreform::Term::symbol("quote"),
            gc_coreform::Term::symbol("core/refs::set"),
        ]);
        let mut m = std::collections::BTreeMap::new();
        m.insert(
            gc_coreform::TermOrdKey(gc_coreform::Term::symbol(":name")),
            gc_coreform::Term::Str(sr.name.clone()),
        );
        m.insert(
            gc_coreform::TermOrdKey(gc_coreform::Term::symbol(":hash")),
            if sr.hash == "nil" {
                gc_coreform::Term::Nil
            } else {
                gc_coreform::Term::Str(sr.hash.clone())
            },
        );
        m.insert(
            gc_coreform::TermOrdKey(gc_coreform::Term::symbol(":policy")),
            gc_coreform::Term::Str(sr.policy.clone()),
        );
        if let Some(exp) = &sr.expected_old {
            m.insert(
                gc_coreform::TermOrdKey(gc_coreform::Term::symbol(":expected-old")),
                if exp == "nil" {
                    gc_coreform::Term::Nil
                } else {
                    gc_coreform::Term::Str(exp.clone())
                },
            );
        }
        let payload = gc_coreform::Term::Map(m);
        let k = gc_coreform::Term::list(vec![
            gc_coreform::Term::symbol("fn"),
            gc_coreform::Term::list(vec![gc_coreform::Term::symbol("_")]),
            body,
        ]);
        body = gc_coreform::Term::list(vec![
            gc_coreform::Term::symbol("core/effect::perform"),
            op,
            payload,
            k,
        ]);
    }
    body
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

fn mk_pkg_lock_program(lock: &Path) -> Vec<gc_coreform::Term> {
    let op = gc_coreform::Term::list(vec![
        gc_coreform::Term::symbol("quote"),
        gc_coreform::Term::symbol("core/pkg::lock"),
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

fn mk_gpk_export_program(
    root: &str,
    out: &Path,
    full: bool,
    depth: u64,
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
    let payload = gc_coreform::Term::Map(
        [(
            gc_coreform::TermOrdKey(gc_coreform::Term::symbol(":in")),
            gc_coreform::Term::Str(input.display().to_string()),
        )]
        .into_iter()
        .collect(),
    );
    let k_body = mk_local_set_refs_chain(set_refs, gc_coreform::Term::symbol("imp"));
    let k = gc_coreform::Term::list(vec![
        gc_coreform::Term::symbol("fn"),
        gc_coreform::Term::list(vec![gc_coreform::Term::symbol("imp")]),
        k_body,
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
        PkgCmd::Export {
            snapshot,
            out,
            full,
            depth,
            include_refs,
        } => (
            mk_gpk_export_program(snapshot, out, *full, *depth, include_refs),
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

fn cmd_vcs_hash(cli: &Cli, input: &PathBuf) -> Result<CmdOut, CliError> {
    let src = std::fs::read_to_string(input)
        .with_context(|| format!("read {}", input.display()))
        .map_err(|e| cli_err(EX_IO, "io/read", format!("{e}")))?;

    // Prefer parsing as module (more common for `.gc` files).
    let h = match parse_module(&src) {
        Ok(forms) => {
            let canon = canonicalize_module(forms)
                .map_err(|e| cli_err(EX_PARSE, "canon/coreform", e.to_string()))?;
            hash_module(&canon)
        }
        Err(_) => {
            let t =
                parse_term(&src).map_err(|e| cli_err(EX_PARSE, "parse/coreform", e.to_string()))?;
            // Hash terms using the same scheme as the canonical term hasher.
            gc_coreform::hash_term(&t)
        }
    };

    let hex = hex32(h);

    let env = JsonEnvelope {
        ok: true,
        kind: "genesis/vcs-hash-v0.2",
        data: Some(serde_json::json!({
            "in": input.display().to_string(),
            "hash": hex,
            "hash_format": "hex",
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
    match &cli.cmd {
        Cmd::Fmt { file, check } => cmd_fmt(cli, file, *check),
        Cmd::Eval { file } => cmd_eval(cli, file),
        Cmd::Test { pkg, caps } => cmd_test(cli, pkg.as_path(), caps.as_deref()),
        Cmd::Pack { pkg } => cmd_pack(cli, pkg.as_path()),
        Cmd::Run { file, caps, log } => cmd_run(cli, file, caps.as_path(), log),
        Cmd::Replay { file, log, store } => cmd_replay(cli, file, log, store),
        Cmd::Store { caps, log, cmd } => cmd_store(cli, caps.as_path(), log, cmd),
        Cmd::Refs { caps, log, cmd } => cmd_refs(cli, caps.as_path(), log, cmd),
        Cmd::Pkg { caps, log, cmd } => cmd_pkg(cli, caps.as_path(), log, cmd),
        Cmd::Vcs { cmd } => match cmd {
            VcsCmd::Hash { input } => cmd_vcs_hash(cli, input),
        },
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
