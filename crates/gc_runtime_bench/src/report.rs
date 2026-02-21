use std::path::Path;

use anyhow::{Context, Result, bail};

use crate::config::Budgets;

#[derive(Debug, Clone, Copy, serde::Serialize)]
pub struct Metrics {
    pub eval_ms: u128,
    pub runner_ms: u128,
    pub bridge_runner_ms: u128,
    pub gpu_compute_submit_ms: u128,
    pub task_runner_ms: u128,
    pub patch_apply_ms: u128,
    pub store_cycle_ms: u128,
    pub sync_pull_ms: u128,
}

#[derive(Debug, serde::Serialize)]
pub struct Report {
    kind: &'static str,
    build_profile: String,
    build_mode: String,
    gpu_compute_backend_policy: String,
    bench_mode: String,
    warmups: usize,
    repeats: usize,
    gpu_compute_backend: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    gpu_compute_adapter: Option<String>,
    budgets: Budgets,
    metrics: Metrics,
}

#[derive(Debug)]
pub struct ReportMeta {
    pub build_profile: String,
    pub build_mode: String,
    pub gpu_compute_backend_policy: String,
    pub bench_mode: String,
    pub warmups: usize,
    pub repeats: usize,
    pub gpu_compute_backend: String,
    pub gpu_compute_adapter: Option<String>,
}

impl Report {
    pub fn new(meta: ReportMeta, budgets: Budgets, metrics: Metrics) -> Self {
        Self {
            kind: "genesis/runtime-microbench-v0.1",
            build_profile: meta.build_profile,
            build_mode: meta.build_mode,
            gpu_compute_backend_policy: meta.gpu_compute_backend_policy,
            bench_mode: meta.bench_mode,
            warmups: meta.warmups,
            repeats: meta.repeats,
            gpu_compute_backend: meta.gpu_compute_backend,
            gpu_compute_adapter: meta.gpu_compute_adapter,
            budgets,
            metrics,
        }
    }

    pub fn write_json(&self, path: &Path) -> Result<()> {
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent)
                .with_context(|| format!("create {}", parent.display()))?;
        }
        let bytes = serde_json::to_vec_pretty(self).context("serialize benchmark report")?;
        std::fs::write(path, bytes).with_context(|| format!("write {}", path.display()))?;
        Ok(())
    }

    pub fn enforce_budgets(&self) -> Result<()> {
        let mut violations: Vec<String> = Vec::new();
        if self.metrics.eval_ms > self.budgets.eval_ms {
            violations.push(format!(
                "eval_ms={} > {}",
                self.metrics.eval_ms, self.budgets.eval_ms
            ));
        }
        if self.metrics.runner_ms > self.budgets.runner_ms {
            violations.push(format!(
                "runner_ms={} > {}",
                self.metrics.runner_ms, self.budgets.runner_ms
            ));
        }
        if self.metrics.bridge_runner_ms > self.budgets.bridge_runner_ms {
            violations.push(format!(
                "bridge_runner_ms={} > {}",
                self.metrics.bridge_runner_ms, self.budgets.bridge_runner_ms
            ));
        }
        if self.metrics.gpu_compute_submit_ms > self.budgets.gpu_compute_submit_ms {
            violations.push(format!(
                "gpu_compute_submit_ms={} > {}",
                self.metrics.gpu_compute_submit_ms, self.budgets.gpu_compute_submit_ms
            ));
        }
        if self.metrics.task_runner_ms > self.budgets.task_runner_ms {
            violations.push(format!(
                "task_runner_ms={} > {}",
                self.metrics.task_runner_ms, self.budgets.task_runner_ms
            ));
        }
        if self.metrics.patch_apply_ms > self.budgets.patch_apply_ms {
            violations.push(format!(
                "patch_apply_ms={} > {}",
                self.metrics.patch_apply_ms, self.budgets.patch_apply_ms
            ));
        }
        if self.metrics.store_cycle_ms > self.budgets.store_cycle_ms {
            violations.push(format!(
                "store_cycle_ms={} > {}",
                self.metrics.store_cycle_ms, self.budgets.store_cycle_ms
            ));
        }
        if self.metrics.sync_pull_ms > self.budgets.sync_pull_ms {
            violations.push(format!(
                "sync_pull_ms={} > {}",
                self.metrics.sync_pull_ms, self.budgets.sync_pull_ms
            ));
        }

        if !violations.is_empty() {
            bail!(
                "runtime microbenchmark budget failures: {}",
                violations.join(", ")
            );
        }
        Ok(())
    }
}
