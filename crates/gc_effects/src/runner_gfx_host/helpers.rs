use super::*;

pub(super) fn payload_map(payload: &Term) -> Option<&BTreeMap<TermOrdKey, Term>> {
    match payload {
        Term::Map(m) => Some(m),
        _ => None,
    }
}

pub(super) fn map_get_i64(map: &BTreeMap<TermOrdKey, Term>, key: &str) -> Option<i64> {
    map.get(&TermOrdKey(Term::symbol(key)))
        .and_then(|t| match t {
            Term::Int(v) => v.to_i64(),
            _ => None,
        })
}

pub(super) fn map_get_string(map: &BTreeMap<TermOrdKey, Term>, key: &str) -> Option<String> {
    map.get(&TermOrdKey(Term::symbol(key)))
        .and_then(|t| match t {
            Term::Str(s) => Some(s.clone()),
            Term::Symbol(s) => Some(s.clone()),
            _ => None,
        })
}

pub(super) fn payload_surface_id(payload: &Term) -> Option<String> {
    payload_map(payload).and_then(|m| map_get_string(m, ":surface"))
}

pub(super) fn map_term(items: Vec<(&str, Term)>) -> Term {
    let mut map = BTreeMap::new();
    for (k, v) in items {
        map.insert(TermOrdKey(Term::symbol(k)), v);
    }
    Term::Map(map)
}

pub(super) fn missing_surface_error(op: &str) -> Term {
    map_term(vec![
        (":ok", Term::Bool(false)),
        (
            ":error/code",
            Term::Str("gfx/first-party-missing-surface".to_string()),
        ),
        (":error/op", Term::symbol(op)),
    ])
}

pub(super) fn unknown_surface_error(op: &str, sid: &str) -> Term {
    map_term(vec![
        (":ok", Term::Bool(false)),
        (
            ":error/code",
            Term::Str("gfx/first-party-unknown-surface".to_string()),
        ),
        (":error/op", Term::symbol(op)),
        (":surface", Term::Str(sid.to_string())),
    ])
}

pub(super) fn is_gfx_host_op(op: &str) -> bool {
    matches!(
        op,
        "gfx/window::create-surface"
            | "gfx/window::resize-surface"
            | "gfx/window::set-title"
            | "gfx/window::request-redraw"
            | "gfx/window::surface-info"
            | "gfx/input::poll-events"
            | "gfx/input::set-cursor-mode"
            | "gfx/audio::enqueue"
            | "gfx/audio::set-master"
    )
}

pub(super) fn mk_error(error_tok: SealId, err: &BridgeError, op: Option<&str>) -> Value {
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
        payload: Box::new(Value::data(Term::Map(mm))),
    }
}
