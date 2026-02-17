use std::fs;
use std::path::{Path, PathBuf};

use assert_cmd::cargo::cargo_bin_cmd;

fn cmd() -> assert_cmd::Command {
    cargo_bin_cmd!("genesis_wasi")
}

fn write_caps(dir: &Path) -> PathBuf {
    let caps = dir.join("caps.toml");
    fs::write(
        &caps,
        r#"
allow = [
  "core/gpk::export",
  "core/gpk::import",
  "core/store::put",
  "core/store::has"
]

[store]
dir = "./.genesis/store"

[refs]
path = "./.genesis/refs.gc"

[op."core/gpk::export"]
base_dir = "."

[op."core/gpk::import"]
base_dir = "."
"#,
    )
    .unwrap();
    caps
}

fn store_put(dir: &Path, caps: &Path, term_src: &str, filename: &str) -> String {
    let p = dir.join(filename);
    fs::write(&p, term_src).unwrap();
    let out = cmd()
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

#[test]
fn wasi_pkg_export_shallow_include_deps_locked_includes_dep_closure_while_none_excludes() {
    let td = tempfile::tempdir().unwrap();
    let dir = td.path();
    let caps = write_caps(dir);

    let root_mod_h = store_put(
        dir,
        &caps,
        r#"{:kind "module" :name "root"}"#,
        "root_mod.gc",
    );
    let dep_mod_h = store_put(dir, &caps, r#"{:kind "module" :name "dep"}"#, "dep_mod.gc");

    let dep_snap_h = store_put(
        dir,
        &caps,
        &format!(
            r#"{{
  :type :vcs/snapshot
  :v 1
  :kind :package
  :pkg/name "dep"
  :pkg/version "0.0.1"
  :modules [{{:path "dep.gc" :hash "{dep_mod_h}"}}]
  :obligations []
}}"#
        ),
        "dep_snap.gc",
    );
    let dep_patch_h = store_put(
        dir,
        &caps,
        r#"{:type :vcs/patch :v 1 :ops []}"#,
        "dep_patch.gc",
    );
    let dep_commit_h = store_put(
        dir,
        &caps,
        &format!(
            r#"{{
  :type :vcs/commit
  :v 1
  :parents []
  :target {{:kind :package :name "dep"}}
  :base nil
  :patch "{dep_patch_h}"
  :result "{dep_snap_h}"
  :obligations []
  :evidence []
  :attestations []
  :message "dep"
}}"#
        ),
        "dep_commit.gc",
    );

    let root_snap_h = store_put(
        dir,
        &caps,
        &format!(
            r#"{{
  :type :vcs/snapshot
  :v 1
  :kind :package
  :pkg/name "root"
  :pkg/version "0.0.1"
  :modules [{{:path "root.gc" :hash "{root_mod_h}"}}]
  :deps [{{:dep/name "dep" :dep/commit "{dep_commit_h}" :dep/snapshot "{dep_snap_h}"}}]
  :obligations []
}}"#
        ),
        "root_snap.gc",
    );

    let bundle_none = dir.join("none.gpk");
    cmd()
        .current_dir(dir)
        .args(["pkg", "--caps"])
        .arg(&caps)
        .args(["export", "--snapshot"])
        .arg(&root_snap_h)
        .args(["--out"])
        .arg(&bundle_none)
        .args(["--include-deps", "none"])
        .assert()
        .success();

    let bundle_locked = dir.join("locked.gpk");
    cmd()
        .current_dir(dir)
        .args(["pkg", "--caps"])
        .arg(&caps)
        .args(["export", "--snapshot"])
        .arg(&root_snap_h)
        .args(["--out"])
        .arg(&bundle_locked)
        .args(["--include-deps", "locked"])
        .assert()
        .success();

    fs::remove_dir_all(dir.join(".genesis").join("store")).unwrap();
    cmd()
        .current_dir(dir)
        .args(["pkg", "--caps"])
        .arg(&caps)
        .args(["import", "--input"])
        .arg(&bundle_none)
        .assert()
        .success();

    for h in [&root_snap_h, &root_mod_h] {
        cmd()
            .current_dir(dir)
            .args(["store", "--caps"])
            .arg(&caps)
            .args(["has"])
            .arg(h)
            .assert()
            .success()
            .stdout("true\n");
    }
    for h in [&dep_commit_h, &dep_snap_h, &dep_patch_h, &dep_mod_h] {
        cmd()
            .current_dir(dir)
            .args(["store", "--caps"])
            .arg(&caps)
            .args(["has"])
            .arg(h)
            .assert()
            .success()
            .stdout("false\n");
    }

    fs::remove_dir_all(dir.join(".genesis").join("store")).unwrap();
    cmd()
        .current_dir(dir)
        .args(["pkg", "--caps"])
        .arg(&caps)
        .args(["import", "--input"])
        .arg(&bundle_locked)
        .assert()
        .success();

    for h in [
        &root_snap_h,
        &root_mod_h,
        &dep_commit_h,
        &dep_snap_h,
        &dep_patch_h,
        &dep_mod_h,
    ] {
        cmd()
            .current_dir(dir)
            .args(["store", "--caps"])
            .arg(&caps)
            .args(["has"])
            .arg(h)
            .assert()
            .success()
            .stdout("true\n");
    }
}
