use std::collections::BTreeMap;

use gc_coreform::{Term, TermOrdKey};
use gc_kernel::{SealId, Value};
use num_traits::ToPrimitive;

use crate::policy::OpPolicy;
use crate::runner_host_bridge::{BridgeError, call_host_bridge};

const XR_BACKEND: &str = "xr-first-party-runtime";
const XR_ADAPTER: &str = "xr-headless-sim";

#[derive(Debug, Clone)]
struct XrSessionState {
    mode: String,
    reference_space: String,
    app: String,
    open: bool,
    frame_seq: u64,
    submitted_frames: u64,
}

impl XrSessionState {
    fn new(mode: String, reference_space: String, app: String) -> Self {
        Self {
            mode,
            reference_space,
            app,
            open: true,
            frame_seq: 0,
            submitted_frames: 0,
        }
    }
}

#[derive(Debug, Clone, Default)]
pub(crate) struct XrHostRuntime {
    next_session: u64,
    sessions: BTreeMap<String, XrSessionState>,
}

pub(crate) fn xr_host_call(
    runtime: &mut XrHostRuntime,
    op: &str,
    payload: &Term,
    pol: Option<&OpPolicy>,
    error_tok: SealId,
) -> Option<Value> {
    if !is_xr_host_op(op) {
        return None;
    }
    if !has_explicit_bridge_profile(pol) {
        return Some(Value::Data(first_party_xr_response(runtime, op, payload)));
    }
    Some(match call_host_bridge("gfx-xr", op, payload, pol) {
        Ok(resp) => Value::Data(resp),
        Err(err) => mk_error(error_tok, &err, Some(op)),
    })
}

fn has_explicit_bridge_profile(pol: Option<&OpPolicy>) -> bool {
    let Some(pol) = pol else {
        return false;
    };
    let has_nonempty_str = |key: &str| {
        pol.extra
            .get(key)
            .and_then(|v| v.as_str())
            .is_some_and(|s| !s.trim().is_empty())
    };
    has_nonempty_str("bridge_cmd")
        || has_nonempty_str("wasi_bridge_response")
        || has_nonempty_str("wasi_bridge_response_file")
        || pol
            .extra
            .get("wasi_bridge_profile")
            .and_then(|v| v.as_bool())
            .unwrap_or(false)
}

fn first_party_xr_response(runtime: &mut XrHostRuntime, op: &str, payload: &Term) -> Term {
    match op {
        "gfx/xr::session-open" => first_party_session_open(runtime, payload),
        "gfx/xr::frame-poll" => first_party_frame_poll(runtime, payload),
        "gfx/xr::input-poll" => first_party_input_poll(runtime, payload),
        "gfx/xr::submit-frame" => first_party_submit_frame(runtime, payload),
        "gfx/xr::session-close" => first_party_session_close(runtime, payload),
        _ => map_term(vec![
            (":ok", Term::Bool(false)),
            (
                ":error/code",
                Term::Str("gfx/xr-first-party-unsupported-op".to_string()),
            ),
            (":error/op", Term::symbol(op)),
        ]),
    }
}

fn first_party_session_open(runtime: &mut XrHostRuntime, payload: &Term) -> Term {
    let opts = payload_map(payload)
        .and_then(|m| m.get(&TermOrdKey(Term::symbol(":opts"))))
        .and_then(payload_map);
    let mode = opts
        .and_then(|m| map_get_string(m, ":mode"))
        .unwrap_or_else(|| "immersive-vr".to_string());
    let reference_space = opts
        .and_then(|m| map_get_string(m, ":reference-space"))
        .unwrap_or_else(|| "local-floor".to_string());
    let app = opts
        .and_then(|m| map_get_string(m, ":app"))
        .unwrap_or_else(|| "genesis-xr-app".to_string());

    runtime.next_session = runtime.next_session.saturating_add(1);
    let session_id = format!("xr-session-{}", runtime.next_session);
    runtime.sessions.insert(
        session_id.clone(),
        XrSessionState::new(mode.clone(), reference_space.clone(), app.clone()),
    );

    map_term(vec![
        (":ok", Term::Bool(true)),
        (":backend", Term::Str(XR_BACKEND.to_string())),
        (":adapter", Term::Str(XR_ADAPTER.to_string())),
        (":session-id", Term::Str(session_id)),
        (":mode", Term::Str(mode)),
        (":reference-space", Term::Str(reference_space)),
        (":app", Term::Str(app)),
    ])
}

fn first_party_frame_poll(runtime: &mut XrHostRuntime, payload: &Term) -> Term {
    let Some(session_id) = payload_session_id(payload) else {
        return missing_session_error("gfx/xr::frame-poll");
    };
    let Some(session) = runtime.sessions.get_mut(&session_id) else {
        return unknown_session_error("gfx/xr::frame-poll", &session_id);
    };
    if !session.open {
        return closed_session_error("gfx/xr::frame-poll", &session_id);
    }

    session.frame_seq = session.frame_seq.saturating_add(1);
    let frame_index = session.frame_seq as i64;
    let frame = map_term(vec![
        (":frame-index", Term::Int(frame_index.into())),
        (
            ":predicted-display-time-ms",
            Term::Int((frame_index * 11).into()),
        ),
        (
            ":views",
            Term::Vector(vec![
                map_term(vec![
                    (":eye", Term::symbol(":left")),
                    (":fov-deg", Term::Int(95_i64.into())),
                ]),
                map_term(vec![
                    (":eye", Term::symbol(":right")),
                    (":fov-deg", Term::Int(95_i64.into())),
                ]),
            ]),
        ),
    ]);

    map_term(vec![
        (":ok", Term::Bool(true)),
        (":backend", Term::Str(XR_BACKEND.to_string())),
        (":adapter", Term::Str(XR_ADAPTER.to_string())),
        (":session-id", Term::Str(session_id)),
        (":frame", frame),
        (":app", Term::Str(session.app.clone())),
        (":mode", Term::Str(session.mode.clone())),
        (
            ":reference-space",
            Term::Str(session.reference_space.clone()),
        ),
    ])
}

fn first_party_input_poll(runtime: &mut XrHostRuntime, payload: &Term) -> Term {
    let Some(session_id) = payload_session_id(payload) else {
        return missing_session_error("gfx/xr::input-poll");
    };
    let Some(session) = runtime.sessions.get_mut(&session_id) else {
        return unknown_session_error("gfx/xr::input-poll", &session_id);
    };
    if !session.open {
        return closed_session_error("gfx/xr::input-poll", &session_id);
    }
    let max_inputs = payload_map(payload)
        .and_then(|m| map_get_i64(m, ":max-inputs"))
        .and_then(|v| usize::try_from(v).ok())
        .unwrap_or(2);
    let mut inputs = Vec::new();
    if max_inputs > 0 {
        inputs.push(map_term(vec![
            (":id", Term::Str("left-controller".to_string())),
            (":kind", Term::symbol(":controller")),
            (":select", Term::Bool(false)),
        ]));
    }
    if max_inputs > 1 {
        inputs.push(map_term(vec![
            (":id", Term::Str("right-controller".to_string())),
            (":kind", Term::symbol(":controller")),
            (":select", Term::Bool(true)),
        ]));
    }
    map_term(vec![
        (":ok", Term::Bool(true)),
        (":backend", Term::Str(XR_BACKEND.to_string())),
        (":adapter", Term::Str(XR_ADAPTER.to_string())),
        (":session-id", Term::Str(session_id)),
        (":inputs", Term::Vector(inputs)),
    ])
}

fn first_party_submit_frame(runtime: &mut XrHostRuntime, payload: &Term) -> Term {
    let Some(session_id) = payload_session_id(payload) else {
        return missing_session_error("gfx/xr::submit-frame");
    };
    let Some(session) = runtime.sessions.get_mut(&session_id) else {
        return unknown_session_error("gfx/xr::submit-frame", &session_id);
    };
    if !session.open {
        return closed_session_error("gfx/xr::submit-frame", &session_id);
    }
    session.submitted_frames = session.submitted_frames.saturating_add(1);
    let frame_index = payload_map(payload)
        .and_then(|m| m.get(&TermOrdKey(Term::symbol(":frame"))))
        .and_then(payload_map)
        .and_then(|fm| map_get_i64(fm, ":frame-index"))
        .unwrap_or(session.frame_seq as i64);

    map_term(vec![
        (":ok", Term::Bool(true)),
        (":backend", Term::Str(XR_BACKEND.to_string())),
        (":adapter", Term::Str(XR_ADAPTER.to_string())),
        (":session-id", Term::Str(session_id)),
        (":accepted", Term::Bool(true)),
        (":frame-index", Term::Int(frame_index.into())),
        (
            ":submitted-frames",
            Term::Int((session.submitted_frames as i64).into()),
        ),
    ])
}

fn first_party_session_close(runtime: &mut XrHostRuntime, payload: &Term) -> Term {
    let Some(session_id) = payload_session_id(payload) else {
        return missing_session_error("gfx/xr::session-close");
    };
    let Some(session) = runtime.sessions.get_mut(&session_id) else {
        return unknown_session_error("gfx/xr::session-close", &session_id);
    };
    session.open = false;
    map_term(vec![
        (":ok", Term::Bool(true)),
        (":backend", Term::Str(XR_BACKEND.to_string())),
        (":adapter", Term::Str(XR_ADAPTER.to_string())),
        (":session-id", Term::Str(session_id)),
        (":closed", Term::Bool(true)),
    ])
}

fn payload_map(payload: &Term) -> Option<&BTreeMap<TermOrdKey, Term>> {
    match payload {
        Term::Map(m) => Some(m),
        _ => None,
    }
}

fn map_get_i64(map: &BTreeMap<TermOrdKey, Term>, key: &str) -> Option<i64> {
    map.get(&TermOrdKey(Term::symbol(key)))
        .and_then(|t| match t {
            Term::Int(v) => v.to_i64(),
            _ => None,
        })
}

fn map_get_string(map: &BTreeMap<TermOrdKey, Term>, key: &str) -> Option<String> {
    map.get(&TermOrdKey(Term::symbol(key)))
        .and_then(|t| match t {
            Term::Str(s) => Some(s.clone()),
            Term::Symbol(s) => Some(s.clone()),
            _ => None,
        })
}

fn payload_session_id(payload: &Term) -> Option<String> {
    payload_map(payload).and_then(|m| map_get_string(m, ":session-id"))
}

fn map_term(items: Vec<(&str, Term)>) -> Term {
    let mut map = BTreeMap::new();
    for (k, v) in items {
        map.insert(TermOrdKey(Term::symbol(k)), v);
    }
    Term::Map(map)
}

fn missing_session_error(op: &str) -> Term {
    map_term(vec![
        (":ok", Term::Bool(false)),
        (
            ":error/code",
            Term::Str("gfx/xr-first-party-missing-session".to_string()),
        ),
        (":error/op", Term::symbol(op)),
    ])
}

fn unknown_session_error(op: &str, sid: &str) -> Term {
    map_term(vec![
        (":ok", Term::Bool(false)),
        (
            ":error/code",
            Term::Str("gfx/xr-first-party-unknown-session".to_string()),
        ),
        (":error/op", Term::symbol(op)),
        (":session-id", Term::Str(sid.to_string())),
    ])
}

fn closed_session_error(op: &str, sid: &str) -> Term {
    map_term(vec![
        (":ok", Term::Bool(false)),
        (
            ":error/code",
            Term::Str("gfx/xr-first-party-session-closed".to_string()),
        ),
        (":error/op", Term::symbol(op)),
        (":session-id", Term::Str(sid.to_string())),
    ])
}

fn is_xr_host_op(op: &str) -> bool {
    matches!(
        op,
        "gfx/xr::session-open"
            | "gfx/xr::frame-poll"
            | "gfx/xr::input-poll"
            | "gfx/xr::submit-frame"
            | "gfx/xr::session-close"
    )
}

fn mk_error(error_tok: SealId, err: &BridgeError, op: Option<&str>) -> Value {
    let mut mm = BTreeMap::new();
    mm.insert(
        TermOrdKey(Term::symbol(":error/code")),
        Term::Str(err.code.clone()),
    );
    mm.insert(
        TermOrdKey(Term::symbol(":error/message")),
        Term::Str(err.message.clone()),
    );
    mm.insert(
        TermOrdKey(Term::symbol(":error/op")),
        op.map(Term::symbol).unwrap_or(Term::Nil),
    );
    Value::Sealed {
        token: error_tok,
        payload: Box::new(Value::Data(Term::Map(mm))),
    }
}
