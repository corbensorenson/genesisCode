use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

use serde_json::{Map, Value, json};
use url::Url;

use super::*;

pub(super) fn request_roots(state: &mut State, config: &Config) -> Result<(), CliError> {
    state.roots_ready = false;
    state.roots_request_sequence = state.roots_request_sequence.saturating_add(1);
    let request_id = json!(format!(
        "{ROOTS_REQUEST_ID}-{}",
        state.roots_request_sequence
    ));
    state.roots_request_id = Some(request_id.clone());
    emit(
        &json!({"jsonrpc": JSONRPC, "id": request_id, "method": "roots/list", "params": {}}),
        config,
    )
}

pub(super) fn process_response(
    object: &Map<String, Value>,
    state: &mut State,
    config: &Config,
) -> Result<(), CliError> {
    let Some(expected_id) = state.roots_request_id.as_ref() else {
        return Ok(());
    };
    if object.get("id") != Some(expected_id) {
        return Ok(());
    }
    state.roots_request_id = None;
    state.roots = object
        .get("result")
        .and_then(|result| result.get("roots"))
        .and_then(Value::as_array)
        .ok_or(())
        .and_then(|roots| validate_roots(roots, config).map_err(|_| ()))
        .unwrap_or_default();
    state.roots_ready = true;
    Ok(())
}

pub(super) fn validate_roots(
    roots: &[Value],
    config: &Config,
) -> Result<BTreeMap<String, PathBuf>, String> {
    if roots.len() > config.max_roots {
        return Err("client supplied too many roots".to_string());
    }
    let mut validated = BTreeMap::new();
    for root in roots {
        let uri = root
            .get("uri")
            .and_then(Value::as_str)
            .filter(|uri| uri.len() <= 16_384)
            .ok_or_else(|| "root URI is missing or too large".to_string())?;
        let url = Url::parse(uri).map_err(|_| "root URI is invalid".to_string())?;
        if url.scheme() != "file" || !matches!(url.host_str(), None | Some("") | Some("localhost"))
        {
            return Err("roots must be local file URIs".to_string());
        }
        let path = url
            .to_file_path()
            .map_err(|_| "root URI cannot be represented as a local path".to_string())?
            .canonicalize()
            .map_err(|_| "root must name an existing accessible directory".to_string())?;
        if !path.is_dir() || !path.starts_with(&config.workspace_boundary) {
            return Err("root resolves outside the configured workspace boundary".to_string());
        }
        if validated.insert(uri.to_string(), path).is_some() {
            return Err("duplicate root URI".to_string());
        }
    }
    Ok(validated)
}

pub(super) fn select_root(
    selected: Option<&Value>,
    state: &State,
) -> Result<PathBuf, &'static str> {
    if state.roots.is_empty() {
        return Err("client exposed no usable workspace roots");
    }
    if let Some(selected) = selected {
        let Some(uri) = selected.as_str() else {
            return Err("root must be an exact file URI string");
        };
        return state
            .roots
            .get(uri)
            .cloned()
            .ok_or("root was not returned by the negotiated roots/list response");
    }
    if state.roots.len() != 1 {
        return Err("root is required when the client exposes multiple workspaces");
    }
    state
        .roots
        .values()
        .next()
        .cloned()
        .ok_or("client exposed no usable workspace roots")
}

pub(super) fn file_uri(path: &Path) -> Option<String> {
    Url::from_directory_path(path).ok().map(Into::into)
}
