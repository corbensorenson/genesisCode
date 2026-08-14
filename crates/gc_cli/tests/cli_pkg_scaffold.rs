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

#[test]
fn gcpm_scaffold_covers_closed_archetype_decision_matrix() {
    let td = tempfile::tempdir().unwrap();
    let dir = td.path();
    let caps = write_caps(dir);
    let cases = [
        ("web", "gfx", "web", false),
        ("service", "backend", "service-runtime", false),
        ("desktop", "gfx", "desktop", false),
        ("mobile", "gpu", "ios", true),
        ("xr-game", "gfx", "web", false),
        ("data-ai", "gpu", "service-runtime", false),
    ];

    for (archetype, backend, primary, has_android) in cases {
        let root_name = format!("case-{archetype}");
        cargo_bin_cmd!("genesis")
            .current_dir(dir)
            .args(["--json", "gcpm", "--caps"])
            .arg(&caps)
            .args([
                "scaffold",
                "--archetype",
                archetype,
                "--name",
                "Decision Matrix",
                "--root",
                &root_name,
            ])
            .assert()
            .success();

        let root = dir.join(root_name);
        let workspace =
            gc_pkg::WorkspaceConfig::load(&root.join("genesis.workspace.toml")).unwrap();
        assert_eq!(workspace.defaults.runtime_backend.as_deref(), Some(backend));
        let deploy = fs::read_to_string(root.join("deploy/presets.toml")).unwrap();
        assert!(deploy.contains(&format!("archetype = \"{archetype}\"")));
        assert!(deploy.contains(&format!("primary_target = \"{primary}\"")));
        assert_eq!(
            deploy.contains("secondary_targets = [\"android\"]"),
            has_android
        );
    }
}

#[test]
fn gcpm_scaffold_round_trips_escaped_toml_metadata() {
    let td = tempfile::tempdir().unwrap();
    let dir = td.path();
    let caps = write_caps(dir);
    let policy = "policy:\"quoted\\line\n\t\u{1}";
    let registry = "gen://registry/\"quoted\\line\n\t\u{1}";

    cargo_bin_cmd!("genesis")
        .current_dir(dir)
        .args(["--json", "gcpm", "--caps"])
        .arg(&caps)
        .args([
            "scaffold",
            "--archetype",
            "web",
            "--name",
            "escaped-metadata",
            "--root",
            "escaped",
            "--policy",
            policy,
            "--registry-default",
            registry,
        ])
        .assert()
        .success();

    let root = dir.join("escaped");
    let workspace = gc_pkg::WorkspaceConfig::load(&root.join("genesis.workspace.toml")).unwrap();
    let lock = gc_pkg::GenesisLock::load(&root.join("genesis.lock")).unwrap();
    assert_eq!(workspace.defaults.policy.as_deref(), Some(policy));
    assert_eq!(workspace.defaults.registry.as_deref(), Some(registry));
    assert_eq!(lock.policy, policy);
    assert_eq!(
        lock.registries.get("default").map(String::as_str),
        Some(registry)
    );
}

#[test]
fn gcpm_scaffold_rejects_invalid_backend_without_mutation() {
    let td = tempfile::tempdir().unwrap();
    let dir = td.path();
    let caps = write_caps(dir);
    let root = dir.join("rejected");

    cargo_bin_cmd!("genesis")
        .current_dir(dir)
        .args(["--json", "gcpm", "--caps"])
        .arg(&caps)
        .args([
            "scaffold",
            "--archetype",
            "web",
            "--name",
            "rejected",
            "--root",
            "rejected",
            "--runtime-backend",
            "not-a-profile",
        ])
        .assert()
        .failure();

    assert!(
        !root.exists(),
        "rejected authority must not create its root"
    );
}

#[test]
fn gcpm_scaffold_preflights_late_collision_before_any_write() {
    let td = tempfile::tempdir().unwrap();
    let dir = td.path();
    let caps = write_caps(dir);
    let root = dir.join("collision");
    fs::create_dir_all(&root).unwrap();
    fs::write(root.join("caps.release.toml"), "retain-me\n").unwrap();

    cargo_bin_cmd!("genesis")
        .current_dir(dir)
        .args(["--json", "gcpm", "--caps"])
        .arg(&caps)
        .args([
            "scaffold",
            "--archetype",
            "web",
            "--name",
            "collision",
            "--root",
            "collision",
        ])
        .assert()
        .failure();

    assert_eq!(
        fs::read_to_string(root.join("caps.release.toml")).unwrap(),
        "retain-me\n"
    );
    assert!(!root.join("genesis.workspace.toml").exists());
    assert!(!root.join("src").exists());
}

#[cfg(unix)]
#[test]
fn gcpm_scaffold_rejects_parent_symlink_without_external_write() {
    let td = tempfile::tempdir().unwrap();
    let dir = td.path();
    let caps = write_caps(dir);
    let root = dir.join("symlinked");
    let outside = dir.join("outside");
    fs::create_dir_all(&root).unwrap();
    fs::create_dir_all(&outside).unwrap();
    std::os::unix::fs::symlink(&outside, root.join("src")).unwrap();

    cargo_bin_cmd!("genesis")
        .current_dir(dir)
        .args(["--json", "gcpm", "--caps"])
        .arg(&caps)
        .args([
            "scaffold",
            "--archetype",
            "web",
            "--name",
            "symlinked",
            "--root",
            "symlinked",
        ])
        .assert()
        .failure();

    assert!(!outside.join("main.gc").exists());
    assert!(!root.join("genesis.workspace.toml").exists());
}
