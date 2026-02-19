use assert_cmd::cargo::cargo_bin_cmd;
use predicates::prelude::*;
use serde_json::Value as JsonValue;
use std::path::{Path, PathBuf};
use tempfile::tempdir;

mod support;

fn build_selfhost_artifact(dir: &std::path::Path) -> std::path::PathBuf {
    support::copy_repo_toolchain_artifact(dir)
}

fn assert_not_unsupported_selfhost_only(stderr: &[u8]) {
    let s = String::from_utf8_lossy(stderr);
    assert!(
        !s.contains("selfhost-only mode currently supports only"),
        "unexpected selfhost-only unsupported-cmd gate error: {s}"
    );
}

fn write_effect_caps(dir: &Path, allow: &[&str]) -> PathBuf {
    let caps = dir.join("caps.toml");
    let mut s = String::new();
    s.push_str("allow = [");
    for (i, op) in allow.iter().enumerate() {
        if i != 0 {
            s.push_str(", ");
        }
        s.push('"');
        s.push_str(op);
        s.push('"');
    }
    s.push_str(
        "]\n\n[store]\ndir = \"./.genesis/store\"\n\n[refs]\npath = \"./.genesis/refs.gc\"\n\n[op.\"core/pkg-low::save-lock\"]\nbase_dir = \".\"\ncreate_dirs = true\n\n[op.\"core/pkg-low::load-lock\"]\nbase_dir = \".\"\n",
    );
    std::fs::write(&caps, s).unwrap();
    caps
}

fn write_sync_caps(dir: &Path) -> PathBuf {
    let caps = dir.join("caps_sync.toml");
    std::fs::write(
        &caps,
        r#"
allow = ["core/sync::pull"]

[store]
dir = "./.genesis/store"

[refs]
path = "./.genesis/refs.gc"

[op."core/sync::pull"]
remote_allow = ["file://"]
"#,
    )
    .unwrap();
    caps
}

fn write_vcs_caps(dir: &Path) -> PathBuf {
    let caps = dir.join("caps_vcs.toml");
    std::fs::write(
        &caps,
        r#"
allow = ["core/vcs-low::log", "core/store::get", "core/refs::get", "core/refs::list"]

[store]
dir = "./.genesis/store"

[refs]
path = "./.genesis/refs.gc"
"#,
    )
    .unwrap();
    caps
}

#[test]
fn selfhost_only_rejects_rust_engine_for_fmt() {
    let dir = tempdir().unwrap();
    let file = dir.path().join("m.gc");
    std::fs::write(&file, "(def x 1)\n").unwrap();

    cargo_bin_cmd!("genesis")
        .args([
            "--selfhost-only",
            "fmt",
            file.to_str().unwrap(),
            "--engine",
            "rust",
        ])
        .assert()
        .failure()
        .code(50)
        .stderr(predicate::str::contains(
            "selfhost-only mode requires --engine selfhost",
        ));
}

#[test]
fn selfhost_only_rejects_non_artifact_bootstrap_mode() {
    let dir = tempdir().unwrap();
    let file = dir.path().join("m.gc");
    std::fs::write(&file, "(def x 1)\n").unwrap();

    cargo_bin_cmd!("genesis")
        .args([
            "--selfhost-only",
            "--selfhost-bootstrap",
            "embedded",
            "fmt",
            file.to_str().unwrap(),
            "--engine",
            "selfhost",
        ])
        .assert()
        .failure()
        .code(50)
        .stderr(predicate::str::contains(
            "selfhost-only mode requires --selfhost-bootstrap artifact-only",
        ));
}

#[test]
fn selfhost_only_requires_explicit_artifact_for_runtime_commands() {
    let dir = tempdir().unwrap();
    let file = dir.path().join("m.gc");
    std::fs::write(&file, "(def x 1)\n").unwrap();

    cargo_bin_cmd!("genesis")
        .args([
            "--selfhost-only",
            "fmt",
            file.to_str().unwrap(),
            "--engine",
            "selfhost",
        ])
        .assert()
        .failure()
        .code(50)
        .stderr(predicate::str::contains(
            "explicit selfhost artifact required",
        ));
}

#[test]
fn selfhost_only_accepts_fmt_selfhost_with_artifact() {
    let dir = tempdir().unwrap();
    let artifact = build_selfhost_artifact(dir.path());
    let file = dir.path().join("m.gc");
    std::fs::write(&file, "(def  x 1)\n x\n").unwrap();

    cargo_bin_cmd!("genesis")
        .args([
            "--selfhost-only",
            "--no-step-limit",
            "--selfhost-artifact",
            artifact.to_str().unwrap(),
            "fmt",
            file.to_str().unwrap(),
            "--engine",
            "selfhost",
        ])
        .assert()
        .success();
}

#[test]
fn selfhost_runtime_json_reports_explicit_artifact_identity() {
    let dir = tempdir().unwrap();
    let artifact = build_selfhost_artifact(dir.path());
    let file = dir.path().join("m.gc");
    std::fs::write(&file, "1\n").unwrap();

    let out = cargo_bin_cmd!("genesis")
        .args([
            "--json",
            "--selfhost-only",
            "--selfhost-artifact",
            artifact.to_str().unwrap(),
            "eval",
            file.to_str().unwrap(),
            "--engine",
            "selfhost",
        ])
        .assert()
        .success()
        .get_output()
        .stdout
        .clone();
    let v: JsonValue = serde_json::from_slice(&out).unwrap();
    let art = &v["data"]["selfhost_artifact"];
    assert_eq!(art["source"].as_str(), Some("explicit"), "{v}");
    assert!(
        art["path"].as_str().is_some_and(|s| !s.is_empty()),
        "missing artifact path in {v}"
    );
    assert!(
        art["hash"].as_str().is_some_and(|s| s.len() == 64),
        "missing artifact hash in {v}"
    );
}

#[test]
fn selfhost_only_accepts_selfhost_artifact_and_keygen() {
    let dir = tempdir().unwrap();
    let bootstrap = build_selfhost_artifact(dir.path());
    let out_artifact = dir.path().join("toolchain.gc");

    let artifact_out = cargo_bin_cmd!("genesis")
        .args([
            "--selfhost-only",
            "--selfhost-artifact",
            bootstrap.to_str().unwrap(),
            "selfhost-artifact",
            "--out",
            out_artifact.to_str().unwrap(),
        ])
        .output()
        .unwrap();
    assert_not_unsupported_selfhost_only(&artifact_out.stderr);
    assert!(out_artifact.exists());

    let out_key = dir.path().join("key.toml");
    cargo_bin_cmd!("genesis")
        .args([
            "--selfhost-only",
            "keygen",
            "--out",
            out_key.to_str().unwrap(),
        ])
        .assert()
        .success();
    assert!(out_key.exists());
}

#[test]
fn selfhost_only_accepts_sign_transparency_verify_command_families() {
    let dir = tempdir().unwrap();
    let key = dir.path().join("key.toml");
    cargo_bin_cmd!("genesis")
        .args(["--selfhost-only", "keygen", "--out"])
        .arg(&key)
        .assert()
        .success();
    let missing_pkg = dir.path().join("missing-package.toml");
    let acceptance = "0".repeat(64);

    let sign_out = cargo_bin_cmd!("genesis")
        .args([
            "--selfhost-only",
            "sign",
            "--pkg",
            missing_pkg.to_str().unwrap(),
            "--key",
            key.to_str().unwrap(),
            "--acceptance",
        ])
        .arg(&acceptance)
        .output()
        .unwrap();
    assert_not_unsupported_selfhost_only(&sign_out.stderr);
    assert!(!sign_out.status.success());

    let tv_out = cargo_bin_cmd!("genesis")
        .args([
            "--selfhost-only",
            "transparency-verify",
            "--pkg",
            missing_pkg.to_str().unwrap(),
        ])
        .output()
        .unwrap();
    assert_not_unsupported_selfhost_only(&tv_out.stderr);
    assert!(!tv_out.status.success());

    let verify_out = cargo_bin_cmd!("genesis")
        .args([
            "--selfhost-only",
            "verify",
            "--pkg",
            missing_pkg.to_str().unwrap(),
            "--acceptance",
        ])
        .arg(&acceptance)
        .output()
        .unwrap();
    assert_not_unsupported_selfhost_only(&verify_out.stderr);
    assert!(!verify_out.status.success());
}

#[test]
fn selfhost_only_accepts_policy_command_family() {
    let dir = tempdir().unwrap();
    let policies = dir.path().join("policies.toml");

    cargo_bin_cmd!("genesis")
        .current_dir(dir.path())
        .args(["--selfhost-only", "policy", "list", "--policies"])
        .arg(&policies)
        .assert()
        .success();

    let show_out = cargo_bin_cmd!("genesis")
        .current_dir(dir.path())
        .args([
            "--selfhost-only",
            "policy",
            "show",
            "policy:missing",
            "--policies",
        ])
        .arg(&policies)
        .output()
        .unwrap();
    assert_not_unsupported_selfhost_only(&show_out.stderr);
    assert!(!show_out.status.success());

    let set_default_out = cargo_bin_cmd!("genesis")
        .current_dir(dir.path())
        .args([
            "--selfhost-only",
            "policy",
            "set-default",
            "policy:missing",
            "--policies",
        ])
        .arg(&policies)
        .output()
        .unwrap();
    assert_not_unsupported_selfhost_only(&set_default_out.stderr);
    assert!(!set_default_out.status.success());
}

#[test]
fn selfhost_only_accepts_store_refs_pkg_and_gc() {
    let dir = tempdir().unwrap();
    let caps = write_effect_caps(
        dir.path(),
        &[
            "core/store::put",
            "core/refs::get",
            "core/pkg-low::save-lock",
            "core/pkg-low::load-lock",
            "core/gc-low::pin",
        ],
    );
    let term = dir.path().join("value.gc");
    std::fs::write(&term, "{:hello true}\n").unwrap();

    cargo_bin_cmd!("genesis")
        .current_dir(dir.path())
        .args(["--selfhost-only", "store", "--caps"])
        .arg(caps.to_str().unwrap())
        .args(["put", "--input"])
        .arg(term.to_str().unwrap())
        .assert()
        .success();

    cargo_bin_cmd!("genesis")
        .current_dir(dir.path())
        .args(["--selfhost-only", "refs", "--caps"])
        .arg(caps.to_str().unwrap())
        .args(["get", "refs/heads/main"])
        .assert()
        .success();

    cargo_bin_cmd!("genesis")
        .current_dir(dir.path())
        .args(["--selfhost-only", "pkg", "--caps"])
        .arg(caps.to_str().unwrap())
        .args(["init", "--workspace", "ws"])
        .assert()
        .success();

    cargo_bin_cmd!("genesis")
        .current_dir(dir.path())
        .args(["--selfhost-only", "pkg", "--caps"])
        .arg(caps.to_str().unwrap())
        .args(["list"])
        .assert()
        .success();

    cargo_bin_cmd!("genesis")
        .current_dir(dir.path())
        .args(["--selfhost-only", "gc", "--caps"])
        .arg(caps.to_str().unwrap())
        .args(["pin", "refs/heads/main", "--pins", ".genesis/pins.toml"])
        .assert()
        .success();
}

#[test]
fn selfhost_only_pkg_update_uses_pkg_low_caps_only() {
    let dir = tempdir().unwrap();
    let caps = write_effect_caps(
        dir.path(),
        &["core/pkg-low::save-lock", "core/pkg-low::load-lock"],
    );

    cargo_bin_cmd!("genesis")
        .current_dir(dir.path())
        .args(["--selfhost-only", "pkg", "--caps"])
        .arg(caps.to_str().unwrap())
        .args(["init", "--workspace", "ws"])
        .assert()
        .success();

    cargo_bin_cmd!("genesis")
        .current_dir(dir.path())
        .args(["--selfhost-only", "pkg", "--caps"])
        .arg(caps.to_str().unwrap())
        .args(["update", "--lock", "genesis.lock"])
        .assert()
        .success();
}

#[test]
fn selfhost_only_accepts_gcpm_alias_command_group() {
    let dir = tempdir().unwrap();
    let caps = write_effect_caps(
        dir.path(),
        &["core/pkg-low::save-lock", "core/pkg-low::load-lock"],
    );

    cargo_bin_cmd!("genesis")
        .current_dir(dir.path())
        .args(["--selfhost-only", "gcpm", "--caps"])
        .arg(caps.to_str().unwrap())
        .args(["init", "--workspace", "ws"])
        .assert()
        .success();

    cargo_bin_cmd!("genesis")
        .current_dir(dir.path())
        .args(["--selfhost-only", "gcpm", "--caps"])
        .arg(caps.to_str().unwrap())
        .args(["list"])
        .assert()
        .success();
}

#[test]
fn gcpm_alias_preserves_pkg_json_kind_contract() {
    let dir = tempdir().unwrap();
    let caps = write_effect_caps(
        dir.path(),
        &["core/pkg-low::save-lock", "core/pkg-low::load-lock"],
    );

    let out = cargo_bin_cmd!("genesis")
        .current_dir(dir.path())
        .args(["--json", "--selfhost-only", "gcpm", "--caps"])
        .arg(caps.to_str().unwrap())
        .args(["init", "--workspace", "ws"])
        .assert()
        .success()
        .get_output()
        .stdout
        .clone();
    let v: JsonValue = serde_json::from_slice(&out).unwrap();
    let kind = v.get("kind").and_then(JsonValue::as_str).unwrap();
    assert_eq!(kind, "genesis/pkg-init-v0.1");
}

#[test]
fn selfhost_only_pkg_lock_non_strict_uses_pkg_low_caps_only() {
    let dir = tempdir().unwrap();
    let caps = write_effect_caps(
        dir.path(),
        &["core/pkg-low::save-lock", "core/pkg-low::load-lock"],
    );

    cargo_bin_cmd!("genesis")
        .current_dir(dir.path())
        .args(["--selfhost-only", "pkg", "--caps"])
        .arg(caps.to_str().unwrap())
        .args(["init", "--workspace", "ws"])
        .assert()
        .success();

    cargo_bin_cmd!("genesis")
        .current_dir(dir.path())
        .args(["--selfhost-only", "pkg", "--caps"])
        .arg(caps.to_str().unwrap())
        .args([
            "add",
            "dep@snapshot:0123456789abcdef0123456789abcdef0123456789abcdef0123456789abcdef",
        ])
        .assert()
        .success();

    cargo_bin_cmd!("genesis")
        .current_dir(dir.path())
        .args(["--selfhost-only", "pkg", "--caps"])
        .arg(caps.to_str().unwrap())
        .args(["lock", "--lock", "genesis.lock"])
        .assert()
        .success();
}

#[test]
fn selfhost_only_pkg_lock_strict_uses_pkg_low_caps_only() {
    let dir = tempdir().unwrap();
    let caps = write_effect_caps(
        dir.path(),
        &[
            "core/pkg-low::save-lock",
            "core/pkg-low::load-lock",
            "core/store::put",
            "core/store::get",
        ],
    );

    cargo_bin_cmd!("genesis")
        .current_dir(dir.path())
        .args(["--selfhost-only", "pkg", "--caps"])
        .arg(caps.to_str().unwrap())
        .args(["init", "--workspace", "ws"])
        .assert()
        .success();

    let snapshot_file = dir.path().join("snapshot.gc");
    std::fs::write(
        &snapshot_file,
        "{:type :vcs/snapshot :v 1 :kind :package :modules [] :obligations []}\n",
    )
    .unwrap();

    let put_out = cargo_bin_cmd!("genesis")
        .current_dir(dir.path())
        .args(["--selfhost-only", "store", "--caps"])
        .arg(caps.to_str().unwrap())
        .args(["put", "--input", "snapshot.gc"])
        .assert()
        .success()
        .get_output()
        .stdout
        .clone();
    let snapshot_h = String::from_utf8(put_out).unwrap().trim().to_string();

    cargo_bin_cmd!("genesis")
        .current_dir(dir.path())
        .args(["--selfhost-only", "pkg", "--caps"])
        .arg(caps.to_str().unwrap())
        .args(["add", &format!("dep@snapshot:{snapshot_h}")])
        .assert()
        .success();

    cargo_bin_cmd!("genesis")
        .current_dir(dir.path())
        .args(["--selfhost-only", "pkg", "--caps"])
        .arg(caps.to_str().unwrap())
        .args(["lock", "--strict", "--lock", "genesis.lock"])
        .assert()
        .success();
}

#[test]
fn selfhost_only_accepts_sync_command_group() {
    let dir = tempdir().unwrap();
    let caps = write_sync_caps(dir.path());
    let remote = format!("file://{}", dir.path().join("remote-registry").display());
    let root_h = "0".repeat(64);

    cargo_bin_cmd!("genesis")
        .current_dir(dir.path())
        .args(["--selfhost-only", "sync", "--caps"])
        .arg(caps.to_str().unwrap())
        .args(["pull", "--remote"])
        .arg(remote)
        .args(["--root"])
        .arg(&root_h)
        .assert()
        .failure()
        .code(20);
}

#[test]
fn selfhost_only_accepts_policy_command_group() {
    let dir = tempdir().unwrap();
    let policies = dir.path().join("policies.toml");
    cargo_bin_cmd!("genesis")
        .current_dir(dir.path())
        .args(["--selfhost-only", "policy", "list", "--policies"])
        .arg(&policies)
        .assert()
        .success()
        .stdout(predicate::str::contains("default"));
}

#[test]
fn selfhost_only_accepts_vcs_command_group() {
    let dir = tempdir().unwrap();
    let caps = write_vcs_caps(dir.path());
    let root_h = "0".repeat(64);

    cargo_bin_cmd!("genesis")
        .current_dir(dir.path())
        .args(["--selfhost-only", "vcs", "--caps"])
        .arg(caps.to_str().unwrap())
        .args(["log"])
        .arg(&root_h)
        .assert()
        .failure()
        .code(20);
}

#[test]
fn selfhost_only_rejects_rust_engine_for_optimize() {
    let dir = tempdir().unwrap();
    let file = dir.path().join("m.gc");
    std::fs::write(&file, "(def x 1)\nx\n").unwrap();

    cargo_bin_cmd!("genesis")
        .args([
            "--selfhost-only",
            "optimize",
            file.to_str().unwrap(),
            "--engine",
            "rust",
        ])
        .assert()
        .failure()
        .code(50)
        .stderr(predicate::str::contains(
            "selfhost-only mode requires --engine selfhost",
        ));
}

#[test]
fn selfhost_only_rejects_rust_coreform_frontend_for_pack() {
    let pkg = concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/../../tests/spec/pkg_basic/package.toml"
    );

    cargo_bin_cmd!("genesis")
        .args([
            "--selfhost-only",
            "--coreform-frontend",
            "rust",
            "pack",
            "--pkg",
            pkg,
        ])
        .assert()
        .failure()
        .code(50)
        .stderr(predicate::str::contains(
            "selfhost-only mode requires --coreform-frontend selfhost",
        ));
}

#[test]
fn selfhost_only_accepts_typecheck_with_selfhost_artifact() {
    let dir = tempdir().unwrap();
    let artifact = build_selfhost_artifact(dir.path());
    let pkg = concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/../../tests/spec/pkg_basic/package.toml"
    );

    cargo_bin_cmd!("genesis")
        .args([
            "--selfhost-only",
            "--no-step-limit",
            "--selfhost-artifact",
            artifact.to_str().unwrap(),
            "typecheck",
            "--pkg",
            pkg,
        ])
        .assert()
        .success();
}

#[test]
fn selfhost_only_accepts_test_with_selfhost_artifact() {
    let dir = tempdir().unwrap();
    let artifact = build_selfhost_artifact(dir.path());
    let pkg = concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/../../tests/spec/pkg_basic/package.toml"
    );

    cargo_bin_cmd!("genesis")
        .args([
            "--selfhost-only",
            "--selfhost-artifact",
            artifact.to_str().unwrap(),
            "test",
            "--pkg",
            pkg,
        ])
        .assert()
        .success();
}

#[test]
fn selfhost_only_accepts_apply_patch_with_selfhost_artifact() {
    let dir = tempdir().unwrap();
    let artifact = build_selfhost_artifact(dir.path());
    let fixture = std::path::Path::new(concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/../../tests/spec/pkg_basic"
    ));
    for name in ["basic.gc", "caps.toml", "package.toml", "pure.gcpatch"] {
        std::fs::copy(fixture.join(name), dir.path().join(name)).unwrap();
    }
    let patch = dir.path().join("pure.gcpatch");
    let pkg = dir.path().join("package.toml");

    cargo_bin_cmd!("genesis")
        .args([
            "--selfhost-only",
            "--selfhost-artifact",
            artifact.to_str().unwrap(),
            "apply-patch",
            patch.to_str().unwrap(),
            "--pkg",
            pkg.to_str().unwrap(),
        ])
        .assert()
        .success();
}

#[test]
fn selfhost_only_accepts_pack_with_selfhost_artifact() {
    let dir = tempdir().unwrap();
    let artifact = build_selfhost_artifact(dir.path());
    let fixture = std::path::Path::new(concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/../../tests/spec/pkg_basic"
    ));
    for name in ["basic.gc", "caps.toml", "package.toml"] {
        std::fs::copy(fixture.join(name), dir.path().join(name)).unwrap();
    }
    let pkg = dir.path().join("package.toml");

    cargo_bin_cmd!("genesis")
        .args([
            "--selfhost-only",
            "--selfhost-artifact",
            artifact.to_str().unwrap(),
            "pack",
            "--pkg",
            pkg.to_str().unwrap(),
        ])
        .assert()
        .success();
}

#[test]
fn selfhost_only_accepts_vcs_hash_with_selfhost_artifact() {
    let dir = tempdir().unwrap();
    let artifact = build_selfhost_artifact(dir.path());
    let file = dir.path().join("m.gc");
    std::fs::write(&file, "(def x 1)\nx\n").unwrap();

    let out = cargo_bin_cmd!("genesis")
        .args([
            "--json",
            "--selfhost-only",
            "--selfhost-artifact",
            artifact.to_str().unwrap(),
            "vcs",
            "hash",
            "--in",
            file.to_str().unwrap(),
        ])
        .assert()
        .success()
        .get_output()
        .stdout
        .clone();
    let v: JsonValue = serde_json::from_slice(&out).unwrap();
    let kind = v
        .get("data")
        .and_then(|d| d.get("hash_kind"))
        .and_then(JsonValue::as_str)
        .unwrap();
    assert_eq!(kind, "module");
}

#[test]
fn selfhost_only_accepts_explain_with_selfhost_artifact() {
    let dir = tempdir().unwrap();
    let artifact = build_selfhost_artifact(dir.path());
    let file = dir.path().join("m.gc");
    std::fs::write(
        &file,
        r#"
          (def c (core/contract::make (fn (msg) nil) nil {}))
          c
        "#,
    )
    .unwrap();

    cargo_bin_cmd!("genesis")
        .args([
            "--selfhost-only",
            "--selfhost-artifact",
            artifact.to_str().unwrap(),
            "--no-step-limit",
            "explain",
            file.to_str().unwrap(),
            "--engine",
            "selfhost",
            "--contract",
            "c",
            "--msg",
            "(msg foo nil)",
        ])
        .assert()
        .success();
}

#[test]
fn selfhost_only_accepts_run_and_replay_with_selfhost_artifact() {
    let dir = tempdir().unwrap();
    let artifact = build_selfhost_artifact(dir.path());
    let file = dir.path().join("prog.gc");
    std::fs::write(
        &file,
        r#"
          (def prog (core/effect::pure 42))
          prog
        "#,
    )
    .unwrap();
    let caps = dir.path().join("caps.toml");
    std::fs::write(&caps, "allow = []\n").unwrap();
    let log = dir.path().join("out.gclog");

    cargo_bin_cmd!("genesis")
        .args([
            "--selfhost-only",
            "--selfhost-artifact",
            artifact.to_str().unwrap(),
            "--no-step-limit",
            "run",
            file.to_str().unwrap(),
            "--engine",
            "selfhost",
            "--caps",
            caps.to_str().unwrap(),
            "--log",
            log.to_str().unwrap(),
        ])
        .assert()
        .success();

    cargo_bin_cmd!("genesis")
        .args([
            "--selfhost-only",
            "--selfhost-artifact",
            artifact.to_str().unwrap(),
            "--no-step-limit",
            "replay",
            file.to_str().unwrap(),
            "--engine",
            "selfhost",
            "--log",
            log.to_str().unwrap(),
        ])
        .assert()
        .success();
}

#[test]
fn selfhost_only_rejects_legacy_pkg_semantic_fallback_in_run_logs() {
    let dir = tempdir().unwrap();
    let artifact = build_selfhost_artifact(dir.path());
    let file = dir.path().join("legacy.gc");
    std::fs::write(
        &file,
        r#"
          (def prog
            (((core/effect::perform (quote core/pkg::init))
               {:workspace "legacy-ws"
                :lock "legacy.lock"
                :policy "policy:default-v0.1"
                :registry-default nil})
             (fn (r) (core/effect::pure r))))
          prog
        "#,
    )
    .unwrap();
    let caps = dir.path().join("caps_legacy_pkg.toml");
    std::fs::write(
        &caps,
        r#"
allow = ["core/pkg-low::init"]

[op."core/pkg-low::init"]
base_dir = "."
create_dirs = true
"#,
    )
    .unwrap();

    cargo_bin_cmd!("genesis")
        .args([
            "--selfhost-only",
            "--selfhost-artifact",
            artifact.to_str().unwrap(),
            "--no-step-limit",
            "run",
            file.to_str().unwrap(),
            "--engine",
            "selfhost",
            "--caps",
            caps.to_str().unwrap(),
        ])
        .assert()
        .failure()
        .code(50)
        .stderr(predicate::str::contains(
            "selfhost-only mode detected legacy semantic fallback",
        ))
        .stderr(predicate::str::contains("core/pkg::init"));
}

#[test]
fn selfhost_only_rejects_legacy_gc_semantic_fallback_in_run_logs() {
    let dir = tempdir().unwrap();
    let artifact = build_selfhost_artifact(dir.path());
    let file = dir.path().join("legacy_gc.gc");
    std::fs::write(
        &file,
        r#"
          (def prog
            (((core/effect::perform (quote core/gc::pin))
               {:pins "pins.toml"
                :target "refs/heads/main"})
             (fn (r) (core/effect::pure r))))
          prog
        "#,
    )
    .unwrap();
    let caps = dir.path().join("caps_legacy_gc.toml");
    std::fs::write(
        &caps,
        r#"
allow = ["core/gc-low::pin"]

[store]
dir = "./.genesis/store"

[refs]
path = "./.genesis/refs.gc"

[op."core/gc-low::pin"]
base_dir = "."
create_dirs = true
"#,
    )
    .unwrap();

    cargo_bin_cmd!("genesis")
        .args([
            "--selfhost-only",
            "--selfhost-artifact",
            artifact.to_str().unwrap(),
            "--no-step-limit",
            "run",
            file.to_str().unwrap(),
            "--engine",
            "selfhost",
            "--caps",
            caps.to_str().unwrap(),
        ])
        .assert()
        .failure()
        .code(50)
        .stderr(predicate::str::contains(
            "selfhost-only mode detected legacy semantic fallback",
        ))
        .stderr(predicate::str::contains("core/gc::pin"));
}

#[test]
fn selfhost_only_rejects_legacy_gpk_semantic_fallback_in_run_logs() {
    let dir = tempdir().unwrap();
    let artifact = build_selfhost_artifact(dir.path());
    std::fs::write(dir.path().join("bad.gpk"), b"not-a-gpk-bundle").unwrap();
    let file = dir.path().join("legacy_gpk.gc");
    std::fs::write(
        &file,
        r#"
          (def prog
            (((core/effect::perform (quote core/gpk::import))
               {:in "bad.gpk"})
             (fn (r) (core/effect::pure r))))
          prog
        "#,
    )
    .unwrap();
    let caps = dir.path().join("caps_legacy_gpk.toml");
    std::fs::write(
        &caps,
        r#"
allow = ["core/gpk-low::import"]

[store]
dir = "./.genesis/store"

[refs]
path = "./.genesis/refs.gc"

[op."core/gpk-low::import"]
base_dir = "."
"#,
    )
    .unwrap();

    cargo_bin_cmd!("genesis")
        .args([
            "--selfhost-only",
            "--selfhost-artifact",
            artifact.to_str().unwrap(),
            "--no-step-limit",
            "run",
            file.to_str().unwrap(),
            "--engine",
            "selfhost",
            "--caps",
            caps.to_str().unwrap(),
        ])
        .assert()
        .failure()
        .code(50)
        .stderr(predicate::str::contains(
            "selfhost-only mode detected legacy semantic fallback",
        ))
        .stderr(predicate::str::contains("core/gpk::import"));
}

#[test]
fn pack_prefers_selfhost_when_artifact_flag_is_set_even_without_selfhost_only() {
    let dir = tempdir().unwrap();
    let fixture = std::path::Path::new(concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/../../tests/spec/pkg_basic"
    ));
    for name in ["basic.gc", "caps.toml", "package.toml"] {
        std::fs::copy(fixture.join(name), dir.path().join(name)).unwrap();
    }
    let pkg = dir.path().join("package.toml");
    let bad_artifact = dir.path().join("bad_toolchain.gc");
    std::fs::write(&bad_artifact, "{ :kind \"bad\" }\n").unwrap();

    cargo_bin_cmd!("genesis")
        .args([
            "--selfhost-artifact",
            bad_artifact.to_str().unwrap(),
            "pack",
            "--pkg",
            pkg.to_str().unwrap(),
        ])
        .assert()
        .failure()
        .code(10)
        .stderr(predicate::str::contains(
            "selfhost artifact bootstrap required",
        ));
}

#[test]
fn pack_prefers_selfhost_when_default_artifact_path_exists() {
    let dir = tempdir().unwrap();
    let fixture = std::path::Path::new(concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/../../tests/spec/pkg_basic"
    ));
    for name in ["basic.gc", "caps.toml", "package.toml"] {
        std::fs::copy(fixture.join(name), dir.path().join(name)).unwrap();
    }
    let pkg = dir.path().join("package.toml");
    let toolchain_dir = dir.path().join(".genesis").join("selfhost");
    std::fs::create_dir_all(&toolchain_dir).unwrap();
    std::fs::write(toolchain_dir.join("toolchain.gc"), "{ :kind \"bad\" }\n").unwrap();

    cargo_bin_cmd!("genesis")
        .args(["pack", "--pkg", pkg.to_str().unwrap()])
        .current_dir(dir.path())
        .assert()
        .failure()
        .code(10)
        .stderr(predicate::str::contains(
            "selfhost artifact bootstrap required",
        ));
}

#[test]
fn fmt_prefers_selfhost_when_artifact_flag_is_set_without_engine() {
    let dir = tempdir().unwrap();
    let file = dir.path().join("m.gc");
    let bad_artifact = dir.path().join("bad_toolchain.gc");
    std::fs::write(&file, "(def x 1)\n").unwrap();
    std::fs::write(&bad_artifact, "{ :kind \"bad\" }\n").unwrap();

    cargo_bin_cmd!("genesis")
        .args([
            "--selfhost-artifact",
            bad_artifact.to_str().unwrap(),
            "fmt",
            file.to_str().unwrap(),
        ])
        .assert()
        .failure()
        .code(1)
        .stderr(predicate::str::contains(
            "selfhost artifact bootstrap required",
        ));
}

#[test]
fn eval_prefers_selfhost_when_artifact_flag_is_set_without_engine() {
    let dir = tempdir().unwrap();
    let file = dir.path().join("m.gc");
    let bad_artifact = dir.path().join("bad_toolchain.gc");
    std::fs::write(&file, "1\n").unwrap();
    std::fs::write(&bad_artifact, "{ :kind \"bad\" }\n").unwrap();

    cargo_bin_cmd!("genesis")
        .args([
            "--selfhost-artifact",
            bad_artifact.to_str().unwrap(),
            "eval",
            file.to_str().unwrap(),
        ])
        .assert()
        .failure()
        .code(1)
        .stderr(predicate::str::contains(
            "selfhost artifact bootstrap required",
        ));
}

#[test]
fn optimize_prefers_selfhost_when_artifact_flag_is_set_without_engine() {
    let dir = tempdir().unwrap();
    let file = dir.path().join("m.gc");
    let bad_artifact = dir.path().join("bad_toolchain.gc");
    std::fs::write(&file, "(def x 1)\nx\n").unwrap();
    std::fs::write(&bad_artifact, "{ :kind \"bad\" }\n").unwrap();

    cargo_bin_cmd!("genesis")
        .args([
            "--selfhost-artifact",
            bad_artifact.to_str().unwrap(),
            "optimize",
            file.to_str().unwrap(),
        ])
        .assert()
        .failure()
        .code(1)
        .stderr(predicate::str::contains(
            "selfhost artifact bootstrap required",
        ));
}

#[test]
fn rust_engine_requires_compat_flag_and_can_override_when_enabled() {
    let dir = tempdir().unwrap();
    let file = dir.path().join("m.gc");
    let bad_artifact = dir.path().join("bad_toolchain.gc");
    std::fs::write(&file, "(def x 1)\n").unwrap();
    std::fs::write(&bad_artifact, "{ :kind \"bad\" }\n").unwrap();

    cargo_bin_cmd!("genesis")
        .args([
            "--selfhost-artifact",
            bad_artifact.to_str().unwrap(),
            "fmt",
            file.to_str().unwrap(),
            "--engine",
            "rust",
        ])
        .assert()
        .failure()
        .code(50)
        .stderr(predicate::str::contains(
            "dedicated parity harness binaries",
        ));

    cargo_bin_cmd!("genesis_parity")
        .args([
            "--selfhost-artifact",
            bad_artifact.to_str().unwrap(),
            "fmt",
            file.to_str().unwrap(),
            "--engine",
            "rust",
        ])
        .assert()
        .success();
}

#[test]
fn default_profile_rejects_rust_coreform_frontend_without_compat_opt_in() {
    let dir = tempdir().unwrap();
    let fixture = std::path::Path::new(concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/../../tests/spec/pkg_basic"
    ));
    for name in ["basic.gc", "caps.toml", "package.toml", "pure.gcpatch"] {
        std::fs::copy(fixture.join(name), dir.path().join(name)).unwrap();
    }
    let pkg = dir.path().join("package.toml");
    let patch = dir.path().join("pure.gcpatch");

    for args in [
        vec!["pack", "--pkg", pkg.to_str().unwrap()],
        vec!["test", "--pkg", pkg.to_str().unwrap()],
        vec!["typecheck", "--pkg", pkg.to_str().unwrap()],
        vec![
            "apply-patch",
            patch.to_str().unwrap(),
            "--pkg",
            pkg.to_str().unwrap(),
        ],
    ] {
        cargo_bin_cmd!("genesis")
            .args(["--coreform-frontend", "rust"])
            .args(&args)
            .assert()
            .failure()
            .code(50)
            .stderr(predicate::str::contains(
                "dedicated parity harness binaries",
            ));
    }

    // Explicit compat opt-in is still available.
    cargo_bin_cmd!("genesis_parity")
        .args(["--coreform-frontend", "rust", "pack", "--pkg"])
        .arg(&pkg)
        .assert()
        .success();
}

#[test]
fn fmt_default_selfhost_requires_explicit_artifact() {
    let dir = tempdir().unwrap();
    let file = dir.path().join("m.gc");
    std::fs::write(&file, "(def x 1)\n").unwrap();

    cargo_bin_cmd!("genesis")
        .args(["fmt", file.to_str().unwrap()])
        .current_dir(dir.path())
        .assert()
        .failure()
        .code(50)
        .stderr(predicate::str::contains(
            "explicit selfhost artifact required",
        ));
}

#[test]
fn selfhost_only_full_production_workflow_runs_without_rust_fallbacks() {
    let dir = tempdir().unwrap();
    let artifact = build_selfhost_artifact(dir.path());
    let fixture = std::path::Path::new(concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/../../tests/spec/pkg_basic"
    ));
    for name in ["basic.gc", "caps.toml", "package.toml", "pure.gcpatch"] {
        std::fs::copy(fixture.join(name), dir.path().join(name)).unwrap();
    }

    let module = dir.path().join("basic.gc");
    let pkg = dir.path().join("package.toml");
    let patch = dir.path().join("pure.gcpatch");
    let optimized = dir.path().join("basic.opt.gc");
    let run_prog = dir.path().join("prog.gc");
    let run_caps = dir.path().join("caps_run.toml");
    let run_log = dir.path().join("run.gclog");
    std::fs::write(
        &run_prog,
        r#"
          (def prog (core/effect::pure 42))
          prog
        "#,
    )
    .unwrap();
    std::fs::write(&run_caps, "allow = []\n").unwrap();

    let common = [
        "--selfhost-only",
        "--selfhost-artifact",
        artifact.to_str().unwrap(),
    ];

    cargo_bin_cmd!("genesis")
        .args(common)
        .args(["fmt", module.to_str().unwrap(), "--engine", "selfhost"])
        .assert()
        .success();

    cargo_bin_cmd!("genesis")
        .args(common)
        .args(["eval", module.to_str().unwrap(), "--engine", "selfhost"])
        .assert()
        .success();

    cargo_bin_cmd!("genesis")
        .args(common)
        .args([
            "run",
            run_prog.to_str().unwrap(),
            "--engine",
            "selfhost",
            "--caps",
            run_caps.to_str().unwrap(),
            "--log",
            run_log.to_str().unwrap(),
        ])
        .assert()
        .success();

    cargo_bin_cmd!("genesis")
        .args(common)
        .args([
            "replay",
            run_prog.to_str().unwrap(),
            "--engine",
            "selfhost",
            "--log",
            run_log.to_str().unwrap(),
        ])
        .assert()
        .success();

    cargo_bin_cmd!("genesis")
        .args(common)
        .args(["pack", "--pkg", pkg.to_str().unwrap()])
        .assert()
        .success();

    cargo_bin_cmd!("genesis")
        .args(common)
        .args(["test", "--pkg", pkg.to_str().unwrap()])
        .assert()
        .success();

    cargo_bin_cmd!("genesis")
        .args(common)
        .args(["typecheck", "--pkg", pkg.to_str().unwrap()])
        .assert()
        .success();

    cargo_bin_cmd!("genesis")
        .args(common)
        .args([
            "optimize",
            module.to_str().unwrap(),
            "--engine",
            "selfhost",
            "--out",
            optimized.to_str().unwrap(),
        ])
        .assert()
        .success();
    assert!(optimized.exists());

    cargo_bin_cmd!("genesis")
        .args(common)
        .args([
            "apply-patch",
            patch.to_str().unwrap(),
            "--pkg",
            pkg.to_str().unwrap(),
        ])
        .assert()
        .success();

    cargo_bin_cmd!("genesis")
        .args(common)
        .args(["pack", "--pkg", pkg.to_str().unwrap()])
        .assert()
        .success();
}

#[test]
fn legacy_high_level_caps_ops_are_rejected_in_default_profile() {
    let dir = tempdir().unwrap();
    let artifact = build_selfhost_artifact(dir.path());
    let file = dir.path().join("prog.gc");
    std::fs::write(&file, "(def prog (core/effect::pure 1))\nprog\n").unwrap();
    let caps = dir.path().join("caps_legacy.toml");
    std::fs::write(
        &caps,
        r#"
allow = ["core/pkg::init"]
"#,
    )
    .unwrap();

    cargo_bin_cmd!("genesis")
        .args([
            "--json",
            "--selfhost-artifact",
            artifact.to_str().unwrap(),
            "run",
            file.to_str().unwrap(),
            "--engine",
            "selfhost",
            "--caps",
            caps.to_str().unwrap(),
        ])
        .assert()
        .failure()
        .code(10);
}
