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
        "]\n\n[store]\ndir = \"./.genesis/store\"\n\n[refs]\npath = \"./.genesis/refs.gc\"\n\n[op.\"core/pkg::init\"]\nbase_dir = \".\"\ncreate_dirs = true\n\n[op.\"core/pkg::list\"]\nbase_dir = \".\"\n\n[op.\"core/pkg-low::load-lock\"]\nbase_dir = \".\"\n",
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
allow = ["core/vcs::log", "core/store::get", "core/refs::get", "core/refs::list"]

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
fn selfhost_only_accepts_store_refs_pkg_and_gc() {
    let dir = tempdir().unwrap();
    let caps = write_effect_caps(
        dir.path(),
        &[
            "core/store::put",
            "core/refs::get",
            "core/pkg::init",
            "core/pkg::list",
            "core/pkg-low::load-lock",
            "core/gc::pin",
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
            "--engine rust` is disabled in the default selfhost profile",
        ));

    cargo_bin_cmd!("genesis")
        .env("GENESIS_ALLOW_RUST_ENGINE", "1")
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
fn fmt_defaults_to_selfhost_via_workspace_artifact_fallback() {
    let dir = tempdir().unwrap();
    let file = dir.path().join("m.gc");
    std::fs::write(&file, "(def x 1)\n").unwrap();

    let out = cargo_bin_cmd!("genesis")
        .args(["--json", "fmt", file.to_str().unwrap()])
        .current_dir(dir.path())
        .assert()
        .success()
        .get_output()
        .stdout
        .clone();
    let v: JsonValue = serde_json::from_slice(&out).unwrap();
    let engine = v
        .get("data")
        .and_then(|d| d.get("engine"))
        .and_then(JsonValue::as_str)
        .unwrap();
    assert_eq!(engine, "selfhost");
}
