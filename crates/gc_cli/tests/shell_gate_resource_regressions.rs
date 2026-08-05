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
fn disk_headroom_strict_and_non_strict_modes_behave_as_expected() {
    let root = repo_root();
    let script = root.join("scripts/check_disk_headroom.sh");

    let non_strict = Command::new("bash")
        .arg(&script)
        .arg("--path")
        .arg(".")
        .arg("--context")
        .arg("disk-test-nonstrict")
        .arg("--min-kb")
        .arg("999999999")
        .arg("--auto-reclaim")
        .arg("0")
        .arg("--strict")
        .arg("0")
        .current_dir(&root)
        .status()
        .expect("run disk headroom non-strict check");
    assert!(
        non_strict.success(),
        "disk headroom check should continue in non-strict mode when below threshold"
    );

    let strict = Command::new("bash")
        .arg(&script)
        .arg("--path")
        .arg(".")
        .arg("--context")
        .arg("disk-test-strict")
        .arg("--min-kb")
        .arg("999999999")
        .arg("--auto-reclaim")
        .arg("0")
        .arg("--strict")
        .arg("1")
        .current_dir(&root)
        .status()
        .expect("run disk headroom strict check");
    assert!(
        !strict.success(),
        "disk headroom check should fail in strict mode when below threshold"
    );
}

#[test]
fn check_reclaim_controls_fail_closed_without_running_maintenance() {
    let root = repo_root();
    let disk = Command::new("bash")
        .arg(root.join("scripts/check_disk_headroom.sh"))
        .arg("--auto-reclaim")
        .arg("1")
        .current_dir(&root)
        .output()
        .expect("run disk reclaim rejection");
    assert_eq!(disk.status.code(), Some(2));
    assert!(
        String::from_utf8_lossy(&disk.stderr).contains("checks are read-only"),
        "disk check must name the read-only boundary"
    );

    let runtime = Command::new("bash")
        .arg(root.join("scripts/check_runtime_backend_feature_matrix.sh"))
        .env("GENESIS_RUNTIME_BACKEND_MATRIX_AUTO_RECLAIM", "1")
        .current_dir(&root)
        .output()
        .expect("run runtime-matrix reclaim rejection");
    assert_eq!(runtime.status.code(), Some(2));
    assert!(
        String::from_utf8_lossy(&runtime.stderr).contains("checks are read-only"),
        "runtime matrix must name the read-only boundary"
    );
}
