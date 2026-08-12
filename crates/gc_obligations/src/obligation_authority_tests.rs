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

fn capabilities_request(suite: &str, used_ops: &[&str]) -> Term {
    let forms = canonicalize_module(
            parse_module(
                "(def ::meta (quote {:caps [sys/time::now] :exports [fixture/tests] :types {fixture/tests ?}}))\n(def fixture/tests {})",
            )
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
                Term::symbol(":capabilities-declared"),
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
        &[test_run(7, Some(7), false)],
        hash_term(&request),
        result,
    )
    .expect_err("substituted request identity must fail closed");
    assert!(error.to_string().contains("result identity mismatch"));
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
