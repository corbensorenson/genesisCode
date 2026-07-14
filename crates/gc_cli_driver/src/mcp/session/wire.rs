use std::io::{self, Write};

use serde_json::{Value, json};

use super::*;

pub(super) fn progress(
    token: &Value,
    value: u8,
    message: &str,
    config: &Config,
) -> Result<(), CliError> {
    emit(
        &json!({
            "jsonrpc": JSONRPC,
            "method": "notifications/progress",
            "params": {"progressToken": token, "progress": value, "total": 1, "message": message}
        }),
        config,
    )
}

pub(super) fn rpc_result(id: Value, result: Value, config: &Config) -> Result<(), CliError> {
    emit(
        &json!({"jsonrpc": JSONRPC, "id": id, "result": result}),
        config,
    )
}

pub(super) fn rpc_error(
    id: Value,
    code: i64,
    message: impl Into<String>,
    data: Option<Value>,
    config: &Config,
) -> Result<(), CliError> {
    let mut error = json!({"code": code, "message": message.into()});
    if let Some(data) = data {
        error["data"] = data;
    }
    emit(
        &json!({"jsonrpc": JSONRPC, "id": id, "error": error}),
        config,
    )
}

pub(super) fn emit(message: &Value, config: &Config) -> Result<(), CliError> {
    let mut line = json_canonical_string(message);
    if line.len().saturating_add(1) > config.max_output_bytes {
        let id = message.get("id").cloned().unwrap_or(Value::Null);
        let audit = message
            .pointer("/result/_meta/genesis~1sessionAudit")
            .or_else(|| message.pointer("/error/data/audit"))
            .cloned();
        line = json_canonical_string(&json!({
            "jsonrpc": JSONRPC,
            "id": id,
            "error": {
                "code": -32003,
                "message": "output frame exceeds configured limit",
                "data": {"audit": audit},
            }
        }));
    }
    let stdout = io::stdout();
    let mut lock = stdout.lock();
    writeln!(lock, "{line}")
        .and_then(|_| lock.flush())
        .map_err(|_| cli_err(EX_IO, "mcp/output", "failed to write MCP output"))
}

pub(super) fn has_cursor(params: &Value) -> bool {
    params.get("cursor").is_some_and(|cursor| !cursor.is_null())
}

pub(super) fn valid_rpc_atom(value: &Value) -> bool {
    value
        .as_str()
        .is_some_and(|value| !value.is_empty() && value.len() <= 256)
        || value.as_i64().is_some()
        || value.as_u64().is_some()
}

pub(super) fn rpc_key(value: &Value) -> String {
    json_canonical_string(value)
}
