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
  "core/store::put",
  "core/pkg::init",
  "core/pkg::add",
  "core/pkg::lock",
  "core/pkg::update",
  "core/pkg::install",
  "core/pkg::verify",
  "core/pkg::list",
  "core/pkg::info",

  "core/pkg::snapshot",
  "core/store::has",
  "core/store::get"
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

[op."core/pkg::update"]
base_dir = "."

[op."core/pkg::install"]
base_dir = "."

[op."core/pkg::verify"]
base_dir = "."

[op."core/pkg::list"]
base_dir = "."

[op."core/pkg::info"]
base_dir = "."

[op."core/pkg::snapshot"]
base_dir = "."
"#,
    )
    .unwrap();
    caps
}

fn store_put(dir: &Path, caps: &Path, term_src: &str, filename: &str) -> String {
    fs::write(dir.join(filename), term_src).unwrap();
    String::from_utf8(
        cargo_bin_cmd!("genesis")
            .current_dir(dir)
            .args(["store", "--caps"])
            .arg(caps)
            .args(["put", "--input"])
            .arg(filename)
            .assert()
            .success()
            .get_output()
            .stdout
            .clone(),
    )
    .unwrap()
    .trim()
    .to_string()
}

#[test]
fn pkg_lock_install_verify_roundtrip_local_snapshot_selector() {
    let td = tempfile::tempdir().unwrap();
    let dir = td.path();
    let caps = write_caps(dir);

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

    // Snapshot produces store artifacts we can lock/install against.
    let snapshot_h = String::from_utf8(
        cargo_bin_cmd!("genesis")
            .current_dir(dir)
            .args(["pkg", "--caps"])
            .arg(&caps)
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

    // Init genesis.lock.
    let lock_h1 = String::from_utf8(
        cargo_bin_cmd!("genesis")
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
    assert!(
        predicate::str::is_match("^[0-9a-f]{64}$")
            .unwrap()
            .eval(&lock_h1)
    );
    assert!(dir.join("genesis.lock").exists());

    // Add requirement as snapshot selector.
    let lock_h2 = String::from_utf8(
        cargo_bin_cmd!("genesis")
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
    assert!(
        predicate::str::is_match("^[0-9a-f]{64}$")
            .unwrap()
            .eval(&lock_h2)
    );

    // Lock resolves requirements into [locked].
    let lock_h3 = String::from_utf8(
        cargo_bin_cmd!("genesis")
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
    assert!(
        predicate::str::is_match("^[0-9a-f]{64}$")
            .unwrap()
            .eval(&lock_h3)
    );

    // Running lock again is deterministic for the same store/refs state.
    cargo_bin_cmd!("genesis")
        .current_dir(dir)
        .args(["pkg", "--caps"])
        .arg(&caps)
        .args(["lock"])
        .assert()
        .success()
        .stdout(format!("{lock_h3}\n"));

    // Install verifies shallow closure exists in local store.
    cargo_bin_cmd!("genesis")
        .current_dir(dir)
        .args(["pkg", "--caps"])
        .arg(&caps)
        .args(["install", "--frozen"])
        .assert()
        .success()
        .stdout("ok\n");

    // Verify is strict (commit/evidence checks when present); snapshot-only locks still pass.
    cargo_bin_cmd!("genesis")
        .current_dir(dir)
        .args(["pkg", "--caps"])
        .arg(&caps)
        .args(["verify"])
        .assert()
        .success()
        .stdout("ok\n");

    // Deleting the store makes install fail with exit code 50 (verify).
    let store_dir = dir.join(".genesis").join("store");
    fs::remove_dir_all(&store_dir).unwrap();
    cargo_bin_cmd!("genesis")
        .current_dir(dir)
        .args(["pkg", "--caps"])
        .arg(&caps)
        .args(["install", "--frozen"])
        .assert()
        .failure()
        .code(50);
}

#[test]
fn pkg_lock_strict_rejects_commit_without_evidence() {
    let td = tempfile::tempdir().unwrap();
    let dir = td.path();
    let caps = write_caps(dir);

    // Build commit closure artifacts directly in store.
    let patch_h = store_put(dir, &caps, r#"{:type :vcs/patch :v 1 :ops []}"#, "patch.gc");
    let snap_h = store_put(
        dir,
        &caps,
        r#"{:type :vcs/snapshot :v 1 :kind :package :pkg/name "x" :pkg/version "0" :modules [] :obligations []}"#,
        "snap.gc",
    );
    let commit_h = store_put(
        dir,
        &caps,
        &format!(
            r#"{{
  :type :vcs/commit
  :v 1
  :parents []
  :target {{ :kind :package :name "x" }}
  :base nil
  :patch "{patch_h}"
  :result "{snap_h}"
  :obligations [core/obligation::unit-tests]
  :evidence []
  :attestations []
  :message "bad"
}}"#
        ),
        "commit.gc",
    );

    cargo_bin_cmd!("genesis")
        .current_dir(dir)
        .args(["pkg", "--caps"])
        .arg(&caps)
        .args(["init", "--workspace", "ws"])
        .assert()
        .success();

    cargo_bin_cmd!("genesis")
        .current_dir(dir)
        .args(["pkg", "--caps"])
        .arg(&caps)
        .args(["add"])
        .arg(format!("bad@commit:{commit_h}"))
        .assert()
        .success();

    // Non-strict lock still resolves commit pins.
    cargo_bin_cmd!("genesis")
        .current_dir(dir)
        .args(["pkg", "--caps"])
        .arg(&caps)
        .args(["lock"])
        .assert()
        .success();

    // Strict lock rejects commit entries with obligations but no evidence.
    cargo_bin_cmd!("genesis")
        .current_dir(dir)
        .args(["pkg", "--caps"])
        .arg(&caps)
        .args(["lock", "--strict"])
        .assert()
        .failure()
        .code(20);
}
