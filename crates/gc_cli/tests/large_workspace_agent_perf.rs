use std::path::{Path, PathBuf};
use std::process::Command;

fn repo_root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../..")
        .canonicalize()
        .expect("canonicalize repo root")
}

#[test]
#[ignore = "perf-gate"]
fn large_workspace_agent_perf_gate_smoke_passes_with_small_local_profile() {
    let root = repo_root();
    let status = Command::new("bash")
        .arg(root.join("scripts/check_large_workspace_agent_perf.sh"))
        .env("GENESIS_PERF_CARGO_PROFILE", "dev")
        .env("GENESIS_LARGE_WORKSPACE_MODULE_COUNT", "64")
        .env("GENESIS_LARGE_WORKSPACE_STEP_LIMIT", "1000000000")
        .env("GENESIS_LARGE_WORKSPACE_WARMUPS", "0")
        .env("GENESIS_LARGE_WORKSPACE_REPEATS", "1")
        .env("GENESIS_LARGE_WORKSPACE_RUNTIME_MIN_HISTORY", "1")
        .env("GENESIS_LARGE_WORKSPACE_RUNTIME_REQUIRE_MIN_HISTORY", "0")
        .env("GENESIS_LARGE_WORKSPACE_BUDGET_GCPM_LOCK_MS", "300000")
        .env("GENESIS_LARGE_WORKSPACE_BUDGET_GCPM_BUILD_MS", "300000")
        .env("GENESIS_LARGE_WORKSPACE_BUDGET_GCPM_TEST_MS", "300000")
        .env(
            "GENESIS_LARGE_WORKSPACE_BUDGET_SELFHOST_REFRESH_MS",
            "300000",
        )
        .current_dir(&root)
        .status()
        .expect("run large-workspace agent perf gate");
    assert!(
        status.success(),
        "large-workspace agent perf gate failed in reduced local smoke mode"
    );
}
