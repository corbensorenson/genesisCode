use assert_cmd::cargo::cargo_bin_cmd;
use predicates::prelude::*;
use tempfile::tempdir;

mod support;

fn build_selfhost_artifact(dir: &std::path::Path) -> std::path::PathBuf {
    support::copy_repo_toolchain_artifact(dir)
}

#[test]
fn eval_selfhost_engine_matches_rust_engine_output() {
    let dir = tempdir().unwrap();
    let file = dir.path().join("m.gc");
    let artifact = build_selfhost_artifact(dir.path());

    let src = r#"
      (def x 40)
      (def y (prim int/add x 2))
      y
    "#;
    std::fs::write(&file, src).unwrap();

    let rust_out = cargo_bin_cmd!("genesis")
        .env("GENESIS_ALLOW_RUST_ENGINE", "1")
        .args(["eval", file.to_str().unwrap(), "--engine", "rust"])
        .assert()
        .success()
        .get_output()
        .stdout
        .clone();

    let selfhost_out = cargo_bin_cmd!("genesis")
        .args([
            "--no-step-limit",
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

    let rust_s = String::from_utf8(rust_out).unwrap();
    let selfhost_s = String::from_utf8(selfhost_out).unwrap();
    assert_eq!(rust_s.trim(), selfhost_s.trim());
}

#[test]
fn eval_selfhost_engine_surfaces_parse_errors() {
    let dir = tempdir().unwrap();
    let file = dir.path().join("bad.gc");
    let artifact = build_selfhost_artifact(dir.path());
    std::fs::write(&file, "(def x 1").unwrap();

    cargo_bin_cmd!("genesis")
        .args([
            "--no-step-limit",
            "--selfhost-artifact",
            artifact.to_str().unwrap(),
            "eval",
            file.to_str().unwrap(),
            "--engine",
            "selfhost",
        ])
        .assert()
        .failure()
        .code(10)
        .stderr(predicate::str::contains("core/parse/"));
}

#[test]
fn eval_selfhost_engine_requires_explicit_artifact_even_without_selfhost_only() {
    let dir = tempdir().unwrap();
    let file = dir.path().join("m.gc");
    std::fs::write(&file, "1\n").unwrap();

    cargo_bin_cmd!("genesis")
        .args(["eval", file.to_str().unwrap(), "--engine", "selfhost"])
        .assert()
        .failure()
        .code(50)
        .stderr(predicate::str::contains(
            "explicit selfhost artifact required",
        ));
}
