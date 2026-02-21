use super::*;
use crate::runner_host_bridge::{BridgeError, call_host_bridge};
use crate::runner_plugin_schema::{
    parse_plugin_schema_ids, validate_plugin_request_schema, validate_plugin_response_schema,
};

fn backend_unavailable_message(op: &str) -> String {
    if op.starts_with("gpu/compute::") || op.starts_with("gfx/gpu::") {
        return format!(
            "capability backend unavailable for {op}; configure per-op bridge policy \
(bridge_cmd/bridge_args/bridge_cmd_sha256) or enable a first-party runtime backend"
        );
    }
    if op.starts_with("gfx/window::")
        || op.starts_with("gfx/input::")
        || op.starts_with("gfx/audio::")
    {
        return format!(
            "capability backend unavailable for {op}; configure per-op bridge policy \
(bridge_cmd/bridge_args/bridge_cmd_sha256) for gfx host integration"
        );
    }
    if op.starts_with("editor/") {
        return format!(
            "capability backend unavailable for {op}; editor host integration is bridge-backed \
and requires explicit per-op bridge policy"
        );
    }
    if op.starts_with("host/plugin::") {
        return format!(
            "capability backend unavailable for {op}; configure per-op bridge policy \
(bridge_cmd/bridge_args/bridge_cmd_sha256 or WASI bridge profile) and plugin/command allowlists"
        );
    }
    if op.starts_with("io/net::") {
        return format!(
            "capability backend unavailable for {op}; configure per-op bridge policy \
(bridge_cmd/bridge_args/bridge_cmd_sha256 or WASI bridge profile) and remote allowlist policy"
        );
    }
    if op.starts_with("sys/process::") {
        return format!(
            "capability backend unavailable for {op}; configure per-op bridge policy \
(bridge_cmd/bridge_args/bridge_cmd_sha256 or WASI bridge profile) and explicit allow_programs"
        );
    }
    format!("capability backend unavailable for {op}")
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

fn mk_bridge_error(error_tok: SealId, err: &BridgeError, op: Option<&str>) -> Value {
    mk_error(error_tok, &err.code, err.message.clone(), op)
}

fn payload_required_string_field(
    payload: &Term,
    op: &str,
    key: &str,
) -> Result<String, EffectsError> {
    let Term::Map(mm) = payload else {
        return Err(EffectsError::BadPayload(format!(
            "{op} payload must be a map"
        )));
    };
    let Some(Term::Str(raw)) = mm.get(&TermOrdKey(Term::symbol(key))) else {
        return Err(EffectsError::BadPayload(format!(
            "{op} payload must contain string field `{key}`"
        )));
    };
    let trimmed = raw.trim();
    if trimmed.is_empty() {
        return Err(EffectsError::BadPayload(format!(
            "{op} payload field `{key}` must not be empty"
        )));
    }
    Ok(trimmed.to_string())
}

fn payload_required_field(payload: &Term, op: &str, key: &str) -> Result<Term, EffectsError> {
    let Term::Map(mm) = payload else {
        return Err(EffectsError::BadPayload(format!(
            "{op} payload must be a map"
        )));
    };
    let Some(value) = mm.get(&TermOrdKey(Term::symbol(key))) else {
        return Err(EffectsError::BadPayload(format!(
            "{op} payload must contain field `{key}`"
        )));
    };
    Ok(value.clone())
}

fn payload_optional_field(
    payload: &Term,
    op: &str,
    key: &str,
) -> Result<Option<Term>, EffectsError> {
    let Term::Map(mm) = payload else {
        return Err(EffectsError::BadPayload(format!(
            "{op} payload must be a map"
        )));
    };
    Ok(mm.get(&TermOrdKey(Term::symbol(key))).cloned())
}

fn payload_optional_bool_field(
    payload: &Term,
    op: &str,
    key: &str,
    default_value: bool,
) -> Result<bool, EffectsError> {
    let Term::Map(mm) = payload else {
        return Err(EffectsError::BadPayload(format!(
            "{op} payload must be a map"
        )));
    };
    let Some(value) = mm.get(&TermOrdKey(Term::symbol(key))) else {
        return Ok(default_value);
    };
    let Term::Bool(flag) = value else {
        return Err(EffectsError::BadPayload(format!(
            "{op} payload field `{key}` must be a bool"
        )));
    };
    Ok(*flag)
}

fn payload_required_string_or_symbol_field(
    payload: &Term,
    op: &str,
    key: &str,
) -> Result<String, EffectsError> {
    let Term::Map(mm) = payload else {
        return Err(EffectsError::BadPayload(format!(
            "{op} payload must be a map"
        )));
    };
    let Some(raw) = mm.get(&TermOrdKey(Term::symbol(key))) else {
        return Err(EffectsError::BadPayload(format!(
            "{op} payload must contain string/symbol field `{key}`"
        )));
    };
    let trimmed = match raw {
        Term::Str(s) | Term::Symbol(s) => s.trim(),
        _ => {
            return Err(EffectsError::BadPayload(format!(
                "{op} payload field `{key}` must be a string or symbol"
            )));
        }
    };
    if trimmed.is_empty() {
        return Err(EffectsError::BadPayload(format!(
            "{op} payload field `{key}` must not be empty"
        )));
    }
    Ok(trimmed.to_string())
}

fn parse_nonempty_string_array(
    pol: Option<&OpPolicy>,
    key: &str,
    missing_msg: &str,
) -> Result<Vec<String>, String> {
    let Some(pol) = pol else {
        return Err(missing_msg.to_string());
    };
    let Some(v) = pol.extra.get(key) else {
        return Err(missing_msg.to_string());
    };
    let Some(arr) = v.as_array() else {
        return Err(format!("{key} must be an array of strings"));
    };
    let mut out = Vec::with_capacity(arr.len());
    for x in arr {
        let Some(raw) = x.as_str() else {
            return Err(format!("{key} entries must be strings"));
        };
        let trimmed = raw.trim();
        if !trimmed.is_empty() {
            out.push(trimmed.to_string());
        }
    }
    if out.is_empty() {
        return Err(format!("{key} must contain at least one entry"));
    }
    Ok(out)
}

fn plugin_allowlist_from_policy(pol: Option<&OpPolicy>, op: &str) -> Result<Vec<String>, String> {
    parse_nonempty_string_array(
        pol,
        "allow_plugins",
        &format!("{op} requires per-op allow_plugins allowlist in caps.toml"),
    )
}

fn plugin_command_allowlist_from_policy(
    pol: Option<&OpPolicy>,
) -> Result<Option<Vec<String>>, String> {
    let Some(pol) = pol else {
        return Ok(None);
    };
    if !pol.extra.contains_key("allow_commands") {
        return Ok(None);
    }
    parse_nonempty_string_array(
        Some(pol),
        "allow_commands",
        "allow_commands must be configured with at least one command",
    )
    .map(Some)
}

fn plugin_schema_allowlist_from_policy(pol: Option<&OpPolicy>) -> Result<Option<Vec<String>>, String> {
    let Some(pol) = pol else {
        return Ok(None);
    };
    if !pol.extra.contains_key("allow_schema_ids") {
        return Ok(None);
    }
    parse_nonempty_string_array(
        Some(pol),
        "allow_schema_ids",
        "allow_schema_ids must be configured with at least one schema id",
    )
    .map(Some)
}

fn capability_host_plugin_command(
    op: &str,
    payload: &Term,
    pol: Option<&OpPolicy>,
    error_tok: SealId,
) -> Result<Value, EffectsError> {
    let plugin = payload_required_string_or_symbol_field(payload, op, ":plugin")?;
    let command = payload_required_string_or_symbol_field(payload, op, ":command")?;
    let schema_ids = parse_plugin_schema_ids(payload, op)?;
    let allow_plugins = match plugin_allowlist_from_policy(pol, op) {
        Ok(v) => v,
        Err(e) => {
            return Ok(mk_error(error_tok, "core/caps/policy-error", e, Some(op)));
        }
    };
    if !allow_plugins.iter().any(|allowed| allowed == &plugin) {
        return Ok(mk_error(
            error_tok,
            "core/caps/policy-error",
            format!(
                "{op} denied for plugin `{plugin}`; configure allow_plugins in caps.toml op policy"
            ),
            Some(op),
        ));
    }
    if let Some(allow_commands) = match plugin_command_allowlist_from_policy(pol) {
        Ok(v) => v,
        Err(e) => {
            return Ok(mk_error(error_tok, "core/caps/policy-error", e, Some(op)));
        }
    } && !allow_commands.iter().any(|allowed| allowed == &command)
    {
        return Ok(mk_error(
            error_tok,
            "core/caps/policy-error",
            format!(
                "{op} denied for command `{command}`; configure allow_commands in caps.toml op policy"
            ),
            Some(op),
        ));
    }
    if schema_ids.has_any() {
        let allow_schema_ids = match plugin_schema_allowlist_from_policy(pol) {
            Ok(Some(v)) => v,
            Ok(None) => {
                return Ok(mk_error(
                    error_tok,
                    "core/caps/policy-error",
                    format!(
                        "{op} typed plugin schemas require per-op allow_schema_ids allowlist in caps.toml"
                    ),
                    Some(op),
                ));
            }
            Err(e) => {
                return Ok(mk_error(error_tok, "core/caps/policy-error", e, Some(op)));
            }
        };
        if let Some(schema_id) = schema_ids.request_schema_id.as_deref()
            && !allow_schema_ids.iter().any(|allowed| allowed == schema_id)
        {
            return Ok(mk_error(
                error_tok,
                "core/caps/policy-error",
                format!(
                    "{op} denied request schema `{schema_id}`; configure allow_schema_ids in caps.toml op policy"
                ),
                Some(op),
            ));
        }
        if let Some(schema_id) = schema_ids.response_schema_id.as_deref()
            && !allow_schema_ids.iter().any(|allowed| allowed == schema_id)
        {
            return Ok(mk_error(
                error_tok,
                "core/caps/policy-error",
                format!(
                    "{op} denied response schema `{schema_id}`; configure allow_schema_ids in caps.toml op policy"
                ),
                Some(op),
            ));
        }
    }
    let plugin_payload = payload_optional_field(payload, op, ":payload")?.unwrap_or(Term::Nil);
    if let Some(schema_id) = schema_ids.request_schema_id.as_deref()
        && let Err(err) = validate_plugin_request_schema(schema_id, &plugin_payload, &plugin, &command)
    {
        return Ok(mk_error(
            error_tok,
            "core/caps/schema-error",
            format!("{op} request schema `{schema_id}` validation failed: {err}"),
            Some(op),
        ));
    }
    let family = if op.starts_with("editor/") {
        "editor"
    } else {
        "host/plugin"
    };
    match call_host_bridge(family, op, payload, pol) {
        Ok(resp) => {
            if let Some(schema_id) = schema_ids.response_schema_id.as_deref()
                && let Err(err) = validate_plugin_response_schema(schema_id, &resp)
            {
                return Ok(mk_error(
                    error_tok,
                    "core/caps/schema-error",
                    format!("{op} response schema `{schema_id}` validation failed: {err}"),
                    Some(op),
                ));
            }
            Ok(Value::Data(resp))
        }
        Err(err) => Ok(mk_bridge_error(error_tok, &err, Some(op))),
    }
}

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

fn parse_url_scheme<'a>(url: &'a str, op: &str, field: &str) -> Result<&'a str, String> {
    let Some((scheme, _rest)) = url.split_once("://") else {
        return Err(format!(
            "{op} payload field `{field}` must include scheme:// (got `{url}`)"
        ));
    };
    if scheme.trim().is_empty() {
        return Err(format!(
            "{op} payload field `{field}` scheme must not be empty"
        ));
    }
    Ok(scheme)
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

fn capability_io_net_http_request(
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

fn capability_io_net_ws_open(
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

fn capability_io_net_tcp_open(
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

fn capability_io_net_tcp_send(
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

fn capability_io_net_tcp_recv_or_close(
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

fn capability_io_net_udp_bind(
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

fn capability_io_net_udp_send(
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

fn capability_io_net_udp_recv_or_close(
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

fn capability_io_net_dns_resolve(
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

fn capability_io_net_ws_send(
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

fn capability_io_net_ws_recv_or_close(
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

fn fs_entry_kind(file_type: &std::fs::FileType) -> &'static str {
    if file_type.is_file() {
        "file"
    } else if file_type.is_dir() {
        "dir"
    } else if file_type.is_symlink() {
        "symlink"
    } else {
        "other"
    }
}

fn fs_rel_display_path(base_dir: &std::path::Path, path: &std::path::Path) -> String {
    path_to_slash(path.strip_prefix(base_dir).unwrap_or(path))
}

fn capability_io_fs_stat(
    op: &str,
    payload: &Term,
    pol: Option<&OpPolicy>,
    error_tok: SealId,
) -> Result<Value, EffectsError> {
    let path_s = payload_path(payload)?;
    let base_dir = effective_base_dir(pol)?;
    let path = sandbox_path_allow_missing(&base_dir, &path_s, false)?;
    let rel_path = fs_rel_display_path(&base_dir, &path);
    let md = match std::fs::symlink_metadata(&path) {
        Ok(md) => Some(md),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => None,
        Err(e) => {
            return Ok(Value::Sealed {
                token: error_tok,
                payload: Box::new(Value::Data(io_error_payload(op, &base_dir, &path, &e))),
            });
        }
    };

    let mut out = BTreeMap::new();
    out.insert(
        TermOrdKey(Term::symbol(":path")),
        Term::Str(rel_path.to_string()),
    );
    out.insert(
        TermOrdKey(Term::symbol(":exists")),
        Term::Bool(md.is_some()),
    );
    match md {
        Some(md) => {
            out.insert(
                TermOrdKey(Term::symbol(":kind")),
                Term::Symbol(fs_entry_kind(&md.file_type()).to_string()),
            );
            out.insert(
                TermOrdKey(Term::symbol(":len-bytes")),
                Term::Int((md.len() as i64).into()),
            );
            out.insert(
                TermOrdKey(Term::symbol(":readonly")),
                Term::Bool(md.permissions().readonly()),
            );
        }
        None => {
            out.insert(
                TermOrdKey(Term::symbol(":kind")),
                Term::Symbol("missing".to_string()),
            );
            out.insert(
                TermOrdKey(Term::symbol(":len-bytes")),
                Term::Int(0_i64.into()),
            );
            out.insert(TermOrdKey(Term::symbol(":readonly")), Term::Bool(false));
        }
    }
    Ok(Value::Data(Term::Map(out)))
}

fn capability_io_fs_list(
    op: &str,
    payload: &Term,
    pol: Option<&OpPolicy>,
    error_tok: SealId,
) -> Result<Value, EffectsError> {
    let path_s = payload_path(payload)?;
    let base_dir = effective_base_dir(pol)?;
    let path = sandbox_path_read(&base_dir, &path_s)?;
    let read_dir = match std::fs::read_dir(&path) {
        Ok(rd) => rd,
        Err(e) => {
            return Ok(Value::Sealed {
                token: error_tok,
                payload: Box::new(Value::Data(io_error_payload(op, &base_dir, &path, &e))),
            });
        }
    };

    let mut entries = Vec::new();
    for entry in read_dir {
        let entry = match entry {
            Ok(entry) => entry,
            Err(e) => {
                return Ok(Value::Sealed {
                    token: error_tok,
                    payload: Box::new(Value::Data(io_error_payload(op, &base_dir, &path, &e))),
                });
            }
        };
        let entry_path = entry.path();
        let entry_md = match entry.metadata() {
            Ok(md) => md,
            Err(e) => {
                return Ok(Value::Sealed {
                    token: error_tok,
                    payload: Box::new(Value::Data(io_error_payload(
                        op,
                        &base_dir,
                        &entry_path,
                        &e,
                    ))),
                });
            }
        };
        let name = entry.file_name().to_string_lossy().to_string();
        let mut row = BTreeMap::new();
        row.insert(TermOrdKey(Term::symbol(":name")), Term::Str(name));
        row.insert(
            TermOrdKey(Term::symbol(":path")),
            Term::Str(fs_rel_display_path(&base_dir, &entry_path)),
        );
        row.insert(
            TermOrdKey(Term::symbol(":kind")),
            Term::Symbol(fs_entry_kind(&entry_md.file_type()).to_string()),
        );
        row.insert(
            TermOrdKey(Term::symbol(":len-bytes")),
            Term::Int((entry_md.len() as i64).into()),
        );
        entries.push(Term::Map(row));
    }
    entries.sort_by_key(print_term);
    Ok(Value::Data(Term::Vector(entries)))
}

fn capability_io_fs_mkdir(
    op: &str,
    payload: &Term,
    pol: Option<&OpPolicy>,
    error_tok: SealId,
) -> Result<Value, EffectsError> {
    let path_s = payload_path(payload)?;
    let base_dir = effective_base_dir(pol)?;
    let create_parents = payload_optional_bool_field(payload, op, ":parents", true)?;
    let path = sandbox_path_allow_missing(&base_dir, &path_s, create_parents)?;
    let result = if create_parents {
        std::fs::create_dir_all(&path)
    } else {
        std::fs::create_dir(&path)
    };
    match result {
        Ok(()) => Ok(Value::Data(Term::Nil)),
        Err(e) => Ok(Value::Sealed {
            token: error_tok,
            payload: Box::new(Value::Data(io_error_payload(op, &base_dir, &path, &e))),
        }),
    }
}

fn capability_io_fs_remove(
    op: &str,
    payload: &Term,
    pol: Option<&OpPolicy>,
    error_tok: SealId,
) -> Result<Value, EffectsError> {
    let path_s = payload_path(payload)?;
    let base_dir = effective_base_dir(pol)?;
    let recursive = payload_optional_bool_field(payload, op, ":recursive", false)?;
    let path = sandbox_path_allow_missing(&base_dir, &path_s, false)?;
    let md = match std::fs::symlink_metadata(&path) {
        Ok(md) => Some(md),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => None,
        Err(e) => {
            return Ok(Value::Sealed {
                token: error_tok,
                payload: Box::new(Value::Data(io_error_payload(op, &base_dir, &path, &e))),
            });
        }
    };
    let Some(md) = md else {
        return Ok(Value::Data(Term::Nil));
    };
    let file_type = md.file_type();
    let result = if file_type.is_dir() && !file_type.is_symlink() {
        if recursive {
            std::fs::remove_dir_all(&path)
        } else {
            std::fs::remove_dir(&path)
        }
    } else {
        std::fs::remove_file(&path)
    };
    match result {
        Ok(()) => Ok(Value::Data(Term::Nil)),
        Err(e) => Ok(Value::Sealed {
            token: error_tok,
            payload: Box::new(Value::Data(io_error_payload(op, &base_dir, &path, &e))),
        }),
    }
}

fn capability_io_fs_rename(
    op: &str,
    payload: &Term,
    pol: Option<&OpPolicy>,
    error_tok: SealId,
) -> Result<Value, EffectsError> {
    let from_path = payload_required_string_field(payload, op, ":from")?;
    let to_path = payload_required_string_field(payload, op, ":to")?;
    let overwrite = payload_optional_bool_field(payload, op, ":overwrite", false)?;
    let base_dir = effective_base_dir(pol)?;
    let create_dirs = pol.is_some_and(|p| p.create_dirs);
    let from = sandbox_path_read(&base_dir, &from_path)?;
    let to = sandbox_path_allow_missing(&base_dir, &to_path, create_dirs)?;
    if !overwrite && to.exists() {
        return Ok(mk_error(
            error_tok,
            "core/caps/policy-error",
            format!(
                "{op} target `{}` already exists; set :overwrite true to allow replacing it",
                fs_rel_display_path(&base_dir, &to)
            ),
            Some(op),
        ));
    }
    let result = if overwrite && to.exists() {
        let md = std::fs::symlink_metadata(&to).map_err(|e| Value::Sealed {
            token: error_tok,
            payload: Box::new(Value::Data(io_error_payload(op, &base_dir, &to, &e))),
        });
        match md {
            Ok(md) => {
                let remove_result = if md.file_type().is_dir() && !md.file_type().is_symlink() {
                    std::fs::remove_dir_all(&to)
                } else {
                    std::fs::remove_file(&to)
                };
                if let Err(e) = remove_result {
                    return Ok(Value::Sealed {
                        token: error_tok,
                        payload: Box::new(Value::Data(io_error_payload(op, &base_dir, &to, &e))),
                    });
                }
                std::fs::rename(&from, &to)
            }
            Err(sealed) => return Ok(sealed),
        }
    } else {
        std::fs::rename(&from, &to)
    };
    match result {
        Ok(()) => Ok(Value::Data(Term::Nil)),
        Err(e) => Ok(Value::Sealed {
            token: error_tok,
            payload: Box::new(Value::Data(io_error_payload(op, &base_dir, &from, &e))),
        }),
    }
}

fn process_allow_programs_from_policy(
    pol: Option<&OpPolicy>,
    op: &str,
) -> Result<Vec<String>, String> {
    let Some(pol) = pol else {
        return Err(format!(
            "{op} requires per-op allow_programs allowlist in caps.toml"
        ));
    };
    let Some(v) = pol.extra.get("allow_programs") else {
        return Err(format!(
            "{op} requires per-op allow_programs allowlist in caps.toml"
        ));
    };
    let Some(arr) = v.as_array() else {
        return Err("allow_programs must be an array of strings".to_string());
    };
    let mut out = Vec::with_capacity(arr.len());
    for x in arr {
        let Some(raw) = x.as_str() else {
            return Err("allow_programs entries must be strings".to_string());
        };
        let trimmed = raw.trim();
        if !trimmed.is_empty() {
            out.push(trimmed.to_string());
        }
    }
    if out.is_empty() {
        return Err("allow_programs must contain at least one program name".to_string());
    }
    Ok(out)
}

fn capability_sys_process_spawn_or_exec(
    op: &str,
    payload: &Term,
    pol: Option<&OpPolicy>,
    error_tok: SealId,
) -> Result<Value, EffectsError> {
    let program = payload_required_string_field(payload, op, ":program")?;
    let allow_programs = match process_allow_programs_from_policy(pol, op) {
        Ok(v) => v,
        Err(e) => {
            return Ok(mk_error(error_tok, "core/caps/policy-error", e, Some(op)));
        }
    };
    if !allow_programs.iter().any(|allowed| allowed == &program) {
        return Ok(mk_error(
            error_tok,
            "core/caps/policy-error",
            format!(
                "sys/process::exec denied for program `{program}`; configure allow_programs in caps.toml op policy"
            ),
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
    match call_host_bridge("process", op, payload, pol) {
        Ok(resp) => Ok(Value::Data(resp)),
        Err(err) => Ok(mk_bridge_error(error_tok, &err, Some(op))),
    }
}

fn capability_sys_process_wait_or_kill(
    op: &str,
    payload: &Term,
    pol: Option<&OpPolicy>,
    error_tok: SealId,
) -> Result<Value, EffectsError> {
    let _process_id = payload_required_string_field(payload, op, ":process-id")?;
    if !has_explicit_bridge_profile(pol) {
        return Ok(mk_error(
            error_tok,
            "core/caps/backend-unavailable",
            backend_unavailable_message(op),
            Some(op),
        ));
    }
    match call_host_bridge("process", op, payload, pol) {
        Ok(resp) => Ok(Value::Data(resp)),
        Err(err) => Ok(mk_bridge_error(error_tok, &err, Some(op))),
    }
}

fn capability_sys_process_stdin_write(
    op: &str,
    payload: &Term,
    pol: Option<&OpPolicy>,
    error_tok: SealId,
) -> Result<Value, EffectsError> {
    let _process_id = payload_required_string_field(payload, op, ":process-id")?;
    let _data = payload_required_field(payload, op, ":data")?;
    if !has_explicit_bridge_profile(pol) {
        return Ok(mk_error(
            error_tok,
            "core/caps/backend-unavailable",
            backend_unavailable_message(op),
            Some(op),
        ));
    }
    match call_host_bridge("process", op, payload, pol) {
        Ok(resp) => Ok(Value::Data(resp)),
        Err(err) => Ok(mk_bridge_error(error_tok, &err, Some(op))),
    }
}

#[expect(
    clippy::too_many_arguments,
    reason = "central capability dispatcher forwards explicit runner context"
)]
pub(super) fn call_capability(
    op: &str,
    payload: &Term,
    pol: Option<&OpPolicy>,
    policy: &CapsPolicy,
    store: Option<&ArtifactStore>,
    refs: Option<&RefsDb>,
    budget: &mut ArtifactBudgetState,
    error_tok: SealId,
) -> Result<Value, EffectsError> {
    let op_eff = dispatch_op_alias(op);
    let timeout_ms = pol.and_then(|p| p.timeout_ms).filter(|ms| *ms > 0);
    if timeout_ms.is_some()
        && matches!(
            op_eff,
            "io/fs::write"
                | "io/fs::mkdir"
                | "io/fs::remove"
                | "io/fs::rename"
                | "sys/process::exec"
                | "sys/process::spawn"
                | "sys/process::kill"
                | "sys/process::stdin-write"
        )
    {
        return Ok(mk_error(
            error_tok,
            "core/caps/policy-error",
            format!("timeout_ms is not supported for {op_eff} (mutating op)"),
            Some(op),
        ));
    }
    match op_eff {
        "core/sync::pull" => capability_sync_pull(
            payload, pol, policy, store, refs, budget, error_tok, op, timeout_ms,
        ),

        "core/sync::push" => capability_sync_push(payload, pol, store, error_tok, op, timeout_ms),

        s if s.starts_with("core/pkg-low::") => capability_pkg_low(
            s, payload, pol, policy, store, refs, budget, error_tok, op, timeout_ms,
        ),
        "core/store::put" => cap_store_put(op, payload, pol, policy, store, budget, error_tok),
        "core/store::has" => cap_store_has(op, payload, pol, policy, store, timeout_ms, error_tok),
        "core/store::get" => cap_store_get(
            op, payload, pol, policy, store, budget, timeout_ms, error_tok,
        ),
        "core/store::verify" => cap_store_verify(op, payload, store, error_tok),
        s if s.starts_with("core/vcs-low::") => capability_vcs_low(
            s, payload, pol, policy, store, refs, budget, error_tok, op, timeout_ms,
        ),
        s if s.starts_with("core/gc-low::") || s.starts_with("core/gpk-low::") => {
            capability_gc_gpk_low(
                s, payload, pol, policy, store, refs, budget, error_tok, op, timeout_ms,
            )
        }
        "core/refs::get" => cap_refs_get(payload, refs),
        "core/refs::list" => cap_refs_list(payload, refs),
        "core/refs::set" => cap_refs_set(op, payload, store, refs, error_tok),
        "core/refs::delete" => cap_refs_delete(op, payload, store, refs, error_tok),
        "host/plugin::command" | "editor/plugin::command" => {
            capability_host_plugin_command(op, payload, pol, error_tok)
        }
        "io/net::http-request" => capability_io_net_http_request(op, payload, pol, error_tok),
        "io/net::dns-resolve" => capability_io_net_dns_resolve(op, payload, pol, error_tok),
        "io/net::tcp-open" => capability_io_net_tcp_open(op, payload, pol, error_tok),
        "io/net::tcp-send" => capability_io_net_tcp_send(op, payload, pol, error_tok),
        "io/net::tcp-recv" | "io/net::tcp-close" => {
            capability_io_net_tcp_recv_or_close(op, payload, pol, error_tok)
        }
        "io/net::udp-bind" => capability_io_net_udp_bind(op, payload, pol, error_tok),
        "io/net::udp-send" => capability_io_net_udp_send(op, payload, pol, error_tok),
        "io/net::udp-recv" | "io/net::udp-close" => {
            capability_io_net_udp_recv_or_close(op, payload, pol, error_tok)
        }
        "io/net::ws-open" => capability_io_net_ws_open(op, payload, pol, error_tok),
        "io/net::ws-send" => capability_io_net_ws_send(op, payload, pol, error_tok),
        "io/net::ws-recv" | "io/net::ws-close" => {
            capability_io_net_ws_recv_or_close(op, payload, pol, error_tok)
        }
        "sys/process::exec" | "sys/process::spawn" => {
            capability_sys_process_spawn_or_exec(op, payload, pol, error_tok)
        }
        "sys/process::wait"
        | "sys/process::kill"
        | "sys/process::stdout-read"
        | "sys/process::stderr-read" => {
            capability_sys_process_wait_or_kill(op, payload, pol, error_tok)
        }
        "sys/process::stdin-write" => {
            capability_sys_process_stdin_write(op, payload, pol, error_tok)
        }
        "sys/time::now" => {
            if let Some(ms) = timeout_ms {
                let r = with_timeout(ms, || {
                    Ok(std::time::SystemTime::now()
                        .duration_since(std::time::UNIX_EPOCH)
                        .unwrap_or_default()
                        .as_millis())
                })?;
                return Ok(match r {
                    Some(t) => Value::Data(Term::Int(BigInt::from(t))),
                    None => mk_error(
                        error_tok,
                        "core/caps/timeout",
                        format!("capability timed out after {ms}ms: sys/time::now"),
                        Some(op),
                    ),
                });
            }
            let t = std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap_or_default()
                .as_millis();
            Ok(Value::Data(Term::Int(BigInt::from(t))))
        }
        "gfx/time::frame-tick" => {
            if let Some(ms) = timeout_ms {
                let r = with_timeout(ms, || {
                    Ok(std::time::SystemTime::now()
                        .duration_since(std::time::UNIX_EPOCH)
                        .unwrap_or_default()
                        .as_millis())
                })?;
                return Ok(match r {
                    Some(t) => {
                        let mut m = BTreeMap::new();
                        m.insert(
                            TermOrdKey(Term::Symbol(":time-ms".to_string())),
                            Term::Int(BigInt::from(t)),
                        );
                        Value::Data(Term::Map(m))
                    }
                    None => mk_error(
                        error_tok,
                        "core/caps/timeout",
                        format!("capability timed out after {ms}ms: gfx/time::frame-tick"),
                        Some(op),
                    ),
                });
            }
            let t = std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap_or_default()
                .as_millis();
            let mut m = BTreeMap::new();
            m.insert(
                TermOrdKey(Term::Symbol(":time-ms".to_string())),
                Term::Int(BigInt::from(t)),
            );
            Ok(Value::Data(Term::Map(m)))
        }
        "io/fs::read" => {
            let path_s = payload_path(payload)?;
            let base_dir = effective_base_dir(pol)?;
            let max_read_bytes = match op_extra_positive_usize(pol, "max_bytes") {
                Ok(v) => v,
                Err(e) => {
                    return Ok(mk_error(error_tok, "core/caps/policy-error", e, Some(op)));
                }
            };
            if let Some(ms) = timeout_ms {
                let base_dir2 = base_dir.clone();
                let path_s2 = path_s.clone();
                let max_read_bytes2 = max_read_bytes;
                let r = with_timeout_cancellable(ms, move |cancel| {
                    let path = sandbox_path_read(&base_dir2, &path_s2)?;
                    let bytes =
                        read_file_with_optional_limit(&path, max_read_bytes2, Some(&cancel));
                    Ok((path, bytes))
                })?;
                return Ok(match r {
                    Some((_path, Ok(bytes))) => Value::Data(Term::Bytes(bytes.into())),
                    Some((path, Err(FsReadError::Io(e)))) => Value::Sealed {
                        token: error_tok,
                        payload: Box::new(Value::Data(io_error_payload(op, &base_dir, &path, &e))),
                    },
                    Some((path, Err(FsReadError::LimitExceeded { observed, limit }))) => {
                        mk_error_with_ctx(
                            error_tok,
                            "core/caps/resource-limit",
                            format!(
                                "file read exceeds configured limit ({observed} > {limit} bytes)"
                            ),
                            Some(op),
                            Term::Map(
                                [
                                    (
                                        TermOrdKey(Term::symbol(":path")),
                                        Term::Str(path_to_slash(
                                            path.strip_prefix(&base_dir).unwrap_or(&path),
                                        )),
                                    ),
                                    (
                                        TermOrdKey(Term::symbol(":limit-bytes")),
                                        Term::Int((limit as i64).into()),
                                    ),
                                ]
                                .into_iter()
                                .collect(),
                            ),
                        )
                    }
                    Some((_path, Err(FsReadError::Cancelled))) => mk_error(
                        error_tok,
                        "core/caps/timeout",
                        format!("capability timed out after {ms}ms: io/fs::read"),
                        Some(op),
                    ),
                    None => mk_error(
                        error_tok,
                        "core/caps/timeout",
                        format!("capability timed out after {ms}ms: io/fs::read"),
                        Some(op),
                    ),
                });
            }
            let path = sandbox_path_read(&base_dir, &path_s)?;
            match read_file_with_optional_limit(&path, max_read_bytes, None) {
                Ok(bytes) => Ok(Value::Data(Term::Bytes(bytes.into()))),
                Err(FsReadError::Io(e)) => Ok(Value::Sealed {
                    token: error_tok,
                    payload: Box::new(Value::Data(io_error_payload(op, &base_dir, &path, &e))),
                }),
                Err(FsReadError::LimitExceeded { observed, limit }) => Ok(mk_error_with_ctx(
                    error_tok,
                    "core/caps/resource-limit",
                    format!("file read exceeds configured limit ({observed} > {limit} bytes)"),
                    Some(op),
                    Term::Map(
                        [
                            (
                                TermOrdKey(Term::symbol(":path")),
                                Term::Str(path_to_slash(
                                    path.strip_prefix(&base_dir).unwrap_or(&path),
                                )),
                            ),
                            (
                                TermOrdKey(Term::symbol(":limit-bytes")),
                                Term::Int((limit as i64).into()),
                            ),
                        ]
                        .into_iter()
                        .collect(),
                    ),
                )),
                Err(FsReadError::Cancelled) => Ok(mk_error(
                    error_tok,
                    "core/caps/timeout",
                    "io/fs::read cancelled".to_string(),
                    Some(op),
                )),
            }
        }
        "io/fs::write" => {
            let path_s = payload_path(payload)?;
            let data = payload_data(payload)?;
            let base_dir = effective_base_dir(pol)?;
            let create_dirs = pol.is_some_and(|p| p.create_dirs);
            let path = sandbox_path_write(&base_dir, &path_s, create_dirs)?;
            match write_file_no_follow(&path, &data) {
                Ok(()) => Ok(Value::Data(Term::Nil)),
                Err(e) => Ok(Value::Sealed {
                    token: error_tok,
                    payload: Box::new(Value::Data(io_error_payload(op, &base_dir, &path, &e))),
                }),
            }
        }
        "io/fs::stat" => capability_io_fs_stat(op, payload, pol, error_tok),
        "io/fs::list" => capability_io_fs_list(op, payload, pol, error_tok),
        "io/fs::mkdir" => capability_io_fs_mkdir(op, payload, pol, error_tok),
        "io/fs::remove" => capability_io_fs_remove(op, payload, pol, error_tok),
        "io/fs::rename" => capability_io_fs_rename(op, payload, pol, error_tok),
        "gfx/gpu::create-buffer"
        | "gfx/gpu::create-texture"
        | "gfx/gpu::create-sampler"
        | "gfx/gpu::create-shader-module"
        | "gfx/gpu::create-bind-group-layout"
        | "gfx/gpu::create-bind-group"
        | "gfx/gpu::create-pipeline-layout"
        | "gfx/gpu::create-render-pipeline"
        | "gpu/compute::create-buffer"
        | "gpu/compute::create-shader-module"
        | "gpu/compute::create-bind-group-layout"
        | "gpu/compute::create-bind-group"
        | "gpu/compute::create-pipeline-layout"
        | "gpu/compute::create-compute-pipeline"
        | "gpu/compute::create-kernel"
        | "gfx/gpu::destroy-resource"
        | "gpu/compute::destroy-resource"
        | "gfx/gpu::write-buffer"
        | "gpu/compute::write-buffer"
        | "gfx/gpu::write-texture"
        | "gfx/gpu::read-buffer"
        | "gpu/compute::read-buffer"
        | "gfx/gpu::read-texture"
        | "gfx/gpu::submit-frame-graph"
        | "gpu/compute::submit"
        | "gfx/gpu::limits"
        | "gpu/compute::limits"
        | "gfx/gpu::features"
        | "gpu/compute::features"
        | "gfx/window::create-surface"
        | "gfx/window::resize-surface"
        | "gfx/window::set-title"
        | "gfx/window::request-redraw"
        | "gfx/window::surface-info"
        | "gfx/input::poll-events"
        | "gfx/input::set-cursor-mode"
        | "gfx/audio::enqueue"
        | "gfx/audio::set-master"
        | "editor/clipboard::get"
        | "editor/clipboard::set"
        | "editor/dialog::open"
        | "editor/dialog::save"
        | "editor/task::spawn"
        | "editor/task::fmt-coreform"
        | "editor/task::lint-module"
        | "editor/task::optimize-module"
        | "editor/task::parse-module"
        | "editor/task::poll"
        | "editor/task::cancel"
        | "editor/task::test-pkg"
        | "editor/task::typecheck-pkg"
        | "editor/watch::subscribe"
        | "editor/watch::poll"
        | "editor/watch::unsubscribe" => Ok(mk_error(
            error_tok,
            "core/caps/backend-unavailable",
            backend_unavailable_message(op),
            Some(op),
        )),
        _ => Ok(mk_error(
            error_tok,
            "core/caps/unknown-op",
            format!("unknown capability op: {op}"),
            Some(op),
        )),
    }
}

#[cfg(test)]
#[path = "runner_capability_dispatch_tests.rs"]
mod tests;
