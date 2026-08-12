fn gfx_outcome_result(
    body: Value,
    limits: KernelLimits,
) -> GfxFrameBudgetOutcomeObservation {
    let mut ctx = mk_eval_ctx(limits);
    match body.apply(&mut ctx, Value::data(Term::Nil)) {
        Err(error) => GfxFrameBudgetOutcomeObservation {
            suite_index: 0,
            entry_index: 0,
            kind: ":apply-error",
            result: Term::Str(error.to_string()),
        },
        Ok(Value::EffectProgram(_)) => GfxFrameBudgetOutcomeObservation {
            suite_index: 0,
            entry_index: 0,
            kind: ":effect-program",
            result: Term::Nil,
        },
        Ok(value)
            if ctx.protocol.is_some_and(
                |protocol| matches!(value, Value::Sealed { token, .. } if token == protocol.error),
            ) =>
        {
            GfxFrameBudgetOutcomeObservation {
                suite_index: 0,
                entry_index: 0,
                kind: ":sealed-error",
                result: Term::Nil,
            }
        }
        Ok(value) => GfxFrameBudgetOutcomeObservation {
            suite_index: 0,
            entry_index: 0,
            kind: ":value",
            result: value.to_term_for_log(ctx.protocol.map(|protocol| protocol.error)),
        },
    }
}

fn gfx_golden_outcomes_term(outcomes: &[GfxGoldenOutcomeObservation]) -> Term {
    Term::Vector(
        outcomes
            .iter()
            .map(|outcome| {
                Term::Map(
                    [
                        (
                            TermOrdKey(Term::symbol(":entry-index")),
                            Term::Int(BigInt::from(outcome.entry_index)),
                        ),
                        (
                            TermOrdKey(Term::symbol(":kind")),
                            Term::symbol(outcome.kind),
                        ),
                        (
                            TermOrdKey(Term::symbol(":render")),
                            outcome.render.clone(),
                        ),
                        (
                            TermOrdKey(Term::symbol(":result")),
                            outcome.result.clone(),
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

fn gfx_frame_outcomes_term(outcomes: &[GfxFrameBudgetOutcomeObservation]) -> Term {
    Term::Vector(
        outcomes
            .iter()
            .map(|outcome| {
                Term::Map(
                    [
                        (
                            TermOrdKey(Term::symbol(":entry-index")),
                            Term::Int(BigInt::from(outcome.entry_index)),
                        ),
                        (
                            TermOrdKey(Term::symbol(":kind")),
                            Term::symbol(outcome.kind),
                        ),
                        (
                            TermOrdKey(Term::symbol(":result")),
                            outcome.result.clone(),
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

fn gfx_runtime_error(kind: &str, result: &Term, golden: bool) -> Result<Option<String>, ObligationError> {
    match kind {
        ":value" => Ok(None),
        ":apply-error" => match result {
            Term::Str(message) => Ok(Some(format!("apply failed: {message}"))),
            _ => Err(authority_error("gfx apply-error outcome must carry a string")),
        },
        ":effect-program" if result == &Term::Nil => Ok(Some(if golden {
            "effect program returned; gfx golden tests must return pure frame/scene data".to_string()
        } else {
            "effect program returned; gfx frame budgets must return pure frame data".to_string()
        })),
        ":sealed-error" if result == &Term::Nil => Ok(Some(if golden {
            "sealed ERROR returned by golden test body".to_string()
        } else {
            "sealed ERROR returned by frame budget test body".to_string()
        })),
        ":effect-program" => Err(authority_error("gfx effect-program outcome must carry nil")),
        ":sealed-error" => Err(authority_error("gfx sealed-error outcome must carry nil")),
        _ => Err(authority_error("unknown gfx outcome kind")),
    }
}

fn gfx_case_error(case: &GfxRuntimePlanCase, errors: &[String], label: &str) -> String {
    let detail = if errors.is_empty() {
        label.to_string()
    } else {
        errors.join("; ")
    };
    format!("{}::{}: {detail}", case.suite, case.name)
}

fn expected_gfx_golden_final(
    manifest: &PackageManifest,
    context: &GfxGoldenAuthorityContext,
    outcomes: &[GfxGoldenOutcomeObservation],
) -> Result<(bool, Vec<String>, Term), ObligationError> {
    if outcomes.len() != context.expected_cases.len() {
        return Err(authority_error("gfx golden outcome inventory length mismatch"));
    }
    let mut errors = context.expected_errors.clone();
    let mut cases = Vec::with_capacity(outcomes.len());
    for (test, outcome) in context.expected_cases.iter().zip(outcomes) {
        if outcome.suite_index != test.identity.suite_index
            || outcome.entry_index != test.identity.entry_index
        {
            return Err(authority_error("gfx golden outcome identity mismatch"));
        }
        let mut runtime_error = gfx_runtime_error(outcome.kind, &outcome.result, true)?;
        let mut actual_hash = Term::Nil;
        let mut actual_png_hash = Term::Nil;
        if runtime_error.is_none() {
            let target = if test.kind == ":frame-graph" {
                crate::obligation_gfx::helpers::extract_frame_graph_term(&outcome.result)
            } else {
                crate::obligation_gfx::helpers::extract_scene_term(&outcome.result)
            };
            if let Some(target) = target {
                actual_hash = Term::Str(hex32(hash_term(target)));
                if let Some(_) = &test.expect_png_hash {
                    let render = exact_map(
                        &outcome.render,
                        "gfx renderer observation",
                        &[":error", ":frame-h", ":pixel-height", ":pixel-width", ":png-h"],
                    )?;
                    if string_field(render, ":frame-h", "gfx renderer observation")?
                        != hex32(hash_term(target))
                        || !matches!(map_field(render, ":pixel-width"), Some(Term::Int(value)) if value == &BigInt::from(test.pixel_width))
                        || !matches!(map_field(render, ":pixel-height"), Some(Term::Int(value)) if value == &BigInt::from(test.pixel_height))
                    {
                        return Err(authority_error("gfx renderer binding contradiction"));
                    }
                    match map_field(render, ":error") {
                        Some(Term::Nil) => match map_field(render, ":png-h") {
                            Some(Term::Str(value)) => actual_png_hash = Term::Str(value.clone()),
                            _ => return Err(authority_error("renderer success must carry :png-h")),
                        },
                        Some(Term::Str(message)) => {
                            if !matches!(map_field(render, ":png-h"), Some(Term::Nil)) {
                                return Err(authority_error("renderer failure must not carry :png-h"));
                            }
                            runtime_error = Some(format!("headless render failed: {message}"));
                        }
                        _ => return Err(authority_error("renderer :error must be string or nil")),
                    }
                } else if outcome.render != Term::Nil {
                    return Err(authority_error("unexpected renderer observation"));
                }
            } else {
                if outcome.render != Term::Nil {
                    return Err(authority_error("invalid gfx output cannot carry renderer observation"));
                }
                runtime_error = Some(if test.kind == ":frame-graph" {
                    "expected frame-graph output".to_string()
                } else {
                    "expected scene output".to_string()
                });
            }
        } else if outcome.render != Term::Nil {
            return Err(authority_error("failed gfx body cannot carry renderer observation"));
        }
        let expect_png = test
            .expect_png_hash
            .as_ref()
            .map(|value| Term::Str(value.clone()))
            .unwrap_or(Term::Nil);
        let case_errors = if let Some(message) = &runtime_error {
            vec![message.clone()]
        } else {
            let mut values = Vec::new();
            if actual_hash != Term::Str(test.expect_hash.clone()) {
                values.push("golden hash mismatch".to_string());
            }
            if test.expect_png_hash.is_some() && actual_png_hash != expect_png {
                values.push("golden png hash mismatch".to_string());
            }
            values
        };
        let case_ok = case_errors.is_empty();
        let case = Term::Map(
            [
                (TermOrdKey(Term::symbol(":actual-h")), actual_hash),
                (TermOrdKey(Term::symbol(":actual-png-h")), actual_png_hash),
                (
                    TermOrdKey(Term::symbol(":error")),
                    runtime_error.clone().map(Term::Str).unwrap_or(Term::Nil),
                ),
                (
                    TermOrdKey(Term::symbol(":errors")),
                    Term::Vector(case_errors.iter().cloned().map(Term::Str).collect()),
                ),
                (
                    TermOrdKey(Term::symbol(":expect-h")),
                    Term::Str(test.expect_hash.clone()),
                ),
                (TermOrdKey(Term::symbol(":expect-png-h")), expect_png),
                (TermOrdKey(Term::symbol(":kind")), Term::symbol(test.kind)),
                (
                    TermOrdKey(Term::symbol(":name")),
                    Term::Str(test.identity.name.clone()),
                ),
                (TermOrdKey(Term::symbol(":ok")), Term::Bool(case_ok)),
                (
                    TermOrdKey(Term::symbol(":pixel-height")),
                    Term::Int(BigInt::from(test.pixel_height)),
                ),
                (
                    TermOrdKey(Term::symbol(":pixel-width")),
                    Term::Int(BigInt::from(test.pixel_width)),
                ),
                (
                    TermOrdKey(Term::symbol(":suite")),
                    Term::symbol(test.identity.suite.clone()),
                ),
            ]
            .into_iter()
            .collect(),
        );
        if !case_ok {
            errors.push(gfx_case_error(&test.identity, &case_errors, "golden case failed"));
        }
        cases.push(case);
    }
    let ok = errors.is_empty();
    let report = Term::Map(
        [
            (TermOrdKey(Term::symbol(":cases")), Term::Vector(cases)),
            (
                TermOrdKey(Term::symbol(":errors")),
                Term::Vector(errors.iter().cloned().map(Term::Str).collect()),
            ),
            (
                TermOrdKey(Term::symbol(":kind")),
                Term::Str("genesis/gfx-golden-images-v0.2".to_string()),
            ),
            (TermOrdKey(Term::symbol(":ok")), Term::Bool(ok)),
            (
                TermOrdKey(Term::symbol(":package")),
                Term::Str(manifest.name.clone()),
            ),
        ]
        .into_iter()
        .collect(),
    );
    Ok((ok, errors, report))
}

fn gfx_limit(limits: &Term, key: &str) -> Option<u64> {
    let Term::Map(map) = limits else { return None };
    match map.get(&TermOrdKey(Term::symbol(key))) {
        Some(Term::Int(value)) => value.to_u64(),
        _ => None,
    }
}

fn expected_gfx_frame_final(
    manifest: &PackageManifest,
    context: &GfxFrameBudgetAuthorityContext,
    outcomes: &[GfxFrameBudgetOutcomeObservation],
) -> Result<(bool, Vec<String>, Term), ObligationError> {
    if outcomes.len() != context.expected_cases.len() {
        return Err(authority_error("gfx frame outcome inventory length mismatch"));
    }
    let mut errors = context.expected_errors.clone();
    let mut cases = Vec::with_capacity(outcomes.len());
    for (test, outcome) in context.expected_cases.iter().zip(outcomes) {
        if outcome.suite_index != test.suite_index || outcome.entry_index != test.entry_index {
            return Err(authority_error("gfx frame outcome identity mismatch"));
        }
        let mut runtime_error = gfx_runtime_error(outcome.kind, &outcome.result, false)?;
        let mut metrics = None;
        let mut frame_time = None;
        if runtime_error.is_none() {
            match crate::obligation_gfx::helpers::extract_frame_graph_and_time(&outcome.result) {
                Ok((frame, time)) => match crate::obligation_gfx::helpers::frame_graph_metrics(frame) {
                    Ok(value) => {
                        metrics = Some(value);
                        frame_time = time;
                    }
                    Err(error) => runtime_error = Some(error),
                },
                Err(error) => runtime_error = Some(error),
            }
        }
        let metric = |selector: fn(&crate::obligation_gfx::helpers::FrameMetrics) -> u64| {
            metrics.as_ref().map(selector)
        };
        let render_passes = metric(|value| value.render_passes);
        let compute_passes = metric(|value| value.compute_passes);
        let draw_commands = metric(|value| value.draw_commands);
        let compute_commands = metric(|value| value.compute_commands);
        let frame_bytes = metric(|value| value.frame_graph_bytes);
        let mut case_errors = runtime_error.iter().cloned().collect::<Vec<_>>();
        for (observed, key, message) in [
            (render_passes, ":max-render-passes-per-frame", "render passes exceed max"),
            (compute_passes, ":max-compute-passes-per-frame", "compute passes exceed max"),
            (draw_commands, ":max-draw-commands-per-frame", "draw commands exceed max"),
            (compute_commands, ":max-compute-commands-per-frame", "compute commands exceed max"),
            (frame_bytes, ":max-frame-graph-bytes", "frame graph bytes exceed max"),
        ] {
            if let (Some(observed), Some(limit)) = (observed, gfx_limit(&context.limits, key))
                && observed > limit
            {
                case_errors.push(message.to_string());
            }
        }
        if let Some(limit) = gfx_limit(&context.limits, ":max-frame-time-ms")
            && frame_time.is_none_or(|observed| observed > limit)
        {
            case_errors.push("frame time exceeds max or is missing".to_string());
        }
        let case_ok = case_errors.is_empty();
        let int_or_nil = |value: Option<u64>| {
            value
                .map(|value| Term::Int(BigInt::from(value)))
                .unwrap_or(Term::Nil)
        };
        let case = Term::Map(
            [
                (TermOrdKey(Term::symbol(":compute-commands")), int_or_nil(compute_commands)),
                (TermOrdKey(Term::symbol(":compute-passes")), int_or_nil(compute_passes)),
                (TermOrdKey(Term::symbol(":draw-commands")), int_or_nil(draw_commands)),
                (
                    TermOrdKey(Term::symbol(":errors")),
                    Term::Vector(case_errors.iter().cloned().map(Term::Str).collect()),
                ),
                (TermOrdKey(Term::symbol(":frame-graph-bytes")), int_or_nil(frame_bytes)),
                (TermOrdKey(Term::symbol(":frame-time-ms")), int_or_nil(frame_time)),
                (TermOrdKey(Term::symbol(":name")), Term::Str(test.name.clone())),
                (TermOrdKey(Term::symbol(":ok")), Term::Bool(case_ok)),
                (TermOrdKey(Term::symbol(":render-passes")), int_or_nil(render_passes)),
                (TermOrdKey(Term::symbol(":suite")), Term::symbol(test.suite.clone())),
            ]
            .into_iter()
            .collect(),
        );
        if !case_ok {
            errors.push(gfx_case_error(test, &case_errors, "frame budget case failed"));
        }
        cases.push(case);
    }
    let ok = errors.is_empty();
    let report = Term::Map(
        [
            (TermOrdKey(Term::symbol(":cases")), Term::Vector(cases)),
            (
                TermOrdKey(Term::symbol(":errors")),
                Term::Vector(errors.iter().cloned().map(Term::Str).collect()),
            ),
            (
                TermOrdKey(Term::symbol(":kind")),
                Term::Str("genesis/gfx-frame-budgets-v0.2".to_string()),
            ),
            (TermOrdKey(Term::symbol(":limits")), context.limits.clone()),
            (TermOrdKey(Term::symbol(":ok")), Term::Bool(ok)),
            (TermOrdKey(Term::symbol(":package")), Term::Str(manifest.name.clone())),
        ]
        .into_iter()
        .collect(),
    );
    Ok((ok, errors, report))
}

fn finish_gfx_runtime_authority(
    operation: ObligationAuthorityOperation,
    store: &EvidenceStore,
    request: Term,
    expected: (bool, Vec<String>, Term),
    frontend: &CoreformFrontend,
    limits: KernelLimits,
) -> Result<ObligationResult, ObligationError> {
    let request_hash = hash_term(&request);
    let result = invoke_authority(request, frontend, limits)?;
    let map = validate_gfx_runtime_outer(operation, &result, request_hash)?;
    if bool_field(map, ":ok", "gfx runtime final result")? != expected.0
        || string_vector(
            required_field(map, ":errors", "gfx runtime final result")?,
            "gfx runtime final result :errors",
        )? != expected.1
        || required_field(map, ":report", "gfx runtime final result")? != &expected.2
    {
        return Err(authority_error("gfx runtime final result contradiction"));
    }
    let artifact = store.put_term(&expected.2)?;
    Ok(ObligationResult {
        name: operation.obligation_name().to_string(),
        ok: expected.0,
        artifact: Some(artifact),
        errors: expected.1,
    })
}

pub(super) fn evaluate_gfx_golden_with_authority(
    store: &EvidenceStore,
    pkg_dir: &Path,
    manifest: &PackageManifest,
    modules: &[LoadedModule],
    frontend: &CoreformFrontend,
    limits: KernelLimits,
) -> Result<ObligationResult, ObligationError> {
    let context = gfx_golden_authority_context(pkg_dir, manifest, modules, limits)?;
    let plan = gfx_golden_authority_plan(manifest, &context, frontend, limits)?;
    let mut outcomes = Vec::with_capacity(plan.len());
    for test in &plan {
        let body = gfx_runtime_body(&context.bodies, &test.identity)?;
        let base = gfx_outcome_result(body, limits);
        let mut render = Term::Nil;
        if base.kind == ":value" && test.kind == ":frame-graph" && test.expect_png_hash.is_some()
            && let Some(frame) = crate::obligation_gfx::helpers::extract_frame_graph_term(&base.result)
        {
            let frame_hash = hex32(hash_term(frame));
            let (error, png_hash) = match gc_gfx::render_frame_graph_headless(
                frame,
                test.pixel_width,
                test.pixel_height,
            ) {
                Ok(output) => (Term::Nil, Term::Str(hex32(output.png_hash))),
                Err(error) => (Term::Str(error), Term::Nil),
            };
            render = Term::Map(
                [
                    (TermOrdKey(Term::symbol(":error")), error),
                    (TermOrdKey(Term::symbol(":frame-h")), Term::Str(frame_hash)),
                    (
                        TermOrdKey(Term::symbol(":pixel-height")),
                        Term::Int(BigInt::from(test.pixel_height)),
                    ),
                    (
                        TermOrdKey(Term::symbol(":pixel-width")),
                        Term::Int(BigInt::from(test.pixel_width)),
                    ),
                    (TermOrdKey(Term::symbol(":png-h")), png_hash),
                ]
                .into_iter()
                .collect(),
            );
        }
        outcomes.push(GfxGoldenOutcomeObservation {
            suite_index: test.identity.suite_index,
            entry_index: test.identity.entry_index,
            kind: base.kind,
            result: base.result,
            render,
        });
    }
    let expected = expected_gfx_golden_final(manifest, &context, &outcomes)?;
    let request = authority_request_term(
        ObligationAuthorityOperation::GfxGoldenImages,
        &manifest.name,
        gfx_runtime_request_inputs(
            context.configured,
            &context.suites,
            None,
            ":finalize",
            Some(gfx_golden_outcomes_term(&outcomes)),
        ),
    );
    finish_gfx_runtime_authority(
        ObligationAuthorityOperation::GfxGoldenImages,
        store,
        request,
        expected,
        frontend,
        limits,
    )
}

pub(super) fn evaluate_gfx_frame_budgets_with_authority(
    store: &EvidenceStore,
    pkg_dir: &Path,
    manifest: &PackageManifest,
    modules: &[LoadedModule],
    frontend: &CoreformFrontend,
    limits: KernelLimits,
) -> Result<ObligationResult, ObligationError> {
    let context = gfx_frame_budget_authority_context(pkg_dir, manifest, modules, limits)?;
    let plan = gfx_frame_budget_authority_plan(manifest, &context, frontend, limits)?;
    let mut outcomes = Vec::with_capacity(plan.len());
    for test in &plan {
        let body = gfx_runtime_body(&context.bodies, test)?;
        let mut outcome = gfx_outcome_result(body, limits);
        outcome.suite_index = test.suite_index;
        outcome.entry_index = test.entry_index;
        outcomes.push(outcome);
    }
    let expected = expected_gfx_frame_final(manifest, &context, &outcomes)?;
    let request = authority_request_term(
        ObligationAuthorityOperation::GfxFrameBudgets,
        &manifest.name,
        gfx_runtime_request_inputs(
            context.configured,
            &context.suites,
            Some(&context.limits),
            ":finalize",
            Some(gfx_frame_outcomes_term(&outcomes)),
        ),
    );
    finish_gfx_runtime_authority(
        ObligationAuthorityOperation::GfxFrameBudgets,
        store,
        request,
        expected,
        frontend,
        limits,
    )
}
