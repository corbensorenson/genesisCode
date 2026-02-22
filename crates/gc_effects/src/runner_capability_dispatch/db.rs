use super::*;

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

pub(super) fn capability_io_db_connect(
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

pub(super) fn capability_io_db_tx_begin(
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

pub(super) fn capability_io_db_query_or_exec(
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

pub(super) fn capability_io_db_tx_finish(
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

pub(super) fn capability_io_db_kv_open(
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

pub(super) fn capability_io_db_kv_get(
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

pub(super) fn capability_io_db_kv_put(
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

pub(super) fn capability_io_db_kv_delete(
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
