use assert_cmd::cargo::cargo_bin_cmd;
use predicates::prelude::*;
use tempfile::tempdir;

mod common;

fn build_selfhost_artifact(dir: &std::path::Path) -> std::path::PathBuf {
    common::copy_repo_selfhost_toolchain_artifact(dir)
}

#[test]
fn optimize_selfhost_engine_matches_rust_engine_output() {
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

    let rust_out = cargo_bin_cmd!("genesis_wasi_parity")
        .args(["optimize", file.to_str().unwrap(), "--engine", "rust"])
        .assert()
        .success()
        .get_output()
        .stdout
        .clone();
    let selfhost_out = cargo_bin_cmd!("genesis_wasi")
        .args([
            "--no-step-limit",
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

    let rust_s = String::from_utf8(rust_out).unwrap();
    let selfhost_s = String::from_utf8(selfhost_out).unwrap();
    assert_eq!(rust_s, selfhost_s);
}

#[test]
fn optimize_json_reports_coreform_frontend_for_ai_drivers() {
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

    let rust_out = cargo_bin_cmd!("genesis_wasi_parity")
        .args([
            "--json",
            "optimize",
            file.to_str().unwrap(),
            "--engine",
            "rust",
        ])
        .assert()
        .success()
        .get_output()
        .stdout
        .clone();
    let rust_v: serde_json::Value = serde_json::from_slice(&rust_out).expect("valid rust json");
    assert_eq!(
        rust_v["data"]["coreform_frontend"]["name"].as_str(),
        Some("rust")
    );

    let selfhost_out = cargo_bin_cmd!("genesis_wasi")
        .args([
            "--no-step-limit",
            "--selfhost-artifact",
            artifact.to_str().unwrap(),
            "--json",
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
    let self_v: serde_json::Value =
        serde_json::from_slice(&selfhost_out).expect("valid selfhost json");
    assert_eq!(
        self_v["data"]["coreform_frontend"]["name"].as_str(),
        Some("selfhost")
    );
}

#[test]
fn optimize_stage1_gate_fails_for_effect_program() {
    let td = tempdir().unwrap();
    let file = td.path().join("effect.gc");
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

    cargo_bin_cmd!("genesis_wasi_parity")
        .args([
            "optimize",
            file.to_str().unwrap(),
            "--engine",
            "rust",
            "--stage1-gate",
        ])
        .assert()
        .failure()
        .code(30);
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

    cargo_bin_cmd!("genesis_wasi_parity")
        .args([
            "optimize",
            file.to_str().unwrap(),
            "--engine",
            "rust",
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
fn optimize_selfhost_engine_surfaces_parse_errors() {
    let dir = tempdir().unwrap();
    let file = dir.path().join("bad.gc");
    let artifact = build_selfhost_artifact(dir.path());
    std::fs::write(&file, "(def x 1").unwrap();

    cargo_bin_cmd!("genesis_wasi")
        .args([
            "--no-step-limit",
            "--selfhost-artifact",
            artifact.to_str().unwrap(),
            "optimize",
            file.to_str().unwrap(),
            "--engine",
            "selfhost",
        ])
        .assert()
        .failure()
        .code(10)
        .stderr(predicate::str::contains("core/parse/"));
}
