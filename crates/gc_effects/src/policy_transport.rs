use std::collections::{BTreeMap, BTreeSet};
use std::path::PathBuf;

use crate::error::EffectsError;

use super::{CapsPolicy, OpPolicy};

const TOP_LEVEL_KEYS: [&str; 8] = [
    "allow", "log", "op", "refs", "runtime", "store", "task", "version",
];
const GENERIC_OPERATION_KEYS: [&str; 5] = [
    "allow",
    "base_dir",
    "create_dirs",
    "log_inline_max_bytes",
    "timeout_ms",
];

fn transport_error(message: impl Into<String>) -> EffectsError {
    EffectsError::Log(format!("caps.toml: transport: {}", message.into()))
}

fn empty_operation(extra: BTreeMap<String, toml::Value>) -> OpPolicy {
    OpPolicy {
        base_dir: None,
        create_dirs: false,
        timeout_ms: None,
        log_inline_max_bytes: None,
        extra,
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
        authorized_plugin: None,
        authorized_ffi: None,
        authorized_sync_credentials: None,
    }
}

fn require_table<'a>(
    document: &'a toml::value::Table,
    key: &str,
) -> Result<Option<&'a toml::value::Table>, EffectsError> {
    document
        .get(key)
        .map(|value| {
            value
                .as_table()
                .ok_or_else(|| transport_error(format!("{key} must be a table")))
        })
        .transpose()
}

fn require_integer_fields(
    table: Option<&toml::value::Table>,
    scope: &str,
    fields: &[&str],
) -> Result<(), EffectsError> {
    let Some(table) = table else {
        return Ok(());
    };
    for field in fields {
        if table.get(*field).is_some_and(|value| !value.is_integer()) {
            return Err(transport_error(format!(
                "{scope}.{field} must be an integer"
            )));
        }
    }
    Ok(())
}

fn retain_string(table: Option<&toml::value::Table>, key: &str) -> Option<String> {
    table
        .and_then(|table| table.get(key))
        .and_then(toml::Value::as_str)
        .map(str::to_string)
}

/// Decode syntax and host-mechanism material without deciding policy semantics.
///
/// GenesisCode receives the original TOML observations and replaces every
/// placeholder below before the policy can be used by a production entrypoint.
pub(super) fn decode_selfhost_transport(source: &str) -> Result<CapsPolicy, EffectsError> {
    let value: toml::Value =
        toml::from_str(source).map_err(|error| EffectsError::Log(format!("caps.toml: {error}")))?;
    let document = value
        .as_table()
        .ok_or_else(|| transport_error("top-level value must be a table"))?;

    let known: BTreeSet<&str> = TOP_LEVEL_KEYS.into_iter().collect();
    for key in document.keys() {
        if !known.contains(key.as_str()) {
            return Err(transport_error(format!("unknown top-level key `{key}`")));
        }
    }
    if document
        .get("version")
        .is_some_and(|value| !value.is_integer())
    {
        return Err(transport_error("version must be an integer"));
    }

    let log = require_table(document, "log")?;
    require_table(document, "refs")?;
    let runtime = require_table(document, "runtime")?;
    let store = require_table(document, "store")?;
    let task = require_table(document, "task")?;
    require_integer_fields(
        log,
        "log",
        &["inline_max_bytes", "max_artifact_bytes_per_run"],
    )?;
    require_integer_fields(store, "store", &["max_run_bytes"])?;
    require_integer_fields(
        runtime,
        "runtime",
        &[
            "max_effect_ops",
            "max_payload_bytes_per_op",
            "max_payload_bytes_per_run",
            "max_response_bytes_per_op",
            "max_response_bytes_per_run",
        ],
    )?;
    require_integer_fields(
        task,
        "task",
        &[
            "default_workers",
            "max_tasks",
            "max_workers",
            "max_queue",
            "max_steps_per_task",
            "max_time_ms_per_task",
        ],
    )?;

    let mut policy = CapsPolicy::empty();
    policy.store.auth_token = retain_string(store, "auth_token");
    policy.store.auth_token_env = retain_string(store, "auth_token_env");
    policy.store.basic_username = retain_string(store, "basic_username");
    policy.store.basic_password = retain_string(store, "basic_password");
    policy.store.basic_password_env = retain_string(store, "basic_password_env");
    policy.store.mtls_ca_pem = retain_string(store, "mtls_ca_pem").map(PathBuf::from);
    policy.store.mtls_identity_pem = retain_string(store, "mtls_identity_pem").map(PathBuf::from);
    policy.store.authorized_remote = None;
    policy.store.authorized_credentials = None;

    if let Some(allow) = document.get("allow") {
        let entries = allow
            .as_array()
            .ok_or_else(|| transport_error("allow must be an array"))?;
        for entry in entries {
            let op = entry
                .as_str()
                .ok_or_else(|| transport_error("allow entries must be strings"))?;
            policy
                .ops
                .entry(op.to_string())
                .or_insert_with(|| empty_operation(BTreeMap::new()));
        }
    }

    if let Some(operations) = require_table(document, "op")? {
        for (op, value) in operations {
            let table = value
                .as_table()
                .ok_or_else(|| transport_error(format!("op {op} config must be a table")))?;
            require_integer_fields(
                Some(table),
                &format!("op {op}"),
                &["timeout_ms", "log_inline_max_bytes"],
            )?;
            let extra = table
                .iter()
                .filter(|(key, _)| !GENERIC_OPERATION_KEYS.contains(&key.as_str()))
                .map(|(key, value)| (key.clone(), value.clone()))
                .collect();
            policy.ops.insert(op.clone(), empty_operation(extra));
        }
    }

    Ok(policy)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn candidate_transport_keeps_denied_overrides_and_opaque_host_fields() {
        let policy = decode_selfhost_transport(
            r#"
allow = ["io/fs::read"]

[op."core/sync::pull"]
allow = false
transfer_workers = 3
auth_token = "secret"
"#,
        )
        .unwrap();

        assert!(policy.ops.contains_key("io/fs::read"));
        let sync = policy.ops.get("core/sync::pull").unwrap();
        assert_eq!(
            sync.extra
                .get("transfer_workers")
                .and_then(toml::Value::as_integer),
            Some(3)
        );
        assert_eq!(
            sync.extra.get("auth_token").and_then(toml::Value::as_str),
            Some("secret")
        );
    }

    #[test]
    fn transport_rejects_structure_but_does_not_apply_allow_precedence() {
        decode_selfhost_transport(
            r#"
allow = ["io/fs::read"]
[op."io/fs::read"]
allow = false
"#,
        )
        .expect("policy decisions are not made by transport decoding");

        let error = decode_selfhost_transport("allow = [1]")
            .expect_err("transport shape errors must fail closed");
        assert!(error.to_string().contains("allow entries must be strings"));
    }
}
