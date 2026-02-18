use assert_cmd::cargo::cargo_bin_cmd;
use predicates::prelude::*;
use serde_json::Value as JsonValue;
use std::path::{Path, PathBuf};
use tempfile::tempdir;

mod common;

fn build_selfhost_artifact(dir: &std::path::Path) -> std::path::PathBuf {
    common::copy_repo_selfhost_toolchain_artifact(dir)
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
        "]\n\n[store]\ndir = \"./.genesis/store\"\n\n[refs]\npath = \"./.genesis/refs.gc\"\n\n[op.\"core/pkg::init\"]\nbase_dir = \".\"\ncreate_dirs = true\n\n[op.\"core/pkg::list\"]\nbase_dir = \".\"\n",
    );
    std::fs::write(&caps, s).unwrap();
    caps
}

fn write_vcs_caps(dir: &Path) -> PathBuf {
    let caps = dir.join("caps_vcs.toml");
    std::fs::write(
        &caps,
        r#"
allow = ["core/vcs::log"]

[store]
dir = "./.genesis/store"
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
fn selfhost_only_rejects_non_routed_commands() {
    let td = tempdir().unwrap();
    let out = td.path().join("selfhost_toolchain.gc");

    cargo_bin_cmd!("genesis_wasi")
        .args([
            "--selfhost-only",
            "selfhost-artifact",
            "--out",
            out.to_str().unwrap(),
        ])
        .assert()
        .failure()
        .code(50)
        .stderr(predicate::str::contains(
            "selfhost-only mode currently supports only `fmt`, `eval`, `explain`, `optimize`, `run`, `replay`, `test`, `pack`, `typecheck`, `apply-patch`, `selfhost-dashboard`, `store`, `refs`, `pkg`, `policy`, `sync`, `gc`, and `vcs/*`",
        ));
}

#[test]
fn selfhost_only_accepts_store_refs_pkg_and_gc() {
    let td = tempdir().unwrap();
    let caps = write_effect_caps(
        td.path(),
        &[
            "core/store::put",
            "core/refs::get",
            "core/pkg::init",
            "core/pkg::list",
            "core/gc::pin",
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
        .code(50)
        .stderr(predicate::str::contains(
            "--engine rust` is disabled in the default selfhost profile",
        ));

    cargo_bin_cmd!("genesis_wasi")
        .env("GENESIS_ALLOW_RUST_ENGINE", "1")
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
        .code(50)
        .stderr(predicate::str::contains(
            "selfhost-only mode requires --coreform-frontend selfhost",
        ));
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

#[test]
fn selfhost_only_accepts_run_and_replay_with_selfhost_artifact() {
    let td = tempdir().unwrap();
    let artifact = build_selfhost_artifact(td.path());
    let file = td.path().join("prog.gc");
    std::fs::write(
        &file,
        r#"
          (def prog (core/effect::pure 42))
          prog
        "#,
    )
    .unwrap();
    let caps = td.path().join("caps.toml");
    std::fs::write(&caps, "allow = []\n").unwrap();
    let log = td.path().join("out.gclog");

    cargo_bin_cmd!("genesis_wasi")
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

    cargo_bin_cmd!("genesis_wasi")
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
fn fmt_defaults_to_selfhost_via_workspace_artifact_fallback() {
    let td = tempdir().unwrap();
    let file = td.path().join("m.gc");
    std::fs::write(&file, "(def x 1)\n").unwrap();

    let out = cargo_bin_cmd!("genesis_wasi")
        .args(["--json", "fmt", file.to_str().unwrap()])
        .current_dir(td.path())
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
