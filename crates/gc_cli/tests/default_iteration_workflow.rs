use std::path::{Path, PathBuf};
use std::process::Command;

fn repo_root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../..")
        .canonicalize()
        .expect("canonicalize repo root")
}

#[test]
fn default_iteration_workflow_check_passes() {
    let root = repo_root();
    let status = Command::new("bash")
        .arg(root.join("scripts/check_default_iteration_workflow.sh"))
        .current_dir(&root)
        .status()
        .expect("run default iteration workflow check");
    assert!(status.success(), "default iteration workflow check failed");
}
