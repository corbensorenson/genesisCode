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
fn gcpm_lock_emits_prompt_safe_telemetry() {
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

    let out = cargo_bin_cmd!("genesis")
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
    let v: serde_json::Value = serde_json::from_slice(&out).unwrap();

    assert_eq!(
        v.pointer("/data/telemetry/schema").and_then(|x| x.as_str()),
        Some("genesis/pkg-telemetry-v0.1")
    );
    assert_eq!(
        v.pointer("/data/telemetry/command")
            .and_then(|x| x.as_str()),
        Some("pkg-lock")
    );
    assert!(
        v.pointer("/data/telemetry/effect_log_hash")
            .and_then(|x| x.as_str())
            .is_some()
    );
    assert!(
        v.pointer("/data/telemetry/value_hash")
            .and_then(|x| x.as_str())
            .is_some()
    );
    assert!(
        v.pointer("/data/telemetry/caps").is_none(),
        "telemetry must remain prompt-safe and omit sensitive paths"
    );
}
