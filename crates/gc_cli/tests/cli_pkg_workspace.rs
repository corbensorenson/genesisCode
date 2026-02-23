use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;

use assert_cmd::cargo::cargo_bin_cmd;
use gc_coreform::{Term, TermOrdKey};

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

fn parse_coreform_value_map(stdout: &[u8]) -> std::collections::BTreeMap<TermOrdKey, Term> {
    let v: serde_json::Value = serde_json::from_slice(stdout).unwrap();
    let value = v.pointer("/data/value").and_then(|x| x.as_str()).unwrap();
    let t = gc_coreform::parse_term(value).unwrap();
    let Term::Map(m) = t else {
        panic!("expected map value");
    };
    m
}

fn map_string(map: &std::collections::BTreeMap<TermOrdKey, Term>, key: &str) -> String {
    map.get(&TermOrdKey(Term::symbol(key)))
        .and_then(|t| match t {
            Term::Str(s) => Some(s.clone()),
            _ => None,
        })
        .unwrap_or_else(|| panic!("expected string at key `{key}`"))
}

fn map_map<'a>(
    map: &'a std::collections::BTreeMap<TermOrdKey, Term>,
    key: &str,
) -> &'a std::collections::BTreeMap<TermOrdKey, Term> {
    map.get(&TermOrdKey(Term::symbol(key)))
        .and_then(|t| match t {
            Term::Map(m) => Some(m),
            _ => None,
        })
        .unwrap_or_else(|| panic!("expected map at key `{key}`"))
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
    assert!(ws_src.contains("runtime_backend ="));
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
fn gcpm_build_target_is_reproducible_and_emits_provenance_bundle() {
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
        .args(["pack", "--pkg", "package.toml"])
        .assert()
        .success();

    let out_a = cargo_bin_cmd!("genesis")
        .current_dir(dir)
        .args(["--json", "gcpm", "--caps"])
        .arg(&caps)
        .args([
            "build",
            "--pkg",
            "package.toml",
            "--target",
            "web",
            "--out-dir",
            ".genesis/build-targets",
        ])
        .assert()
        .success()
        .get_output()
        .stdout
        .clone();
    let json_a: serde_json::Value = serde_json::from_slice(&out_a).unwrap();
    assert_eq!(
        json_a.get("kind").and_then(|x| x.as_str()),
        Some("genesis/pkg-build-v0.1")
    );
    let map_a = parse_coreform_value_map(&out_a);
    let bundle_root_raw = map_string(&map_a, ":bundle-root");
    let bundle_root_rel = PathBuf::from(&bundle_root_raw);
    let bundle_root = if bundle_root_rel.is_absolute() {
        bundle_root_rel
    } else {
        dir.join(bundle_root_rel)
    };
    let bundle_h = map_string(&map_a, ":bundle-h");
    assert_eq!(map_string(&map_a, ":target"), "web".to_string());
    assert!(bundle_root.join("build_manifest.gc").is_file());
    assert!(bundle_root.join("provenance.gc").is_file());
    assert!(bundle_root.join("package.toml").is_file());
    assert!(bundle_root.join("package_artifact.txt").is_file());
    let manifest_before = fs::read_to_string(bundle_root.join("build_manifest.gc")).unwrap();
    let provenance_before = fs::read_to_string(bundle_root.join("provenance.gc")).unwrap();

    let out_b = cargo_bin_cmd!("genesis")
        .current_dir(dir)
        .args(["--json", "gcpm", "--caps"])
        .arg(&caps)
        .args([
            "build",
            "--pkg",
            "package.toml",
            "--target",
            "web",
            "--out-dir",
            ".genesis/build-targets",
        ])
        .assert()
        .success()
        .get_output()
        .stdout
        .clone();
    let map_b = parse_coreform_value_map(&out_b);
    assert_eq!(map_string(&map_b, ":bundle-h"), bundle_h);
    assert_eq!(map_string(&map_b, ":bundle-root"), bundle_root_raw);

    let manifest_after = fs::read_to_string(bundle_root.join("build_manifest.gc")).unwrap();
    let provenance_after = fs::read_to_string(bundle_root.join("provenance.gc")).unwrap();
    assert_eq!(manifest_before, manifest_after);
    assert_eq!(provenance_before, provenance_after);

    let manifest_t = gc_coreform::parse_term(&manifest_before).unwrap();
    let Term::Map(manifest_m) = manifest_t else {
        panic!("build manifest must be map");
    };
    assert_eq!(
        manifest_m.get(&TermOrdKey(Term::symbol(":type"))),
        Some(&Term::symbol(":gcpm/build-manifest"))
    );
    assert_eq!(
        manifest_m.get(&TermOrdKey(Term::symbol(":target"))),
        Some(&Term::Str("web".to_string()))
    );

    let provenance_t = gc_coreform::parse_term(&provenance_before).unwrap();
    let Term::Map(provenance_m) = provenance_t else {
        panic!("provenance must be map");
    };
    assert_eq!(
        provenance_m.get(&TermOrdKey(Term::symbol(":type"))),
        Some(&Term::symbol(":gcpm/build-provenance"))
    );
    assert_eq!(
        provenance_m.get(&TermOrdKey(Term::symbol(":bundle-h"))),
        Some(&Term::Str(bundle_h))
    );
}

#[test]
fn gcpm_build_supports_mobile_and_edge_target_contracts() {
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
        .args(["pack", "--pkg", "package.toml"])
        .assert()
        .success();

    let targets = [
        ("ios", "native", "mobile-ios", "ios-app-bundle-v1"),
        (
            "android",
            "native",
            "mobile-android",
            "android-app-bundle-v1",
        ),
        (
            "edge",
            "wasm32-wasi-preview2",
            "edge-runtime",
            "edge-wasi-bundle-v1",
        ),
        (
            "service-runtime",
            "wasm32-wasi-preview2",
            "service-runtime",
            "service-runtime-bundle-v1",
        ),
    ];

    for (target, expected_runtime, expected_host, expected_artifact_format) in targets {
        let out = cargo_bin_cmd!("genesis")
            .current_dir(dir)
            .args(["--json", "gcpm", "--caps"])
            .arg(&caps)
            .args([
                "build",
                "--pkg",
                "package.toml",
                "--target",
                target,
                "--out-dir",
                ".genesis/build-targets-ext",
            ])
            .assert()
            .success()
            .get_output()
            .stdout
            .clone();
        let map = parse_coreform_value_map(&out);
        assert_eq!(map_string(&map, ":target"), target);
        let bundle_root_raw = map_string(&map, ":bundle-root");
        let bundle_root_rel = PathBuf::from(&bundle_root_raw);
        let bundle_root = if bundle_root_rel.is_absolute() {
            bundle_root_rel
        } else {
            dir.join(bundle_root_rel)
        };
        let build_manifest_src =
            fs::read_to_string(bundle_root.join("build_manifest.gc")).expect("read build_manifest");
        let build_manifest =
            gc_coreform::parse_term(&build_manifest_src).expect("parse build_manifest");
        let Term::Map(build_manifest_map) = build_manifest else {
            panic!("build_manifest must be map");
        };
        assert_eq!(
            map_string(&build_manifest_map, ":pipeline-kind"),
            "executable-target-bundle-v2"
        );
        let profile = map_map(&build_manifest_map, ":target-profile");
        assert_eq!(
            map_string(profile, ":runtime"),
            expected_runtime,
            "target {target} runtime mismatch"
        );
        assert_eq!(
            map_string(profile, ":host-profile"),
            expected_host,
            "target {target} host-profile mismatch"
        );
        assert_eq!(
            map_string(profile, ":artifact-format"),
            expected_artifact_format,
            "target {target} artifact-format mismatch"
        );

        let (package_rel, signature_rel, launch_rel) = match target {
            "web" => (
                "artifact/package.webbundle",
                "artifact/package.webbundle.sig",
                "artifact/launch_web.sh",
            ),
            "desktop" => (
                "artifact/package.desktop.app",
                "artifact/package.desktop.app.sig",
                "artifact/launch_desktop.sh",
            ),
            "service" => (
                "artifact/package.service.bin",
                "artifact/package.service.bin.sig",
                "artifact/launch_service.sh",
            ),
            "ios" => (
                "artifact/package.ipa",
                "artifact/package.ipa.sig",
                "artifact/launch_ios.sh",
            ),
            "android" => (
                "artifact/package.aab",
                "artifact/package.aab.sig",
                "artifact/launch_android.sh",
            ),
            "edge" => (
                "artifact/package.edge.wasm",
                "artifact/package.edge.wasm.sig",
                "artifact/launch_edge.sh",
            ),
            "service-runtime" => (
                "artifact/package.service-runtime.wasm",
                "artifact/package.service-runtime.wasm.sig",
                "artifact/launch_service_runtime.sh",
            ),
            other => panic!("unexpected target {other}"),
        };
        let package_path = bundle_root.join(package_rel);
        let signature_path = bundle_root.join(signature_rel);
        let launch_script = bundle_root.join(launch_rel);
        assert!(
            package_path.is_file(),
            "target {target} missing package artifact"
        );
        assert!(
            signature_path.is_file(),
            "target {target} missing package signature"
        );
        assert!(
            launch_script.is_file(),
            "target {target} missing launch executable"
        );

        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            let launch_mode = fs::metadata(&launch_script).unwrap().permissions().mode();
            assert_ne!(
                launch_mode & 0o111,
                0,
                "target {target} launch script not executable"
            );
        }

        let bundle_h = map_string(&map, ":bundle-h");
        let boot_out = Command::new("bash")
            .arg(&launch_script)
            .arg("--boot")
            .output()
            .expect("run boot lane");
        assert!(
            boot_out.status.success(),
            "target {target} boot lane failed: {:?}",
            boot_out
        );
        assert_eq!(
            String::from_utf8(boot_out.stdout).unwrap().trim(),
            format!("boot-ok:{target}:{bundle_h}")
        );

        let smoke_out = Command::new("bash")
            .arg(&launch_script)
            .arg("--smoke")
            .output()
            .expect("run smoke lane");
        assert!(
            smoke_out.status.success(),
            "target {target} smoke lane failed: {:?}",
            smoke_out
        );
        assert_eq!(
            String::from_utf8(smoke_out.stdout).unwrap().trim(),
            format!("smoke-ok:{target}:{bundle_h}")
        );
    }
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
fn gcpm_run_fails_closed_on_incompatible_workspace_runtime_backend_profile() {
    let td = tempfile::tempdir().unwrap();
    let dir = td.path();
    let caps = write_caps(dir);
    fs::write(dir.join("module.gc"), "(def m::x 1)\nm::x\n").unwrap();
    fs::write(
        dir.join("genesis.workspace.toml"),
        r#"
version = 1
workspace = "ws"

[[members]]
name = "ws"
path = "."
role = "root"

[defaults]
policy = "policy:default-v0.1"
runtime_backend = "backend"

[tasks."eval-local"]
cmd = "eval"
file = "module.gc"
"#,
    )
    .unwrap();

    cargo_bin_cmd!("genesis")
        .current_dir(dir)
        .args(["--json", "gcpm", "--caps"])
        .arg(&caps)
        .args(["run", "eval-local"])
        .assert()
        .code(10)
        .stdout(predicates::str::contains(
            "incompatible with active runtime backend profile",
        ));
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

#[test]
fn gcpm_env_runtime_backend_profile_contract_is_machine_readable() {
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

    let out_default = cargo_bin_cmd!("genesis")
        .current_dir(dir)
        .args(["--json", "gcpm", "--caps"])
        .arg(&caps)
        .args(["env", "--profile", "dev"])
        .assert()
        .success()
        .get_output()
        .stdout
        .clone();
    let map_default = parse_coreform_value_map(&out_default);
    let active = map_default
        .get(&TermOrdKey(Term::symbol(":active-runtime-backend-profile")))
        .and_then(|t| match t {
            Term::Str(s) => Some(s.clone()),
            _ => None,
        })
        .unwrap();
    let selected_default = map_default
        .get(&TermOrdKey(Term::symbol(":runtime-backend-profile")))
        .and_then(|t| match t {
            Term::Str(s) => Some(s.clone()),
            _ => None,
        })
        .unwrap();
    assert!(!selected_default.is_empty());
    assert!(!active.is_empty());
    assert_eq!(
        map_default.get(&TermOrdKey(Term::symbol(":runtime-backend-compatible"))),
        Some(&Term::Bool(true))
    );

    let out_override = cargo_bin_cmd!("genesis")
        .current_dir(dir)
        .args(["--json", "gcpm", "--caps"])
        .arg(&caps)
        .args([
            "env",
            "--profile",
            "dev",
            "--runtime-backend",
            "profile-headless",
        ])
        .assert()
        .success()
        .get_output()
        .stdout
        .clone();
    let map_override = parse_coreform_value_map(&out_override);
    assert_eq!(
        map_override.get(&TermOrdKey(Term::symbol(":runtime-backend-profile"))),
        Some(&Term::Str("headless".to_string()))
    );
    assert_eq!(
        map_override.get(&TermOrdKey(Term::symbol(":runtime-backend-compatible"))),
        Some(&Term::Bool(true))
    );

    if active != "backend" {
        cargo_bin_cmd!("genesis")
            .current_dir(dir)
            .args(["--json", "gcpm", "--caps"])
            .arg(&caps)
            .args(["env", "--profile", "dev", "--runtime-backend", "backend"])
            .assert()
            .code(10)
            .stdout(predicates::str::contains(
                "incompatible with active runtime backend",
            ));
    }
}

#[test]
fn gcpm_profile_runtime_emits_profile_artifact_and_history() {
    let td = tempfile::tempdir().unwrap();
    let dir = td.path();
    let caps = write_caps(dir);
    let out_file = dir.join("runtime_profile.gc");
    let history_file = dir.join("runtime_profile_history.jsonl");

    let out = cargo_bin_cmd!("genesis")
        .current_dir(dir)
        .args(["--json", "gcpm", "--caps"])
        .arg(&caps)
        .args([
            "profile-runtime",
            "--out",
            "runtime_profile.gc",
            "--history",
            "runtime_profile_history.jsonl",
            "--min-history",
            "999",
            "--task-budget-us",
            "20000000",
            "--io-budget-us",
            "20000000",
            "--memory-budget-us",
            "20000000",
        ])
        .assert()
        .success()
        .get_output()
        .stdout
        .clone();
    let v: serde_json::Value = serde_json::from_slice(&out).unwrap();
    assert_eq!(
        v.get("kind").and_then(|x| x.as_str()),
        Some("genesis/pkg-runtime-profile-v0.1")
    );
    assert_eq!(v.get("ok").and_then(|x| x.as_bool()), Some(true));

    let map = parse_coreform_value_map(&out);
    assert_eq!(
        map.get(&TermOrdKey(Term::symbol(":ok"))),
        Some(&Term::Bool(true))
    );
    assert!(map.contains_key(&TermOrdKey(Term::symbol(":task-elapsed-us"))));
    assert!(map.contains_key(&TermOrdKey(Term::symbol(":io-elapsed-us"))));
    assert!(map.contains_key(&TermOrdKey(Term::symbol(":memory-elapsed-us"))));
    assert!(out_file.is_file());
    assert!(history_file.is_file());

    let profile_src = fs::read_to_string(&out_file).unwrap();
    let profile_t = gc_coreform::parse_term(&profile_src).unwrap();
    let Term::Map(profile_m) = profile_t else {
        panic!("runtime profile artifact must be map");
    };
    assert_eq!(
        profile_m.get(&TermOrdKey(Term::symbol(":kind"))),
        Some(&Term::symbol(":runtime-profile"))
    );
    assert!(profile_m.contains_key(&TermOrdKey(Term::symbol(":task-scheduler"))));
    assert!(profile_m.contains_key(&TermOrdKey(Term::symbol(":io-store-cycle"))));
    assert!(profile_m.contains_key(&TermOrdKey(Term::symbol(":memory-pressure"))));
}

#[test]
fn gcpm_profile_runtime_fails_closed_when_budget_is_exceeded() {
    let td = tempfile::tempdir().unwrap();
    let dir = td.path();
    let caps = write_caps(dir);

    let out = cargo_bin_cmd!("genesis")
        .current_dir(dir)
        .args(["--json", "gcpm", "--caps"])
        .arg(&caps)
        .args([
            "profile-runtime",
            "--out",
            "runtime_profile.gc",
            "--history",
            "runtime_profile_history.jsonl",
            "--no-history-append",
            "--task-budget-us",
            "1",
            "--io-budget-us",
            "1",
            "--memory-budget-us",
            "1",
        ])
        .assert()
        .code(50)
        .get_output()
        .stdout
        .clone();
    let v: serde_json::Value = serde_json::from_slice(&out).unwrap();
    assert_eq!(
        v.get("kind").and_then(|x| x.as_str()),
        Some("genesis/pkg-runtime-profile-v0.1")
    );
    assert_eq!(v.get("ok").and_then(|x| x.as_bool()), Some(false));

    let map = parse_coreform_value_map(&out);
    assert_eq!(
        map.get(&TermOrdKey(Term::symbol(":ok"))),
        Some(&Term::Bool(false))
    );
    assert_eq!(
        map.get(&TermOrdKey(Term::symbol(":task-budget-ok"))),
        Some(&Term::Bool(false))
    );
}
