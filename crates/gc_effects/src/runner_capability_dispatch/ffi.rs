use super::*;
#[path = "ffi_policy.rs"]
mod policy;

fn is_hex64(s: &str) -> bool {
    s.len() == 64 && s.bytes().all(|b| b.is_ascii_hexdigit())
}

#[derive(Debug, Clone)]
struct FfiSignedPolicyMetadata {
    policy_artifact_h: String,
    policy_signature_h: String,
    policy_key_id: String,
    evidence_mode: String,
}

fn ffi_signed_policy_metadata(
    op: &str,
    pol: Option<&OpPolicy>,
    error_tok: SealId,
) -> Result<Option<FfiSignedPolicyMetadata>, Value> {
    if !policy::signed_policy_required(pol) {
        return Ok(None);
    }
    let policy_artifact_h = policy::required_signed_string(pol, "policy_artifact_h", op)
        .map_err(|msg| mk_error(error_tok, "core/caps/policy-error", msg, Some(op)))?;
    if !is_hex64(&policy_artifact_h) {
        return Err(mk_error(
            error_tok,
            "core/caps/policy-error",
            format!("{op} policy_artifact_h must be 64-hex"),
            Some(op),
        ));
    }
    let policy_signature_h = policy::required_signed_string(pol, "policy_signature_h", op)
        .map_err(|msg| mk_error(error_tok, "core/caps/policy-error", msg, Some(op)))?;
    if !is_hex64(&policy_signature_h) {
        return Err(mk_error(
            error_tok,
            "core/caps/policy-error",
            format!("{op} policy_signature_h must be 64-hex"),
            Some(op),
        ));
    }
    let policy_key_id = policy::required_signed_string(pol, "policy_key_id", op)
        .map_err(|msg| mk_error(error_tok, "core/caps/policy-error", msg, Some(op)))?;
    let evidence_mode = policy::required_signed_string(pol, "evidence_mode", op)
        .map_err(|msg| mk_error(error_tok, "core/caps/policy-error", msg, Some(op)))?;
    if evidence_mode != "deterministic" {
        return Err(mk_error(
            error_tok,
            "core/caps/policy-error",
            format!("{op} evidence_mode must be `deterministic`"),
            Some(op),
        ));
    }
    Ok(Some(FfiSignedPolicyMetadata {
        policy_artifact_h,
        policy_signature_h,
        policy_key_id,
        evidence_mode,
    }))
}

fn ffi_call_payload_len(payload: &Term) -> usize {
    match payload_required_field(payload, "host/ffi::call", ":payload") {
        Ok(term) => print_term(&term).len(),
        Err(_) => 0,
    }
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

fn ffi_boundary_envelope(
    op: &str,
    payload: &Term,
    response: Term,
    signed_policy: Option<&FfiSignedPolicyMetadata>,
) -> Value {
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

    let mut envelope = std::collections::BTreeMap::new();
    envelope.insert(TermOrdKey(Term::symbol(":ok")), Term::Bool(true));
    envelope.insert(
        TermOrdKey(Term::symbol(":ffi-op")),
        Term::Symbol(op.to_string()),
    );
    envelope.insert(
        TermOrdKey(Term::symbol(":request-h")),
        Term::Str(request_h.clone()),
    );
    envelope.insert(
        TermOrdKey(Term::symbol(":result-h")),
        Term::Str(result_h.clone()),
    );
    envelope.insert(TermOrdKey(Term::symbol(":result")), response);

    if let Some(signed_policy) = signed_policy {
        let provenance = Term::Map(
            [
                (
                    TermOrdKey(Term::symbol(":policy-artifact-h")),
                    Term::Str(signed_policy.policy_artifact_h.clone()),
                ),
                (
                    TermOrdKey(Term::symbol(":policy-signature-h")),
                    Term::Str(signed_policy.policy_signature_h.clone()),
                ),
                (
                    TermOrdKey(Term::symbol(":policy-key-id")),
                    Term::Str(signed_policy.policy_key_id.clone()),
                ),
                (
                    TermOrdKey(Term::symbol(":evidence-mode")),
                    Term::Str(signed_policy.evidence_mode.clone()),
                ),
                (TermOrdKey(Term::symbol(":request-h")), Term::Str(request_h)),
                (TermOrdKey(Term::symbol(":result-h")), Term::Str(result_h)),
            ]
            .into_iter()
            .collect(),
        );
        envelope.insert(TermOrdKey(Term::symbol(":ffi-provenance")), provenance);
    }

    Value::data(Term::Map(envelope))
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
    let allow_schema_ids = match policy::schema_allowlist_from_policy(pol) {
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
) -> Result<FfiPreflight, Value> {
    let signed_policy = ffi_signed_policy_metadata(op, pol, error_tok)?;
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
    Ok(FfiPreflight {
        schema_ids,
        signed_policy,
    })
}

struct FfiPreflight {
    schema_ids: crate::runner_ffi_schema::FfiSchemaIds,
    signed_policy: Option<FfiSignedPolicyMetadata>,
}

fn ffi_common_bridge_call(
    bridge_runtime: &mut HostBridgeRuntime,
    op: &str,
    payload: &Term,
    pol: Option<&OpPolicy>,
    schema_ids: &crate::runner_ffi_schema::FfiSchemaIds,
    signed_policy: Option<&FfiSignedPolicyMetadata>,
    error_tok: SealId,
) -> Value {
    match call_host_bridge(bridge_runtime, "host/ffi", op, payload, pol) {
        Ok(response) => {
            if let Err(err) = ffi_validate_response_schema(op, &response, schema_ids, error_tok) {
                return err;
            }
            ffi_boundary_envelope(op, payload, response, signed_policy)
        }
        Err(err) => mk_bridge_error(error_tok, &err, Some(op)),
    }
}

fn capability_host_ffi_call(
    bridge_runtime: &mut HostBridgeRuntime,
    op: &str,
    payload: &Term,
    pol: Option<&OpPolicy>,
    error_tok: SealId,
) -> Value {
    let preflight = match ffi_common_preflight(op, payload, pol, error_tok) {
        Ok(preflight) => preflight,
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

    let allow_abi_ids = match policy::allowlist_from_policy(pol, "allow_abi_ids", op) {
        Ok(v) => v,
        Err(err) => return mk_error(error_tok, "core/caps/policy-error", err, Some(op)),
    };
    if let Err(err) =
        ffi_policy_allowlist_check(op, &abi_id, &allow_abi_ids, "allow_abi_ids", error_tok)
    {
        return err;
    }
    let allow_libraries = match policy::allowlist_from_policy(pol, "allow_libraries", op) {
        Ok(v) => v,
        Err(err) => return mk_error(error_tok, "core/caps/policy-error", err, Some(op)),
    };
    if let Err(err) =
        ffi_policy_allowlist_check(op, &library, &allow_libraries, "allow_libraries", error_tok)
    {
        return err;
    }
    let allow_symbols = match policy::allowlist_from_policy(pol, "allow_symbols", op) {
        Ok(v) => v,
        Err(err) => return mk_error(error_tok, "core/caps/policy-error", err, Some(op)),
    };
    if let Err(err) =
        ffi_policy_allowlist_check(op, &symbol, &allow_symbols, "allow_symbols", error_tok)
    {
        return err;
    }
    if preflight.signed_policy.is_some() {
        let max_call_payload_bytes =
            match policy::positive_usize_from_policy(pol, "max_call_payload_bytes") {
                Ok(Some(v)) => v,
                Ok(None) => {
                    return mk_error(
                        error_tok,
                        "core/caps/policy-error",
                        format!(
                            "{op} requires max_call_payload_bytes when signed_policy_required=true"
                        ),
                        Some(op),
                    );
                }
                Err(err) => return mk_error(error_tok, "core/caps/policy-error", err, Some(op)),
            };
        let observed = ffi_call_payload_len(payload);
        if observed > max_call_payload_bytes {
            return mk_error(
                error_tok,
                "core/caps/resource-limit",
                format!(
                    "{op} payload bytes exceed max_call_payload_bytes ({observed} > {max_call_payload_bytes})"
                ),
                Some(op),
            );
        }
    }

    ffi_common_bridge_call(
        bridge_runtime,
        op,
        payload,
        pol,
        &preflight.schema_ids,
        preflight.signed_policy.as_ref(),
        error_tok,
    )
}

fn capability_host_ffi_buffer_pin(
    bridge_runtime: &mut HostBridgeRuntime,
    op: &str,
    payload: &Term,
    pol: Option<&OpPolicy>,
    error_tok: SealId,
) -> Value {
    let preflight = match ffi_common_preflight(op, payload, pol, error_tok) {
        Ok(preflight) => preflight,
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
    let allow_abi_ids = match policy::allowlist_from_policy(pol, "allow_abi_ids", op) {
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
    let max_buffer_bytes = match policy::positive_usize_from_policy(pol, "max_buffer_bytes") {
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

    ffi_common_bridge_call(
        bridge_runtime,
        op,
        payload,
        pol,
        &preflight.schema_ids,
        preflight.signed_policy.as_ref(),
        error_tok,
    )
}

fn capability_host_ffi_buffer_unpin(
    bridge_runtime: &mut HostBridgeRuntime,
    op: &str,
    payload: &Term,
    pol: Option<&OpPolicy>,
    error_tok: SealId,
) -> Value {
    let preflight = match ffi_common_preflight(op, payload, pol, error_tok) {
        Ok(preflight) => preflight,
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
    let allow_abi_ids = match policy::allowlist_from_policy(pol, "allow_abi_ids", op) {
        Ok(v) => v,
        Err(err) => return mk_error(error_tok, "core/caps/policy-error", err, Some(op)),
    };
    if let Err(err) =
        ffi_policy_allowlist_check(op, &abi_id, &allow_abi_ids, "allow_abi_ids", error_tok)
    {
        return err;
    }
    ffi_common_bridge_call(
        bridge_runtime,
        op,
        payload,
        pol,
        &preflight.schema_ids,
        preflight.signed_policy.as_ref(),
        error_tok,
    )
}

pub(super) fn capability_host_ffi(
    op: &str,
    bridge_runtime: &mut HostBridgeRuntime,
    payload: &Term,
    pol: Option<&OpPolicy>,
    error_tok: SealId,
) -> Result<Value, EffectsError> {
    let out = match op {
        "host/ffi::call" => capability_host_ffi_call(bridge_runtime, op, payload, pol, error_tok),
        "host/ffi::buffer-pin" => {
            capability_host_ffi_buffer_pin(bridge_runtime, op, payload, pol, error_tok)
        }
        "host/ffi::buffer-unpin" => {
            capability_host_ffi_buffer_unpin(bridge_runtime, op, payload, pol, error_tok)
        }
        _ => mk_error(
            error_tok,
            "core/caps/unknown-op",
            format!("unknown capability op: {op}"),
            Some(op),
        ),
    };
    Ok(out)
}
