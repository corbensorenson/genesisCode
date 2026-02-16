use assert_cmd::cargo::cargo_bin_cmd;
use tempfile::tempdir;

fn build_selfhost_artifact(dir: &std::path::Path) -> std::path::PathBuf {
    let artifact = dir.join("selfhost_toolchain.gc");
    cargo_bin_cmd!("genesis")
        .args(["selfhost-artifact", "--out"])
        .arg(&artifact)
        .assert()
        .success();
    artifact
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

    let base = cargo_bin_cmd!("genesis")
        .args(["eval", file.to_str().unwrap()])
        .assert()
        .success()
        .get_output()
        .stdout
        .clone();
    let stage1 = cargo_bin_cmd!("genesis")
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

    cargo_bin_cmd!("genesis")
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

    cargo_bin_cmd!("genesis")
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

    let rust = cargo_bin_cmd!("genesis")
        .args(["optimize", file.to_str().unwrap(), "--engine", "rust"])
        .assert()
        .success()
        .get_output()
        .stdout
        .clone();
    let selfhost = cargo_bin_cmd!("genesis")
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

    let rust = cargo_bin_cmd!("genesis")
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
    let selfhost = cargo_bin_cmd!("genesis")
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

#[test]
fn optimize_stage2_gate_and_emit_wasm_succeeds_for_scalar_pure_module() {
    let dir = tempdir().unwrap();
    let file = dir.path().join("m.gc");
    let wasm = dir.path().join("m.wasm");
    std::fs::write(
        &file,
        r#"
          (def x (prim int/add 20 22))
          x
        "#,
    )
    .unwrap();

    cargo_bin_cmd!("genesis")
        .args([
            "optimize",
            file.to_str().unwrap(),
            "--stage2-gate",
            "--emit-wasm",
            wasm.to_str().unwrap(),
        ])
        .assert()
        .success();

    let bytes = std::fs::read(&wasm).unwrap();
    assert!(!bytes.is_empty());
}

#[test]
fn eval_stage2_gate_matches_baseline_for_scalar_pure_module() {
    let dir = tempdir().unwrap();
    let file = dir.path().join("m.gc");
    std::fs::write(
        &file,
        r#"
          (def x (prim int/add 20 22))
          x
        "#,
    )
    .unwrap();

    let base = cargo_bin_cmd!("genesis")
        .args(["eval", file.to_str().unwrap()])
        .assert()
        .success()
        .get_output()
        .stdout
        .clone();
    let gated = cargo_bin_cmd!("genesis")
        .args(["eval", file.to_str().unwrap(), "--stage2-gate"])
        .assert()
        .success()
        .get_output()
        .stdout
        .clone();

    let base_s = String::from_utf8(base).unwrap();
    let gated_s = String::from_utf8(gated).unwrap();
    assert_eq!(base_s.trim(), gated_s.trim());
}

#[test]
fn eval_stage2_gate_allows_unsupported_module() {
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

    cargo_bin_cmd!("genesis")
        .args(["eval", file.to_str().unwrap(), "--stage2-gate"])
        .assert()
        .success();
}

#[test]
fn optimize_stage2_gate_allows_unsupported_module() {
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

    cargo_bin_cmd!("genesis")
        .args(["optimize", file.to_str().unwrap(), "--stage2-gate"])
        .assert()
        .success();
}

#[test]
fn optimize_emit_wasm_fails_for_unsupported_module() {
    let dir = tempdir().unwrap();
    let file = dir.path().join("effect.gc");
    let wasm = dir.path().join("effect.wasm");
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

    cargo_bin_cmd!("genesis")
        .args([
            "optimize",
            file.to_str().unwrap(),
            "--emit-wasm",
            wasm.to_str().unwrap(),
        ])
        .assert()
        .failure()
        .code(30);
}
