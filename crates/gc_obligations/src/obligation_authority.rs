use super::*;

include!("obligation_authority_caps.rs");
include!("obligation_authority_lint.rs");
include!("obligation_authority_property.rs");
include!("obligation_authority_property_finalize.rs");
include!("obligation_authority_replay.rs");
include!("obligation_authority_stage.rs");

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(super) enum ObligationAuthorityOperation {
    AiStyle,
    UnitTests,
    Budgets,
    CapabilitiesDeclared,
    ConcurrencyReplay,
    Determinism,
    Lint,
    PropertyTests,
    ReplayableTests,
    Stage1Validation,
    Typecheck,
    TypecheckStrict,
}

impl ObligationAuthorityOperation {
    fn symbol(self) -> &'static str {
        match self {
            Self::AiStyle => ":ai-style",
            Self::UnitTests => ":unit-tests",
            Self::Budgets => ":budgets",
            Self::CapabilitiesDeclared => ":capabilities-declared",
            Self::ConcurrencyReplay => ":concurrency-replay",
            Self::Determinism => ":determinism",
            Self::Lint => ":lint",
            Self::PropertyTests => ":property-tests",
            Self::ReplayableTests => ":replayable-tests",
            Self::Stage1Validation => ":stage1-validation",
            Self::Typecheck => ":typecheck",
            Self::TypecheckStrict => ":typecheck-strict",
        }
    }

    fn obligation_name(self) -> &'static str {
        match self {
            Self::AiStyle => "core/obligation::ai-style",
            Self::UnitTests => "core/obligation::unit-tests",
            Self::Budgets => "core/obligation::budgets",
            Self::CapabilitiesDeclared => "core/obligation::capabilities-declared",
            Self::ConcurrencyReplay => "core/obligation::concurrency-replay",
            Self::Determinism => "core/obligation::determinism",
            Self::Lint => "core/obligation::lint",
            Self::PropertyTests => "core/obligation::property-tests",
            Self::ReplayableTests => "core/obligation::replayable-tests",
            Self::Stage1Validation => "core/obligation::stage1-validation",
            Self::Typecheck => "core/obligation::typecheck",
            Self::TypecheckStrict => "core/obligation::typecheck-strict",
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
    modules: &[LoadedModule],
    tests: &[TestRun],
) -> Result<Term, ObligationError> {
    let inputs = match operation {
        ObligationAuthorityOperation::UnitTests => Term::Map(
            [(
                TermOrdKey(Term::symbol(":tests")),
                Term::Vector(unit_test_observations(store, tests)?),
            )]
            .into_iter()
            .collect(),
        ),
        ObligationAuthorityOperation::Budgets => Term::Map(
            [
                (
                    TermOrdKey(Term::symbol(":limits")),
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
                ),
                (
                    TermOrdKey(Term::symbol(":tests")),
                    Term::Vector(budget_observations(tests)),
                ),
            ]
            .into_iter()
            .collect(),
        ),
        ObligationAuthorityOperation::CapabilitiesDeclared => capability_inputs(modules, tests),
        ObligationAuthorityOperation::ConcurrencyReplay
        | ObligationAuthorityOperation::ReplayableTests => {
            return Err(authority_error(
                "replay operations require closed replay observations",
            ));
        }
        ObligationAuthorityOperation::Determinism => capability_inputs(modules, tests),
        ObligationAuthorityOperation::AiStyle | ObligationAuthorityOperation::Lint => {
            typecheck_inputs(modules)
        }
        ObligationAuthorityOperation::PropertyTests => {
            return Err(authority_error(
                "property tests require closed two-phase observations",
            ));
        }
        ObligationAuthorityOperation::Stage1Validation => {
            return Err(authority_error(
                "stage1 validation requires closed optimizer observations",
            ));
        }
        ObligationAuthorityOperation::Typecheck | ObligationAuthorityOperation::TypecheckStrict => {
            typecheck_inputs(modules)
        }
    };
    Ok(authority_request_term(operation, &manifest.name, inputs))
}

fn authority_request_term(
    operation: ObligationAuthorityOperation,
    package: &str,
    inputs: Term,
) -> Term {
    Term::Map(
        [
            (
                TermOrdKey(Term::symbol(":kind")),
                Term::Str("genesis/obligation-authority-request-v0.2".to_string()),
            ),
            (TermOrdKey(Term::symbol(":inputs")), inputs),
            (
                TermOrdKey(Term::symbol(":operation")),
                Term::symbol(operation.symbol()),
            ),
            (
                TermOrdKey(Term::symbol(":package")),
                Term::Str(package.to_string()),
            ),
            (TermOrdKey(Term::symbol(":v")), Term::Int(2.into())),
        ]
        .into_iter()
        .collect(),
    )
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

fn decode_authority_result(
    operation: ObligationAuthorityOperation,
    store: &EvidenceStore,
    manifest: &PackageManifest,
    modules: &[LoadedModule],
    tests: &[TestRun],
    replay_observations: &[ReplayObservation],
    request_hash: [u8; 32],
    term: Term,
) -> Result<ObligationResult, ObligationError> {
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
            ":request-h",
            ":v",
        ],
    )?;
    if string_field(map, ":kind", "obligation authority result")?
        != "genesis/obligation-authority-result-v0.2"
        || string_field(map, ":name", "obligation authority result")? != operation.obligation_name()
        || !matches!(map_field(map, ":operation"), Some(Term::Symbol(value)) if value == operation.symbol())
        || string_field(map, ":request-h", "obligation authority result")? != hex32(request_hash)
        || !matches!(map_field(map, ":v"), Some(Term::Int(value)) if value == &2.into())
    {
        return Err(authority_error("result identity mismatch"));
    }
    let ok = bool_field(map, ":ok", "obligation authority result")?;
    let errors = string_vector(
        required_field(map, ":errors", "obligation authority result")?,
        "obligation authority result :errors",
    )?;
    let report = required_field(map, ":report", "obligation authority result")?;
    let (report, side_artifacts) = match operation {
        ObligationAuthorityOperation::AiStyle | ObligationAuthorityOperation::Lint => {
            decode_artifact_transport(report)?
        }
        _ => (report.clone(), BTreeMap::new()),
    };
    let report = &report;
    match operation {
        ObligationAuthorityOperation::AiStyle => {
            validate_ai_style_report(report, manifest, modules, ok, &errors, &side_artifacts)?
        }
        ObligationAuthorityOperation::UnitTests => {
            validate_unit_report(report, manifest, tests, ok)?
        }
        ObligationAuthorityOperation::Budgets => {
            validate_budget_report(report, manifest, tests, ok, &errors)?
        }
        ObligationAuthorityOperation::CapabilitiesDeclared => {
            validate_capabilities_report(report, manifest, ok, &errors)?
        }
        ObligationAuthorityOperation::ConcurrencyReplay => validate_replay_report(
            operation,
            report,
            manifest,
            replay_observations,
            ok,
            &errors,
        )?,
        ObligationAuthorityOperation::Determinism => {
            validate_determinism_report(report, manifest, ok, &errors)?
        }
        ObligationAuthorityOperation::Lint => {
            validate_lint_report(report, manifest, modules, ok, &errors, &side_artifacts)?
        }
        ObligationAuthorityOperation::ReplayableTests => validate_replay_report(
            operation,
            report,
            manifest,
            replay_observations,
            ok,
            &errors,
        )?,
        ObligationAuthorityOperation::PropertyTests => {
            return Err(authority_error(
                "property tests require the two-phase authority decoder",
            ));
        }
        ObligationAuthorityOperation::Stage1Validation => {
            return Err(authority_error(
                "stage1 validation requires the optimizer-observation decoder",
            ));
        }
        ObligationAuthorityOperation::Typecheck => {
            validate_typecheck_obligation_report(report, modules, false, ok, &errors)?
        }
        ObligationAuthorityOperation::TypecheckStrict => {
            validate_typecheck_obligation_report(report, modules, true, ok, &errors)?
        }
    }
    for (hash, term) in side_artifacts {
        let stored = store.put_term(&term)?;
        if stored != hash {
            return Err(authority_error("side artifact changed during persistence"));
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

pub(super) fn evaluate_obligation_with_authority(
    operation: ObligationAuthorityOperation,
    store: &EvidenceStore,
    manifest: &PackageManifest,
    modules: &[LoadedModule],
    tests: &[TestRun],
    frontend: &CoreformFrontend,
    limits: KernelLimits,
) -> Result<ObligationResult, ObligationError> {
    let request = request_term(operation, store, manifest, modules, tests)?;
    let request_hash = hash_term(&request);
    let term = invoke_authority(request, frontend, limits)?;
    decode_authority_result(
        operation,
        store,
        manifest,
        modules,
        tests,
        &[],
        request_hash,
        term,
    )
}

#[cfg(test)]
#[path = "obligation_authority_tests.rs"]
mod tests;
