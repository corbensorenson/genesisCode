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
fn pkg_low_semantic_boundary_check_passes() {
    let root = repo_root();
    let status = Command::new("bash")
        .arg(root.join("scripts/check_pkg_low_semantic_boundary.sh"))
        .current_dir(&root)
        .status()
        .expect("run pkg-low semantic boundary check");
    assert!(status.success(), "pkg-low semantic boundary check failed");
}
