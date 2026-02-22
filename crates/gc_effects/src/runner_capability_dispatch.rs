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
    if op.starts_with("io/db::") {
        return format!(
            "capability backend unavailable for {op}; configure per-op bridge policy \
(bridge_cmd/bridge_args/bridge_cmd_sha256 or WASI bridge profile) and durable-data policy gates"
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

fn payload_required_map_field(
    payload: &Term,
    op: &str,
) -> Result<BTreeMap<TermOrdKey, Term>, EffectsError> {
    let Term::Map(mm) = payload else {
        return Err(EffectsError::BadPayload(format!(
            "{op} payload must be a map"
        )));
    };
    Ok(mm.clone())
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

fn plugin_schema_allowlist_from_policy(
    pol: Option<&OpPolicy>,
) -> Result<Option<Vec<String>>, String> {
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
        && let Err(err) =
            validate_plugin_request_schema(schema_id, &plugin_payload, &plugin, &command)
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
    if !allow_ports.iter().any(|candidate| *candidate == bind_port) {
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

fn capability_io_net_tcp_listen(
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

fn capability_io_net_tcp_accept(
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

fn capability_io_net_http_listen(
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

fn capability_io_net_http_respond(
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

fn capability_io_net_ws_accept(
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

fn db_target_allowlist_from_policy(
    pol: Option<&OpPolicy>,
    op: &str,
) -> Result<Vec<String>, String> {
    parse_nonempty_string_array(
        pol,
        "db_target_allow",
        &format!("{op} requires per-op db_target_allow allowlist in caps.toml"),
    )
}

fn db_query_class_allowlist_from_policy(
    pol: Option<&OpPolicy>,
    op: &str,
) -> Result<Vec<String>, String> {
    parse_nonempty_string_array(
        pol,
        "allow_query_classes",
        &format!("{op} requires per-op allow_query_classes allowlist in caps.toml"),
    )
}

fn db_positive_usize_from_policy(
    pol: Option<&OpPolicy>,
    op: &str,
    key: &str,
) -> Result<usize, String> {
    let Some(pol) = pol else {
        return Err(format!("{op} requires per-op {key} bound in caps.toml"));
    };
    let Some(v) = pol.extra.get(key) else {
        return Err(format!("{op} requires per-op {key} bound in caps.toml"));
    };
    let Some(raw) = v.as_integer() else {
        return Err(format!("{key} must be an integer"));
    };
    if raw <= 0 {
        return Err(format!("{key} must be greater than zero"));
    }
    usize::try_from(raw).map_err(|_| format!("{key} exceeds platform usize range"))
}

fn validate_db_target_policy(
    pol: Option<&OpPolicy>,
    target: &str,
    op: &str,
    field: &str,
) -> Result<(), String> {
    let _scheme = parse_url_scheme(target, op, field)?;
    let allowlist = db_target_allowlist_from_policy(pol, op)?;
    if allowlist.iter().any(|rule| target.starts_with(rule.trim())) {
        return Ok(());
    }
    Err("target is not in policy db_target_allow allowlist".to_string())
}

fn validate_db_query_class_policy(
    pol: Option<&OpPolicy>,
    op: &str,
    query_class: &str,
) -> Result<(), String> {
    let allowlist = db_query_class_allowlist_from_policy(pol, op)?;
    if allowlist
        .iter()
        .any(|allowed| allowed.trim().eq_ignore_ascii_case(query_class))
    {
        return Ok(());
    }
    Err(format!(
        "query class `{query_class}` is not in allow_query_classes policy"
    ))
}

fn capability_io_db_connect(
    op: &str,
    payload: &Term,
    pol: Option<&OpPolicy>,
    error_tok: SealId,
) -> Result<Value, EffectsError> {
    let target = payload_required_string_field(payload, op, ":target")?;
    if let Err(e) = validate_db_target_policy(pol, &target, op, ":target") {
        return Ok(mk_error(
            error_tok,
            "core/caps/policy-error",
            format!("{op} target denied: {e}"),
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
    match call_host_bridge("db", op, payload, pol) {
        Ok(resp) => Ok(Value::Data(resp)),
        Err(err) => Ok(mk_bridge_error(error_tok, &err, Some(op))),
    }
}

fn capability_io_db_tx_begin(
    op: &str,
    payload: &Term,
    pol: Option<&OpPolicy>,
    error_tok: SealId,
) -> Result<Value, EffectsError> {
    let _connection_id = payload_required_string_field(payload, op, ":connection-id")?;
    if !has_explicit_bridge_profile(pol) {
        return Ok(mk_error(
            error_tok,
            "core/caps/backend-unavailable",
            backend_unavailable_message(op),
            Some(op),
        ));
    }
    match call_host_bridge("db", op, payload, pol) {
        Ok(resp) => Ok(Value::Data(resp)),
        Err(err) => Ok(mk_bridge_error(error_tok, &err, Some(op))),
    }
}

fn capability_io_db_query_or_exec(
    op: &str,
    payload: &Term,
    pol: Option<&OpPolicy>,
    error_tok: SealId,
) -> Result<Value, EffectsError> {
    let _connection_id = payload_required_string_field(payload, op, ":connection-id")?;
    let query_class = payload_required_string_or_symbol_field(payload, op, ":query-class")?;
    if let Err(e) = validate_db_query_class_policy(pol, op, &query_class) {
        return Ok(mk_error(
            error_tok,
            "core/caps/policy-error",
            format!("{op} denied: {e}"),
            Some(op),
        ));
    }
    let max_result_bytes = match db_positive_usize_from_policy(pol, op, "max_result_bytes") {
        Ok(v) => v,
        Err(e) => {
            return Ok(mk_error(error_tok, "core/caps/policy-error", e, Some(op)));
        }
    };
    let mut payload_map = payload_required_map_field(payload, op)?;
    payload_map.insert(
        TermOrdKey(Term::symbol(":max-result-bytes")),
        Term::Int((max_result_bytes as i64).into()),
    );
    if op == "io/db::query" {
        let max_row_count = match db_positive_usize_from_policy(pol, op, "max_row_count") {
            Ok(v) => v,
            Err(e) => {
                return Ok(mk_error(error_tok, "core/caps/policy-error", e, Some(op)));
            }
        };
        payload_map.insert(
            TermOrdKey(Term::symbol(":max-row-count")),
            Term::Int((max_row_count as i64).into()),
        );
    }
    let effective_payload = Term::Map(payload_map);
    if !has_explicit_bridge_profile(pol) {
        return Ok(mk_error(
            error_tok,
            "core/caps/backend-unavailable",
            backend_unavailable_message(op),
            Some(op),
        ));
    }
    match call_host_bridge("db", op, &effective_payload, pol) {
        Ok(resp) => Ok(Value::Data(resp)),
        Err(err) => Ok(mk_bridge_error(error_tok, &err, Some(op))),
    }
}

fn capability_io_db_tx_finish(
    op: &str,
    payload: &Term,
    pol: Option<&OpPolicy>,
    error_tok: SealId,
) -> Result<Value, EffectsError> {
    let _tx_id = payload_required_string_field(payload, op, ":tx-id")?;
    if !has_explicit_bridge_profile(pol) {
        return Ok(mk_error(
            error_tok,
            "core/caps/backend-unavailable",
            backend_unavailable_message(op),
            Some(op),
        ));
    }
    match call_host_bridge("db", op, payload, pol) {
        Ok(resp) => Ok(Value::Data(resp)),
        Err(err) => Ok(mk_bridge_error(error_tok, &err, Some(op))),
    }
}

fn capability_io_db_kv_open(
    op: &str,
    payload: &Term,
    pol: Option<&OpPolicy>,
    error_tok: SealId,
) -> Result<Value, EffectsError> {
    let target = payload_required_string_field(payload, op, ":target")?;
    if let Err(e) = validate_db_target_policy(pol, &target, op, ":target") {
        return Ok(mk_error(
            error_tok,
            "core/caps/policy-error",
            format!("{op} target denied: {e}"),
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
    match call_host_bridge("db", op, payload, pol) {
        Ok(resp) => Ok(Value::Data(resp)),
        Err(err) => Ok(mk_bridge_error(error_tok, &err, Some(op))),
    }
}

fn capability_io_db_kv_get(
    op: &str,
    payload: &Term,
    pol: Option<&OpPolicy>,
    error_tok: SealId,
) -> Result<Value, EffectsError> {
    let _store_id = payload_required_string_field(payload, op, ":store-id")?;
    let _key = payload_required_string_field(payload, op, ":key")?;
    let max_result_bytes = match db_positive_usize_from_policy(pol, op, "max_result_bytes") {
        Ok(v) => v,
        Err(e) => {
            return Ok(mk_error(error_tok, "core/caps/policy-error", e, Some(op)));
        }
    };
    let mut payload_map = payload_required_map_field(payload, op)?;
    payload_map.insert(
        TermOrdKey(Term::symbol(":max-result-bytes")),
        Term::Int((max_result_bytes as i64).into()),
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
    match call_host_bridge("db", op, &effective_payload, pol) {
        Ok(resp) => Ok(Value::Data(resp)),
        Err(err) => Ok(mk_bridge_error(error_tok, &err, Some(op))),
    }
}

fn capability_io_db_kv_put(
    op: &str,
    payload: &Term,
    pol: Option<&OpPolicy>,
    error_tok: SealId,
) -> Result<Value, EffectsError> {
    let _store_id = payload_required_string_field(payload, op, ":store-id")?;
    let _key = payload_required_string_field(payload, op, ":key")?;
    let _value = payload_required_field(payload, op, ":value")?;
    let max_value_bytes = match db_positive_usize_from_policy(pol, op, "max_value_bytes") {
        Ok(v) => v,
        Err(e) => {
            return Ok(mk_error(error_tok, "core/caps/policy-error", e, Some(op)));
        }
    };
    let mut payload_map = payload_required_map_field(payload, op)?;
    payload_map.insert(
        TermOrdKey(Term::symbol(":max-value-bytes")),
        Term::Int((max_value_bytes as i64).into()),
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
    match call_host_bridge("db", op, &effective_payload, pol) {
        Ok(resp) => Ok(Value::Data(resp)),
        Err(err) => Ok(mk_bridge_error(error_tok, &err, Some(op))),
    }
}

fn capability_io_db_kv_delete(
    op: &str,
    payload: &Term,
    pol: Option<&OpPolicy>,
    error_tok: SealId,
) -> Result<Value, EffectsError> {
    let _store_id = payload_required_string_field(payload, op, ":store-id")?;
    let _key = payload_required_string_field(payload, op, ":key")?;
    if !has_explicit_bridge_profile(pol) {
        return Ok(mk_error(
            error_tok,
            "core/caps/backend-unavailable",
            backend_unavailable_message(op),
            Some(op),
        ));
    }
    match call_host_bridge("db", op, payload, pol) {
        Ok(resp) => Ok(Value::Data(resp)),
        Err(err) => Ok(mk_bridge_error(error_tok, &err, Some(op))),
    }
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

fn payload_error_value(error_tok: SealId, op: &str, err: EffectsError) -> Value {
    mk_error(
        error_tok,
        "core/caps/payload-error",
        err.to_string(),
        Some(op),
    )
}

fn payload_required_positive_usize_field(
    payload: &Term,
    op: &str,
    key: &str,
) -> Result<usize, EffectsError> {
    let value = payload_required_field(payload, op, key)?;
    let Term::Int(n) = value else {
        return Err(EffectsError::BadPayload(format!(
            "{op} payload field `{key}` must be an int"
        )));
    };
    let Some(as_i64) = n.to_i64() else {
        return Err(EffectsError::BadPayload(format!(
            "{op} payload field `{key}` must fit i64"
        )));
    };
    if as_i64 <= 0 {
        return Err(EffectsError::BadPayload(format!(
            "{op} payload field `{key}` must be > 0"
        )));
    }
    usize::try_from(as_i64).map_err(|_| {
        EffectsError::BadPayload(format!(
            "{op} payload field `{key}` exceeds platform usize range"
        ))
    })
}

fn media_policy_allowlist(
    pol: Option<&OpPolicy>,
    key: &str,
    default: &[&str],
) -> Result<Vec<String>, String> {
    let Some(pol) = pol else {
        return Ok(default.iter().map(|x| x.to_string()).collect());
    };
    let Some(v) = pol.extra.get(key) else {
        return Ok(default.iter().map(|x| x.to_string()).collect());
    };
    let Some(arr) = v.as_array() else {
        return Err(format!("{key} must be an array of strings"));
    };
    let mut out = Vec::with_capacity(arr.len());
    for item in arr {
        let normalized = match item {
            toml::Value::String(s) => s.trim().to_string(),
            _ => {
                return Err(format!("{key} entries must be strings"));
            }
        };
        if !normalized.is_empty() {
            out.push(normalized.to_ascii_lowercase());
        }
    }
    if out.is_empty() {
        return Err(format!("{key} must contain at least one format"));
    }
    out.sort();
    out.dedup();
    Ok(out)
}

fn media_policy_positive_usize(
    pol: Option<&OpPolicy>,
    key: &str,
    default_value: usize,
) -> Result<usize, String> {
    match op_extra_positive_usize(pol, key)? {
        Some(value) => Ok(value),
        None => Ok(default_value),
    }
}

fn media_policy_contains(allowlist: &[String], value: &str) -> bool {
    allowlist.iter().any(|allowed| allowed == value)
}

fn media_hash_response(
    op: &str,
    data: &[u8],
    kind: Option<&str>,
    algorithm: &str,
) -> Value {
    let mut response = BTreeMap::new();
    response.insert(TermOrdKey(Term::symbol(":ok")), Term::Bool(true));
    response.insert(
        TermOrdKey(Term::symbol(":op")),
        Term::Str(op.to_string()),
    );
    response.insert(
        TermOrdKey(Term::symbol(":algorithm")),
        Term::Str(algorithm.to_string()),
    );
    response.insert(
        TermOrdKey(Term::symbol(":hash")),
        Term::Str(blake3::hash(data).to_hex().to_string()),
    );
    response.insert(
        TermOrdKey(Term::symbol(":bytes")),
        Term::Int((data.len() as i64).into()),
    );
    if let Some(kind) = kind {
        response.insert(
            TermOrdKey(Term::symbol(":kind")),
            Term::Str(kind.to_string()),
        );
    }
    Value::Data(Term::Map(response))
}

fn capability_core_media_asset_hash(
    op: &str,
    payload: &Term,
    pol: Option<&OpPolicy>,
    error_tok: SealId,
) -> Result<Value, EffectsError> {
    let data = match payload_data(payload) {
        Ok(bytes) => bytes,
        Err(err) => return Ok(payload_error_value(error_tok, op, err)),
    };
    let max_input_bytes = match media_policy_positive_usize(pol, "max_input_bytes", 16 * 1024 * 1024)
    {
        Ok(v) => v,
        Err(e) => return Ok(mk_error(error_tok, "core/caps/policy-error", e, Some(op))),
    };
    if data.len() > max_input_bytes {
        return Ok(mk_resource_limit_error(
            error_tok,
            op,
            "media input bytes",
            data.len(),
            max_input_bytes,
        ));
    }
    let algorithm = match payload_optional_field(payload, op, ":algorithm") {
        Ok(Some(Term::Str(s) | Term::Symbol(s))) => s.trim().to_ascii_lowercase(),
        Ok(Some(_)) => {
            return Ok(mk_error(
                error_tok,
                "core/caps/payload-error",
                format!("{op} payload field `:algorithm` must be string/symbol"),
                Some(op),
            ));
        }
        Ok(None) => "blake3".to_string(),
        Err(err) => return Ok(payload_error_value(error_tok, op, err)),
    };
    if algorithm != "blake3" {
        return Ok(mk_error(
            error_tok,
            "core/caps/policy-error",
            format!("{op} only supports algorithm `blake3`"),
            Some(op),
        ));
    }
    let kind = match payload_optional_field(payload, op, ":kind") {
        Ok(Some(Term::Str(s) | Term::Symbol(s))) => Some(s.trim().to_ascii_lowercase()),
        Ok(Some(_)) => {
            return Ok(mk_error(
                error_tok,
                "core/caps/payload-error",
                format!("{op} payload field `:kind` must be string/symbol"),
                Some(op),
            ));
        }
        Ok(None) => None,
        Err(err) => return Ok(payload_error_value(error_tok, op, err)),
    };
    Ok(media_hash_response(op, &data, kind.as_deref(), &algorithm))
}

fn capability_core_media_image_transcode(
    op: &str,
    payload: &Term,
    pol: Option<&OpPolicy>,
    error_tok: SealId,
) -> Result<Value, EffectsError> {
    let data = match payload_data(payload) {
        Ok(bytes) => bytes,
        Err(err) => return Ok(payload_error_value(error_tok, op, err)),
    };
    let source_format = match payload_required_string_or_symbol_field(payload, op, ":source-format") {
        Ok(value) => value.to_ascii_lowercase(),
        Err(err) => return Ok(payload_error_value(error_tok, op, err)),
    };
    let target_format = match payload_required_string_or_symbol_field(payload, op, ":target-format") {
        Ok(value) => value.to_ascii_lowercase(),
        Err(err) => return Ok(payload_error_value(error_tok, op, err)),
    };
    let width = match payload_required_positive_usize_field(payload, op, ":width") {
        Ok(value) => value,
        Err(err) => return Ok(payload_error_value(error_tok, op, err)),
    };
    let height = match payload_required_positive_usize_field(payload, op, ":height") {
        Ok(value) => value,
        Err(err) => return Ok(payload_error_value(error_tok, op, err)),
    };

    let max_input_bytes = match media_policy_positive_usize(pol, "max_input_bytes", 16 * 1024 * 1024)
    {
        Ok(v) => v,
        Err(e) => return Ok(mk_error(error_tok, "core/caps/policy-error", e, Some(op))),
    };
    if data.len() > max_input_bytes {
        return Ok(mk_resource_limit_error(
            error_tok,
            op,
            "media input bytes",
            data.len(),
            max_input_bytes,
        ));
    }

    let max_output_bytes =
        match media_policy_positive_usize(pol, "max_output_bytes", 32 * 1024 * 1024) {
            Ok(v) => v,
            Err(e) => return Ok(mk_error(error_tok, "core/caps/policy-error", e, Some(op))),
        };
    let max_pixels = match media_policy_positive_usize(pol, "max_pixels", 8_388_608) {
        Ok(v) => v,
        Err(e) => return Ok(mk_error(error_tok, "core/caps/policy-error", e, Some(op))),
    };
    let allow_source = match media_policy_allowlist(pol, "allow_source_formats", &["rgba8", "gray8"])
    {
        Ok(v) => v,
        Err(e) => return Ok(mk_error(error_tok, "core/caps/policy-error", e, Some(op))),
    };
    let allow_target = match media_policy_allowlist(pol, "allow_target_formats", &["rgba8", "gray8"])
    {
        Ok(v) => v,
        Err(e) => return Ok(mk_error(error_tok, "core/caps/policy-error", e, Some(op))),
    };
    if !media_policy_contains(&allow_source, &source_format) {
        return Ok(mk_error(
            error_tok,
            "core/caps/policy-error",
            format!("{op} source format `{source_format}` is not allowlisted"),
            Some(op),
        ));
    }
    if !media_policy_contains(&allow_target, &target_format) {
        return Ok(mk_error(
            error_tok,
            "core/caps/policy-error",
            format!("{op} target format `{target_format}` is not allowlisted"),
            Some(op),
        ));
    }

    let Some(pixel_count) = width.checked_mul(height) else {
        return Ok(mk_error(
            error_tok,
            "core/caps/resource-limit",
            format!("{op} pixel count overflow for width={width} height={height}"),
            Some(op),
        ));
    };
    if pixel_count > max_pixels {
        return Ok(mk_error(
            error_tok,
            "core/caps/resource-limit",
            format!(
                "{op} pixel count exceeds policy limit ({pixel_count} > {max_pixels})"
            ),
            Some(op),
        ));
    }

    let src_channels = match source_format.as_str() {
        "rgba8" => 4usize,
        "gray8" => 1usize,
        _ => {
            return Ok(mk_error(
                error_tok,
                "core/caps/payload-error",
                format!("{op} unsupported source format `{source_format}`"),
                Some(op),
            ));
        }
    };
    let dst_channels = match target_format.as_str() {
        "rgba8" => 4usize,
        "gray8" => 1usize,
        _ => {
            return Ok(mk_error(
                error_tok,
                "core/caps/payload-error",
                format!("{op} unsupported target format `{target_format}`"),
                Some(op),
            ));
        }
    };

    let Some(expected_input_len) = pixel_count.checked_mul(src_channels) else {
        return Ok(mk_error(
            error_tok,
            "core/caps/resource-limit",
            format!("{op} input length overflow"),
            Some(op),
        ));
    };
    if data.len() != expected_input_len {
        return Ok(mk_error(
            error_tok,
            "core/caps/payload-error",
            format!(
                "{op} input bytes mismatch: expected {expected_input_len}, got {}",
                data.len()
            ),
            Some(op),
        ));
    }

    let mut output = if source_format == target_format {
        data.clone()
    } else if source_format == "rgba8" && target_format == "gray8" {
        let mut out = Vec::with_capacity(pixel_count);
        for px in data.chunks_exact(4) {
            let r = px[0] as u16;
            let g = px[1] as u16;
            let b = px[2] as u16;
            let gray = ((77 * r + 150 * g + 29 * b + 128) >> 8) as u8;
            out.push(gray);
        }
        out
    } else if source_format == "gray8" && target_format == "rgba8" {
        let mut out = Vec::with_capacity(pixel_count.saturating_mul(4));
        for gray in &data {
            out.extend_from_slice(&[*gray, *gray, *gray, 255u8]);
        }
        out
    } else {
        return Ok(mk_error(
            error_tok,
            "core/caps/payload-error",
            format!(
                "{op} unsupported conversion `{source_format}` -> `{target_format}`"
            ),
            Some(op),
        ));
    };

    let Some(expected_output_len) = pixel_count.checked_mul(dst_channels) else {
        return Ok(mk_error(
            error_tok,
            "core/caps/resource-limit",
            format!("{op} output length overflow"),
            Some(op),
        ));
    };
    if output.len() != expected_output_len {
        return Ok(mk_error(
            error_tok,
            "core/caps/resource-limit",
            format!(
                "{op} output byte mismatch: expected {expected_output_len}, got {}",
                output.len()
            ),
            Some(op),
        ));
    }
    if output.len() > max_output_bytes {
        return Ok(mk_resource_limit_error(
            error_tok,
            op,
            "media output bytes",
            output.len(),
            max_output_bytes,
        ));
    }

    let mut response = BTreeMap::new();
    response.insert(TermOrdKey(Term::symbol(":ok")), Term::Bool(true));
    response.insert(
        TermOrdKey(Term::symbol(":kind")),
        Term::Str("image".to_string()),
    );
    response.insert(
        TermOrdKey(Term::symbol(":source-format")),
        Term::Str(source_format),
    );
    response.insert(
        TermOrdKey(Term::symbol(":target-format")),
        Term::Str(target_format),
    );
    response.insert(
        TermOrdKey(Term::symbol(":width")),
        Term::Int((width as i64).into()),
    );
    response.insert(
        TermOrdKey(Term::symbol(":height")),
        Term::Int((height as i64).into()),
    );
    response.insert(
        TermOrdKey(Term::symbol(":input-bytes")),
        Term::Int((data.len() as i64).into()),
    );
    response.insert(
        TermOrdKey(Term::symbol(":output-bytes")),
        Term::Int((output.len() as i64).into()),
    );
    response.insert(
        TermOrdKey(Term::symbol(":hash")),
        Term::Str(blake3::hash(&output).to_hex().to_string()),
    );
    response.insert(
        TermOrdKey(Term::symbol(":data")),
        Term::Bytes(std::mem::take(&mut output).into()),
    );
    Ok(Value::Data(Term::Map(response)))
}

fn capability_core_media_audio_transcode(
    op: &str,
    payload: &Term,
    pol: Option<&OpPolicy>,
    error_tok: SealId,
) -> Result<Value, EffectsError> {
    let data = match payload_data(payload) {
        Ok(bytes) => bytes,
        Err(err) => return Ok(payload_error_value(error_tok, op, err)),
    };
    let source_format = match payload_required_string_or_symbol_field(payload, op, ":source-format") {
        Ok(value) => value.to_ascii_lowercase(),
        Err(err) => return Ok(payload_error_value(error_tok, op, err)),
    };
    let target_format = match payload_required_string_or_symbol_field(payload, op, ":target-format") {
        Ok(value) => value.to_ascii_lowercase(),
        Err(err) => return Ok(payload_error_value(error_tok, op, err)),
    };
    let channels = match payload_required_positive_usize_field(payload, op, ":channels") {
        Ok(v) => v,
        Err(err) => return Ok(payload_error_value(error_tok, op, err)),
    };
    let sample_rate = match payload_required_positive_usize_field(payload, op, ":sample-rate") {
        Ok(v) => v,
        Err(err) => return Ok(payload_error_value(error_tok, op, err)),
    };

    let max_input_bytes = match media_policy_positive_usize(pol, "max_input_bytes", 16 * 1024 * 1024)
    {
        Ok(v) => v,
        Err(e) => return Ok(mk_error(error_tok, "core/caps/policy-error", e, Some(op))),
    };
    if data.len() > max_input_bytes {
        return Ok(mk_resource_limit_error(
            error_tok,
            op,
            "media input bytes",
            data.len(),
            max_input_bytes,
        ));
    }
    if channels > 8 {
        return Ok(mk_error(
            error_tok,
            "core/caps/resource-limit",
            format!("{op} channels exceeds limit ({} > 8)", channels),
            Some(op),
        ));
    }

    let max_frames = match media_policy_positive_usize(pol, "max_frames", 2_000_000) {
        Ok(v) => v,
        Err(e) => return Ok(mk_error(error_tok, "core/caps/policy-error", e, Some(op))),
    };
    let max_output_bytes =
        match media_policy_positive_usize(pol, "max_output_bytes", 32 * 1024 * 1024) {
            Ok(v) => v,
            Err(e) => return Ok(mk_error(error_tok, "core/caps/policy-error", e, Some(op))),
        };
    let min_sample_rate = match media_policy_positive_usize(pol, "min_sample_rate", 8_000) {
        Ok(v) => v,
        Err(e) => return Ok(mk_error(error_tok, "core/caps/policy-error", e, Some(op))),
    };
    let max_sample_rate = match media_policy_positive_usize(pol, "max_sample_rate", 192_000) {
        Ok(v) => v,
        Err(e) => return Ok(mk_error(error_tok, "core/caps/policy-error", e, Some(op))),
    };
    if sample_rate < min_sample_rate || sample_rate > max_sample_rate {
        return Ok(mk_error(
            error_tok,
            "core/caps/policy-error",
            format!(
                "{op} sample-rate {} outside policy bounds [{}, {}]",
                sample_rate, min_sample_rate, max_sample_rate
            ),
            Some(op),
        ));
    }

    let allow_source = match media_policy_allowlist(pol, "allow_source_formats", &["pcm-s16le", "pcm-f32le"])
    {
        Ok(v) => v,
        Err(e) => return Ok(mk_error(error_tok, "core/caps/policy-error", e, Some(op))),
    };
    let allow_target = match media_policy_allowlist(pol, "allow_target_formats", &["pcm-s16le", "pcm-f32le"])
    {
        Ok(v) => v,
        Err(e) => return Ok(mk_error(error_tok, "core/caps/policy-error", e, Some(op))),
    };
    if !media_policy_contains(&allow_source, &source_format) {
        return Ok(mk_error(
            error_tok,
            "core/caps/policy-error",
            format!("{op} source format `{source_format}` is not allowlisted"),
            Some(op),
        ));
    }
    if !media_policy_contains(&allow_target, &target_format) {
        return Ok(mk_error(
            error_tok,
            "core/caps/policy-error",
            format!("{op} target format `{target_format}` is not allowlisted"),
            Some(op),
        ));
    }

    let source_sample_bytes = match source_format.as_str() {
        "pcm-s16le" => 2usize,
        "pcm-f32le" => 4usize,
        _ => {
            return Ok(mk_error(
                error_tok,
                "core/caps/payload-error",
                format!("{op} unsupported source format `{source_format}`"),
                Some(op),
            ));
        }
    };
    let target_sample_bytes = match target_format.as_str() {
        "pcm-s16le" => 2usize,
        "pcm-f32le" => 4usize,
        _ => {
            return Ok(mk_error(
                error_tok,
                "core/caps/payload-error",
                format!("{op} unsupported target format `{target_format}`"),
                Some(op),
            ));
        }
    };

    let Some(input_frame_bytes) = source_sample_bytes.checked_mul(channels) else {
        return Ok(mk_error(
            error_tok,
            "core/caps/resource-limit",
            format!("{op} input frame-size overflow"),
            Some(op),
        ));
    };
    if data.len() % input_frame_bytes != 0 {
        return Ok(mk_error(
            error_tok,
            "core/caps/payload-error",
            format!(
                "{op} input bytes ({}) not aligned to frame size {}",
                data.len(),
                input_frame_bytes
            ),
            Some(op),
        ));
    }
    let frames = data.len() / input_frame_bytes;
    if frames > max_frames {
        return Ok(mk_error(
            error_tok,
            "core/caps/resource-limit",
            format!("{op} frame count exceeds policy limit ({frames} > {max_frames})"),
            Some(op),
        ));
    }

    let mut output = Vec::new();
    if source_format == target_format {
        output = data.clone();
    } else if source_format == "pcm-s16le" && target_format == "pcm-f32le" {
        output.reserve(frames.saturating_mul(channels).saturating_mul(4));
        for chunk in data.chunks_exact(2) {
            let sample = i16::from_le_bytes([chunk[0], chunk[1]]) as f32 / 32768.0;
            output.extend_from_slice(&sample.to_le_bytes());
        }
    } else if source_format == "pcm-f32le" && target_format == "pcm-s16le" {
        output.reserve(frames.saturating_mul(channels).saturating_mul(2));
        for chunk in data.chunks_exact(4) {
            let sample = f32::from_le_bytes([chunk[0], chunk[1], chunk[2], chunk[3]]);
            if !sample.is_finite() {
                return Ok(mk_error(
                    error_tok,
                    "core/caps/payload-error",
                    format!("{op} input contains non-finite pcm-f32 sample"),
                    Some(op),
                ));
            }
            let scaled = (sample.clamp(-1.0, 1.0) * 32767.0).round() as i16;
            output.extend_from_slice(&scaled.to_le_bytes());
        }
    } else {
        return Ok(mk_error(
            error_tok,
            "core/caps/payload-error",
            format!(
                "{op} unsupported conversion `{source_format}` -> `{target_format}`"
            ),
            Some(op),
        ));
    }

    let Some(expected_output) = frames
        .checked_mul(channels)
        .and_then(|n| n.checked_mul(target_sample_bytes))
    else {
        return Ok(mk_error(
            error_tok,
            "core/caps/resource-limit",
            format!("{op} output frame-size overflow"),
            Some(op),
        ));
    };
    if output.len() != expected_output {
        return Ok(mk_error(
            error_tok,
            "core/caps/resource-limit",
            format!(
                "{op} output bytes mismatch: expected {expected_output}, got {}",
                output.len()
            ),
            Some(op),
        ));
    }
    if output.len() > max_output_bytes {
        return Ok(mk_resource_limit_error(
            error_tok,
            op,
            "media output bytes",
            output.len(),
            max_output_bytes,
        ));
    }

    let mut response = BTreeMap::new();
    response.insert(TermOrdKey(Term::symbol(":ok")), Term::Bool(true));
    response.insert(
        TermOrdKey(Term::symbol(":kind")),
        Term::Str("audio".to_string()),
    );
    response.insert(
        TermOrdKey(Term::symbol(":source-format")),
        Term::Str(source_format),
    );
    response.insert(
        TermOrdKey(Term::symbol(":target-format")),
        Term::Str(target_format),
    );
    response.insert(
        TermOrdKey(Term::symbol(":channels")),
        Term::Int((channels as i64).into()),
    );
    response.insert(
        TermOrdKey(Term::symbol(":sample-rate")),
        Term::Int((sample_rate as i64).into()),
    );
    response.insert(
        TermOrdKey(Term::symbol(":frames")),
        Term::Int((frames as i64).into()),
    );
    response.insert(
        TermOrdKey(Term::symbol(":input-bytes")),
        Term::Int((data.len() as i64).into()),
    );
    response.insert(
        TermOrdKey(Term::symbol(":output-bytes")),
        Term::Int((output.len() as i64).into()),
    );
    response.insert(
        TermOrdKey(Term::symbol(":hash")),
        Term::Str(blake3::hash(&output).to_hex().to_string()),
    );
    response.insert(
        TermOrdKey(Term::symbol(":data")),
        Term::Bytes(output.into()),
    );
    Ok(Value::Data(Term::Map(response)))
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
        "core/media::asset-hash" => capability_core_media_asset_hash(op, payload, pol, error_tok),
        "core/media::image-transcode" => {
            capability_core_media_image_transcode(op, payload, pol, error_tok)
        }
        "core/media::audio-transcode" => {
            capability_core_media_audio_transcode(op, payload, pol, error_tok)
        }
        "host/plugin::command" | "editor/plugin::command" => {
            capability_host_plugin_command(op, payload, pol, error_tok)
        }
        "io/net::http-request" => capability_io_net_http_request(op, payload, pol, error_tok),
        "io/net::dns-resolve" => capability_io_net_dns_resolve(op, payload, pol, error_tok),
        "io/net::tcp-listen" => capability_io_net_tcp_listen(op, payload, pol, error_tok),
        "io/net::tcp-accept" => capability_io_net_tcp_accept(op, payload, pol, error_tok),
        "io/net::tcp-open" => capability_io_net_tcp_open(op, payload, pol, error_tok),
        "io/net::tcp-send" => capability_io_net_tcp_send(op, payload, pol, error_tok),
        "io/net::tcp-recv" | "io/net::tcp-close" => {
            capability_io_net_tcp_recv_or_close(op, payload, pol, error_tok)
        }
        "io/net::http-listen" => capability_io_net_http_listen(op, payload, pol, error_tok),
        "io/net::http-respond" => capability_io_net_http_respond(op, payload, pol, error_tok),
        "io/net::udp-bind" => capability_io_net_udp_bind(op, payload, pol, error_tok),
        "io/net::udp-send" => capability_io_net_udp_send(op, payload, pol, error_tok),
        "io/net::udp-recv" | "io/net::udp-close" => {
            capability_io_net_udp_recv_or_close(op, payload, pol, error_tok)
        }
        "io/net::ws-open" => capability_io_net_ws_open(op, payload, pol, error_tok),
        "io/net::ws-accept" => capability_io_net_ws_accept(op, payload, pol, error_tok),
        "io/net::ws-send" => capability_io_net_ws_send(op, payload, pol, error_tok),
        "io/net::ws-recv" | "io/net::ws-close" => {
            capability_io_net_ws_recv_or_close(op, payload, pol, error_tok)
        }
        "io/db::connect" => capability_io_db_connect(op, payload, pol, error_tok),
        "io/db::tx-begin" => capability_io_db_tx_begin(op, payload, pol, error_tok),
        "io/db::query" | "io/db::exec" => {
            capability_io_db_query_or_exec(op, payload, pol, error_tok)
        }
        "io/db::tx-commit" | "io/db::tx-rollback" => {
            capability_io_db_tx_finish(op, payload, pol, error_tok)
        }
        "io/db::kv-open" => capability_io_db_kv_open(op, payload, pol, error_tok),
        "io/db::kv-get" => capability_io_db_kv_get(op, payload, pol, error_tok),
        "io/db::kv-put" => capability_io_db_kv_put(op, payload, pol, error_tok),
        "io/db::kv-delete" => capability_io_db_kv_delete(op, payload, pol, error_tok),
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
