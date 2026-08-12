fn expected_property_final(
    manifest: &PackageManifest,
    context: &PropertyAuthorityContext,
    outcomes: &[PropertyOutcomeObservation],
) -> Result<(bool, Vec<String>, Term), ObligationError> {
    if outcomes.len() != context.expected_tests.len() {
        return Err(authority_error("property outcome inventory length mismatch"));
    }
    if !context.configured {
        return Ok((
            true,
            Vec::new(),
            Term::Map(
                [
                    (
                        TermOrdKey(Term::symbol(":kind")),
                        Term::Str("genesis/property-tests-v0.2".to_string()),
                    ),
                    (TermOrdKey(Term::symbol(":note")), Term::Str("no property tests".to_string())),
                    (TermOrdKey(Term::symbol(":ok")), Term::Bool(true)),
                    (TermOrdKey(Term::symbol(":package")), Term::Str(manifest.name.clone())),
                ]
                .into_iter()
                .collect(),
            ),
        ));
    }
    let mut errors = context.expected_plan_errors.clone();
    let mut reports = Vec::with_capacity(outcomes.len());
    for (test, outcome) in context.expected_tests.iter().zip(outcomes) {
        if outcome.suite_index != test.suite_index || outcome.entry_index != test.entry_index {
            return Err(authority_error("property outcome identity mismatch"));
        }
        let failure = outcome.attempts.last().filter(|attempt| {
            attempt.kind != ":value" || attempt.result != Term::Bool(true)
        });
        let passed = failure.is_none() && outcome.attempts.len() == test.seeds.len();
        if !passed && failure.is_none() {
            return Err(authority_error("property outcome stopped before a failure"));
        }
        let mut report = BTreeMap::from([
            (TermOrdKey(Term::symbol(":cases")), Term::Int(BigInt::from(test.cases))),
            (TermOrdKey(Term::symbol(":name")), Term::Str(test.name.clone())),
            (TermOrdKey(Term::symbol(":ok")), Term::Bool(passed)),
            (
                TermOrdKey(Term::symbol(":seeds")),
                Term::Vector(test.seeds.iter().map(|seed| Term::Int(BigInt::from(*seed))).collect()),
            ),
            (TermOrdKey(Term::symbol(":suite")), Term::symbol(test.suite.clone())),
        ]);
        if let Some(failure) = failure {
            let result = match failure.kind {
                ":apply-error" => Term::Str(format!(
                    "apply failed: {}",
                    match &failure.result { Term::Str(value) => value.as_str(), _ => "" }
                )),
                ":effect-program" => Term::Str(
                    "effect program returned (property tests must be pure)".to_string(),
                ),
                _ => failure.result.clone(),
            };
            report.insert(
                TermOrdKey(Term::symbol(":first-failure")),
                Term::Map(
                    [
                        (TermOrdKey(Term::symbol(":i")), Term::Int(BigInt::from(failure.index))),
                        (TermOrdKey(Term::symbol(":result")), result),
                        (TermOrdKey(Term::symbol(":seed")), Term::Int(BigInt::from(failure.seed))),
                    ]
                    .into_iter()
                    .collect(),
                ),
            );
            errors.push(match failure.kind {
                ":apply-error" => format!(
                    "property test apply failed {}::{} at case {}: {}",
                    test.suite,
                    test.name,
                    failure.index,
                    match &failure.result { Term::Str(value) => value.as_str(), _ => "" }
                ),
                ":effect-program" => format!(
                    "property test {}::{} returned an effect program (must be pure)",
                    test.suite, test.name
                ),
                _ => format!(
                    "property test failed {}::{} at case {}",
                    test.suite, test.name, failure.index
                ),
            });
        }
        reports.push(Term::Map(report));
    }
    let ok = errors.is_empty();
    let mut report = BTreeMap::from([
        (
            TermOrdKey(Term::symbol(":config")),
            Term::Map(
                [(
                    TermOrdKey(Term::symbol(":cases-per-test")),
                    Term::Int(BigInt::from(context.default_cases)),
                )]
                .into_iter()
                .collect(),
            ),
        ),
        (
            TermOrdKey(Term::symbol(":kind")),
            Term::Str("genesis/property-tests-v0.2".to_string()),
        ),
        (TermOrdKey(Term::symbol(":ok")), Term::Bool(ok)),
        (TermOrdKey(Term::symbol(":package")), Term::Str(manifest.name.clone())),
        (TermOrdKey(Term::symbol(":tests")), Term::Vector(reports)),
    ]);
    if !errors.is_empty() {
        report.insert(
            TermOrdKey(Term::symbol(":errors")),
            Term::Vector(errors.iter().cloned().map(Term::Str).collect()),
        );
    }
    Ok((ok, errors, Term::Map(report)))
}

pub(super) fn property_authority_finalize(
    store: &EvidenceStore,
    manifest: &PackageManifest,
    context: &PropertyAuthorityContext,
    outcomes: &[PropertyOutcomeObservation],
    frontend: &CoreformFrontend,
    limits: KernelLimits,
) -> Result<ObligationResult, ObligationError> {
    let request = authority_request_term(
        ObligationAuthorityOperation::PropertyTests,
        &manifest.name,
        property_request_inputs(context, ":finalize", Some(property_outcomes_term(outcomes))),
    );
    let request_hash = hash_term(&request);
    let result = invoke_authority(request, frontend, limits)?;
    let map = validate_property_outer(&result, request_hash)?;
    let expected = expected_property_final(manifest, context, outcomes)?;
    let errors = string_vector(
        required_field(map, ":errors", "property final result")?,
        "property final result :errors",
    )?;
    if bool_field(map, ":ok", "property final result")? != expected.0
        || errors != expected.1
        || required_field(map, ":report", "property final result")? != &expected.2
    {
        return Err(authority_error("property final result contradiction"));
    }
    let artifact = store.put_term(&expected.2)?;
    Ok(ObligationResult {
        name: "core/obligation::property-tests".to_string(),
        ok: expected.0,
        artifact: Some(artifact),
        errors: expected.1,
    })
}
