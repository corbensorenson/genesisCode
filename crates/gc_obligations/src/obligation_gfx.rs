use super::*;

mod helpers;

use helpers::*;

pub(super) fn obligation_gfx_golden_images(
    store: &EvidenceStore,
    pkg_dir: &Path,
    manifest: &PackageManifest,
    modules: &[LoadedModule],
    limits: KernelLimits,
) -> Result<ObligationResult, ObligationError> {
    let mut ok = true;
    let mut errors: Vec<String> = Vec::new();
    let mut case_terms: Vec<Term> = Vec::new();

    if manifest.gfx.golden_tests.is_empty() {
        ok = false;
        errors.push(
            "gfx.golden_tests is empty; configure suite symbols for core/obligation::gfx-golden-images"
                .to_string(),
        );
    }

    let eval = eval_package_once(pkg_dir, manifest, modules, limits)?;
    let golden_case_fn = eval
        .lookup_any("core/gfx/obligation::golden-case")
        .ok_or_else(|| {
            ObligationError::Module(
                "missing prelude binding core/gfx/obligation::golden-case".to_string(),
            )
        })?;
    let golden_report_fn = eval
        .lookup_any("core/gfx/obligation::golden-report")
        .ok_or_else(|| {
            ObligationError::Module(
                "missing prelude binding core/gfx/obligation::golden-report".to_string(),
            )
        })?;
    let mut cases: Vec<GfxGoldenCase> = Vec::new();

    for suite in &manifest.gfx.golden_tests {
        let Some(suite_v) = eval.lookup_any(suite) else {
            ok = false;
            errors.push(format!("missing gfx golden suite symbol {suite}"));
            continue;
        };
        let Some(suite_map) = value_as_map(&suite_v) else {
            ok = false;
            errors.push(format!("gfx golden suite {suite} must be a map"));
            continue;
        };
        for (k, vv) in suite_map {
            let name = match &k.0 {
                Term::Str(s) => s.clone(),
                Term::Symbol(s) => s.clone(),
                other => {
                    ok = false;
                    errors.push(format!(
                        "gfx golden suite {suite}: key must be string/symbol, got {}",
                        print_term(other)
                    ));
                    continue;
                }
            };
            match parse_gfx_golden_entry(vv) {
                Ok(parsed) => cases.push(GfxGoldenCase {
                    id: TestId {
                        suite_sym: suite.clone(),
                        test_name: name,
                    },
                    body: parsed.body,
                    kind: parsed.kind,
                    expect_hash: parsed.expect_hash,
                    expect_png_hash: parsed.expect_png_hash,
                    pixel_width: parsed.pixel_width,
                    pixel_height: parsed.pixel_height,
                }),
                Err(e) => {
                    ok = false;
                    errors.push(format!("gfx golden suite {suite}::{name}: {e}"));
                }
            }
        }
    }

    for c in &cases {
        let mut runtime_error: Option<String> = None;
        let mut actual_h = Term::Nil;
        let mut actual_png_h = Term::Nil;
        let mut ctx = mk_eval_ctx(limits);
        let value = match c.body.clone().apply(&mut ctx, Value::data(Term::Nil)) {
            Ok(v) => v,
            Err(e) => {
                runtime_error = Some(format!("apply failed: {e}"));
                Value::data(Term::Nil)
            }
        };

        if runtime_error.is_none() && matches!(value, Value::EffectProgram(_)) {
            runtime_error = Some(
                "effect program returned; gfx golden tests must return pure frame/scene data"
                    .to_string(),
            );
        }

        if runtime_error.is_none() {
            let is_error = ctx
                .protocol
                .is_some_and(|p| matches!(value, Value::Sealed { token, .. } if token == p.error));
            if is_error {
                runtime_error = Some("sealed ERROR returned by golden test body".to_string());
            }
        }

        if runtime_error.is_none() {
            let term = value.to_term_for_log(ctx.protocol.map(|p| p.error));
            match c.kind {
                GfxGoldenKind::FrameGraph => match extract_frame_graph_term(&term) {
                    Some(frame) => {
                        actual_h = Term::Str(hex32(hash_term(frame)));
                        if c.expect_png_hash.is_some() {
                            match gc_gfx::render_frame_graph_headless(
                                frame,
                                c.pixel_width,
                                c.pixel_height,
                            ) {
                                Ok(img) => {
                                    actual_png_h = Term::Str(hex32(img.png_hash));
                                }
                                Err(e) => {
                                    runtime_error = Some(format!("headless render failed: {e}"));
                                }
                            }
                        }
                    }
                    None => {
                        runtime_error = Some("expected frame-graph output".to_string());
                    }
                },
                GfxGoldenKind::Scene => match extract_scene_term(&term) {
                    Some(scene) => {
                        actual_h = Term::Str(hex32(hash_term(scene)));
                    }
                    None => {
                        runtime_error = Some("expected scene output".to_string());
                    }
                },
            };
        }

        let case_args = vec![
            Term::Symbol(c.id.suite_sym.clone()),
            Term::Str(c.id.test_name.clone()),
            Term::Symbol(match c.kind {
                GfxGoldenKind::FrameGraph => ":frame-graph".to_string(),
                GfxGoldenKind::Scene => ":scene".to_string(),
            }),
            Term::Str(c.expect_hash.clone()),
            actual_h,
            c.expect_png_hash
                .as_ref()
                .map(|h| Term::Str(h.clone()))
                .unwrap_or(Term::Nil),
            actual_png_h,
            Term::Int((c.pixel_width as i64).into()),
            Term::Int((c.pixel_height as i64).into()),
            runtime_error.map(Term::Str).unwrap_or(Term::Nil),
        ];
        let case_value = apply_curried_term_args(&mut ctx, golden_case_fn.clone(), &case_args)?;
        let case_term = case_value.to_term_for_log(ctx.protocol.map(|p| p.error));
        let case_ok = term_map_get_bool(&case_term, ":ok").unwrap_or(false);
        ok &= case_ok;
        if !case_ok {
            let case_errors = term_map_get_string_vec(&case_term, ":errors");
            if case_errors.is_empty() {
                errors.push(format!(
                    "{}::{}: golden case failed",
                    c.id.suite_sym, c.id.test_name
                ));
            } else {
                errors.push(format!(
                    "{}::{}: {}",
                    c.id.suite_sym,
                    c.id.test_name,
                    case_errors.join("; ")
                ));
            }
        }
        case_terms.push(case_term);
    }

    let report_args = vec![
        Term::Str(manifest.name.clone()),
        Term::Bool(ok),
        Term::Vector(case_terms),
        Term::Vector(errors.iter().cloned().map(Term::Str).collect()),
    ];
    let mut report_ctx = mk_eval_ctx(limits);
    let report_value = apply_curried_term_args(&mut report_ctx, golden_report_fn, &report_args)?;
    let report = report_value.to_term_for_log(report_ctx.protocol.map(|p| p.error));
    let artifact = store.put_term(&report)?;
    Ok(ObligationResult {
        name: "core/obligation::gfx-golden-images".to_string(),
        ok,
        artifact: Some(artifact),
        errors,
    })
}

pub(super) fn obligation_gfx_frame_budgets(
    store: &EvidenceStore,
    pkg_dir: &Path,
    manifest: &PackageManifest,
    modules: &[LoadedModule],
    limits: KernelLimits,
) -> Result<ObligationResult, ObligationError> {
    let mut ok = true;
    let mut errors: Vec<String> = Vec::new();
    let mut case_terms: Vec<Term> = Vec::new();

    if manifest.gfx.frame_budget_tests.is_empty() {
        ok = false;
        errors.push(
            "gfx.frame_budget_tests is empty; configure suite symbols for core/obligation::gfx-frame-budgets"
                .to_string(),
        );
    }
    let has_any_limit = manifest.gfx.max_render_passes_per_frame.is_some()
        || manifest.gfx.max_compute_passes_per_frame.is_some()
        || manifest.gfx.max_draw_commands_per_frame.is_some()
        || manifest.gfx.max_compute_commands_per_frame.is_some()
        || manifest.gfx.max_frame_graph_bytes.is_some()
        || manifest.gfx.max_frame_time_ms.is_some();
    if !has_any_limit {
        ok = false;
        errors.push(
            "gfx frame budget obligation requires at least one configured gfx.* budget limit"
                .to_string(),
        );
    }

    let eval = eval_package_once(pkg_dir, manifest, modules, limits)?;
    let frame_budget_case_fn = eval
        .lookup_any("core/gfx/obligation::frame-budget-case")
        .ok_or_else(|| {
            ObligationError::Module(
                "missing prelude binding core/gfx/obligation::frame-budget-case".to_string(),
            )
        })?;
    let frame_budget_report_fn = eval
        .lookup_any("core/gfx/obligation::frame-budget-report")
        .ok_or_else(|| {
            ObligationError::Module(
                "missing prelude binding core/gfx/obligation::frame-budget-report".to_string(),
            )
        })?;
    let mut cases: Vec<GfxFrameBudgetCase> = Vec::new();

    for suite in &manifest.gfx.frame_budget_tests {
        let Some(suite_v) = eval.lookup_any(suite) else {
            ok = false;
            errors.push(format!("missing gfx frame-budget suite symbol {suite}"));
            continue;
        };
        let Some(suite_map) = value_as_map(&suite_v) else {
            ok = false;
            errors.push(format!("gfx frame-budget suite {suite} must be a map"));
            continue;
        };
        for (k, vv) in suite_map {
            let name = match &k.0 {
                Term::Str(s) => s.clone(),
                Term::Symbol(s) => s.clone(),
                other => {
                    ok = false;
                    errors.push(format!(
                        "gfx frame-budget suite {suite}: key must be string/symbol, got {}",
                        print_term(other)
                    ));
                    continue;
                }
            };
            match parse_gfx_frame_budget_entry(vv) {
                Ok(body) => cases.push(GfxFrameBudgetCase {
                    id: TestId {
                        suite_sym: suite.clone(),
                        test_name: name,
                    },
                    body,
                }),
                Err(e) => {
                    ok = false;
                    errors.push(format!("gfx frame-budget suite {suite}::{name}: {e}"));
                }
            }
        }
    }

    let mut limits_map = BTreeMap::new();
    if let Some(v) = manifest.gfx.max_render_passes_per_frame {
        limits_map.insert(
            TermOrdKey(Term::symbol(":max-render-passes-per-frame")),
            Term::Int((v as i64).into()),
        );
    }
    if let Some(v) = manifest.gfx.max_compute_passes_per_frame {
        limits_map.insert(
            TermOrdKey(Term::symbol(":max-compute-passes-per-frame")),
            Term::Int((v as i64).into()),
        );
    }
    if let Some(v) = manifest.gfx.max_draw_commands_per_frame {
        limits_map.insert(
            TermOrdKey(Term::symbol(":max-draw-commands-per-frame")),
            Term::Int((v as i64).into()),
        );
    }
    if let Some(v) = manifest.gfx.max_compute_commands_per_frame {
        limits_map.insert(
            TermOrdKey(Term::symbol(":max-compute-commands-per-frame")),
            Term::Int((v as i64).into()),
        );
    }
    if let Some(v) = manifest.gfx.max_frame_graph_bytes {
        limits_map.insert(
            TermOrdKey(Term::symbol(":max-frame-graph-bytes")),
            Term::Int((v as i64).into()),
        );
    }
    if let Some(v) = manifest.gfx.max_frame_time_ms {
        limits_map.insert(
            TermOrdKey(Term::symbol(":max-frame-time-ms")),
            Term::Int((v as i64).into()),
        );
    }
    let limits_term = Term::Map(limits_map.clone());

    for c in &cases {
        let mut runtime_error: Option<String> = None;
        let mut metrics_opt: Option<FrameMetrics> = None;
        let mut frame_time_ms: Option<u64> = None;

        let mut ctx = mk_eval_ctx(limits);
        let value = match c.body.clone().apply(&mut ctx, Value::data(Term::Nil)) {
            Ok(v) => v,
            Err(e) => {
                runtime_error = Some(format!("apply failed: {e}"));
                Value::data(Term::Nil)
            }
        };

        if runtime_error.is_none() && matches!(value, Value::EffectProgram(_)) {
            runtime_error = Some(
                "effect program returned; gfx frame budgets must return pure frame data"
                    .to_string(),
            );
        }
        if runtime_error.is_none() {
            let is_error = ctx
                .protocol
                .is_some_and(|p| matches!(value, Value::Sealed { token, .. } if token == p.error));
            if is_error {
                runtime_error = Some("sealed ERROR returned by frame budget test body".to_string());
            }
        }

        if runtime_error.is_none() {
            let term = value.to_term_for_log(ctx.protocol.map(|p| p.error));
            match extract_frame_graph_and_time(&term) {
                Ok((frame, time_ms)) => match frame_graph_metrics(frame) {
                    Ok(m) => {
                        metrics_opt = Some(m);
                        frame_time_ms = time_ms;
                    }
                    Err(e) => {
                        runtime_error = Some(e);
                    }
                },
                Err(e) => {
                    runtime_error = Some(e);
                }
            }
        }

        let mut metrics_term_map = BTreeMap::new();
        if let Some(m) = metrics_opt {
            metrics_term_map.insert(
                TermOrdKey(Term::symbol(":render-passes")),
                Term::Int((m.render_passes as i64).into()),
            );
            metrics_term_map.insert(
                TermOrdKey(Term::symbol(":compute-passes")),
                Term::Int((m.compute_passes as i64).into()),
            );
            metrics_term_map.insert(
                TermOrdKey(Term::symbol(":draw-commands")),
                Term::Int((m.draw_commands as i64).into()),
            );
            metrics_term_map.insert(
                TermOrdKey(Term::symbol(":compute-commands")),
                Term::Int((m.compute_commands as i64).into()),
            );
            metrics_term_map.insert(
                TermOrdKey(Term::symbol(":frame-graph-bytes")),
                Term::Int((m.frame_graph_bytes as i64).into()),
            );
        }
        let case_args = vec![
            Term::Symbol(c.id.suite_sym.clone()),
            Term::Str(c.id.test_name.clone()),
            Term::Map(metrics_term_map),
            limits_term.clone(),
            frame_time_ms
                .map(|x| Term::Int((x as i64).into()))
                .unwrap_or(Term::Nil),
            runtime_error.map(Term::Str).unwrap_or(Term::Nil),
        ];
        let case_value =
            apply_curried_term_args(&mut ctx, frame_budget_case_fn.clone(), &case_args)?;
        let case_term = case_value.to_term_for_log(ctx.protocol.map(|p| p.error));
        let case_ok = term_map_get_bool(&case_term, ":ok").unwrap_or(false);
        ok &= case_ok;
        if !case_ok {
            let case_errors = term_map_get_string_vec(&case_term, ":errors");
            if case_errors.is_empty() {
                errors.push(format!(
                    "{}::{}: frame budget case failed",
                    c.id.suite_sym, c.id.test_name
                ));
            } else {
                errors.push(format!(
                    "{}::{}: {}",
                    c.id.suite_sym,
                    c.id.test_name,
                    case_errors.join("; ")
                ));
            }
        }
        case_terms.push(case_term);
    }

    let report_args = vec![
        Term::Str(manifest.name.clone()),
        Term::Bool(ok),
        Term::Map(limits_map),
        Term::Vector(case_terms),
        Term::Vector(errors.iter().cloned().map(Term::Str).collect()),
    ];
    let mut report_ctx = mk_eval_ctx(limits);
    let report_value =
        apply_curried_term_args(&mut report_ctx, frame_budget_report_fn, &report_args)?;
    let report = report_value.to_term_for_log(report_ctx.protocol.map(|p| p.error));
    let artifact = store.put_term(&report)?;
    Ok(ObligationResult {
        name: "core/obligation::gfx-frame-budgets".to_string(),
        ok,
        artifact: Some(artifact),
        errors,
    })
}
