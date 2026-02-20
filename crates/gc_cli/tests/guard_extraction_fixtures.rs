use std::path::{Path, PathBuf};
use std::process::Command;

fn repo_root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../..")
        .canonicalize()
        .expect("canonicalize repo root")
}

#[test]
fn guard_extraction_fixtures_are_stable() {
    let root = repo_root();
    let status = Command::new("bash")
        .arg(root.join("scripts/check_guard_extraction_fixtures.sh"))
        .current_dir(&root)
        .status()
        .expect("run guard extraction fixture check");
    assert!(status.success(), "guard extraction fixture check failed");
}
