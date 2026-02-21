use std::path::Path;
use std::process::Command;

#[test]
fn agent_authoring_bundle_guard_check_passes() {
    let root = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../..")
        .canonicalize()
        .expect("canonicalize repo root");
    let status = Command::new("bash")
        .arg(root.join("scripts/check_agent_authoring_bundle.sh"))
        .current_dir(&root)
        .status()
        .expect("run agent authoring bundle guard");
    assert!(status.success(), "agent authoring bundle guard failed");
}
