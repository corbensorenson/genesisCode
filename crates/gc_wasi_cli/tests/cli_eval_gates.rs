use assert_cmd::cargo::cargo_bin_cmd;
use predicates::prelude::*;
use serde_json::Value as JsonValue;
use tempfile::tempdir;

#[test]
fn eval_help_exposes_stage_gating_flags() {
    cargo_bin_cmd!("genesis_wasi")
        .args(["eval", "--help"])
        .assert()
        .success()
        .stdout(predicate::str::contains("--stage1-pipeline"))
        .stdout(predicate::str::contains("--stage1-gate"))
        .stdout(predicate::str::contains("--stage2-gate"));
}

#[test]
fn eval_stage2_gate_succeeds_for_scalar_pure_program() {
    let td = tempdir().unwrap();
    let file = td.path().join("pure.gc");
    std::fs::write(
        &file,
        r#"
          (def x (prim int/add 20 22))
          x
        "#,
    )
    .unwrap();

    cargo_bin_cmd!("genesis_wasi")
        .args(["eval", file.to_str().unwrap(), "--stage2-gate"])
        .assert()
        .success()
        .stdout(predicate::str::contains("42"));
}

#[test]
fn eval_stage2_gate_allows_unsupported_non_scalar_module() {
    let td = tempdir().unwrap();
    let file = td.path().join("map.gc");
    std::fs::write(
        &file,
        r#"
          (quote {a 1 b 2})
        "#,
    )
    .unwrap();

    cargo_bin_cmd!("genesis_wasi")
        .args(["eval", file.to_str().unwrap(), "--stage2-gate"])
        .assert()
        .success()
        .stdout(predicate::str::contains("{a 1 b 2}"));
}

#[test]
fn eval_stage2_gate_uses_stage1_transformed_input_for_stage2_report() {
    let td = tempdir().unwrap();
    let file = td.path().join("if_fold.gc");
    std::fs::write(
        &file,
        r#"
          (if true
            (prim int/add 1 2)
            (quote {a 1}))
        "#,
    )
    .unwrap();

    let out = cargo_bin_cmd!("genesis_wasi")
        .args(["--json", "eval", file.to_str().unwrap(), "--stage2-gate"])
        .assert()
        .success()
        .get_output()
        .stdout
        .clone();
    let v: JsonValue = serde_json::from_slice(&out).unwrap();
    let stage2 = &v["data"]["stage2"];
    assert_eq!(stage2["supported"].as_bool(), Some(true), "{v}");
    assert_eq!(stage2["ok"].as_bool(), Some(true), "{v}");
}

#[test]
fn eval_stage1_gate_fails_for_effect_program() {
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

    cargo_bin_cmd!("genesis_wasi")
        .args(["eval", file.to_str().unwrap(), "--stage1-gate"])
        .assert()
        .failure()
        .code(30);
}
