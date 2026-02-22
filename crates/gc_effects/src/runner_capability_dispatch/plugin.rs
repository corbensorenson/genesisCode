use super::*;

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

pub(super) fn capability_host_plugin_command(
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
