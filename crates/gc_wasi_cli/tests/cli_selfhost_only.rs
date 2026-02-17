use assert_cmd::cargo::cargo_bin_cmd;
use predicates::prelude::*;
use serde_json::Value as JsonValue;
use tempfile::tempdir;

fn build_selfhost_artifact(dir: &std::path::Path) -> std::path::PathBuf {
    let artifact = dir.join("selfhost_toolchain.gc");
    cargo_bin_cmd!("genesis_wasi")
        .args(["selfhost-artifact", "--out"])
        .arg(&artifact)
        .assert()
        .success();
    artifact
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
            "selfhost-only mode currently supports only `fmt`, `eval`, `run`, `replay`, `test`, `pack`, and `vcs hash`",
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
