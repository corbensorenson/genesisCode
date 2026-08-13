use super::*;
#[cfg(test)]
use crate::policy::AuthorizedPluginPolicy;
use crate::policy::AuthorizedStringList;

fn authorized_plugin_allowlist(
    state: &AuthorizedStringList,
    key: &str,
    missing: &str,
) -> Result<Vec<String>, String> {
    match state {
        AuthorizedStringList::Absent => Err(missing.to_string()),
        AuthorizedStringList::InvalidType => Err(format!("{key} must be an array of strings")),
        AuthorizedStringList::InvalidEntry => Err(format!("{key} entries must be strings")),
        AuthorizedStringList::Empty => Err(format!("{key} must contain at least one entry")),
        AuthorizedStringList::Valid(values) => Ok(values.clone()),
    }
}

fn plugin_allowlist_from_policy(pol: Option<&OpPolicy>, op: &str) -> Result<Vec<String>, String> {
    let missing = format!("{op} requires per-op allow_plugins allowlist in caps.toml");
    if let Some(plugin) = pol.and_then(|policy| policy.authorized_plugin.as_ref()) {
        return authorized_plugin_allowlist(&plugin.plugins, "allow_plugins", &missing);
    }
    parse_nonempty_string_array(pol, "allow_plugins", &missing)
}

fn plugin_command_allowlist_from_policy(
    pol: Option<&OpPolicy>,
    op: &str,
) -> Result<Vec<String>, String> {
    let missing = format!("{op} requires per-op allow_commands allowlist in caps.toml");
    if let Some(plugin) = pol.and_then(|policy| policy.authorized_plugin.as_ref()) {
        return authorized_plugin_allowlist(&plugin.commands, "allow_commands", &missing);
    }
    parse_nonempty_string_array(pol, "allow_commands", &missing)
}

fn plugin_schema_allowlist_from_policy(
    pol: Option<&OpPolicy>,
) -> Result<Option<Vec<String>>, String> {
    if let Some(plugin) = pol.and_then(|policy| policy.authorized_plugin.as_ref()) {
        return match &plugin.schema_ids {
            AuthorizedStringList::Absent => Ok(None),
            state => authorized_plugin_allowlist(
                state,
                "allow_schema_ids",
                "allow_schema_ids must be configured with at least one schema id",
            )
            .map(Some),
        };
    }
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

#[cfg(test)]
mod authority_tests {
    use super::*;
    use toml::Value as TomlValue;

    fn policy(plugin: AuthorizedPluginPolicy) -> OpPolicy {
        OpPolicy {
            base_dir: None,
            create_dirs: false,
            timeout_ms: None,
            log_inline_max_bytes: None,
            extra: BTreeMap::from([
                (
                    "allow_plugins".to_string(),
                    TomlValue::String("raw fallback must not be used".to_string()),
                ),
                (
                    "allow_commands".to_string(),
                    TomlValue::String("raw fallback must not be used".to_string()),
                ),
                (
                    "allow_schema_ids".to_string(),
                    TomlValue::String("raw fallback must not be used".to_string()),
                ),
            ]),
            authorized_cap: None,
            authorized_max_bytes: None,
            authorized_process_programs: None,
            authorized_database: None,
            authorized_network: None,
            authorized_crypto: None,
            authorized_gpu: None,
            authorized_gfx_profile: None,
            authorized_xr_policy: None,
            authorized_bridge_identity: None,
            authorized_plugin: Some(plugin),
            authorized_ffi: None,
            authorized_sync_credentials: None,
        }
    }

    #[test]
    fn plugin_dispatch_consumes_authorized_policy_before_raw_policy() {
        let policy = policy(AuthorizedPluginPolicy {
            plugins: AuthorizedStringList::Valid(vec!["demo".to_string()]),
            commands: AuthorizedStringList::Valid(vec!["run".to_string()]),
            schema_ids: AuthorizedStringList::Valid(vec!["schema.v1".to_string()]),
        });
        assert_eq!(
            plugin_allowlist_from_policy(Some(&policy), "host/plugin::command").unwrap(),
            vec!["demo"]
        );
        assert_eq!(
            plugin_command_allowlist_from_policy(Some(&policy), "host/plugin::command").unwrap(),
            vec!["run"]
        );
        assert_eq!(
            plugin_schema_allowlist_from_policy(Some(&policy)).unwrap(),
            Some(vec!["schema.v1".to_string()])
        );
    }

    #[test]
    fn plugin_dispatch_preserves_authorized_policy_errors_and_optional_schema() {
        let policy = policy(AuthorizedPluginPolicy {
            plugins: AuthorizedStringList::InvalidEntry,
            commands: AuthorizedStringList::Empty,
            schema_ids: AuthorizedStringList::Absent,
        });
        assert_eq!(
            plugin_allowlist_from_policy(Some(&policy), "host/plugin::command").unwrap_err(),
            "allow_plugins entries must be strings"
        );
        assert_eq!(
            plugin_command_allowlist_from_policy(Some(&policy), "host/plugin::command")
                .unwrap_err(),
            "allow_commands must contain at least one entry"
        );
        assert_eq!(
            plugin_schema_allowlist_from_policy(Some(&policy)).unwrap(),
            None
        );
    }
}

pub(super) fn capability_host_plugin_command(
    op: &str,
    bridge_runtime: &mut HostBridgeRuntime,
    payload: &Term,
    pol: Option<&OpPolicy>,
    error_tok: SealId,
) -> Result<Value, EffectsError> {
    let plugin = payload_required_string_or_symbol_field(payload, op, ":plugin")?;
    let command = payload_required_string_or_symbol_field(payload, op, ":command")?;
    let schema_ids = match parse_plugin_schema_ids(payload, op) {
        Ok(ids) => ids,
        Err(EffectsError::BadPayload(msg)) => {
            return Ok(mk_error(
                error_tok,
                "core/caps/payload-error",
                msg,
                Some(op),
            ));
        }
        Err(e) => return Err(e),
    };
    let allow_plugins = match plugin_allowlist_from_policy(pol, op) {
        Ok(v) => v,
        Err(e) => {
            return Ok(mk_error(error_tok, "core/caps/policy-error", e, Some(op)));
        }
    };
    if !allowlist_contains_exact_or_glob(&allow_plugins, &plugin) {
        return Ok(mk_error(
            error_tok,
            "core/caps/policy-error",
            format!(
                "{op} denied for plugin `{plugin}`; configure allow_plugins in caps.toml op policy"
            ),
            Some(op),
        ));
    }
    let allow_commands = match plugin_command_allowlist_from_policy(pol, op) {
        Ok(v) => v,
        Err(e) => {
            return Ok(mk_error(error_tok, "core/caps/policy-error", e, Some(op)));
        }
    };
    if !allowlist_contains_exact_or_glob(&allow_commands, &command) {
        return Ok(mk_error(
            error_tok,
            "core/caps/policy-error",
            format!(
                "{op} denied for command `{command}`; configure allow_commands in caps.toml op policy"
            ),
            Some(op),
        ));
    }
    let digest_pin_missing = match bridge_digest_pin_is_missing(pol) {
        Ok(missing) => missing,
        Err(message) => {
            return Ok(mk_error(
                error_tok,
                "core/caps/policy-error",
                message,
                Some(op),
            ));
        }
    };
    if digest_pin_missing {
        return Ok(mk_error(
            error_tok,
            "core/caps/policy-error",
            format!(
                "{op} requires bridge_cmd_sha256 digest pin when bridge_cmd transport is configured"
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
            && !allowlist_contains_exact_or_glob(&allow_schema_ids, schema_id)
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
            && !allowlist_contains_exact_or_glob(&allow_schema_ids, schema_id)
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
    match call_host_bridge(bridge_runtime, family, op, payload, pol) {
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
            Ok(Value::data(resp))
        }
        Err(err) => Ok(mk_bridge_error(error_tok, &err, Some(op))),
    }
}
