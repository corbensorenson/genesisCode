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

pub(super) fn payload_session_id(payload: &Term) -> Option<String> {
    payload_map(payload).and_then(|m| map_get_string(m, ":session-id"))
}

pub(super) fn parse_haptics_request(payload: &Term, op: &str) -> Result<XrHapticsRequest, Term> {
    let Some(session_id) = payload_session_id(payload) else {
        return Err(missing_session_error(op));
    };
    let Some(map) = payload_map(payload) else {
        return Err(bad_payload_error(op, "payload must be a map"));
    };
    let Some(input_id) = map_get_string(map, ":input-id") else {
        return Err(bad_payload_error(
            op,
            "payload must contain string field `:input-id`",
        ));
    };
    let input_id = input_id.trim().to_string();
    if input_id.is_empty() {
        return Err(bad_payload_error(
            op,
            "payload field `:input-id` must not be empty",
        ));
    }
    let Some(amplitude) = map_get_i64(map, ":amplitude") else {
        return Err(bad_payload_error(
            op,
            "payload must contain integer field `:amplitude`",
        ));
    };
    if amplitude <= 0 {
        return Err(bad_payload_error(
            op,
            "payload field `:amplitude` must be greater than zero",
        ));
    }
    let Some(duration_ms) = map_get_i64(map, ":duration-ms") else {
        return Err(bad_payload_error(
            op,
            "payload must contain integer field `:duration-ms`",
        ));
    };
    if duration_ms <= 0 {
        return Err(bad_payload_error(
            op,
            "payload field `:duration-ms` must be greater than zero",
        ));
    }
    Ok(XrHapticsRequest {
        session_id,
        input_id,
        amplitude,
        duration_ms,
    })
}

pub(super) fn parse_haptics_policy(
    pol: Option<&OpPolicy>,
    op: &str,
) -> Result<XrHapticsPolicy, Term> {
    let Some(pol) = pol else {
        return Err(policy_error(
            op,
            "per-op `allow_haptics_inputs` policy is required for gfx/xr::haptics-pulse",
        ));
    };
    let Some(raw_allow) = pol.extra.get("allow_haptics_inputs") else {
        return Err(policy_error(
            op,
            "per-op `allow_haptics_inputs` policy is required for gfx/xr::haptics-pulse",
        ));
    };
    let Some(allow_arr) = raw_allow.as_array() else {
        return Err(policy_error(
            op,
            "`allow_haptics_inputs` must be an array of strings",
        ));
    };
    let mut allowed_inputs = Vec::with_capacity(allow_arr.len());
    for item in allow_arr {
        let Some(raw) = item.as_str() else {
            return Err(policy_error(
                op,
                "`allow_haptics_inputs` entries must be strings",
            ));
        };
        let trimmed = raw.trim();
        if !trimmed.is_empty() {
            allowed_inputs.push(trimmed.to_string());
        }
    }
    if allowed_inputs.is_empty() {
        return Err(policy_error(
            op,
            "`allow_haptics_inputs` must contain at least one input id",
        ));
    }

    let max_amplitude = parse_positive_policy_i64(pol, "max_haptics_amplitude", 1000, op)?;
    if max_amplitude > 1000 {
        return Err(policy_error(
            op,
            "`max_haptics_amplitude` must be <= 1000 (milli-amplitude scale)",
        ));
    }
    let max_duration_ms = parse_positive_policy_i64(pol, "max_haptics_duration_ms", 250, op)?;
    Ok(XrHapticsPolicy {
        allowed_inputs,
        max_amplitude,
        max_duration_ms,
    })
}

pub(super) fn parse_positive_policy_i64(
    pol: &OpPolicy,
    key: &str,
    default_value: i64,
    op: &str,
) -> Result<i64, Term> {
    let Some(value) = pol.extra.get(key) else {
        return Ok(default_value);
    };
    let Some(raw) = value.as_integer() else {
        return Err(policy_error(op, &format!("`{key}` must be an integer")));
    };
    if raw <= 0 {
        return Err(policy_error(op, &format!("`{key}` must be > 0")));
    }
    Ok(raw)
}

pub(super) fn validate_haptics_policy(
    request: &XrHapticsRequest,
    policy: &XrHapticsPolicy,
    op: &str,
) -> Result<(), Term> {
    if request.amplitude > policy.max_amplitude {
        return Err(policy_error(
            op,
            &format!(
                "haptics amplitude {} exceeds max_haptics_amplitude {}",
                request.amplitude, policy.max_amplitude
            ),
        ));
    }
    if request.duration_ms > policy.max_duration_ms {
        return Err(policy_error(
            op,
            &format!(
                "haptics duration {}ms exceeds max_haptics_duration_ms {}",
                request.duration_ms, policy.max_duration_ms
            ),
        ));
    }
    let allowed = policy
        .allowed_inputs
        .iter()
        .any(|candidate| candidate.eq_ignore_ascii_case(&request.input_id));
    if !allowed {
        return Err(policy_error(
            op,
            &format!(
                "input `{}` is not allowed by allow_haptics_inputs policy",
                request.input_id
            ),
        ));
    }
    Ok(())
}

pub(super) fn map_term(items: Vec<(&str, Term)>) -> Term {
    let mut map = BTreeMap::new();
    for (k, v) in items {
        map.insert(TermOrdKey(Term::symbol(k)), v);
    }
    Term::Map(map)
}

pub(super) fn bad_payload_error(op: &str, msg: &str) -> Term {
    map_term(vec![
        (":ok", Term::Bool(false)),
        (
            ":error/code",
            Term::Str("gfx/xr-first-party-bad-payload".to_string()),
        ),
        (":error/op", Term::symbol(op)),
        (":error/message", Term::Str(msg.to_string())),
    ])
}

pub(super) fn policy_error(op: &str, msg: &str) -> Term {
    map_term(vec![
        (":ok", Term::Bool(false)),
        (
            ":error/code",
            Term::Str("core/caps/policy-error".to_string()),
        ),
        (":error/op", Term::symbol(op)),
        (":error/message", Term::Str(msg.to_string())),
    ])
}

pub(super) fn missing_session_error(op: &str) -> Term {
    map_term(vec![
        (":ok", Term::Bool(false)),
        (
            ":error/code",
            Term::Str("gfx/xr-first-party-missing-session".to_string()),
        ),
        (":error/op", Term::symbol(op)),
    ])
}

pub(super) fn unknown_session_error(op: &str, sid: &str) -> Term {
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

pub(super) fn closed_session_error(op: &str, sid: &str) -> Term {
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

pub(super) fn is_xr_host_op(op: &str) -> bool {
    matches!(
        op,
        "gfx/xr::session-open"
            | "gfx/xr::frame-poll"
            | "gfx/xr::input-poll"
            | "gfx/xr::hands-poll"
            | "gfx/xr::hit-test"
            | "gfx/xr::spatial-mesh-poll"
            | "gfx/xr::anchor-create"
            | "gfx/xr::anchor-update"
            | "gfx/xr::anchor-destroy"
            | "gfx/xr::layer-create"
            | "gfx/xr::layer-update"
            | "gfx/xr::layer-destroy"
            | "gfx/xr::haptics-pulse"
            | "gfx/xr::submit-frame"
            | "gfx/xr::session-close"
    )
}
