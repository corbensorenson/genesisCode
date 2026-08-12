use super::*;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(super) enum ObligationAuthorityOperation {
    UnitTests,
    Budgets,
}

impl ObligationAuthorityOperation {
    fn symbol(self) -> &'static str {
        match self {
            Self::UnitTests => ":unit-tests",
            Self::Budgets => ":budgets",
        }
    }

    fn obligation_name(self) -> &'static str {
        match self {
            Self::UnitTests => "core/obligation::unit-tests",
            Self::Budgets => "core/obligation::budgets",
        }
    }
}

fn authority_error(message: impl Into<String>) -> ObligationError {
    ObligationError::Test(format!("obligation authority: {}", message.into()))
}

fn map_field<'a>(map: &'a BTreeMap<TermOrdKey, Term>, field: &str) -> Option<&'a Term> {
    map.get(&TermOrdKey(Term::symbol(field)))
}

fn required_field<'a>(
    map: &'a BTreeMap<TermOrdKey, Term>,
    field: &str,
    context: &str,
) -> Result<&'a Term, ObligationError> {
    map_field(map, field)
        .ok_or_else(|| authority_error(format!("{context} missing required field {field}")))
}

fn exact_map<'a>(
    term: &'a Term,
    context: &str,
    fields: &[&str],
) -> Result<&'a BTreeMap<TermOrdKey, Term>, ObligationError> {
    let Term::Map(map) = term else {
        return Err(authority_error(format!("{context} must be a map")));
    };
    if map.len() != fields.len()
        || fields
            .iter()
            .any(|field| !map.contains_key(&TermOrdKey(Term::symbol(*field))))
    {
        return Err(authority_error(format!(
            "{context} must contain exactly [{}]",
            fields.join(", ")
        )));
    }
    Ok(map)
}

fn bool_field(
    map: &BTreeMap<TermOrdKey, Term>,
    field: &str,
    context: &str,
) -> Result<bool, ObligationError> {
    match map_field(map, field) {
        Some(Term::Bool(value)) => Ok(*value),
        _ => Err(authority_error(format!("{context} {field} must be bool"))),
    }
}

fn string_field(
    map: &BTreeMap<TermOrdKey, Term>,
    field: &str,
    context: &str,
) -> Result<String, ObligationError> {
    match map_field(map, field) {
        Some(Term::Str(value)) => Ok(value.clone()),
        _ => Err(authority_error(format!("{context} {field} must be string"))),
    }
}

fn string_vector(term: &Term, context: &str) -> Result<Vec<String>, ObligationError> {
    let Term::Vector(values) = term else {
        return Err(authority_error(format!("{context} must be a vector")));
    };
    values
        .iter()
        .map(|value| match value {
            Term::Str(value) => Ok(value.clone()),
            _ => Err(authority_error(format!(
                "{context} must contain only strings"
            ))),
        })
        .collect()
}

fn optional_hash_term(value: Option<[u8; 32]>) -> Term {
    value
        .map(|hash| Term::Bytes(hash.to_vec().into()))
        .unwrap_or(Term::Nil)
}

fn optional_u64_term(value: Option<u64>) -> Term {
    value
        .map(|value| Term::Int(BigInt::from(value)))
        .unwrap_or(Term::Nil)
}

fn unit_test_observations(
    store: &EvidenceStore,
    tests: &[TestRun],
) -> Result<Vec<Term>, ObligationError> {
    tests
        .iter()
        .map(|test| {
            let log_artifact = test
                .effect_log
                .as_ref()
                .map(|log| store.put_term(&log.to_term()))
                .transpose()?
                .map(Term::Str)
                .unwrap_or(Term::Nil);
            Ok(Term::Map(
                [
                    (
                        TermOrdKey(Term::symbol(":actual-h")),
                        Term::Bytes(test.value_hash.to_vec().into()),
                    ),
                    (
                        TermOrdKey(Term::symbol(":expected-h")),
                        optional_hash_term(test.expected_hash),
                    ),
                    (TermOrdKey(Term::symbol(":log-artifact")), log_artifact),
                    (
                        TermOrdKey(Term::symbol(":name")),
                        Term::Str(test.id.test_name.clone()),
                    ),
                    (
                        TermOrdKey(Term::symbol(":sealed-error")),
                        Term::Bool(test.sealed_error),
                    ),
                    (
                        TermOrdKey(Term::symbol(":suite")),
                        Term::symbol(test.id.suite_sym.clone()),
                    ),
                ]
                .into_iter()
                .collect(),
            ))
        })
        .collect()
}

fn budget_observations(tests: &[TestRun]) -> Vec<Term> {
    tests
        .iter()
        .map(|test| {
            Term::Map(
                [
                    (
                        TermOrdKey(Term::symbol(":effect-entries")),
                        Term::Int(BigInt::from(test.effect_entries)),
                    ),
                    (
                        TermOrdKey(Term::symbol(":effect-log-bytes")),
                        Term::Int(BigInt::from(test.effect_log_bytes)),
                    ),
                    (
                        TermOrdKey(Term::symbol(":name")),
                        Term::Str(test.id.test_name.clone()),
                    ),
                    (
                        TermOrdKey(Term::symbol(":steps")),
                        Term::Int(BigInt::from(test.steps)),
                    ),
                    (
                        TermOrdKey(Term::symbol(":suite")),
                        Term::symbol(test.id.suite_sym.clone()),
                    ),
                ]
                .into_iter()
                .collect(),
            )
        })
        .collect()
}

fn request_term(
    operation: ObligationAuthorityOperation,
    store: &EvidenceStore,
    manifest: &PackageManifest,
    tests: &[TestRun],
) -> Result<Term, ObligationError> {
    let (limits, observations) = match operation {
        ObligationAuthorityOperation::UnitTests => (
            Term::Map(BTreeMap::new()),
            unit_test_observations(store, tests)?,
        ),
        ObligationAuthorityOperation::Budgets => (
            Term::Map(
                [
                    (
                        TermOrdKey(Term::symbol(":max-effect-entries-per-test")),
                        optional_u64_term(manifest.budgets.max_effect_entries_per_test),
                    ),
                    (
                        TermOrdKey(Term::symbol(":max-effect-log-bytes-per-test")),
                        optional_u64_term(manifest.budgets.max_effect_log_bytes_per_test),
                    ),
                    (
                        TermOrdKey(Term::symbol(":max-steps-per-test")),
                        optional_u64_term(manifest.budgets.max_steps_per_test),
                    ),
                ]
                .into_iter()
                .collect(),
            ),
            budget_observations(tests),
        ),
    };
    Ok(Term::Map(
        [
            (
                TermOrdKey(Term::symbol(":kind")),
                Term::Str("genesis/obligation-authority-request-v0.1".to_string()),
            ),
            (TermOrdKey(Term::symbol(":limits")), limits),
            (
                TermOrdKey(Term::symbol(":operation")),
                Term::symbol(operation.symbol()),
            ),
            (
                TermOrdKey(Term::symbol(":package")),
                Term::Str(manifest.name.clone()),
            ),
            (
                TermOrdKey(Term::symbol(":tests")),
                Term::Vector(observations),
            ),
            (TermOrdKey(Term::symbol(":v")), Term::Int(1.into())),
        ]
        .into_iter()
        .collect(),
    ))
}

fn invoke_authority(
    request: Term,
    frontend: &CoreformFrontend,
    limits: KernelLimits,
) -> Result<Term, ObligationError> {
    let resolved_authority_frontend;
    let frontend = if frontend_is_rust(frontend) {
        resolved_authority_frontend = default_coreform_frontend();
        &resolved_authority_frontend
    } else {
        frontend
    };
    enforce_frontend_allowed(frontend, "obligation authority")?;
    let CoreformFrontend::Selfhost(config) = frontend else {
        return Err(authority_error("invalid obligation authority frontend"));
    };
    let mut ctx = EvalCtx::with_step_limit(None);
    ctx.set_mem_limits(limits.mem_limits);
    let prelude = build_prelude(&mut ctx);
    let mut env = prelude.env;
    load_selfhost_coreform_toolchain_v1_with_mode(
        &mut ctx,
        &mut env,
        config.bootstrap_mode,
        config.artifact.as_deref(),
    )
    .map_err(|error| authority_error(format!("selfhost/init: {error}")))?;
    ctx.steps = 0;
    ctx.step_limit = limits.step_limit.resolve();
    let authority = env.get("core/cli::obligation-authority").ok_or_else(|| {
        authority_error("missing required production binding core/cli::obligation-authority")
    })?;
    let value = authority
        .apply(&mut ctx, Value::data(request))
        .map_err(|error| authority_error(format!("authority apply failed: {error}")))?;
    if let Some(error) = extract_protocol_error(&ctx, &value) {
        return Err(authority_error(format!(
            "authority returned sealed error: {error}"
        )));
    }
    Ok(value
        .as_data()
        .cloned()
        .unwrap_or_else(|| value.to_term_for_log(ctx.protocol.map(|protocol| protocol.error))))
}

fn validate_unit_report(
    report: &Term,
    manifest: &PackageManifest,
    tests: &[TestRun],
    outer_ok: bool,
) -> Result<(), ObligationError> {
    let map = exact_map(
        report,
        "unit-test report",
        &[":kind", ":ok", ":package", ":tests"],
    )?;
    if string_field(map, ":kind", "unit-test report")? != "genesis/unit-tests-v0.2"
        || string_field(map, ":package", "unit-test report")? != manifest.name
        || bool_field(map, ":ok", "unit-test report")? != outer_ok
    {
        return Err(authority_error("unit-test report identity mismatch"));
    }
    let Some(Term::Vector(rows)) = map_field(map, ":tests") else {
        return Err(authority_error("unit-test report :tests must be vector"));
    };
    if rows.len() != tests.len() {
        return Err(authority_error(
            "unit-test report inventory length mismatch",
        ));
    }
    let mut folded_ok = true;
    for (row, test) in rows.iter().zip(tests) {
        let Term::Map(row_map) = row else {
            return Err(authority_error("unit-test row must be map"));
        };
        let row_ok = bool_field(row_map, ":ok", "unit-test row")?;
        let expected_fields = 4 + usize::from(!row_ok) + usize::from(test.effect_log.is_some());
        if row_map.len() != expected_fields
            || !matches!(map_field(row_map, ":suite"), Some(Term::Symbol(value)) if value == &test.id.suite_sym)
            || !matches!(map_field(row_map, ":name"), Some(Term::Str(value)) if value == &test.id.test_name)
            || !matches!(map_field(row_map, ":value-h"), Some(Term::Bytes(value)) if value.as_ref() == test.value_hash)
        {
            return Err(authority_error("unit-test row observation mismatch"));
        }
        if !row_ok
            && !matches!(map_field(row_map, ":error"), Some(Term::Str(value)) if value == "test failed")
        {
            return Err(authority_error(
                "failed unit-test row must carry canonical error",
            ));
        }
        if test.effect_log.is_some()
            != matches!(map_field(row_map, ":log-artifact"), Some(Term::Str(_)))
        {
            return Err(authority_error("unit-test row log artifact mismatch"));
        }
        folded_ok &= row_ok;
    }
    if folded_ok != outer_ok {
        return Err(authority_error("unit-test aggregate result mismatch"));
    }
    Ok(())
}

fn validate_budget_report(
    report: &Term,
    manifest: &PackageManifest,
    tests: &[TestRun],
    outer_ok: bool,
    errors: &[String],
) -> Result<(), ObligationError> {
    let fields = if outer_ok {
        vec![":kind", ":limits", ":ok", ":package", ":tests"]
    } else {
        vec![":errors", ":kind", ":limits", ":ok", ":package", ":tests"]
    };
    let map = exact_map(report, "budget report", &fields)?;
    if string_field(map, ":kind", "budget report")? != "genesis/budgets-v0.2"
        || string_field(map, ":package", "budget report")? != manifest.name
        || bool_field(map, ":ok", "budget report")? != outer_ok
    {
        return Err(authority_error("budget report identity mismatch"));
    }
    if !outer_ok
        && string_vector(
            required_field(map, ":errors", "budget report")?,
            "budget report :errors",
        )? != errors
    {
        return Err(authority_error("budget report errors mismatch"));
    }
    let Some(Term::Vector(rows)) = map_field(map, ":tests") else {
        return Err(authority_error("budget report :tests must be vector"));
    };
    if rows.len() != tests.len() {
        return Err(authority_error("budget report inventory length mismatch"));
    }
    let mut folded_ok = true;
    for (row, test) in rows.iter().zip(tests) {
        let row_map = exact_map(
            row,
            "budget row",
            &[
                ":effect-entries",
                ":effect-log-bytes",
                ":name",
                ":ok",
                ":steps",
                ":suite",
            ],
        )?;
        let row_ok = bool_field(row_map, ":ok", "budget row")?;
        if !matches!(map_field(row_map, ":suite"), Some(Term::Symbol(value)) if value == &test.id.suite_sym)
            || !matches!(map_field(row_map, ":name"), Some(Term::Str(value)) if value == &test.id.test_name)
            || !matches!(map_field(row_map, ":steps"), Some(Term::Int(value)) if value == &BigInt::from(test.steps))
            || !matches!(map_field(row_map, ":effect-entries"), Some(Term::Int(value)) if value == &BigInt::from(test.effect_entries))
            || !matches!(map_field(row_map, ":effect-log-bytes"), Some(Term::Int(value)) if value == &BigInt::from(test.effect_log_bytes))
        {
            return Err(authority_error("budget row observation mismatch"));
        }
        folded_ok &= row_ok;
    }
    if folded_ok != outer_ok {
        return Err(authority_error("budget aggregate result mismatch"));
    }
    Ok(())
}

pub(super) fn evaluate_obligation_with_authority(
    operation: ObligationAuthorityOperation,
    store: &EvidenceStore,
    manifest: &PackageManifest,
    tests: &[TestRun],
    frontend: &CoreformFrontend,
    limits: KernelLimits,
) -> Result<ObligationResult, ObligationError> {
    let term = invoke_authority(
        request_term(operation, store, manifest, tests)?,
        frontend,
        limits,
    )?;
    let map = exact_map(
        &term,
        "obligation authority result",
        &[
            ":errors",
            ":kind",
            ":name",
            ":ok",
            ":operation",
            ":report",
            ":v",
        ],
    )?;
    if string_field(map, ":kind", "obligation authority result")?
        != "genesis/obligation-authority-result-v0.1"
        || string_field(map, ":name", "obligation authority result")? != operation.obligation_name()
        || !matches!(map_field(map, ":operation"), Some(Term::Symbol(value)) if value == operation.symbol())
        || !matches!(map_field(map, ":v"), Some(Term::Int(value)) if value == &1.into())
    {
        return Err(authority_error("result identity mismatch"));
    }
    let ok = bool_field(map, ":ok", "obligation authority result")?;
    let errors = string_vector(
        required_field(map, ":errors", "obligation authority result")?,
        "obligation authority result :errors",
    )?;
    let report = required_field(map, ":report", "obligation authority result")?;
    match operation {
        ObligationAuthorityOperation::UnitTests => {
            validate_unit_report(report, manifest, tests, ok)?
        }
        ObligationAuthorityOperation::Budgets => {
            validate_budget_report(report, manifest, tests, ok, &errors)?
        }
    }
    let artifact = store.put_term(report)?;
    Ok(ObligationResult {
        name: operation.obligation_name().to_string(),
        ok,
        artifact: Some(artifact),
        errors,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use gc_prelude::SelfhostBootstrapMode;

    fn fixture_frontend() -> CoreformFrontend {
        CoreformFrontend::Selfhost(SelfhostFrontendConfig {
            bootstrap_mode: SelfhostBootstrapMode::ArtifactOnly,
            artifact: Some(
                PathBuf::from(env!("CARGO_MANIFEST_DIR"))
                    .join("../..")
                    .join("selfhost/toolchain.gc"),
            ),
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
        )
        .expect("request")
        {
            Term::Map(map) => map,
            _ => panic!("request constructor must return a map"),
        };
        open.insert(TermOrdKey(Term::symbol(":extra")), Term::Bool(true));
        let error = invoke_authority(Term::Map(open), &frontend, limits())
            .expect_err("open request must fail");
        assert!(error.to_string().contains("sealed error"));

        let unknown = Term::Map(
            [
                (
                    TermOrdKey(Term::symbol(":kind")),
                    Term::Str("genesis/obligation-authority-request-v0.1".to_string()),
                ),
                (
                    TermOrdKey(Term::symbol(":limits")),
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
                (TermOrdKey(Term::symbol(":tests")), Term::Vector(vec![])),
                (TermOrdKey(Term::symbol(":v")), Term::Int(1.into())),
            ]
            .into_iter()
            .collect(),
        );
        let error = invoke_authority(unknown, &frontend, limits())
            .expect_err("unknown operation must fail");
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
}
