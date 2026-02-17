use std::fs;

use assert_cmd::cargo::cargo_bin_cmd;
use gc_coreform::{Term, TermOrdKey, parse_term, print_term};
use serde_json::Value as JsonValue;
use tempfile::tempdir;

fn map_get<'a>(m: &'a std::collections::BTreeMap<TermOrdKey, Term>, k: &str) -> Option<&'a Term> {
    m.get(&TermOrdKey(Term::symbol(k)))
}

#[test]
fn selfhost_artifact_can_be_built_and_used_for_selfhost_fmt() {
    let td = tempdir().unwrap();
    let artifact = td.path().join("selfhost_toolchain.gc");
    let file = td.path().join("m.gc");

    cargo_bin_cmd!("genesis")
        .args(["selfhost-artifact", "--out"])
        .arg(&artifact)
        .assert()
        .success();

    let artifact_s = fs::read_to_string(&artifact).unwrap();
    let term = parse_term(&artifact_s).unwrap();
    let Term::Map(root) = term else {
        panic!("artifact must be a map");
    };
    assert!(matches!(
        map_get(&root, ":kind"),
        Some(Term::Str(s)) if s == "genesis/selfhost-toolchain-artifact-v0.2"
    ));
    assert!(matches!(map_get(&root, ":v"), Some(Term::Int(i)) if i == &1.into()));

    let src = r#"
      (def x (prim int/add 1 2))
      x
    "#;
    fs::write(&file, src).unwrap();
    cargo_bin_cmd!("genesis")
        .env("GENESIS_ALLOW_RUST_ENGINE", "1")
        .args(["fmt", "--engine", "rust"])
        .arg(&file)
        .assert()
        .success();
    let expected = fs::read_to_string(&file).unwrap();

    fs::write(&file, src).unwrap();
    cargo_bin_cmd!("genesis")
        .env("GENESIS_SELFHOST_TOOLCHAIN_ARTIFACT", &artifact)
        .args(["fmt", "--engine", "selfhost"])
        .arg(&file)
        .assert()
        .success();
    let actual = fs::read_to_string(&file).unwrap();
    assert_eq!(expected, actual);
}

#[test]
fn invalid_selfhost_artifact_is_rejected_by_loader() {
    let td = tempdir().unwrap();
    let artifact = td.path().join("selfhost_toolchain.gc");
    let file = td.path().join("m.gc");

    cargo_bin_cmd!("genesis")
        .args(["selfhost-artifact", "--out"])
        .arg(&artifact)
        .assert()
        .success();

    let artifact_s = fs::read_to_string(&artifact).unwrap();
    let mut term = parse_term(&artifact_s).unwrap();
    let Term::Map(root) = &mut term else {
        panic!("artifact must be map");
    };
    let modules = root
        .get_mut(&TermOrdKey(Term::symbol(":modules")))
        .expect("modules");
    let Term::Vector(mods) = modules else {
        panic!("modules must be vector");
    };
    let Term::Map(first) = mods.first_mut().expect("first module") else {
        panic!("first module must be map");
    };
    first.insert(
        TermOrdKey(Term::symbol(":module-h")),
        Term::Bytes(vec![0u8; 32].into()),
    );
    fs::write(&artifact, print_term(&term)).unwrap();

    fs::write(&file, "(def x 1)\nx\n").unwrap();
    cargo_bin_cmd!("genesis")
        .env("GENESIS_SELFHOST_TOOLCHAIN_ARTIFACT", &artifact)
        .args(["fmt", "--engine", "selfhost"])
        .arg(&file)
        .assert()
        .failure()
        .code(1);
}

fn artifact_summary_counts(v: &JsonValue) -> (u64, u64) {
    let data = v
        .get("data")
        .expect("json envelope has data object for selfhost-artifact");
    let supported = data
        .get("stage2_supported_modules")
        .and_then(JsonValue::as_u64)
        .expect("stage2_supported_modules");
    let validated = data
        .get("stage2_validated_modules")
        .and_then(JsonValue::as_u64)
        .expect("stage2_validated_modules");
    (supported, validated)
}

#[test]
fn selfhost_artifact_thresholds_accept_exact_observed_stage2_coverage() {
    let td = tempdir().unwrap();
    let baseline_artifact = td.path().join("baseline.gc");
    let gated_artifact = td.path().join("gated.gc");

    let baseline_out = cargo_bin_cmd!("genesis")
        .args(["selfhost-artifact", "--out"])
        .arg(&baseline_artifact)
        .arg("--json")
        .assert()
        .success()
        .get_output()
        .stdout
        .clone();
    let baseline_json: JsonValue = serde_json::from_slice(&baseline_out).unwrap();
    let (supported, validated) = artifact_summary_counts(&baseline_json);

    let gated_out = cargo_bin_cmd!("genesis")
        .args([
            "selfhost-artifact",
            "--out",
            gated_artifact.to_str().unwrap(),
            "--min-stage2-supported-modules",
            &supported.to_string(),
            "--min-stage2-validated-modules",
            &validated.to_string(),
            "--json",
        ])
        .assert()
        .success()
        .get_output()
        .stdout
        .clone();
    let gated_json: JsonValue = serde_json::from_slice(&gated_out).unwrap();
    assert!(
        gated_json
            .get("ok")
            .and_then(JsonValue::as_bool)
            .expect("ok bool")
    );
    let data = gated_json.get("data").expect("data object");
    assert!(
        data.get("stage2_requirements_ok")
            .and_then(JsonValue::as_bool)
            .expect("stage2_requirements_ok")
    );
    assert_eq!(
        data.get("min_stage2_supported_modules")
            .and_then(JsonValue::as_u64)
            .expect("min_stage2_supported_modules"),
        supported
    );
    assert_eq!(
        data.get("min_stage2_validated_modules")
            .and_then(JsonValue::as_u64)
            .expect("min_stage2_validated_modules"),
        validated
    );
}

#[test]
fn selfhost_artifact_thresholds_fail_when_minimums_exceed_observed_stage2_coverage() {
    let td = tempdir().unwrap();
    let baseline_artifact = td.path().join("baseline.gc");
    let failing_artifact = td.path().join("failing.gc");

    let baseline_out = cargo_bin_cmd!("genesis")
        .args(["selfhost-artifact", "--out"])
        .arg(&baseline_artifact)
        .arg("--json")
        .assert()
        .success()
        .get_output()
        .stdout
        .clone();
    let baseline_json: JsonValue = serde_json::from_slice(&baseline_out).unwrap();
    let (supported, validated) = artifact_summary_counts(&baseline_json);

    let failing_out = cargo_bin_cmd!("genesis")
        .args([
            "selfhost-artifact",
            "--out",
            failing_artifact.to_str().unwrap(),
            "--min-stage2-supported-modules",
            &(supported.saturating_add(1)).to_string(),
            "--min-stage2-validated-modules",
            &(validated.saturating_add(1)).to_string(),
            "--json",
        ])
        .assert()
        .failure()
        .code(30)
        .get_output()
        .stdout
        .clone();
    let failing_json: JsonValue = serde_json::from_slice(&failing_out).unwrap();
    assert!(
        !failing_json
            .get("ok")
            .and_then(JsonValue::as_bool)
            .expect("ok bool")
    );
    let data = failing_json.get("data").expect("data object");
    assert!(
        !data
            .get("stage2_requirements_ok")
            .and_then(JsonValue::as_bool)
            .expect("stage2_requirements_ok")
    );
    let errs = data
        .get("stage2_requirement_errors")
        .and_then(JsonValue::as_array)
        .expect("stage2_requirement_errors array");
    assert!(
        errs.len() >= 2,
        "expected at least two threshold failures, got {}",
        errs.len()
    );

    let artifact_s = fs::read_to_string(&failing_artifact).unwrap();
    let term = parse_term(&artifact_s).unwrap();
    let Term::Map(root) = term else {
        panic!("artifact must be a map");
    };
    assert!(matches!(map_get(&root, ":ok"), Some(Term::Bool(false))));
    let req = map_get(&root, ":stage2-requirements").expect("requirements map");
    let Term::Map(req_map) = req else {
        panic!(":stage2-requirements must be map");
    };
    assert!(matches!(map_get(req_map, ":ok"), Some(Term::Bool(false))));
    let req_errs = map_get(req_map, ":errors").expect("requirements errors");
    let Term::Vector(v) = req_errs else {
        panic!(":errors must be vector");
    };
    assert!(!v.is_empty(), "requirements errors should not be empty");
}
