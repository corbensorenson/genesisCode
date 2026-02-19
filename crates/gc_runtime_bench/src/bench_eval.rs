use anyhow::{Context, Result};
use gc_coreform::{canonicalize_module, parse_module};
use gc_kernel::{EvalCtx, compile_module, eval_compiled_module};
use gc_prelude::build_prelude;

use crate::config::BenchConfig;
use crate::measure::best_of;

pub fn run(cfg: &BenchConfig) -> Result<u128> {
    let src = "(def bench/eval-x 41)\n(prim int/add bench/eval-x 1)\n";
    let forms = canonicalize_module(parse_module(src).context("parse eval benchmark module")?)
        .context("canonicalize eval benchmark module")?;
    let compiled = compile_module(&forms).context("compile eval benchmark module")?;

    best_of(cfg.warmups, cfg.repeats, || {
        let mut ctx = EvalCtx::with_step_limit(None);
        let prelude = build_prelude(&mut ctx);
        let mut env = prelude.env;
        let _ = eval_compiled_module(&mut ctx, &mut env, &compiled)
            .context("eval compiled benchmark module")?;
        Ok(())
    })
}
