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
    let mut command = Command::new("bash");
    command
        .arg(root.join("scripts/check_upgrade_plan_health.sh"))
        .current_dir(&root);
    for name in [
        "CARGO_TARGET_DIR",
        "GENESIS_CARGO_CACHE_HIT",
        "GENESIS_CARGO_CACHE_KEY_SHA256",
        "GENESIS_CARGO_CACHE_RESOLVED",
        "GENESIS_CARGO_CACHE_RUSTC_IDENTITY_JSON",
        "GENESIS_CARGO_CACHE_SCOPE",
        "GENESIS_GENERATED_STATE_LEASE_PID",
        "GENESIS_GENERATED_STATE_LEASE_TOKEN",
        "GENESIS_GENERATED_STATE_ROOT",
        "GENESIS_SELFHOST_TOOLCHAIN_ARTIFACT",
        "GENESIS_SELFHOST_TOOLCHAIN_FRESHNESS",
        "GENESIS_SELFHOST_TOOLCHAIN_MANIFEST",
    ] {
        command.env_remove(name);
    }
    let status = command.status().expect("run upgrade-plan health check");
    assert!(status.success(), "upgrade-plan health check failed");
}
