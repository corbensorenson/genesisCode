use super::*;

pub(crate) fn js_err(code: &str, msg: impl ToString) -> JsValue {
    JsValue::from_str(&format!("{code}: {}", msg.to_string()))
}

pub(crate) fn extract_protocol_error_string(ctx: &EvalCtx, v: &Value) -> Option<String> {
    let tok = ctx.protocol?.error;
    let Value::Sealed { token, payload } = v else {
        return None;
    };
    if *token != tok {
        return None;
    }

    let payload_term = payload.to_term_for_log(Some(tok));
    match &payload_term {
        Term::Map(m) => {
            let code = m
                .get(&TermOrdKey(Term::Symbol(":error/code".to_string())))
                .and_then(|t| match t {
                    Term::Str(s) => Some(s.as_str()),
                    _ => None,
                })
                .unwrap_or("core/error");
            let msg = m
                .get(&TermOrdKey(Term::Symbol(":error/message".to_string())))
                .and_then(|t| match t {
                    Term::Str(s) => Some(s.as_str()),
                    _ => None,
                })
                .unwrap_or("error");
            Some(format!("{code}: {msg}"))
        }
        _ => Some(print_term(&payload_term)),
    }
}

pub(crate) fn selfhost_parse_and_canon_forms(
    ctx: &mut EvalCtx,
    env: &Env,
    src: &str,
) -> Result<Vec<Term>, JsValue> {
    let parse_fn = env
        .get("selfhost/parse::parse-module")
        .ok_or_else(|| js_err("selfhost/missing", "missing selfhost/parse::parse-module"))?;
    let parsed = parse_fn
        .apply(ctx, Value::Data(Term::Str(src.to_owned())))
        .map_err(|e| js_err("selfhost/eval", e))?;
    if let Some(s) = extract_protocol_error_string(ctx, &parsed) {
        return Err(js_err("selfhost/error", s));
    }
    let Some(Term::Vector(parsed_forms)) = parsed.as_data() else {
        return Err(js_err(
            "selfhost/bad_return",
            format!(
                "selfhost parse-module returned non-vector: {}",
                parsed.debug_repr()
            ),
        ));
    };

    let canon_fn = env
        .get("selfhost/canon::canonicalize-module")
        .ok_or_else(|| {
            js_err(
                "selfhost/missing",
                "missing selfhost/canon::canonicalize-module",
            )
        })?;
    let canon = canon_fn
        .apply(ctx, Value::Data(Term::Vector(parsed_forms.clone())))
        .map_err(|e| js_err("selfhost/eval", e))?;
    if let Some(s) = extract_protocol_error_string(ctx, &canon) {
        return Err(js_err("selfhost/error", s));
    }
    let Some(Term::Vector(forms)) = canon.as_data() else {
        return Err(js_err(
            "selfhost/bad_return",
            format!(
                "selfhost canonicalize-module returned non-vector: {}",
                canon.debug_repr()
            ),
        ));
    };
    Ok(forms.clone())
}

pub(crate) fn bootstrap_selfhost(
    ctx: &mut EvalCtx,
    env: &mut Env,
    artifact_src: Option<&str>,
) -> Result<(), JsValue> {
    match artifact_src {
        Some(src) => load_selfhost_coreform_toolchain_v1_from_artifact_source(ctx, env, src)
            .map_err(|e| js_err("selfhost/init", e)),
        None => {
            load_selfhost_coreform_toolchain_v1(ctx, env).map_err(|e| js_err("selfhost/init", e))
        }
    }
}

#[derive(Debug, Serialize, Deserialize)]
pub(crate) struct GfxHeadlessHashes {
    pub(crate) width: u32,
    pub(crate) height: u32,
    pub(crate) pixel_h: String,
    pub(crate) png_h: String,
}

fn extract_frame_graph_term_for_render(t: &Term) -> Option<&Term> {
    let typed = matches!(
        t,
        Term::Map(m)
            if matches!(
                m.get(&TermOrdKey(Term::Symbol(":type".to_string()))),
                Some(Term::Symbol(s)) if s == ":gfx/frame-graph"
            )
    );
    if typed {
        return Some(t);
    }
    let Term::Map(m) = t else { return None };
    for key in [":frame", ":frame-graph"] {
        let Some(candidate) = m.get(&TermOrdKey(Term::Symbol(key.to_string()))) else {
            continue;
        };
        let is_fg = matches!(
            candidate,
            Term::Map(cm)
                if matches!(
                    cm.get(&TermOrdKey(Term::Symbol(":type".to_string()))),
                    Some(Term::Symbol(s)) if s == ":gfx/frame-graph"
                )
        );
        if is_fg {
            return Some(candidate);
        }
    }
    None
}

pub(crate) fn gfx_render_frame_graph_headless_hashes_inner(
    frame_graph_src: &str,
    width: u32,
    height: u32,
) -> Result<GfxHeadlessHashes, String> {
    let t = parse_term(frame_graph_src).map_err(|e| format!("parse: {e}"))?;
    let frame = extract_frame_graph_term_for_render(&t).ok_or_else(|| {
        "gfx/shape: expected :gfx/frame-graph term or map with :frame".to_string()
    })?;
    let img = gc_gfx::render_frame_graph_headless(frame, width, height)
        .map_err(|e| format!("gfx/render: {e}"))?;
    Ok(GfxHeadlessHashes {
        width: img.width,
        height: img.height,
        pixel_h: hex::encode(img.pixel_hash),
        png_h: hex::encode(img.png_hash),
    })
}

#[wasm_bindgen]
pub fn gfx_render_frame_graph_headless_hashes(
    frame_graph_src: &str,
    width: u32,
    height: u32,
) -> Result<JsValue, JsValue> {
    let out = gfx_render_frame_graph_headless_hashes_inner(frame_graph_src, width, height)
        .map_err(|e| js_err("gfx/headless", e))?;
    serde_wasm_bindgen::to_value(&out).map_err(|e| js_err("serde", e))
}

pub(crate) fn gate_eval_forms(
    forms: &mut Vec<Term>,
    stage1_pipeline: bool,
    stage1_gate: bool,
    stage2_gate: bool,
) -> Result<(), JsValue> {
    let mut stage1_for_stage2 = None;
    if stage1_pipeline || stage1_gate {
        let out = gc_opt::stage1_pipeline(forms).map_err(|e| js_err("stage1/error", e))?;
        if stage1_gate && !out.gate_report.ok {
            let msg = if out.gate_report.errors.is_empty() {
                "core/obligation::stage1-validation failed".to_string()
            } else {
                format!(
                    "core/obligation::stage1-validation failed: {}",
                    out.gate_report.errors.join("; ")
                )
            };
            return Err(js_err("obligation/stage1-validation", msg));
        }
        *forms = out.transformed_forms;
    }
    if stage2_gate && !stage1_pipeline && !stage1_gate {
        let out = gc_opt::stage1_pipeline(forms).map_err(|e| js_err("stage1/error", e))?;
        stage1_for_stage2 = Some(out);
    }

    if stage2_gate {
        let s2 = match stage1_for_stage2.as_ref() {
            Some(out) => gc_opt::stage2_validation_report(&out.transformed_forms),
            None => gc_opt::stage2_validation_report(forms),
        };
        if !s2.supported || !s2.ok {
            let msg = if s2.errors.is_empty() {
                "core/obligation::translation-validation (stage2 CoreForm->WASM) failed".to_string()
            } else {
                format!(
                    "core/obligation::translation-validation (stage2 CoreForm->WASM) failed: {}",
                    s2.errors.join("; ")
                )
            };
            return Err(js_err("obligation/translation-validation", msg));
        }
    }

    Ok(())
}
