use std::path::{Path, PathBuf};
use std::process::Command;

use serde_json::Value;

fn repo_root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../..")
        .canonicalize()
        .expect("canonicalize repo root")
}

#[test]
#[ignore = "perf-gate"]
fn ai_stress_suite_fault_injection_flips_verification_fields_and_fails() {
    let root = repo_root();
    let tmp = tempfile::tempdir().expect("tempdir");
    let report = tmp.path().join("stress_report.json");
    let history = tmp.path().join("stress_history.jsonl");

    let output = Command::new("bash")
        .arg(root.join("scripts/render_ai_stress_suite_report.sh"))
        .arg(&report)
        .arg(&history)
        .arg(&history)
        .env(
            "GENESIS_STRESS_FAULT_INJECT",
            "bridge_gpu_compute_replay,editor_task_replay,selfhost_parallel_obligations",
        )
        .current_dir(&root)
        .output()
        .expect("run ai stress suite");

    assert!(
        !output.status.success(),
        "ai stress suite unexpectedly succeeded\nstdout:\n{}\nstderr:\n{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );

    let raw = std::fs::read_to_string(&report).expect("read generated stress report");
    let v: Value = serde_json::from_str(&raw).expect("parse stress report json");
    assert!(
        !v.get("replay_integrity_verified")
            .and_then(Value::as_bool)
            .unwrap_or(true),
        "fault injection should flip replay_integrity_verified to false"
    );
    assert!(
        !v.get("gpu_compute_verified")
            .and_then(Value::as_bool)
            .unwrap_or(true),
        "fault injection should flip gpu_compute_verified to false"
    );
    assert!(
        !v.get("bridge_budget_verified")
            .and_then(Value::as_bool)
            .unwrap_or(true),
        "fault injection should flip bridge_budget_verified to false"
    );
}
