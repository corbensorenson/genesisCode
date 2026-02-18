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
  "core/store::has",
  "core/store::get",
  "core/pkg-low::save-lock",
  "core/pkg-low::load-lock",
  "core/pkg-low::init",
  "core/pkg-low::add",
  "core/pkg-low::lock",
  "core/pkg-low::install",
  "core/pkg-low::verify"
]

[store]
dir = "./.genesis/store"

[refs]
path = "./.genesis/refs.gc"

[op."core/pkg-low::init"]
base_dir = "."
create_dirs = true

[op."core/pkg-low::add"]
base_dir = "."

[op."core/pkg-low::lock"]
base_dir = "."

[op."core/pkg-low::install"]
base_dir = "."

[op."core/pkg-low::verify"]
base_dir = "."

[op."core/pkg-low::save-lock"]
base_dir = "."
create_dirs = true

[op."core/pkg-low::load-lock"]
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

fn is_hex64(s: &str) -> bool {
    s.len() == 64 && s.as_bytes().iter().all(|b| b.is_ascii_hexdigit())
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
fn wasi_pkg_lock_install_verify_roundtrip_snapshot_selector() {
    let td = tempfile::tempdir().unwrap();
    let dir = td.path();
    let caps = write_caps(dir);

    let snapshot_h = store_put(
        dir,
        &caps,
        r#"{:type :vcs/snapshot :v 1 :kind :package :pkg/name "mini" :pkg/version "0.0.1" :modules [] :obligations []}"#,
        "mini_snapshot.gc",
    );
    assert!(is_hex64(&snapshot_h));

    let lock_h1 = String::from_utf8(
        cmd()
            .current_dir(dir)
            .args(["pkg", "--caps"])
            .arg(&caps)
            .args(["init", "--workspace", "ws"])
            .assert()
            .success()
            .get_output()
            .stdout
            .clone(),
    )
    .unwrap()
    .trim()
    .to_string();
    assert!(is_hex64(&lock_h1));

    let lock_h2 = String::from_utf8(
        cmd()
            .current_dir(dir)
            .args(["pkg", "--caps"])
            .arg(&caps)
            .args(["add"])
            .arg(format!("mini@snapshot:{snapshot_h}"))
            .assert()
            .success()
            .get_output()
            .stdout
            .clone(),
    )
    .unwrap()
    .trim()
    .to_string();
    assert!(is_hex64(&lock_h2));

    let lock_h3 = String::from_utf8(
        cmd()
            .current_dir(dir)
            .args(["pkg", "--caps"])
            .arg(&caps)
            .args(["lock"])
            .assert()
            .success()
            .get_output()
            .stdout
            .clone(),
    )
    .unwrap()
    .trim()
    .to_string();
    assert!(is_hex64(&lock_h3));

    cmd()
        .current_dir(dir)
        .args(["pkg", "--caps"])
        .arg(&caps)
        .args(["lock"])
        .assert()
        .success()
        .stdout(format!("{lock_h3}\n"));

    cmd()
        .current_dir(dir)
        .args(["pkg", "--caps"])
        .arg(&caps)
        .args(["install", "--frozen"])
        .assert()
        .success()
        .stdout("ok\n");

    cmd()
        .current_dir(dir)
        .args(["pkg", "--caps"])
        .arg(&caps)
        .args(["verify"])
        .assert()
        .success()
        .stdout("ok\n");

    let store_dir = dir.join(".genesis").join("store");
    fs::remove_dir_all(&store_dir).unwrap();
    cmd()
        .current_dir(dir)
        .args(["pkg", "--caps"])
        .arg(&caps)
        .args(["install", "--frozen"])
        .assert()
        .failure()
        .code(50);
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
