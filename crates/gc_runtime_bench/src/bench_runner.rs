use anyhow::{Context, Result};
use gc_coreform::{canonicalize_module, hash_module, parse_module};
use gc_effects::{CapsPolicy, run};
use gc_kernel::{EvalCtx, eval_module};
use gc_prelude::build_prelude;

use crate::config::BenchConfig;
use crate::measure::best_of;

pub fn run_effect_runner(cfg: &BenchConfig) -> Result<u128> {
    let src = "(def bench/prog (core/effect::perform 'sys/time::now nil (fn (r) (core/effect::pure r))))\nbench/prog\n";
    let forms = canonicalize_module(parse_module(src).context("parse runner benchmark module")?)
        .context("canonicalize runner benchmark module")?;
    let program_hash = hash_module(&forms);
    let policy =
        CapsPolicy::from_toml_str("allow = [\"sys/time::now\"]").context("parse runner policy")?;

    best_of(cfg.warmups, cfg.repeats, || {
        let mut ctx = EvalCtx::with_step_limit(None);
        let prelude = build_prelude(&mut ctx);
        let mut env = prelude.env;
        let program = eval_module(&mut ctx, &mut env, &forms).context("eval runner module")?;
        let _ = run(
            &mut ctx,
            &policy,
            program,
            program_hash,
            "runtime-bench".to_string(),
        )
        .context("run effect program")?;
        Ok(())
    })
}
