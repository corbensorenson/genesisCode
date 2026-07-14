use std::path::{Component, Path};

use serde_json::json;

use super::*;
use crate::warm_protocol::ProtocolError;

pub(super) fn validate_workspace_argv(argv: &[String], root: &Path) -> Result<(), ProtocolError> {
    for argument in argv {
        let path = Path::new(argument);
        if path.is_absolute()
            || path
                .components()
                .any(|component| matches!(component, Component::ParentDir | Component::Prefix(_)))
        {
            return Err(ProtocolError {
                request_id: None,
                code: "warm/workspace-path",
                message: "request arguments may not use absolute or parent paths".to_string(),
                retryable: false,
                details: json!({}),
            });
        }
        let candidate = root.join(path);
        let boundary = if candidate.exists() {
            candidate.canonicalize().ok()
        } else {
            candidate
                .parent()
                .filter(|parent| parent.exists())
                .and_then(|parent| parent.canonicalize().ok())
        };
        if boundary.is_some_and(|candidate| !candidate.starts_with(root)) {
            return Err(ProtocolError {
                request_id: None,
                code: "warm/workspace-path-escape",
                message: "request path resolves outside its workspace".to_string(),
                retryable: false,
                details: json!({}),
            });
        }
    }
    Ok(())
}

pub(super) fn build_sub_cli(inherited: &[String], argv: &[String]) -> Result<Cli, ProtocolError> {
    let full_argv = std::iter::once("genesis".to_string())
        .chain(inherited.iter().cloned())
        .chain(argv.iter().cloned())
        .collect::<Vec<_>>();
    let parsed = Cli::try_parse_from(full_argv).map_err(|error| ProtocolError {
        request_id: None,
        code: "warm/argv-parse",
        message: error.to_string(),
        retryable: false,
        details: json!({}),
    })?;
    if matches!(parsed.cmd, Cmd::Warm { .. } | Cmd::Mcp { .. }) {
        return Err(ProtocolError {
            request_id: None,
            code: "warm/nested",
            message: "nested server commands are not allowed".to_string(),
            retryable: false,
            details: json!({}),
        });
    }
    Ok(parsed)
}
