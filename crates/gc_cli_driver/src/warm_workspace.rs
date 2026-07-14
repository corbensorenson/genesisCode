use std::collections::HashSet;
use std::path::{Component, Path, PathBuf};
use std::time::{Duration, Instant};

use serde_json::json;

use crate::warm_protocol::{ProtocolError, WorkspaceRef};
use crate::warm_session_config::WarmConfig;
use crate::warm_state::SessionState;

pub(super) struct WorkspaceEntry {
    pub(super) root: PathBuf,
    pub(super) last_used: Instant,
}

fn active_workspace_ids(state: &SessionState) -> HashSet<String> {
    state
        .pending
        .iter()
        .map(|request| request.workspace_id.clone())
        .chain(
            state
                .running
                .iter()
                .map(|request| request.workspace_id.clone()),
        )
        .collect()
}

pub(super) fn evict_idle_workspaces(state: &mut SessionState, idle: Duration) -> usize {
    let active = active_workspace_ids(state);
    let evicted = state
        .workspaces
        .iter()
        .filter(|(id, entry)| !active.contains(*id) && entry.last_used.elapsed() >= idle)
        .map(|(id, _)| id.clone())
        .collect::<Vec<_>>();
    for id in &evicted {
        state.workspaces.remove(id);
    }
    evicted.len()
}

pub(super) fn resolve_workspace(
    state: &mut SessionState,
    config: &WarmConfig,
    workspace: &WorkspaceRef,
) -> Result<PathBuf, ProtocolError> {
    let requested = Path::new(&workspace.root);
    if requested.is_absolute()
        || requested
            .components()
            .any(|component| matches!(component, Component::ParentDir | Component::Prefix(_)))
    {
        return Err(ProtocolError {
            request_id: None,
            code: "warm/workspace-root",
            message: "workspace root must be relative and may not contain parent components"
                .to_string(),
            retryable: false,
            details: json!({"workspace_id": workspace.id}),
        });
    }
    let resolved = config
        .workspace_root
        .join(requested)
        .canonicalize()
        .map_err(|_| ProtocolError {
            request_id: None,
            code: "warm/workspace-unavailable",
            message: "workspace root does not resolve to an existing directory".to_string(),
            retryable: true,
            details: json!({"workspace_id": workspace.id}),
        })?;
    if !resolved.is_dir() || !resolved.starts_with(&config.workspace_root) {
        return Err(ProtocolError {
            request_id: None,
            code: "warm/workspace-escape",
            message: "workspace root escapes the configured workspace boundary".to_string(),
            retryable: false,
            details: json!({"workspace_id": workspace.id}),
        });
    }
    if let Some(existing) = state.workspaces.get_mut(&workspace.id) {
        if existing.root != resolved {
            return Err(ProtocolError {
                request_id: None,
                code: "warm/workspace-rebind",
                message: "workspace ID is already bound to another root".to_string(),
                retryable: false,
                details: json!({"workspace_id": workspace.id}),
            });
        }
        existing.last_used = Instant::now();
        return Ok(existing.root.clone());
    }
    if state.workspaces.len() >= config.max_workspaces {
        return Err(ProtocolError {
            request_id: None,
            code: "warm/workspace-limit",
            message: "workspace registry is full".to_string(),
            retryable: true,
            details: json!({"limit": config.max_workspaces}),
        });
    }
    state.workspaces.insert(
        workspace.id.clone(),
        WorkspaceEntry {
            root: resolved.clone(),
            last_used: Instant::now(),
        },
    );
    Ok(resolved)
}
