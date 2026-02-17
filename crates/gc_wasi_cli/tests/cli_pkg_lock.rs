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
  "core/store::put",
  "core/pkg::init",
  "core/pkg::add",
  "core/pkg::lock",
  "core/pkg::install",
  "core/pkg::verify"
]

[store]
dir = "./.genesis/store"

[refs]
path = "./.genesis/refs.gc"

[op."core/pkg::init"]
base_dir = "."
create_dirs = true

[op."core/pkg::add"]
base_dir = "."

[op."core/pkg::lock"]
base_dir = "."

[op."core/pkg::install"]
base_dir = "."

[op."core/pkg::verify"]
base_dir = "."
"#,
    )
    .unwrap();
    caps
}

fn store_put(dir: &Path, caps: &Path, term_src: &str, filename: &str) -> String {
    fs::write(dir.join(filename), term_src).unwrap();
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

fn setup_locked_commit_with_missing_patch(dir: &Path, caps: &Path, dep_name: &str) -> String {
    let snap_h = store_put(
        dir,
        caps,
        r#"{:type :vcs/snapshot :v 1 :kind :package :pkg/name "x" :pkg/version "0" :modules [] :obligations []}"#,
        "snap_missing_patch.gc",
    );
    let missing_patch = "f".repeat(64);
    let commit_h = store_put(
        dir,
        caps,
        &format!(
            r#"{{
  :type :vcs/commit
  :v 1
  :parents []
  :target {{ :kind :package :name "x" }}
  :base nil
  :patch "{missing_patch}"
  :result "{snap_h}"
  :obligations []
  :evidence []
  :attestations []
  :message "bad-closure"
}}"#
        ),
        "commit_missing_patch.gc",
    );

    cmd()
        .current_dir(dir)
        .args(["pkg", "--caps"])
        .arg(caps)
        .args(["init", "--workspace", "ws"])
        .assert()
        .success();

    cmd()
        .current_dir(dir)
        .args(["pkg", "--caps"])
        .arg(caps)
        .args(["add"])
        .arg(format!("{dep_name}@commit:{commit_h}"))
        .assert()
        .success();

    // Non-strict lock resolves selector to commit/snapshot.
    cmd()
        .current_dir(dir)
        .args(["pkg", "--caps"])
        .arg(caps)
        .args(["lock"])
        .assert()
        .success();

    commit_h
}

#[test]
fn wasi_pkg_lock_strict_rejects_commit_with_missing_patch_closure() {
    let td = tempfile::tempdir().unwrap();
    let dir = td.path();
    let caps = write_caps(dir);
    let _commit_h = setup_locked_commit_with_missing_patch(dir, &caps, "badpatch");

    cmd()
        .current_dir(dir)
        .args(["pkg", "--caps"])
        .arg(&caps)
        .args(["lock", "--strict"])
        .assert()
        .failure()
        .code(20);
}

#[test]
fn wasi_pkg_install_strict_rejects_commit_with_missing_patch_closure() {
    let td = tempfile::tempdir().unwrap();
    let dir = td.path();
    let caps = write_caps(dir);
    let _commit_h = setup_locked_commit_with_missing_patch(dir, &caps, "badpatch-install");

    cmd()
        .current_dir(dir)
        .args(["pkg", "--caps"])
        .arg(&caps)
        .args(["install"])
        .assert()
        .success();

    cmd()
        .current_dir(dir)
        .args(["pkg", "--caps"])
        .arg(&caps)
        .args(["install", "--strict"])
        .assert()
        .failure()
        .code(20);
}

#[test]
fn wasi_pkg_verify_rejects_commit_with_missing_patch_closure() {
    let td = tempfile::tempdir().unwrap();
    let dir = td.path();
    let caps = write_caps(dir);
    let _commit_h = setup_locked_commit_with_missing_patch(dir, &caps, "badpatch-verify");

    cmd()
        .current_dir(dir)
        .args(["pkg", "--caps"])
        .arg(&caps)
        .args(["verify"])
        .assert()
        .failure()
        .code(20);
}
