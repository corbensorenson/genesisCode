use std::collections::{BTreeMap, BTreeSet, VecDeque};
use std::path::{Path, PathBuf};
use std::sync::mpsc::{self, Receiver, TryRecvError};
use std::time::Duration;

use super::super::*;
use super::McpOptions;
use super::catalog::{MCP_PROTOCOL_VERSION, ToolBinding, bindings, tool_argv};
use super::resources::{read_resource, resource_definitions};
use crate::session_resources::{SessionAudit, SessionResourceLimits};
use crate::warm_protocol::{InputEvent, spawn_bounded_reader};
use crate::warm_request::{build_sub_cli, normalize_session_argv, validate_workspace_argv};
use crate::warm_session_config::{inherited_global_args, prime_runtime};
use crate::warm_worker::WorkerControl;
use crate::warm_worker::{WorkerJob, WorkerResult, run_worker_inline, spawn_worker};
use serde_json::{Map, Value, json};

const JSONRPC: &str = "2.0";
const ROOTS_REQUEST_ID: &str = "genesis-roots-1";
mod cancellation;
mod roots;
mod wire;

use cancellation::*;
use roots::*;
use wire::*;

#[cfg(test)]
mod tests;

struct Config {
    max_queue: usize,
    max_frame_bytes: usize,
    max_output_bytes: usize,
    max_requests: u64,
    max_roots: usize,
    workspace_boundary: PathBuf,
    resources: SessionResourceLimits,
}

struct PendingCall {
    id: Value,
    key: String,
    progress_token: Option<Value>,
    cli: Cli,
    workspace_root: PathBuf,
    argv: Vec<String>,
    inherited: Vec<String>,
}

struct RunningCall {
    id: Value,
    key: String,
    progress_token: Option<Value>,
    cancelled: bool,
    drain_timeout: bool,
    control: Option<WorkerControl>,
}

struct State {
    initialize_seen: bool,
    initialized: bool,
    client_roots: bool,
    roots_ready: bool,
    roots_request_id: Option<Value>,
    roots_request_sequence: u64,
    roots: BTreeMap<String, PathBuf>,
    pending: VecDeque<PendingCall>,
    running: Option<RunningCall>,
    active_ids: BTreeSet<String>,
    handled_frames: u64,
    input_eof: bool,
    drain_deadline: Option<std::time::Instant>,
    drain_reason: Option<&'static str>,
    completed_calls: u64,
    cancelled_calls: u64,
    resource_exceeded_calls: u64,
}

impl State {
    fn new(boundary: &Path) -> Self {
        Self {
            initialize_seen: false,
            initialized: false,
            client_roots: false,
            roots_ready: true,
            roots_request_id: None,
            roots_request_sequence: 0,
            roots: BTreeMap::from([(
                file_uri(boundary).unwrap_or_else(|| "genesis://workspace/default".to_string()),
                boundary.to_path_buf(),
            )]),
            pending: VecDeque::new(),
            running: None,
            active_ids: BTreeSet::new(),
            handled_frames: 0,
            input_eof: false,
            drain_deadline: None,
            drain_reason: None,
            completed_calls: 0,
            cancelled_calls: 0,
            resource_exceeded_calls: 0,
        }
    }
}

pub(crate) fn cmd_mcp(
    cli: &Cli,
    flavor: Flavor,
    options: McpOptions<'_>,
) -> Result<CmdOut, CliError> {
    let McpOptions {
        prime_selfhost,
        max_queue,
        max_frame_bytes,
        max_output_bytes,
        max_requests,
        max_roots,
        workspace_root,
        resources,
    } = options;
    if !(1..=4096).contains(&max_queue)
        || !(256..=16_777_216).contains(&max_frame_bytes)
        || !(1024..=16_777_216).contains(&max_output_bytes)
        || max_requests == 0
        || !(1..=1024).contains(&max_roots)
    {
        return Err(cli_err(
            EX_PARSE,
            "mcp/limit-invalid",
            "MCP resource limits are outside their supported bounds",
        ));
    }
    let workspace_boundary = workspace_root.canonicalize().map_err(|_| {
        cli_err(
            EX_IO,
            "mcp/workspace-root",
            "MCP workspace boundary must be an existing accessible directory",
        )
    })?;
    let resources = SessionResourceLimits::from_options(resources)
        .map_err(|message| cli_err(EX_PARSE, "mcp/resource-limit", message))?;
    if !workspace_boundary.is_dir() {
        return Err(cli_err(
            EX_IO,
            "mcp/workspace-root",
            "MCP workspace boundary must be a directory",
        ));
    }
    prime_runtime(cli, prime_selfhost)?;
    let config = Config {
        max_queue,
        max_frame_bytes,
        max_output_bytes,
        max_requests,
        max_roots,
        workspace_boundary,
        resources,
    };
    serve(cli, flavor, &config)?;
    Ok(CmdOut {
        exit_code: EX_OK,
        stdout: String::new(),
        json: Value::Null,
    })
}

mod transport;
use transport::serve;

fn enqueue_tool(
    id: Value,
    params: &Value,
    state: &mut State,
    config: &Config,
    tools: &[ToolBinding],
    inherited: &[String],
) -> Result<(), CliError> {
    if !state.roots_ready {
        return rpc_error(id, -32001, "workspace roots are not ready", None, config);
    }
    if state.pending.len() >= config.max_queue {
        return rpc_error(id, -32000, "bounded tool queue is full", None, config);
    }
    let Some(params) = params.as_object() else {
        return rpc_error(
            id,
            -32602,
            "tool call params must be an object",
            None,
            config,
        );
    };
    if params.contains_key("task") {
        return rpc_error(id, -32602, "MCP Tasks were not negotiated", None, config);
    }
    if let Some(unknown) = params
        .keys()
        .find(|key| !matches!(key.as_str(), "name" | "arguments" | "_meta"))
    {
        return rpc_error(
            id,
            -32602,
            format!("unknown tool-call field `{unknown}`"),
            None,
            config,
        );
    }
    let Some(name) = params.get("name").and_then(Value::as_str) else {
        return rpc_error(id, -32602, "tool name is required", None, config);
    };
    let Some(tool) = tools.iter().find(|tool| tool.name == name) else {
        return rpc_error(id, -32602, "unknown tool name", None, config);
    };
    let arguments = match params.get("arguments") {
        None => Map::new(),
        Some(Value::Object(arguments)) => arguments.clone(),
        Some(_) => return rpc_error(id, -32602, "tool arguments must be an object", None, config),
    };
    let workspace_root = match select_root(arguments.get("root"), state) {
        Ok(root) => root,
        Err(message) => return rpc_error(id, -32602, message, None, config),
    };
    let argv = match tool_argv(tool, &arguments) {
        Ok(argv) => argv,
        Err(message) => return rpc_error(id, -32602, message, None, config),
    };
    let argv = normalize_session_argv(argv);
    if let Err(error) = validate_workspace_argv(&argv, &workspace_root) {
        return rpc_error(id, -32602, error.message, None, config);
    }
    let cli = match build_sub_cli(inherited, &argv) {
        Ok(cli) => cli,
        Err(error) => return rpc_error(id, -32602, error.message, None, config),
    };
    let progress_token = params
        .get("_meta")
        .and_then(|meta| meta.get("progressToken"))
        .cloned();
    if progress_token
        .as_ref()
        .is_some_and(|token| !valid_rpc_atom(token))
    {
        return rpc_error(
            id,
            -32602,
            "progress token must be a string or integer",
            None,
            config,
        );
    }
    let key = rpc_key(&id);
    if !state.active_ids.insert(key.clone()) {
        return rpc_error(id, -32600, "request id is already active", None, config);
    }
    state.pending.push_back(PendingCall {
        id,
        key,
        progress_token,
        cli,
        workspace_root,
        argv,
        inherited: inherited.to_vec(),
    });
    Ok(())
}

fn start_next(
    state: &mut State,
    flavor: Flavor,
    worker_tx: &mpsc::Sender<WorkerResult>,
    config: &Config,
) -> Result<(), CliError> {
    if state.running.is_some() {
        return Ok(());
    }
    let Some(pending) = state.pending.pop_front() else {
        return Ok(());
    };
    if let Some(token) = &pending.progress_token {
        progress(token, 0, "started", config)?;
    }
    let request_id = pending.key.clone();
    let job = WorkerJob {
        request_id,
        cli: pending.cli,
        flavor,
        workspace_root: pending.workspace_root,
        inherited: pending.inherited,
        argv: pending.argv,
        limits: config.resources.clone(),
    };
    state.running = Some(RunningCall {
        id: pending.id,
        key: pending.key,
        progress_token: pending.progress_token,
        cancelled: false,
        drain_timeout: false,
        control: None,
    });
    if matches!(flavor, Flavor::Wasi) {
        let result = run_worker_inline(job);
        return finish_worker(result, state, config);
    }
    match spawn_worker(job, worker_tx.clone()) {
        Ok(control) => {
            if let Some(running) = state.running.as_mut() {
                running.control = Some(control);
            }
        }
        Err(_) => {
            if let Some(running) = state.running.take() {
                state.active_ids.remove(&running.key);
                let audit = SessionAudit::not_started(&config.resources, "worker-launch-failed");
                rpc_error(
                    running.id,
                    -32603,
                    "failed to start isolated tool worker",
                    Some(json!({"audit": audit.as_json()})),
                    config,
                )?;
            }
        }
    }
    Ok(())
}

fn drain_worker(
    worker_rx: &Receiver<WorkerResult>,
    state: &mut State,
    config: &Config,
) -> Result<(), CliError> {
    loop {
        match worker_rx.try_recv() {
            Ok(result) => finish_worker(result, state, config)?,
            Err(TryRecvError::Empty) => return Ok(()),
            Err(TryRecvError::Disconnected) => return Ok(()),
        }
    }
}

fn finish_worker(result: WorkerResult, state: &mut State, config: &Config) -> Result<(), CliError> {
    let result_key = match &result {
        WorkerResult::Completed { request_id, .. }
        | WorkerResult::CommandError { request_id, .. }
        | WorkerResult::WorkspaceError { request_id, .. }
        | WorkerResult::Crashed { request_id, .. }
        | WorkerResult::Aborted { request_id, .. }
        | WorkerResult::Cancelled { request_id, .. }
        | WorkerResult::ResourceExceeded { request_id, .. } => request_id,
    };
    if state
        .running
        .as_ref()
        .is_none_or(|running| running.key != *result_key)
    {
        return Ok(());
    }
    let Some(running) = state.running.take() else {
        return Ok(());
    };
    state.active_ids.remove(&running.key);
    let audit = match &result {
        WorkerResult::Completed { audit, .. }
        | WorkerResult::CommandError { audit, .. }
        | WorkerResult::Crashed { audit, .. }
        | WorkerResult::Aborted { audit, .. }
        | WorkerResult::Cancelled { audit, .. }
        | WorkerResult::ResourceExceeded { audit, .. } => Some(audit.as_json()),
        WorkerResult::WorkspaceError { audit, .. } => audit.as_ref().map(SessionAudit::as_json),
    };
    if running.drain_timeout {
        state.cancelled_calls = state.cancelled_calls.saturating_add(1);
        return rpc_error(
            running.id,
            -32006,
            "tool call was terminated when the bounded disconnect drain expired",
            Some(json!({"reason": state.drain_reason, "audit": audit})),
            config,
        );
    }
    if running.cancelled {
        state.cancelled_calls = state.cancelled_calls.saturating_add(1);
        return rpc_error(
            running.id,
            -32800,
            "tool call was cancelled and its worker was reaped",
            Some(json!({"audit": audit})),
            config,
        );
    }
    if let Some(token) = &running.progress_token {
        progress(token, 1, "completed", config)?;
    }
    match result {
        WorkerResult::Completed {
            result: Ok(output),
            audit,
            ..
        } => {
            state.completed_calls = state.completed_calls.saturating_add(1);
            tool_result(
                running.id,
                output.json,
                output.exit_code != EX_OK,
                &audit,
                config,
            )
        }
        WorkerResult::Completed {
            result: Err(error),
            audit,
            ..
        } => {
            state.completed_calls = state.completed_calls.saturating_add(1);
            let envelope = command_error_envelope(error);
            tool_result(running.id, envelope, true, &audit, config)
        }
        WorkerResult::CommandError {
            envelope, audit, ..
        } => {
            state.completed_calls = state.completed_calls.saturating_add(1);
            tool_result(running.id, envelope, true, &audit, config)
        }
        WorkerResult::WorkspaceError { audit, .. } => rpc_error(
            running.id,
            -32603,
            "tool worker could not enter the selected workspace",
            Some(json!({"audit": audit.map(|audit| audit.as_json())})),
            config,
        ),
        WorkerResult::Crashed { audit, .. } => rpc_error(
            running.id,
            -32603,
            "tool worker terminated unexpectedly",
            Some(json!({"audit": audit.as_json()})),
            config,
        ),
        WorkerResult::Aborted { signal, audit, .. } => rpc_error(
            running.id,
            -32008,
            "isolated tool worker terminated abnormally; the server remains available",
            Some(json!({
                "server_available": true,
                "signal": signal,
                "audit": audit.as_json(),
            })),
            config,
        ),
        WorkerResult::Cancelled { audit, .. } => {
            state.cancelled_calls = state.cancelled_calls.saturating_add(1);
            rpc_error(
                running.id,
                -32800,
                "tool worker was cancelled and reaped",
                Some(json!({"audit": audit.as_json()})),
                config,
            )
        }
        WorkerResult::ResourceExceeded {
            resource,
            command_envelope,
            audit,
            ..
        } => {
            state.resource_exceeded_calls = state.resource_exceeded_calls.saturating_add(1);
            rpc_error(
                running.id,
                -32007,
                "tool worker exceeded a session resource limit",
                Some(json!({
                    "resource": resource,
                    "command_envelope": command_envelope,
                    "audit": audit.as_json(),
                })),
                config,
            )
        }
    }
}

fn command_error_envelope(error: CliError) -> Value {
    let value = json_envelope_value(JsonEnvelope::<Value> {
        ok: false,
        kind: "genesis/error-v0.2",
        data: None,
        error: Some(error.json),
    })
    .unwrap_or_else(|_| json!({"ok": false, "kind": "genesis/error-v0.2"}));
    annotate_envelope(value, error.exit_code)
}

fn tool_result(
    id: Value,
    structured: Value,
    is_error: bool,
    audit: &SessionAudit,
    config: &Config,
) -> Result<(), CliError> {
    let text = json_canonical_string(&structured);
    rpc_result(
        id,
        json!({
            "content": [{"type": "text", "text": text}],
            "structuredContent": structured,
            "isError": is_error,
            "_meta": {"genesis/sessionAudit": audit.as_json()}
        }),
        config,
    )
}
