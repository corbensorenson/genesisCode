use std::path::{Path, PathBuf};

use assert_cmd::cargo::cargo_bin_cmd;
use serde_json::Value as JsonValue;
use tempfile::tempdir;

fn write_effect_caps(dir: &Path, allow: &[&str]) -> PathBuf {
    let caps = dir.join("caps.toml");
    let mut s = String::new();
    s.push_str("allow = [");
    for (i, op) in allow.iter().enumerate() {
        if i != 0 {
            s.push_str(", ");
        }
        s.push('"');
        s.push_str(op);
        s.push('"');
    }
    s.push_str(
        "]\n\n[store]\ndir = \"./.genesis/store\"\n\n[refs]\npath = \"./.genesis/refs.gc\"\n\n[op.\"core/pkg-low::save-lock\"]\nbase_dir = \".\"\ncreate_dirs = true\n\n[op.\"core/pkg-low::load-lock\"]\nbase_dir = \".\"\n",
    );
    std::fs::write(&caps, s).unwrap();
    caps
}

#[test]
fn selfhost_only_accepts_gcpm_alias_command_group() {
    let td = tempdir().unwrap();
    let caps = write_effect_caps(
        td.path(),
        &["core/pkg-low::save-lock", "core/pkg-low::load-lock"],
    );

    cargo_bin_cmd!("genesis_wasi")
        .current_dir(td.path())
        .args(["--selfhost-only", "gcpm", "--caps"])
        .arg(caps.to_str().unwrap())
        .args(["init", "--workspace", "ws"])
        .assert()
        .success();

    cargo_bin_cmd!("genesis_wasi")
        .current_dir(td.path())
        .args(["--selfhost-only", "gcpm", "--caps"])
        .arg(caps.to_str().unwrap())
        .args(["list"])
        .assert()
        .success();
}

#[test]
fn gcpm_alias_preserves_pkg_json_kind_contract() {
    let td = tempdir().unwrap();
    let caps = write_effect_caps(
        td.path(),
        &["core/pkg-low::save-lock", "core/pkg-low::load-lock"],
    );

    let out = cargo_bin_cmd!("genesis_wasi")
        .current_dir(td.path())
        .args(["--json", "--selfhost-only", "gcpm", "--caps"])
        .arg(caps.to_str().unwrap())
        .args(["init", "--workspace", "ws"])
        .assert()
        .success()
        .get_output()
        .stdout
        .clone();

    let v: JsonValue = serde_json::from_slice(&out).unwrap();
    let kind = v.get("kind").and_then(JsonValue::as_str).unwrap();
    assert_eq!(kind, "genesis/pkg-init-v0.1");
}
