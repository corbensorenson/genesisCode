use super::*;
use gc_prelude::SelfhostBootstrapMode;

fn fixture_frontend() -> CoreformFrontend {
    let artifact = std::env::var_os("GENESIS_TEST_SELFHOST_ARTIFACT")
        .map(PathBuf::from)
        .unwrap_or_else(|| {
            PathBuf::from(env!("CARGO_MANIFEST_DIR"))
                .join("../..")
                .join("selfhost/toolchain.gc")
        });
    CoreformFrontend::Selfhost(SelfhostFrontendConfig {
        bootstrap_mode: SelfhostBootstrapMode::ArtifactOnly,
        artifact: Some(artifact),
    })
}

fn limits() -> KernelLimits {
    KernelLimits {
        step_limit: StepLimit::Default,
        mem_limits: MemLimits::default(),
    }
}

fn test_run(actual: u8, expected: Option<u8>, sealed_error: bool) -> TestRun {
    TestRun {
        id: TestId {
            suite_sym: "fixture/tests".to_string(),
            test_name: "case".to_string(),
        },
        sealed_error,
        expected_hash: expected.map(|value| [value; 32]),
        effect_log: None,
        steps: 11,
        effect_entries: 2,
        effect_log_bytes: 17,
        value_hash: [actual; 32],
    }
}

#[test]
fn unit_test_authority_decides_from_raw_hashes_and_sealed_error() {
    let temp = tempfile::tempdir().expect("tempdir");
    let store = EvidenceStore::open(&temp.path().join("store")).expect("evidence store");
    let (manifest, _) = PackageManifest::load(
        &PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .join("../..")
            .join("tests/spec/pkg_basic/package.toml"),
    )
    .expect("fixture manifest");
    let frontend = fixture_frontend();

    let passed = evaluate_obligation_with_authority(
        ObligationAuthorityOperation::UnitTests,
        &store,
        &manifest,
        &[],
        &[test_run(7, Some(7), false)],
        &frontend,
        limits(),
    )
    .expect("matching hash authority result");
    assert!(passed.ok);

    let mismatch = evaluate_obligation_with_authority(
        ObligationAuthorityOperation::UnitTests,
        &store,
        &manifest,
        &[],
        &[test_run(7, Some(8), false)],
        &frontend,
        limits(),
    )
    .expect("mismatched hash authority result");
    assert!(!mismatch.ok);

    let sealed = evaluate_obligation_with_authority(
        ObligationAuthorityOperation::UnitTests,
        &store,
        &manifest,
        &[],
        &[test_run(7, None, true)],
        &frontend,
        limits(),
    )
    .expect("sealed error authority result");
    assert!(!sealed.ok);
}

#[test]
fn rust_frontend_selection_does_not_replace_selfhost_obligation_authority() {
    let temp = tempfile::tempdir().expect("tempdir");
    let store = EvidenceStore::open(&temp.path().join("store")).expect("evidence store");
    let (manifest, _) = PackageManifest::load(
        &PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .join("../..")
            .join("tests/spec/pkg_basic/package.toml"),
    )
    .expect("fixture manifest");

    let result = evaluate_obligation_with_authority(
        ObligationAuthorityOperation::UnitTests,
        &store,
        &manifest,
        &[],
        &[test_run(7, Some(7), false)],
        &CoreformFrontend::Rust,
        limits(),
    )
    .expect("Rust frontend selection must retain GenesisCode obligation authority");
    assert!(result.ok);
}

#[test]
fn obligation_authority_rejects_open_and_unknown_requests() {
    let frontend = fixture_frontend();
    let mut open = match request_term(
        ObligationAuthorityOperation::UnitTests,
        &EvidenceStore::open(&tempfile::tempdir().expect("tempdir").path().join("store"))
            .expect("evidence store"),
        &PackageManifest::load(
            &PathBuf::from(env!("CARGO_MANIFEST_DIR"))
                .join("../..")
                .join("tests/spec/pkg_basic/package.toml"),
        )
        .expect("fixture manifest")
        .0,
        &[],
        &[],
    )
    .expect("request")
    {
        Term::Map(map) => map,
        _ => panic!("request constructor must return a map"),
    };
    open.insert(TermOrdKey(Term::symbol(":extra")), Term::Bool(true));
    let error =
        invoke_authority(Term::Map(open), &frontend, limits()).expect_err("open request must fail");
    assert!(error.to_string().contains("sealed error"));

    let unknown = Term::Map(
        [
            (
                TermOrdKey(Term::symbol(":kind")),
                Term::Str("genesis/obligation-authority-request-v0.2".to_string()),
            ),
            (
                TermOrdKey(Term::symbol(":inputs")),
                Term::Map(BTreeMap::new()),
            ),
            (
                TermOrdKey(Term::symbol(":operation")),
                Term::symbol(":unknown"),
            ),
            (
                TermOrdKey(Term::symbol(":package")),
                Term::Str("fixture".to_string()),
            ),
            (TermOrdKey(Term::symbol(":v")), Term::Int(2.into())),
        ]
        .into_iter()
        .collect(),
    );
    let error =
        invoke_authority(unknown, &frontend, limits()).expect_err("unknown operation must fail");
    assert!(error.to_string().contains("sealed error"));
}

fn module_observation_request(operation: &str, caps: &str, suite: &str, used_ops: &[&str]) -> Term {
    let forms = canonicalize_module(
        parse_module(&format!(
            "(def ::meta (quote {{:caps [{caps}] :exports [fixture/tests] :types {{fixture/tests ?}}}}))\n(def fixture/tests {{}})"
        ))
        .expect("module parse"),
    )
    .expect("module canonicalization");
    Term::Map(
        [
            (
                TermOrdKey(Term::symbol(":inputs")),
                Term::Map(
                    [
                        (
                            TermOrdKey(Term::symbol(":modules")),
                            Term::Vector(vec![Term::Map(
                                [
                                    (TermOrdKey(Term::symbol(":forms")), Term::Vector(forms)),
                                    (
                                        TermOrdKey(Term::symbol(":path")),
                                        Term::Str("fixture.gc".to_string()),
                                    ),
                                ]
                                .into_iter()
                                .collect(),
                            )]),
                        ),
                        (
                            TermOrdKey(Term::symbol(":tests")),
                            Term::Vector(vec![Term::Map(
                                [
                                    (
                                        TermOrdKey(Term::symbol(":name")),
                                        Term::Str("case".to_string()),
                                    ),
                                    (TermOrdKey(Term::symbol(":suite")), Term::symbol(suite)),
                                    (
                                        TermOrdKey(Term::symbol(":used-ops")),
                                        Term::Vector(
                                            used_ops.iter().map(|op| Term::symbol(*op)).collect(),
                                        ),
                                    ),
                                ]
                                .into_iter()
                                .collect(),
                            )]),
                        ),
                    ]
                    .into_iter()
                    .collect(),
                ),
            ),
            (
                TermOrdKey(Term::symbol(":kind")),
                Term::Str("genesis/obligation-authority-request-v0.2".to_string()),
            ),
            (
                TermOrdKey(Term::symbol(":operation")),
                Term::symbol(operation),
            ),
            (
                TermOrdKey(Term::symbol(":package")),
                Term::Str("fixture".to_string()),
            ),
            (TermOrdKey(Term::symbol(":v")), Term::Int(2.into())),
        ]
        .into_iter()
        .collect(),
    )
}

fn capabilities_request(suite: &str, used_ops: &[&str]) -> Term {
    module_observation_request(":capabilities-declared", "sys/time::now", suite, used_ops)
}

fn determinism_request(suite: &str, used_ops: &[&str]) -> Term {
    module_observation_request(":determinism", "", suite, used_ops)
}

#[test]
fn capability_authority_resolves_suite_and_decides_observed_operations() {
    let frontend = fixture_frontend();
    let allowed = invoke_authority(
        capabilities_request("fixture/tests", &["sys/time::now"]),
        &frontend,
        limits(),
    )
    .expect("declared capability result");
    assert_eq!(term_map_get_bool(&allowed, ":ok"), Some(true));

    let denied = invoke_authority(
        capabilities_request("fixture/tests", &["io/fs::write"]),
        &frontend,
        limits(),
    )
    .expect("undeclared capability result");
    assert_eq!(term_map_get_bool(&denied, ":ok"), Some(false));
    let Term::Map(map) = denied else {
        panic!("authority result must be a map");
    };
    assert_eq!(
        string_vector(
            required_field(&map, ":errors", "result").expect("errors field"),
            "errors",
        )
        .expect("error strings"),
        vec![
            "test case used op io/fs::write but module fixture.gc did not declare it in :caps"
                .to_string()
        ]
    );

    let unknown_suite = invoke_authority(
        capabilities_request("fixture/missing", &["sys/time::now"]),
        &frontend,
        limits(),
    )
    .expect("unknown suite is a failed obligation decision");
    assert_eq!(term_map_get_bool(&unknown_suite, ":ok"), Some(false));
}

#[test]
fn capability_authority_rejects_open_test_observations() {
    let mut request = capabilities_request("fixture/tests", &["sys/time::now"]);
    let Term::Map(request_map) = &mut request else {
        panic!("request must be a map");
    };
    let Some(Term::Map(inputs)) = request_map.get_mut(&TermOrdKey(Term::symbol(":inputs"))) else {
        panic!("inputs must be a map");
    };
    let Some(Term::Vector(tests)) = inputs.get_mut(&TermOrdKey(Term::symbol(":tests"))) else {
        panic!("tests must be a vector");
    };
    let Term::Map(test) = &mut tests[0] else {
        panic!("test must be a map");
    };
    test.insert(TermOrdKey(Term::symbol(":extra")), Term::Bool(true));
    let error = invoke_authority(request, &fixture_frontend(), limits())
        .expect_err("open capability observation must fail");
    assert!(error.to_string().contains("sealed error"));
}

#[test]
fn capability_authority_validates_all_modules_with_no_effectful_tests() {
    let mut request = capabilities_request("fixture/tests", &[]);
    let Term::Map(request_map) = &mut request else {
        panic!("request must be a map");
    };
    let Some(Term::Map(inputs)) = request_map.get_mut(&TermOrdKey(Term::symbol(":inputs"))) else {
        panic!("inputs must be a map");
    };
    inputs.insert(TermOrdKey(Term::symbol(":tests")), Term::Vector(Vec::new()));
    let Some(Term::Vector(modules)) = inputs.get_mut(&TermOrdKey(Term::symbol(":modules"))) else {
        panic!("modules must be a vector");
    };
    let Term::Map(module) = &mut modules[0] else {
        panic!("module must be a map");
    };
    module.insert(TermOrdKey(Term::symbol(":extra")), Term::Bool(true));
    let error = invoke_authority(request, &fixture_frontend(), limits())
        .expect_err("unused open module observation must fail");
    assert!(error.to_string().contains("sealed error"));
}

#[test]
fn determinism_authority_decides_static_and_runtime_effect_rules() {
    let frontend = fixture_frontend();
    let pure = invoke_authority(
        determinism_request("fixture/tests", &[]),
        &frontend,
        limits(),
    )
    .expect("pure determinism result");
    assert_eq!(term_map_get_bool(&pure, ":ok"), Some(true));

    let effectful = invoke_authority(
        determinism_request("fixture/tests", &["io/fs::write"]),
        &frontend,
        limits(),
    )
    .expect("effectful determinism result");
    assert_eq!(term_map_get_bool(&effectful, ":ok"), Some(false));
    let Term::Map(effectful_map) = effectful else {
        panic!("authority result must be a map");
    };
    assert_eq!(
        string_vector(
            required_field(&effectful_map, ":errors", "result").expect("errors field"),
            "errors",
        )
        .expect("error strings"),
        vec![
            "test case in fixture/tests performed effects but module declares :caps []".to_string()
        ]
    );

    let unknown_suite = invoke_authority(
        determinism_request("fixture/missing", &["io/fs::write"]),
        &frontend,
        limits(),
    )
    .expect("unknown suite retains legacy no-owner semantics");
    assert_eq!(term_map_get_bool(&unknown_suite, ":ok"), Some(true));

    let (_temp, store, manifest, modules) = authority_fixture("pkg_fail_determinism");
    let static_failure = evaluate_obligation_with_authority(
        ObligationAuthorityOperation::Determinism,
        &store,
        &manifest,
        &modules,
        &[],
        &frontend,
        limits(),
    )
    .expect("static determinism result");
    assert!(!static_failure.ok);
    assert_eq!(
        static_failure.errors,
        vec![
            "fail.gc declares :caps [] but has inferred effects (unknown=false, ops={\"io/fs::write\"})"
                .to_string()
        ]
    );
}

#[test]
fn determinism_authority_rejects_open_observations_and_contradictory_reports() {
    let mut request = determinism_request("fixture/tests", &[]);
    let Term::Map(request_map) = &mut request else {
        panic!("request must be a map");
    };
    let Some(Term::Map(inputs)) = request_map.get_mut(&TermOrdKey(Term::symbol(":inputs"))) else {
        panic!("inputs must be a map");
    };
    let Some(Term::Vector(tests)) = inputs.get_mut(&TermOrdKey(Term::symbol(":tests"))) else {
        panic!("tests must be a vector");
    };
    let Term::Map(test) = &mut tests[0] else {
        panic!("test must be a map");
    };
    test.insert(TermOrdKey(Term::symbol(":effectful")), Term::Bool(false));
    let error = invoke_authority(request, &fixture_frontend(), limits())
        .expect_err("open determinism observation must fail");
    assert!(error.to_string().contains("sealed error"));

    let (_temp, store, manifest, modules) = authority_fixture("pkg_basic");
    let request = request_term(
        ObligationAuthorityOperation::Determinism,
        &store,
        &manifest,
        &modules,
        &[],
    )
    .expect("determinism request");
    let mut result = invoke_authority(request.clone(), &fixture_frontend(), limits())
        .expect("determinism result");
    let Term::Map(result_map) = &mut result else {
        panic!("result must be a map");
    };
    let Some(Term::Map(report)) = result_map.get_mut(&TermOrdKey(Term::symbol(":report"))) else {
        panic!("report must be a map");
    };
    report.insert(TermOrdKey(Term::symbol(":ok")), Term::Bool(false));
    let error = decode_authority_result(
        ObligationAuthorityOperation::Determinism,
        &store,
        &manifest,
        &modules,
        &[],
        hash_term(&request),
        result,
    )
    .expect_err("contradictory determinism report must fail closed");
    assert!(
        error
            .to_string()
            .contains("determinism report identity or aggregate mismatch")
    );
}

#[test]
fn obligation_authority_rejects_result_bound_to_another_request() {
    let temp = tempfile::tempdir().expect("tempdir");
    let store = EvidenceStore::open(&temp.path().join("store")).expect("evidence store");
    let (manifest, _) = PackageManifest::load(
        &PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .join("../..")
            .join("tests/spec/pkg_basic/package.toml"),
    )
    .expect("fixture manifest");
    let request = request_term(
        ObligationAuthorityOperation::UnitTests,
        &store,
        &manifest,
        &[],
        &[test_run(7, Some(7), false)],
    )
    .expect("request");
    let mut result =
        invoke_authority(request.clone(), &fixture_frontend(), limits()).expect("authority result");
    let Term::Map(result_map) = &mut result else {
        panic!("authority result must be a map");
    };
    result_map.insert(
        TermOrdKey(Term::symbol(":request-h")),
        Term::Str("0".repeat(64)),
    );
    let error = decode_authority_result(
        ObligationAuthorityOperation::UnitTests,
        &store,
        &manifest,
        &[],
        &[test_run(7, Some(7), false)],
        hash_term(&request),
        result,
    )
    .expect_err("substituted request identity must fail closed");
    assert!(error.to_string().contains("result identity mismatch"));
}

fn authority_fixture(
    fixture: &str,
) -> (
    tempfile::TempDir,
    EvidenceStore,
    PackageManifest,
    Vec<LoadedModule>,
) {
    let temp = tempfile::tempdir().expect("tempdir");
    let store = EvidenceStore::open(&temp.path().join("store")).expect("evidence store");
    let package = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("../..")
        .join("tests/spec")
        .join(fixture)
        .join("package.toml");
    let (manifest, package_dir) = PackageManifest::load(&package).expect("fixture manifest");
    let modules = load_modules(
        &package_dir,
        &manifest.modules,
        &fixture_frontend(),
        limits(),
    )
    .expect("fixture modules");
    (temp, store, manifest, modules)
}

#[test]
fn typecheck_obligation_authority_runs_existing_selfhost_checker() {
    let (_temp, store, manifest, modules) = authority_fixture("pkg_typecheck_inference_parity");
    let passed = evaluate_obligation_with_authority(
        ObligationAuthorityOperation::Typecheck,
        &store,
        &manifest,
        &modules,
        &[],
        &fixture_frontend(),
        limits(),
    )
    .expect("valid package typecheck result");
    assert!(passed.ok);
    assert!(passed.errors.is_empty());

    let (_temp, store, manifest, modules) = authority_fixture("pkg_fail_typecheck");
    let failed = evaluate_obligation_with_authority(
        ObligationAuthorityOperation::Typecheck,
        &store,
        &manifest,
        &modules,
        &[],
        &fixture_frontend(),
        limits(),
    )
    .expect("invalid package typecheck result");
    assert!(!failed.ok);
    assert!(!failed.errors.is_empty());

    let (_temp, store, manifest, modules) = authority_fixture("pkg_typecheck_strict");
    let strict_passed = evaluate_obligation_with_authority(
        ObligationAuthorityOperation::TypecheckStrict,
        &store,
        &manifest,
        &modules,
        &[],
        &fixture_frontend(),
        limits(),
    )
    .expect("valid strict package typecheck result");
    assert!(strict_passed.ok, "{:?}", strict_passed.errors);

    let (_temp, store, manifest, modules) = authority_fixture("pkg_fail_typecheck_strict");
    let strict_failed = evaluate_obligation_with_authority(
        ObligationAuthorityOperation::TypecheckStrict,
        &store,
        &manifest,
        &modules,
        &[],
        &fixture_frontend(),
        limits(),
    )
    .expect("invalid strict package typecheck result");
    assert!(!strict_failed.ok);
    assert!(
        strict_failed
            .errors
            .iter()
            .any(|error| { error.contains("strict effect mode forbids unknown effect ops") })
    );
}

#[test]
fn typecheck_obligation_authority_rejects_open_module_observations() {
    let (_temp, store, manifest, modules) = authority_fixture("pkg_typecheck_inference_parity");
    let mut request = request_term(
        ObligationAuthorityOperation::Typecheck,
        &store,
        &manifest,
        &modules,
        &[],
    )
    .expect("typecheck request");
    let Term::Map(request_map) = &mut request else {
        panic!("request must be a map");
    };
    let Some(Term::Map(inputs)) = request_map.get_mut(&TermOrdKey(Term::symbol(":inputs"))) else {
        panic!("inputs must be a map");
    };
    let Some(Term::Vector(observations)) = inputs.get_mut(&TermOrdKey(Term::symbol(":modules")))
    else {
        panic!("modules must be a vector");
    };
    let Term::Map(module) = &mut observations[0] else {
        panic!("module must be a map");
    };
    module.insert(
        TermOrdKey(Term::symbol(":meta")),
        Term::Map(BTreeMap::new()),
    );
    let error = invoke_authority(request, &fixture_frontend(), limits())
        .expect_err("host-supplied metadata must fail closed");
    assert!(error.to_string().contains("sealed error"));

    let mut strict_request = request_term(
        ObligationAuthorityOperation::TypecheckStrict,
        &store,
        &manifest,
        &modules,
        &[],
    )
    .expect("strict typecheck request");
    let Term::Map(strict_request_map) = &mut strict_request else {
        panic!("request must be a map");
    };
    let Some(Term::Map(strict_inputs)) =
        strict_request_map.get_mut(&TermOrdKey(Term::symbol(":inputs")))
    else {
        panic!("inputs must be a map");
    };
    let Some(Term::Vector(strict_observations)) =
        strict_inputs.get_mut(&TermOrdKey(Term::symbol(":modules")))
    else {
        panic!("modules must be a vector");
    };
    let Term::Map(strict_module) = &mut strict_observations[0] else {
        panic!("module must be a map");
    };
    strict_module.insert(
        TermOrdKey(Term::symbol(":strict-effects")),
        Term::Bool(false),
    );
    let error = invoke_authority(strict_request, &fixture_frontend(), limits())
        .expect_err("host-supplied strictness must fail closed");
    assert!(error.to_string().contains("sealed error"));
}

#[test]
fn budget_authority_applies_manifest_thresholds_to_raw_measurements() {
    let temp = tempfile::tempdir().expect("tempdir");
    let store = EvidenceStore::open(&temp.path().join("store")).expect("evidence store");
    let (manifest, _) = PackageManifest::load(
        &PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .join("../..")
            .join("tests/spec/pkg_fail_budgets/package.toml"),
    )
    .expect("fixture manifest");
    let frontend = fixture_frontend();

    let mut within = test_run(1, None, false);
    within.steps = 999;
    let passed = evaluate_obligation_with_authority(
        ObligationAuthorityOperation::Budgets,
        &store,
        &manifest,
        &[],
        &[within],
        &frontend,
        limits(),
    )
    .expect("within-budget authority result");
    assert!(passed.ok);

    let mut exceeded = test_run(1, None, false);
    exceeded.steps = 1001;
    let failed = evaluate_obligation_with_authority(
        ObligationAuthorityOperation::Budgets,
        &store,
        &manifest,
        &[],
        &[exceeded],
        &frontend,
        limits(),
    )
    .expect("over-budget authority result");
    assert!(!failed.ok);
    assert_eq!(
        failed.errors,
        vec!["test fixture/tests::case exceeded max_steps_per_test: 1001 > 1000"]
    );
}
