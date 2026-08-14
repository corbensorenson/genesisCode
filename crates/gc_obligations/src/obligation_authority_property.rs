#[derive(Clone)]
pub(super) struct PropertyAuthorityContext {
    configured: bool,
    default_cases: u64,
    suites: Term,
    bodies: BTreeMap<(usize, usize), Value>,
    expected_tests: Vec<PropertyPlanTest>,
    expected_plan_errors: Vec<String>,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub(super) struct PropertyPlanTest {
    pub(super) suite_index: usize,
    pub(super) entry_index: usize,
    pub(super) suite: String,
    pub(super) name: String,
    pub(super) cases: u64,
    pub(super) seeds: Vec<u64>,
}

#[derive(Clone, Debug)]
pub(super) struct PropertyAttemptObservation {
    pub(super) index: u64,
    pub(super) seed: u64,
    pub(super) kind: &'static str,
    pub(super) result: Term,
}

#[derive(Clone, Debug)]
pub(super) struct PropertyOutcomeObservation {
    pub(super) suite_index: usize,
    pub(super) entry_index: usize,
    pub(super) attempts: Vec<PropertyAttemptObservation>,
}

fn property_entry_observation(index: usize, key: &Term, value: &Value) -> (Term, Option<Value>) {
    let (key_kind, name) = match key {
        Term::Str(value) => (":string", Some(value.clone())),
        Term::Symbol(value) => (":symbol", Some(value.clone())),
        _ => (":other", None),
    };
    let mut body = None;
    let (entry_kind, body_state, cases_kind, cases, cases_display) = if is_callable_value(value) {
        body = Some(value.clone());
        (":callable", ":callable", ":absent", Term::Nil, Term::Nil)
    } else if let Some(map) = value_as_map(value) {
        let body_state = match map.get(&TermOrdKey(Term::symbol(":body"))) {
            None => ":missing",
            Some(value) if is_callable_value(value) => {
                body = Some(value.clone());
                ":callable"
            }
            Some(_) => ":other",
        };
        match map.get(&TermOrdKey(Term::symbol(":cases"))) {
            None => (":map", body_state, ":absent", Term::Nil, Term::Nil),
            Some(Value::Data(value)) => match value.as_ref() {
                Term::Int(value) => (
                    ":map",
                    body_state,
                    ":int",
                    Term::Int(value.clone()),
                    Term::Nil,
                ),
                _ => (
                    ":map",
                    body_state,
                    ":other",
                    Term::Nil,
                    Term::Str(Value::Data(value.clone()).debug_repr()),
                ),
            },
            Some(value) => (
                ":map",
                body_state,
                ":other",
                Term::Nil,
                Term::Str(value.debug_repr()),
            ),
        }
    } else {
        (":other", ":missing", ":absent", Term::Nil, Term::Nil)
    };
    (
        Term::Map(
            [
                (
                    TermOrdKey(Term::symbol(":body-state")),
                    Term::symbol(body_state),
                ),
                (TermOrdKey(Term::symbol(":cases")), cases),
                (
                    TermOrdKey(Term::symbol(":cases-display")),
                    cases_display,
                ),
                (
                    TermOrdKey(Term::symbol(":cases-kind")),
                    Term::symbol(cases_kind),
                ),
                (
                    TermOrdKey(Term::symbol(":entry-display")),
                    Term::Str(value.debug_repr()),
                ),
                (
                    TermOrdKey(Term::symbol(":entry-kind")),
                    Term::symbol(entry_kind),
                ),
                (
                    TermOrdKey(Term::symbol(":index")),
                    Term::Int(BigInt::from(index)),
                ),
                (
                    TermOrdKey(Term::symbol(":key-display")),
                    Term::Str(print_term(key)),
                ),
                (
                    TermOrdKey(Term::symbol(":key-kind")),
                    Term::symbol(key_kind),
                ),
                (
                    TermOrdKey(Term::symbol(":name")),
                    name.map(Term::Str).unwrap_or(Term::Nil),
                ),
            ]
            .into_iter()
            .collect(),
        ),
        body,
    )
}

pub(super) fn property_authority_context(
    pkg_dir: &Path,
    manifest: &PackageManifest,
    modules: &[LoadedModule],
    limits: KernelLimits,
) -> Result<PropertyAuthorityContext, ObligationError> {
    let configured = !manifest.property_tests.is_empty();
    let default_cases = manifest.property.cases_per_test.unwrap_or(64);
    if !configured {
        return Ok(PropertyAuthorityContext {
            configured,
            default_cases,
            suites: Term::Vector(Vec::new()),
            bodies: BTreeMap::new(),
            expected_tests: Vec::new(),
            expected_plan_errors: Vec::new(),
        });
    }
    let eval = eval_package_once(pkg_dir, manifest, modules, limits)?;
    let mut suites = Vec::with_capacity(manifest.property_tests.len());
    let mut bodies = BTreeMap::new();
    let mut expected_tests = Vec::new();
    let mut expected_plan_errors = Vec::new();
    for (suite_index, suite) in manifest.property_tests.iter().enumerate() {
        let (state, entries) = match eval.lookup_any(suite) {
            None => {
                expected_plan_errors.push(format!("missing property suite symbol {suite}"));
                (":missing", Vec::new())
            }
            Some(value) => match value_as_map(&value) {
                None => {
                    expected_plan_errors.push(format!("property suite {suite} must be a map"));
                    (":not-map", Vec::new())
                }
                Some(map) => {
                    let mut observations = Vec::new();
                    for (entry_index, (key, value)) in map.iter().enumerate() {
                        let (observation, body) =
                            property_entry_observation(entry_index, &key.0, value);
                        if let Some(body) = body {
                            bodies.insert((suite_index, entry_index), body);
                        }
                        let name = match &key.0 {
                            Term::Str(value) | Term::Symbol(value) => Some(value.clone()),
                            other => {
                                expected_plan_errors.push(format!(
                                    "property suite {suite}: key must be string/symbol, got {}",
                                    print_term(other)
                                ));
                                None
                            }
                        };
                        if let Some(name) = name {
                            match crate::obligation_exec::parse_property_entry(value, default_cases) {
                                Ok((_, cases)) => expected_tests.push(PropertyPlanTest {
                                    suite_index,
                                    entry_index,
                                    suite: suite.clone(),
                                    name: name.clone(),
                                    cases,
                                    seeds: (0..cases)
                                        .map(|index| {
                                            crate::obligation_exec::property_seed_for_case(
                                                &manifest.name,
                                                suite,
                                                &name,
                                                index,
                                            )
                                        })
                                        .collect(),
                                }),
                                Err(error) => expected_plan_errors.push(format!(
                                    "property suite {suite}::{name}: {}",
                                    property_parse_error_message(&error)
                                )),
                            }
                        }
                        observations.push(observation);
                    }
                    (":map", observations)
                }
            },
        };
        suites.push(Term::Map(
            [
                (
                    TermOrdKey(Term::symbol(":entries")),
                    Term::Vector(entries),
                ),
                (
                    TermOrdKey(Term::symbol(":index")),
                    Term::Int(BigInt::from(suite_index)),
                ),
                (
                    TermOrdKey(Term::symbol(":state")),
                    Term::symbol(state),
                ),
                (
                    TermOrdKey(Term::symbol(":suite")),
                    Term::symbol(suite.clone()),
                ),
            ]
            .into_iter()
            .collect(),
        ));
    }
    Ok(PropertyAuthorityContext {
        configured,
        default_cases,
        suites: Term::Vector(suites),
        bodies,
        expected_tests,
        expected_plan_errors,
    })
}

fn property_parse_error_message(error: &ObligationError) -> String {
    match error {
        ObligationError::Test(message) => message.clone(),
        other => other.to_string(),
    }
}

fn property_request_inputs(
    context: &PropertyAuthorityContext,
    phase: &str,
    outcomes: Option<Term>,
) -> Term {
    let mut fields = BTreeMap::from([
        (
            TermOrdKey(Term::symbol(":configured")),
            Term::Bool(context.configured),
        ),
        (
            TermOrdKey(Term::symbol(":default-cases")),
            Term::Int(BigInt::from(context.default_cases)),
        ),
        (
            TermOrdKey(Term::symbol(":phase")),
            Term::symbol(phase),
        ),
        (
            TermOrdKey(Term::symbol(":suites")),
            context.suites.clone(),
        ),
    ]);
    if let Some(outcomes) = outcomes {
        fields.insert(TermOrdKey(Term::symbol(":outcomes")), outcomes);
    }
    Term::Map(fields)
}

fn property_plan_test_term(test: &PropertyPlanTest) -> Term {
    Term::Map(
        [
            (
                TermOrdKey(Term::symbol(":cases")),
                Term::Int(BigInt::from(test.cases)),
            ),
            (
                TermOrdKey(Term::symbol(":entry-index")),
                Term::Int(BigInt::from(test.entry_index)),
            ),
            (
                TermOrdKey(Term::symbol(":name")),
                Term::Str(test.name.clone()),
            ),
            (
                TermOrdKey(Term::symbol(":seeds")),
                Term::Vector(
                    test.seeds
                        .iter()
                        .map(|seed| Term::Int(BigInt::from(*seed)))
                        .collect(),
                ),
            ),
            (
                TermOrdKey(Term::symbol(":suite")),
                Term::symbol(test.suite.clone()),
            ),
            (
                TermOrdKey(Term::symbol(":suite-index")),
                Term::Int(BigInt::from(test.suite_index)),
            ),
        ]
        .into_iter()
        .collect(),
    )
}

fn expected_property_plan_report(
    manifest: &PackageManifest,
    context: &PropertyAuthorityContext,
) -> Term {
    Term::Map(
        [
            (
                TermOrdKey(Term::symbol(":configured")),
                Term::Bool(context.configured),
            ),
            (
                TermOrdKey(Term::symbol(":default-cases")),
                Term::Int(BigInt::from(context.default_cases)),
            ),
            (
                TermOrdKey(Term::symbol(":errors")),
                Term::Vector(
                    context
                        .expected_plan_errors
                        .iter()
                        .cloned()
                        .map(Term::Str)
                        .collect(),
                ),
            ),
            (
                TermOrdKey(Term::symbol(":kind")),
                Term::Str("genesis/property-test-plan-v0.1".to_string()),
            ),
            (
                TermOrdKey(Term::symbol(":package")),
                Term::Str(manifest.name.clone()),
            ),
            (
                TermOrdKey(Term::symbol(":stop-rule")),
                Term::symbol(":first-non-pass"),
            ),
            (
                TermOrdKey(Term::symbol(":tests")),
                Term::Vector(
                    context
                        .expected_tests
                        .iter()
                        .map(property_plan_test_term)
                        .collect(),
                ),
            ),
        ]
        .into_iter()
        .collect(),
    )
}

fn validate_property_outer(
    term: &Term,
    request_hash: [u8; 32],
) -> Result<&BTreeMap<TermOrdKey, Term>, ObligationError> {
    let map = exact_map(
        term,
        "property authority result",
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
    if string_field(map, ":kind", "property authority result")?
        != "genesis/obligation-authority-result-v0.2"
        || string_field(map, ":name", "property authority result")?
            != "core/obligation::property-tests"
        || !matches!(map_field(map, ":operation"), Some(Term::Symbol(value)) if value == ":property-tests")
        || string_field(map, ":request-h", "property authority result")? != hex32(request_hash)
        || !matches!(map_field(map, ":v"), Some(Term::Int(value)) if value == &2.into())
    {
        return Err(authority_error("property result identity mismatch"));
    }
    Ok(map)
}

pub(super) fn property_authority_plan(
    manifest: &PackageManifest,
    context: &PropertyAuthorityContext,
    frontend: &CoreformFrontend,
    limits: KernelLimits,
) -> Result<Vec<PropertyPlanTest>, ObligationError> {
    let request = authority_request_term(
        ObligationAuthorityOperation::PropertyTests,
        &manifest.name,
        property_request_inputs(context, ":plan", None),
    );
    let request_hash = hash_term(&request);
    let result = invoke_authority(request, frontend, limits)?;
    decode_property_plan_result(manifest, context, request_hash, result)
}

fn decode_property_plan_result(
    manifest: &PackageManifest,
    context: &PropertyAuthorityContext,
    request_hash: [u8; 32],
    result: Term,
) -> Result<Vec<PropertyPlanTest>, ObligationError> {
    let map = validate_property_outer(&result, request_hash)?;
    let errors = string_vector(
        required_field(map, ":errors", "property plan result")?,
        "property plan result :errors",
    )?;
    let expected_ok = context.expected_plan_errors.is_empty();
    let report = required_field(map, ":report", "property plan result")?;
    if errors != context.expected_plan_errors
        || bool_field(map, ":ok", "property plan result")? != expected_ok
        || report != &expected_property_plan_report(manifest, context)
    {
        return Err(authority_error("property plan contradiction"));
    }
    let report = exact_map(
        report,
        "property plan report",
        &[
            ":configured",
            ":default-cases",
            ":errors",
            ":kind",
            ":package",
            ":stop-rule",
            ":tests",
        ],
    )?;
    let Some(Term::Vector(rows)) = map_field(report, ":tests") else {
        return Err(authority_error("property plan :tests must be a vector"));
    };
    rows.iter()
        .map(|row| {
            let row = exact_map(
                row,
                "property plan test",
                &[
                    ":cases",
                    ":entry-index",
                    ":name",
                    ":seeds",
                    ":suite",
                    ":suite-index",
                ],
            )?;
            let integer = |field: &str| -> Result<u64, ObligationError> {
                match map_field(row, field) {
                    Some(Term::Int(value)) => value.to_u64().ok_or_else(|| {
                        authority_error(format!("property plan {field} must fit u64"))
                    }),
                    _ => Err(authority_error(format!(
                        "property plan {field} must be int"
                    ))),
                }
            };
            let index = |field: &str| -> Result<usize, ObligationError> {
                usize::try_from(integer(field)?).map_err(|_| {
                    authority_error(format!("property plan {field} must fit usize"))
                })
            };
            let seeds = match map_field(row, ":seeds") {
                Some(Term::Vector(values)) => values
                    .iter()
                    .map(|value| match value {
                        Term::Int(value) => value
                            .to_u64()
                            .ok_or_else(|| authority_error("property seed must fit u64")),
                        _ => Err(authority_error("property seed must be int")),
                    })
                    .collect::<Result<Vec<_>, _>>()?,
                _ => return Err(authority_error("property plan :seeds must be a vector")),
            };
            Ok(PropertyPlanTest {
                suite_index: index(":suite-index")?,
                entry_index: index(":entry-index")?,
                suite: match map_field(row, ":suite") {
                    Some(Term::Symbol(value)) => value.clone(),
                    _ => return Err(authority_error("property plan :suite must be symbol")),
                },
                name: string_field(row, ":name", "property plan test")?,
                cases: integer(":cases")?,
                seeds,
            })
        })
        .collect()
}

pub(super) fn property_body(
    context: &PropertyAuthorityContext,
    test: &PropertyPlanTest,
) -> Result<Value, ObligationError> {
    context
        .bodies
        .get(&(test.suite_index, test.entry_index))
        .cloned()
        .ok_or_else(|| authority_error("authorized property plan references no callable body"))
}

fn property_outcomes_term(outcomes: &[PropertyOutcomeObservation]) -> Term {
    Term::Vector(
        outcomes
            .iter()
            .map(|outcome| {
                Term::Map(
                    [
                        (
                            TermOrdKey(Term::symbol(":attempts")),
                            Term::Vector(
                                outcome
                                    .attempts
                                    .iter()
                                    .map(|attempt| {
                                        Term::Map(
                                            [
                                                (
                                                    TermOrdKey(Term::symbol(":i")),
                                                    Term::Int(BigInt::from(attempt.index)),
                                                ),
                                                (
                                                    TermOrdKey(Term::symbol(":kind")),
                                                    Term::symbol(attempt.kind),
                                                ),
                                                (
                                                    TermOrdKey(Term::symbol(":result")),
                                                    attempt.result.clone(),
                                                ),
                                                (
                                                    TermOrdKey(Term::symbol(":seed")),
                                                    Term::Int(BigInt::from(attempt.seed)),
                                                ),
                                            ]
                                            .into_iter()
                                            .collect(),
                                        )
                                    })
                                    .collect(),
                            ),
                        ),
                        (
                            TermOrdKey(Term::symbol(":entry-index")),
                            Term::Int(BigInt::from(outcome.entry_index)),
                        ),
                        (
                            TermOrdKey(Term::symbol(":suite-index")),
                            Term::Int(BigInt::from(outcome.suite_index)),
                        ),
                    ]
                    .into_iter()
                    .collect(),
                )
            })
            .collect(),
    )
}
