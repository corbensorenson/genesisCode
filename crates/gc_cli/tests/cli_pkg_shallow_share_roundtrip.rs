use std::fs;
use std::path::{Path, PathBuf};

use assert_cmd::cargo::cargo_bin_cmd;
use predicates::prelude::*;

fn write_caps_src(dir: &Path) -> PathBuf {
    let caps = dir.join("caps.toml");
    fs::write(
        &caps,
        r#"
allow = [
  "core/store::put",
  "core/pkg::snapshot",
  "core/gpk::export",
  "core/store::get"
]

[store]
dir = "./.genesis/store"

[refs]
path = "./.genesis/refs.gc"

[op."core/pkg::snapshot"]
base_dir = "."

[op."core/gpk::export"]
base_dir = "."
"#,
    )
    .unwrap();
    caps
}

fn write_caps_dst(dir: &Path) -> PathBuf {
    let caps = dir.join("caps.toml");
    fs::write(
        &caps,
        r#"
allow = [
  "core/gpk::import",
  "core/store::get",
  "core/store::has",

  "core/pkg-low::save-lock",
  "core/pkg-low::load-lock",
  "core/pkg::init",
  "core/pkg::add",
  "core/pkg::lock",
  "core/pkg::install"
]

[store]
dir = "./.genesis/store"

[refs]
path = "./.genesis/refs.gc"

[op."core/gpk::import"]
base_dir = "."

[op."core/pkg::init"]
base_dir = "."
create_dirs = true

[op."core/pkg::add"]
base_dir = "."

[op."core/pkg-low::save-lock"]
base_dir = "."
create_dirs = true

[op."core/pkg-low::load-lock"]
base_dir = "."

[op."core/pkg::lock"]
base_dir = "."

[op."core/pkg::install"]
base_dir = "."
"#,
    )
    .unwrap();
    caps
}

fn store_get(dir: &Path, caps: &Path, hash: &str) -> String {
    let out = cargo_bin_cmd!("genesis")
        .current_dir(dir)
        .args(["store", "--caps"])
        .arg(caps)
        .args(["get"])
        .arg(hash)
        .assert()
        .success()
        .get_output()
        .stdout
        .clone();
    String::from_utf8(out).unwrap()
}

#[test]
fn shallow_share_roundtrip_export_import_install() {
    let td_src = tempfile::tempdir().unwrap();
    let src = td_src.path();
    let caps_src = write_caps_src(src);

    let module_src = r#"
      (def mini::x 1)
      mini::x
    "#;
    let module_forms = gc_coreform::parse_module(module_src).unwrap();
    let module_forms = gc_coreform::canonicalize_module(module_forms).unwrap();
    let module_h = gc_coreform::hash_module(&module_forms);
    let module_h_hex = blake3::Hash::from_bytes(module_h).to_hex().to_string();

    fs::write(
        src.join("package.toml"),
        format!(
            r#"
name = "mini"
version = "0.0.1"
dependencies = []
obligations = []

[[modules]]
path = "mini.gc"
hash = "{module_h_hex}"
"#
        ),
    )
    .unwrap();
    fs::write(src.join("mini.gc"), module_src).unwrap();

    let snapshot_h = String::from_utf8(
        cargo_bin_cmd!("genesis")
            .current_dir(src)
            .args(["pkg", "--caps"])
            .arg(&caps_src)
            .args(["snapshot", "--pkg"])
            .arg("package.toml")
            .assert()
            .success()
            .get_output()
            .stdout
            .clone(),
    )
    .unwrap()
    .trim()
    .to_string();
    assert!(
        predicate::str::is_match("^[0-9a-f]{64}$")
            .unwrap()
            .eval(&snapshot_h)
    );

    let bundle_path = src.join("mini.gpk");
    let _bundle_h = String::from_utf8(
        cargo_bin_cmd!("genesis")
            .current_dir(src)
            .args(["pkg", "--caps"])
            .arg(&caps_src)
            .args(["export", "--snapshot"])
            .arg(&snapshot_h)
            .args(["--out"])
            .arg(&bundle_path)
            .assert()
            .success()
            .get_output()
            .stdout
            .clone(),
    )
    .unwrap()
    .trim()
    .to_string();

    let snap_src_bytes = store_get(src, &caps_src, &snapshot_h);

    // Destination workspace (empty store).
    let td_dst = tempfile::tempdir().unwrap();
    let dst = td_dst.path();
    let caps_dst = write_caps_dst(dst);

    fs::write(dst.join("mini.gpk"), fs::read(&bundle_path).unwrap()).unwrap();

    let root_h = String::from_utf8(
        cargo_bin_cmd!("genesis")
            .current_dir(dst)
            .args(["pkg", "--caps"])
            .arg(&caps_dst)
            .args(["import", "--input"])
            .arg("mini.gpk")
            .assert()
            .success()
            .get_output()
            .stdout
            .clone(),
    )
    .unwrap()
    .trim()
    .to_string();
    assert_eq!(root_h, snapshot_h);

    cargo_bin_cmd!("genesis")
        .current_dir(dst)
        .args(["store", "--caps"])
        .arg(&caps_dst)
        .args(["has"])
        .arg(&snapshot_h)
        .assert()
        .success()
        .stdout("true\n");

    let snap_dst_bytes = store_get(dst, &caps_dst, &snapshot_h);
    assert_eq!(snap_dst_bytes.trim_end(), snap_src_bytes.trim_end());

    // Install: lock a dependency pinned to the snapshot hash, then verify shallow closure exists.
    cargo_bin_cmd!("genesis")
        .current_dir(dst)
        .args(["pkg", "--caps"])
        .arg(&caps_dst)
        .args(["init", "--workspace", "ws"])
        .assert()
        .success()
        .stdout(predicate::str::is_match("^[0-9a-f]{64}\n$").unwrap());

    cargo_bin_cmd!("genesis")
        .current_dir(dst)
        .args(["pkg", "--caps"])
        .arg(&caps_dst)
        .args(["add"])
        .arg(format!("mini@snapshot:{snapshot_h}"))
        .assert()
        .success()
        .stdout(predicate::str::is_match("^[0-9a-f]{64}\n$").unwrap());

    cargo_bin_cmd!("genesis")
        .current_dir(dst)
        .args(["pkg", "--caps"])
        .arg(&caps_dst)
        .args(["lock"])
        .assert()
        .success()
        .stdout(predicate::str::is_match("^[0-9a-f]{64}\n$").unwrap());

    cargo_bin_cmd!("genesis")
        .current_dir(dst)
        .args(["pkg", "--caps"])
        .arg(&caps_dst)
        .args(["install", "--frozen"])
        .assert()
        .success()
        .stdout("ok\n");
}
