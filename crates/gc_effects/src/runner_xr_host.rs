use std::collections::BTreeMap;

use gc_coreform::{Term, TermOrdKey};
use gc_kernel::{SealId, Value};
use num_traits::ToPrimitive;

use crate::policy::OpPolicy;
use crate::runner_host_bridge::{BridgeError, call_host_bridge};

#[path = "runner_xr_host/advanced.rs"]
mod advanced;
#[path = "runner_xr_host/helpers.rs"]
mod helpers;

use advanced::*;
use helpers::*;

const XR_FIRST_PARTY_BACKEND: &str = "xr-first-party-runtime";
const XR_FIRST_PARTY_ADAPTER: &str = "xr-headless-sim";
const XR_WEBXR_DEVICE_BACKEND: &str = "xr-webxr-device-runtime";
const XR_WEBXR_DEVICE_ADAPTER: &str = "webxr-device";

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum XrBackendKind {
    FirstParty,
    WebxrDevice,
}

#[derive(Debug, Clone)]
struct XrAnchorState {
    space: String,
    label: String,
    pose: Term,
}

#[derive(Debug, Clone)]
struct XrLayerState {
    layer_type: String,
    layout: String,
    opacity: i64,
    transform: Term,
}

#[derive(Debug, Clone)]
struct XrSessionState {
    mode: String,
    reference_space: String,
    app: String,
    open: bool,
    frame_seq: u64,
    submitted_frames: u64,
    haptics_seq: u64,
    submitted_haptics: u64,
    anchor_seq: u64,
    anchors: BTreeMap<String, XrAnchorState>,
    layer_seq: u64,
    layers: BTreeMap<String, XrLayerState>,
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
            haptics_seq: 0,
            submitted_haptics: 0,
            anchor_seq: 0,
            anchors: BTreeMap::new(),
            layer_seq: 0,
            layers: BTreeMap::new(),
        }
    }
}

#[derive(Debug, Clone)]
struct XrHapticsRequest {
    session_id: String,
    input_id: String,
    amplitude: i64,
    duration_ms: i64,
}

#[derive(Debug, Clone)]
struct XrHapticsPolicy {
    allowed_inputs: Vec<String>,
    max_amplitude: i64,
    max_duration_ms: i64,
}

#[derive(Debug, Clone, Default)]
pub(crate) struct XrHostRuntime {
    next_session: u64,
    device_capture_seq: u64,
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
    let backend_kind = match parse_xr_backend_kind(pol, op) {
        Ok(kind) => kind,
        Err(err) => return Some(Value::Data(err)),
    };
    if backend_kind == XrBackendKind::WebxrDevice {
        return Some(webxr_device_bridge_call(
            runtime, op, payload, pol, error_tok,
        ));
    }

    if op == "gfx/xr::haptics-pulse" {
        let request = match parse_haptics_request(payload, op) {
            Ok(req) => req,
            Err(err) => return Some(Value::Data(err)),
        };
        let haptics_policy = match parse_haptics_policy(pol, op) {
            Ok(policy) => policy,
            Err(err) => return Some(Value::Data(err)),
        };
        if let Err(err) = validate_haptics_policy(&request, &haptics_policy, op) {
            return Some(Value::Data(err));
        }
        if !has_explicit_bridge_profile(pol) {
            return Some(Value::Data(first_party_haptics_pulse(runtime, &request)));
        }
        return Some(match call_host_bridge("gfx-xr", op, payload, pol) {
            Ok(resp) => Value::Data(resp),
            Err(err) => mk_error(error_tok, &err, Some(op)),
        });
    }
    if !has_explicit_bridge_profile(pol) {
        return Some(Value::Data(first_party_xr_response(
            runtime, op, payload, pol,
        )));
    }
    Some(match call_host_bridge("gfx-xr", op, payload, pol) {
        Ok(resp) => Value::Data(resp),
        Err(err) => mk_error(error_tok, &err, Some(op)),
    })
}

fn parse_xr_backend_kind(pol: Option<&OpPolicy>, op: &str) -> Result<XrBackendKind, Term> {
    let Some(pol) = pol else {
        return Ok(XrBackendKind::FirstParty);
    };
    let Some(raw) = pol.extra.get("xr_backend").and_then(|v| v.as_str()) else {
        return Ok(XrBackendKind::FirstParty);
    };
    let normalized = raw.trim().to_ascii_lowercase();
    if normalized.is_empty()
        || normalized == "first-party"
        || normalized == "first-party-runtime"
        || normalized == "headless-sim"
        || normalized == "xr-headless-sim"
    {
        return Ok(XrBackendKind::FirstParty);
    }
    if normalized == "webxr-device"
        || normalized == "device-runtime"
        || normalized == "browser-device"
    {
        return Ok(XrBackendKind::WebxrDevice);
    }
    Err(policy_error(
        op,
        &format!(
            "unsupported `xr_backend` value `{normalized}`; expected first-party-runtime or webxr-device"
        ),
    ))
}

fn webxr_device_bridge_call(
    runtime: &mut XrHostRuntime,
    op: &str,
    payload: &Term,
    pol: Option<&OpPolicy>,
    error_tok: SealId,
) -> Value {
    if !has_explicit_bridge_profile(pol) {
        return Value::Data(policy_error(
            op,
            "`xr_backend = webxr-device` requires an explicit bridge profile (`bridge_cmd` or `wasi_bridge_profile`)",
        ));
    }
    match call_host_bridge("webxr-device", op, payload, pol) {
        Ok(resp) => Value::Data(normalize_webxr_device_response(runtime, op, resp)),
        Err(err) => mk_error(error_tok, &err, Some(op)),
    }
}

fn normalize_webxr_device_response(runtime: &mut XrHostRuntime, op: &str, response: Term) -> Term {
    runtime.device_capture_seq = runtime.device_capture_seq.saturating_add(1);
    let capture_seq = runtime.device_capture_seq as i64;
    let envelope = map_term(vec![
        (
            ":schema",
            Term::symbol(":gfx/xr-webxr-device-replay-envelope.v1"),
        ),
        (":capture-seq", Term::Int(capture_seq.into())),
        (":source", Term::symbol(":webxr-device")),
        (":op", Term::symbol(op)),
        (":deterministic", Term::Bool(true)),
    ]);

    let mut out = match response {
        Term::Map(map) => map,
        other => {
            return map_term(vec![
                (":ok", Term::Bool(false)),
                (
                    ":error/code",
                    Term::Str("gfx/xr-webxr-device-bridge-bad-response".to_string()),
                ),
                (":error/op", Term::symbol(op)),
                (":response", other),
                (":replay-envelope", envelope),
                (":backend", Term::Str(XR_WEBXR_DEVICE_BACKEND.to_string())),
                (":adapter", Term::Str(XR_WEBXR_DEVICE_ADAPTER.to_string())),
            ]);
        }
    };

    out.insert(
        TermOrdKey(Term::symbol(":backend")),
        Term::Str(XR_WEBXR_DEVICE_BACKEND.to_string()),
    );
    out.entry(TermOrdKey(Term::symbol(":adapter")))
        .or_insert_with(|| Term::Str(XR_WEBXR_DEVICE_ADAPTER.to_string()));
    out.insert(TermOrdKey(Term::symbol(":replay-envelope")), envelope);
    Term::Map(out)
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

fn first_party_xr_response(
    runtime: &mut XrHostRuntime,
    op: &str,
    payload: &Term,
    pol: Option<&OpPolicy>,
) -> Term {
    match op {
        "gfx/xr::session-open" => first_party_session_open(runtime, payload),
        "gfx/xr::frame-poll" => first_party_frame_poll(runtime, payload),
        "gfx/xr::input-poll" => first_party_input_poll(runtime, payload),
        "gfx/xr::hands-poll" => first_party_hands_poll(runtime, payload, pol),
        "gfx/xr::hit-test" => first_party_hit_test(runtime, payload, pol),
        "gfx/xr::spatial-mesh-poll" => first_party_spatial_mesh_poll(runtime, payload, pol),
        "gfx/xr::anchor-create" => first_party_anchor_create(runtime, payload, pol),
        "gfx/xr::anchor-update" => first_party_anchor_update(runtime, payload, pol),
        "gfx/xr::anchor-destroy" => first_party_anchor_destroy(runtime, payload),
        "gfx/xr::layer-create" => first_party_layer_create(runtime, payload, pol),
        "gfx/xr::layer-update" => first_party_layer_update(runtime, payload, pol),
        "gfx/xr::layer-destroy" => first_party_layer_destroy(runtime, payload),
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
        (":backend", Term::Str(XR_FIRST_PARTY_BACKEND.to_string())),
        (":adapter", Term::Str(XR_FIRST_PARTY_ADAPTER.to_string())),
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
        (":backend", Term::Str(XR_FIRST_PARTY_BACKEND.to_string())),
        (":adapter", Term::Str(XR_FIRST_PARTY_ADAPTER.to_string())),
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
        (":backend", Term::Str(XR_FIRST_PARTY_BACKEND.to_string())),
        (":adapter", Term::Str(XR_FIRST_PARTY_ADAPTER.to_string())),
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
        (":backend", Term::Str(XR_FIRST_PARTY_BACKEND.to_string())),
        (":adapter", Term::Str(XR_FIRST_PARTY_ADAPTER.to_string())),
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
        (":backend", Term::Str(XR_FIRST_PARTY_BACKEND.to_string())),
        (":adapter", Term::Str(XR_FIRST_PARTY_ADAPTER.to_string())),
        (":session-id", Term::Str(session_id)),
        (":closed", Term::Bool(true)),
    ])
}

fn first_party_haptics_pulse(runtime: &mut XrHostRuntime, request: &XrHapticsRequest) -> Term {
    let Some(session) = runtime.sessions.get_mut(&request.session_id) else {
        return unknown_session_error("gfx/xr::haptics-pulse", &request.session_id);
    };
    if !session.open {
        return closed_session_error("gfx/xr::haptics-pulse", &request.session_id);
    }
    session.haptics_seq = session.haptics_seq.saturating_add(1);
    session.submitted_haptics = session.submitted_haptics.saturating_add(1);
    let pulse_id = format!("xr-haptic-{}", session.haptics_seq);
    map_term(vec![
        (":ok", Term::Bool(true)),
        (":backend", Term::Str(XR_FIRST_PARTY_BACKEND.to_string())),
        (":adapter", Term::Str(XR_FIRST_PARTY_ADAPTER.to_string())),
        (":session-id", Term::Str(request.session_id.clone())),
        (":input-id", Term::Str(request.input_id.clone())),
        (":amplitude", Term::Int(request.amplitude.into())),
        (":duration-ms", Term::Int(request.duration_ms.into())),
        (":accepted", Term::Bool(true)),
        (":pulse-id", Term::Str(pulse_id)),
        (
            ":submitted-haptics",
            Term::Int((session.submitted_haptics as i64).into()),
        ),
    ])
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
