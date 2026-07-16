use serde_json::Value;
use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;

fn repository_root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .and_then(Path::parent)
        .expect("gc_cli must live under <repository>/crates")
        .to_path_buf()
}

#[test]
fn construct_study_replays_byte_identically_with_the_shipped_binary() {
    let root = repository_root();
    let temporary = tempfile::tempdir().expect("construct study temporary directory");
    let actual = temporary.path().join("report.json");
    let output = Command::new("python3")
        .current_dir(&root)
        .arg("scripts/lib/genesisbench_construct_validity.py")
        .arg("--run")
        .arg("--genesis-bin")
        .arg(env!("CARGO_BIN_EXE_genesis"))
        .arg("--selfhost-artifact")
        .arg(root.join("selfhost/toolchain.gc"))
        .arg("--output")
        .arg(&actual)
        .output()
        .expect("execute construct study");
    assert!(
        output.status.success(),
        "construct study failed: {}",
        String::from_utf8_lossy(&output.stderr)
    );

    let expected = root.join("benchmarks/genesisbench/v0.1/construct-validity/report.json");
    let actual_bytes = fs::read(&actual).expect("read replayed report");
    let expected_bytes = fs::read(&expected).expect("read checked-in report");
    let actual_identity: Value =
        serde_json::from_slice(&actual_bytes).expect("parse replayed report JSON");
    let expected_identity: Value =
        serde_json::from_slice(&expected_bytes).expect("parse checked-in report JSON");
    assert!(
        actual_bytes == expected_bytes,
        "construct study must replay byte-identically: actual_identity={} expected_identity={} actual_bytes={} expected_bytes={}",
        actual_identity["contentIdentitySha256"],
        expected_identity["contentIdentitySha256"],
        actual_bytes.len(),
        expected_bytes.len(),
    );

    let report: Value = serde_json::from_slice(&actual_bytes).expect("parse construct report");
    assert_eq!(
        report["statistics"]["alternativeAcceptanceBasisPoints"],
        10_000
    );
    assert_eq!(report["statistics"]["negativeRejectionBasisPoints"], 10_000);
    assert_eq!(
        report["mutationAnalysis"]["mutationScoreBasisPoints"],
        10_000
    );
    assert_eq!(report["mutationAnalysis"]["survived"], 0);
    assert_eq!(report["maintenance"]["maintenanceBasisPoints"], 10_000);
    assert_eq!(
        report["maintenance"]["records"].as_array().map(Vec::len),
        Some(5)
    );
    assert_eq!(report["saturation"]["triggerScenario"], true);
    assert_eq!(report["saturation"]["singleEpochScenario"], false);
    assert_eq!(report["saturation"]["underThresholdScenario"], false);
}

#[test]
fn construct_study_static_contract_and_negative_controls_pass() {
    let root = repository_root();
    let output = Command::new("python3")
        .current_dir(&root)
        .arg("scripts/lib/genesisbench_construct_validity.py")
        .arg("--check")
        .arg("--self-test")
        .output()
        .expect("validate construct study");
    assert!(
        output.status.success(),
        "construct study validation failed: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    assert!(
        String::from_utf8_lossy(&output.stdout).contains("controls=13"),
        "construct study must execute its mutation controls"
    );
}
