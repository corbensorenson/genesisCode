use std::fs;

use assert_cmd::cargo::cargo_bin_cmd;

#[path = "support/pkg_workspace_test_support.rs"]
mod pkg_workspace_test_support;
use pkg_workspace_test_support::{map_string, parse_coreform_value_map, write_caps};

#[test]
fn gcpm_scaffold_creates_archetype_workspace_package_and_presets() {
    let td = tempfile::tempdir().unwrap();
    let dir = td.path();
    let caps = write_caps(dir);

    let out = cargo_bin_cmd!("genesis")
        .current_dir(dir)
        .args(["--json", "gcpm", "--caps"])
        .arg(&caps)
        .args([
            "scaffold",
            "--archetype",
            "web",
            "--name",
            "ai-web-demo",
            "--root",
            "demo-app",
        ])
        .assert()
        .success()
        .get_output()
        .stdout
        .clone();

    let v: serde_json::Value = serde_json::from_slice(&out).unwrap();
    assert_eq!(
        v.get("kind").and_then(|x| x.as_str()),
        Some("genesis/pkg-scaffold-v0.1")
    );

    let root = dir.join("demo-app");
    for rel in [
        "genesis.workspace.toml",
        "genesis.lock",
        "package.toml",
        "src/main.gc",
        "deploy/presets.toml",
        "caps.toml",
        "caps.ci.toml",
        "caps.release.toml",
        "caps.backend.toml",
        "README.gcpm.md",
    ] {
        assert!(root.join(rel).is_file(), "missing scaffold file {rel}");
    }

    let ws_src = fs::read_to_string(root.join("genesis.workspace.toml")).unwrap();
    assert!(ws_src.contains("workspace = \"ai-web-demo\""));
    assert!(ws_src.contains("runtime_backend = \"gfx\""));
    assert!(ws_src.contains("[profiles.\"backend\"]"));
    assert!(ws_src.contains("caps_policy = \"caps.backend.toml\""));
    assert!(ws_src.contains("runtime_backend = \"backend\""));
    assert!(ws_src.contains("[tasks.\"build-primary\"]"));
    let backend_caps_src = fs::read_to_string(root.join("caps.backend.toml")).unwrap();
    let dev_caps_src = fs::read_to_string(root.join("caps.toml")).unwrap();
    let ci_caps_src = fs::read_to_string(root.join("caps.ci.toml")).unwrap();
    let release_caps_src = fs::read_to_string(root.join("caps.release.toml")).unwrap();
    assert!(dev_caps_src.contains("[task]"));
    assert!(dev_caps_src.contains("[runtime]"));
    assert!(dev_caps_src.contains("max_effect_ops = 1024"));
    assert!(dev_caps_src.contains("max_payload_bytes_per_run = 4194304"));
    assert!(dev_caps_src.contains("max_response_bytes_per_run = 4194304"));
    assert!(dev_caps_src.contains("max_time_ms_per_task = 4000"));
    assert!(ci_caps_src.contains("max_effect_ops = 256"));
    assert!(ci_caps_src.contains("max_time_ms_per_task = 2000"));
    assert!(ci_caps_src.contains("max_payload_bytes_per_run = 1048576"));
    assert!(release_caps_src.contains("max_effect_ops = 768"));
    assert!(release_caps_src.contains("max_time_ms_per_task = 3000"));
    assert!(release_caps_src.contains("max_payload_bytes_per_run = 2097152"));
    assert!(backend_caps_src.contains("io/net::http-request"));
    assert!(backend_caps_src.contains("host/ffi::call"));
    assert!(backend_caps_src.contains("allow_programs = [\"*\"]"));
    let preset_src = fs::read_to_string(root.join("deploy/presets.toml")).unwrap();
    assert!(preset_src.contains("archetype = \"web\""));
    assert!(preset_src.contains("primary_target = \"web\""));
}

#[test]
fn gcpm_scaffold_requires_force_to_overwrite_existing_files() {
    let td = tempfile::tempdir().unwrap();
    let dir = td.path();
    let caps = write_caps(dir);

    let run_scaffold = |force: bool| {
        let mut cmd = cargo_bin_cmd!("genesis");
        cmd.current_dir(dir)
            .args(["--json", "gcpm", "--caps"])
            .arg(&caps)
            .args([
                "scaffold",
                "--archetype",
                "service",
                "--name",
                "svc-core",
                "--root",
                "svc-app",
            ]);
        if force {
            cmd.arg("--force");
        }
        cmd
    };

    let first = run_scaffold(false)
        .assert()
        .success()
        .get_output()
        .stdout
        .clone();
    let first_map = parse_coreform_value_map(&first);
    let first_hash = map_string(&first_map, ":scaffold-h");
    let package_path = dir.join("svc-app").join("package.toml");
    fs::write(&package_path, "corrupted = true\n").unwrap();

    run_scaffold(false).assert().failure();

    let second = run_scaffold(true)
        .assert()
        .success()
        .get_output()
        .stdout
        .clone();
    let second_map = parse_coreform_value_map(&second);
    let second_hash = map_string(&second_map, ":scaffold-h");
    assert_eq!(
        first_hash, second_hash,
        "force rewrite must be deterministic"
    );

    let repaired = fs::read_to_string(package_path).unwrap();
    assert!(repaired.contains("name = \"svc-core-service\""));
}
