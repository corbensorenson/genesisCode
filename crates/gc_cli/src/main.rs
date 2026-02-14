use std::path::PathBuf;

use anyhow::{Context, anyhow};
use clap::{Parser, Subcommand};

use gc_coreform::{Term, canonicalize_module, parse_module, parse_term, print_module, print_term};
use gc_effects::{CapsPolicy, EffectLog};
use gc_kernel::{Apply, EvalCtx, Value, eval_module, eval_term};
use gc_obligations::PackageManifest;
use gc_prelude::build_prelude;

#[derive(Parser)]
#[command(name = "genesis", version)]
struct Cli {
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
}

fn main() -> anyhow::Result<()> {
    let cli = Cli::parse();
    match cli.cmd {
        Cmd::Fmt { file, check } => cmd_fmt(&file, check),
        Cmd::Eval { file } => cmd_eval(&file),
        Cmd::Explain {
            file,
            contract,
            msg,
        } => cmd_explain(&file, &contract, &msg),
        Cmd::Run { file, caps, log } => cmd_run(&file, &caps, log.as_deref()),
        Cmd::Replay { file, log } => cmd_replay(&file, &log),
        Cmd::Test { pkg, caps } => cmd_test(&pkg, caps.as_deref()),
        Cmd::Pack { pkg } => cmd_pack(&pkg),
        Cmd::Typecheck { pkg } => cmd_typecheck(&pkg),
        Cmd::Optimize { file, out } => cmd_optimize(&file, out.as_ref()),
        Cmd::ApplyPatch { patch, pkg, caps } => cmd_apply_patch(&patch, &pkg, caps.as_deref()),
    }
}

fn cmd_fmt(file: &PathBuf, check: bool) -> anyhow::Result<()> {
    let src = std::fs::read_to_string(file).with_context(|| format!("read {}", file.display()))?;
    let forms = parse_module(&src).map_err(|e| anyhow!(e))?;
    let canon = canonicalize_module(forms)?;
    let out = print_module(&canon);

    if check {
        if normalize_newlines(&src) != normalize_newlines(&out) {
            return Err(anyhow!("{} is not canonically formatted", file.display()));
        }
        return Ok(());
    }

    std::fs::write(file, out).with_context(|| format!("write {}", file.display()))?;
    Ok(())
}

fn cmd_eval(file: &PathBuf) -> anyhow::Result<()> {
    let src = std::fs::read_to_string(file).with_context(|| format!("read {}", file.display()))?;
    let forms = parse_module(&src).map_err(|e| anyhow!(e))?;
    let forms = canonicalize_module(forms)?;

    let mut ctx = EvalCtx::new();
    let prelude = build_prelude(&mut ctx);
    let mut env = prelude.env;

    let v = eval_module(&mut ctx, &mut env, &forms).map_err(|e| anyhow!("eval error: {}", e))?;
    println!("{}", render_value(&v));
    Ok(())
}

fn cmd_explain(file: &PathBuf, contract_src: &str, msg_src: &str) -> anyhow::Result<()> {
    let src = std::fs::read_to_string(file).with_context(|| format!("read {}", file.display()))?;
    let forms = parse_module(&src).map_err(|e| anyhow!(e))?;
    let forms = canonicalize_module(forms)?;

    let mut ctx = EvalCtx::new();
    let prelude = build_prelude(&mut ctx);
    let mut env = prelude.env;

    // Evaluate the module to populate env.
    eval_module(&mut ctx, &mut env, &forms).map_err(|e| anyhow!("eval error: {}", e))?;

    let contract_term = parse_term(contract_src).map_err(|e| anyhow!("parse --contract: {e}"))?;
    let contract =
        eval_term(&mut ctx, &env, &contract_term).map_err(|e| anyhow!("eval --contract: {e}"))?;

    let msg_term = parse_term(msg_src).map_err(|e| anyhow!("parse --msg: {e}"))?;
    let msg_val = Value::Data(msg_term);

    let explain = env
        .get("core/contract::explain")
        .ok_or_else(|| anyhow!("missing prelude binding core/contract::explain"))?;
    let r = explain
        .apply(&mut ctx, contract)?
        .apply(&mut ctx, msg_val)
        .map_err(|e| anyhow!("explain failed: {e}"))?;

    println!("{}", render_value(&r));
    Ok(())
}

fn cmd_run(
    file: &std::path::Path,
    caps: &std::path::Path,
    log: Option<&std::path::Path>,
) -> anyhow::Result<()> {
    let src = std::fs::read_to_string(file).with_context(|| format!("read {}", file.display()))?;
    let forms = parse_module(&src).map_err(|e| anyhow!(e))?;
    let forms = canonicalize_module(forms)?;
    let program_hash = gc_coreform::hash_module(&forms);

    let policy = CapsPolicy::load(caps).with_context(|| format!("read {}", caps.display()))?;

    let mut ctx = EvalCtx::new();
    let prelude = build_prelude(&mut ctx);
    let mut env = prelude.env;

    let prog = eval_module(&mut ctx, &mut env, &forms).map_err(|e| anyhow!("eval error: {}", e))?;

    let toolchain = format!("genesis {}", env!("CARGO_PKG_VERSION"));
    let r = gc_effects::run(&mut ctx, &policy, prog, program_hash, toolchain)
        .map_err(|e| anyhow!("run failed: {e}"))?;

    let log_path = log
        .map(|p| p.to_path_buf())
        .unwrap_or_else(|| file.with_extension("gclog"));
    std::fs::write(&log_path, r.log.to_string_canonical() + "\n")
        .with_context(|| format!("write {}", log_path.display()))?;

    println!("{}", render_value(&r.value));
    Ok(())
}

fn cmd_replay(file: &PathBuf, log_path: &PathBuf) -> anyhow::Result<()> {
    let src = std::fs::read_to_string(file).with_context(|| format!("read {}", file.display()))?;
    let forms = parse_module(&src).map_err(|e| anyhow!(e))?;
    let forms = canonicalize_module(forms)?;
    let program_hash = gc_coreform::hash_module(&forms);

    let log_src = std::fs::read_to_string(log_path)
        .with_context(|| format!("read {}", log_path.display()))?;
    let log_term = gc_coreform::parse_term(&log_src).map_err(|e| anyhow!("parse log: {e}"))?;
    let log = EffectLog::from_term(&log_term).map_err(|e| anyhow!("bad log: {e}"))?;
    if log.program_hash != program_hash {
        return Err(anyhow!(
            "program hash mismatch: log is for different program"
        ));
    }

    let mut ctx = EvalCtx::new();
    let prelude = build_prelude(&mut ctx);
    let mut env = prelude.env;

    let prog = eval_module(&mut ctx, &mut env, &forms).map_err(|e| anyhow!("eval error: {}", e))?;
    let v = gc_effects::replay(&mut ctx, prog, &log).map_err(|e| anyhow!("replay failed: {e}"))?;
    println!("{}", render_value(&v));
    Ok(())
}

fn cmd_test(pkg: &std::path::Path, caps: Option<&std::path::Path>) -> anyhow::Result<()> {
    let r = gc_obligations::test_package(pkg, caps).map_err(|e| anyhow!("{e}"))?;
    println!("{}", r.acceptance_artifact);
    if !r.ok {
        return Err(anyhow!("package obligations failed"));
    }
    Ok(())
}

fn cmd_pack(pkg: &std::path::Path) -> anyhow::Result<()> {
    let h = gc_obligations::pack(pkg).map_err(|e| anyhow!("{e}"))?;
    println!("{h}");
    Ok(())
}

fn cmd_typecheck(pkg: &std::path::Path) -> anyhow::Result<()> {
    let (manifest, pkg_dir) = PackageManifest::load(pkg).map_err(|e| anyhow!("{e}"))?;

    let mut mods = Vec::new();
    for m in &manifest.modules {
        let abs = pkg_dir.join(&m.path);
        let src =
            std::fs::read_to_string(&abs).with_context(|| format!("read {}", abs.display()))?;
        let forms = parse_module(&src).map_err(|e| anyhow!(e))?;
        let forms = canonicalize_module(forms)?;
        let meta = extract_meta_static(&forms);
        mods.push(gc_types::ModuleForTypecheck {
            path: m.path.clone(),
            forms,
            meta,
        });
    }

    let report = gc_types::typecheck_package(&mods);
    println!("{}", print_term(&report.to_term()));
    if !report.ok {
        return Err(anyhow!("typecheck failed"));
    }
    Ok(())
}

fn cmd_optimize(file: &PathBuf, out: Option<&PathBuf>) -> anyhow::Result<()> {
    let src = std::fs::read_to_string(file).with_context(|| format!("read {}", file.display()))?;
    let forms = parse_module(&src).map_err(|e| anyhow!(e))?;
    let forms = canonicalize_module(forms)?;
    let opt = gc_opt::optimize_module(&forms);
    let opt = canonicalize_module(opt)?;
    let out_s = print_module(&opt);
    match out {
        Some(p) => std::fs::write(p, out_s).with_context(|| format!("write {}", p.display()))?,
        None => print!("{out_s}"),
    }
    Ok(())
}

fn cmd_apply_patch(
    patch: &std::path::Path,
    pkg: &std::path::Path,
    caps: Option<&std::path::Path>,
) -> anyhow::Result<()> {
    let r = gc_patches::apply_patch(patch, pkg, caps).map_err(|e| anyhow!("{e}"))?;
    println!("{}", r.report_artifact);
    if !r.ok {
        return Err(anyhow!("patch applied but obligations failed"));
    }
    Ok(())
}

fn normalize_newlines(s: &str) -> String {
    s.replace("\r\n", "\n")
}

fn render_value(v: &Value) -> String {
    match v {
        Value::Data(t) => print_term(t),
        Value::Vector(_) | Value::Map(_) => print_term(&v.to_term_for_log(None)),
        _ => v.debug_repr(),
    }
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
