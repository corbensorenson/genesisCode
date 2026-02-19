use std::fs;
use std::path::Path;
use std::sync::OnceLock;

use assert_cmd::cargo::cargo_bin_cmd;
use gc_coreform::{Term, TermOrdKey, parse_term, print_term};
use serde_json::Value as JsonValue;
use tempfile::tempdir;

fn map_get<'a>(m: &'a std::collections::BTreeMap<TermOrdKey, Term>, k: &str) -> Option<&'a Term> {
    m.get(&TermOrdKey(Term::symbol(k)))
}

struct BaselineArtifact {
    bytes: Vec<u8>,
    supported: u64,
    validated: u64,
}

fn baseline() -> &'static BaselineArtifact {
    static BASELINE: OnceLock<BaselineArtifact> = OnceLock::new();
    BASELINE.get_or_init(|| {
        // Use the repo toolchain artifact as the baseline to keep test iteration fast.
        // If it ever diverges from a fresh rebuild, the determinism test will fail and force
        // the repo artifact to be updated.
        let repo_root = Path::new(env!("CARGO_MANIFEST_DIR"))
            .ancestors()
            .nth(2)
            .expect("workspace root");
        let artifact = repo_root.join("selfhost").join("toolchain.gc");
        assert!(artifact.is_file(), "missing {}", artifact.display());
        let bytes = fs::read(&artifact).unwrap();

        let s = std::str::from_utf8(&bytes).expect("artifact is utf-8 text");
        let term = parse_term(s).unwrap();
        let Term::Map(root) = term else {
            panic!("artifact must be a map");
        };
        let modules = map_get(&root, ":modules").expect("modules");
        let Term::Vector(mods) = modules else {
            panic!(":modules must be vector");
        };
        let mut supported = 0u64;
        let mut validated = 0u64;
        for m in mods {
            let Term::Map(mm) = m else { continue };
            let sup = matches!(map_get(mm, ":stage2-supported"), Some(Term::Bool(true)));
            let ok = matches!(map_get(mm, ":stage2-ok"), Some(Term::Bool(true)));
            if sup {
                supported += 1;
                if ok {
                    validated += 1;
                }
            }
        }

        BaselineArtifact {
            bytes,
            supported,
            validated,
        }
    })
}

fn write_baseline_copy(dst: &Path) {
    fs::write(dst, &baseline().bytes).unwrap();
}

#[test]
fn selfhost_artifact_with_noncanonical_forms_is_rejected_even_if_hash_matches_forms() {
    let td = tempdir().unwrap();
    let artifact = td.path().join("selfhost_toolchain.gc");
    let file = td.path().join("m.gc");

    write_baseline_copy(&artifact);

    // Replace the canonical forms for one module with intentionally non-canonical forms, and
    // update :module-h to match those forms. The loader must still reject it.
    let artifact_s = fs::read_to_string(&artifact).unwrap();
    let mut term = parse_term(&artifact_s).unwrap();
    let Term::Map(root) = &mut term else {
        panic!("artifact must be a map");
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

    let noncanon_src = std::fs::read_to_string(
        std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("../../tests/spec/coreform/app_sugar.in.gc"),
    )
    .unwrap();
    let noncanon_forms = gc_coreform::parse_module(&noncanon_src).unwrap();
    let noncanon_hash = gc_coreform::hash_module(&noncanon_forms);

    first.insert(
        TermOrdKey(Term::symbol(":forms")),
        Term::Vector(noncanon_forms),
    );
    first.insert(
        TermOrdKey(Term::symbol(":module-h")),
        Term::Bytes(noncanon_hash.to_vec().into()),
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

#[test]
fn selfhost_artifact_is_byte_for_byte_deterministic_across_rebuilds() {
    let td = tempdir().unwrap();
    let artifact_b = td.path().join("b.gc");

    // Compare a fresh build against the cached baseline build to guarantee determinism.
    cargo_bin_cmd!("genesis")
        .args(["selfhost-artifact", "--out"])
        .arg(&artifact_b)
        .assert()
        .success();

    let a = &baseline().bytes;
    let b = fs::read(&artifact_b).unwrap();
    assert_eq!(a, &b, "selfhost-artifact output must be deterministic");
}

#[test]
fn selfhost_artifact_can_be_built_and_used_for_selfhost_fmt() {
    let td = tempdir().unwrap();
    let artifact = td.path().join("selfhost_toolchain.gc");
    let file = td.path().join("m.gc");

    write_baseline_copy(&artifact);

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
    cargo_bin_cmd!("genesis_parity")
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

    write_baseline_copy(&artifact);

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

#[test]
fn selfhost_artifact_tampered_forms_are_rejected_even_if_module_hash_is_unchanged() {
    let td = tempdir().unwrap();
    let artifact = td.path().join("selfhost_toolchain.gc");
    let file = td.path().join("m.gc");

    write_baseline_copy(&artifact);

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

    // Replace canonical forms but keep :module-h intact.
    first.insert(
        TermOrdKey(Term::symbol(":forms")),
        Term::Vector(vec![Term::Nil]),
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

#[test]
fn selfhost_artifact_thresholds_accept_exact_observed_stage2_coverage() {
    let td = tempdir().unwrap();
    let gated_artifact = td.path().join("gated.gc");

    let supported = baseline().supported;
    let validated = baseline().validated;

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
    let failing_artifact = td.path().join("failing.gc");

    let supported = baseline().supported;
    let validated = baseline().validated;

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

#[test]
fn selfhost_artifact_includes_cli_core_module_with_passing_stage1_gate() {
    let artifact_s = std::str::from_utf8(&baseline().bytes).expect("utf-8 baseline artifact");
    let term = parse_term(artifact_s).unwrap();
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
