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
fn upgrade_plan_health_check_passes() {
    let root = repo_root();
    let status = Command::new("bash")
        .arg(root.join("scripts/check_upgrade_plan_health.sh"))
        .current_dir(&root)
        .status()
        .expect("run upgrade-plan health check");
    assert!(status.success(), "upgrade-plan health check failed");
}
