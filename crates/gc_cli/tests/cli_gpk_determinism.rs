use std::fs;
use std::path::{Path, PathBuf};

use assert_cmd::cargo::cargo_bin_cmd;
use predicates::prelude::*;

fn write_caps(dir: &Path) -> PathBuf {
    let caps = dir.join("caps.toml");
    fs::write(
        &caps,
        r#"
allow = [
  "core/pkg::snapshot",
  "core/gpk::export",
  "core/store::has",
  "core/store::get"
]

[store]
dir = "./.genesis/store"

[op."core/pkg::snapshot"]
base_dir = "."

[op."core/gpk::export"]
base_dir = "."
"#,
    )
    .unwrap();
    caps
}

fn run_hash_stdout(cmd: &mut assert_cmd::Command) -> String {
    let out = cmd
        .assert()
        .success()
        .stdout(predicate::str::is_match("[0-9a-f]{64}\\s*").unwrap())
        .get_output()
        .stdout
        .clone();
    String::from_utf8(out).unwrap().trim().to_string()
}

#[test]
fn gpk_export_is_byte_for_byte_deterministic_for_same_snapshot() {
    let td = tempfile::tempdir().unwrap();
    let dir = td.path();
    let caps = write_caps(dir);

    // Minimal package.
    fs::write(
        dir.join("package.toml"),
        r#"
name = "mini"
version = "0.0.1"
dependencies = []
obligations = []

[[modules]]
path = "mini.gc"
"#,
    )
    .unwrap();
    fs::write(dir.join("mini.gc"), "(def mini::x 1)\nmini::x\n").unwrap();

    let mut snap_cmd = cargo_bin_cmd!("genesis");
    snap_cmd
        .current_dir(dir)
        .args(["pkg", "--caps"])
        .arg(&caps)
        .args(["snapshot", "--pkg", "package.toml"]);
    let snapshot_h = run_hash_stdout(&mut snap_cmd);

    let out_a = dir.join("a.gpk");
    let out_b = dir.join("b.gpk");

    let mut exp1 = cargo_bin_cmd!("genesis");
    exp1.current_dir(dir)
        .args(["pkg", "--caps"])
        .arg(&caps)
        .args(["export", "--snapshot"])
        .arg(&snapshot_h)
        .args(["--out"])
        .arg(&out_a);
    let h1 = run_hash_stdout(&mut exp1);

    let mut exp2 = cargo_bin_cmd!("genesis");
    exp2.current_dir(dir)
        .args(["pkg", "--caps"])
        .arg(&caps)
        .args(["export", "--snapshot"])
        .arg(&snapshot_h)
        .args(["--out"])
        .arg(&out_b);
    let h2 = run_hash_stdout(&mut exp2);

    assert_eq!(h1, h2, "bundle hash must be deterministic");

    let a = fs::read(&out_a).unwrap();
    let b = fs::read(&out_b).unwrap();
    assert_eq!(a, b, ".gpk output must be byte-for-byte deterministic");
}
