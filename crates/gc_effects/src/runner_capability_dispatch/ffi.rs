use super::*;

fn ffi_allowlist_from_policy(
    pol: Option<&OpPolicy>,
    key: &str,
    op: &str,
) -> Result<Vec<String>, String> {
    parse_nonempty_string_array(
        pol,
        key,
        &format!("{op} requires per-op {key} allowlist in caps.toml"),
    )
}

fn ffi_schema_allowlist_from_policy(pol: Option<&OpPolicy>) -> Result<Option<Vec<String>>, String> {
    let Some(pol) = pol else {
        return Ok(None);
    };
    if !pol.extra.contains_key("allow_schema_ids") {
        return Ok(None);
    }
    parse_nonempty_string_array(
        Some(pol),
        "allow_schema_ids",
        "allow_schema_ids must be configured with at least one entry",
    )
    .map(Some)
}

fn ffi_bridge_digest_pin_is_required(pol: Option<&OpPolicy>) -> bool {
    let Some(pol) = pol else {
        return false;
    };
    let has_bridge_cmd = pol
        .extra
        .get("bridge_cmd")
        .and_then(|v| v.as_str())
        .is_some_and(|s| !s.trim().is_empty());
    let has_wasi_bridge_profile = pol
        .extra
        .get("wasi_bridge_profile")
        .and_then(|v| v.as_bool())
        .unwrap_or(false);
    has_bridge_cmd && !has_wasi_bridge_profile
}

fn ffi_bridge_digest_pin_from_policy(pol: Option<&OpPolicy>) -> Option<String> {
    pol.and_then(|p| p.extra.get("bridge_cmd_sha256"))
        .and_then(|v| v.as_str())
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .map(ToString::to_string)
}

fn term_bytes_or_string_len(value: &Term) -> Result<usize, String> {
    match value {
        Term::Bytes(bytes) => Ok(bytes.len()),
        Term::Str(s) => Ok(s.len()),
        _ => Err("must be bytes|string".to_string()),
    }
}

fn ffi_boundary_envelope(op: &str, payload: &Term, response: Term) -> Value {
    let request_envelope = Term::Map(
        [
            (
                TermOrdKey(Term::symbol(":op")),
                Term::Symbol(op.to_string()),
            ),
            (TermOrdKey(Term::symbol(":payload")), payload.clone()),
        ]
        .into_iter()
        .collect(),
    );
    let request_h = blake3::Hash::from_bytes(hash_term(&request_envelope))
        .to_hex()
        .to_string();
    let result_h = blake3::Hash::from_bytes(hash_term(&response))
        .to_hex()
        .to_string();

    Value::Data(Term::Map(
        [
            (TermOrdKey(Term::symbol(":ok")), Term::Bool(true)),
            (
                TermOrdKey(Term::symbol(":ffi-op")),
                Term::Symbol(op.to_string()),
            ),
            (TermOrdKey(Term::symbol(":request-h")), Term::Str(request_h)),
            (TermOrdKey(Term::symbol(":result-h")), Term::Str(result_h)),
            (TermOrdKey(Term::symbol(":result")), response),
        ]
        .into_iter()
        .collect(),
    ))
}

fn ffi_check_schema_ids(
    op: &str,
    schema_ids: &crate::runner_ffi_schema::FfiSchemaIds,
    pol: Option<&OpPolicy>,
    error_tok: SealId,
) -> Result<Option<Vec<String>>, Value> {
    if !schema_ids.has_any() {
        return Ok(None);
    }
    let allow_schema_ids = match ffi_schema_allowlist_from_policy(pol) {
        Ok(Some(v)) => v,
        Ok(None) => {
            return Err(mk_error(
                error_tok,
                "core/caps/policy-error",
                format!("{op} typed ffi schemas require per-op allow_schema_ids allowlist"),
                Some(op),
            ));
        }
        Err(e) => {
            return Err(mk_error(error_tok, "core/caps/policy-error", e, Some(op)));
        }
    };
    if let Some(schema_id) = schema_ids.request_schema_id.as_deref()
        && !allow_schema_ids.iter().any(|allowed| allowed == schema_id)
    {
        return Err(mk_error(
            error_tok,
            "core/caps/policy-error",
            format!("{op} denied request schema `{schema_id}`; configure allow_schema_ids"),
            Some(op),
        ));
    }
    if let Some(schema_id) = schema_ids.response_schema_id.as_deref()
        && !allow_schema_ids.iter().any(|allowed| allowed == schema_id)
    {
        return Err(mk_error(
            error_tok,
            "core/caps/policy-error",
            format!("{op} denied response schema `{schema_id}`; configure allow_schema_ids"),
            Some(op),
        ));
    }
    Ok(Some(allow_schema_ids))
}

fn ffi_policy_allowlist_check(
    op: &str,
    value: &str,
    allowlist: &[String],
    key: &str,
    error_tok: SealId,
) -> Result<(), Value> {
    if allowlist_contains_exact_or_glob(allowlist, value) {
        return Ok(());
    }
    Err(mk_error(
        error_tok,
        "core/caps/policy-error",
        format!("{op} denied `{value}`; configure {key} allowlist in caps.toml"),
        Some(op),
    ))
}

fn ffi_validate_request_schema(
    op: &str,
    payload: &Term,
    schema_ids: &crate::runner_ffi_schema::FfiSchemaIds,
    error_tok: SealId,
) -> Result<(), Value> {
    if let Some(schema_id) = schema_ids.request_schema_id.as_deref()
        && let Err(err) =
            crate::runner_ffi_schema::validate_ffi_request_schema(schema_id, payload, op)
    {
        return Err(mk_error(
            error_tok,
            "core/caps/schema-error",
            format!("{op} request schema `{schema_id}` validation failed: {err}"),
            Some(op),
        ));
    }
    Ok(())
}

fn ffi_validate_response_schema(
    op: &str,
    response: &Term,
    schema_ids: &crate::runner_ffi_schema::FfiSchemaIds,
    error_tok: SealId,
) -> Result<(), Value> {
    if let Some(schema_id) = schema_ids.response_schema_id.as_deref()
        && let Err(err) =
            crate::runner_ffi_schema::validate_ffi_response_schema(schema_id, response)
    {
        return Err(mk_error(
            error_tok,
            "core/caps/schema-error",
            format!("{op} response schema `{schema_id}` validation failed: {err}"),
            Some(op),
        ));
    }
    Ok(())
}

fn ffi_common_preflight(
    op: &str,
    payload: &Term,
    pol: Option<&OpPolicy>,
    error_tok: SealId,
) -> Result<crate::runner_ffi_schema::FfiSchemaIds, Value> {
    if ffi_bridge_digest_pin_is_required(pol) && ffi_bridge_digest_pin_from_policy(pol).is_none() {
        return Err(mk_error(
            error_tok,
            "core/caps/policy-error",
            format!(
                "{op} requires bridge_cmd_sha256 digest pin when bridge_cmd transport is configured"
            ),
            Some(op),
        ));
    }
    let schema_ids = match crate::runner_ffi_schema::parse_ffi_schema_ids(payload, op) {
        Ok(ids) => ids,
        Err(EffectsError::BadPayload(msg)) => {
            return Err(mk_error(
                error_tok,
                "core/caps/payload-error",
                msg,
                Some(op),
            ));
        }
        Err(err) => {
            return Err(mk_error(
                error_tok,
                "core/caps/payload-error",
                err.to_string(),
                Some(op),
            ));
        }
    };
    let _ = ffi_check_schema_ids(op, &schema_ids, pol, error_tok)?;
    ffi_validate_request_schema(op, payload, &schema_ids, error_tok)?;
    Ok(schema_ids)
}

fn ffi_common_bridge_call(
    op: &str,
    payload: &Term,
    pol: Option<&OpPolicy>,
    schema_ids: &crate::runner_ffi_schema::FfiSchemaIds,
    error_tok: SealId,
) -> Value {
    match call_host_bridge("host/ffi", op, payload, pol) {
        Ok(response) => {
            if let Err(err) = ffi_validate_response_schema(op, &response, schema_ids, error_tok) {
                return err;
            }
            ffi_boundary_envelope(op, payload, response)
        }
        Err(err) => mk_bridge_error(error_tok, &err, Some(op)),
    }
}

fn capability_host_ffi_call(
    op: &str,
    payload: &Term,
    pol: Option<&OpPolicy>,
    error_tok: SealId,
) -> Value {
    let schema_ids = match ffi_common_preflight(op, payload, pol, error_tok) {
        Ok(ids) => ids,
        Err(err) => return err,
    };
    let abi_id = match payload_required_string_or_symbol_field(payload, op, ":abi-id") {
        Ok(v) => v,
        Err(err) => {
            return mk_error(
                error_tok,
                "core/caps/payload-error",
                err.to_string(),
                Some(op),
            );
        }
    };
    let library = match payload_required_string_or_symbol_field(payload, op, ":library") {
        Ok(v) => v,
        Err(err) => {
            return mk_error(
                error_tok,
                "core/caps/payload-error",
                err.to_string(),
                Some(op),
            );
        }
    };
    let symbol = match payload_required_string_or_symbol_field(payload, op, ":symbol") {
        Ok(v) => v,
        Err(err) => {
            return mk_error(
                error_tok,
                "core/caps/payload-error",
                err.to_string(),
                Some(op),
            );
        }
    };

    let allow_abi_ids = match ffi_allowlist_from_policy(pol, "allow_abi_ids", op) {
        Ok(v) => v,
        Err(err) => return mk_error(error_tok, "core/caps/policy-error", err, Some(op)),
    };
    if let Err(err) =
        ffi_policy_allowlist_check(op, &abi_id, &allow_abi_ids, "allow_abi_ids", error_tok)
    {
        return err;
    }
    let allow_libraries = match ffi_allowlist_from_policy(pol, "allow_libraries", op) {
        Ok(v) => v,
        Err(err) => return mk_error(error_tok, "core/caps/policy-error", err, Some(op)),
    };
    if let Err(err) =
        ffi_policy_allowlist_check(op, &library, &allow_libraries, "allow_libraries", error_tok)
    {
        return err;
    }
    let allow_symbols = match ffi_allowlist_from_policy(pol, "allow_symbols", op) {
        Ok(v) => v,
        Err(err) => return mk_error(error_tok, "core/caps/policy-error", err, Some(op)),
    };
    if let Err(err) =
        ffi_policy_allowlist_check(op, &symbol, &allow_symbols, "allow_symbols", error_tok)
    {
        return err;
    }

    ffi_common_bridge_call(op, payload, pol, &schema_ids, error_tok)
}

fn capability_host_ffi_buffer_pin(
    op: &str,
    payload: &Term,
    pol: Option<&OpPolicy>,
    error_tok: SealId,
) -> Value {
    let schema_ids = match ffi_common_preflight(op, payload, pol, error_tok) {
        Ok(ids) => ids,
        Err(err) => return err,
    };
    let abi_id = match payload_required_string_or_symbol_field(payload, op, ":abi-id") {
        Ok(v) => v,
        Err(err) => {
            return mk_error(
                error_tok,
                "core/caps/payload-error",
                err.to_string(),
                Some(op),
            );
        }
    };
    let allow_abi_ids = match ffi_allowlist_from_policy(pol, "allow_abi_ids", op) {
        Ok(v) => v,
        Err(err) => return mk_error(error_tok, "core/caps/policy-error", err, Some(op)),
    };
    if let Err(err) =
        ffi_policy_allowlist_check(op, &abi_id, &allow_abi_ids, "allow_abi_ids", error_tok)
    {
        return err;
    }

    let bytes = match payload_required_field(payload, op, ":bytes") {
        Ok(v) => v,
        Err(err) => {
            return mk_error(
                error_tok,
                "core/caps/payload-error",
                err.to_string(),
                Some(op),
            );
        }
    };
    let observed = match term_bytes_or_string_len(&bytes) {
        Ok(len) => len,
        Err(err) => {
            return mk_error(
                error_tok,
                "core/caps/payload-error",
                format!("{op} payload field `:bytes` {err}"),
                Some(op),
            );
        }
    };
    let max_buffer_bytes = match op_extra_positive_usize(pol, "max_buffer_bytes") {
        Ok(Some(v)) => v,
        Ok(None) => {
            return mk_error(
                error_tok,
                "core/caps/policy-error",
                format!("{op} requires max_buffer_bytes policy bound"),
                Some(op),
            );
        }
        Err(err) => return mk_error(error_tok, "core/caps/policy-error", err, Some(op)),
    };
    if observed > max_buffer_bytes {
        return mk_error(
            error_tok,
            "core/caps/resource-limit",
            format!("{op} payload bytes exceed max_buffer_bytes ({observed} > {max_buffer_bytes})"),
            Some(op),
        );
    }

    ffi_common_bridge_call(op, payload, pol, &schema_ids, error_tok)
}

fn capability_host_ffi_buffer_unpin(
    op: &str,
    payload: &Term,
    pol: Option<&OpPolicy>,
    error_tok: SealId,
) -> Value {
    let schema_ids = match ffi_common_preflight(op, payload, pol, error_tok) {
        Ok(ids) => ids,
        Err(err) => return err,
    };
    let abi_id = match payload_required_string_or_symbol_field(payload, op, ":abi-id") {
        Ok(v) => v,
        Err(err) => {
            return mk_error(
                error_tok,
                "core/caps/payload-error",
                err.to_string(),
                Some(op),
            );
        }
    };
    let _handle = match payload_required_string_or_symbol_field(payload, op, ":handle") {
        Ok(v) => v,
        Err(err) => {
            return mk_error(
                error_tok,
                "core/caps/payload-error",
                err.to_string(),
                Some(op),
            );
        }
    };
    let allow_abi_ids = match ffi_allowlist_from_policy(pol, "allow_abi_ids", op) {
        Ok(v) => v,
        Err(err) => return mk_error(error_tok, "core/caps/policy-error", err, Some(op)),
    };
    if let Err(err) =
        ffi_policy_allowlist_check(op, &abi_id, &allow_abi_ids, "allow_abi_ids", error_tok)
    {
        return err;
    }
    ffi_common_bridge_call(op, payload, pol, &schema_ids, error_tok)
}

pub(super) fn capability_host_ffi(
    op: &str,
    payload: &Term,
    pol: Option<&OpPolicy>,
    error_tok: SealId,
) -> Result<Value, EffectsError> {
    let out = match op {
        "host/ffi::call" => capability_host_ffi_call(op, payload, pol, error_tok),
        "host/ffi::buffer-pin" => capability_host_ffi_buffer_pin(op, payload, pol, error_tok),
        "host/ffi::buffer-unpin" => capability_host_ffi_buffer_unpin(op, payload, pol, error_tok),
        _ => mk_error(
            error_tok,
            "core/caps/unknown-op",
            format!("unknown capability op: {op}"),
            Some(op),
        ),
    };
    Ok(out)
}
