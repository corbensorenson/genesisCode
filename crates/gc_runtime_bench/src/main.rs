mod bench_eval;
mod bench_patch;
mod bench_runner;
mod bench_store_sync;
mod config;
mod measure;
mod report;

use anyhow::Result;

use crate::bench_eval::run as bench_eval;
use crate::bench_patch::run_patch_apply;
use crate::bench_runner::run_effect_runner;
use crate::bench_store_sync::run_store_sync;
use crate::config::parse as parse_config;
use crate::report::{Metrics, Report};

fn main() -> Result<()> {
    let cfg = parse_config()?;

    let eval_ms = bench_eval(&cfg)?;
    let runner_ms = run_effect_runner(&cfg)?;
    let patch_apply_ms = run_patch_apply(&cfg)?;
    let (store_cycle_ms, sync_pull_ms) = run_store_sync(&cfg)?;

    let metrics = Metrics {
        eval_ms,
        runner_ms,
        patch_apply_ms,
        store_cycle_ms,
        sync_pull_ms,
    };
    let report = Report::new(cfg.warmups, cfg.repeats, cfg.budgets, metrics);
    report.write_json(&cfg.out)?;
    report.enforce_budgets()?;

    println!("runtime-microbench: wrote {}", cfg.out.display());
    println!(
        "runtime-microbench: eval_ms={} runner_ms={} patch_apply_ms={} store_cycle_ms={} sync_pull_ms={}",
        metrics.eval_ms,
        metrics.runner_ms,
        metrics.patch_apply_ms,
        metrics.store_cycle_ms,
        metrics.sync_pull_ms
    );
    Ok(())
}
