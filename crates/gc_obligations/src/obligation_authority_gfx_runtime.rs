#[derive(Clone, Debug, Eq, PartialEq)]
pub(super) struct GfxRuntimePlanCase {
    pub(super) suite_index: usize,
    pub(super) entry_index: usize,
    pub(super) suite: String,
    pub(super) name: String,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub(super) struct GfxGoldenPlanCase {
    pub(super) identity: GfxRuntimePlanCase,
    pub(super) kind: &'static str,
    pub(super) expect_hash: String,
    pub(super) expect_png_hash: Option<String>,
    pub(super) pixel_width: u32,
    pub(super) pixel_height: u32,
}

#[derive(Clone, Debug)]
pub(super) struct GfxGoldenOutcomeObservation {
    pub(super) suite_index: usize,
    pub(super) entry_index: usize,
    pub(super) kind: &'static str,
    pub(super) result: Term,
    pub(super) render: Term,
}

#[derive(Clone, Debug)]
pub(super) struct GfxFrameBudgetOutcomeObservation {
    pub(super) suite_index: usize,
    pub(super) entry_index: usize,
    pub(super) kind: &'static str,
    pub(super) result: Term,
}

#[derive(Clone)]
pub(super) struct GfxGoldenAuthorityContext {
    pub(super) configured: bool,
    pub(super) suites: Term,
    pub(super) bodies: BTreeMap<(usize, usize), Value>,
    pub(super) expected_cases: Vec<GfxGoldenPlanCase>,
    pub(super) expected_errors: Vec<String>,
}

#[derive(Clone)]
pub(super) struct GfxFrameBudgetAuthorityContext {
    pub(super) configured: bool,
    pub(super) limits: Term,
    pub(super) suites: Term,
    pub(super) bodies: BTreeMap<(usize, usize), Value>,
    pub(super) expected_cases: Vec<GfxRuntimePlanCase>,
    pub(super) expected_errors: Vec<String>,
}

fn gfx_field_observation(value: Option<&Value>) -> Term {
    let (state, term, display) = match value {
        None => (":absent", Term::Nil, Term::Nil),
        Some(Value::Data(term)) => (
            ":data",
            term.as_ref().clone(),
            Term::Str(print_term(term.as_ref())),
        ),
        Some(value) => (":other", Term::Nil, Term::Str(value.debug_repr())),
    };
    Term::Map(
        [
            (
                TermOrdKey(Term::symbol(":display")),
                display,
            ),
            (TermOrdKey(Term::symbol(":state")), Term::symbol(state)),
            (TermOrdKey(Term::symbol(":value")), term),
        ]
        .into_iter()
        .collect(),
    )
}

fn gfx_key_observation(index: usize, key: &Term) -> (Vec<(TermOrdKey, Term)>, Option<String>) {
    let (kind, name) = match key {
        Term::Str(value) => (":string", Some(value.clone())),
        Term::Symbol(value) => (":symbol", Some(value.clone())),
        _ => (":other", None),
    };
    (
        vec![
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
                Term::symbol(kind),
            ),
            (
                TermOrdKey(Term::symbol(":name")),
                name.clone().map(Term::Str).unwrap_or(Term::Nil),
            ),
        ],
        name,
    )
}

fn gfx_golden_entry_observation(
    index: usize,
    key: &Term,
    value: &Value,
) -> (Term, Option<Value>) {
    let (mut fields, _) = gfx_key_observation(index, key);
    let mut body = None;
    let (entry_state, body_state, map) = match value_as_map(value) {
        Some(map) => {
            let body_state = match map.get(&TermOrdKey(Term::symbol(":body"))) {
                None => ":missing",
                Some(value) if is_callable_value(value) => {
                    body = Some(value.clone());
                    ":callable"
                }
                Some(_) => ":other",
            };
            (":map", body_state, Some(map))
        }
        None => (":other", ":missing", None),
    };
    fields.extend([
        (
            TermOrdKey(Term::symbol(":body-state")),
            Term::symbol(body_state),
        ),
        (
            TermOrdKey(Term::symbol(":entry-display")),
            Term::Str(value.debug_repr()),
        ),
        (
            TermOrdKey(Term::symbol(":entry-state")),
            Term::symbol(entry_state),
        ),
    ]);
    for (output, source) in [
        (":expect-h", ":expect-h"),
        (":expect-png-h", ":expect-png-h"),
        (":kind", ":kind"),
        (":pixel-height", ":pixel-height"),
        (":pixel-width", ":pixel-width"),
    ] {
        fields.push((
            TermOrdKey(Term::symbol(output)),
            gfx_field_observation(
                map.and_then(|map| map.get(&TermOrdKey(Term::symbol(source)))),
            ),
        ));
    }
    (Term::Map(fields.into_iter().collect()), body)
}

fn gfx_frame_entry_observation(
    index: usize,
    key: &Term,
    value: &Value,
) -> (Term, Option<Value>) {
    let (mut fields, _) = gfx_key_observation(index, key);
    let mut body = None;
    let (entry_state, body_state) = if is_callable_value(value) {
        body = Some(value.clone());
        (":callable", ":callable")
    } else if let Some(map) = value_as_map(value) {
        match map.get(&TermOrdKey(Term::symbol(":body"))) {
            None => (":map", ":missing"),
            Some(value) if is_callable_value(value) => {
                body = Some(value.clone());
                (":map", ":callable")
            }
            Some(_) => (":map", ":other"),
        }
    } else {
        (":other", ":missing")
    };
    fields.extend([
        (
            TermOrdKey(Term::symbol(":body-state")),
            Term::symbol(body_state),
        ),
        (
            TermOrdKey(Term::symbol(":entry-display")),
            Term::Str(value.debug_repr()),
        ),
        (
            TermOrdKey(Term::symbol(":entry-state")),
            Term::symbol(entry_state),
        ),
    ]);
    (Term::Map(fields.into_iter().collect()), body)
}

fn gfx_suite_term(index: usize, suite: &str, state: &str, entries: Vec<Term>) -> Term {
    Term::Map(
        [
            (
                TermOrdKey(Term::symbol(":entries")),
                Term::Vector(entries),
            ),
            (
                TermOrdKey(Term::symbol(":index")),
                Term::Int(BigInt::from(index)),
            ),
            (
                TermOrdKey(Term::symbol(":state")),
                Term::symbol(state),
            ),
            (
                TermOrdKey(Term::symbol(":suite")),
                Term::symbol(suite.to_string()),
            ),
        ]
        .into_iter()
        .collect(),
    )
}

pub(super) fn gfx_golden_authority_context(
    pkg_dir: &Path,
    manifest: &PackageManifest,
    modules: &[LoadedModule],
    limits: KernelLimits,
) -> Result<GfxGoldenAuthorityContext, ObligationError> {
    let configured = !manifest.gfx.golden_tests.is_empty();
    let mut suites = Vec::new();
    let mut bodies = BTreeMap::new();
    let mut expected_cases = Vec::new();
    let mut expected_errors = Vec::new();
    if !configured {
        expected_errors.push(
            "gfx.golden_tests is empty; configure suite symbols for core/obligation::gfx-golden-images"
                .to_string(),
        );
        return Ok(GfxGoldenAuthorityContext {
            configured,
            suites: Term::Vector(suites),
            bodies,
            expected_cases,
            expected_errors,
        });
    }
    let eval = eval_package_once(pkg_dir, manifest, modules, limits)?;
    for (suite_index, suite) in manifest.gfx.golden_tests.iter().enumerate() {
        let (state, entries) = match eval.lookup_any(suite) {
            None => {
                expected_errors.push(format!("missing gfx golden suite symbol {suite}"));
                (":missing", Vec::new())
            }
            Some(value) => match value_as_map(&value) {
                None => {
                    expected_errors.push(format!("gfx golden suite {suite} must be a map"));
                    (":not-map", Vec::new())
                }
                Some(map) => {
                    let mut entries = Vec::new();
                    for (entry_index, (key, value)) in map.iter().enumerate() {
                        let (observation, body) =
                            gfx_golden_entry_observation(entry_index, &key.0, value);
                        if let Some(body) = body {
                            bodies.insert((suite_index, entry_index), body);
                        }
                        match &key.0 {
                            Term::Str(name) | Term::Symbol(name) => {
                                match crate::obligation_gfx::helpers::parse_gfx_golden_entry(value) {
                                    Ok(parsed) => expected_cases.push(GfxGoldenPlanCase {
                                        identity: GfxRuntimePlanCase {
                                            suite_index,
                                            entry_index,
                                            suite: suite.clone(),
                                            name: name.clone(),
                                        },
                                        kind: match parsed.kind {
                                            crate::obligation_gfx::helpers::GfxGoldenKind::FrameGraph => ":frame-graph",
                                            crate::obligation_gfx::helpers::GfxGoldenKind::Scene => ":scene",
                                        },
                                        expect_hash: parsed.expect_hash,
                                        expect_png_hash: parsed.expect_png_hash,
                                        pixel_width: parsed.pixel_width,
                                        pixel_height: parsed.pixel_height,
                                    }),
                                    Err(error) => expected_errors.push(format!(
                                        "gfx golden suite {suite}::{name}: {}",
                                        gfx_test_error_message(&error)
                                    )),
                                }
                            }
                            other => expected_errors.push(format!(
                                "gfx golden suite {suite}: key must be string/symbol, got {}",
                                print_term(other)
                            )),
                        }
                        entries.push(observation);
                    }
                    (":map", entries)
                }
            },
        };
        suites.push(gfx_suite_term(suite_index, suite, state, entries));
    }
    Ok(GfxGoldenAuthorityContext {
        configured,
        suites: Term::Vector(suites),
        bodies,
        expected_cases,
        expected_errors,
    })
}

fn gfx_limits_term(manifest: &PackageManifest) -> Term {
    let mut limits = BTreeMap::new();
    for (name, value) in [
        (":max-render-passes-per-frame", manifest.gfx.max_render_passes_per_frame),
        (":max-compute-passes-per-frame", manifest.gfx.max_compute_passes_per_frame),
        (":max-draw-commands-per-frame", manifest.gfx.max_draw_commands_per_frame),
        (":max-compute-commands-per-frame", manifest.gfx.max_compute_commands_per_frame),
        (":max-frame-graph-bytes", manifest.gfx.max_frame_graph_bytes),
        (":max-frame-time-ms", manifest.gfx.max_frame_time_ms),
    ] {
        if let Some(value) = value {
            limits.insert(
                TermOrdKey(Term::symbol(name)),
                Term::Int(BigInt::from(value)),
            );
        }
    }
    Term::Map(limits)
}

pub(super) fn gfx_frame_budget_authority_context(
    pkg_dir: &Path,
    manifest: &PackageManifest,
    modules: &[LoadedModule],
    limits: KernelLimits,
) -> Result<GfxFrameBudgetAuthorityContext, ObligationError> {
    let configured = !manifest.gfx.frame_budget_tests.is_empty();
    let limits_term = gfx_limits_term(manifest);
    let mut suites = Vec::new();
    let mut bodies = BTreeMap::new();
    let mut expected_cases = Vec::new();
    let mut expected_errors = Vec::new();
    if !configured {
        expected_errors.push(
            "gfx.frame_budget_tests is empty; configure suite symbols for core/obligation::gfx-frame-budgets"
                .to_string(),
        );
    }
    if matches!(&limits_term, Term::Map(map) if map.is_empty()) {
        expected_errors.push(
            "gfx frame budget obligation requires at least one configured gfx.* budget limit"
                .to_string(),
        );
    }
    if configured {
        let eval = eval_package_once(pkg_dir, manifest, modules, limits)?;
        for (suite_index, suite) in manifest.gfx.frame_budget_tests.iter().enumerate() {
            let (state, entries) = match eval.lookup_any(suite) {
                None => {
                    expected_errors.push(format!("missing gfx frame-budget suite symbol {suite}"));
                    (":missing", Vec::new())
                }
                Some(value) => match value_as_map(&value) {
                    None => {
                        expected_errors.push(format!("gfx frame-budget suite {suite} must be a map"));
                        (":not-map", Vec::new())
                    }
                    Some(map) => {
                        let mut entries = Vec::new();
                        for (entry_index, (key, value)) in map.iter().enumerate() {
                            let (observation, body) =
                                gfx_frame_entry_observation(entry_index, &key.0, value);
                            if let Some(body) = body {
                                bodies.insert((suite_index, entry_index), body);
                            }
                            match &key.0 {
                                Term::Str(name) | Term::Symbol(name) => {
                                    match crate::obligation_gfx::helpers::parse_gfx_frame_budget_entry(value) {
                                        Ok(_) => expected_cases.push(GfxRuntimePlanCase {
                                            suite_index,
                                            entry_index,
                                            suite: suite.clone(),
                                            name: name.clone(),
                                        }),
                                        Err(error) => expected_errors.push(format!(
                                            "gfx frame-budget suite {suite}::{name}: {}",
                                            gfx_test_error_message(&error)
                                        )),
                                    }
                                }
                                other => expected_errors.push(format!(
                                    "gfx frame-budget suite {suite}: key must be string/symbol, got {}",
                                    print_term(other)
                                )),
                            }
                            entries.push(observation);
                        }
                        (":map", entries)
                    }
                },
            };
            suites.push(gfx_suite_term(suite_index, suite, state, entries));
        }
    }
    Ok(GfxFrameBudgetAuthorityContext {
        configured,
        limits: limits_term,
        suites: Term::Vector(suites),
        bodies,
        expected_cases,
        expected_errors,
    })
}

fn gfx_test_error_message(error: &ObligationError) -> String {
    match error {
        ObligationError::Test(message) => message.clone(),
        other => other.to_string(),
    }
}

fn gfx_runtime_request_inputs(
    configured: bool,
    suites: &Term,
    limits: Option<&Term>,
    phase: &str,
    outcomes: Option<Term>,
) -> Term {
    let mut fields = BTreeMap::from([
        (
            TermOrdKey(Term::symbol(":configured")),
            Term::Bool(configured),
        ),
        (
            TermOrdKey(Term::symbol(":phase")),
            Term::symbol(phase),
        ),
        (TermOrdKey(Term::symbol(":suites")), suites.clone()),
    ]);
    if let Some(limits) = limits {
        fields.insert(TermOrdKey(Term::symbol(":limits")), limits.clone());
    }
    if let Some(outcomes) = outcomes {
        fields.insert(TermOrdKey(Term::symbol(":outcomes")), outcomes);
    }
    Term::Map(fields)
}

fn gfx_runtime_plan_case_term(case: &GfxRuntimePlanCase) -> Term {
    Term::Map(
        [
            (
                TermOrdKey(Term::symbol(":entry-index")),
                Term::Int(BigInt::from(case.entry_index)),
            ),
            (
                TermOrdKey(Term::symbol(":name")),
                Term::Str(case.name.clone()),
            ),
            (
                TermOrdKey(Term::symbol(":suite")),
                Term::symbol(case.suite.clone()),
            ),
            (
                TermOrdKey(Term::symbol(":suite-index")),
                Term::Int(BigInt::from(case.suite_index)),
            ),
        ]
        .into_iter()
        .collect(),
    )
}

fn gfx_golden_plan_case_term(case: &GfxGoldenPlanCase) -> Term {
    let Term::Map(mut fields) = gfx_runtime_plan_case_term(&case.identity) else {
        return Term::Nil;
    };
    fields.extend([
        (
            TermOrdKey(Term::symbol(":expect-h")),
            Term::Str(case.expect_hash.clone()),
        ),
        (
            TermOrdKey(Term::symbol(":expect-png-h")),
            case.expect_png_hash
                .as_ref()
                .map(|value| Term::Str(value.clone()))
                .unwrap_or(Term::Nil),
        ),
        (
            TermOrdKey(Term::symbol(":kind")),
            Term::symbol(case.kind),
        ),
        (
            TermOrdKey(Term::symbol(":pixel-height")),
            Term::Int(BigInt::from(case.pixel_height)),
        ),
        (
            TermOrdKey(Term::symbol(":pixel-width")),
            Term::Int(BigInt::from(case.pixel_width)),
        ),
    ]);
    Term::Map(fields)
}

fn expected_gfx_plan_report(
    operation: ObligationAuthorityOperation,
    package: &str,
    configured: bool,
    limits: Option<&Term>,
    tests: Vec<Term>,
    errors: &[String],
) -> Term {
    let mut fields = BTreeMap::from([
        (
            TermOrdKey(Term::symbol(":configured")),
            Term::Bool(configured),
        ),
        (
            TermOrdKey(Term::symbol(":errors")),
            Term::Vector(errors.iter().cloned().map(Term::Str).collect()),
        ),
        (
            TermOrdKey(Term::symbol(":kind")),
            Term::Str(match operation {
                ObligationAuthorityOperation::GfxGoldenImages => {
                    "genesis/gfx-golden-plan-v0.1".to_string()
                }
                ObligationAuthorityOperation::GfxFrameBudgets => {
                    "genesis/gfx-frame-budget-plan-v0.1".to_string()
                }
                _ => String::new(),
            }),
        ),
        (
            TermOrdKey(Term::symbol(":package")),
            Term::Str(package.to_string()),
        ),
        (TermOrdKey(Term::symbol(":tests")), Term::Vector(tests)),
    ]);
    if let Some(limits) = limits {
        fields.insert(TermOrdKey(Term::symbol(":limits")), limits.clone());
    }
    Term::Map(fields)
}

fn validate_gfx_runtime_outer<'a>(
    operation: ObligationAuthorityOperation,
    term: &'a Term,
    request_hash: [u8; 32],
) -> Result<&'a BTreeMap<TermOrdKey, Term>, ObligationError> {
    let map = exact_map(
        term,
        "gfx runtime authority result",
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
    if string_field(map, ":kind", "gfx runtime authority result")?
        != "genesis/obligation-authority-result-v0.2"
        || string_field(map, ":name", "gfx runtime authority result")?
            != operation.obligation_name()
        || !matches!(map_field(map, ":operation"), Some(Term::Symbol(value)) if value == operation.symbol())
        || string_field(map, ":request-h", "gfx runtime authority result")?
            != hex32(request_hash)
        || !matches!(map_field(map, ":v"), Some(Term::Int(value)) if value == &2.into())
    {
        return Err(authority_error("gfx runtime result identity mismatch"));
    }
    Ok(map)
}

pub(super) fn gfx_golden_authority_plan(
    manifest: &PackageManifest,
    context: &GfxGoldenAuthorityContext,
    frontend: &CoreformFrontend,
    limits: KernelLimits,
) -> Result<Vec<GfxGoldenPlanCase>, ObligationError> {
    let request = authority_request_term(
        ObligationAuthorityOperation::GfxGoldenImages,
        &manifest.name,
        gfx_runtime_request_inputs(context.configured, &context.suites, None, ":plan", None),
    );
    let request_hash = hash_term(&request);
    let result = invoke_authority(request, frontend, limits)?;
    let map = validate_gfx_runtime_outer(
        ObligationAuthorityOperation::GfxGoldenImages,
        &result,
        request_hash,
    )?;
    let expected_report = expected_gfx_plan_report(
        ObligationAuthorityOperation::GfxGoldenImages,
        &manifest.name,
        context.configured,
        None,
        context
            .expected_cases
            .iter()
            .map(gfx_golden_plan_case_term)
            .collect(),
        &context.expected_errors,
    );
    if bool_field(map, ":ok", "gfx golden plan result")?
        != context.expected_errors.is_empty()
        || string_vector(
            required_field(map, ":errors", "gfx golden plan result")?,
            "gfx golden plan result :errors",
        )? != context.expected_errors
        || required_field(map, ":report", "gfx golden plan result")? != &expected_report
    {
        return Err(authority_error(format!(
            "gfx golden plan contradiction: expected {}, got {}",
            print_term(&expected_report),
            print_term(required_field(map, ":report", "gfx golden plan result")?)
        )));
    }
    Ok(context.expected_cases.clone())
}

pub(super) fn gfx_frame_budget_authority_plan(
    manifest: &PackageManifest,
    context: &GfxFrameBudgetAuthorityContext,
    frontend: &CoreformFrontend,
    limits: KernelLimits,
) -> Result<Vec<GfxRuntimePlanCase>, ObligationError> {
    let request = authority_request_term(
        ObligationAuthorityOperation::GfxFrameBudgets,
        &manifest.name,
        gfx_runtime_request_inputs(
            context.configured,
            &context.suites,
            Some(&context.limits),
            ":plan",
            None,
        ),
    );
    let request_hash = hash_term(&request);
    let result = invoke_authority(request, frontend, limits)?;
    let map = validate_gfx_runtime_outer(
        ObligationAuthorityOperation::GfxFrameBudgets,
        &result,
        request_hash,
    )?;
    let expected_report = expected_gfx_plan_report(
        ObligationAuthorityOperation::GfxFrameBudgets,
        &manifest.name,
        context.configured,
        Some(&context.limits),
        context
            .expected_cases
            .iter()
            .map(gfx_runtime_plan_case_term)
            .collect(),
        &context.expected_errors,
    );
    if bool_field(map, ":ok", "gfx frame plan result")?
        != context.expected_errors.is_empty()
        || string_vector(
            required_field(map, ":errors", "gfx frame plan result")?,
            "gfx frame plan result :errors",
        )? != context.expected_errors
        || required_field(map, ":report", "gfx frame plan result")? != &expected_report
    {
        return Err(authority_error(format!(
            "gfx frame budget plan contradiction: expected {}, got {}",
            print_term(&expected_report),
            print_term(required_field(map, ":report", "gfx frame plan result")?)
        )));
    }
    Ok(context.expected_cases.clone())
}

fn gfx_runtime_body(
    bodies: &BTreeMap<(usize, usize), Value>,
    case: &GfxRuntimePlanCase,
) -> Result<Value, ObligationError> {
    bodies
        .get(&(case.suite_index, case.entry_index))
        .cloned()
        .ok_or_else(|| authority_error("authorized gfx plan references no callable body"))
}
