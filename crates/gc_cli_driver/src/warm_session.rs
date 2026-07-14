use std::collections::{HashMap, HashSet, VecDeque};
use std::io;
use std::sync::mpsc::{self, Receiver, TryRecvError};
use std::time::{Duration, Instant};

use serde_json::json;

use super::*;
use crate::session_resources::{SessionAudit, SessionResourceLimits};
use crate::warm_protocol::{
    InputEvent, WARM_PROTOCOL_V02, WarmFrame, WarmMethod, parse_frame, read_bounded_event,
    spawn_bounded_reader,
};
use crate::warm_request::{build_sub_cli, normalize_session_argv, validate_workspace_argv};
use crate::warm_session_config::{
    WarmConfig, WarmOptions, inherited_global_args, prime_runtime, warm_session_cache_key,
};
use crate::warm_state::{PendingRequest, RunningRequest, SessionState};
use crate::warm_worker::{WorkerJob, WorkerResult, run_worker_inline, spawn_worker};
use crate::warm_workspace::{evict_idle_workspaces, resolve_workspace};

const POLL_INTERVAL: Duration = Duration::from_millis(5);

mod admission;
use admission::{begin_drain, enforce_drain_deadline, expire_pending, handle_frame};

fn start_next(
    state: &mut SessionState,
    flavor: Flavor,
    worker_sender: &mpsc::Sender<WorkerResult>,
    config: &WarmConfig,
    inherited: &[String],
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
        state.cancelled_requests = state.cancelled_requests.saturating_add(1);
        let audit = SessionAudit::not_started(&config.resources, "queue-deadline-expired");
        state.protocol_error(
            Some(request.id),
            "warm/deadline-exceeded",
            "request deadline expired before execution",
            false,
            json!({
                "accepted_index": request.accepted_index,
                "phase": "queued",
                "hard_termination": false,
                "audit": audit.as_json(),
            }),
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
        drain_timeout: false,
        control: None,
    };
    let mut limits = config.resources.clone();
    if let Some(deadline) = request.deadline {
        limits.max_wall = limits
            .max_wall
            .min(deadline.saturating_duration_since(Instant::now()));
    }
    let job = WorkerJob {
        request_id: request.id,
        cli: request.cli,
        flavor,
        workspace_root: request.workspace_root,
        inherited: inherited.to_vec(),
        argv: request.argv,
        limits,
    };
    state.running = Some(running);
    if matches!(flavor, Flavor::Wasi) {
        return Ok(Some(run_worker_inline(job)));
    }
    match spawn_worker(job, worker_sender.clone()) {
        Ok(control) => {
            if let Some(running) = state.running.as_mut() {
                running.control = Some(control);
            }
        }
        Err(message) => {
            let running = state.running.take();
            let request_id = running.as_ref().map(|request| request.id.clone());
            let audit = SessionAudit::not_started(&config.resources, "worker-launch-failed");
            state.protocol_error(
                request_id,
                "warm/worker-launch",
                "failed to launch warm worker",
                true,
                json!({
                    "accepted_index": running.map(|request| request.accepted_index),
                    "reason": message,
                    "audit": audit.as_json(),
                }),
                0,
            )?;
        }
    }
    Ok(None)
}

fn handle_worker_result(
    state: &mut SessionState,
    outcome: WorkerResult,
    limits: &SessionResourceLimits,
) -> Result<(), CliError> {
    let outcome_id = match &outcome {
        WorkerResult::Completed { request_id, .. }
        | WorkerResult::CommandError { request_id, .. }
        | WorkerResult::WorkspaceError { request_id, .. }
        | WorkerResult::Crashed { request_id, .. }
        | WorkerResult::Cancelled { request_id, .. }
        | WorkerResult::ResourceExceeded { request_id, .. } => request_id,
    };
    if state
        .running
        .as_ref()
        .is_none_or(|running| running.id != *outcome_id)
    {
        return Ok(());
    }
    let Some(running) = state.running.take() else {
        return Ok(());
    };
    let audit = match &outcome {
        WorkerResult::Completed { audit, .. }
        | WorkerResult::CommandError { audit, .. }
        | WorkerResult::Crashed { audit, .. }
        | WorkerResult::Cancelled { audit, .. }
        | WorkerResult::ResourceExceeded { audit, .. } => Some(audit.as_json()),
        WorkerResult::WorkspaceError { audit, .. } => audit.as_ref().map(SessionAudit::as_json),
    };
    let hard_termination = audit
        .as_ref()
        .and_then(|audit| audit.get("worker_profile"))
        .and_then(serde_json::Value::as_str)
        == Some("native-isolated-v0.1");
    if running.drain_timeout {
        state.cancelled_requests = state.cancelled_requests.saturating_add(1);
        return state.protocol_error(
            Some(running.id),
            "warm/drain-timeout",
            "running request was terminated when the bounded drain expired",
            false,
            json!({
                "phase": "running",
                "reason": state.drain_reason,
                "hard_termination": hard_termination,
                "audit": audit,
            }),
            0,
        );
    }
    if running.deadline_expired
        || running
            .deadline
            .is_some_and(|deadline| Instant::now() >= deadline)
    {
        state.cancelled_requests = state.cancelled_requests.saturating_add(1);
        return state.protocol_error(
            Some(running.id),
            "warm/deadline-exceeded",
            "request deadline expired during execution",
            false,
            json!({"phase": "running", "hard_termination": hard_termination, "audit": audit}),
            0,
        );
    }
    if running.cancellation_requested {
        state.cancelled_requests = state.cancelled_requests.saturating_add(1);
        return state.protocol_error(
            Some(running.id),
            "warm/cancelled",
            "running request was cancelled and its worker was reaped",
            false,
            json!({"phase": "running", "hard_termination": hard_termination, "audit": audit}),
            0,
        );
    }
    match outcome {
        WorkerResult::Completed {
            result: Ok(output),
            audit,
            ..
        } => {
            state.completed_requests = state.completed_requests.saturating_add(1);
            state.emit_success(
                &running.id,
                "completed",
                json!({
                    "accepted_index": running.accepted_index,
                    "exit_code": output.exit_code,
                    "result": output.json,
                    "audit": audit.as_json(),
                }),
                0,
            )
        }
        WorkerResult::Completed {
            result: Err(error),
            audit,
            ..
        } => {
            state.completed_requests = state.completed_requests.saturating_add(1);
            state.protocol_error(
                Some(running.id),
                "warm/command-error",
                "command returned a typed CLI error",
                false,
                json!({
                    "accepted_index": running.accepted_index,
                    "exit_code": error.exit_code,
                    "command_error": error.json,
                    "audit": audit.as_json(),
                }),
                0,
            )
        }
        WorkerResult::CommandError {
            exit_code,
            envelope,
            audit,
            ..
        } => {
            state.completed_requests = state.completed_requests.saturating_add(1);
            state.protocol_error(
                Some(running.id),
                "warm/command-error",
                "command returned a typed CLI error",
                false,
                json!({
                    "accepted_index": running.accepted_index,
                    "exit_code": exit_code,
                    "command_envelope": envelope,
                    "audit": audit.as_json(),
                }),
                0,
            )
        }
        WorkerResult::WorkspaceError { message, audit, .. } => state.protocol_error(
            Some(running.id),
            "warm/workspace-transition",
            "worker could not enter or restore the request workspace",
            true,
            json!({"reason": message, "audit": audit.map(|audit| audit.as_json())}),
            0,
        ),
        WorkerResult::Crashed { audit, .. } => {
            state.protocol_error(
                Some(running.id),
                "warm/worker-crash",
                "worker crashed; session generation was reset",
                true,
                json!({"requires_initialize": true, "audit": audit.as_json()}),
                0,
            )?;
            state.discard_pending_after_crash(limits)?;
            state.crash_count = state.crash_count.saturating_add(1);
            state.generation = state.generation.saturating_add(1);
            state.initialized = false;
            state.workspaces.clear();
            state.seen_ids.clear();
            Ok(())
        }
        WorkerResult::Cancelled { audit, .. } => {
            state.cancelled_requests = state.cancelled_requests.saturating_add(1);
            state.protocol_error(
                Some(running.id),
                "warm/cancelled",
                "worker was cancelled and reaped",
                false,
                json!({"phase": "running", "hard_termination": true, "audit": audit.as_json()}),
                0,
            )
        }
        WorkerResult::ResourceExceeded {
            resource,
            command_envelope,
            audit,
            ..
        } => {
            state.resource_exceeded_requests = state.resource_exceeded_requests.saturating_add(1);
            state.protocol_error(
                Some(running.id),
                "warm/resource-exceeded",
                "isolated worker exceeded a session resource limit",
                false,
                json!({
                    "resource": resource,
                    "command_envelope": command_envelope,
                    "audit": audit.as_json(),
                }),
                0,
            )
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
            begin_drain(state, config, "input-io", true)?;
        }
        InputEvent::Eof => begin_drain(state, config, "eof", true)?,
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
            handle_worker_result(state, outcome, &config.resources)?;
        }
        enforce_drain_deadline(state, config)?;
        expire_pending(state, config)?;
        if let Some(outcome) = start_next(state, flavor, &worker_sender, config, inherited)? {
            handle_worker_result(state, outcome, &config.resources)?;
        }
        if let Some(running) = state.running.as_mut()
            && running
                .deadline
                .is_some_and(|deadline| Instant::now() >= deadline)
        {
            running.deadline_expired = true;
            if let Some(control) = &running.control {
                control.cancel();
            }
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
            Err(TryRecvError::Disconnected) => {
                begin_drain(state, config, "input-disconnected", true)?
            }
            Err(TryRecvError::Empty) => {}
        }
        if state.running.is_some() {
            if let Ok(outcome) = worker_receiver.recv_timeout(POLL_INTERVAL) {
                handle_worker_result(state, outcome, &config.resources)?;
            }
        } else if !state.input_eof && !state.shutting_down {
            match input_receiver.recv_timeout(POLL_INTERVAL) {
                Ok(event) => process_input_event(event, state, config, inherited, cli, flavor)?,
                Err(mpsc::RecvTimeoutError::Disconnected) => {
                    begin_drain(state, config, "input-disconnected", true)?
                }
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
        expire_pending(state, config)?;
        enforce_drain_deadline(state, config)?;
        if let Some(outcome) = start_next(state, flavor, &worker_sender, config, inherited)? {
            handle_worker_result(state, outcome, &config.resources)?;
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
    options: WarmOptions<'_>,
) -> Result<CmdOut, CliError> {
    let WarmOptions {
        prime_selfhost,
        max_queue,
        max_frame_bytes,
        max_workspaces,
        workspace_idle_ms,
        max_requests,
        workspace_root,
        resources,
    } = options;
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
    let resources = SessionResourceLimits::from_options(resources)
        .map_err(|message| cli_err(EX_PARSE, "warm/resource-limit", message))?;
    let config = WarmConfig {
        prime_selfhost,
        max_queue,
        max_frame_bytes,
        max_workspaces,
        workspace_idle: Duration::from_millis(workspace_idle_ms),
        max_requests,
        workspace_root,
        resources,
    };
    prime_runtime(cli, config.prime_selfhost)?;
    let inherited = inherited_global_args(cli, &config.resources);
    let mut state = SessionState {
        initialized: false,
        generation: 0,
        handled_frames: 0,
        accepted_requests: 0,
        response_sequence: 0,
        crash_count: 0,
        completed_requests: 0,
        cancelled_requests: 0,
        resource_exceeded_requests: 0,
        shutting_down: false,
        input_eof: false,
        drain_deadline: None,
        drain_reason: None,
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
            "requests_completed": state.completed_requests,
            "requests_cancelled": state.cancelled_requests,
            "requests_resource_exceeded": state.resource_exceeded_requests,
            "prime_selfhost": config.prime_selfhost,
            "session_cache_key": state.session_cache_key,
            "resource_limits": config.resources.as_json(),
            "resource_identity": config.resources.identity(),
            "drain": {
                "reason": state.drain_reason,
                "max_requests": config.resources.max_drain_requests,
                "timeout_ms": config.resources.drain_timeout.as_millis(),
            },
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
