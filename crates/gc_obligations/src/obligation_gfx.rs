use super::*;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum GfxGoldenKind {
    FrameGraph,
    Scene,
}

#[derive(Debug, Clone)]
struct GfxGoldenCase {
    id: TestId,
    body: Value,
    kind: GfxGoldenKind,
    expect_hash: String,
    expect_png_hash: Option<String>,
    pixel_width: u32,
    pixel_height: u32,
}

#[derive(Debug, Clone)]
struct ParsedGfxGoldenEntry {
    body: Value,
    kind: GfxGoldenKind,
    expect_hash: String,
    expect_png_hash: Option<String>,
    pixel_width: u32,
    pixel_height: u32,
}

#[derive(Debug, Clone)]
struct GfxFrameBudgetCase {
    id: TestId,
    body: Value,
}

#[derive(Clone, Copy, Debug)]
struct FrameMetrics {
    render_passes: u64,
    compute_passes: u64,
    draw_commands: u64,
    compute_commands: u64,
    frame_graph_bytes: u64,
}

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
        let value = match c.body.clone().apply(&mut ctx, Value::Data(Term::Nil)) {
            Ok(v) => v,
            Err(e) => {
                runtime_error = Some(format!("apply failed: {e}"));
                Value::Data(Term::Nil)
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
        let value = match c.body.clone().apply(&mut ctx, Value::Data(Term::Nil)) {
            Ok(v) => v,
            Err(e) => {
                runtime_error = Some(format!("apply failed: {e}"));
                Value::Data(Term::Nil)
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

pub(super) fn obligation_gfx_api_stability(
    store: &EvidenceStore,
    manifest: &PackageManifest,
    modules: &[LoadedModule],
    limits: KernelLimits,
) -> Result<ObligationResult, ObligationError> {
    let mut ok = true;
    let mut errors: Vec<String> = Vec::new();

    let mut def_hashes: BTreeMap<String, [u8; 32]> = BTreeMap::new();
    for m in modules {
        for f in &m.forms {
            if let Some((name, expr)) = parse_def(f) {
                let h = hash_term(&expr);
                if let Some(prev) = def_hashes.insert(name.clone(), h)
                    && prev != h
                {
                    ok = false;
                    errors.push(format!(
                        "symbol {} has conflicting definitions across modules",
                        name
                    ));
                }
            }
        }
    }

    let mut exported: BTreeSet<String> = BTreeSet::new();
    for m in modules {
        if let Some(meta) = extract_meta_static(&m.forms)
            && let Some(es) = meta_exports(&meta)
        {
            for e in es {
                if e.starts_with("core/gfx/") {
                    exported.insert(e);
                }
            }
        }
    }

    let expected: BTreeSet<String> = manifest.gfx.api_exports.iter().cloned().collect();
    let tracked: BTreeSet<String> = if expected.is_empty() {
        exported.clone()
    } else {
        expected.clone()
    };

    let mut def_entries: Vec<Term> = Vec::new();
    let mut missing_defs: Vec<String> = Vec::new();
    for sym in &tracked {
        match def_hashes.get(sym) {
            Some(h) => def_entries.push(Term::Map(
                [
                    (TermOrdKey(Term::symbol(":sym")), Term::Symbol(sym.clone())),
                    (
                        TermOrdKey(Term::symbol(":expr-h")),
                        Term::Bytes(h.to_vec().into()),
                    ),
                ]
                .into_iter()
                .collect(),
            )),
            None => {
                missing_defs.push(sym.clone());
            }
        }
    }

    let surface = Term::Map(
        [
            (
                TermOrdKey(Term::symbol(":kind")),
                Term::Str("genesis/gfx-api-surface-v0.2".to_string()),
            ),
            (
                TermOrdKey(Term::symbol(":exports")),
                Term::Vector(tracked.iter().cloned().map(Term::Symbol).collect()),
            ),
            (TermOrdKey(Term::symbol(":defs")), Term::Vector(def_entries)),
        ]
        .into_iter()
        .collect(),
    );
    let surface_hash = hex32(hash_term(&surface));
    let expected_surface = manifest
        .gfx
        .api_surface_hash
        .as_ref()
        .map(|s| s.to_ascii_lowercase());
    if let Some(want) = expected_surface.as_ref()
        && !is_hex32(want)
    {
        ok = false;
        errors.push("gfx.api_surface_hash must be 64 lowercase hex chars".to_string());
    }

    let mut analysis_ctx = mk_eval_ctx(limits);
    let prelude = build_prelude(&mut analysis_ctx);
    let analysis_fn = prelude
        .env
        .get("core/gfx/obligation::api-stability-analysis")
        .ok_or_else(|| {
            ObligationError::Module(
                "missing prelude binding core/gfx/obligation::api-stability-analysis".to_string(),
            )
        })?;
    let report_fn = prelude
        .env
        .get("core/gfx/obligation::api-stability-report")
        .ok_or_else(|| {
            ObligationError::Module(
                "missing prelude binding core/gfx/obligation::api-stability-report".to_string(),
            )
        })?;

    let analysis_args = vec![
        Term::Vector(exported.iter().cloned().map(Term::Symbol).collect()),
        Term::Vector(expected.iter().cloned().map(Term::Symbol).collect()),
        Term::Vector(tracked.iter().cloned().map(Term::Symbol).collect()),
        expected_surface
            .as_ref()
            .map(|s| Term::Str(s.clone()))
            .unwrap_or(Term::Nil),
        Term::Str(surface_hash.clone()),
        Term::Bool(!manifest.gfx.api_exports.is_empty() || manifest.gfx.api_surface_hash.is_some()),
        Term::Vector(missing_defs.into_iter().map(Term::Symbol).collect()),
    ];
    let analysis_value = apply_curried_term_args(&mut analysis_ctx, analysis_fn, &analysis_args)?;
    let analysis_term = analysis_value.to_term_for_log(analysis_ctx.protocol.map(|p| p.error));
    let analysis_ok = term_map_get_bool(&analysis_term, ":ok").unwrap_or(false);
    ok &= analysis_ok;
    errors.extend(term_map_get_string_vec(&analysis_term, ":errors"));

    let report_args = vec![
        Term::Str(manifest.name.clone()),
        Term::Bool(ok),
        Term::Str(surface_hash),
        expected_surface.map(Term::Str).unwrap_or(Term::Nil),
        surface,
        Term::Vector(errors.iter().cloned().map(Term::Str).collect()),
    ];
    let mut report_ctx = mk_eval_ctx(limits);
    let report_value = apply_curried_term_args(&mut report_ctx, report_fn, &report_args)?;
    let report = report_value.to_term_for_log(report_ctx.protocol.map(|p| p.error));

    let artifact = store.put_term(&report)?;
    Ok(ObligationResult {
        name: "core/obligation::gfx-api-stability".to_string(),
        ok,
        artifact: Some(artifact),
        errors,
    })
}

fn parse_gfx_golden_entry(v: &Value) -> Result<ParsedGfxGoldenEntry, ObligationError> {
    let Some(m) = value_as_map(v) else {
        return Err(ObligationError::Test(
            "golden entry must be a map".to_string(),
        ));
    };
    let body = m
        .get(&TermOrdKey(Term::symbol(":body")))
        .ok_or_else(|| ObligationError::Test("golden entry missing :body".to_string()))?;
    if !is_callable_value(body) {
        return Err(ObligationError::Test(
            "golden entry :body must be callable".to_string(),
        ));
    }
    let expect = match m.get(&TermOrdKey(Term::symbol(":expect-h"))) {
        Some(Value::Data(Term::Str(s))) => s.to_ascii_lowercase(),
        Some(Value::Data(Term::Symbol(s))) => s.to_ascii_lowercase(),
        Some(other) => {
            return Err(ObligationError::Test(format!(
                "golden entry :expect-h must be string/symbol, got {}",
                other.debug_repr()
            )));
        }
        None => {
            return Err(ObligationError::Test(
                "golden entry missing :expect-h".to_string(),
            ));
        }
    };
    if !is_hex32(&expect) {
        return Err(ObligationError::Test(
            "golden entry :expect-h must be 64 lowercase hex chars".to_string(),
        ));
    }
    let expect_png_hash = match m.get(&TermOrdKey(Term::symbol(":expect-png-h"))) {
        None | Some(Value::Data(Term::Nil)) => None,
        Some(Value::Data(Term::Str(s))) => {
            let h = s.to_ascii_lowercase();
            if !is_hex32(&h) {
                return Err(ObligationError::Test(
                    "golden entry :expect-png-h must be 64 lowercase hex chars".to_string(),
                ));
            }
            Some(h)
        }
        Some(Value::Data(Term::Symbol(s))) => {
            let h = s.to_ascii_lowercase();
            if !is_hex32(&h) {
                return Err(ObligationError::Test(
                    "golden entry :expect-png-h must be 64 lowercase hex chars".to_string(),
                ));
            }
            Some(h)
        }
        Some(other) => {
            return Err(ObligationError::Test(format!(
                "golden entry :expect-png-h must be string/symbol or nil, got {}",
                other.debug_repr()
            )));
        }
    };
    let pixel_width = parse_u32_field(m, ":pixel-width")?.unwrap_or(256);
    let pixel_height = parse_u32_field(m, ":pixel-height")?.unwrap_or(256);
    if pixel_width == 0 || pixel_height == 0 {
        return Err(ObligationError::Test(
            "golden entry pixel size must be non-zero".to_string(),
        ));
    }
    let kind = match m.get(&TermOrdKey(Term::symbol(":kind"))) {
        Some(Value::Data(Term::Symbol(s))) | Some(Value::Data(Term::Str(s))) => match s.as_str() {
            ":frame-graph" | "frame-graph" => GfxGoldenKind::FrameGraph,
            ":scene" | "scene" => GfxGoldenKind::Scene,
            _ => {
                return Err(ObligationError::Test(format!(
                    "golden entry :kind must be :frame-graph or :scene, got {s}"
                )));
            }
        },
        Some(other) => {
            return Err(ObligationError::Test(format!(
                "golden entry :kind must be symbol/string, got {}",
                other.debug_repr()
            )));
        }
        None => GfxGoldenKind::FrameGraph,
    };
    if expect_png_hash.is_some() && kind != GfxGoldenKind::FrameGraph {
        return Err(ObligationError::Test(
            "golden entry :expect-png-h currently supports only :frame-graph kind".to_string(),
        ));
    }
    Ok(ParsedGfxGoldenEntry {
        body: body.clone(),
        kind,
        expect_hash: expect,
        expect_png_hash,
        pixel_width,
        pixel_height,
    })
}

fn parse_gfx_frame_budget_entry(v: &Value) -> Result<Value, ObligationError> {
    if is_callable_value(v) {
        return Ok(v.clone());
    }
    let Some(m) = value_as_map(v) else {
        return Err(ObligationError::Test(
            "frame budget entry must be callable or map".to_string(),
        ));
    };
    let body = m
        .get(&TermOrdKey(Term::symbol(":body")))
        .ok_or_else(|| ObligationError::Test("frame budget entry missing :body".to_string()))?;
    if !is_callable_value(body) {
        return Err(ObligationError::Test(
            "frame budget entry :body must be callable".to_string(),
        ));
    }
    Ok(body.clone())
}

fn term_is_typed_map(t: &Term, typ: &str) -> bool {
    let Term::Map(m) = t else { return false };
    matches!(
        m.get(&TermOrdKey(Term::symbol(":type"))),
        Some(Term::Symbol(s)) if s == typ
    )
}

fn extract_frame_graph_term(t: &Term) -> Option<&Term> {
    if term_is_typed_map(t, ":gfx/frame-graph") {
        return Some(t);
    }
    let Term::Map(m) = t else { return None };
    for key in [":frame", ":frame-graph"] {
        if let Some(inner) = m.get(&TermOrdKey(Term::symbol(key)))
            && term_is_typed_map(inner, ":gfx/frame-graph")
        {
            return Some(inner);
        }
    }
    None
}

fn extract_scene_term(t: &Term) -> Option<&Term> {
    if term_is_typed_map(t, ":gfx/scene") {
        return Some(t);
    }
    let Term::Map(m) = t else { return None };
    for key in [":scene"] {
        if let Some(inner) = m.get(&TermOrdKey(Term::symbol(key)))
            && term_is_typed_map(inner, ":gfx/scene")
        {
            return Some(inner);
        }
    }
    None
}

fn extract_frame_graph_and_time(t: &Term) -> Result<(&Term, Option<u64>), String> {
    if term_is_typed_map(t, ":gfx/frame-graph") {
        return Ok((t, None));
    }
    let Term::Map(m) = t else {
        return Err("expected frame-graph or {:frame <frame-graph> ...}".to_string());
    };
    let frame = m
        .get(&TermOrdKey(Term::symbol(":frame")))
        .or_else(|| m.get(&TermOrdKey(Term::symbol(":frame-graph"))))
        .ok_or_else(|| "missing :frame/:frame-graph field".to_string())?;
    if !term_is_typed_map(frame, ":gfx/frame-graph") {
        return Err(":frame value must be :gfx/frame-graph".to_string());
    }
    let frame_time_ms = match m.get(&TermOrdKey(Term::symbol(":frame-time-ms"))) {
        None | Some(Term::Nil) => None,
        Some(Term::Int(i)) => i.to_u64(),
        Some(other) => {
            return Err(format!(
                ":frame-time-ms must be int or nil, got {}",
                print_term(other)
            ));
        }
    };
    Ok((frame, frame_time_ms))
}

fn frame_graph_metrics(frame: &Term) -> Result<FrameMetrics, String> {
    let Term::Map(m) = frame else {
        return Err("frame graph must be a map".to_string());
    };
    let render = m
        .get(&TermOrdKey(Term::symbol(":render-passes")))
        .ok_or_else(|| "frame graph missing :render-passes".to_string())?;
    let compute = m
        .get(&TermOrdKey(Term::symbol(":compute-passes")))
        .ok_or_else(|| "frame graph missing :compute-passes".to_string())?;
    let Term::Vector(render_passes) = render else {
        return Err(":render-passes must be a vector".to_string());
    };
    let Term::Vector(compute_passes) = compute else {
        return Err(":compute-passes must be a vector".to_string());
    };

    let mut draw_commands = 0u64;
    for rp in render_passes {
        let Term::Map(rm) = rp else {
            return Err("render pass must be a map".to_string());
        };
        let cmds = rm
            .get(&TermOrdKey(Term::symbol(":commands")))
            .ok_or_else(|| "render pass missing :commands".to_string())?;
        let Term::Vector(v) = cmds else {
            return Err("render pass :commands must be a vector".to_string());
        };
        draw_commands = draw_commands.saturating_add(v.len() as u64);
    }

    let mut compute_commands = 0u64;
    for cp in compute_passes {
        let Term::Map(cm) = cp else {
            return Err("compute pass must be a map".to_string());
        };
        let cmds = cm
            .get(&TermOrdKey(Term::symbol(":commands")))
            .ok_or_else(|| "compute pass missing :commands".to_string())?;
        let Term::Vector(v) = cmds else {
            return Err("compute pass :commands must be a vector".to_string());
        };
        compute_commands = compute_commands.saturating_add(v.len() as u64);
    }

    Ok(FrameMetrics {
        render_passes: render_passes.len() as u64,
        compute_passes: compute_passes.len() as u64,
        draw_commands,
        compute_commands,
        frame_graph_bytes: print_term(frame).len() as u64,
    })
}

fn is_hex32(s: &str) -> bool {
    s.len() == 64 && s.chars().all(|c| c.is_ascii_hexdigit())
}

fn parse_u32_field(
    m: &BTreeMap<TermOrdKey, Value>,
    key: &str,
) -> Result<Option<u32>, ObligationError> {
    match m.get(&TermOrdKey(Term::symbol(key))) {
        None | Some(Value::Data(Term::Nil)) => Ok(None),
        Some(Value::Data(Term::Int(i))) => {
            let n = i.to_u64().ok_or_else(|| {
                ObligationError::Test(format!("{key} must be a non-negative integer"))
            })?;
            let n = u32::try_from(n)
                .map_err(|_| ObligationError::Test(format!("{key} exceeds u32 range")))?;
            Ok(Some(n))
        }
        Some(other) => Err(ObligationError::Test(format!(
            "{key} must be int or nil, got {}",
            other.debug_repr()
        ))),
    }
}
