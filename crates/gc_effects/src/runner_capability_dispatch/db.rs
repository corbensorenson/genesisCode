use super::*;
use crate::policy::{AuthorizedMaxBytes, AuthorizedStringList};

fn authorized_string_list(
    state: &AuthorizedStringList,
    key: &str,
    missing_msg: &str,
) -> Result<Vec<String>, String> {
    match state {
        AuthorizedStringList::Absent => Err(missing_msg.to_string()),
        AuthorizedStringList::InvalidType => Err(format!("{key} must be an array of strings")),
        AuthorizedStringList::InvalidEntry => Err(format!("{key} entries must be strings")),
        AuthorizedStringList::Empty => Err(format!("{key} must contain at least one entry")),
        AuthorizedStringList::Valid(values) => Ok(values.clone()),
    }
}

fn authorized_positive_usize(state: &AuthorizedMaxBytes, key: &str) -> Result<usize, String> {
    match state {
        AuthorizedMaxBytes::Absent => Err(format!("missing {key}")),
        AuthorizedMaxBytes::InvalidType => Err(format!("{key} must be an integer")),
        AuthorizedMaxBytes::NonPositive => Err(format!("{key} must be greater than zero")),
        AuthorizedMaxBytes::PlatformOverflow => Err(format!("{key} exceeds platform usize range")),
        AuthorizedMaxBytes::Valid(value) => Ok(*value),
    }
}

fn db_target_allowlist_from_policy(
    pol: Option<&OpPolicy>,
    op: &str,
) -> Result<Vec<String>, String> {
    let missing = format!("{op} requires per-op db_target_allow allowlist in caps.toml");
    if let Some(authorized) = pol.and_then(|policy| policy.authorized_database.as_ref()) {
        return authorized_string_list(&authorized.target_allow, "db_target_allow", &missing);
    }
    parse_nonempty_string_array(pol, "db_target_allow", &missing)
}

fn db_query_class_allowlist_from_policy(
    pol: Option<&OpPolicy>,
    op: &str,
) -> Result<Vec<String>, String> {
    let missing = format!("{op} requires per-op allow_query_classes allowlist in caps.toml");
    if let Some(authorized) = pol.and_then(|policy| policy.authorized_database.as_ref()) {
        return authorized_string_list(&authorized.query_classes, "allow_query_classes", &missing);
    }
    parse_nonempty_string_array(pol, "allow_query_classes", &missing)
}

fn db_positive_usize_from_policy(
    pol: Option<&OpPolicy>,
    op: &str,
    key: &str,
) -> Result<usize, String> {
    let Some(pol) = pol else {
        return Err(format!("{op} requires per-op {key} bound in caps.toml"));
    };
    if let Some(authorized) = &pol.authorized_database {
        let state = match key {
            "max_result_bytes" => &authorized.max_result_bytes,
            "max_row_count" => &authorized.max_row_count,
            "max_value_bytes" => &authorized.max_value_bytes,
            _ => return Err(format!("unknown database policy bound {key}")),
        };
        return authorized_positive_usize(state, key).map_err(|error| {
            if matches!(state, AuthorizedMaxBytes::Absent) {
                format!("{op} requires per-op {key} bound in caps.toml")
            } else {
                error
            }
        });
    }
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

#[cfg(test)]
mod tests {
    use std::collections::BTreeMap;

    use toml::Value as TomlValue;

    use super::*;
    use crate::policy::AuthorizedDatabasePolicy;

    fn policy_with(key: &str, raw: TomlValue, authorized: AuthorizedDatabasePolicy) -> OpPolicy {
        OpPolicy {
            base_dir: None,
            create_dirs: false,
            timeout_ms: None,
            log_inline_max_bytes: None,
            extra: BTreeMap::from([(key.to_string(), raw)]),
            authorized_cap: None,
            authorized_max_bytes: None,
            authorized_process_programs: None,
            authorized_database: Some(authorized),
            authorized_network: None,
            authorized_crypto: None,
            authorized_gpu: None,
            authorized_gfx_profile: None,
            authorized_xr_policy: None,
            authorized_bridge_identity: None,
            authorized_plugin: None,
            authorized_ffi: None,
        }
    }

    fn database_policy() -> AuthorizedDatabasePolicy {
        AuthorizedDatabasePolicy {
            target_allow: AuthorizedStringList::Absent,
            query_classes: AuthorizedStringList::Absent,
            max_result_bytes: AuthorizedMaxBytes::Absent,
            max_row_count: AuthorizedMaxBytes::Absent,
            max_value_bytes: AuthorizedMaxBytes::Absent,
        }
    }

    #[test]
    fn database_dispatch_consumes_authorized_policy_before_raw_policy() {
        let mut target = database_policy();
        target.target_allow = AuthorizedStringList::Valid(vec!["sqlite://safe".to_string()]);
        let policy = policy_with(
            "db_target_allow",
            TomlValue::String("invalid raw fallback".to_string()),
            target,
        );
        assert_eq!(
            db_target_allowlist_from_policy(Some(&policy), "io/db::connect").unwrap(),
            vec!["sqlite://safe".to_string()]
        );

        let mut bound = database_policy();
        bound.max_result_bytes = AuthorizedMaxBytes::Valid(4096);
        let policy = policy_with(
            "max_result_bytes",
            TomlValue::String("invalid raw fallback".to_string()),
            bound,
        );
        assert_eq!(
            db_positive_usize_from_policy(Some(&policy), "io/db::query", "max_result_bytes")
                .unwrap(),
            4096
        );
    }

    #[test]
    fn database_dispatch_preserves_authorized_policy_errors() {
        let mut database = database_policy();
        database.query_classes = AuthorizedStringList::InvalidEntry;
        database.max_row_count = AuthorizedMaxBytes::NonPositive;
        let policy = policy_with("ignored", TomlValue::Boolean(true), database);
        assert_eq!(
            db_query_class_allowlist_from_policy(Some(&policy), "io/db::query").unwrap_err(),
            "allow_query_classes entries must be strings"
        );
        assert_eq!(
            db_positive_usize_from_policy(Some(&policy), "io/db::query", "max_row_count")
                .unwrap_err(),
            "max_row_count must be greater than zero"
        );
    }
}

fn validate_db_target_policy(
    pol: Option<&OpPolicy>,
    target: &str,
    op: &str,
    field: &str,
) -> Result<(), String> {
    let _scheme = parse_url_scheme(target, op, field)?;
    let allowlist = db_target_allowlist_from_policy(pol, op)?;
    if allowlist_contains_prefix_or_glob(&allowlist, target) {
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
    if allowlist_contains_exact_or_glob_ci(&allowlist, query_class) {
        return Ok(());
    }
    Err(format!(
        "query class `{query_class}` is not in allow_query_classes policy"
    ))
}

pub(super) fn capability_io_db_connect(
    op: &str,
    bridge_runtime: &mut HostBridgeRuntime,
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
    match call_host_bridge(bridge_runtime, "db", op, payload, pol) {
        Ok(resp) => Ok(Value::data(resp)),
        Err(err) => Ok(mk_bridge_error(error_tok, &err, Some(op))),
    }
}

pub(super) fn capability_io_db_tx_begin(
    op: &str,
    bridge_runtime: &mut HostBridgeRuntime,
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
    match call_host_bridge(bridge_runtime, "db", op, payload, pol) {
        Ok(resp) => Ok(Value::data(resp)),
        Err(err) => Ok(mk_bridge_error(error_tok, &err, Some(op))),
    }
}

pub(super) fn capability_io_db_query_or_exec(
    op: &str,
    bridge_runtime: &mut HostBridgeRuntime,
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
    match call_host_bridge(bridge_runtime, "db", op, &effective_payload, pol) {
        Ok(resp) => Ok(Value::data(resp)),
        Err(err) => Ok(mk_bridge_error(error_tok, &err, Some(op))),
    }
}

pub(super) fn capability_io_db_tx_finish(
    op: &str,
    bridge_runtime: &mut HostBridgeRuntime,
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
    match call_host_bridge(bridge_runtime, "db", op, payload, pol) {
        Ok(resp) => Ok(Value::data(resp)),
        Err(err) => Ok(mk_bridge_error(error_tok, &err, Some(op))),
    }
}

pub(super) fn capability_io_db_kv_open(
    op: &str,
    bridge_runtime: &mut HostBridgeRuntime,
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
    match call_host_bridge(bridge_runtime, "db", op, payload, pol) {
        Ok(resp) => Ok(Value::data(resp)),
        Err(err) => Ok(mk_bridge_error(error_tok, &err, Some(op))),
    }
}

pub(super) fn capability_io_db_kv_get(
    op: &str,
    bridge_runtime: &mut HostBridgeRuntime,
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
    match call_host_bridge(bridge_runtime, "db", op, &effective_payload, pol) {
        Ok(resp) => Ok(Value::data(resp)),
        Err(err) => Ok(mk_bridge_error(error_tok, &err, Some(op))),
    }
}

pub(super) fn capability_io_db_kv_put(
    op: &str,
    bridge_runtime: &mut HostBridgeRuntime,
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
    match call_host_bridge(bridge_runtime, "db", op, &effective_payload, pol) {
        Ok(resp) => Ok(Value::data(resp)),
        Err(err) => Ok(mk_bridge_error(error_tok, &err, Some(op))),
    }
}

pub(super) fn capability_io_db_kv_delete(
    op: &str,
    bridge_runtime: &mut HostBridgeRuntime,
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
    match call_host_bridge(bridge_runtime, "db", op, payload, pol) {
        Ok(resp) => Ok(Value::data(resp)),
        Err(err) => Ok(mk_bridge_error(error_tok, &err, Some(op))),
    }
}
