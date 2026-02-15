use std::path::PathBuf;

use anyhow::Context;
use clap::{Parser, Subcommand};
use serde::Serialize;

use gc_coreform::{canonicalize_module, hash_module, parse_module, parse_term, print_module};
use gc_kernel::{EvalCtx, MemLimits, StepLimit, Value, eval_module};
use gc_prelude::build_prelude;

const EX_OK: u8 = 0;
const EX_PARSE: u8 = 10;
const EX_FMT: u8 = 11;
const EX_EVAL: u8 = 20;
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

fn normalize_newlines(s: &str) -> String {
    s.replace("\r\n", "\n")
}

fn render_value_for_cli(ctx: &EvalCtx, v: &Value) -> (String, &'static str) {
    let protocol_error = ctx.protocol.map(|p| p.error);
    let t = v.to_term_for_log(protocol_error);
    (gc_coreform::print_term(&t), "coreform/term")
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

    let hex = {
        const LUT: &[u8; 16] = b"0123456789abcdef";
        let mut out = String::with_capacity(64);
        for b in h {
            out.push(LUT[(b >> 4) as usize] as char);
            out.push(LUT[(b & 0x0f) as usize] as char);
        }
        out
    };

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
