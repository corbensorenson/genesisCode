use std::fs;
use std::path::{Path, PathBuf};

use assert_cmd::cargo::cargo_bin_cmd;

fn write_caps(dir: &Path) -> PathBuf {
    let caps = dir.join("caps.toml");
    fs::write(
        &caps,
        r#"
allow = [
  "core/store::put",
  "core/store::has",
  "core/store::get",
  "core/gpk-low::export",
  "core/gpk-low::import"
]

[store]
dir = "./.genesis/store"

[refs]
path = "./.genesis/refs.gc"

[op."core/gpk-low::export"]
base_dir = "."
create_dirs = true

[op."core/gpk-low::import"]
base_dir = "."
create_dirs = true
"#,
    )
    .unwrap();
    caps
}

fn store_put(dir: &Path, caps: &Path, term_src: &str, filename: &str) -> String {
    fs::write(dir.join(filename), term_src).unwrap();
    let out = cargo_bin_cmd!("genesis")
        .current_dir(dir)
        .args(["store", "--caps"])
        .arg(caps)
        .args(["put", "--input"])
        .arg(filename)
        .assert()
        .success()
        .get_output()
        .stdout
        .clone();
    String::from_utf8(out).unwrap().trim().to_string()
}

fn pkg_export_shallow(dir: &Path, caps: &Path, root: &str, out_name: &str) -> String {
    let out = cargo_bin_cmd!("genesis")
        .current_dir(dir)
        .args(["pkg", "--caps"])
        .arg(caps)
        .args(["export", "--snapshot"])
        .arg(root)
        .args(["--out"])
        .arg(out_name)
        .assert()
        .success()
        .get_output()
        .stdout
        .clone();
    String::from_utf8(out).unwrap().trim().to_string()
}

fn pkg_import(dir: &Path, caps: &Path, in_name: &str) -> String {
    let out = cargo_bin_cmd!("genesis")
        .current_dir(dir)
        .args(["pkg", "--caps"])
        .arg(caps)
        .args(["import", "--input"])
        .arg(in_name)
        .assert()
        .success()
        .get_output()
        .stdout
        .clone();
    String::from_utf8(out).unwrap().trim().to_string()
}

fn store_has(dir: &Path, caps: &Path, hash: &str) -> bool {
    let out = cargo_bin_cmd!("genesis")
        .current_dir(dir)
        .args(["store", "--caps"])
        .arg(caps)
        .args(["has"])
        .arg(hash)
        .assert()
        .success()
        .get_output()
        .stdout
        .clone();
    String::from_utf8(out).unwrap().trim() == "true"
}

#[test]
fn gpk_shallow_includes_contract_snapshot_overrides() {
    let td = tempfile::tempdir().unwrap();
    let dir = td.path();
    let caps = write_caps(dir);

    let handler_h = store_put(dir, &caps, "{:handler \"x\"}", "handler.gc");
    let snap_h = store_put(
        dir,
        &caps,
        &format!(
            r#"{{
  :type :vcs/snapshot
  :v 1
  :kind :contract
  :proto nil
  :overrides {{ my/op::a "{handler_h}" }}
}}"#
        ),
        "snap.gc",
    );

    let _bundle_h = pkg_export_shallow(dir, &caps, &snap_h, "c.gpk");

    // Remove store to verify bundle completeness.
    let store_dir = dir.join(".genesis").join("store");
    fs::remove_dir_all(&store_dir).unwrap();

    let root = pkg_import(dir, &caps, "c.gpk");
    assert_eq!(root, snap_h);
    assert!(store_has(dir, &caps, &snap_h));
    assert!(store_has(dir, &caps, &handler_h));
}

#[test]
fn gpk_shallow_includes_module_snapshot_defs() {
    let td = tempfile::tempdir().unwrap();
    let dir = td.path();
    let caps = write_caps(dir);

    let def_h = store_put(dir, &caps, "{:def 1}", "def.gc");
    let snap_h = store_put(
        dir,
        &caps,
        &format!(
            r#"{{
  :type :vcs/snapshot
  :v 1
  :kind :module
  :module/name "m"
  :exports [my/mod::x]
  :obligations []
  :defs {{ my/mod::x "{def_h}" }}
}}"#
        ),
        "m-snap.gc",
    );

    let _bundle_h = pkg_export_shallow(dir, &caps, &snap_h, "m.gpk");
    let store_dir = dir.join(".genesis").join("store");
    fs::remove_dir_all(&store_dir).unwrap();

    let root = pkg_import(dir, &caps, "m.gpk");
    assert_eq!(root, snap_h);
    assert!(store_has(dir, &caps, &snap_h));
    assert!(store_has(dir, &caps, &def_h));
}

#[test]
fn gpk_shallow_includes_workspace_snapshot_modules_transitively() {
    let td = tempfile::tempdir().unwrap();
    let dir = td.path();
    let caps = write_caps(dir);

    let def_h = store_put(dir, &caps, "{:def 2}", "def2.gc");
    let mod_snap_h = store_put(
        dir,
        &caps,
        &format!(
            r#"{{
  :type :vcs/snapshot
  :v 1
  :kind :module
  :module/name "m2"
  :exports []
  :obligations []
  :defs {{ my/mod::y "{def_h}" }}
}}"#
        ),
        "m2-snap.gc",
    );

    let ws_snap_h = store_put(
        dir,
        &caps,
        &format!(
            r#"{{
  :type :vcs/snapshot
  :v 1
  :kind :workspace
  :workspace "ws"
  :lock nil
  :modules {{ "m2" "{mod_snap_h}" }}
}}"#
        ),
        "ws-snap.gc",
    );

    let _bundle_h = pkg_export_shallow(dir, &caps, &ws_snap_h, "ws.gpk");
    let store_dir = dir.join(".genesis").join("store");
    fs::remove_dir_all(&store_dir).unwrap();

    let root = pkg_import(dir, &caps, "ws.gpk");
    assert_eq!(root, ws_snap_h);
    assert!(store_has(dir, &caps, &ws_snap_h));
    assert!(store_has(dir, &caps, &mod_snap_h));
    assert!(store_has(dir, &caps, &def_h));
}
