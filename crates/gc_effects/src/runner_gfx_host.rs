use std::collections::BTreeMap;

use gc_coreform::{Term, TermOrdKey};
use gc_kernel::{SealId, Value};
use num_bigint::BigInt;
use num_traits::ToPrimitive;

#[derive(Debug, Clone)]
pub(crate) struct GfxHostRuntime {
    next_surface_id: u64,
    surfaces: BTreeMap<String, SurfaceState>,
    audio_master: Term,
    audio_queue: Vec<Term>,
}

impl Default for GfxHostRuntime {
    fn default() -> Self {
        Self {
            next_surface_id: 0,
            surfaces: BTreeMap::new(),
            audio_master: Term::Int(BigInt::from(1)),
            audio_queue: Vec::new(),
        }
    }
}

#[derive(Debug, Clone)]
struct SurfaceState {
    width: u64,
    height: u64,
    title: String,
    cursor_mode: String,
    pending_redraws: u64,
    event_seq: u64,
}

pub(crate) fn gfx_host_call(
    runtime: &mut GfxHostRuntime,
    op: &str,
    payload: &Term,
    error_tok: SealId,
) -> Option<Value> {
    match op {
        "gfx/window::create-surface" => {
            Some(window_create_surface(runtime, payload, error_tok, op))
        }
        "gfx/window::resize-surface" => {
            Some(window_resize_surface(runtime, payload, error_tok, op))
        }
        "gfx/window::set-title" => Some(window_set_title(runtime, payload, error_tok, op)),
        "gfx/window::request-redraw" => {
            Some(window_request_redraw(runtime, payload, error_tok, op))
        }
        "gfx/window::surface-info" => Some(window_surface_info(runtime, payload, error_tok, op)),
        "gfx/input::poll-events" => Some(input_poll_events(runtime, payload, error_tok, op)),
        "gfx/input::set-cursor-mode" => {
            Some(input_set_cursor_mode(runtime, payload, error_tok, op))
        }
        "gfx/audio::enqueue" => Some(audio_enqueue(runtime, payload, error_tok, op)),
        "gfx/audio::set-master" => Some(audio_set_master(runtime, payload, error_tok, op)),
        _ => None,
    }
}

fn window_create_surface(
    runtime: &mut GfxHostRuntime,
    payload: &Term,
    _error_tok: SealId,
    _op: &str,
) -> Value {
    let opts = map_field(payload, ":opts").unwrap_or(payload);
    let width = map_field_u64(opts, ":width").unwrap_or(1280);
    let height = map_field_u64(opts, ":height").unwrap_or(720);
    let title =
        map_field_str_or_symbol(opts, ":title").unwrap_or_else(|| "GenesisCode".to_string());
    let surface_id = format!("surface-{:016x}", runtime.next_surface_id);
    runtime.next_surface_id = runtime.next_surface_id.saturating_add(1);
    runtime.surfaces.insert(
        surface_id.clone(),
        SurfaceState {
            width,
            height,
            title: title.clone(),
            cursor_mode: ":normal".to_string(),
            pending_redraws: 0,
            event_seq: 0,
        },
    );
    Value::Data(map_term([
        (":ok", Term::Bool(true)),
        (":surface", Term::Str(surface_id)),
        (":width", Term::Int(BigInt::from(width))),
        (":height", Term::Int(BigInt::from(height))),
        (":title", Term::Str(title)),
    ]))
}

fn window_resize_surface(
    runtime: &mut GfxHostRuntime,
    payload: &Term,
    error_tok: SealId,
    op: &str,
) -> Value {
    let Some(surface_id) = map_field_str_or_symbol(payload, ":surface") else {
        return mk_error(
            error_tok,
            "gfx/window/bad-payload",
            "gfx/window::resize-surface payload must include :surface".to_string(),
            Some(op),
        );
    };
    let Some(width) = map_field_u64(payload, ":width") else {
        return mk_error(
            error_tok,
            "gfx/window/bad-payload",
            "gfx/window::resize-surface payload must include :width int".to_string(),
            Some(op),
        );
    };
    let Some(height) = map_field_u64(payload, ":height") else {
        return mk_error(
            error_tok,
            "gfx/window/bad-payload",
            "gfx/window::resize-surface payload must include :height int".to_string(),
            Some(op),
        );
    };
    let Some(surface) = runtime.surfaces.get_mut(&surface_id) else {
        return mk_error(
            error_tok,
            "gfx/window/not-found",
            format!("unknown surface: {surface_id}"),
            Some(op),
        );
    };
    surface.width = width;
    surface.height = height;
    Value::Data(map_term([
        (":ok", Term::Bool(true)),
        (":surface", Term::Str(surface_id)),
        (":width", Term::Int(BigInt::from(width))),
        (":height", Term::Int(BigInt::from(height))),
    ]))
}

fn window_set_title(
    runtime: &mut GfxHostRuntime,
    payload: &Term,
    error_tok: SealId,
    op: &str,
) -> Value {
    let Some(surface_id) = map_field_str_or_symbol(payload, ":surface") else {
        return mk_error(
            error_tok,
            "gfx/window/bad-payload",
            "gfx/window::set-title payload must include :surface".to_string(),
            Some(op),
        );
    };
    let Some(title) = map_field_str_or_symbol(payload, ":title") else {
        return mk_error(
            error_tok,
            "gfx/window/bad-payload",
            "gfx/window::set-title payload must include :title".to_string(),
            Some(op),
        );
    };
    let Some(surface) = runtime.surfaces.get_mut(&surface_id) else {
        return mk_error(
            error_tok,
            "gfx/window/not-found",
            format!("unknown surface: {surface_id}"),
            Some(op),
        );
    };
    surface.title = title.clone();
    Value::Data(map_term([
        (":ok", Term::Bool(true)),
        (":surface", Term::Str(surface_id)),
        (":title", Term::Str(title)),
    ]))
}

fn window_request_redraw(
    runtime: &mut GfxHostRuntime,
    payload: &Term,
    error_tok: SealId,
    op: &str,
) -> Value {
    let Some(surface_id) = map_field_str_or_symbol(payload, ":surface") else {
        return mk_error(
            error_tok,
            "gfx/window/bad-payload",
            "gfx/window::request-redraw payload must include :surface".to_string(),
            Some(op),
        );
    };
    let Some(surface) = runtime.surfaces.get_mut(&surface_id) else {
        return mk_error(
            error_tok,
            "gfx/window/not-found",
            format!("unknown surface: {surface_id}"),
            Some(op),
        );
    };
    surface.pending_redraws = surface.pending_redraws.saturating_add(1);
    Value::Data(map_term([
        (":ok", Term::Bool(true)),
        (":surface", Term::Str(surface_id)),
        (
            ":pending-redraws",
            Term::Int(BigInt::from(surface.pending_redraws)),
        ),
    ]))
}

fn window_surface_info(
    runtime: &mut GfxHostRuntime,
    payload: &Term,
    error_tok: SealId,
    op: &str,
) -> Value {
    let Some(surface_id) = map_field_str_or_symbol(payload, ":surface") else {
        return mk_error(
            error_tok,
            "gfx/window/bad-payload",
            "gfx/window::surface-info payload must include :surface".to_string(),
            Some(op),
        );
    };
    let Some(surface) = runtime.surfaces.get(&surface_id) else {
        return mk_error(
            error_tok,
            "gfx/window/not-found",
            format!("unknown surface: {surface_id}"),
            Some(op),
        );
    };
    Value::Data(map_term([
        (":surface", Term::Str(surface_id)),
        (":width", Term::Int(BigInt::from(surface.width))),
        (":height", Term::Int(BigInt::from(surface.height))),
        (":title", Term::Str(surface.title.clone())),
        (":cursor-mode", Term::Symbol(surface.cursor_mode.clone())),
        (
            ":pending-redraws",
            Term::Int(BigInt::from(surface.pending_redraws)),
        ),
    ]))
}

fn input_poll_events(
    runtime: &mut GfxHostRuntime,
    payload: &Term,
    error_tok: SealId,
    op: &str,
) -> Value {
    let Some(surface_id) = map_field_str_or_symbol(payload, ":surface") else {
        return mk_error(
            error_tok,
            "gfx/input/bad-payload",
            "gfx/input::poll-events payload must include :surface".to_string(),
            Some(op),
        );
    };
    let Some(surface) = runtime.surfaces.get_mut(&surface_id) else {
        return mk_error(
            error_tok,
            "gfx/window/not-found",
            format!("unknown surface: {surface_id}"),
            Some(op),
        );
    };
    let mut events = Vec::new();
    for _ in 0..surface.pending_redraws {
        surface.event_seq = surface.event_seq.saturating_add(1);
        events.push(map_term([
            (":kind", Term::Symbol(":redraw".to_string())),
            (":surface", Term::Str(surface_id.clone())),
            (":seq", Term::Int(BigInt::from(surface.event_seq))),
        ]));
    }
    surface.pending_redraws = 0;
    Value::Data(map_term([
        (":surface", Term::Str(surface_id)),
        (":events", Term::Vector(events)),
    ]))
}

fn input_set_cursor_mode(
    runtime: &mut GfxHostRuntime,
    payload: &Term,
    error_tok: SealId,
    op: &str,
) -> Value {
    let Some(surface_id) = map_field_str_or_symbol(payload, ":surface") else {
        return mk_error(
            error_tok,
            "gfx/input/bad-payload",
            "gfx/input::set-cursor-mode payload must include :surface".to_string(),
            Some(op),
        );
    };
    let Some(mode) = map_field_str_or_symbol(payload, ":mode") else {
        return mk_error(
            error_tok,
            "gfx/input/bad-payload",
            "gfx/input::set-cursor-mode payload must include :mode".to_string(),
            Some(op),
        );
    };
    let Some(surface) = runtime.surfaces.get_mut(&surface_id) else {
        return mk_error(
            error_tok,
            "gfx/window/not-found",
            format!("unknown surface: {surface_id}"),
            Some(op),
        );
    };
    surface.cursor_mode = mode.clone();
    Value::Data(map_term([
        (":ok", Term::Bool(true)),
        (":surface", Term::Str(surface_id)),
        (":mode", Term::Symbol(mode)),
    ]))
}

fn audio_enqueue(
    runtime: &mut GfxHostRuntime,
    payload: &Term,
    _error_tok: SealId,
    _op: &str,
) -> Value {
    let event = map_field(payload, ":event").cloned().unwrap_or(Term::Nil);
    runtime.audio_queue.push(event);
    Value::Data(map_term([
        (":ok", Term::Bool(true)),
        (
            ":queued",
            Term::Int(BigInt::from(runtime.audio_queue.len() as u64)),
        ),
    ]))
}

fn audio_set_master(
    runtime: &mut GfxHostRuntime,
    payload: &Term,
    error_tok: SealId,
    op: &str,
) -> Value {
    let Some(gain) = map_field(payload, ":gain").cloned() else {
        return mk_error(
            error_tok,
            "gfx/audio/bad-payload",
            "gfx/audio::set-master payload must include :gain".to_string(),
            Some(op),
        );
    };
    runtime.audio_master = gain.clone();
    Value::Data(map_term([(":ok", Term::Bool(true)), (":gain", gain)]))
}

fn map_term<const N: usize>(pairs: [(&str, Term); N]) -> Term {
    Term::Map(
        pairs
            .into_iter()
            .map(|(k, v)| (TermOrdKey(Term::symbol(k)), v))
            .collect(),
    )
}

fn map_field<'a>(t: &'a Term, key: &str) -> Option<&'a Term> {
    let Term::Map(m) = t else {
        return None;
    };
    m.get(&TermOrdKey(Term::symbol(key)))
}

fn map_field_u64(t: &Term, key: &str) -> Option<u64> {
    match map_field(t, key) {
        Some(Term::Int(i)) if i.sign() != num_bigint::Sign::Minus => i.to_u64(),
        _ => None,
    }
}

fn map_field_str_or_symbol(t: &Term, key: &str) -> Option<String> {
    match map_field(t, key) {
        Some(Term::Str(s)) => Some(s.clone()),
        Some(Term::Symbol(s)) => Some(s.clone()),
        _ => None,
    }
}

fn mk_error(error_tok: SealId, code: &str, msg: String, op: Option<&str>) -> Value {
    let mut mm = BTreeMap::new();
    mm.insert(
        TermOrdKey(Term::symbol(":error/code")),
        Term::Str(code.to_string()),
    );
    mm.insert(TermOrdKey(Term::symbol(":error/message")), Term::Str(msg));
    mm.insert(
        TermOrdKey(Term::symbol(":error/op")),
        op.map(Term::symbol).unwrap_or(Term::Nil),
    );
    Value::Sealed {
        token: error_tok,
        payload: Box::new(Value::Data(Term::Map(mm))),
    }
}
