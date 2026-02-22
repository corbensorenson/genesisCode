use gc_coreform::{Term, TermOrdKey};

use crate::EffectsError;

pub(crate) const PLUGIN_REQUEST_EXEC_V1: &str = "genesis/plugin.request.exec.v1";
pub(crate) const PLUGIN_REQUEST_JSONRPC_V1: &str = "genesis/plugin.request.jsonrpc.v1";
pub(crate) const PLUGIN_RESPONSE_RESULT_V1: &str = "genesis/plugin.response.result.v1";
pub(crate) const PLUGIN_RESPONSE_BYTES_V1: &str = "genesis/plugin.response.bytes.v1";

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct PluginSchemaIds {
    pub request_schema_id: Option<String>,
    pub response_schema_id: Option<String>,
}

impl PluginSchemaIds {
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

fn parse_schema_alias(
    payload: &Term,
    op: &str,
    canonical_key: &str,
    legacy_key: &str,
) -> Result<Option<String>, EffectsError> {
    let canonical = parse_optional_string_or_symbol_field(payload, op, canonical_key)?;
    let legacy = parse_optional_string_or_symbol_field(payload, op, legacy_key)?;
    match (canonical, legacy) {
        (Some(a), Some(b)) if a != b => Err(EffectsError::BadPayload(format!(
            "{op} payload schema alias mismatch: `{canonical_key}` ({a}) != `{legacy_key}` ({b})"
        ))),
        (Some(a), _) => Ok(Some(a)),
        (None, Some(b)) => Ok(Some(b)),
        (None, None) => Ok(None),
    }
}

pub(crate) fn parse_plugin_schema_ids(
    payload: &Term,
    op: &str,
) -> Result<PluginSchemaIds, EffectsError> {
    let request_schema_id =
        parse_schema_alias(payload, op, ":request-schema-id", ":request-schema")?;
    let response_schema_id =
        parse_schema_alias(payload, op, ":response-schema-id", ":response-schema")?;
    Ok(PluginSchemaIds {
        request_schema_id,
        response_schema_id,
    })
}

fn term_is_string_or_bytes(value: &Term) -> bool {
    matches!(value, Term::Str(_) | Term::Bytes(_))
}

fn term_is_string_or_symbol(value: &Term) -> bool {
    matches!(value, Term::Str(_) | Term::Symbol(_))
}

fn term_is_string_keyed_map(value: &Term) -> bool {
    let Term::Map(mm) = value else {
        return false;
    };
    mm.iter()
        .all(|(k, v)| matches!(&k.0, Term::Str(_)) && matches!(v, Term::Str(_)))
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

fn validate_exec_request(payload: &Term, plugin: &str, command: &str) -> Result<(), String> {
    let Term::Map(mm) = payload else {
        return Err(format!(
            "schema {PLUGIN_REQUEST_EXEC_V1} requires map payload for {plugin}/{command}"
        ));
    };
    let args = required_map_field(mm, ":args")?;
    let Term::Vector(_) = args else {
        return Err("field `:args` must be a vector".to_string());
    };
    if let Some(cwd) = optional_map_field(mm, ":cwd")
        && !matches!(cwd, Term::Str(_))
    {
        return Err("field `:cwd` must be a string".to_string());
    }
    if let Some(env) = optional_map_field(mm, ":env")
        && !term_is_string_keyed_map(env)
    {
        return Err("field `:env` must be map<string,string>".to_string());
    }
    if let Some(stdin) = optional_map_field(mm, ":stdin")
        && !matches!(stdin, Term::Nil | Term::Str(_) | Term::Bytes(_))
    {
        return Err("field `:stdin` must be nil|string|bytes".to_string());
    }
    Ok(())
}

fn validate_jsonrpc_request(payload: &Term, plugin: &str, command: &str) -> Result<(), String> {
    let Term::Map(mm) = payload else {
        return Err(format!(
            "schema {PLUGIN_REQUEST_JSONRPC_V1} requires map payload for {plugin}/{command}"
        ));
    };
    let method = required_map_field(mm, ":method")?;
    let Term::Str(method_name) = method else {
        return Err("field `:method` must be a string".to_string());
    };
    if method_name.trim().is_empty() {
        return Err("field `:method` must not be empty".to_string());
    }
    if let Some(id) = optional_map_field(mm, ":id")
        && !matches!(id, Term::Nil | Term::Int(_) | Term::Str(_))
    {
        return Err("field `:id` must be nil|int|string".to_string());
    }
    Ok(())
}

pub(crate) fn validate_plugin_request_schema(
    schema_id: &str,
    payload: &Term,
    plugin: &str,
    command: &str,
) -> Result<(), String> {
    match schema_id {
        PLUGIN_REQUEST_EXEC_V1 => validate_exec_request(payload, plugin, command),
        PLUGIN_REQUEST_JSONRPC_V1 => validate_jsonrpc_request(payload, plugin, command),
        _ => Err(format!("unknown request schema id `{schema_id}`")),
    }
}

fn validate_result_error_map(error_value: &Term) -> Result<(), String> {
    let Term::Map(mm) = error_value else {
        return Err("field `:error` must be a map".to_string());
    };
    let message = required_map_field(mm, ":message")?;
    let Term::Str(msg) = message else {
        return Err("field `:error/:message` must be a string".to_string());
    };
    if msg.trim().is_empty() {
        return Err("field `:error/:message` must not be empty".to_string());
    }
    if let Some(code) = optional_map_field(mm, ":code")
        && !term_is_string_or_symbol(code)
    {
        return Err("field `:error/:code` must be string|symbol".to_string());
    }
    Ok(())
}

fn validate_result_response(response: &Term) -> Result<(), String> {
    let Term::Map(mm) = response else {
        return Err(format!(
            "schema {PLUGIN_RESPONSE_RESULT_V1} requires map response"
        ));
    };
    let ok_term = required_map_field(mm, ":ok")?;
    let Term::Bool(ok) = ok_term else {
        return Err("field `:ok` must be bool".to_string());
    };
    if *ok {
        if optional_map_field(mm, ":error").is_some() {
            return Err("field `:error` must be absent when `:ok` is true".to_string());
        }
    } else {
        let err = required_map_field(mm, ":error")?;
        validate_result_error_map(err)?;
    }
    Ok(())
}

fn validate_bytes_response(response: &Term) -> Result<(), String> {
    let Term::Map(mm) = response else {
        return Err(format!(
            "schema {PLUGIN_RESPONSE_BYTES_V1} requires map response"
        ));
    };
    let ok_term = required_map_field(mm, ":ok")?;
    let Term::Bool(ok) = ok_term else {
        return Err("field `:ok` must be bool".to_string());
    };
    if *ok {
        let data = required_map_field(mm, ":data")?;
        if !term_is_string_or_bytes(data) {
            return Err("field `:data` must be string|bytes when `:ok` is true".to_string());
        }
    } else {
        let err = required_map_field(mm, ":error")?;
        validate_result_error_map(err)?;
    }
    Ok(())
}

pub(crate) fn validate_plugin_response_schema(
    schema_id: &str,
    response: &Term,
) -> Result<(), String> {
    match schema_id {
        PLUGIN_RESPONSE_RESULT_V1 => validate_result_response(response),
        PLUGIN_RESPONSE_BYTES_V1 => validate_bytes_response(response),
        _ => Err(format!("unknown response schema id `{schema_id}`")),
    }
}
