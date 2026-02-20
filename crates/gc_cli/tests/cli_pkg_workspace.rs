use std::fs;
use std::path::{Path, PathBuf};

use assert_cmd::cargo::cargo_bin_cmd;

fn write_caps(dir: &Path) -> PathBuf {
    let caps = dir.join("caps.toml");
    fs::write(&caps, "allow = []\n").unwrap();
    caps
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
