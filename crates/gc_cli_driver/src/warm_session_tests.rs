use super::*;
use std::path::PathBuf;

use crate::warm_workspace::WorkspaceEntry;

fn state() -> SessionState {
    SessionState {
        initialized: true,
        generation: 0,
        handled_frames: 0,
        accepted_requests: 0,
        response_sequence: 0,
        crash_count: 0,
        shutting_down: false,
        input_eof: false,
        session_cache_key: "0".repeat(64),
        seen_ids: HashSet::new(),
        workspaces: HashMap::new(),
        pending: VecDeque::new(),
        running: None,
    }
}

fn fixture_cli() -> Cli {
    Cli::parse_from(["genesis", "cli-schema"])
}

#[test]
fn idle_eviction_preserves_active_workspace() {
    let mut state = state();
    let stale = Instant::now()
        .checked_sub(Duration::from_secs(10))
        .unwrap_or_else(Instant::now);
    state.workspaces.insert(
        "idle".to_string(),
        WorkspaceEntry {
            root: PathBuf::from("idle"),
            last_used: stale,
        },
    );
    state.workspaces.insert(
        "active".to_string(),
        WorkspaceEntry {
            root: PathBuf::from("active"),
            last_used: stale,
        },
    );
    state.pending.push_back(PendingRequest {
        id: "request".to_string(),
        cli: fixture_cli(),
        workspace_id: "active".to_string(),
        workspace_root: PathBuf::from("active"),
        deadline: None,
        accepted_index: 0,
    });

    assert_eq!(evict_idle_workspaces(&mut state, Duration::from_secs(1)), 1);
    assert!(!state.workspaces.contains_key("idle"));
    assert!(state.workspaces.contains_key("active"));
}

#[test]
fn worker_crash_resets_generation_and_discards_queue() {
    let mut state = state();
    state.running = Some(RunningRequest {
        id: "running".to_string(),
        workspace_id: "ws".to_string(),
        deadline: None,
        accepted_index: 0,
        cancellation_requested: false,
        deadline_expired: false,
    });
    state.pending.push_back(PendingRequest {
        id: "queued".to_string(),
        cli: fixture_cli(),
        workspace_id: "ws".to_string(),
        workspace_root: PathBuf::from("ws"),
        deadline: None,
        accepted_index: 1,
    });

    handle_worker_result(
        &mut state,
        WorkerResult::Crashed {
            request_id: "running".to_string(),
        },
    )
    .unwrap();

    assert_eq!(state.generation, 1);
    assert_eq!(state.crash_count, 1);
    assert!(!state.initialized);
    assert!(state.pending.is_empty());
    assert!(state.running.is_none());
}
