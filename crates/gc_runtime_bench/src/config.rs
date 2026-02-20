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

#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize)]
pub enum BenchMode {
    Full,
    ComputeOnly,
}

impl BenchMode {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Full => "full",
            Self::ComputeOnly => "compute-only",
        }
    }
}

#[derive(Debug, Clone)]
pub struct BenchConfig {
    pub warmups: usize,
    pub repeats: usize,
    pub budgets: Budgets,
    pub out: PathBuf,
    pub build_profile: String,
    pub build_mode: String,
    pub gpu_compute_backend_policy: String,
    pub bench_mode: BenchMode,
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

fn env_string(name: &str, default: &str) -> String {
    std::env::var(name)
        .ok()
        .map(|v| v.trim().to_string())
        .filter(|v| !v.is_empty())
        .unwrap_or_else(|| default.to_string())
}

pub fn parse() -> Result<BenchConfig> {
    let mut args = std::env::args().skip(1);
    let mut out = PathBuf::from(".genesis/perf/runtime_microbench_metrics.json");
    let mut bench_mode = BenchMode::Full;
    while let Some(arg) = args.next() {
        match arg.as_str() {
            "--out" => {
                let Some(p) = args.next() else {
                    bail!("--out requires a file path");
                };
                out = PathBuf::from(p);
            }
            "--mode" => {
                let Some(mode) = args.next() else {
                    bail!("--mode requires a value (full|compute-only)");
                };
                bench_mode = match mode.trim() {
                    "full" => BenchMode::Full,
                    "compute-only" => BenchMode::ComputeOnly,
                    other => bail!("unknown --mode value: {other} (expected full|compute-only)"),
                };
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
    let build_profile = env_string("GENESIS_RUNTIME_MICROBENCH_PROFILE", "unknown");
    let build_mode = env_string("GENESIS_RUNTIME_MICROBENCH_BUILD_MODE", "unknown");
    let gpu_compute_backend_policy =
        env_string("GENESIS_GPU_COMPUTE_BACKEND_POLICY", "dev-allow-fallback");

    Ok(BenchConfig {
        warmups,
        repeats,
        budgets,
        out,
        build_profile,
        build_mode,
        gpu_compute_backend_policy,
        bench_mode,
    })
}
