use std::collections::{HashMap, HashSet, VecDeque};
use std::io;
use std::path::Path;
use std::sync::mpsc::{self, Receiver, TryRecvError};
use std::time::{Duration, Instant};

use serde_json::json;

use super::*;
use crate::warm_protocol::{
    InputEvent, WARM_PROTOCOL_V02, WarmFrame, WarmMethod, parse_frame, read_bounded_event,
    spawn_bounded_reader,
};
use crate::warm_request::{build_sub_cli, validate_workspace_argv};
use crate::warm_session_config::{
    WarmConfig, inherited_global_args, prime_runtime, warm_session_cache_key,
};
use crate::warm_state::{PendingRequest, RunningRequest, SessionState};
use crate::warm_worker::{WorkerJob, WorkerResult, run_worker_inline, spawn_worker};
use crate::warm_workspace::{evict_idle_workspaces, resolve_workspace};

const POLL_INTERVAL: Duration = Duration::from_millis(5);

fn handle_frame(
    frame: WarmFrame,
    state: &mut SessionState,
    config: &WarmConfig,
    inherited: &[String],
    cli: &Cli,
    flavor: Flavor,
    evicted_workspaces: usize,
) -> Result<Option<WorkerResult>, CliError> {
    if !state.seen_ids.insert(frame.id.clone()) {
        state.protocol_error(
            Some(frame.id),
            "warm/duplicate-id",
            "request ID was already used in this generation",
            false,
            json!({}),
            evicted_workspaces,
        )?;
        return Ok(None);
    }

    if !state.initialized && !matches!(frame.method, WarmMethod::Initialize { .. }) {
        state.protocol_error(
            Some(frame.id),
            "warm/not-initialized",
            "initialize must be the first method in each generation",
            true,
            json!({"generation": state.generation}),
            evicted_workspaces,
        )?;
        return Ok(None);
    }

    match frame.method {
        WarmMethod::Initialize {
            client_name,
            client_version,
        } => {
            if state.initialized {
                state.protocol_error(
                    Some(frame.id),
                    "warm/already-initialized",
                    "session generation is already initialized",
                    false,
                    json!({}),
                    evicted_workspaces,
                )?;
            } else {
                state.initialized = true;
                state.emit_success(
                    &frame.id,
                    "initialized",
                    json!({
                        "server": {"name": "genesis", "version": env!("CARGO_PKG_VERSION")},
                        "client": {"name": client_name, "version": client_version},
                        "limits": {
                            "max_queue": config.max_queue,
                            "max_frame_bytes": config.max_frame_bytes,
                            "max_workspaces": config.max_workspaces,
                            "workspace_idle_ms": config.workspace_idle.as_millis(),
                            "max_requests": config.max_requests,
                        },
                        "capabilities": {
                            "concurrent_control": matches!(flavor, Flavor::Native),
                            "queued_cancellation": true,
                            "running_cancellation": "cooperative-result-suppression",
                            "hard_termination": false,
                            "restart": "idle-only",
                        }
                    }),
                    evicted_workspaces,
                )?;
            }
        }
        WarmMethod::Execute {
            workspace,
            argv,
            deadline_ms,
        } => {
            if state.shutting_down {
                state.protocol_error(
                    Some(frame.id),
                    "warm/shutting-down",
                    "session is draining and no longer accepts execution",
                    true,
                    json!({}),
                    evicted_workspaces,
                )?;
            } else if state.pending.len() >= config.max_queue {
                state.protocol_error(
                    Some(frame.id),
                    "warm/queue-full",
                    "bounded execute queue is full",
                    true,
                    json!({"limit": config.max_queue}),
                    evicted_workspaces,
                )?;
            } else {
                let request_id = frame.id.clone();
                let result = resolve_workspace(state, config, &workspace).and_then(|root| {
                    validate_workspace_argv(&argv, &root)?;
                    let sub_cli = build_sub_cli(inherited, &argv)?;
                    Ok((root, sub_cli))
                });
                match result {
                    Ok((workspace_root, sub_cli)) => {
                        let deadline = deadline_ms.and_then(|milliseconds| {
                            Instant::now().checked_add(Duration::from_millis(milliseconds))
                        });
                        let accepted_index = state.accepted_requests;
                        state.accepted_requests = state.accepted_requests.saturating_add(1);
                        state.pending.push_back(PendingRequest {
                            id: request_id.clone(),
                            cli: sub_cli,
                            workspace_id: workspace.id,
                            workspace_root,
                            deadline,
                            accepted_index,
                        });
                        state.emit_success(
                            &request_id,
                            "accepted",
                            json!({"accepted_index": accepted_index}),
                            evicted_workspaces,
                        )?;
                    }
                    Err(mut error) => {
                        error.request_id = Some(request_id);
                        state.emit_error(error, evicted_workspaces)?;
                    }
                }
            }
        }
        WarmMethod::Cancel { target_id } => {
            if let Some(position) = state
                .pending
                .iter()
                .position(|request| request.id == target_id)
            {
                if let Some(target) = state.pending.remove(position) {
                    state.protocol_error(
                        Some(target.id),
                        "warm/cancelled",
                        "queued request was cancelled before execution",
                        false,
                        json!({"hard_termination": false}),
                        evicted_workspaces,
                    )?;
                }
                state.emit_success(
                    &frame.id,
                    "cancelled",
                    json!({"target_id": target_id, "target_state": "queued"}),
                    evicted_workspaces,
                )?;
            } else if state
                .running
                .as_ref()
                .is_some_and(|request| request.id == target_id)
            {
                if let Some(running) = state.running.as_mut() {
                    running.cancellation_requested = true;
                }
                state.emit_success(
                    &frame.id,
                    "cancellation-requested",
                    json!({
                        "target_id": target_id,
                        "target_state": "running",
                        "hard_termination": false,
                    }),
                    evicted_workspaces,
                )?;
            } else {
                state.protocol_error(
                    Some(frame.id),
                    "warm/cancel-target",
                    "cancel target is not queued or running",
                    false,
                    json!({"target_id": target_id}),
                    evicted_workspaces,
                )?;
            }
        }
        WarmMethod::Restart => {
            if state.running.is_some() || !state.pending.is_empty() {
                state.protocol_error(
                    Some(frame.id),
                    "warm/restart-busy",
                    "graceful restart requires an idle worker and empty queue",
                    true,
                    json!({}),
                    evicted_workspaces,
                )?;
            } else {
                match prime_runtime(cli, config.prime_selfhost) {
                    Ok(()) => {
                        state.generation = state.generation.saturating_add(1);
                        state.initialized = false;
                        state.workspaces.clear();
                        state.seen_ids.clear();
                        state.emit_success(
                            &frame.id,
                            "restarted",
                            json!({"requires_initialize": true}),
                            evicted_workspaces,
                        )?;
                    }
                    Err(error) => {
                        state.protocol_error(
                            Some(frame.id),
                            "warm/restart-failed",
                            "runtime priming failed during restart",
                            true,
                            json!({"command_error": error.json.code}),
                            evicted_workspaces,
                        )?;
                    }
                }
            }
        }
        WarmMethod::Shutdown => {
            state.shutting_down = true;
            state.emit_success(
                &frame.id,
                "draining",
                json!({
                    "running": state.running.is_some(),
                    "queued": state.pending.len(),
                }),
                evicted_workspaces,
            )?;
        }
        WarmMethod::Ping => {
            state.emit_success(
                &frame.id,
                "ready",
                json!({"initialized": state.initialized, "shutting_down": state.shutting_down}),
                evicted_workspaces,
            )?;
        }
    }
    Ok(None)
}

fn expire_pending(state: &mut SessionState) -> Result<(), CliError> {
    let expired = state
        .pending
        .iter()
        .filter(|request| {
            request
                .deadline
                .is_some_and(|deadline| Instant::now() >= deadline)
        })
        .map(|request| request.id.clone())
        .collect::<HashSet<_>>();
    if expired.is_empty() {
        return Ok(());
    }
    let mut retained = VecDeque::new();
    while let Some(request) = state.pending.pop_front() {
        if expired.contains(&request.id) {
            state.protocol_error(
                Some(request.id),
                "warm/deadline-exceeded",
                "request deadline expired before execution",
                false,
                json!({"phase": "queued", "hard_termination": false}),
                0,
            )?;
        } else {
            retained.push_back(request);
        }
    }
    state.pending = retained;
    Ok(())
}

fn start_next(
    state: &mut SessionState,
    flavor: Flavor,
    worker_sender: &mpsc::Sender<WorkerResult>,
) -> Result<Option<WorkerResult>, CliError> {
    if state.running.is_some() {
        return Ok(None);
    }
    let Some(request) = state.pending.pop_front() else {
        return Ok(None);
    };
    if request
        .deadline
        .is_some_and(|deadline| Instant::now() >= deadline)
    {
        state.protocol_error(
            Some(request.id),
            "warm/deadline-exceeded",
            "request deadline expired before execution",
            false,
            json!({"phase": "queued", "hard_termination": false}),
            0,
        )?;
        return Ok(None);
    }
    let running = RunningRequest {
        id: request.id.clone(),
        workspace_id: request.workspace_id,
        deadline: request.deadline,
        accepted_index: request.accepted_index,
        cancellation_requested: false,
        deadline_expired: false,
    };
    let job = WorkerJob {
        request_id: request.id,
        cli: request.cli,
        flavor,
        workspace_root: request.workspace_root,
    };
    state.running = Some(running);
    if matches!(flavor, Flavor::Wasi) {
        return Ok(Some(run_worker_inline(job)));
    }
    if let Err(message) = spawn_worker(job, worker_sender.clone()) {
        let request_id = state.running.take().map(|request| request.id);
        state.protocol_error(
            request_id,
            "warm/worker-launch",
            "failed to launch warm worker",
            true,
            json!({"reason": message}),
            0,
        )?;
    }
    Ok(None)
}

fn handle_worker_result(state: &mut SessionState, outcome: WorkerResult) -> Result<(), CliError> {
    let outcome_id = match &outcome {
        WorkerResult::Completed { request_id, .. }
        | WorkerResult::WorkspaceError { request_id, .. }
        | WorkerResult::Crashed { request_id } => request_id,
    };
    if !state
        .running
        .as_ref()
        .is_some_and(|running| running.id == *outcome_id)
    {
        return Ok(());
    }
    let Some(running) = state.running.take() else {
        return Ok(());
    };
    if running.deadline_expired
        || running
            .deadline
            .is_some_and(|deadline| Instant::now() >= deadline)
    {
        return state.protocol_error(
            Some(running.id),
            "warm/deadline-exceeded",
            "request deadline expired during execution",
            false,
            json!({"phase": "running", "hard_termination": false}),
            0,
        );
    }
    if running.cancellation_requested {
        return state.protocol_error(
            Some(running.id),
            "warm/cancelled",
            "running request completed after cooperative cancellation was requested",
            false,
            json!({"phase": "running", "hard_termination": false}),
            0,
        );
    }
    match outcome {
        WorkerResult::Completed {
            result: Ok(output), ..
        } => state.emit_success(
            &running.id,
            "completed",
            json!({
                "accepted_index": running.accepted_index,
                "exit_code": output.exit_code,
                "result": output.json,
            }),
            0,
        ),
        WorkerResult::Completed {
            result: Err(error), ..
        } => state.protocol_error(
            Some(running.id),
            "warm/command-error",
            "command returned a typed CLI error",
            false,
            json!({
                "accepted_index": running.accepted_index,
                "exit_code": error.exit_code,
                "command_error": error.json,
            }),
            0,
        ),
        WorkerResult::WorkspaceError { message, .. } => state.protocol_error(
            Some(running.id),
            "warm/workspace-transition",
            "worker could not enter or restore the request workspace",
            true,
            json!({"reason": message}),
            0,
        ),
        WorkerResult::Crashed { .. } => {
            state.protocol_error(
                Some(running.id),
                "warm/worker-crash",
                "worker crashed; session generation was reset",
                true,
                json!({"requires_initialize": true}),
                0,
            )?;
            state.discard_pending_after_crash()?;
            state.crash_count = state.crash_count.saturating_add(1);
            state.generation = state.generation.saturating_add(1);
            state.initialized = false;
            state.workspaces.clear();
            state.seen_ids.clear();
            Ok(())
        }
    }
}

fn process_input_event(
    event: InputEvent,
    state: &mut SessionState,
    config: &WarmConfig,
    inherited: &[String],
    cli: &Cli,
    flavor: Flavor,
) -> Result<(), CliError> {
    if matches!(
        event,
        InputEvent::Line(_) | InputEvent::Oversize | InputEvent::InvalidUtf8
    ) {
        if state.handled_frames >= config.max_requests {
            state.shutting_down = true;
            return state.protocol_error(
                None,
                "warm/session-limit",
                "warm session frame limit reached",
                false,
                json!({"limit": config.max_requests}),
                0,
            );
        }
        state.handled_frames = state.handled_frames.saturating_add(1);
    }
    match event {
        InputEvent::Line(line) => {
            let evicted = evict_idle_workspaces(state, config.workspace_idle);
            match parse_frame(&line) {
                Ok(frame) => {
                    let _ = handle_frame(frame, state, config, inherited, cli, flavor, evicted)?;
                }
                Err(error) => state.emit_error(error, evicted)?,
            }
        }
        InputEvent::Oversize => state.protocol_error(
            None,
            "warm/frame-too-large",
            "input frame exceeded the configured byte limit",
            false,
            json!({"limit": config.max_frame_bytes}),
            0,
        )?,
        InputEvent::InvalidUtf8 => state.protocol_error(
            None,
            "warm/frame-utf8",
            "input frame is not valid UTF-8",
            false,
            json!({}),
            0,
        )?,
        InputEvent::IoError(message) => {
            state.protocol_error(
                None,
                "warm/input-io",
                "warm input stream failed",
                true,
                json!({"reason": message}),
                0,
            )?;
            state.input_eof = true;
        }
        InputEvent::Eof => state.input_eof = true,
    }
    Ok(())
}

fn run_native_loop(
    cli: &Cli,
    flavor: Flavor,
    config: &WarmConfig,
    inherited: &[String],
    state: &mut SessionState,
) -> Result<(), CliError> {
    let capacity = config.max_queue.saturating_add(8);
    let (input_receiver, _reader_handle) =
        spawn_bounded_reader(config.max_frame_bytes, capacity)
            .map_err(|error| cli_err(EX_IO, "warm/reader-launch", error.to_string()))?;
    let (worker_sender, worker_receiver) = mpsc::channel();
    loop {
        if let Ok(outcome) = worker_receiver.try_recv() {
            handle_worker_result(state, outcome)?;
        }
        expire_pending(state)?;
        if let Some(outcome) = start_next(state, flavor, &worker_sender)? {
            handle_worker_result(state, outcome)?;
        }
        if let Some(running) = state.running.as_mut()
            && running
                .deadline
                .is_some_and(|deadline| Instant::now() >= deadline)
        {
            running.deadline_expired = true;
        }
        if (state.shutting_down || state.input_eof)
            && state.running.is_none()
            && state.pending.is_empty()
        {
            break;
        }
        match input_receiver.try_recv() {
            Ok(event) => {
                process_input_event(event, state, config, inherited, cli, flavor)?;
                continue;
            }
            Err(TryRecvError::Disconnected) => state.input_eof = true,
            Err(TryRecvError::Empty) => {}
        }
        if state.running.is_some() {
            if let Ok(outcome) = worker_receiver.recv_timeout(POLL_INTERVAL) {
                handle_worker_result(state, outcome)?;
            }
        } else if !state.input_eof && !state.shutting_down {
            match input_receiver.recv_timeout(POLL_INTERVAL) {
                Ok(event) => process_input_event(event, state, config, inherited, cli, flavor)?,
                Err(mpsc::RecvTimeoutError::Disconnected) => state.input_eof = true,
                Err(mpsc::RecvTimeoutError::Timeout) => {}
            }
        }
    }
    Ok(())
}

fn run_wasi_loop(
    cli: &Cli,
    flavor: Flavor,
    config: &WarmConfig,
    inherited: &[String],
    state: &mut SessionState,
) -> Result<(), CliError> {
    let stdin = io::stdin();
    let mut reader = stdin.lock();
    let (worker_sender, _worker_receiver): (_, Receiver<WorkerResult>) = mpsc::channel();
    loop {
        expire_pending(state)?;
        if let Some(outcome) = start_next(state, flavor, &worker_sender)? {
            handle_worker_result(state, outcome)?;
        }
        if (state.shutting_down || state.input_eof)
            && state.running.is_none()
            && state.pending.is_empty()
        {
            break;
        }
        let event = read_bounded_event(&mut reader, config.max_frame_bytes);
        process_input_event(event, state, config, inherited, cli, flavor)?;
    }
    Ok(())
}

pub(super) fn cmd_warm(
    cli: &Cli,
    flavor: Flavor,
    prime_selfhost: bool,
    max_queue: usize,
    max_frame_bytes: usize,
    max_workspaces: usize,
    workspace_idle_ms: u64,
    max_requests: u64,
    workspace_root: &Path,
) -> Result<CmdOut, CliError> {
    if !(1..=4096).contains(&max_queue) {
        return Err(cli_err(
            EX_PARSE,
            "warm/max-queue",
            "max_queue must be in 1..=4096",
        ));
    }
    if !(256..=16_777_216).contains(&max_frame_bytes) {
        return Err(cli_err(
            EX_PARSE,
            "warm/max-frame-bytes",
            "max_frame_bytes must be in 256..=16777216",
        ));
    }
    if !(1..=4096).contains(&max_workspaces) {
        return Err(cli_err(
            EX_PARSE,
            "warm/max-workspaces",
            "max_workspaces must be in 1..=4096",
        ));
    }
    let workspace_root = workspace_root.canonicalize().map_err(|_| {
        cli_err(
            EX_IO,
            "warm/workspace-root",
            "configured workspace root does not resolve to an existing directory",
        )
    })?;
    let config = WarmConfig {
        prime_selfhost,
        max_queue,
        max_frame_bytes,
        max_workspaces,
        workspace_idle: Duration::from_millis(workspace_idle_ms),
        max_requests,
        workspace_root,
    };
    prime_runtime(cli, config.prime_selfhost)?;
    let inherited = inherited_global_args(cli);
    let mut state = SessionState {
        initialized: false,
        generation: 0,
        handled_frames: 0,
        accepted_requests: 0,
        response_sequence: 0,
        crash_count: 0,
        shutting_down: false,
        input_eof: false,
        session_cache_key: warm_session_cache_key(cli, flavor, &config, &inherited),
        seen_ids: HashSet::new(),
        workspaces: HashMap::new(),
        pending: VecDeque::new(),
        running: None,
    };
    if matches!(flavor, Flavor::Native) {
        run_native_loop(cli, flavor, &config, &inherited, &mut state)?;
    } else {
        run_wasi_loop(cli, flavor, &config, &inherited, &mut state)?;
    }
    let envelope = JsonEnvelope {
        ok: true,
        kind: "genesis/warm-session-v0.2",
        data: Some(json!({
            "protocol": WARM_PROTOCOL_V02,
            "frames_handled": state.handled_frames,
            "requests_accepted": state.accepted_requests,
            "generation": state.generation,
            "crash_count": state.crash_count,
            "prime_selfhost": config.prime_selfhost,
            "session_cache_key": state.session_cache_key,
        })),
        error: None,
    };
    Ok(CmdOut {
        exit_code: EX_OK,
        stdout: String::new(),
        json: json_envelope_value(envelope)?,
    })
}

#[cfg(test)]
#[path = "warm_session_tests.rs"]
mod tests;
