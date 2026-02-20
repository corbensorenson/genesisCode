use std::path::PathBuf;

use anyhow::{Result, bail};

#[derive(Debug, Clone, Copy, serde::Serialize)]
pub struct Budgets {
    pub eval_ms: u128,
    pub runner_ms: u128,
    pub bridge_runner_ms: u128,
    pub gpu_compute_submit_ms: u128,
    pub task_runner_ms: u128,
    pub patch_apply_ms: u128,
    pub store_cycle_ms: u128,
    pub sync_pull_ms: u128,
}

#[derive(Debug, Clone)]
pub struct BenchConfig {
    pub warmups: usize,
    pub repeats: usize,
    pub budgets: Budgets,
    pub out: PathBuf,
}

fn env_usize(name: &str, default: usize) -> usize {
    std::env::var(name)
        .ok()
        .and_then(|v| v.trim().parse::<usize>().ok())
        .filter(|n| *n > 0)
        .unwrap_or(default)
}

fn env_u128(name: &str, default: u128) -> u128 {
    std::env::var(name)
        .ok()
        .and_then(|v| v.trim().parse::<u128>().ok())
        .filter(|n| *n > 0)
        .unwrap_or(default)
}

pub fn parse() -> Result<BenchConfig> {
    let mut args = std::env::args().skip(1);
    let mut out = PathBuf::from(".genesis/perf/runtime_microbench_metrics.json");
    while let Some(arg) = args.next() {
        match arg.as_str() {
            "--out" => {
                let Some(p) = args.next() else {
                    bail!("--out requires a file path");
                };
                out = PathBuf::from(p);
            }
            other => bail!("unknown argument: {other}"),
        }
    }

    let warmups = env_usize("GENESIS_MICROBENCH_WARMUPS", 1);
    let repeats = env_usize("GENESIS_MICROBENCH_REPEATS", 3);
    let budgets = Budgets {
        eval_ms: env_u128("GENESIS_BUDGET_MICRO_EVAL_MS", 2_000),
        runner_ms: env_u128("GENESIS_BUDGET_MICRO_RUNNER_MS", 4_000),
        bridge_runner_ms: env_u128("GENESIS_BUDGET_MICRO_BRIDGE_RUNNER_MS", 6_000),
        gpu_compute_submit_ms: env_u128("GENESIS_BUDGET_MICRO_GPU_COMPUTE_SUBMIT_MS", 8_000),
        task_runner_ms: env_u128("GENESIS_BUDGET_MICRO_TASK_RUNNER_MS", 6_000),
        patch_apply_ms: env_u128("GENESIS_BUDGET_MICRO_PATCH_APPLY_MS", 45_000),
        store_cycle_ms: env_u128("GENESIS_BUDGET_MICRO_STORE_CYCLE_MS", 1_000),
        sync_pull_ms: env_u128("GENESIS_BUDGET_MICRO_SYNC_PULL_MS", 4_000),
    };

    Ok(BenchConfig {
        warmups,
        repeats,
        budgets,
        out,
    })
}
