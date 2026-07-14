use std::path::Path;
use std::process::Command;

#[test]
#[ignore = "perf-gate"]
fn genesiscode_authoring_skill_guard_check_passes() {
    let root = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../..")
        .canonicalize()
        .expect("canonicalize repo root");
    let status = Command::new("bash")
        .arg(root.join("scripts/check_genesiscode_authoring_skill.sh"))
        .current_dir(&root)
        .status()
        .expect("run genesiscode authoring skill guard");
    assert!(status.success(), "genesiscode authoring skill guard failed");
}
