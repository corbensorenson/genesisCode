use std::fs;
use std::path::{Path, PathBuf};

use assert_cmd::cargo::cargo_bin_cmd;

fn write_caps(dir: &Path) -> PathBuf {
    let caps = dir.join("caps.toml");
    fs::write(&caps, "allow = []\n").unwrap();
    caps
}

fn write_caps_with_store_remote(dir: &Path, remote: &str, remote_allow: &str) -> PathBuf {
    let caps = dir.join("caps.toml");
    fs::write(
        &caps,
        format!(
            r#"
allow = [
  "core/store::get"
]

[store]
dir = "./.genesis/store"
remote = "{remote}"
remote_allow = ["{remote_allow}"]
"#
        ),
    )
    .unwrap();
    caps
}

fn put_remote_artifact(remote_dir: &Path, hex: &str, bytes: &[u8]) {
    let store = remote_dir.join("v1").join("store");
    fs::create_dir_all(&store).unwrap();
    fs::write(store.join(hex), bytes).unwrap();
}

#[test]
fn gcpm_new_creates_workspace_descriptor_and_lock() {
    let td = tempfile::tempdir().unwrap();
    let dir = td.path();
    let caps = write_caps(dir);

    let out = cargo_bin_cmd!("genesis")
        .current_dir(dir)
        .args(["--json", "gcpm", "--caps"])
        .arg(&caps)
        .args([
            "new",
            "--workspace",
            "ws",
            "--policy",
            "policy:default-v0.1",
            "--registry-default",
            "gen://registry",
        ])
        .assert()
        .success()
        .get_output()
        .stdout
        .clone();

    let v: serde_json::Value = serde_json::from_slice(&out).unwrap();
    assert_eq!(
        v.get("kind").and_then(|x| x.as_str()),
        Some("genesis/pkg-new-v0.1")
    );
    assert!(dir.join("genesis.lock").exists());
    assert!(dir.join("genesis.workspace.toml").exists());
    let ws_src = fs::read_to_string(dir.join("genesis.workspace.toml")).unwrap();
    assert!(ws_src.contains("[[members]]"));
    assert!(ws_src.contains("[profiles.\"dev\"]"));
}

#[test]
fn gcpm_remove_deletes_requirement_and_locked_entry() {
    let td = tempfile::tempdir().unwrap();
    let dir = td.path();
    let caps = write_caps(dir);

    fs::write(
        dir.join("genesis.lock"),
        r#"
version = 1
workspace = "ws"
policy = "policy:default-v0.1"

[requirements]
"dep" = { selector = "snapshot:aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa", update_policy = "manual", registry = "default" }

[locked]
"dep" = { snapshot = "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa", source_selector = "snapshot:aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa" }
"#,
    )
    .unwrap();

    let out = cargo_bin_cmd!("genesis")
        .current_dir(dir)
        .args(["--json", "gcpm", "--caps"])
        .arg(&caps)
        .args(["remove", "dep", "--lock", "genesis.lock"])
        .assert()
        .success()
        .get_output()
        .stdout
        .clone();

    let v: serde_json::Value = serde_json::from_slice(&out).unwrap();
    assert_eq!(
        v.get("kind").and_then(|x| x.as_str()),
        Some("genesis/pkg-remove-v0.1")
    );
    assert_eq!(
        v.pointer("/data/value")
            .and_then(|x| x.as_str())
            .map(|s| s.contains(":removed true")),
        Some(true)
    );
    let lock_src = fs::read_to_string(dir.join("genesis.lock")).unwrap();
    assert!(!lock_src.contains("\"dep\" ="));
}

#[test]
fn gcpm_migrate_creates_workspace_and_lock_from_package_manifest() {
    let td = tempfile::tempdir().unwrap();
    let dir = td.path();
    let caps = write_caps(dir);
    fs::write(dir.join("lib.gc"), "(def lib::x 1)\nlib::x\n").unwrap();
    fs::write(
        dir.join("package.toml"),
        r#"
name = "mini"
version = "0.1.0"
obligations = []
dependencies = [{ name = "dep", path = "deps/dep", hash = "bbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb" }]

[[modules]]
path = "lib.gc"
"#,
    )
    .unwrap();
    let out = cargo_bin_cmd!("genesis")
        .current_dir(dir)
        .args(["--json", "gcpm", "--caps"])
        .arg(&caps)
        .args([
            "migrate",
            "--pkg",
            "package.toml",
            "--workspace",
            "mono",
            "--registry-default",
            "gen://registry",
        ])
        .assert()
        .success()
        .get_output()
        .stdout
        .clone();

    let v: serde_json::Value = serde_json::from_slice(&out).unwrap();
    assert_eq!(
        v.get("kind").and_then(|x| x.as_str()),
        Some("genesis/pkg-migrate-v0.1")
    );
    let ws_src = fs::read_to_string(dir.join("genesis.workspace.toml")).unwrap();
    assert!(ws_src.contains("workspace = \"mono\""));
    assert!(ws_src.contains("[tasks.\"test\"]"));

    let lock_src = fs::read_to_string(dir.join("genesis.lock")).unwrap();
    assert!(lock_src.contains("\"dep\" ="));
    assert!(
        lock_src
            .contains("snapshot:bbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb")
    );
}

#[test]
fn gcpm_test_alias_runs_package_obligations() {
    let td = tempfile::tempdir().unwrap();
    let dir = td.path();
    let caps = write_caps(dir);
    fs::write(dir.join("lib.gc"), "(def mini::x 1)\nmini::x\n").unwrap();
    fs::write(
        dir.join("package.toml"),
        r#"
name = "mini"
version = "0.1.0"
obligations = []
dependencies = []

[[modules]]
path = "lib.gc"
"#,
    )
    .unwrap();
    cargo_bin_cmd!("genesis")
        .current_dir(dir)
        .args(["--json", "pack", "--pkg", "package.toml"])
        .assert()
        .success();

    let out = cargo_bin_cmd!("genesis")
        .current_dir(dir)
        .args(["--json", "gcpm", "--caps"])
        .arg(&caps)
        .args(["test", "--pkg", "package.toml"])
        .assert()
        .success()
        .get_output()
        .stdout
        .clone();

    let v: serde_json::Value = serde_json::from_slice(&out).unwrap();
    assert_eq!(
        v.get("kind").and_then(|x| x.as_str()),
        Some("genesis/test-v0.2")
    );
    assert_eq!(v.get("ok").and_then(|x| x.as_bool()), Some(true));
}

#[test]
fn gcpm_run_executes_workspace_task_without_shell_glue() {
    let td = tempfile::tempdir().unwrap();
    let dir = td.path();
    let caps = write_caps(dir);
    fs::write(dir.join("lib.gc"), "(def mini::x 1)\nmini::x\n").unwrap();
    fs::write(
        dir.join("package.toml"),
        r#"
name = "mini"
version = "0.1.0"
obligations = []
dependencies = []

[[modules]]
path = "lib.gc"
"#,
    )
    .unwrap();
    cargo_bin_cmd!("genesis")
        .current_dir(dir)
        .args(["--json", "pack", "--pkg", "package.toml"])
        .assert()
        .success();

    cargo_bin_cmd!("genesis")
        .current_dir(dir)
        .args(["--json", "gcpm", "--caps"])
        .arg(&caps)
        .args([
            "migrate",
            "--pkg",
            "package.toml",
            "--workspace",
            "mono",
            "--registry-default",
            "gen://registry",
        ])
        .assert()
        .success();

    let mut ws_src = fs::read_to_string(dir.join("genesis.workspace.toml")).unwrap();
    ws_src.push_str(
        r#"
[tasks."build-local"]
cmd = "build"
pkg = "package.toml"

[tasks."lint-local"]
cmd = "lint"
pkg = "package.toml"
"#,
    );
    fs::write(dir.join("genesis.workspace.toml"), ws_src).unwrap();

    let out = cargo_bin_cmd!("genesis")
        .current_dir(dir)
        .args(["--json", "gcpm", "--caps"])
        .arg(&caps)
        .args(["run", "test"])
        .assert()
        .success()
        .get_output()
        .stdout
        .clone();

    let v: serde_json::Value = serde_json::from_slice(&out).unwrap();
    assert_eq!(
        v.get("kind").and_then(|x| x.as_str()),
        Some("genesis/test-v0.2")
    );
    assert_eq!(v.get("ok").and_then(|x| x.as_bool()), Some(true));

    let out_build = cargo_bin_cmd!("genesis")
        .current_dir(dir)
        .args(["--json", "gcpm", "--caps"])
        .arg(&caps)
        .args(["run", "build-local"])
        .assert()
        .success()
        .get_output()
        .stdout
        .clone();
    let vb: serde_json::Value = serde_json::from_slice(&out_build).unwrap();
    assert_eq!(
        vb.get("kind").and_then(|x| x.as_str()),
        Some("genesis/pack-v0.2")
    );

    let out_lint = cargo_bin_cmd!("genesis")
        .current_dir(dir)
        .args(["--json", "gcpm", "--caps"])
        .arg(&caps)
        .args(["run", "lint-local"])
        .assert()
        .code(30)
        .get_output()
        .stdout
        .clone();
    let vl: serde_json::Value = serde_json::from_slice(&out_lint).unwrap();
    assert_eq!(
        vl.get("kind").and_then(|x| x.as_str()),
        Some("genesis/typecheck-v0.2")
    );
}

#[test]
fn gcpm_env_materializes_deterministic_profile_record() {
    let td = tempfile::tempdir().unwrap();
    let dir = td.path();
    let caps = write_caps(dir);
    fs::write(dir.join("caps.ci.toml"), "allow = []\n").unwrap();
    fs::write(dir.join("caps.release.toml"), "allow = []\n").unwrap();

    cargo_bin_cmd!("genesis")
        .current_dir(dir)
        .args(["--json", "gcpm", "--caps"])
        .arg(&caps)
        .args([
            "new",
            "--workspace",
            "ws",
            "--policy",
            "policy:default-v0.1",
            "--registry-default",
            "gen://registry",
        ])
        .assert()
        .success();

    let out = cargo_bin_cmd!("genesis")
        .current_dir(dir)
        .args(["--json", "gcpm", "--caps"])
        .arg(&caps)
        .args(["env", "--profile", "dev"])
        .assert()
        .success()
        .get_output()
        .stdout
        .clone();
    let v: serde_json::Value = serde_json::from_slice(&out).unwrap();
    assert_eq!(
        v.get("kind").and_then(|x| x.as_str()),
        Some("genesis/pkg-env-v0.1")
    );
    let env_root = dir.join(".genesis").join("env");
    assert!(env_root.exists());
    let entries: Vec<_> = fs::read_dir(&env_root)
        .unwrap()
        .map(|e| e.unwrap().path())
        .collect();
    assert_eq!(entries.len(), 1);
    assert!(entries[0].join("env.gcenv").is_file());
    assert!(entries[0].join("provenance.gc").is_file());
    assert!(entries[0].join("workspace.toml").is_file());
    assert!(entries[0].join("genesis.lock").is_file());
    assert!(entries[0].join("profile.gc").is_file());
    assert!(entries[0].join("members.gc").is_file());
    assert!(entries[0].join("deps.gc").is_file());
    assert!(entries[0].join("caps-policy.toml").is_file());
}

#[test]
fn gcpm_env_hydrate_fetches_missing_locked_artifacts_via_store_get() {
    let td = tempfile::tempdir().unwrap();
    let dir = td.path();

    let remote_dir = dir.join("remote-registry");
    fs::create_dir_all(&remote_dir).unwrap();
    let remote = format!("file://{}/", remote_dir.display());
    let remote_allow = format!("{remote}v1/");
    let caps = write_caps_with_store_remote(dir, &remote, &remote_allow);

    fs::write(dir.join("caps.ci.toml"), "allow = []\n").unwrap();
    fs::write(dir.join("caps.release.toml"), "allow = []\n").unwrap();

    cargo_bin_cmd!("genesis")
        .current_dir(dir)
        .args(["--json", "gcpm", "--caps"])
        .arg(&caps)
        .args([
            "new",
            "--workspace",
            "ws",
            "--policy",
            "policy:default-v0.1",
            "--registry-default",
            "gen://registry",
        ])
        .assert()
        .success();

    let snapshot = gc_coreform::parse_term("{:type :vcs/snapshot :v 1 :kind :package}").unwrap();
    let snapshot_bytes = gc_coreform::print_term(&snapshot).into_bytes();
    let snapshot_h = blake3::hash(&snapshot_bytes).to_hex().to_string();
    put_remote_artifact(&remote_dir, &snapshot_h, &snapshot_bytes);

    let commit = gc_coreform::parse_term(&format!(
        "{{:type :vcs/commit :v 1 :parents [] :base nil :patch nil :result \"{snapshot_h}\" :obligations [] :evidence []}}"
    ))
    .unwrap();
    let commit_bytes = gc_coreform::print_term(&commit).into_bytes();
    let commit_h = blake3::hash(&commit_bytes).to_hex().to_string();
    put_remote_artifact(&remote_dir, &commit_h, &commit_bytes);

    fs::write(
        dir.join("genesis.lock"),
        format!(
            r#"
version = 1
workspace = "ws"
policy = "policy:default-v0.1"

[requirements]
"dep" = {{ selector = "commit:{commit_h}", update_policy = "manual", registry = "default" }}

[locked]
"dep" = {{ commit = "{commit_h}", snapshot = "{snapshot_h}", registry = "default", source_selector = "commit:{commit_h}" }}
"#
        ),
    )
    .unwrap();

    cargo_bin_cmd!("genesis")
        .current_dir(dir)
        .args(["--json", "gcpm", "--caps"])
        .arg(&caps)
        .args(["env", "--profile", "dev"])
        .assert()
        .code(10);

    let log = dir.join("env-hydrate.gclog");
    let out = cargo_bin_cmd!("genesis")
        .current_dir(dir)
        .args(["--json", "gcpm", "--caps"])
        .arg(&caps)
        .args(["--log"])
        .arg(&log)
        .args(["env", "--profile", "dev", "--hydrate"])
        .assert()
        .success()
        .get_output()
        .stdout
        .clone();
    let v: serde_json::Value = serde_json::from_slice(&out).unwrap();
    assert_eq!(
        v.get("kind").and_then(|x| x.as_str()),
        Some("genesis/pkg-env-v0.1")
    );
    assert!(
        dir.join(".genesis")
            .join("store")
            .join(&snapshot_h)
            .is_file()
    );
    assert!(dir.join(".genesis").join("store").join(&commit_h).is_file());

    let log_src = fs::read_to_string(log).unwrap();
    assert!(log_src.contains("core/store::get"));
}
