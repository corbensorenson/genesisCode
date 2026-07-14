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

#[derive(Debug, Clone, Copy, serde::Serialize)]
pub struct WorkloadSizes {
    pub fib_n: usize,
    pub vec_len: usize,
    pub map_len: usize,
    pub str_concat_count: usize,
    pub dispatch_count: usize,
}

#[derive(Debug, Clone, Copy, serde::Serialize)]
pub struct WorkloadBudgets {
    pub fib_ms: u128,
    pub vec_build_ms: u128,
    pub map_build_ms: u128,
    pub str_concat_ms: u128,
    pub selfhost_parse_ms: u128,
    pub dispatch_ms: u128,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize)]
pub enum BenchMode {
    Full,
    ComputeOnly,
    Workloads,
}

impl BenchMode {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Full => "full",
            Self::ComputeOnly => "compute-only",
            Self::Workloads => "workloads",
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
    pub workload_profile: String,
    pub workload_sizes: WorkloadSizes,
    pub workload_roadmap_sizes: WorkloadSizes,
    pub workload_selfhost_parse_corpus: Vec<String>,
    pub workload_roadmap_selfhost_parse_corpus: Vec<String>,
    pub workload_budgets: WorkloadBudgets,
    pub roadmap_sample: Option<String>,
}

fn env_usize(name: &str, default: usize) -> usize {
    std::env::var(name)
        .ok()
        .and_then(|v| v.trim().parse::<usize>().ok())
        .filter(|n| *n > 0)
        .unwrap_or(default)
}

fn env_nonnegative_usize(name: &str, default: usize) -> usize {
    std::env::var(name)
        .ok()
        .and_then(|v| v.trim().parse::<usize>().ok())
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

fn default_workload_sizes(profile: &str) -> WorkloadSizes {
    match profile {
        "roadmap" => WorkloadSizes {
            fib_n: 25,
            vec_len: 1_000_000,
            map_len: 100_000,
            str_concat_count: 10_000,
            dispatch_count: 100_000,
        },
        _ => WorkloadSizes {
            fib_n: 25,
            vec_len: 1_000,
            map_len: 1_000,
            str_concat_count: 1_000,
            dispatch_count: 5_000,
        },
    }
}

fn default_selfhost_parse_corpus(profile: &str) -> Vec<String> {
    match profile {
        "roadmap" => vec![
            "selfhost/parse.gc".to_string(),
            "prelude/prelude.gc".to_string(),
        ],
        _ => vec![
            "selfhost/parse_core_v1.gc".to_string(),
            "prelude/modules/00_core.gc".to_string(),
        ],
    }
}

fn env_string_list(name: &str, default: Vec<String>) -> Vec<String> {
    std::env::var(name)
        .ok()
        .map(|v| {
            v.split(',')
                .map(str::trim)
                .filter(|v| !v.is_empty())
                .map(ToOwned::to_owned)
                .collect::<Vec<_>>()
        })
        .filter(|items| !items.is_empty())
        .unwrap_or(default)
}

pub fn parse() -> Result<BenchConfig> {
    let mut args = std::env::args().skip(1);
    let mut out = PathBuf::from(".genesis/perf/runtime_microbench_metrics.json");
    let mut bench_mode = BenchMode::Full;
    let mut roadmap_sample = None;
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
                    bail!("--mode requires a value (full|compute-only|workloads)");
                };
                bench_mode = match mode.trim() {
                    "full" => BenchMode::Full,
                    "compute-only" => BenchMode::ComputeOnly,
                    "workloads" => BenchMode::Workloads,
                    other => bail!(
                        "unknown --mode value: {other} (expected full|compute-only|workloads)"
                    ),
                };
            }
            "--roadmap-sample" => {
                let Some(workload_id) = args.next() else {
                    bail!("--roadmap-sample requires PB-1, PB-4, PB-5, or PB-7");
                };
                if !matches!(workload_id.as_str(), "PB-1" | "PB-4" | "PB-5" | "PB-7") {
                    bail!(
                        "--roadmap-sample only supports active workloads PB-1, PB-4, PB-5, and PB-7"
                    );
                }
                roadmap_sample = Some(workload_id);
            }
            other => bail!("unknown argument: {other}"),
        }
    }
    if roadmap_sample.is_some() && bench_mode != BenchMode::Workloads {
        bail!("--roadmap-sample requires --mode workloads");
    }

    let warmups = env_nonnegative_usize("GENESIS_MICROBENCH_WARMUPS", 1);
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
    let workload_profile = env_string("GENESIS_RUNTIME_WORKLOAD_PROFILE", "smoke");
    let mut workload_sizes = default_workload_sizes(&workload_profile);
    workload_sizes.fib_n = env_usize("GENESIS_WORKLOAD_FIB_N", workload_sizes.fib_n);
    workload_sizes.vec_len = env_usize("GENESIS_WORKLOAD_VEC_LEN", workload_sizes.vec_len);
    workload_sizes.map_len = env_usize("GENESIS_WORKLOAD_MAP_LEN", workload_sizes.map_len);
    workload_sizes.str_concat_count = env_usize(
        "GENESIS_WORKLOAD_STR_CONCAT_COUNT",
        workload_sizes.str_concat_count,
    );
    workload_sizes.dispatch_count = env_usize(
        "GENESIS_WORKLOAD_DISPATCH_COUNT",
        workload_sizes.dispatch_count,
    );
    let workload_roadmap_sizes = default_workload_sizes("roadmap");
    let workload_selfhost_parse_corpus = env_string_list(
        "GENESIS_WORKLOAD_SELFHOST_PARSE_CORPUS",
        default_selfhost_parse_corpus(&workload_profile),
    );
    let workload_roadmap_selfhost_parse_corpus = default_selfhost_parse_corpus("roadmap");
    let workload_budgets = WorkloadBudgets {
        fib_ms: env_u128("GENESIS_BUDGET_WORKLOAD_FIB_MS", 10_000),
        vec_build_ms: env_u128("GENESIS_BUDGET_WORKLOAD_VEC_BUILD_MS", 1_000),
        map_build_ms: env_u128("GENESIS_BUDGET_WORKLOAD_MAP_BUILD_MS", 2_000),
        str_concat_ms: env_u128("GENESIS_BUDGET_WORKLOAD_STR_CONCAT_MS", 1_000),
        selfhost_parse_ms: env_u128("GENESIS_BUDGET_WORKLOAD_SELFHOST_PARSE_MS", 120_000),
        dispatch_ms: env_u128("GENESIS_BUDGET_WORKLOAD_DISPATCH_MS", 2_000),
    };

    Ok(BenchConfig {
        warmups,
        repeats,
        budgets,
        out,
        build_profile,
        build_mode,
        gpu_compute_backend_policy,
        bench_mode,
        workload_profile,
        workload_sizes,
        workload_roadmap_sizes,
        workload_selfhost_parse_corpus,
        workload_roadmap_selfhost_parse_corpus,
        workload_budgets,
        roadmap_sample,
    })
}
