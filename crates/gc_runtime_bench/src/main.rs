mod bench_bridge_task;
mod bench_eval;
mod bench_gpu_compute;
mod bench_patch;
mod bench_runner;
mod bench_store_sync;
mod bench_workloads;
mod config;
mod device_bridge;
mod measure;
mod report;

use anyhow::Result;

use crate::bench_bridge_task::{run_bridge_runner, run_task_runner};
use crate::bench_eval::run as bench_eval;
use crate::bench_gpu_compute::run_gpu_compute_submit;
use crate::bench_patch::run_patch_apply;
use crate::bench_runner::run_effect_runner;
use crate::bench_store_sync::run_store_sync;
use crate::bench_workloads::{run as bench_workloads, run_roadmap_sample};
use crate::config::{BenchMode, parse as parse_config};
use crate::report::{Metrics, Report, ReportMeta, WorkloadReport, WorkloadReportMeta};

fn main() -> Result<()> {
    if device_bridge::maybe_run_bridge_mode_from_argv()? {
        return Ok(());
    }

    let cfg = parse_config()?;

    if cfg.bench_mode == BenchMode::Workloads {
        if let Some(workload_id) = cfg.roadmap_sample.as_deref() {
            let sample = run_roadmap_sample(&cfg, workload_id)?;
            println!("{}", serde_json::to_string(&sample)?);
            return Ok(());
        }
        let metrics = bench_workloads(&cfg)?;
        let report = WorkloadReport::new(
            WorkloadReportMeta {
                build_profile: cfg.build_profile.clone(),
                build_mode: cfg.build_mode.clone(),
                bench_mode: cfg.bench_mode.as_str().to_string(),
                workload_profile: cfg.workload_profile.clone(),
                warmups: cfg.warmups,
                repeats: cfg.repeats,
                sizes: cfg.workload_sizes,
                roadmap_sizes: cfg.workload_roadmap_sizes,
                selfhost_parse_corpus: cfg.workload_selfhost_parse_corpus.clone(),
                roadmap_selfhost_parse_corpus: cfg.workload_roadmap_selfhost_parse_corpus.clone(),
            },
            cfg.workload_budgets,
            metrics,
        );
        report.write_json(&cfg.out)?;
        report.enforce_budgets()?;
        println!("runtime-workload-bench: wrote {}", cfg.out.display());
        println!(
            "runtime-workload-bench: profile={} fib_ms={} vec_build_ms={} map_build_ms={} str_concat_ms={} selfhost_parse_ms={} dispatch_ms={}",
            cfg.workload_profile,
            metrics.fib_ms,
            metrics.vec_build_ms,
            metrics.map_build_ms,
            metrics.str_concat_ms,
            metrics.selfhost_parse_ms,
            metrics.dispatch_ms,
        );
        return Ok(());
    }

    let (metrics, gpu_compute_backend, gpu_compute_adapter) = match cfg.bench_mode {
        BenchMode::Full => {
            let eval_ms = bench_eval(&cfg)?;
            let runner_ms = run_effect_runner(&cfg)?;
            let bridge_runner_ms = run_bridge_runner(&cfg)?;
            let (gpu_compute_submit_ms, gpu_compute_backend, gpu_compute_adapter) =
                run_gpu_compute_submit(&cfg)?;
            let task_runner_ms = run_task_runner(&cfg)?;
            let patch_apply_ms = run_patch_apply(&cfg)?;
            let (store_cycle_ms, sync_pull_ms) = run_store_sync(&cfg)?;

            (
                Metrics {
                    eval_ms,
                    runner_ms,
                    bridge_runner_ms,
                    gpu_compute_submit_ms,
                    task_runner_ms,
                    patch_apply_ms,
                    store_cycle_ms,
                    sync_pull_ms,
                },
                gpu_compute_backend,
                gpu_compute_adapter,
            )
        }
        BenchMode::ComputeOnly => {
            let (gpu_compute_submit_ms, gpu_compute_backend, gpu_compute_adapter) =
                run_gpu_compute_submit(&cfg)?;
            (
                Metrics {
                    eval_ms: 0,
                    runner_ms: 0,
                    bridge_runner_ms: 0,
                    gpu_compute_submit_ms,
                    task_runner_ms: 0,
                    patch_apply_ms: 0,
                    store_cycle_ms: 0,
                    sync_pull_ms: 0,
                },
                gpu_compute_backend,
                gpu_compute_adapter,
            )
        }
        BenchMode::Workloads => {
            anyhow::bail!("workloads mode must exit before microbench dispatch")
        }
    };
    let report = Report::new(
        ReportMeta {
            build_profile: cfg.build_profile.clone(),
            build_mode: cfg.build_mode.clone(),
            gpu_compute_backend_policy: cfg.gpu_compute_backend_policy.clone(),
            bench_mode: cfg.bench_mode.as_str().to_string(),
            warmups: cfg.warmups,
            repeats: cfg.repeats,
            gpu_compute_backend: gpu_compute_backend.clone(),
            gpu_compute_adapter: gpu_compute_adapter.clone(),
        },
        cfg.budgets,
        metrics,
    );
    report.write_json(&cfg.out)?;
    report.enforce_budgets()?;

    println!("runtime-microbench: wrote {}", cfg.out.display());
    println!(
        "runtime-microbench: mode={} eval_ms={} runner_ms={} bridge_runner_ms={} gpu_compute_submit_ms={} task_runner_ms={} patch_apply_ms={} store_cycle_ms={} sync_pull_ms={} gpu_compute_backend={} gpu_compute_adapter={}",
        cfg.bench_mode.as_str(),
        metrics.eval_ms,
        metrics.runner_ms,
        metrics.bridge_runner_ms,
        metrics.gpu_compute_submit_ms,
        metrics.task_runner_ms,
        metrics.patch_apply_ms,
        metrics.store_cycle_ms,
        metrics.sync_pull_ms,
        gpu_compute_backend,
        gpu_compute_adapter.as_deref().unwrap_or("unknown")
    );
    Ok(())
}
