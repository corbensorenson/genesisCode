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

        /// Scan the entire evidence store and verify name->content hashes (can be slow).
        #[arg(long)]
        scan_store: bool,
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
        Cmd::Typecheck { pkg } => cmd_typecheck(cli, pkg),
        Cmd::Optimize { file, out } => cmd_optimize(cli, file, out.as_ref()),
        Cmd::ApplyPatch { patch, pkg, caps } => cmd_apply_patch(cli, patch, pkg, caps.as_deref()),
        Cmd::Verify {
            pkg,
            acceptance,
            scan_store,
        } => cmd_verify(cli, pkg, acceptance.as_deref(), *scan_store),
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

    let opt = gc_opt::optimize_module(&forms);
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
    scan_store: bool,
) -> Result<CmdOut, CliError> {
    let r = gc_obligations::verify_package(pkg, acceptance, scan_store).map_err(obligation_err)?;
    let exit_code = if r.ok { EX_OK } else { EX_VERIFY };

    let env = JsonEnvelope {
        ok: r.ok,
        kind: "genesis/verify-v0.2",
        data: Some(serde_json::json!({
            "pkg": pkg.display().to_string(),
            "acceptance_artifact": r.acceptance_artifact,
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
