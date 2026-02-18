use std::fs;
use std::path::{Path, PathBuf};

use assert_cmd::cargo::cargo_bin_cmd;

fn write_caps(dir: &Path) -> PathBuf {
    let caps = dir.join("caps.toml");
    fs::write(
        &caps,
        r#"
allow = [
  "core/pkg::init",
  "core/pkg::lock",
  "core/pkg::update",
  "core/pkg::publish",
  "core/pkg-low::load-lock",
  "core/pkg-low::save-lock"
]

[store]
dir = "./.genesis/store"

[refs]
path = "./.genesis/refs.gc"

[op."core/pkg::init"]
base_dir = "."
create_dirs = true

[op."core/pkg::lock"]
base_dir = "."

[op."core/pkg::update"]
base_dir = "."

[op."core/pkg::publish"]
base_dir = "."

[op."core/pkg-low::load-lock"]
base_dir = "."

[op."core/pkg-low::save-lock"]
base_dir = "."
"#,
    )
    .unwrap();
    caps
}

#[test]
fn gcpm_lock_and_update_emit_ai_report_artifacts() {
    let td = tempfile::tempdir().unwrap();
    let dir = td.path();
    let caps = write_caps(dir);

    cargo_bin_cmd!("genesis")
        .current_dir(dir)
        .env("GENESIS_ALLOW_RUST_ENGINE", "1")
        .args(["--coreform-frontend", "rust", "gcpm", "--caps"])
        .arg(&caps)
        .args(["init", "--workspace", "ws"])
        .assert()
        .success();

    let lock_out = cargo_bin_cmd!("genesis")
        .current_dir(dir)
        .env("GENESIS_ALLOW_RUST_ENGINE", "1")
        .args(["--json", "--coreform-frontend", "rust", "gcpm", "--caps"])
        .arg(&caps)
        .args(["lock", "--lock", "genesis.lock"])
        .assert()
        .success()
        .get_output()
        .stdout
        .clone();
    let lock_v: serde_json::Value = serde_json::from_slice(&lock_out).unwrap();
    assert_eq!(
        lock_v.get("kind").and_then(|x| x.as_str()),
        Some("genesis/pkg-lock-v0.1")
    );
    assert_eq!(
        lock_v
            .pointer("/data/report/schema")
            .and_then(|x| x.as_str()),
        Some("genesis/pkg-lock-report-v0.1")
    );
    assert_eq!(
        lock_v
            .pointer("/data/report/workflow")
            .and_then(|x| x.as_str()),
        Some("lock")
    );

    let update_out = cargo_bin_cmd!("genesis")
        .current_dir(dir)
        .env("GENESIS_ALLOW_RUST_ENGINE", "1")
        .args(["--json", "--coreform-frontend", "rust", "gcpm", "--caps"])
        .arg(&caps)
        .args(["update", "--lock", "genesis.lock"])
        .assert()
        .success()
        .get_output()
        .stdout
        .clone();
    let update_v: serde_json::Value = serde_json::from_slice(&update_out).unwrap();
    assert_eq!(
        update_v.get("kind").and_then(|x| x.as_str()),
        Some("genesis/pkg-update-v0.1")
    );
    assert_eq!(
        update_v
            .pointer("/data/report/schema")
            .and_then(|x| x.as_str()),
        Some("genesis/pkg-update-report-v0.1")
    );
    assert_eq!(
        update_v
            .pointer("/data/report/workflow")
            .and_then(|x| x.as_str()),
        Some("update")
    );
}

#[test]
fn gcpm_publish_failure_still_emits_publish_report_artifact() {
    let td = tempfile::tempdir().unwrap();
    let dir = td.path();
    let caps = write_caps(dir);

    let out = cargo_bin_cmd!("genesis")
        .current_dir(dir)
        .env("GENESIS_ALLOW_RUST_ENGINE", "1")
        .args(["--json", "--coreform-frontend", "rust", "gcpm", "--caps"])
        .arg(&caps)
        .args([
            "publish",
            "--remote",
            "gen://registry",
            "--ref",
            "refs/heads/main",
            "--policy",
            "not-a-hash",
            "--commit",
            "bad",
        ])
        .assert()
        .failure()
        .get_output()
        .stdout
        .clone();

    let v: serde_json::Value = serde_json::from_slice(&out).unwrap();
    assert_eq!(
        v.get("kind").and_then(|x| x.as_str()),
        Some("genesis/pkg-publish-v0.1")
    );
    assert_eq!(
        v.pointer("/data/report/schema").and_then(|x| x.as_str()),
        Some("genesis/pkg-publish-report-v0.1")
    );
    assert_eq!(
        v.pointer("/data/report/workflow").and_then(|x| x.as_str()),
        Some("publish")
    );
}
