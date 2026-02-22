use super::*;

fn net_allowlist_from_policy(pol: Option<&OpPolicy>, op: &str) -> Result<Vec<String>, String> {
    let Some(pol) = pol else {
        return Err(format!(
            "{op} requires per-op url_allow allowlist in caps.toml"
        ));
    };
    let allow_key = if pol.extra.contains_key("url_allow") {
        "url_allow"
    } else {
        "remote_allow"
    };
    let Some(v) = pol.extra.get(allow_key) else {
        return Err(format!(
            "{op} requires per-op url_allow allowlist in caps.toml"
        ));
    };
    let Some(arr) = v.as_array() else {
        return Err(format!("{allow_key} must be an array of strings"));
    };
    let mut out = Vec::with_capacity(arr.len());
    for x in arr {
        let Some(raw) = x.as_str() else {
            return Err(format!("{allow_key} entries must be strings"));
        };
        let trimmed = raw.trim();
        if !trimmed.is_empty() {
            out.push(trimmed.to_string());
        }
    }
    if out.is_empty() {
        return Err("url_allow must contain at least one URL prefix".to_string());
    }
    Ok(out)
}

fn net_allow_http_from_policy(pol: Option<&OpPolicy>) -> Result<bool, String> {
    let Some(pol) = pol else {
        return Ok(false);
    };
    let Some(v) = pol.extra.get("allow_http") else {
        return Ok(false);
    };
    let Some(allow_http) = v.as_bool() else {
        return Err("allow_http must be a boolean".to_string());
    };
    Ok(allow_http)
}

fn net_wasi_network_profile_from_policy(pol: Option<&OpPolicy>) -> Result<Option<String>, String> {
    let Some(pol) = pol else {
        return Ok(None);
    };
    let Some(v) = pol.extra.get("wasi_network_profile") else {
        return Ok(None);
    };
    let Some(raw) = v.as_str() else {
        return Err("wasi_network_profile must be a string".to_string());
    };
    let trimmed = raw.trim();
    if trimmed.is_empty() {
        return Err("wasi_network_profile must not be empty".to_string());
    }
    Ok(Some(trimmed.to_string()))
}

fn net_bind_hosts_from_policy(pol: Option<&OpPolicy>, op: &str) -> Result<Vec<String>, String> {
    parse_nonempty_string_array(
        pol,
        "allow_bind_hosts",
        &format!("{op} requires per-op allow_bind_hosts allowlist in caps.toml"),
    )
}

fn parse_nonempty_u16_array(
    pol: Option<&OpPolicy>,
    key: &str,
    missing_msg: &str,
) -> Result<Vec<u16>, String> {
    let Some(pol) = pol else {
        return Err(missing_msg.to_string());
    };
    let Some(v) = pol.extra.get(key) else {
        return Err(missing_msg.to_string());
    };
    let Some(arr) = v.as_array() else {
        return Err(format!("{key} must be an array of integers"));
    };
    let mut out = Vec::with_capacity(arr.len());
    for x in arr {
        let Some(raw) = x.as_integer() else {
            return Err(format!("{key} entries must be integers"));
        };
        if !(1..=65535).contains(&raw) {
            return Err(format!("{key} entries must be between 1 and 65535"));
        }
        out.push(raw as u16);
    }
    if out.is_empty() {
        return Err(format!("{key} must contain at least one entry"));
    }
    Ok(out)
}

fn net_bind_ports_from_policy(pol: Option<&OpPolicy>, op: &str) -> Result<Vec<u16>, String> {
    parse_nonempty_u16_array(
        pol,
        "allow_bind_ports",
        &format!("{op} requires per-op allow_bind_ports allowlist in caps.toml"),
    )
}

fn net_max_request_bytes_from_policy(pol: Option<&OpPolicy>, op: &str) -> Result<usize, String> {
    let Some(pol) = pol else {
        return Err(format!(
            "{op} requires per-op max_request_bytes bound in caps.toml"
        ));
    };
    let Some(v) = pol.extra.get("max_request_bytes") else {
        return Err(format!(
            "{op} requires per-op max_request_bytes bound in caps.toml"
        ));
    };
    let Some(raw) = v.as_integer() else {
        return Err("max_request_bytes must be an integer".to_string());
    };
    if raw <= 0 {
        return Err("max_request_bytes must be greater than zero".to_string());
    }
    let Ok(bound) = usize::try_from(raw) else {
        return Err("max_request_bytes exceeds platform usize range".to_string());
    };
    Ok(bound)
}

fn validate_net_wasi_profile(profile: Option<&str>, scheme: &str) -> Result<(), String> {
    if !cfg!(target_os = "wasi") {
        return Ok(());
    }
    let profile = profile.unwrap_or("none");
    match profile {
        "none" => Err("WASI network access is disabled; set wasi_network_profile to `local` or `preview2` in caps.toml op policy".to_string()),
        "local" => {
            if matches!(scheme, "file" | "inproc")
                || (matches!(scheme, "http" | "https") && gc_registry::wasi_http_bridge_configured())
            {
                Ok(())
            } else {
                Err(format!(
                    "wasi_network_profile=local only allows file:// or inproc:// URLs (got scheme `{scheme}`)"
                ))
            }
        }
        "preview2" => Ok(()),
        other => Err(format!(
            "invalid wasi_network_profile `{other}`; expected `none`, `local`, or `preview2`"
        )),
    }
}

fn parse_bind_host_port(target: &str, op: &str, field: &str) -> Result<(String, u16), String> {
    let Some((_scheme, rest)) = target.split_once("://") else {
        return Err(format!(
            "{op} payload field `{field}` must include scheme:// (got `{target}`)"
        ));
    };
    let authority = rest.split('/').next().unwrap_or_default().trim();
    if authority.is_empty() {
        return Err(format!(
            "{op} payload field `{field}` must include bind host:port"
        ));
    }
    if let Some(stripped) = authority.strip_prefix('[') {
        let Some((host, port_part)) = stripped.split_once(']') else {
            return Err(format!(
                "{op} payload field `{field}` has invalid IPv6 bind authority `{authority}`"
            ));
        };
        let Some(port_raw) = port_part.strip_prefix(':') else {
            return Err(format!(
                "{op} payload field `{field}` must include bind port in authority `{authority}`"
            ));
        };
        let port = port_raw.parse::<u16>().map_err(|_| {
            format!("{op} payload field `{field}` has invalid bind port `{port_raw}`")
        })?;
        if host.trim().is_empty() {
            return Err(format!(
                "{op} payload field `{field}` has empty bind host in authority `{authority}`"
            ));
        }
        return Ok((host.trim().to_lowercase(), port));
    }
    let Some((host_raw, port_raw)) = authority.rsplit_once(':') else {
        return Err(format!(
            "{op} payload field `{field}` must include bind host:port in authority `{authority}`"
        ));
    };
    let host = host_raw.trim();
    if host.is_empty() {
        return Err(format!(
            "{op} payload field `{field}` has empty bind host in authority `{authority}`"
        ));
    }
    let port = port_raw
        .parse::<u16>()
        .map_err(|_| format!("{op} payload field `{field}` has invalid bind port `{port_raw}`"))?;
    Ok((host.to_lowercase(), port))
}

fn validate_net_bind_policy(
    pol: Option<&OpPolicy>,
    target: &str,
    op: &str,
    field: &str,
) -> Result<(), String> {
    let (bind_host, bind_port) = parse_bind_host_port(target, op, field)?;
    let allow_hosts = net_bind_hosts_from_policy(pol, op)?;
    let allow_ports = net_bind_ports_from_policy(pol, op)?;
    let host_ok = allow_hosts
        .iter()
        .any(|candidate| candidate.trim().eq_ignore_ascii_case(&bind_host));
    if !host_ok {
        return Err(format!(
            "bind host `{bind_host}` is not in allow_bind_hosts policy"
        ));
    }
    if !allow_ports.contains(&bind_port) {
        return Err(format!(
            "bind port `{bind_port}` is not in allow_bind_ports policy"
        ));
    }
    Ok(())
}

fn url_matches_allowlist(url: &str, allow: &str, scheme: &str) -> bool {
    let rule = allow.trim();
    if rule.ends_with("://") {
        return scheme == rule.trim_end_matches("://");
    }
    url.starts_with(rule)
}

fn validate_net_target_policy(
    pol: Option<&OpPolicy>,
    target: &str,
    op: &str,
    field: &str,
) -> Result<(), String> {
    let scheme = parse_url_scheme(target, op, field)?;
    let allow_http = net_allow_http_from_policy(pol)?;
    if scheme == "http" && !allow_http {
        return Err("http URLs are disabled by policy (set allow_http=true)".to_string());
    }
    let wasi_profile = net_wasi_network_profile_from_policy(pol)?;
    validate_net_wasi_profile(wasi_profile.as_deref(), scheme)?;
    let allowlist = net_allowlist_from_policy(pol, op)?;
    if allowlist
        .iter()
        .any(|rule| url_matches_allowlist(target, rule, scheme))
    {
        return Ok(());
    }
    Err("target is not in policy url_allow allowlist".to_string())
}

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
        Ok(resp) => Ok(Value::Data(resp)),
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
        Ok(resp) => Ok(Value::Data(resp)),
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
        Ok(resp) => Ok(Value::Data(resp)),
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
        Ok(resp) => Ok(Value::Data(resp)),
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
        Ok(resp) => Ok(Value::Data(resp)),
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
        Ok(resp) => Ok(Value::Data(resp)),
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
        Ok(resp) => Ok(Value::Data(resp)),
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
        Ok(resp) => Ok(Value::Data(resp)),
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
        Ok(resp) => Ok(Value::Data(resp)),
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
        Ok(resp) => Ok(Value::Data(resp)),
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
        Ok(resp) => Ok(Value::Data(resp)),
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
        Ok(resp) => Ok(Value::Data(resp)),
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
        Ok(resp) => Ok(Value::Data(resp)),
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
        Ok(resp) => Ok(Value::Data(resp)),
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
        Ok(resp) => Ok(Value::Data(resp)),
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
        Ok(resp) => Ok(Value::Data(resp)),
        Err(err) => Ok(mk_bridge_error(error_tok, &err, Some(op))),
    }
}
