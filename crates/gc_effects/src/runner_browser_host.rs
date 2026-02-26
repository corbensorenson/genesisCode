use std::collections::BTreeMap;

use gc_coreform::{Term, TermOrdKey};
use gc_kernel::{SealId, Value};
use num_traits::ToPrimitive;

use crate::policy::OpPolicy;
use crate::runner_host_bridge::{BridgeError, call_host_bridge};

const BROWSER_BACKEND: &str = "browser-first-party-runtime";
const BROWSER_ADAPTER: &str = "browser-host";

#[derive(Debug, Clone)]
struct BrowserWindowState {
    width: i64,
    height: i64,
    title: String,
    visible: bool,
    open: bool,
    poll_seq: u64,
}

impl BrowserWindowState {
    fn new(width: i64, height: i64, title: String, visible: bool) -> Self {
        Self {
            width,
            height,
            title,
            visible,
            open: true,
            poll_seq: 0,
        }
    }
}

#[derive(Debug, Clone, Default)]
pub(crate) struct BrowserHostRuntime {
    next_window: u64,
    windows: BTreeMap<String, BrowserWindowState>,
    storage: BTreeMap<String, Term>,
    audio_queued: u64,
    master_gain: i64,
}

pub(crate) fn browser_host_call(
    runtime: &mut BrowserHostRuntime,
    op: &str,
    payload: &Term,
    pol: Option<&OpPolicy>,
    error_tok: SealId,
) -> Option<Value> {
    if !is_browser_host_op(op) {
        return None;
    }
    if !has_explicit_bridge_profile(pol) {
        return Some(Value::Data(first_party_browser_response(
            runtime, op, payload,
        )));
    }
    Some(match call_host_bridge("browser", op, payload, pol) {
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

fn first_party_browser_response(
    runtime: &mut BrowserHostRuntime,
    op: &str,
    payload: &Term,
) -> Term {
    match op {
        "browser/window::open" => first_party_window_open(runtime, payload),
        "browser/window::close" => first_party_window_close(runtime, payload),
        "browser/window::info" => first_party_window_info(runtime, payload),
        "browser/input::poll" => first_party_input_poll(runtime, payload),
        "browser/audio::set-master" => first_party_audio_set_master(runtime, payload),
        "browser/audio::enqueue" => first_party_audio_enqueue(runtime, payload),
        "browser/storage::get" => first_party_storage_get(runtime, payload),
        "browser/storage::set" => first_party_storage_set(runtime, payload),
        "browser/storage::delete" => first_party_storage_delete(runtime, payload),
        _ => map_term(vec![
            (":ok", Term::Bool(false)),
            (
                ":error/code",
                Term::Str("browser/first-party-policy-disabled-op".to_string()),
            ),
            (":error/op", Term::symbol(op)),
            (":schema", Term::symbol(":core/host-policy-disabled.v1")),
            (":policy-disabled", Term::Bool(true)),
        ]),
    }
}

fn first_party_window_open(runtime: &mut BrowserHostRuntime, payload: &Term) -> Term {
    let opts = payload_map(payload)
        .and_then(|m| m.get(&TermOrdKey(Term::symbol(":opts"))))
        .and_then(payload_map);
    let width = opts
        .and_then(|m| map_get_i64(m, ":width"))
        .unwrap_or(1280_i64);
    let height = opts
        .and_then(|m| map_get_i64(m, ":height"))
        .unwrap_or(720_i64);
    let title = opts
        .and_then(|m| map_get_string(m, ":title"))
        .unwrap_or_else(|| "genesis-browser-window".to_string());
    let visible = opts
        .and_then(|m| map_get_bool(m, ":visible"))
        .unwrap_or(true);

    runtime.next_window = runtime.next_window.saturating_add(1);
    let window_id = format!("browser-window-{}", runtime.next_window);
    runtime.windows.insert(
        window_id.clone(),
        BrowserWindowState::new(width, height, title.clone(), visible),
    );

    map_term(vec![
        (":ok", Term::Bool(true)),
        (":backend", Term::Str(BROWSER_BACKEND.to_string())),
        (":adapter", Term::Str(BROWSER_ADAPTER.to_string())),
        (":window-id", Term::Str(window_id)),
        (":width", Term::Int(width.into())),
        (":height", Term::Int(height.into())),
        (":title", Term::Str(title)),
        (":visible", Term::Bool(visible)),
    ])
}

fn first_party_window_close(runtime: &mut BrowserHostRuntime, payload: &Term) -> Term {
    let Some(window_id) = payload_window_id(payload) else {
        return missing_window_error("browser/window::close");
    };
    let Some(window) = runtime.windows.get_mut(&window_id) else {
        return unknown_window_error("browser/window::close", &window_id);
    };
    window.open = false;
    map_term(vec![
        (":ok", Term::Bool(true)),
        (":backend", Term::Str(BROWSER_BACKEND.to_string())),
        (":adapter", Term::Str(BROWSER_ADAPTER.to_string())),
        (":window-id", Term::Str(window_id)),
        (":closed", Term::Bool(true)),
    ])
}

fn first_party_window_info(runtime: &mut BrowserHostRuntime, payload: &Term) -> Term {
    let Some(window_id) = payload_window_id(payload) else {
        return missing_window_error("browser/window::info");
    };
    let Some(window) = runtime.windows.get(&window_id) else {
        return unknown_window_error("browser/window::info", &window_id);
    };
    map_term(vec![
        (":ok", Term::Bool(true)),
        (":backend", Term::Str(BROWSER_BACKEND.to_string())),
        (":adapter", Term::Str(BROWSER_ADAPTER.to_string())),
        (":window-id", Term::Str(window_id)),
        (":width", Term::Int(window.width.into())),
        (":height", Term::Int(window.height.into())),
        (":title", Term::Str(window.title.clone())),
        (":visible", Term::Bool(window.visible)),
        (":open", Term::Bool(window.open)),
    ])
}

fn first_party_input_poll(runtime: &mut BrowserHostRuntime, payload: &Term) -> Term {
    let Some(window_id) = payload_window_id(payload) else {
        return missing_window_error("browser/input::poll");
    };
    let Some(window) = runtime.windows.get_mut(&window_id) else {
        return unknown_window_error("browser/input::poll", &window_id);
    };

    let max_events = payload_map(payload)
        .and_then(|m| map_get_i64(m, ":max-events"))
        .and_then(|v| usize::try_from(v).ok())
        .unwrap_or(8_usize);

    let mut events = Vec::new();
    if window.open && window.visible && max_events > 0 {
        window.poll_seq = window.poll_seq.saturating_add(1);
        let seq = window.poll_seq as i64;
        events.push(map_term(vec![
            (":kind", Term::symbol(":animation-frame")),
            (":window-id", Term::Str(window_id.clone())),
            (":seq", Term::Int(seq.into())),
            (":time-ms", Term::Int((seq * 16).into())),
        ]));
    }

    map_term(vec![
        (":ok", Term::Bool(true)),
        (":backend", Term::Str(BROWSER_BACKEND.to_string())),
        (":adapter", Term::Str(BROWSER_ADAPTER.to_string())),
        (":window-id", Term::Str(window_id)),
        (":events", Term::Vector(events)),
    ])
}

fn first_party_audio_set_master(runtime: &mut BrowserHostRuntime, payload: &Term) -> Term {
    let gain = payload_map(payload)
        .and_then(|m| map_get_i64(m, ":gain"))
        .unwrap_or(1_i64);
    runtime.master_gain = gain;
    map_term(vec![
        (":ok", Term::Bool(true)),
        (":backend", Term::Str(BROWSER_BACKEND.to_string())),
        (":adapter", Term::Str(BROWSER_ADAPTER.to_string())),
        (":gain", Term::Int(runtime.master_gain.into())),
    ])
}

fn first_party_audio_enqueue(runtime: &mut BrowserHostRuntime, _payload: &Term) -> Term {
    runtime.audio_queued = runtime.audio_queued.saturating_add(1);
    map_term(vec![
        (":ok", Term::Bool(true)),
        (":backend", Term::Str(BROWSER_BACKEND.to_string())),
        (":adapter", Term::Str(BROWSER_ADAPTER.to_string())),
        (":queued", Term::Int((runtime.audio_queued as i64).into())),
    ])
}

fn first_party_storage_get(runtime: &mut BrowserHostRuntime, payload: &Term) -> Term {
    let Some(key) = payload_key(payload) else {
        return missing_key_error("browser/storage::get");
    };
    let value = runtime.storage.get(&key).cloned();
    map_term(vec![
        (":ok", Term::Bool(true)),
        (":backend", Term::Str(BROWSER_BACKEND.to_string())),
        (":adapter", Term::Str(BROWSER_ADAPTER.to_string())),
        (":key", Term::Str(key)),
        (":found", Term::Bool(value.is_some())),
        (":value", value.unwrap_or(Term::Nil)),
    ])
}

fn first_party_storage_set(runtime: &mut BrowserHostRuntime, payload: &Term) -> Term {
    let Some(key) = payload_key(payload) else {
        return missing_key_error("browser/storage::set");
    };
    let Some(value) = payload_map(payload).and_then(|m| map_get_term(m, ":value")) else {
        return missing_value_error("browser/storage::set");
    };
    runtime.storage.insert(key.clone(), value);
    map_term(vec![
        (":ok", Term::Bool(true)),
        (":backend", Term::Str(BROWSER_BACKEND.to_string())),
        (":adapter", Term::Str(BROWSER_ADAPTER.to_string())),
        (":key", Term::Str(key)),
        (":stored", Term::Bool(true)),
    ])
}

fn first_party_storage_delete(runtime: &mut BrowserHostRuntime, payload: &Term) -> Term {
    let Some(key) = payload_key(payload) else {
        return missing_key_error("browser/storage::delete");
    };
    let deleted = runtime.storage.remove(&key).is_some();
    map_term(vec![
        (":ok", Term::Bool(true)),
        (":backend", Term::Str(BROWSER_BACKEND.to_string())),
        (":adapter", Term::Str(BROWSER_ADAPTER.to_string())),
        (":key", Term::Str(key)),
        (":deleted", Term::Bool(deleted)),
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

fn map_get_bool(map: &BTreeMap<TermOrdKey, Term>, key: &str) -> Option<bool> {
    map.get(&TermOrdKey(Term::symbol(key)))
        .and_then(|t| match t {
            Term::Bool(v) => Some(*v),
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

fn map_get_term(map: &BTreeMap<TermOrdKey, Term>, key: &str) -> Option<Term> {
    map.get(&TermOrdKey(Term::symbol(key))).cloned()
}

fn payload_window_id(payload: &Term) -> Option<String> {
    payload_map(payload).and_then(|m| map_get_string(m, ":window-id"))
}

fn payload_key(payload: &Term) -> Option<String> {
    payload_map(payload).and_then(|m| map_get_string(m, ":key"))
}

fn map_term(items: Vec<(&str, Term)>) -> Term {
    let mut map = BTreeMap::new();
    for (k, v) in items {
        map.insert(TermOrdKey(Term::symbol(k)), v);
    }
    Term::Map(map)
}

fn missing_window_error(op: &str) -> Term {
    map_term(vec![
        (":ok", Term::Bool(false)),
        (
            ":error/code",
            Term::Str("browser/first-party-missing-window".to_string()),
        ),
        (":error/op", Term::symbol(op)),
    ])
}

fn unknown_window_error(op: &str, window_id: &str) -> Term {
    map_term(vec![
        (":ok", Term::Bool(false)),
        (
            ":error/code",
            Term::Str("browser/first-party-unknown-window".to_string()),
        ),
        (":error/op", Term::symbol(op)),
        (":window-id", Term::Str(window_id.to_string())),
    ])
}

fn missing_key_error(op: &str) -> Term {
    map_term(vec![
        (":ok", Term::Bool(false)),
        (
            ":error/code",
            Term::Str("browser/first-party-missing-key".to_string()),
        ),
        (":error/op", Term::symbol(op)),
    ])
}

fn missing_value_error(op: &str) -> Term {
    map_term(vec![
        (":ok", Term::Bool(false)),
        (
            ":error/code",
            Term::Str("browser/first-party-missing-value".to_string()),
        ),
        (":error/op", Term::symbol(op)),
    ])
}

fn is_browser_host_op(op: &str) -> bool {
    matches!(
        op,
        "browser/window::open"
            | "browser/window::close"
            | "browser/window::info"
            | "browser/input::poll"
            | "browser/audio::set-master"
            | "browser/audio::enqueue"
            | "browser/storage::get"
            | "browser/storage::set"
            | "browser/storage::delete"
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
