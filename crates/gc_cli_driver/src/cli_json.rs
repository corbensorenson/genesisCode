use super::*;

pub(super) fn canonicalize_json(v: &serde_json::Value) -> serde_json::Value {
    match v {
        serde_json::Value::Object(m) => {
            let mut sorted: BTreeMap<String, serde_json::Value> = BTreeMap::new();
            for (k, vv) in m {
                sorted.insert(k.clone(), canonicalize_json(vv));
            }
            let mut out = serde_json::Map::new();
            for (k, vv) in sorted {
                out.insert(k, vv);
            }
            serde_json::Value::Object(out)
        }
        serde_json::Value::Array(xs) => {
            serde_json::Value::Array(xs.iter().map(canonicalize_json).collect())
        }
        _ => v.clone(),
    }
}

pub(super) fn json_canonical_string(v: &serde_json::Value) -> String {
    serde_json::to_string(&canonicalize_json(v)).unwrap_or_else(|e| {
        format!(
            "{{\"ok\":false,\"kind\":\"genesis/error-v0.2\",\"error\":{{\"code\":\"json/serialize\",\"message\":\"failed to render json output: {e}\"}}}}"
        )
    })
}

#[derive(Debug)]
pub(super) struct CliError {
    pub(super) exit_code: u8,
    pub(super) json: JsonError,
}

#[derive(Debug, Serialize)]
pub(super) struct JsonError {
    pub(super) code: &'static str,
    pub(super) message: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub(super) context: Option<serde_json::Value>,
}

#[derive(Debug, Serialize)]
pub(super) struct JsonEnvelope<T> {
    pub(super) ok: bool,
    pub(super) kind: &'static str,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub(super) data: Option<T>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub(super) error: Option<JsonError>,
}

pub(super) fn json_envelope_value<T: Serialize>(
    env: JsonEnvelope<T>,
) -> Result<serde_json::Value, CliError> {
    serde_json::to_value(env).map_err(|e| {
        cli_err(
            EX_INTERNAL,
            "json/serialize",
            format!("failed to serialize CLI json envelope: {e}"),
        )
    })
}
