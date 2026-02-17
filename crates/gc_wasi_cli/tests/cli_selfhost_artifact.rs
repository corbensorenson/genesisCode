use assert_cmd::cargo::cargo_bin_cmd;
use gc_coreform::{Term, TermOrdKey, parse_term};
use predicates::prelude::*;
use serde_json::Value as JsonValue;
use tempfile::tempdir;

fn map_get<'a>(m: &'a std::collections::BTreeMap<TermOrdKey, Term>, k: &str) -> Option<&'a Term> {
    m.get(&TermOrdKey(Term::symbol(k)))
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
fn selfhost_artifact_help_exposes_stage2_threshold_flags() {
    cargo_bin_cmd!("genesis_wasi")
        .args(["selfhost-artifact", "--help"])
        .assert()
        .success()
        .stdout(predicate::str::contains("--out"))
        .stdout(predicate::str::contains("--min-stage2-supported-modules"))
        .stdout(predicate::str::contains("--min-stage2-validated-modules"));
}

#[test]
fn selfhost_artifact_wasi_thresholds_accept_exact_observed_stage2_coverage() {
    let td = tempdir().unwrap();
    let baseline_artifact = td.path().join("baseline.gc");
    let gated_artifact = td.path().join("gated.gc");

    let baseline_out = cargo_bin_cmd!("genesis_wasi")
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

    let gated_out = cargo_bin_cmd!("genesis_wasi")
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

    let artifact_s = std::fs::read_to_string(&gated_artifact).unwrap();
    let term = parse_term(&artifact_s).unwrap();
    let Term::Map(root) = term else {
        panic!("artifact must be a map");
    };
    assert!(matches!(map_get(&root, ":ok"), Some(Term::Bool(true))));
}

#[test]
fn selfhost_artifact_wasi_thresholds_fail_when_minimums_exceed_observed_coverage() {
    let td = tempdir().unwrap();
    let baseline_artifact = td.path().join("baseline.gc");
    let failing_artifact = td.path().join("failing.gc");

    let baseline_out = cargo_bin_cmd!("genesis_wasi")
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

    let failing_out = cargo_bin_cmd!("genesis_wasi")
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

    let artifact_s = std::fs::read_to_string(&failing_artifact).unwrap();
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
}

#[test]
fn selfhost_artifact_wasi_includes_cli_core_module_with_passing_stage1_gate() {
    let td = tempdir().unwrap();
    let artifact = td.path().join("selfhost_toolchain.gc");

    cargo_bin_cmd!("genesis_wasi")
        .args(["selfhost-artifact", "--out"])
        .arg(&artifact)
        .assert()
        .success();

    let artifact_s = std::fs::read_to_string(&artifact).unwrap();
    let term = parse_term(&artifact_s).unwrap();
    let Term::Map(root) = term else {
        panic!("artifact must be map");
    };
    let modules = map_get(&root, ":modules").expect("modules");
    let Term::Vector(mods) = modules else {
        panic!(":modules must be vector");
    };
    let cli_module = mods
        .iter()
        .find_map(|m| {
            let Term::Map(mm) = m else {
                return None;
            };
            match map_get(mm, ":path") {
                Some(Term::Str(path)) if path == "selfhost/cli_coreform_v1.gc" => Some(mm),
                _ => None,
            }
        })
        .expect("artifact must contain selfhost/cli_coreform_v1.gc entry");

    assert!(matches!(
        map_get(cli_module, ":stage1-ok"),
        Some(Term::Bool(true))
    ));
    assert!(matches!(
        map_get(cli_module, ":stage1-errors"),
        Some(Term::Vector(v)) if v.is_empty()
    ));
}
