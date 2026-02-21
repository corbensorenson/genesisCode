use super::*;
use crate::runner_host_bridge::{BridgeError, call_host_bridge};

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

fn capability_host_plugin_command(
    op: &str,
    payload: &Term,
    pol: Option<&OpPolicy>,
    error_tok: SealId,
) -> Result<Value, EffectsError> {
    let plugin = payload_required_string_or_symbol_field(payload, op, ":plugin")?;
    let command = payload_required_string_or_symbol_field(payload, op, ":command")?;
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
    let family = if op.starts_with("editor/") {
        "editor"
    } else {
        "host/plugin"
    };
    match call_host_bridge(family, op, payload, pol) {
        Ok(resp) => Ok(Value::Data(resp)),
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

fn process_allow_programs_from_policy(pol: Option<&OpPolicy>) -> Result<Vec<String>, String> {
    let Some(pol) = pol else {
        return Err(
            "sys/process::exec requires per-op allow_programs allowlist in caps.toml".to_string(),
        );
    };
    let Some(v) = pol.extra.get("allow_programs") else {
        return Err(
            "sys/process::exec requires per-op allow_programs allowlist in caps.toml".to_string(),
        );
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

fn capability_sys_process_exec(
    op: &str,
    payload: &Term,
    pol: Option<&OpPolicy>,
    error_tok: SealId,
) -> Result<Value, EffectsError> {
    let program = payload_required_string_field(payload, op, ":program")?;
    let allow_programs = match process_allow_programs_from_policy(pol) {
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
    if timeout_ms.is_some() && op_eff == "io/fs::write" {
        return Ok(mk_error(
            error_tok,
            "core/caps/policy-error",
            "timeout_ms is not supported for io/fs::write (mutating op)".to_string(),
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
        "sys/process::exec" => capability_sys_process_exec(op, payload, pol, error_tok),
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
mod tests {
    use super::{ArtifactBudgetState, backend_unavailable_message, call_capability};
    use crate::CapsPolicy;
    use gc_coreform::{Term, TermOrdKey};
    use gc_kernel::{SealId, Value};

    fn code_from_error(v: Value) -> String {
        let Value::Sealed { payload, .. } = v else {
            panic!("expected sealed error value");
        };
        let Value::Data(Term::Map(mm)) = *payload else {
            panic!("expected sealed payload map");
        };
        let Some(Term::Str(code)) = mm.get(&TermOrdKey(Term::symbol(":error/code"))) else {
            panic!("expected :error/code in payload map");
        };
        code.clone()
    }

    fn msg_from_error(v: Value) -> String {
        let Value::Sealed { payload, .. } = v else {
            panic!("expected sealed error value");
        };
        let Value::Data(Term::Map(mm)) = *payload else {
            panic!("expected sealed payload map");
        };
        let Some(Term::Str(msg)) = mm.get(&TermOrdKey(Term::symbol(":error/message"))) else {
            panic!("expected :error/message in payload map");
        };
        msg.clone()
    }

    #[test]
    fn stable_host_integrated_ops_report_backend_unavailable_actionably() {
        let policy =
            CapsPolicy::from_toml_str(r#"allow = ["editor/task::typecheck-pkg"]"#).expect("caps");
        let mut budget = ArtifactBudgetState::default();
        let out = call_capability(
            "editor/task::typecheck-pkg",
            &Term::Nil,
            policy.op_policy("editor/task::typecheck-pkg"),
            &policy,
            None,
            None,
            &mut budget,
            SealId(1),
        )
        .expect("call capability");
        assert_eq!(
            code_from_error(out.clone()),
            "core/caps/backend-unavailable"
        );
        let msg = msg_from_error(out);
        assert!(
            msg.contains("editor host integration is bridge-backed"),
            "message must be actionable for editor ops: {msg}"
        );
    }

    #[test]
    fn backend_unavailable_message_guides_gpu_compute_configuration() {
        let msg = backend_unavailable_message("gpu/compute::submit");
        assert!(msg.contains("bridge_cmd/bridge_args/bridge_cmd_sha256"));
        assert!(msg.contains("first-party runtime backend"));
    }

    #[test]
    fn io_net_http_request_policy_gate_enforces_remote_allowlist() {
        let policy = CapsPolicy::from_toml_str(
            r#"
allow = ["io/net::http-request"]

[op."io/net::http-request"]
url_allow = ["https://registry.example.com/api/"]
wasi_network_profile = "preview2"
wasi_bridge_profile = true
wasi_bridge_response = "{:ok true :status 200 :body \"ok\"}"
"#,
        )
        .expect("caps");
        let mut budget = ArtifactBudgetState::default();
        let payload = Term::Map(
            [(
                TermOrdKey(Term::symbol(":url")),
                Term::Str("https://evil.example.com/api/ping".to_string()),
            )]
            .into_iter()
            .collect(),
        );
        let out = call_capability(
            "io/net::http-request",
            &payload,
            policy.op_policy("io/net::http-request"),
            &policy,
            None,
            None,
            &mut budget,
            SealId(7),
        )
        .expect("call capability");
        assert_eq!(code_from_error(out), "core/caps/policy-error");
    }

    #[test]
    fn io_net_http_request_wasi_bridge_profile_returns_data() {
        let policy = CapsPolicy::from_toml_str(
            r#"
allow = ["io/net::http-request"]

[op."io/net::http-request"]
url_allow = ["https://registry.example.com/api/"]
wasi_network_profile = "preview2"
wasi_bridge_profile = true
wasi_bridge_response = "{:ok true :status 200 :body \"ok\"}"
"#,
        )
        .expect("caps");
        let mut budget = ArtifactBudgetState::default();
        let payload = Term::Map(
            [(
                TermOrdKey(Term::symbol(":url")),
                Term::Str("https://registry.example.com/api/ping".to_string()),
            )]
            .into_iter()
            .collect(),
        );
        let out = call_capability(
            "io/net::http-request",
            &payload,
            policy.op_policy("io/net::http-request"),
            &policy,
            None,
            None,
            &mut budget,
            SealId(9),
        )
        .expect("call capability");
        let Value::Data(Term::Map(mm)) = out else {
            panic!("expected data map");
        };
        assert_eq!(
            mm.get(&TermOrdKey(Term::symbol(":status"))),
            Some(&Term::Int(200_i64.into()))
        );
    }

    #[test]
    fn io_net_ws_open_policy_gate_enforces_remote_allowlist() {
        let policy = CapsPolicy::from_toml_str(
            r#"
allow = ["io/net::ws-open"]

[op."io/net::ws-open"]
url_allow = ["wss://realtime.example.com/ws/"]
wasi_network_profile = "preview2"
wasi_bridge_profile = true
wasi_bridge_response = "{:ok true :stream-id \"ws-1\"}"
"#,
        )
        .expect("caps");
        let mut budget = ArtifactBudgetState::default();
        let payload = Term::Map(
            [(
                TermOrdKey(Term::symbol(":url")),
                Term::Str("wss://evil.example.com/ws/room".to_string()),
            )]
            .into_iter()
            .collect(),
        );
        let out = call_capability(
            "io/net::ws-open",
            &payload,
            policy.op_policy("io/net::ws-open"),
            &policy,
            None,
            None,
            &mut budget,
            SealId(10),
        )
        .expect("call capability");
        assert_eq!(code_from_error(out), "core/caps/policy-error");
    }

    #[test]
    fn io_net_ws_family_wasi_bridge_profile_returns_data() {
        let policy = CapsPolicy::from_toml_str(
            r#"
allow = ["io/net::ws-open", "io/net::ws-send", "io/net::ws-recv", "io/net::ws-close"]

[op."io/net::ws-open"]
url_allow = ["wss://realtime.example.com/ws/"]
wasi_network_profile = "preview2"
wasi_bridge_profile = true
wasi_bridge_response = "{:ok true :stream-id \"ws-1\"}"

[op."io/net::ws-send"]
wasi_bridge_profile = true
wasi_bridge_response = "{:ok true :sent-bytes 5}"

[op."io/net::ws-recv"]
wasi_bridge_profile = true
wasi_bridge_response = "{:ok true :data b\"hello\" :eof false}"

[op."io/net::ws-close"]
wasi_bridge_profile = true
wasi_bridge_response = "{:ok true :closed true}"
"#,
        )
        .expect("caps");
        let mut budget = ArtifactBudgetState::default();
        let open_payload = Term::Map(
            [(
                TermOrdKey(Term::symbol(":url")),
                Term::Str("wss://realtime.example.com/ws/room".to_string()),
            )]
            .into_iter()
            .collect(),
        );
        let open_out = call_capability(
            "io/net::ws-open",
            &open_payload,
            policy.op_policy("io/net::ws-open"),
            &policy,
            None,
            None,
            &mut budget,
            SealId(12),
        )
        .expect("call capability");
        let Value::Data(Term::Map(open_map)) = open_out else {
            panic!("expected ws-open data map");
        };
        assert_eq!(
            open_map.get(&TermOrdKey(Term::symbol(":stream-id"))),
            Some(&Term::Str("ws-1".to_string()))
        );

        let send_payload = Term::Map(
            [
                (
                    TermOrdKey(Term::symbol(":stream-id")),
                    Term::Str("ws-1".to_string()),
                ),
                (
                    TermOrdKey(Term::symbol(":data")),
                    Term::Bytes(b"hello".to_vec().into()),
                ),
            ]
            .into_iter()
            .collect(),
        );
        let send_out = call_capability(
            "io/net::ws-send",
            &send_payload,
            policy.op_policy("io/net::ws-send"),
            &policy,
            None,
            None,
            &mut budget,
            SealId(14),
        )
        .expect("call capability");
        let Value::Data(Term::Map(send_map)) = send_out else {
            panic!("expected ws-send data map");
        };
        assert_eq!(
            send_map.get(&TermOrdKey(Term::symbol(":sent-bytes"))),
            Some(&Term::Int(5_i64.into()))
        );

        let recv_payload = Term::Map(
            [(
                TermOrdKey(Term::symbol(":stream-id")),
                Term::Str("ws-1".to_string()),
            )]
            .into_iter()
            .collect(),
        );
        let recv_out = call_capability(
            "io/net::ws-recv",
            &recv_payload,
            policy.op_policy("io/net::ws-recv"),
            &policy,
            None,
            None,
            &mut budget,
            SealId(16),
        )
        .expect("call capability");
        let Value::Data(Term::Map(recv_map)) = recv_out else {
            panic!("expected ws-recv data map");
        };
        assert_eq!(
            recv_map.get(&TermOrdKey(Term::symbol(":eof"))),
            Some(&Term::Bool(false))
        );

        let close_payload = Term::Map(
            [(
                TermOrdKey(Term::symbol(":stream-id")),
                Term::Str("ws-1".to_string()),
            )]
            .into_iter()
            .collect(),
        );
        let close_out = call_capability(
            "io/net::ws-close",
            &close_payload,
            policy.op_policy("io/net::ws-close"),
            &policy,
            None,
            None,
            &mut budget,
            SealId(18),
        )
        .expect("call capability");
        let Value::Data(Term::Map(close_map)) = close_out else {
            panic!("expected ws-close data map");
        };
        assert_eq!(
            close_map.get(&TermOrdKey(Term::symbol(":closed"))),
            Some(&Term::Bool(true))
        );
    }

    #[test]
    fn io_net_tcp_open_policy_gate_enforces_remote_allowlist() {
        let policy = CapsPolicy::from_toml_str(
            r#"
allow = ["io/net::tcp-open"]

[op."io/net::tcp-open"]
url_allow = ["tcp://allowed.example.com:443"]
wasi_network_profile = "preview2"
wasi_bridge_profile = true
wasi_bridge_response = "{:ok true :stream-id \"tcp-1\"}"
"#,
        )
        .expect("caps");
        let mut budget = ArtifactBudgetState::default();
        let payload = Term::Map(
            [(
                TermOrdKey(Term::symbol(":remote")),
                Term::Str("tcp://evil.example.com:443".to_string()),
            )]
            .into_iter()
            .collect(),
        );
        let out = call_capability(
            "io/net::tcp-open",
            &payload,
            policy.op_policy("io/net::tcp-open"),
            &policy,
            None,
            None,
            &mut budget,
            SealId(32),
        )
        .expect("call capability");
        assert_eq!(code_from_error(out), "core/caps/policy-error");
    }

    #[test]
    fn io_net_tcp_family_wasi_bridge_profile_returns_data() {
        let policy = CapsPolicy::from_toml_str(
            r#"
allow = ["io/net::tcp-open", "io/net::tcp-send", "io/net::tcp-recv", "io/net::tcp-close"]

[op."io/net::tcp-open"]
url_allow = ["tcp://allowed.example.com:443"]
wasi_network_profile = "preview2"
wasi_bridge_profile = true
wasi_bridge_response = "{:ok true :stream-id \"tcp-1\"}"

[op."io/net::tcp-send"]
wasi_bridge_profile = true
wasi_bridge_response = "{:ok true :sent-bytes 4}"

[op."io/net::tcp-recv"]
wasi_bridge_profile = true
wasi_bridge_response = "{:ok true :data b\"pong\" :eof false}"

[op."io/net::tcp-close"]
wasi_bridge_profile = true
wasi_bridge_response = "{:ok true :closed true}"
"#,
        )
        .expect("caps");
        let mut budget = ArtifactBudgetState::default();
        let open_payload = Term::Map(
            [(
                TermOrdKey(Term::symbol(":remote")),
                Term::Str("tcp://allowed.example.com:443".to_string()),
            )]
            .into_iter()
            .collect(),
        );
        let open_out = call_capability(
            "io/net::tcp-open",
            &open_payload,
            policy.op_policy("io/net::tcp-open"),
            &policy,
            None,
            None,
            &mut budget,
            SealId(34),
        )
        .expect("call capability");
        let Value::Data(Term::Map(open_map)) = open_out else {
            panic!("expected tcp-open data map");
        };
        assert_eq!(
            open_map.get(&TermOrdKey(Term::symbol(":stream-id"))),
            Some(&Term::Str("tcp-1".to_string()))
        );
        let send_payload = Term::Map(
            [
                (
                    TermOrdKey(Term::symbol(":stream-id")),
                    Term::Str("tcp-1".to_string()),
                ),
                (
                    TermOrdKey(Term::symbol(":data")),
                    Term::Bytes(b"ping".to_vec().into()),
                ),
            ]
            .into_iter()
            .collect(),
        );
        let send_out = call_capability(
            "io/net::tcp-send",
            &send_payload,
            policy.op_policy("io/net::tcp-send"),
            &policy,
            None,
            None,
            &mut budget,
            SealId(36),
        )
        .expect("call capability");
        let Value::Data(Term::Map(send_map)) = send_out else {
            panic!("expected tcp-send data map");
        };
        assert_eq!(
            send_map.get(&TermOrdKey(Term::symbol(":sent-bytes"))),
            Some(&Term::Int(4_i64.into()))
        );
        let recv_payload = Term::Map(
            [(
                TermOrdKey(Term::symbol(":stream-id")),
                Term::Str("tcp-1".to_string()),
            )]
            .into_iter()
            .collect(),
        );
        let recv_out = call_capability(
            "io/net::tcp-recv",
            &recv_payload,
            policy.op_policy("io/net::tcp-recv"),
            &policy,
            None,
            None,
            &mut budget,
            SealId(38),
        )
        .expect("call capability");
        let Value::Data(Term::Map(recv_map)) = recv_out else {
            panic!("expected tcp-recv data map");
        };
        assert_eq!(
            recv_map.get(&TermOrdKey(Term::symbol(":eof"))),
            Some(&Term::Bool(false))
        );
        let close_payload = Term::Map(
            [(
                TermOrdKey(Term::symbol(":stream-id")),
                Term::Str("tcp-1".to_string()),
            )]
            .into_iter()
            .collect(),
        );
        let close_out = call_capability(
            "io/net::tcp-close",
            &close_payload,
            policy.op_policy("io/net::tcp-close"),
            &policy,
            None,
            None,
            &mut budget,
            SealId(40),
        )
        .expect("call capability");
        let Value::Data(Term::Map(close_map)) = close_out else {
            panic!("expected tcp-close data map");
        };
        assert_eq!(
            close_map.get(&TermOrdKey(Term::symbol(":closed"))),
            Some(&Term::Bool(true))
        );
    }

    #[test]
    fn io_net_udp_bind_policy_gate_enforces_allowlist() {
        let policy = CapsPolicy::from_toml_str(
            r#"
allow = ["io/net::udp-bind"]

[op."io/net::udp-bind"]
url_allow = ["udp://127.0.0.1:5353"]
wasi_network_profile = "preview2"
wasi_bridge_profile = true
wasi_bridge_response = "{:ok true :socket-id \"udp-1\"}"
"#,
        )
        .expect("caps");
        let mut budget = ArtifactBudgetState::default();
        let payload = Term::Map(
            [(
                TermOrdKey(Term::symbol(":local")),
                Term::Str("udp://0.0.0.0:5353".to_string()),
            )]
            .into_iter()
            .collect(),
        );
        let out = call_capability(
            "io/net::udp-bind",
            &payload,
            policy.op_policy("io/net::udp-bind"),
            &policy,
            None,
            None,
            &mut budget,
            SealId(42),
        )
        .expect("call capability");
        assert_eq!(code_from_error(out), "core/caps/policy-error");
    }

    #[test]
    fn io_net_udp_family_wasi_bridge_profile_returns_data() {
        let policy = CapsPolicy::from_toml_str(
            r#"
allow = ["io/net::udp-bind", "io/net::udp-send", "io/net::udp-recv", "io/net::udp-close"]

[op."io/net::udp-bind"]
url_allow = ["udp://127.0.0.1:5353"]
wasi_network_profile = "preview2"
wasi_bridge_profile = true
wasi_bridge_response = "{:ok true :socket-id \"udp-1\"}"

[op."io/net::udp-send"]
url_allow = ["udp://127.0.0.1:5354"]
wasi_network_profile = "preview2"
wasi_bridge_profile = true
wasi_bridge_response = "{:ok true :sent-bytes 3}"

[op."io/net::udp-recv"]
wasi_bridge_profile = true
wasi_bridge_response = "{:ok true :remote \"udp://127.0.0.1:5354\" :data b\"ack\"}"

[op."io/net::udp-close"]
wasi_bridge_profile = true
wasi_bridge_response = "{:ok true :closed true}"
"#,
        )
        .expect("caps");
        let mut budget = ArtifactBudgetState::default();
        let bind_payload = Term::Map(
            [(
                TermOrdKey(Term::symbol(":local")),
                Term::Str("udp://127.0.0.1:5353".to_string()),
            )]
            .into_iter()
            .collect(),
        );
        let bind_out = call_capability(
            "io/net::udp-bind",
            &bind_payload,
            policy.op_policy("io/net::udp-bind"),
            &policy,
            None,
            None,
            &mut budget,
            SealId(44),
        )
        .expect("call capability");
        let Value::Data(Term::Map(bind_map)) = bind_out else {
            panic!("expected udp-bind data map");
        };
        assert_eq!(
            bind_map.get(&TermOrdKey(Term::symbol(":socket-id"))),
            Some(&Term::Str("udp-1".to_string()))
        );
        let send_payload = Term::Map(
            [
                (
                    TermOrdKey(Term::symbol(":socket-id")),
                    Term::Str("udp-1".to_string()),
                ),
                (
                    TermOrdKey(Term::symbol(":remote")),
                    Term::Str("udp://127.0.0.1:5354".to_string()),
                ),
                (
                    TermOrdKey(Term::symbol(":data")),
                    Term::Bytes(b"msg".to_vec().into()),
                ),
            ]
            .into_iter()
            .collect(),
        );
        let send_out = call_capability(
            "io/net::udp-send",
            &send_payload,
            policy.op_policy("io/net::udp-send"),
            &policy,
            None,
            None,
            &mut budget,
            SealId(46),
        )
        .expect("call capability");
        let Value::Data(Term::Map(send_map)) = send_out else {
            panic!("expected udp-send data map");
        };
        assert_eq!(
            send_map.get(&TermOrdKey(Term::symbol(":sent-bytes"))),
            Some(&Term::Int(3_i64.into()))
        );
        let recv_payload = Term::Map(
            [(
                TermOrdKey(Term::symbol(":socket-id")),
                Term::Str("udp-1".to_string()),
            )]
            .into_iter()
            .collect(),
        );
        let recv_out = call_capability(
            "io/net::udp-recv",
            &recv_payload,
            policy.op_policy("io/net::udp-recv"),
            &policy,
            None,
            None,
            &mut budget,
            SealId(48),
        )
        .expect("call capability");
        let Value::Data(Term::Map(recv_map)) = recv_out else {
            panic!("expected udp-recv data map");
        };
        assert_eq!(
            recv_map.get(&TermOrdKey(Term::symbol(":remote"))),
            Some(&Term::Str("udp://127.0.0.1:5354".to_string()))
        );
        let close_payload = Term::Map(
            [(
                TermOrdKey(Term::symbol(":socket-id")),
                Term::Str("udp-1".to_string()),
            )]
            .into_iter()
            .collect(),
        );
        let close_out = call_capability(
            "io/net::udp-close",
            &close_payload,
            policy.op_policy("io/net::udp-close"),
            &policy,
            None,
            None,
            &mut budget,
            SealId(50),
        )
        .expect("call capability");
        let Value::Data(Term::Map(close_map)) = close_out else {
            panic!("expected udp-close data map");
        };
        assert_eq!(
            close_map.get(&TermOrdKey(Term::symbol(":closed"))),
            Some(&Term::Bool(true))
        );
    }

    #[test]
    fn io_net_dns_resolve_policy_gate_enforces_allowlist() {
        let policy = CapsPolicy::from_toml_str(
            r#"
allow = ["io/net::dns-resolve"]

[op."io/net::dns-resolve"]
url_allow = ["dns://allowed.example.com"]
wasi_network_profile = "preview2"
wasi_bridge_profile = true
wasi_bridge_response = "{:ok true :records [{:type \"A\" :value \"127.0.0.1\"}]}"
"#,
        )
        .expect("caps");
        let mut budget = ArtifactBudgetState::default();
        let payload = Term::Map(
            [(
                TermOrdKey(Term::symbol(":name")),
                Term::Str("evil.example.com".to_string()),
            )]
            .into_iter()
            .collect(),
        );
        let out = call_capability(
            "io/net::dns-resolve",
            &payload,
            policy.op_policy("io/net::dns-resolve"),
            &policy,
            None,
            None,
            &mut budget,
            SealId(52),
        )
        .expect("call capability");
        assert_eq!(code_from_error(out), "core/caps/policy-error");
    }

    #[test]
    fn io_net_dns_resolve_wasi_bridge_profile_returns_data() {
        let policy = CapsPolicy::from_toml_str(
            r#"
allow = ["io/net::dns-resolve"]

[op."io/net::dns-resolve"]
url_allow = ["dns://allowed.example.com"]
wasi_network_profile = "preview2"
wasi_bridge_profile = true
wasi_bridge_response = "{:ok true :records [{:type \"A\" :value \"127.0.0.1\"}]}"
"#,
        )
        .expect("caps");
        let mut budget = ArtifactBudgetState::default();
        let payload = Term::Map(
            [(
                TermOrdKey(Term::symbol(":name")),
                Term::Str("allowed.example.com".to_string()),
            )]
            .into_iter()
            .collect(),
        );
        let out = call_capability(
            "io/net::dns-resolve",
            &payload,
            policy.op_policy("io/net::dns-resolve"),
            &policy,
            None,
            None,
            &mut budget,
            SealId(54),
        )
        .expect("call capability");
        let Value::Data(Term::Map(mm)) = out else {
            panic!("expected dns-resolve data map");
        };
        assert!(mm.contains_key(&TermOrdKey(Term::symbol(":records"))));
    }

    #[test]
    fn sys_process_exec_policy_gate_requires_allowlisted_program() {
        let policy = CapsPolicy::from_toml_str(
            r#"
allow = ["sys/process::exec"]

[op."sys/process::exec"]
allow_programs = ["gcpm"]
wasi_bridge_profile = true
wasi_bridge_response = "{:ok true :status 0}"
"#,
        )
        .expect("caps");
        let mut budget = ArtifactBudgetState::default();
        let payload = Term::Map(
            [(
                TermOrdKey(Term::symbol(":program")),
                Term::Str("bash".to_string()),
            )]
            .into_iter()
            .collect(),
        );
        let out = call_capability(
            "sys/process::exec",
            &payload,
            policy.op_policy("sys/process::exec"),
            &policy,
            None,
            None,
            &mut budget,
            SealId(11),
        )
        .expect("call capability");
        assert_eq!(code_from_error(out), "core/caps/policy-error");
    }

    #[test]
    fn sys_process_exec_wasi_bridge_profile_returns_data() {
        let policy = CapsPolicy::from_toml_str(
            r#"
allow = ["sys/process::exec"]

[op."sys/process::exec"]
allow_programs = ["gcpm"]
wasi_bridge_profile = true
wasi_bridge_response = "{:ok true :status 0 :stdout \"ready\"}"
"#,
        )
        .expect("caps");
        let mut budget = ArtifactBudgetState::default();
        let payload = Term::Map(
            [(
                TermOrdKey(Term::symbol(":program")),
                Term::Str("gcpm".to_string()),
            )]
            .into_iter()
            .collect(),
        );
        let out = call_capability(
            "sys/process::exec",
            &payload,
            policy.op_policy("sys/process::exec"),
            &policy,
            None,
            None,
            &mut budget,
            SealId(13),
        )
        .expect("call capability");
        let Value::Data(Term::Map(mm)) = out else {
            panic!("expected data map");
        };
        assert_eq!(
            mm.get(&TermOrdKey(Term::symbol(":status"))),
            Some(&Term::Int(0_i64.into()))
        );
    }

    #[test]
    fn host_plugin_policy_gate_requires_allowlisted_plugin() {
        let policy = CapsPolicy::from_toml_str(
            r#"
allow = ["host/plugin::command"]

[op."host/plugin::command"]
allow_plugins = ["demo"]
allow_commands = ["run"]
wasi_bridge_profile = true
wasi_bridge_response = "{:ok true}"
"#,
        )
        .expect("caps");
        let mut budget = ArtifactBudgetState::default();
        let payload = Term::Map(
            [
                (
                    TermOrdKey(Term::symbol(":plugin")),
                    Term::Str("other".to_string()),
                ),
                (
                    TermOrdKey(Term::symbol(":command")),
                    Term::Str("run".to_string()),
                ),
            ]
            .into_iter()
            .collect(),
        );
        let out = call_capability(
            "host/plugin::command",
            &payload,
            policy.op_policy("host/plugin::command"),
            &policy,
            None,
            None,
            &mut budget,
            SealId(17),
        )
        .expect("call capability");
        assert_eq!(code_from_error(out), "core/caps/policy-error");
    }

    #[test]
    fn host_plugin_wasi_bridge_profile_returns_data() {
        let policy = CapsPolicy::from_toml_str(
            r#"
allow = ["host/plugin::command"]

[op."host/plugin::command"]
allow_plugins = ["demo"]
allow_commands = ["run"]
wasi_bridge_profile = true
wasi_bridge_response = "{:ok true :status \"ok\"}"
"#,
        )
        .expect("caps");
        let mut budget = ArtifactBudgetState::default();
        let payload = Term::Map(
            [
                (
                    TermOrdKey(Term::symbol(":plugin")),
                    Term::Str("demo".to_string()),
                ),
                (
                    TermOrdKey(Term::symbol(":command")),
                    Term::Symbol("run".to_string()),
                ),
            ]
            .into_iter()
            .collect(),
        );
        let out = call_capability(
            "host/plugin::command",
            &payload,
            policy.op_policy("host/plugin::command"),
            &policy,
            None,
            None,
            &mut budget,
            SealId(19),
        )
        .expect("call capability");
        let Value::Data(Term::Map(mm)) = out else {
            panic!("expected data map");
        };
        assert_eq!(
            mm.get(&TermOrdKey(Term::symbol(":status"))),
            Some(&Term::Str("ok".to_string()))
        );
    }
}
