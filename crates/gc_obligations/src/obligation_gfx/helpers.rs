use super::*;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(super) enum GfxGoldenKind {
    FrameGraph,
    Scene,
}

#[derive(Debug, Clone)]
pub(super) struct GfxGoldenCase {
    pub(super) id: TestId,
    pub(super) body: Value,
    pub(super) kind: GfxGoldenKind,
    pub(super) expect_hash: String,
    pub(super) expect_png_hash: Option<String>,
    pub(super) pixel_width: u32,
    pub(super) pixel_height: u32,
}

#[derive(Debug, Clone)]
pub(super) struct ParsedGfxGoldenEntry {
    pub(super) body: Value,
    pub(super) kind: GfxGoldenKind,
    pub(super) expect_hash: String,
    pub(super) expect_png_hash: Option<String>,
    pub(super) pixel_width: u32,
    pub(super) pixel_height: u32,
}

#[derive(Debug, Clone)]
pub(super) struct GfxFrameBudgetCase {
    pub(super) id: TestId,
    pub(super) body: Value,
}

#[derive(Clone, Copy, Debug)]
pub(super) struct FrameMetrics {
    pub(super) render_passes: u64,
    pub(super) compute_passes: u64,
    pub(super) draw_commands: u64,
    pub(super) compute_commands: u64,
    pub(super) frame_graph_bytes: u64,
}

pub(super) fn parse_gfx_golden_entry(v: &Value) -> Result<ParsedGfxGoldenEntry, ObligationError> {
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

pub(super) fn parse_gfx_frame_budget_entry(v: &Value) -> Result<Value, ObligationError> {
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

pub(super) fn extract_frame_graph_term(t: &Term) -> Option<&Term> {
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

pub(super) fn extract_scene_term(t: &Term) -> Option<&Term> {
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

pub(super) fn extract_frame_graph_and_time(t: &Term) -> Result<(&Term, Option<u64>), String> {
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

pub(super) fn frame_graph_metrics(frame: &Term) -> Result<FrameMetrics, String> {
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

pub(super) fn is_hex32(s: &str) -> bool {
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
