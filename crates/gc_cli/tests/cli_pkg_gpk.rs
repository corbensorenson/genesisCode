use std::fs;
use std::path::{Path, PathBuf};

use assert_cmd::cargo::cargo_bin_cmd;
use gc_coreform::{Term, TermOrdKey, parse_term};
use predicates::prelude::*;

fn write_caps(dir: &Path) -> PathBuf {
    let caps = dir.join("caps.toml");
    fs::write(
        &caps,
        r#"
allow = [
  "core/store::put",
  "core/pkg-low::snapshot",
  "core/gpk-low::export",
  "core/gpk-low::import",
  "core/store::has",
  "core/store::get"
]

[store]
dir = "./.genesis/store"

[refs]
path = "./.genesis/refs.gc"

[op."core/pkg-low::snapshot"]
base_dir = "."

[op."core/gpk-low::export"]
base_dir = "."

[op."core/gpk-low::import"]
base_dir = "."
"#,
    )
    .unwrap();
    caps
}

#[test]
fn pkg_snapshot_then_export_import_gpk_shallow_roundtrip() {
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

    fs::write(
        dir.join("mini.gc"),
        r#"
          (def mini::x 1)
          mini::x
        "#,
    )
    .unwrap();

    let log_snap = dir.join("snap.gclog");
    let snap_out = cargo_bin_cmd!("genesis")
        .current_dir(dir)
        .args(["pkg", "--caps"])
        .arg(&caps)
        .args(["--log"])
        .arg(&log_snap)
        .args(["snapshot", "--pkg"])
        .arg("package.toml")
        .assert()
        .success()
        .get_output()
        .stdout
        .clone();
    let snapshot_h = String::from_utf8(snap_out).unwrap().trim().to_string();
    assert!(
        predicate::str::is_match("^[0-9a-f]{64}$")
            .unwrap()
            .eval(&snapshot_h)
    );

    let bundle = dir.join("mini.gpk");
    let log_exp = dir.join("export.gclog");
    let exp_out = cargo_bin_cmd!("genesis")
        .current_dir(dir)
        .args(["pkg", "--caps"])
        .arg(&caps)
        .args(["--log"])
        .arg(&log_exp)
        .args(["export", "--snapshot"])
        .arg(&snapshot_h)
        .args(["--out"])
        .arg(&bundle)
        .assert()
        .success()
        .get_output()
        .stdout
        .clone();
    let bundle_h = String::from_utf8(exp_out).unwrap().trim().to_string();
    assert!(
        predicate::str::is_match("^[0-9a-f]{64}$")
            .unwrap()
            .eval(&bundle_h)
    );
    assert!(bundle.exists());

    // Simulate empty store.
    let store_dir = dir.join(".genesis").join("store");
    fs::remove_dir_all(&store_dir).unwrap();

    let log_imp = dir.join("import.gclog");
    let imp_out = cargo_bin_cmd!("genesis")
        .current_dir(dir)
        .args(["pkg", "--caps"])
        .arg(&caps)
        .args(["--log"])
        .arg(&log_imp)
        .args(["import", "--input"])
        .arg(&bundle)
        .assert()
        .success()
        .get_output()
        .stdout
        .clone();
    let root_h = String::from_utf8(imp_out).unwrap().trim().to_string();
    assert_eq!(root_h, snapshot_h);

    // Verify snapshot exists again.
    cargo_bin_cmd!("genesis")
        .current_dir(dir)
        .args(["store", "--caps"])
        .arg(&caps)
        .args(["has"])
        .arg(&snapshot_h)
        .assert()
        .success()
        .stdout("true\n");

    // Fetch snapshot term and ensure module artifact is present too.
    let snap_term_out = cargo_bin_cmd!("genesis")
        .current_dir(dir)
        .args(["store", "--caps"])
        .arg(&caps)
        .args(["get"])
        .arg(&snapshot_h)
        .assert()
        .success()
        .get_output()
        .stdout
        .clone();
    let snap_term = String::from_utf8(snap_term_out).unwrap();
    let t = parse_term(&snap_term).expect("parse snapshot term");
    let Term::Map(m) = t else {
        panic!("snapshot artifact must be a map");
    };
    let Term::Vector(mods) = m
        .get(&TermOrdKey(Term::symbol(":modules")))
        .expect("snapshot missing :modules")
        .clone()
    else {
        panic!("snapshot :modules must be a vector");
    };
    let first = mods.first().expect("at least one module");
    let Term::Map(mm) = first else {
        panic!("module entry must be a map");
    };
    let Term::Str(mod_h) = mm
        .get(&TermOrdKey(Term::symbol(":hash")))
        .expect("module entry missing :hash")
        .clone()
    else {
        panic!("module :hash must be a string");
    };

    cargo_bin_cmd!("genesis")
        .current_dir(dir)
        .args(["store", "--caps"])
        .arg(&caps)
        .args(["has"])
        .arg(mod_h)
        .assert()
        .success()
        .stdout("true\n");
}
