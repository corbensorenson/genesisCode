use gc_coreform::{Term, TermOrdKey};

use crate::EffectsError;

pub(crate) const FFI_REQUEST_CALL_V1: &str = "genesis/ffi.request.call.v1";
pub(crate) const FFI_REQUEST_BUFFER_PIN_V1: &str = "genesis/ffi.request.buffer-pin.v1";
pub(crate) const FFI_REQUEST_BUFFER_UNPIN_V1: &str = "genesis/ffi.request.buffer-unpin.v1";
pub(crate) const FFI_RESPONSE_CALL_V1: &str = "genesis/ffi.response.call.v1";
pub(crate) const FFI_RESPONSE_BUFFER_HANDLE_V1: &str = "genesis/ffi.response.buffer-handle.v1";
pub(crate) const FFI_RESPONSE_STATUS_V1: &str = "genesis/ffi.response.status.v1";

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct FfiSchemaIds {
    pub request_schema_id: Option<String>,
    pub response_schema_id: Option<String>,
}

impl FfiSchemaIds {
    pub(crate) fn has_any(&self) -> bool {
        self.request_schema_id.is_some() || self.response_schema_id.is_some()
    }
}

fn parse_optional_string_or_symbol_field(
    payload: &Term,
    op: &str,
    key: &str,
) -> Result<Option<String>, EffectsError> {
    let Term::Map(mm) = payload else {
        return Err(EffectsError::BadPayload(format!(
            "{op} payload must be a map"
        )));
    };
    let Some(value) = mm.get(&TermOrdKey(Term::symbol(key))) else {
        return Ok(None);
    };
    if matches!(value, Term::Nil) {
        return Ok(None);
    }
    let trimmed = match value {
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
    Ok(Some(trimmed.to_string()))
}

fn reject_legacy_schema_field(
    payload: &Term,
    op: &str,
    canonical_key: &str,
    legacy_key: &str,
) -> Result<(), EffectsError> {
    let Term::Map(mm) = payload else {
        return Err(EffectsError::BadPayload(format!(
            "{op} payload must be a map"
        )));
    };
    if mm.contains_key(&TermOrdKey(Term::symbol(legacy_key))) {
        return Err(EffectsError::BadPayload(format!(
            "{op} payload field `{legacy_key}` is retired; use `{canonical_key}`"
        )));
    }
    Ok(())
}

pub(crate) fn parse_ffi_schema_ids(payload: &Term, op: &str) -> Result<FfiSchemaIds, EffectsError> {
    reject_legacy_schema_field(payload, op, ":request-schema-id", ":request-schema")?;
    reject_legacy_schema_field(payload, op, ":response-schema-id", ":response-schema")?;
    let request_schema_id =
        parse_optional_string_or_symbol_field(payload, op, ":request-schema-id")?;
    let response_schema_id =
        parse_optional_string_or_symbol_field(payload, op, ":response-schema-id")?;
    Ok(FfiSchemaIds {
        request_schema_id,
        response_schema_id,
    })
}

fn required_map_field<'a>(
    map: &'a std::collections::BTreeMap<TermOrdKey, Term>,
    key: &str,
) -> Result<&'a Term, String> {
    map.get(&TermOrdKey(Term::symbol(key)))
        .ok_or_else(|| format!("missing required field `{key}`"))
}

fn optional_map_field<'a>(
    map: &'a std::collections::BTreeMap<TermOrdKey, Term>,
    key: &str,
) -> Option<&'a Term> {
    map.get(&TermOrdKey(Term::symbol(key)))
}

fn term_is_nonempty_string_or_symbol(value: &Term) -> bool {
    match value {
        Term::Str(s) | Term::Symbol(s) => !s.trim().is_empty(),
        _ => false,
    }
}

fn validate_error_map(value: &Term) -> Result<(), String> {
    let Term::Map(mm) = value else {
        return Err("field `:error` must be a map".to_string());
    };
    let msg = required_map_field(mm, ":message")?;
    let Term::Str(message) = msg else {
        return Err("field `:error/:message` must be a string".to_string());
    };
    if message.trim().is_empty() {
        return Err("field `:error/:message` must not be empty".to_string());
    }
    if let Some(code) = optional_map_field(mm, ":code")
        && !matches!(code, Term::Str(_) | Term::Symbol(_))
    {
        return Err("field `:error/:code` must be string|symbol".to_string());
    }
    Ok(())
}

fn validate_request_call(payload: &Term, op: &str) -> Result<(), String> {
    let Term::Map(mm) = payload else {
        return Err(format!(
            "schema {FFI_REQUEST_CALL_V1} requires map payload for {op}"
        ));
    };
    let abi_id = required_map_field(mm, ":abi-id")?;
    if !term_is_nonempty_string_or_symbol(abi_id) {
        return Err("field `:abi-id` must be non-empty string|symbol".to_string());
    }
    let library = required_map_field(mm, ":library")?;
    if !term_is_nonempty_string_or_symbol(library) {
        return Err("field `:library` must be non-empty string|symbol".to_string());
    }
    let symbol = required_map_field(mm, ":symbol")?;
    if !term_is_nonempty_string_or_symbol(symbol) {
        return Err("field `:symbol` must be non-empty string|symbol".to_string());
    }
    if let Some(mode) = optional_map_field(mm, ":mode")
        && !matches!(mode, Term::Str(_) | Term::Symbol(_))
    {
        return Err("field `:mode` must be string|symbol".to_string());
    }
    Ok(())
}

fn validate_request_buffer_pin(payload: &Term, op: &str) -> Result<(), String> {
    let Term::Map(mm) = payload else {
        return Err(format!(
            "schema {FFI_REQUEST_BUFFER_PIN_V1} requires map payload for {op}"
        ));
    };
    let abi_id = required_map_field(mm, ":abi-id")?;
    if !term_is_nonempty_string_or_symbol(abi_id) {
        return Err("field `:abi-id` must be non-empty string|symbol".to_string());
    }
    let bytes = required_map_field(mm, ":bytes")?;
    if !matches!(bytes, Term::Bytes(_) | Term::Str(_)) {
        return Err("field `:bytes` must be bytes|string".to_string());
    }
    if let Some(read_only) = optional_map_field(mm, ":read-only")
        && !matches!(read_only, Term::Bool(_))
    {
        return Err("field `:read-only` must be bool".to_string());
    }
    if let Some(lifetime) = optional_map_field(mm, ":lifetime")
        && !matches!(lifetime, Term::Str(_) | Term::Symbol(_))
    {
        return Err("field `:lifetime` must be string|symbol".to_string());
    }
    if let Some(owner) = optional_map_field(mm, ":owner")
        && !matches!(owner, Term::Str(_))
    {
        return Err("field `:owner` must be string".to_string());
    }
    Ok(())
}

fn validate_request_buffer_unpin(payload: &Term, op: &str) -> Result<(), String> {
    let Term::Map(mm) = payload else {
        return Err(format!(
            "schema {FFI_REQUEST_BUFFER_UNPIN_V1} requires map payload for {op}"
        ));
    };
    let abi_id = required_map_field(mm, ":abi-id")?;
    if !term_is_nonempty_string_or_symbol(abi_id) {
        return Err("field `:abi-id` must be non-empty string|symbol".to_string());
    }
    let handle = required_map_field(mm, ":handle")?;
    if !term_is_nonempty_string_or_symbol(handle) {
        return Err("field `:handle` must be non-empty string|symbol".to_string());
    }
    if let Some(reason) = optional_map_field(mm, ":reason")
        && !matches!(reason, Term::Str(_) | Term::Symbol(_))
    {
        return Err("field `:reason` must be string|symbol".to_string());
    }
    Ok(())
}

pub(crate) fn validate_ffi_request_schema(
    schema_id: &str,
    payload: &Term,
    op: &str,
) -> Result<(), String> {
    match schema_id {
        FFI_REQUEST_CALL_V1 if op == "host/ffi::call" => validate_request_call(payload, op),
        FFI_REQUEST_BUFFER_PIN_V1 if op == "host/ffi::buffer-pin" => {
            validate_request_buffer_pin(payload, op)
        }
        FFI_REQUEST_BUFFER_UNPIN_V1 if op == "host/ffi::buffer-unpin" => {
            validate_request_buffer_unpin(payload, op)
        }
        FFI_REQUEST_CALL_V1 | FFI_REQUEST_BUFFER_PIN_V1 | FFI_REQUEST_BUFFER_UNPIN_V1 => Err(
            format!("request schema `{schema_id}` is incompatible with op `{op}`"),
        ),
        _ => Err(format!("unknown request schema id `{schema_id}`")),
    }
}

fn validate_response_call(response: &Term) -> Result<(), String> {
    let Term::Map(mm) = response else {
        return Err(format!(
            "schema {FFI_RESPONSE_CALL_V1} requires map response"
        ));
    };
    let ok = required_map_field(mm, ":ok")?;
    let Term::Bool(is_ok) = ok else {
        return Err("field `:ok` must be bool".to_string());
    };
    if *is_ok {
        let _ = required_map_field(mm, ":result")?;
        return Ok(());
    }
    let err = required_map_field(mm, ":error")?;
    validate_error_map(err)
}

fn validate_response_buffer_handle(response: &Term) -> Result<(), String> {
    let Term::Map(mm) = response else {
        return Err(format!(
            "schema {FFI_RESPONSE_BUFFER_HANDLE_V1} requires map response"
        ));
    };
    let ok = required_map_field(mm, ":ok")?;
    let Term::Bool(is_ok) = ok else {
        return Err("field `:ok` must be bool".to_string());
    };
    if *is_ok {
        let handle = required_map_field(mm, ":handle")?;
        if !term_is_nonempty_string_or_symbol(handle) {
            return Err("field `:handle` must be non-empty string|symbol".to_string());
        }
        return Ok(());
    }
    let err = required_map_field(mm, ":error")?;
    validate_error_map(err)
}

fn validate_response_status(response: &Term) -> Result<(), String> {
    let Term::Map(mm) = response else {
        return Err(format!(
            "schema {FFI_RESPONSE_STATUS_V1} requires map response"
        ));
    };
    let ok = required_map_field(mm, ":ok")?;
    let Term::Bool(is_ok) = ok else {
        return Err("field `:ok` must be bool".to_string());
    };
    if *is_ok {
        if let Some(status) = optional_map_field(mm, ":status")
            && !matches!(status, Term::Str(_) | Term::Symbol(_))
        {
            return Err("field `:status` must be string|symbol".to_string());
        }
        return Ok(());
    }
    let err = required_map_field(mm, ":error")?;
    validate_error_map(err)
}

pub(crate) fn validate_ffi_response_schema(schema_id: &str, response: &Term) -> Result<(), String> {
    match schema_id {
        FFI_RESPONSE_CALL_V1 => validate_response_call(response),
        FFI_RESPONSE_BUFFER_HANDLE_V1 => validate_response_buffer_handle(response),
        FFI_RESPONSE_STATUS_V1 => validate_response_status(response),
        _ => Err(format!("unknown response schema id `{schema_id}`")),
    }
}
