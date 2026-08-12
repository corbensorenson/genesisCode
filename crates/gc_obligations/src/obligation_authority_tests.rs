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
        &[],
        hash_term(&request),
        result,
    )
    .expect_err("substituted request identity must fail closed");
    assert!(error.to_string().contains("result identity mismatch"));
}

#[test]
fn property_authority_plans_exact_seeds_and_rejects_seed_tampering() {
    let package = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("../..")
        .join("tests/spec/pkg_fail_property_tests/package.toml");
    let (manifest, package_dir) = PackageManifest::load(&package).expect("fixture manifest");
    let modules = load_modules(
        &package_dir,
        &manifest.modules,
        &fixture_frontend(),
        limits(),
    )
    .expect("fixture modules");
    let context = property_authority_context(&package_dir, &manifest, &modules, limits())
        .expect("property observations");
    let request = authority_request_term(
        ObligationAuthorityOperation::PropertyTests,
        &manifest.name,
        property_request_inputs(&context, ":plan", None),
    );
    let request_hash = hash_term(&request);
    let result = invoke_authority(request, &fixture_frontend(), limits()).expect("property plan");
    let plan = decode_property_plan_result(&manifest, &context, request_hash, result.clone())
        .expect("closed property plan");
    assert_eq!(plan.len(), 1);
    assert_eq!(
        plan[0].seeds,
        vec![
            16_683_527_519_985_115_011,
            16_484_361_299_643_496_824,
            10_195_190_632_922_736_109,
            9_306_649_646_461_805_599,
        ]
    );

    let mut tampered = result;
    let Term::Map(outer) = &mut tampered else {
        panic!("authority result must be map");
    };
    let Some(Term::Map(report)) = outer.get_mut(&TermOrdKey(Term::symbol(":report"))) else {
        panic!("plan report must be map");
    };
    let Some(Term::Vector(tests)) = report.get_mut(&TermOrdKey(Term::symbol(":tests"))) else {
        panic!("plan tests must be vector");
    };
    let Term::Map(test) = &mut tests[0] else {
        panic!("plan test must be map");
    };
    let Some(Term::Vector(seeds)) = test.get_mut(&TermOrdKey(Term::symbol(":seeds"))) else {
        panic!("plan seeds must be vector");
    };
    seeds[0] = Term::Int(BigInt::from(0));
    let error = decode_property_plan_result(&manifest, &context, request_hash, tampered)
        .expect_err("tampered property seed must fail closed");
    assert!(error.to_string().contains("property plan contradiction"));
}

#[test]
fn stage1_authority_aggregates_failures_and_rejects_report_tampering() {
    let package = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("../..")
        .join("tests/spec/pkg_basic/package.toml");
    let (manifest, package_dir) = PackageManifest::load(&package).expect("fixture manifest");
    let store = EvidenceStore::open(&package_dir).expect("fixture evidence store");
    let observations = vec![
        Stage1Observation {
            path: "a.gc".to_string(),
            original_module_hash: [1; 32],
            transformed_module_hash: [2; 32],
            original_value_hash: Some([3; 32]),
            transformed_value_hash: Some([3; 32]),
            original_eval_error: None,
            transformed_eval_error: None,
            egg_runs: 1,
            egg_iterations: 2,
            egg_eclasses: 3,
            egg_enodes: 4,
        },
        Stage1Observation {
            path: "b.gc".to_string(),
            original_module_hash: [5; 32],
            transformed_module_hash: [6; 32],
            original_value_hash: None,
            transformed_value_hash: Some([7; 32]),
            original_eval_error: Some("synthetic".to_string()),
            transformed_eval_error: None,
            egg_runs: 0,
            egg_iterations: 0,
            egg_eclasses: 0,
            egg_enodes: 0,
        },
        Stage1Observation {
            path: "c.gc".to_string(),
            original_module_hash: [8; 32],
            transformed_module_hash: [9; 32],
            original_value_hash: Some([10; 32]),
            transformed_value_hash: Some([11; 32]),
            original_eval_error: None,
            transformed_eval_error: None,
            egg_runs: 1,
            egg_iterations: 1,
            egg_eclasses: 1,
            egg_enodes: 1,
        },
    ];
    let request = authority_request_term(
        ObligationAuthorityOperation::Stage1Validation,
        &manifest.name,
        stage1_inputs(&observations),
    );
    let request_hash = hash_term(&request);
    let result = invoke_authority(request, &fixture_frontend(), limits()).expect("stage1 result");
    let decoded = decode_stage1_result(
        &store,
        &manifest,
        &observations,
        request_hash,
        result.clone(),
    )
    .expect("closed stage1 result");
    assert!(!decoded.ok);
    assert_eq!(
        decoded.errors,
        vec![
            "b.gc: original module is not gate-valid: synthetic",
            "c.gc: pure value hash mismatch after stage1 transform",
        ]
    );

    let mut substituted_observations = observations.clone();
    substituted_observations[0].path = "substituted.gc".to_string();
    let substituted_request = authority_request_term(
        ObligationAuthorityOperation::Stage1Validation,
        &manifest.name,
        stage1_inputs(&substituted_observations),
    );
    let substitution_error = decode_stage1_result(
        &store,
        &manifest,
        &substituted_observations,
        hash_term(&substituted_request),
        result.clone(),
    )
    .expect_err("stage1 result must remain bound to the exact observation request");
    assert!(substitution_error.to_string().contains("identity mismatch"));

    let mut malformed_inputs = stage1_inputs(&observations);
    let Term::Map(inputs) = &mut malformed_inputs else {
        panic!("stage1 inputs must be a map");
    };
    let Some(Term::Vector(modules)) = inputs.get_mut(&TermOrdKey(Term::symbol(":modules"))) else {
        panic!("stage1 modules must be a vector");
    };
    let Term::Map(module) = &mut modules[0] else {
        panic!("stage1 module must be a map");
    };
    module.insert(TermOrdKey(Term::symbol(":undeclared")), Term::Bool(true));
    let malformed_request = authority_request_term(
        ObligationAuthorityOperation::Stage1Validation,
        &manifest.name,
        malformed_inputs,
    );
    let malformed_error = invoke_authority(malformed_request, &fixture_frontend(), limits())
        .expect_err("open stage1 observation must fail closed");
    assert!(malformed_error.to_string().contains("exactly valid path"));

    let mut tampered = result;
    let Term::Map(outer) = &mut tampered else {
        panic!("authority result must be map");
    };
    let Some(Term::Map(report)) = outer.get_mut(&TermOrdKey(Term::symbol(":report"))) else {
        panic!("stage1 report must be map");
    };
    report.insert(TermOrdKey(Term::symbol(":ok")), Term::Bool(true));
    let error = decode_stage1_result(&store, &manifest, &observations, request_hash, tampered)
        .expect_err("tampered stage1 result must fail closed");
    assert!(
        error
            .to_string()
            .contains("contradicts optimizer observations")
    );
}

#[test]
fn stage1_eval_observation_obeys_caller_step_limit() {
    let forms = parse_module(
        "(def countdown (fn (n) (if (prim int/eq? n 0) 0 (countdown (prim int/sub n 1)))))\n(countdown 1000)\n",
    )
    .expect("bounded stage1 fixture");
    let (value_hash, error) = crate::obligation_stage::observe_stage1_eval(
        &forms,
        KernelLimits {
            step_limit: StepLimit::Limit(100),
            mem_limits: MemLimits::default(),
        },
    );
    assert_eq!(value_hash, None);
    assert!(
        error
            .as_deref()
            .is_some_and(|message| message.contains("step limit exceeded"))
    );
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
fn lint_and_ai_style_authorities_decide_and_persist_closed_artifacts() {
    let artifact = fixture_frontend();

    let (_temp, store, manifest, modules) = authority_fixture("pkg_lint");
    let lint = evaluate_obligation_with_authority(
        ObligationAuthorityOperation::Lint,
        &store,
        &manifest,
        &modules,
        &[],
        &artifact,
        limits(),
    )
    .expect("valid lint result");
    assert!(lint.ok, "{:?}", lint.errors);

    let (_temp, store, manifest, modules) = authority_fixture("pkg_fail_lint");
    let failed_lint = evaluate_obligation_with_authority(
        ObligationAuthorityOperation::Lint,
        &store,
        &manifest,
        &modules,
        &[],
        &artifact,
        limits(),
    )
    .expect("failing lint result");
    assert!(!failed_lint.ok);
    assert_eq!(
        failed_lint.errors,
        vec![
            "lint.gc: editor/lint/export-missing-def: export has no matching def: pkg/fail-lint::missing"
                .to_string()
        ]
    );

    let (_temp, store, manifest, modules) = authority_fixture("pkg_lint_autofix");
    let autofix = evaluate_obligation_with_authority(
        ObligationAuthorityOperation::Lint,
        &store,
        &manifest,
        &modules,
        &[],
        &artifact,
        limits(),
    )
    .expect("lint autofix result");
    assert!(autofix.ok);
    let report_path = store.path_for(autofix.artifact.as_deref().expect("lint report hash"));
    let report = parse_term(&std::fs::read_to_string(report_path).expect("lint report bytes"))
        .expect("lint report term");
    let Term::Map(report) = report else {
        panic!("lint report must be map");
    };
    let Some(Term::Vector(rows)) = report.get(&TermOrdKey(Term::symbol(":autofix-patches"))) else {
        panic!("lint report must contain autofix rows");
    };
    let Term::Map(first) = &rows[0] else {
        panic!("lint autofix row must be map");
    };
    let Some(Term::Str(patch_hash)) = first.get(&TermOrdKey(Term::symbol(":patch"))) else {
        panic!("lint autofix row must contain patch hash");
    };
    store
        .verify_hex(patch_hash)
        .expect("authority-produced patch must be persisted exactly");

    let (_temp, store, manifest, modules) = authority_fixture("pkg_ai_style");
    let style = evaluate_obligation_with_authority(
        ObligationAuthorityOperation::AiStyle,
        &store,
        &manifest,
        &modules,
        &[],
        &artifact,
        limits(),
    )
    .expect("valid AI-style result");
    assert!(style.ok, "{:?}", style.errors);

    let (_temp, store, manifest, modules) = authority_fixture("pkg_fail_ai_style");
    let failed_style = evaluate_obligation_with_authority(
        ObligationAuthorityOperation::AiStyle,
        &store,
        &manifest,
        &modules,
        &[],
        &artifact,
        limits(),
    )
    .expect("failing AI-style result");
    assert!(!failed_style.ok);
    assert_eq!(
        failed_style.errors,
        vec!["ai.gc: editor/lint/missing-intent: ::meta should include :intent string".to_string()]
    );

    let (_temp, store, manifest, modules) = authority_fixture("pkg_lint_autofix");
    let autofix_style = evaluate_obligation_with_authority(
        ObligationAuthorityOperation::AiStyle,
        &store,
        &manifest,
        &modules,
        &[],
        &artifact,
        limits(),
    )
    .expect("AI-style autofix result");
    assert!(!autofix_style.ok);
    assert_eq!(
        autofix_style.errors,
        vec![
            "lint.gc: editor/lint/missing-intent: ::meta should include :intent string".to_string(),
            "lint.gc: editor/lint/missing-types-map: ::meta should include :types map".to_string(),
        ]
    );
}

#[test]
fn lint_authority_rejects_side_artifact_and_final_report_tampering() {
    let (_temp, store, manifest, modules) = authority_fixture("pkg_lint_autofix");
    let request = request_term(
        ObligationAuthorityOperation::Lint,
        &store,
        &manifest,
        &modules,
        &[],
    )
    .expect("lint request");
    let mut result = invoke_authority(request.clone(), &fixture_frontend(), limits())
        .expect("lint authority result");
    let Term::Map(result_map) = &mut result else {
        panic!("authority result must be map");
    };
    let Some(Term::Map(transport)) = result_map.get_mut(&TermOrdKey(Term::symbol(":report")))
    else {
        panic!("lint transport must be map");
    };
    let Some(Term::Vector(artifacts)) =
        transport.get_mut(&TermOrdKey(Term::symbol(":artifact-terms")))
    else {
        panic!("lint transport must contain artifacts");
    };
    let Term::Map(first) = &mut artifacts[0] else {
        panic!("side artifact must be map");
    };
    first.insert(TermOrdKey(Term::symbol(":hash")), Term::Str("0".repeat(64)));
    let error = decode_authority_result(
        ObligationAuthorityOperation::Lint,
        &store,
        &manifest,
        &modules,
        &[],
        &[],
        hash_term(&request),
        result,
    )
    .expect_err("substituted side artifact hash must fail closed");
    assert!(error.to_string().contains("side artifact hash mismatch"));

    let (_temp, store, manifest, modules) = authority_fixture("pkg_ai_style");
    let request = request_term(
        ObligationAuthorityOperation::AiStyle,
        &store,
        &manifest,
        &modules,
        &[],
    )
    .expect("AI-style request");
    let mut result = invoke_authority(request.clone(), &fixture_frontend(), limits())
        .expect("AI-style authority result");
    let Term::Map(result_map) = &mut result else {
        panic!("authority result must be map");
    };
    let Some(Term::Map(transport)) = result_map.get_mut(&TermOrdKey(Term::symbol(":report")))
    else {
        panic!("AI-style transport must be map");
    };
    let Some(Term::Map(final_report)) = transport.get_mut(&TermOrdKey(Term::symbol(":final")))
    else {
        panic!("AI-style final report must be map");
    };
    final_report.insert(TermOrdKey(Term::symbol(":ok")), Term::Bool(false));
    let error = decode_authority_result(
        ObligationAuthorityOperation::AiStyle,
        &store,
        &manifest,
        &modules,
        &[],
        &[],
        hash_term(&request),
        result,
    )
    .expect_err("contradictory AI-style report must fail closed");
    assert!(
        error
            .to_string()
            .contains("AI-style report identity mismatch")
    );
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

fn replay_observation(actual: u8, replayed: u8) -> ReplayObservation {
    ReplayObservation {
        suite: "fixture/tests".to_string(),
        name: "task-case".to_string(),
        log_artifact: "1".repeat(64),
        program: true,
        actual_hash: [actual; 32],
        replay_hash: Some([replayed; 32]),
        entries: vec![
            ReplayEntryObservation {
                position: 0,
                op: "core/task::spawn".to_string(),
                task_id: Some("task-0001".to_string()),
                schedule_step: Some(0),
                await_edge: None,
            },
            ReplayEntryObservation {
                position: 1,
                op: "core/task::await".to_string(),
                task_id: Some("task-0001".to_string()),
                schedule_step: Some(1),
                await_edge: Some("task-0001".to_string()),
            },
        ],
    }
}

#[test]
fn replay_authorities_decide_from_closed_host_observations() {
    let temp = tempfile::tempdir().expect("tempdir");
    let store = EvidenceStore::open(&temp.path().join("store")).expect("evidence store");
    let (manifest, _) = PackageManifest::load(
        &PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .join("../..")
            .join("tests/spec/pkg_basic/package.toml"),
    )
    .expect("fixture manifest");
    let frontend = fixture_frontend();

    for operation in [
        ObligationAuthorityOperation::ReplayableTests,
        ObligationAuthorityOperation::ConcurrencyReplay,
    ] {
        let passed = evaluate_replay_obligation_with_authority(
            operation,
            &store,
            &manifest,
            &[replay_observation(7, 7)],
            &frontend,
            limits(),
        )
        .expect("matching replay observation");
        assert!(passed.ok);

        let failed = evaluate_replay_obligation_with_authority(
            operation,
            &store,
            &manifest,
            &[replay_observation(7, 8)],
            &frontend,
            limits(),
        )
        .expect("mismatched replay observation");
        assert!(!failed.ok);
        assert_eq!(failed.errors.len(), 1);
        assert!(failed.errors[0].contains("replay mismatch"));
    }

    let mut malformed = replay_observation(7, 7);
    malformed.entries[1].schedule_step = None;
    malformed.entries[1].task_id = None;
    malformed.entries[1].await_edge = None;
    let failed = evaluate_replay_obligation_with_authority(
        ObligationAuthorityOperation::ConcurrencyReplay,
        &store,
        &manifest,
        &[malformed],
        &frontend,
        limits(),
    )
    .expect("malformed scheduling facts are a failed policy decision");
    assert!(!failed.ok);
    assert_eq!(failed.errors.len(), 3);
    assert!(failed.errors[0].contains("expected :schedule-step 1, got None"));
    assert!(failed.errors[1].contains("missing :await-edge"));
    assert!(failed.errors[2].contains("missing :task-id"));
}

#[test]
fn replay_authority_rejects_open_observations_and_contradictory_reports() {
    let temp = tempfile::tempdir().expect("tempdir");
    let store = EvidenceStore::open(&temp.path().join("store")).expect("evidence store");
    let (manifest, _) = PackageManifest::load(
        &PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .join("../..")
            .join("tests/spec/pkg_basic/package.toml"),
    )
    .expect("fixture manifest");
    let frontend = fixture_frontend();
    let observations = vec![replay_observation(7, 7)];
    let request = authority_request_term(
        ObligationAuthorityOperation::ConcurrencyReplay,
        &manifest.name,
        replay_inputs(&observations),
    );
    let request_hash = hash_term(&request);

    let mut open_request = request.clone();
    let Term::Map(request_map) = &mut open_request else {
        panic!("request must be map");
    };
    let Some(Term::Map(inputs)) = request_map.get_mut(&TermOrdKey(Term::symbol(":inputs"))) else {
        panic!("request inputs must be map");
    };
    let Some(Term::Vector(tests)) = inputs.get_mut(&TermOrdKey(Term::symbol(":tests"))) else {
        panic!("request tests must be vector");
    };
    let Term::Map(test) = &mut tests[0] else {
        panic!("test observation must be map");
    };
    test.insert(TermOrdKey(Term::symbol(":trusted-ok")), Term::Bool(true));
    let error = invoke_authority(open_request, &frontend, limits())
        .expect_err("open replay observation must fail closed");
    assert!(error.to_string().contains("sealed error"));

    let mut result = invoke_authority(request, &frontend, limits()).expect("authority result");
    let Term::Map(result_map) = &mut result else {
        panic!("result must be map");
    };
    let Some(Term::Map(report)) = result_map.get_mut(&TermOrdKey(Term::symbol(":report"))) else {
        panic!("report must be map");
    };
    report.insert(
        TermOrdKey(Term::symbol(":concurrent-tests")),
        Term::Int(99.into()),
    );
    let error = decode_authority_result(
        ObligationAuthorityOperation::ConcurrencyReplay,
        &store,
        &manifest,
        &[],
        &[],
        &observations,
        request_hash,
        result,
    )
    .expect_err("contradictory concurrency count must fail closed");
    assert!(error.to_string().contains("contradicts host observations"));
}
