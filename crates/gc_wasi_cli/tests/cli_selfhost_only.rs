use assert_cmd::cargo::cargo_bin_cmd;
use predicates::prelude::*;
use serde_json::Value as JsonValue;
use std::path::{Path, PathBuf};
use tempfile::tempdir;

mod common;

fn build_selfhost_artifact(dir: &std::path::Path) -> std::path::PathBuf {
    common::copy_repo_selfhost_toolchain_artifact(dir)
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
wasi_network_profile = "local"
"#,
    )
    .unwrap();
    caps
}

#[test]
fn top_level_help_exposes_selfhost_only_flag() {
    cargo_bin_cmd!("genesis_wasi")
        .args(["--help"])
        .assert()
        .success()
        .stdout(predicate::str::contains("--selfhost-only"));
}

#[test]
fn selfhost_only_rejects_rust_engine_for_eval() {
    let td = tempdir().unwrap();
    let file = td.path().join("m.gc");
    std::fs::write(&file, "(def x 1)\nx\n").unwrap();

    cargo_bin_cmd!("genesis_wasi")
        .args([
            "--selfhost-only",
            "eval",
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
fn selfhost_only_requires_explicit_artifact_for_runtime_commands() {
    let td = tempdir().unwrap();
    let file = td.path().join("m.gc");
    std::fs::write(&file, "(def x 1)\nx\n").unwrap();

    cargo_bin_cmd!("genesis_wasi")
        .args([
            "--selfhost-only",
            "eval",
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
fn selfhost_only_accepts_selfhost_artifact_and_keygen() {
    let td = tempdir().unwrap();
    let bootstrap = build_selfhost_artifact(td.path());
    let out_artifact = td.path().join("selfhost_toolchain.gc");

    let artifact_out = cargo_bin_cmd!("genesis_wasi")
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

    let out_key = td.path().join("k.toml");
    cargo_bin_cmd!("genesis_wasi")
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
    let td = tempdir().unwrap();
    let key = td.path().join("key.toml");
    cargo_bin_cmd!("genesis_wasi")
        .args(["--selfhost-only", "keygen", "--out"])
        .arg(&key)
        .assert()
        .success();
    let missing_pkg = td.path().join("missing-package.toml");
    let acceptance = "0".repeat(64);

    let sign_out = cargo_bin_cmd!("genesis_wasi")
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

    let tv_out = cargo_bin_cmd!("genesis_wasi")
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

    let verify_out = cargo_bin_cmd!("genesis_wasi")
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
    let td = tempdir().unwrap();
    let policies = td.path().join("policies.toml");

    cargo_bin_cmd!("genesis_wasi")
        .current_dir(td.path())
        .args(["--selfhost-only", "policy", "list", "--policies"])
        .arg(&policies)
        .assert()
        .success();

    let show_out = cargo_bin_cmd!("genesis_wasi")
        .current_dir(td.path())
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

    let set_default_out = cargo_bin_cmd!("genesis_wasi")
        .current_dir(td.path())
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
    let td = tempdir().unwrap();
    let caps = write_effect_caps(
        td.path(),
        &[
            "core/store::put",
            "core/refs::get",
            "core/pkg-low::save-lock",
            "core/pkg-low::load-lock",
            "core/gc-low::pin",
        ],
    );
    let term = td.path().join("value.gc");
    std::fs::write(&term, "{:hello true}\n").unwrap();

    cargo_bin_cmd!("genesis_wasi")
        .current_dir(td.path())
        .args(["--selfhost-only", "store", "--caps"])
        .arg(caps.to_str().unwrap())
        .args(["put", "--input"])
        .arg(term.to_str().unwrap())
        .assert()
        .success();

    cargo_bin_cmd!("genesis_wasi")
        .current_dir(td.path())
        .args(["--selfhost-only", "refs", "--caps"])
        .arg(caps.to_str().unwrap())
        .args(["get", "refs/heads/main"])
        .assert()
        .success();

    cargo_bin_cmd!("genesis_wasi")
        .current_dir(td.path())
        .args(["--selfhost-only", "pkg", "--caps"])
        .arg(caps.to_str().unwrap())
        .args(["init", "--workspace", "ws"])
        .assert()
        .success();

    cargo_bin_cmd!("genesis_wasi")
        .current_dir(td.path())
        .args(["--selfhost-only", "pkg", "--caps"])
        .arg(caps.to_str().unwrap())
        .args(["list"])
        .assert()
        .success();

    cargo_bin_cmd!("genesis_wasi")
        .current_dir(td.path())
        .args(["--selfhost-only", "gc", "--caps"])
        .arg(caps.to_str().unwrap())
        .args(["pin", "refs/heads/main", "--pins", ".genesis/pins.toml"])
        .assert()
        .success();
}

#[test]
fn selfhost_only_pkg_update_uses_pkg_low_caps_only() {
    let td = tempdir().unwrap();
    let caps = write_effect_caps(
        td.path(),
        &["core/pkg-low::save-lock", "core/pkg-low::load-lock"],
    );

    cargo_bin_cmd!("genesis_wasi")
        .current_dir(td.path())
        .args(["--selfhost-only", "pkg", "--caps"])
        .arg(caps.to_str().unwrap())
        .args(["init", "--workspace", "ws"])
        .assert()
        .success();

    cargo_bin_cmd!("genesis_wasi")
        .current_dir(td.path())
        .args(["--selfhost-only", "pkg", "--caps"])
        .arg(caps.to_str().unwrap())
        .args(["update", "--lock", "genesis.lock"])
        .assert()
        .success();
}

#[test]
fn selfhost_only_pkg_lock_non_strict_uses_pkg_low_caps_only() {
    let td = tempdir().unwrap();
    let caps = write_effect_caps(
        td.path(),
        &["core/pkg-low::save-lock", "core/pkg-low::load-lock"],
    );

    cargo_bin_cmd!("genesis_wasi")
        .current_dir(td.path())
        .args(["--selfhost-only", "pkg", "--caps"])
        .arg(caps.to_str().unwrap())
        .args(["init", "--workspace", "ws"])
        .assert()
        .success();

    cargo_bin_cmd!("genesis_wasi")
        .current_dir(td.path())
        .args(["--selfhost-only", "pkg", "--caps"])
        .arg(caps.to_str().unwrap())
        .args([
            "add",
            "dep@snapshot:0123456789abcdef0123456789abcdef0123456789abcdef0123456789abcdef",
        ])
        .assert()
        .success();

    cargo_bin_cmd!("genesis_wasi")
        .current_dir(td.path())
        .args(["--selfhost-only", "pkg", "--caps"])
        .arg(caps.to_str().unwrap())
        .args(["lock", "--lock", "genesis.lock"])
        .assert()
        .success();
}

#[test]
fn selfhost_only_pkg_lock_strict_uses_pkg_low_caps_only() {
    let td = tempdir().unwrap();
    let caps = write_effect_caps(
        td.path(),
        &[
            "core/pkg-low::save-lock",
            "core/pkg-low::load-lock",
            "core/store::put",
            "core/store::get",
        ],
    );

    cargo_bin_cmd!("genesis_wasi")
        .current_dir(td.path())
        .args(["--selfhost-only", "pkg", "--caps"])
        .arg(caps.to_str().unwrap())
        .args(["init", "--workspace", "ws"])
        .assert()
        .success();

    let snapshot_file = td.path().join("snapshot.gc");
    std::fs::write(
        &snapshot_file,
        "{:type :vcs/snapshot :v 1 :kind :package :modules [] :obligations []}\n",
    )
    .unwrap();

    let put_out = cargo_bin_cmd!("genesis_wasi")
        .current_dir(td.path())
        .args(["--selfhost-only", "store", "--caps"])
        .arg(caps.to_str().unwrap())
        .args(["put", "--input", "snapshot.gc"])
        .assert()
        .success()
        .get_output()
        .stdout
        .clone();
    let snapshot_h = String::from_utf8(put_out).unwrap().trim().to_string();

    cargo_bin_cmd!("genesis_wasi")
        .current_dir(td.path())
        .args(["--selfhost-only", "pkg", "--caps"])
        .arg(caps.to_str().unwrap())
        .args(["add", &format!("dep@snapshot:{snapshot_h}")])
        .assert()
        .success();

    cargo_bin_cmd!("genesis_wasi")
        .current_dir(td.path())
        .args(["--selfhost-only", "pkg", "--caps"])
        .arg(caps.to_str().unwrap())
        .args(["lock", "--strict", "--lock", "genesis.lock"])
        .assert()
        .success();
}

#[test]
fn selfhost_only_accepts_policy_command_group() {
    let td = tempdir().unwrap();
    let policies = td.path().join("policies.toml");

    cargo_bin_cmd!("genesis_wasi")
        .current_dir(td.path())
        .args(["--selfhost-only", "policy", "list", "--policies"])
        .arg(&policies)
        .assert()
        .success()
        .stdout(predicate::str::contains("default"));
}

#[test]
fn selfhost_only_accepts_vcs_command_group() {
    let td = tempdir().unwrap();
    let caps = write_vcs_caps(td.path());
    let root_h = "0".repeat(64);

    cargo_bin_cmd!("genesis_wasi")
        .current_dir(td.path())
        .args(["--selfhost-only", "vcs", "--caps"])
        .arg(caps.to_str().unwrap())
        .args(["log"])
        .arg(&root_h)
        .assert()
        .failure()
        .code(20);
}

#[test]
fn selfhost_only_accepts_sync_command_group() {
    let td = tempdir().unwrap();
    let caps = write_sync_caps(td.path());
    let remote = format!("file://{}", td.path().join("remote-registry").display());
    let root_h = "0".repeat(64);

    cargo_bin_cmd!("genesis_wasi")
        .current_dir(td.path())
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
fn selfhost_only_rejects_rust_engine_for_explain() {
    let td = tempdir().unwrap();
    let file = td.path().join("m.gc");
    std::fs::write(
        &file,
        "(def c (core/contract::make (fn (msg) nil) nil {}))\nc\n",
    )
    .unwrap();

    cargo_bin_cmd!("genesis_wasi")
        .args([
            "--selfhost-only",
            "explain",
            file.to_str().unwrap(),
            "--engine",
            "rust",
            "--contract",
            "c",
            "--msg",
            "(msg foo nil)",
        ])
        .assert()
        .failure()
        .code(50)
        .stderr(predicate::str::contains(
            "selfhost-only mode requires --engine selfhost",
        ));
}

#[test]
fn fmt_prefers_selfhost_when_artifact_flag_is_set_without_engine() {
    let td = tempdir().unwrap();
    let file = td.path().join("m.gc");
    let bad_artifact = td.path().join("bad_toolchain.gc");
    std::fs::write(&file, "(def x 1)\n").unwrap();
    std::fs::write(&bad_artifact, "{ :kind \"bad\" }\n").unwrap();

    cargo_bin_cmd!("genesis_wasi")
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
    let td = tempdir().unwrap();
    let file = td.path().join("m.gc");
    let bad_artifact = td.path().join("bad_toolchain.gc");
    std::fs::write(&file, "1\n").unwrap();
    std::fs::write(&bad_artifact, "{ :kind \"bad\" }\n").unwrap();

    cargo_bin_cmd!("genesis_wasi")
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
    let td = tempdir().unwrap();
    let file = td.path().join("m.gc");
    let bad_artifact = td.path().join("bad_toolchain.gc");
    std::fs::write(&file, "(def x 1)\nx\n").unwrap();
    std::fs::write(&bad_artifact, "{ :kind \"bad\" }\n").unwrap();

    cargo_bin_cmd!("genesis_wasi")
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
    let td = tempdir().unwrap();
    let file = td.path().join("m.gc");
    let bad_artifact = td.path().join("bad_toolchain.gc");
    std::fs::write(&file, "1\n").unwrap();
    std::fs::write(&bad_artifact, "{ :kind \"bad\" }\n").unwrap();

    cargo_bin_cmd!("genesis_wasi")
        .args([
            "--selfhost-artifact",
            bad_artifact.to_str().unwrap(),
            "eval",
            file.to_str().unwrap(),
            "--engine",
            "rust",
        ])
        .assert()
        .failure()
        .code(2)
        .stderr(predicate::str::contains(
            "invalid value 'rust' for '--engine <ENGINE>'",
        ))
        .stderr(predicate::str::contains("expected `selfhost`"));

    cargo_bin_cmd!("genesis_wasi_parity")
        .args([
            "--selfhost-artifact",
            bad_artifact.to_str().unwrap(),
            "eval",
            file.to_str().unwrap(),
            "--engine",
            "rust",
        ])
        .assert()
        .success();
}

#[test]
fn selfhost_only_accepts_test_with_selfhost_artifact() {
    let td = tempdir().unwrap();
    let artifact = build_selfhost_artifact(td.path());
    let pkg = concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/../../tests/spec/pkg_basic/package.toml"
    );

    cargo_bin_cmd!("genesis_wasi")
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
fn selfhost_only_rejects_rust_coreform_frontend_for_pack() {
    let pkg = concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/../../tests/spec/pkg_basic/package.toml"
    );

    cargo_bin_cmd!("genesis_wasi")
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
        .code(2)
        .stderr(predicate::str::contains(
            "invalid value 'rust' for '--coreform-frontend <COREFORM_FRONTEND>'",
        ))
        .stderr(predicate::str::contains("expected `selfhost`"));
}

#[test]
fn selfhost_only_accepts_pack_with_selfhost_artifact() {
    let td = tempdir().unwrap();
    let artifact = build_selfhost_artifact(td.path());
    let pkg = concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/../../tests/spec/pkg_basic/package.toml"
    );

    cargo_bin_cmd!("genesis_wasi")
        .args([
            "--selfhost-only",
            "--selfhost-artifact",
            artifact.to_str().unwrap(),
            "pack",
            "--pkg",
            pkg,
        ])
        .assert()
        .success();
}

#[test]
fn selfhost_only_accepts_typecheck_with_selfhost_artifact() {
    let td = tempdir().unwrap();
    let artifact = build_selfhost_artifact(td.path());
    let pkg = concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/../../tests/spec/pkg_basic/package.toml"
    );

    cargo_bin_cmd!("genesis_wasi")
        .args([
            "--selfhost-only",
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
fn selfhost_only_accepts_apply_patch_with_selfhost_artifact() {
    let td = tempdir().unwrap();
    let artifact = build_selfhost_artifact(td.path());
    let pkg_dir = td.path().join("pkg");
    std::fs::create_dir_all(&pkg_dir).unwrap();
    std::fs::copy(
        concat!(
            env!("CARGO_MANIFEST_DIR"),
            "/../../tests/spec/pkg_basic/basic.gc"
        ),
        pkg_dir.join("basic.gc"),
    )
    .unwrap();
    std::fs::copy(
        concat!(
            env!("CARGO_MANIFEST_DIR"),
            "/../../tests/spec/pkg_basic/package.toml"
        ),
        pkg_dir.join("package.toml"),
    )
    .unwrap();
    std::fs::copy(
        concat!(
            env!("CARGO_MANIFEST_DIR"),
            "/../../tests/spec/pkg_basic/caps.toml"
        ),
        pkg_dir.join("caps.toml"),
    )
    .unwrap();
    std::fs::copy(
        concat!(
            env!("CARGO_MANIFEST_DIR"),
            "/../../tests/spec/pkg_basic/pure.gcpatch"
        ),
        pkg_dir.join("pure.gcpatch"),
    )
    .unwrap();

    cargo_bin_cmd!("genesis_wasi")
        .current_dir(&pkg_dir)
        .args([
            "--selfhost-only",
            "--selfhost-artifact",
            artifact.to_str().unwrap(),
            "apply-patch",
            "pure.gcpatch",
            "--pkg",
            "package.toml",
        ])
        .assert()
        .success();
}

#[test]
fn selfhost_only_accepts_selfhost_dashboard_with_selfhost_artifact() {
    let td = tempdir().unwrap();
    let artifact = build_selfhost_artifact(td.path());
    let store = td.path().join("store");
    let markdown = td.path().join("status").join("SELFHOST_CUTOVER.md");

    cargo_bin_cmd!("genesis_wasi")
        .args([
            "--selfhost-only",
            "--selfhost-artifact",
            artifact.to_str().unwrap(),
            "selfhost-dashboard",
            "--store",
            store.to_str().unwrap(),
            "--markdown",
            markdown.to_str().unwrap(),
        ])
        .assert()
        .success();
}

#[test]
fn selfhost_only_accepts_vcs_hash_with_selfhost_artifact() {
    let td = tempdir().unwrap();
    let artifact = build_selfhost_artifact(td.path());
    let file = td.path().join("m.gc");
    std::fs::write(&file, "(def x 1)\nx\n").unwrap();

    let out = cargo_bin_cmd!("genesis_wasi")
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
fn selfhost_only_rejects_rust_engine_for_run() {
    let td = tempdir().unwrap();
    let file = td.path().join("prog.gc");
    let caps = td.path().join("caps.toml");
    std::fs::write(
        &file,
        r#"
          (def prog (core/effect::pure 1))
          prog
        "#,
    )
    .unwrap();
    std::fs::write(&caps, "allow = []\n").unwrap();

    cargo_bin_cmd!("genesis_wasi")
        .args([
            "--selfhost-only",
            "run",
            file.to_str().unwrap(),
            "--engine",
            "rust",
            "--caps",
            caps.to_str().unwrap(),
        ])
        .assert()
        .failure()
        .code(50)
        .stderr(predicate::str::contains(
            "selfhost-only mode requires --engine selfhost",
        ));
}

#[test]
fn selfhost_only_rejects_rust_engine_for_optimize() {
    let td = tempdir().unwrap();
    let file = td.path().join("prog.gc");
    std::fs::write(
        &file,
        r#"
          (def x (prim int/add 40 2))
          x
        "#,
    )
    .unwrap();

    cargo_bin_cmd!("genesis_wasi")
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

#[path = "cli_selfhost_only_tail.rs"]
mod cli_selfhost_only_tail;
