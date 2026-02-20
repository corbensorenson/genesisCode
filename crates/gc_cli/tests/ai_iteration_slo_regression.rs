use std::path::{Path, PathBuf};
use std::process::Command;

fn repo_root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../..")
        .canonicalize()
        .expect("canonicalize repo root")
}

#[test]
fn ai_iteration_slo_fails_when_changed_fast_fails() {
    let root = repo_root();
    let output = Command::new("bash")
        .arg(root.join("scripts/check_ai_iteration_slo.sh"))
        .env("GENESIS_MIN_FREE_KB", "999999999")
        .current_dir(&root)
        .output()
        .expect("run ai-iteration slo check");

    assert!(
        !output.status.success(),
        "ai-iteration slo check unexpectedly succeeded\nstdout:\n{}\nstderr:\n{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
}
