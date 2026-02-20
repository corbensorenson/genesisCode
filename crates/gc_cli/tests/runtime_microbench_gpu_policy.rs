use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;

fn repo_root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../..")
        .canonicalize()
        .expect("canonicalize repo root")
}

fn write_report(path: &Path, backend: &str, gpu_submit_ms: u64) {
    let json = format!(
        r#"{{
  "kind": "genesis/runtime-microbench-v0.1",
  "gpu_compute_backend": "{backend}",
  "metrics": {{
    "bridge_runner_ms": 10,
    "gpu_compute_submit_ms": {gpu_submit_ms},
    "task_runner_ms": 10
  }},
  "budgets": {{
    "bridge_runner_ms": 100,
    "gpu_compute_submit_ms": 100,
    "task_runner_ms": 100
  }}
}}
"#
    );
    fs::write(path, json).expect("write runtime microbench report");
}

#[test]
fn runtime_microbench_fails_when_required_backend_is_not_present() {
    let root = repo_root();
    let tmp = tempfile::tempdir().expect("create tempdir");
    let out = tmp.path().join("runtime_microbench_metrics.json");
    let slo = tmp.path().join("concurrency_gpu_slo_report.json");
    write_report(&out, "deterministic-fallback", 25);

    let output = Command::new("bash")
        .arg(root.join("scripts/check_runtime_microbench_budgets.sh"))
        .env("GENESIS_RUNTIME_MICROBENCH_SKIP_RUN", "1")
        .env("GENESIS_RUNTIME_MICROBENCH_OUT", out.as_os_str())
        .env("GENESIS_CONCURRENCY_GPU_SLO_OUT", slo.as_os_str())
        .env("GENESIS_MIN_FREE_KB", "1")
        .env(
            "GENESIS_RUNTIME_MICROBENCH_REQUIRED_GPU_BACKEND",
            "device-bridge",
        )
        .current_dir(&root)
        .output()
        .expect("run runtime microbench budget script");
    assert!(
        !output.status.success(),
        "script unexpectedly succeeded\nstdout:\n{}\nstderr:\n{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr),
    );
}

#[test]
fn runtime_microbench_uses_backend_specific_gpu_budget() {
    let root = repo_root();
    let tmp = tempfile::tempdir().expect("create tempdir");
    let out = tmp.path().join("runtime_microbench_metrics.json");
    let slo = tmp.path().join("concurrency_gpu_slo_report.json");
    write_report(&out, "device-bridge", 40);

    let fail = Command::new("bash")
        .arg(root.join("scripts/check_runtime_microbench_budgets.sh"))
        .env("GENESIS_RUNTIME_MICROBENCH_SKIP_RUN", "1")
        .env("GENESIS_RUNTIME_MICROBENCH_OUT", out.as_os_str())
        .env("GENESIS_CONCURRENCY_GPU_SLO_OUT", slo.as_os_str())
        .env("GENESIS_MIN_FREE_KB", "1")
        .env("GENESIS_BUDGET_MICRO_GPU_COMPUTE_SUBMIT_MS_DEVICE", "30")
        .env("GENESIS_BUDGET_MICRO_GPU_COMPUTE_SUBMIT_MS_FALLBACK", "100")
        .current_dir(&root)
        .status()
        .expect("run runtime microbench with tight device budget");
    assert!(
        !fail.success(),
        "script should fail when device backend exceeds device-specific budget"
    );

    let pass = Command::new("bash")
        .arg(root.join("scripts/check_runtime_microbench_budgets.sh"))
        .env("GENESIS_RUNTIME_MICROBENCH_SKIP_RUN", "1")
        .env("GENESIS_RUNTIME_MICROBENCH_OUT", out.as_os_str())
        .env("GENESIS_CONCURRENCY_GPU_SLO_OUT", slo.as_os_str())
        .env("GENESIS_MIN_FREE_KB", "1")
        .env("GENESIS_BUDGET_MICRO_GPU_COMPUTE_SUBMIT_MS_DEVICE", "50")
        .env("GENESIS_BUDGET_MICRO_GPU_COMPUTE_SUBMIT_MS_FALLBACK", "100")
        .current_dir(&root)
        .status()
        .expect("run runtime microbench with relaxed device budget");
    assert!(
        pass.success(),
        "script should pass when device backend is within device-specific budget"
    );
}
