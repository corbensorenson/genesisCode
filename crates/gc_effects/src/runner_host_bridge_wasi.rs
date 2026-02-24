use super::*;

fn map_lookup_str_or_sym(
    map: &std::collections::BTreeMap<TermOrdKey, Term>,
    key: &str,
) -> Option<Term> {
    map.get(&TermOrdKey(Term::symbol(key)))
        .or_else(|| map.get(&TermOrdKey(Term::Str(key.to_string()))))
        .cloned()
}

fn wasi_bridge_response_for_op(
    pol: Option<&OpPolicy>,
    op: &str,
) -> Result<Option<Term>, BridgeError> {
    let Some(pol) = pol else {
        return Ok(None);
    };

    if let Some(raw) = pol
        .extra
        .get("wasi_bridge_response")
        .and_then(|v| v.as_str())
    {
        let parsed = parse_term(raw).map_err(|e| BridgeError {
            code: "wasi/bridge-response-parse".to_string(),
            message: format!("wasi_bridge_response parse error: {e}"),
        })?;
        return Ok(Some(parsed));
    }

    if let Some(raw) = pol
        .extra
        .get("wasi_bridge_responses")
        .and_then(|v| v.as_str())
    {
        let parsed = parse_term(raw).map_err(|e| BridgeError {
            code: "wasi/bridge-responses-parse".to_string(),
            message: format!("wasi_bridge_responses parse error: {e}"),
        })?;
        if let Term::Map(m) = parsed
            && let Some(resp) = map_lookup_str_or_sym(&m, op)
        {
            return Ok(Some(resp));
        }
    }

    if let Some(file_raw) = pol
        .extra
        .get("wasi_bridge_response_file")
        .and_then(|v| v.as_str())
    {
        let base_dir = effective_base_dir(Some(pol)).map_err(|e| BridgeError {
            code: "wasi/bridge-response-file-path".to_string(),
            message: e.to_string(),
        })?;
        let file = sandbox_path_read(&base_dir, file_raw).map_err(|e| BridgeError {
            code: "wasi/bridge-response-file-path".to_string(),
            message: e.to_string(),
        })?;
        let bytes = std::fs::read(&file).map_err(|e| BridgeError {
            code: "wasi/bridge-response-file-read".to_string(),
            message: e.to_string(),
        })?;
        let parsed = decode_bridge_stdout("wasi", &bytes, None)?;
        if let Term::Map(m) = parsed.clone()
            && let Some(resp) = map_lookup_str_or_sym(&m, op)
        {
            return Ok(Some(resp));
        }
        return Ok(Some(parsed));
    }

    if let Ok(raw) = std::env::var("GENESIS_WASI_BRIDGE_RESPONSES") {
        let parsed = parse_term(&raw).map_err(|e| BridgeError {
            code: "wasi/bridge-env-parse".to_string(),
            message: e.to_string(),
        })?;
        if let Term::Map(m) = parsed
            && let Some(resp) = map_lookup_str_or_sym(&m, op)
        {
            return Ok(Some(resp));
        }
    }

    Ok(None)
}

pub(crate) fn run_wasi_bridge_profile(
    family: &str,
    op: &str,
    payload: &Term,
    pol: Option<&OpPolicy>,
    max_bytes: Option<usize>,
) -> Result<Term, BridgeError> {
    runner_host_bridge_policy::enforce_payload_limit(family, payload, max_bytes)?;
    let Some(response) = wasi_bridge_response_for_op(pol, op)? else {
        return Err(BridgeError {
            code: format!("{family}/bridge-wasi-profile-required"),
            message: format!(
                "{op} requires wasi bridge profile data (set per-op `wasi_bridge_response`/`wasi_bridge_response_file` or GENESIS_WASI_BRIDGE_RESPONSES)"
            ),
        });
    };
    runner_host_bridge_policy::enforce_response_limit(family, &response, max_bytes)?;
    Ok(response)
}
