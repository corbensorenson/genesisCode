use assert_cmd::cargo::cargo_bin_cmd;
use tempfile::tempdir;

mod support;

fn build_selfhost_artifact(dir: &std::path::Path) -> std::path::PathBuf {
    support::copy_repo_toolchain_artifact(dir)
}

fn genesis_cmd() -> assert_cmd::Command {
    let mut cmd = cargo_bin_cmd!("genesis");
    cmd.env(
        "GENESIS_SELFHOST_TOOLCHAIN_ARTIFACT",
        support::repo_toolchain_artifact(),
    );
    cmd
}

#[test]
fn eval_stage1_pipeline_and_gate_match_baseline_for_pure_module() {
    let dir = tempdir().unwrap();
    let file = dir.path().join("m.gc");
    std::fs::write(
        &file,
        r#"
          (def x (prim int/add 1 (prim int/add 2 0)))
          x
        "#,
    )
    .unwrap();

    let base = genesis_cmd()
        .args(["eval", file.to_str().unwrap()])
        .assert()
        .success()
        .get_output()
        .stdout
        .clone();
    let stage1 = genesis_cmd()
        .args([
            "eval",
            file.to_str().unwrap(),
            "--stage1-pipeline",
            "--stage1-gate",
        ])
        .assert()
        .success()
        .get_output()
        .stdout
        .clone();

    let base_s = String::from_utf8(base).unwrap();
    let stage1_s = String::from_utf8(stage1).unwrap();
    assert_eq!(base_s.trim(), stage1_s.trim());
}

#[test]
fn eval_stage1_gate_fails_for_effect_program() {
    let dir = tempdir().unwrap();
    let file = dir.path().join("effect.gc");
    std::fs::write(
        &file,
        r#"
          (core/effect::perform
            'sys/time::now
            nil
            (fn (t) (core/effect::pure t)))
        "#,
    )
    .unwrap();

    genesis_cmd()
        .args(["eval", file.to_str().unwrap(), "--stage1-gate"])
        .assert()
        .failure()
        .code(30);
}

#[test]
fn optimize_stage1_gate_fails_for_effect_program() {
    let dir = tempdir().unwrap();
    let file = dir.path().join("effect.gc");
    std::fs::write(
        &file,
        r#"
          (core/effect::perform
            'sys/time::now
            nil
            (fn (t) (core/effect::pure t)))
        "#,
    )
    .unwrap();

    genesis_cmd()
        .args(["optimize", file.to_str().unwrap(), "--stage1-gate"])
        .assert()
        .failure()
        .code(30);
}

#[test]
fn optimize_selfhost_engine_matches_rust_stage1_output() {
    let dir = tempdir().unwrap();
    let file = dir.path().join("m.gc");
    let artifact = build_selfhost_artifact(dir.path());
    std::fs::write(
        &file,
        r#"
          (def x (prim int/add 0 (prim int/add 1 2)))
          x
        "#,
    )
    .unwrap();

    let rust = cargo_bin_cmd!("genesis_parity")
        .args(["optimize", file.to_str().unwrap(), "--engine", "rust"])
        .assert()
        .success()
        .get_output()
        .stdout
        .clone();
    let selfhost = genesis_cmd()
        .args([
            "--selfhost-artifact",
            artifact.to_str().unwrap(),
            "optimize",
            file.to_str().unwrap(),
            "--engine",
            "selfhost",
        ])
        .assert()
        .success()
        .get_output()
        .stdout
        .clone();

    let rust_s = String::from_utf8(rust).unwrap();
    let selfhost_s = String::from_utf8(selfhost).unwrap();
    assert_eq!(rust_s, selfhost_s);
}

#[test]
fn eval_selfhost_engine_stage1_gate_matches_rust() {
    let dir = tempdir().unwrap();
    let file = dir.path().join("m.gc");
    let artifact = build_selfhost_artifact(dir.path());
    std::fs::write(
        &file,
        r#"
          (def x (prim int/add 4 (prim int/add 5 0)))
          x
        "#,
    )
    .unwrap();

    let rust = cargo_bin_cmd!("genesis_parity")
        .args([
            "eval",
            file.to_str().unwrap(),
            "--engine",
            "rust",
            "--stage1-gate",
        ])
        .assert()
        .success()
        .get_output()
        .stdout
        .clone();
    let selfhost = genesis_cmd()
        .args([
            "--selfhost-artifact",
            artifact.to_str().unwrap(),
            "eval",
            file.to_str().unwrap(),
            "--engine",
            "selfhost",
            "--stage1-gate",
        ])
        .assert()
        .success()
        .get_output()
        .stdout
        .clone();

    let rust_s = String::from_utf8(rust).unwrap();
    let selfhost_s = String::from_utf8(selfhost).unwrap();
    assert_eq!(rust_s.trim(), selfhost_s.trim());
}
