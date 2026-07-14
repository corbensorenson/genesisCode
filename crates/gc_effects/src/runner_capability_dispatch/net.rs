use super::*;

#[path = "net_policy.rs"]
mod net_policy;

use net_policy::{
    net_max_request_bytes_from_policy, validate_net_bind_policy, validate_net_target_policy,
};

pub(super) fn capability_io_net_tcp_listen(
    op: &str,
    payload: &Term,
    pol: Option<&OpPolicy>,
    error_tok: SealId,
) -> Result<Value, EffectsError> {
    let local = payload_required_string_field(payload, op, ":local")?;
    if let Err(e) = validate_net_target_policy(pol, &local, op, ":local") {
        return Ok(mk_error(
            error_tok,
            "core/caps/policy-error",
            format!("{op} bind denied: {e}"),
            Some(op),
        ));
    }
    if let Err(e) = validate_net_bind_policy(pol, &local, op, ":local") {
        return Ok(mk_error(
            error_tok,
            "core/caps/policy-error",
            format!("{op} bind denied: {e}"),
            Some(op),
        ));
    }
    if !has_explicit_bridge_profile(pol) {
        return Ok(mk_error(
            error_tok,
            "core/caps/backend-unavailable",
            backend_unavailable_message(op),
            Some(op),
        ));
    }
    match call_host_bridge("net", op, payload, pol) {
        Ok(resp) => Ok(Value::data(resp)),
        Err(err) => Ok(mk_bridge_error(error_tok, &err, Some(op))),
    }
}

pub(super) fn capability_io_net_tcp_accept(
    op: &str,
    payload: &Term,
    pol: Option<&OpPolicy>,
    error_tok: SealId,
) -> Result<Value, EffectsError> {
    let _listener_id = payload_required_string_field(payload, op, ":listener-id")?;
    let max_request_bytes = match net_max_request_bytes_from_policy(pol, op) {
        Ok(v) => v,
        Err(e) => {
            return Ok(mk_error(error_tok, "core/caps/policy-error", e, Some(op)));
        }
    };
    let mut payload_map = payload_required_map_field(payload, op)?;
    payload_map.insert(
        TermOrdKey(Term::symbol(":max-request-bytes")),
        Term::Int((max_request_bytes as i64).into()),
    );
    let effective_payload = Term::Map(payload_map);
    if !has_explicit_bridge_profile(pol) {
        return Ok(mk_error(
            error_tok,
            "core/caps/backend-unavailable",
            backend_unavailable_message(op),
            Some(op),
        ));
    }
    match call_host_bridge("net", op, &effective_payload, pol) {
        Ok(resp) => Ok(Value::data(resp)),
        Err(err) => Ok(mk_bridge_error(error_tok, &err, Some(op))),
    }
}

pub(super) fn capability_io_net_http_listen(
    op: &str,
    payload: &Term,
    pol: Option<&OpPolicy>,
    error_tok: SealId,
) -> Result<Value, EffectsError> {
    let local = payload_required_string_field(payload, op, ":local")?;
    if let Err(e) = validate_net_target_policy(pol, &local, op, ":local") {
        return Ok(mk_error(
            error_tok,
            "core/caps/policy-error",
            format!("{op} bind denied: {e}"),
            Some(op),
        ));
    }
    if let Err(e) = validate_net_bind_policy(pol, &local, op, ":local") {
        return Ok(mk_error(
            error_tok,
            "core/caps/policy-error",
            format!("{op} bind denied: {e}"),
            Some(op),
        ));
    }
    let max_request_bytes = match net_max_request_bytes_from_policy(pol, op) {
        Ok(v) => v,
        Err(e) => {
            return Ok(mk_error(error_tok, "core/caps/policy-error", e, Some(op)));
        }
    };
    let mut payload_map = payload_required_map_field(payload, op)?;
    payload_map.insert(
        TermOrdKey(Term::symbol(":max-request-bytes")),
        Term::Int((max_request_bytes as i64).into()),
    );
    let effective_payload = Term::Map(payload_map);
    if !has_explicit_bridge_profile(pol) {
        return Ok(mk_error(
            error_tok,
            "core/caps/backend-unavailable",
            backend_unavailable_message(op),
            Some(op),
        ));
    }
    match call_host_bridge("net", op, &effective_payload, pol) {
        Ok(resp) => Ok(Value::data(resp)),
        Err(err) => Ok(mk_bridge_error(error_tok, &err, Some(op))),
    }
}

pub(super) fn capability_io_net_http_respond(
    op: &str,
    payload: &Term,
    pol: Option<&OpPolicy>,
    error_tok: SealId,
) -> Result<Value, EffectsError> {
    let _listener_id = payload_required_string_field(payload, op, ":listener-id")?;
    let _request_id = payload_required_string_field(payload, op, ":request-id")?;
    let status = payload_required_field(payload, op, ":status")?;
    if !matches!(status, Term::Int(_)) {
        return Err(EffectsError::BadPayload(format!(
            "{op} payload field `:status` must be an integer"
        )));
    }
    if !has_explicit_bridge_profile(pol) {
        return Ok(mk_error(
            error_tok,
            "core/caps/backend-unavailable",
            backend_unavailable_message(op),
            Some(op),
        ));
    }
    match call_host_bridge("net", op, payload, pol) {
        Ok(resp) => Ok(Value::data(resp)),
        Err(err) => Ok(mk_bridge_error(error_tok, &err, Some(op))),
    }
}

pub(super) fn capability_io_net_ws_accept(
    op: &str,
    payload: &Term,
    pol: Option<&OpPolicy>,
    error_tok: SealId,
) -> Result<Value, EffectsError> {
    let _listener_id = payload_required_string_field(payload, op, ":listener-id")?;
    let _request_id = payload_required_string_field(payload, op, ":request-id")?;
    let max_request_bytes = match net_max_request_bytes_from_policy(pol, op) {
        Ok(v) => v,
        Err(e) => {
            return Ok(mk_error(error_tok, "core/caps/policy-error", e, Some(op)));
        }
    };
    let mut payload_map = payload_required_map_field(payload, op)?;
    payload_map.insert(
        TermOrdKey(Term::symbol(":max-request-bytes")),
        Term::Int((max_request_bytes as i64).into()),
    );
    let effective_payload = Term::Map(payload_map);
    if !has_explicit_bridge_profile(pol) {
        return Ok(mk_error(
            error_tok,
            "core/caps/backend-unavailable",
            backend_unavailable_message(op),
            Some(op),
        ));
    }
    match call_host_bridge("net", op, &effective_payload, pol) {
        Ok(resp) => Ok(Value::data(resp)),
        Err(err) => Ok(mk_bridge_error(error_tok, &err, Some(op))),
    }
}

pub(super) fn capability_io_net_http_request(
    op: &str,
    payload: &Term,
    pol: Option<&OpPolicy>,
    error_tok: SealId,
) -> Result<Value, EffectsError> {
    let url = payload_required_string_field(payload, op, ":url")?;
    if let Err(e) = validate_net_target_policy(pol, &url, op, ":url") {
        return Ok(mk_error(
            error_tok,
            "core/caps/policy-error",
            format!("io/net::http-request remote denied: {e}"),
            Some(op),
        ));
    }
    if !has_explicit_bridge_profile(pol) {
        return Ok(mk_error(
            error_tok,
            "core/caps/backend-unavailable",
            backend_unavailable_message(op),
            Some(op),
        ));
    }
    match call_host_bridge("net", op, payload, pol) {
        Ok(resp) => Ok(Value::data(resp)),
        Err(err) => Ok(mk_bridge_error(error_tok, &err, Some(op))),
    }
}

pub(super) fn capability_io_net_ws_open(
    op: &str,
    payload: &Term,
    pol: Option<&OpPolicy>,
    error_tok: SealId,
) -> Result<Value, EffectsError> {
    let url = payload_required_string_field(payload, op, ":url")?;
    if let Err(e) = validate_net_target_policy(pol, &url, op, ":url") {
        return Ok(mk_error(
            error_tok,
            "core/caps/policy-error",
            format!("{op} remote denied: {e}"),
            Some(op),
        ));
    }
    if !has_explicit_bridge_profile(pol) {
        return Ok(mk_error(
            error_tok,
            "core/caps/backend-unavailable",
            backend_unavailable_message(op),
            Some(op),
        ));
    }
    match call_host_bridge("net", op, payload, pol) {
        Ok(resp) => Ok(Value::data(resp)),
        Err(err) => Ok(mk_bridge_error(error_tok, &err, Some(op))),
    }
}

pub(super) fn capability_io_net_tcp_open(
    op: &str,
    payload: &Term,
    pol: Option<&OpPolicy>,
    error_tok: SealId,
) -> Result<Value, EffectsError> {
    let remote = payload_required_string_field(payload, op, ":remote")?;
    if let Err(e) = validate_net_target_policy(pol, &remote, op, ":remote") {
        return Ok(mk_error(
            error_tok,
            "core/caps/policy-error",
            format!("{op} remote denied: {e}"),
            Some(op),
        ));
    }
    if !has_explicit_bridge_profile(pol) {
        return Ok(mk_error(
            error_tok,
            "core/caps/backend-unavailable",
            backend_unavailable_message(op),
            Some(op),
        ));
    }
    match call_host_bridge("net", op, payload, pol) {
        Ok(resp) => Ok(Value::data(resp)),
        Err(err) => Ok(mk_bridge_error(error_tok, &err, Some(op))),
    }
}

pub(super) fn capability_io_net_tcp_send(
    op: &str,
    payload: &Term,
    pol: Option<&OpPolicy>,
    error_tok: SealId,
) -> Result<Value, EffectsError> {
    let _stream_id = payload_required_string_field(payload, op, ":stream-id")?;
    let _data = payload_required_field(payload, op, ":data")?;
    if !has_explicit_bridge_profile(pol) {
        return Ok(mk_error(
            error_tok,
            "core/caps/backend-unavailable",
            backend_unavailable_message(op),
            Some(op),
        ));
    }
    match call_host_bridge("net", op, payload, pol) {
        Ok(resp) => Ok(Value::data(resp)),
        Err(err) => Ok(mk_bridge_error(error_tok, &err, Some(op))),
    }
}

pub(super) fn capability_io_net_tcp_recv_or_close(
    op: &str,
    payload: &Term,
    pol: Option<&OpPolicy>,
    error_tok: SealId,
) -> Result<Value, EffectsError> {
    let _stream_id = payload_required_string_field(payload, op, ":stream-id")?;
    if !has_explicit_bridge_profile(pol) {
        return Ok(mk_error(
            error_tok,
            "core/caps/backend-unavailable",
            backend_unavailable_message(op),
            Some(op),
        ));
    }
    match call_host_bridge("net", op, payload, pol) {
        Ok(resp) => Ok(Value::data(resp)),
        Err(err) => Ok(mk_bridge_error(error_tok, &err, Some(op))),
    }
}

pub(super) fn capability_io_net_udp_bind(
    op: &str,
    payload: &Term,
    pol: Option<&OpPolicy>,
    error_tok: SealId,
) -> Result<Value, EffectsError> {
    let local = payload_required_string_field(payload, op, ":local")?;
    if let Err(e) = validate_net_target_policy(pol, &local, op, ":local") {
        return Ok(mk_error(
            error_tok,
            "core/caps/policy-error",
            format!("{op} bind denied: {e}"),
            Some(op),
        ));
    }
    if !has_explicit_bridge_profile(pol) {
        return Ok(mk_error(
            error_tok,
            "core/caps/backend-unavailable",
            backend_unavailable_message(op),
            Some(op),
        ));
    }
    match call_host_bridge("net", op, payload, pol) {
        Ok(resp) => Ok(Value::data(resp)),
        Err(err) => Ok(mk_bridge_error(error_tok, &err, Some(op))),
    }
}

pub(super) fn capability_io_net_udp_send(
    op: &str,
    payload: &Term,
    pol: Option<&OpPolicy>,
    error_tok: SealId,
) -> Result<Value, EffectsError> {
    let _socket_id = payload_required_string_field(payload, op, ":socket-id")?;
    let remote = payload_required_string_field(payload, op, ":remote")?;
    let _data = payload_required_field(payload, op, ":data")?;
    if let Err(e) = validate_net_target_policy(pol, &remote, op, ":remote") {
        return Ok(mk_error(
            error_tok,
            "core/caps/policy-error",
            format!("{op} remote denied: {e}"),
            Some(op),
        ));
    }
    if !has_explicit_bridge_profile(pol) {
        return Ok(mk_error(
            error_tok,
            "core/caps/backend-unavailable",
            backend_unavailable_message(op),
            Some(op),
        ));
    }
    match call_host_bridge("net", op, payload, pol) {
        Ok(resp) => Ok(Value::data(resp)),
        Err(err) => Ok(mk_bridge_error(error_tok, &err, Some(op))),
    }
}

pub(super) fn capability_io_net_udp_recv_or_close(
    op: &str,
    payload: &Term,
    pol: Option<&OpPolicy>,
    error_tok: SealId,
) -> Result<Value, EffectsError> {
    let _socket_id = payload_required_string_field(payload, op, ":socket-id")?;
    if !has_explicit_bridge_profile(pol) {
        return Ok(mk_error(
            error_tok,
            "core/caps/backend-unavailable",
            backend_unavailable_message(op),
            Some(op),
        ));
    }
    match call_host_bridge("net", op, payload, pol) {
        Ok(resp) => Ok(Value::data(resp)),
        Err(err) => Ok(mk_bridge_error(error_tok, &err, Some(op))),
    }
}

pub(super) fn capability_io_net_dns_resolve(
    op: &str,
    payload: &Term,
    pol: Option<&OpPolicy>,
    error_tok: SealId,
) -> Result<Value, EffectsError> {
    let name = payload_required_string_field(payload, op, ":name")?;
    let target = format!("dns://{name}");
    if let Err(e) = validate_net_target_policy(pol, &target, op, ":name") {
        return Ok(mk_error(
            error_tok,
            "core/caps/policy-error",
            format!("{op} query denied: {e}"),
            Some(op),
        ));
    }
    if !has_explicit_bridge_profile(pol) {
        return Ok(mk_error(
            error_tok,
            "core/caps/backend-unavailable",
            backend_unavailable_message(op),
            Some(op),
        ));
    }
    match call_host_bridge("net", op, payload, pol) {
        Ok(resp) => Ok(Value::data(resp)),
        Err(err) => Ok(mk_bridge_error(error_tok, &err, Some(op))),
    }
}

pub(super) fn capability_io_net_ws_send(
    op: &str,
    payload: &Term,
    pol: Option<&OpPolicy>,
    error_tok: SealId,
) -> Result<Value, EffectsError> {
    let _stream_id = payload_required_string_field(payload, op, ":stream-id")?;
    let _data = payload_required_field(payload, op, ":data")?;
    if !has_explicit_bridge_profile(pol) {
        return Ok(mk_error(
            error_tok,
            "core/caps/backend-unavailable",
            backend_unavailable_message(op),
            Some(op),
        ));
    }
    match call_host_bridge("net", op, payload, pol) {
        Ok(resp) => Ok(Value::data(resp)),
        Err(err) => Ok(mk_bridge_error(error_tok, &err, Some(op))),
    }
}

pub(super) fn capability_io_net_ws_recv_or_close(
    op: &str,
    payload: &Term,
    pol: Option<&OpPolicy>,
    error_tok: SealId,
) -> Result<Value, EffectsError> {
    let _stream_id = payload_required_string_field(payload, op, ":stream-id")?;
    if !has_explicit_bridge_profile(pol) {
        return Ok(mk_error(
            error_tok,
            "core/caps/backend-unavailable",
            backend_unavailable_message(op),
            Some(op),
        ));
    }
    match call_host_bridge("net", op, payload, pol) {
        Ok(resp) => Ok(Value::data(resp)),
        Err(err) => Ok(mk_bridge_error(error_tok, &err, Some(op))),
    }
}
