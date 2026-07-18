use std::fs;
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
fn changed_fast_defaults_to_temporary_metrics_and_ignores_legacy_output_env() {
    let root = repo_root();
    let nonce = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .expect("system clock after epoch")
        .as_nanos();
    let temp = std::env::temp_dir().join(format!(
        "genesis-changed-fast-boundary-{}-{nonce}",
        std::process::id()
    ));
    fs::create_dir_all(&temp).expect("create changed-fast boundary fixture");
    let changed = temp.join("changed.txt");
    let report = temp.join("legacy-report.json");
    let history = temp.join("legacy-history.jsonl");
    fs::write(&changed, "README.md\n").expect("write changed-file fixture");

    let output = Command::new("bash")
        .arg(root.join("scripts/test_changed_fast.sh"))
        .arg("--runner")
        .arg("cargo")
        .arg("--changed-files-from")
        .arg(&changed)
        .arg("--budget-ms")
        .arg("420000")
        .arg("--min-history")
        .arg("1")
        .env("GENESIS_TEST_CHANGED_REPORT", &report)
        .env("GENESIS_TEST_CHANGED_HISTORY", &history)
        .current_dir(&root)
        .output()
        .expect("run changed-fast temporary metrics probe");

    let stdout = String::from_utf8_lossy(&output.stdout);
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(
        output.status.success(),
        "changed-fast temporary probe failed\nstdout:\n{stdout}\nstderr:\n{stderr}"
    );
    assert!(stdout.contains("report=temporary"));
    assert!(
        stdout.contains(
            "budget_subject=prepush-standard budget_ms=420000 disk_budget_bytes=3221225472"
        ),
        "explicit duration must not collapse the profile-fallback disk envelope"
    );
    assert!(
        !report.exists(),
        "legacy report environment override was honored"
    );
    assert!(
        !history.exists(),
        "legacy history environment override was honored"
    );
    fs::remove_dir_all(&temp).expect("remove changed-fast boundary fixture");
}
