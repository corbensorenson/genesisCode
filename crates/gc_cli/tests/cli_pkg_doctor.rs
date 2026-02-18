use std::fs;
use std::path::{Path, PathBuf};

use assert_cmd::cargo::cargo_bin_cmd;

fn write_caps(dir: &Path) -> PathBuf {
    let caps = dir.join("caps.toml");
    fs::write(
        &caps,
        r#"
allow = [
  "core/pkg-low::verify",
  "core/pkg-low::load-lock",
  "core/store::has",
  "core/store::get"
]

[store]
dir = "./.genesis/store"

[refs]
path = "./.genesis/refs.gc"

[op."core/pkg-low::verify"]
base_dir = "."

[op."core/pkg-low::load-lock"]
base_dir = "."
"#,
    )
    .unwrap();
    caps
}

#[test]
fn gcpm_doctor_json_success_has_stable_kind_and_report_schema() {
    let td = tempfile::tempdir().unwrap();
    let dir = td.path();
    let caps = write_caps(dir);

    fs::write(
        dir.join("genesis.lock"),
        r#"
version = 1
workspace = "ws"
policy = "policy:default-v0.1"

[requirements]

[locked]
"#,
    )
    .unwrap();

    let out = cargo_bin_cmd!("genesis")
        .current_dir(dir)
        .env("GENESIS_ALLOW_RUST_ENGINE", "1")
        .args(["--json", "--coreform-frontend", "rust", "gcpm", "--caps"])
        .arg(&caps)
        .args(["doctor", "--lock", "genesis.lock"])
        .assert()
        .success()
        .get_output()
        .stdout
        .clone();

    let v: serde_json::Value = serde_json::from_slice(&out).unwrap();
    assert_eq!(
        v.get("kind").and_then(|x| x.as_str()),
        Some("genesis/pkg-doctor-v0.1")
    );
    assert_eq!(v.get("ok").and_then(|x| x.as_bool()), Some(true));
    assert_eq!(
        v.pointer("/data/doctor/schema").and_then(|x| x.as_str()),
        Some("genesis/pkg-doctor-report-v0.2")
    );
    assert_eq!(
        v.pointer("/data/doctor/issue_count")
            .and_then(|x| x.as_u64()),
        Some(0)
    );
}

#[test]
fn gcpm_doctor_reports_missing_artifacts_with_fix_hints() {
    let td = tempfile::tempdir().unwrap();
    let dir = td.path();
    let caps = write_caps(dir);
    let missing = "a".repeat(64);

    fs::write(
        dir.join("genesis.lock"),
        format!(
            r#"
version = 1
workspace = "ws"
policy = "policy:default-v0.1"

[requirements]

[locked]
"dep" = {{ snapshot = "{missing}", source_selector = "snapshot:{missing}" }}
"#
        ),
    )
    .unwrap();

    let out = cargo_bin_cmd!("genesis")
        .current_dir(dir)
        .env("GENESIS_ALLOW_RUST_ENGINE", "1")
        .args(["--json", "--coreform-frontend", "rust", "gcpm", "--caps"])
        .arg(&caps)
        .args(["doctor", "--lock", "genesis.lock"])
        .assert()
        .failure()
        .code(50)
        .get_output()
        .stdout
        .clone();

    let v: serde_json::Value = serde_json::from_slice(&out).unwrap();
    assert_eq!(
        v.get("kind").and_then(|x| x.as_str()),
        Some("genesis/pkg-doctor-v0.1")
    );
    assert_eq!(v.get("ok").and_then(|x| x.as_bool()), Some(false));
    assert!(
        v.pointer("/data/doctor/issue_count")
            .and_then(|x| x.as_u64())
            .unwrap_or(0)
            >= 1
    );
    let fixes = v
        .pointer("/data/doctor/fixes")
        .and_then(|x| x.as_array())
        .cloned()
        .unwrap_or_default();
    assert!(
        !fixes.is_empty(),
        "expected at least one deterministic fix hint"
    );
}

#[test]
fn gcpm_doctor_detects_lock_drift_with_actionable_fix_metadata() {
    let td = tempfile::tempdir().unwrap();
    let dir = td.path();
    let caps = write_caps(dir);
    let snapshot_h = "b".repeat(64);

    fs::write(
        dir.join("genesis.lock"),
        format!(
            r#"
version = 1
workspace = "ws"
policy = "policy:default-v0.1"

[requirements]
"dep" = {{ selector = "snapshot:{snapshot_h}", update_policy = "manual", registry = "default" }}

[locked]
"#
        ),
    )
    .unwrap();

    let out = cargo_bin_cmd!("genesis")
        .current_dir(dir)
        .env("GENESIS_ALLOW_RUST_ENGINE", "1")
        .args(["--json", "--coreform-frontend", "rust", "gcpm", "--caps"])
        .arg(&caps)
        .args(["doctor", "--lock", "genesis.lock"])
        .assert()
        .failure()
        .code(50)
        .get_output()
        .stdout
        .clone();

    let v: serde_json::Value = serde_json::from_slice(&out).unwrap();
    assert_eq!(
        v.get("kind").and_then(|x| x.as_str()),
        Some("genesis/pkg-doctor-v0.1")
    );
    assert_eq!(v.get("ok").and_then(|x| x.as_bool()), Some(false));
    assert_eq!(
        v.pointer("/data/doctor/checks/1/id")
            .and_then(|x| x.as_str()),
        Some("lock.drift")
    );
    assert_eq!(
        v.pointer("/data/doctor/checks/1/missing_locked/0")
            .and_then(|x| x.as_str()),
        Some("dep")
    );
    assert_eq!(
        v.pointer("/data/doctor/fixes/0/action/op")
            .and_then(|x| x.as_str()),
        Some("gcpm.lock")
    );
}
