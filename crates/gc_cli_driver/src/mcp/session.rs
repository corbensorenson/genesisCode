use std::collections::{BTreeMap, BTreeSet, VecDeque};
use std::path::{Path, PathBuf};
use std::sync::mpsc::{self, Receiver, TryRecvError};
use std::time::Duration;

use super::super::*;
use super::McpOptions;
use super::catalog::{MCP_PROTOCOL_VERSION, ToolBinding, bindings, tool_argv};
use super::resources::{read_resource, resource_definitions};
use crate::warm_protocol::{InputEvent, spawn_bounded_reader};
use crate::warm_request::{build_sub_cli, validate_workspace_argv};
use crate::warm_session_config::{inherited_global_args, prime_runtime};
use crate::warm_worker::{WorkerJob, WorkerResult, run_worker_inline, spawn_worker};
use serde_json::{Map, Value, json};

const JSONRPC: &str = "2.0";
const ROOTS_REQUEST_ID: &str = "genesis-roots-1";
const POLL_INTERVAL: Duration = Duration::from_millis(5);

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
}

struct PendingCall {
    id: Value,
    key: String,
    progress_token: Option<Value>,
    cli: Cli,
    workspace_root: PathBuf,
}

struct RunningCall {
    id: Value,
    key: String,
    progress_token: Option<Value>,
    cancelled: bool,
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
    };
    serve(cli, flavor, &config)?;
    Ok(CmdOut {
        exit_code: EX_OK,
        stdout: String::new(),
        json: Value::Null,
    })
}

fn serve(cli: &Cli, flavor: Flavor, config: &Config) -> Result<(), CliError> {
    let tools = bindings(runtime_profile())
        .map_err(|message| cli_err(EX_INTERNAL, "mcp/interface-invalid", message))?;
    let mut inherited = inherited_global_args(cli);
    if !inherited.iter().any(|argument| argument == "--json") {
        inherited.insert(0, "--json".to_string());
    }
    let mut state = State::new(&config.workspace_boundary);
    let (input, _reader) =
        spawn_bounded_reader(config.max_frame_bytes, config.max_queue.saturating_add(16)).map_err(
            |_| {
                cli_err(
                    EX_IO,
                    "mcp/input",
                    "failed to start bounded MCP input reader",
                )
            },
        )?;
    let (worker_tx, worker_rx) = mpsc::channel();

    loop {
        drain_worker(&worker_rx, &mut state, config)?;
        start_next(&mut state, flavor, &worker_tx, config)?;
        if state.input_eof && state.running.is_none() {
            return Ok(());
        }
        match input.recv_timeout(POLL_INTERVAL) {
            Ok(event) => process_input(event, &mut state, config, &tools, &inherited, flavor)?,
            Err(mpsc::RecvTimeoutError::Timeout) => {}
            Err(mpsc::RecvTimeoutError::Disconnected) => state.input_eof = true,
        }
    }
}

fn process_input(
    event: InputEvent,
    state: &mut State,
    config: &Config,
    tools: &[ToolBinding],
    inherited: &[String],
    _flavor: Flavor,
) -> Result<(), CliError> {
    if matches!(
        event,
        InputEvent::Line(_) | InputEvent::Oversize | InputEvent::InvalidUtf8
    ) {
        if state.handled_frames >= config.max_requests {
            rpc_error(
                Value::Null,
                -32004,
                "session frame limit reached",
                None,
                config,
            )?;
            state.input_eof = true;
            cancel_all(state);
            return Ok(());
        }
        state.handled_frames = state.handled_frames.saturating_add(1);
    }
    match event {
        InputEvent::Line(line) => match serde_json::from_str::<Value>(&line) {
            Ok(value) => process_message(value, state, config, tools, inherited)?,
            Err(_) => rpc_error(Value::Null, -32700, "parse error", None, config)?,
        },
        InputEvent::Oversize => rpc_error(
            Value::Null,
            -32003,
            "input frame exceeds configured limit",
            Some(json!({"limit": config.max_frame_bytes})),
            config,
        )?,
        InputEvent::InvalidUtf8 => rpc_error(
            Value::Null,
            -32700,
            "input frame is not UTF-8",
            None,
            config,
        )?,
        InputEvent::IoError(_) | InputEvent::Eof => {
            state.input_eof = true;
            cancel_all(state);
        }
    }
    Ok(())
}

fn process_message(
    value: Value,
    state: &mut State,
    config: &Config,
    tools: &[ToolBinding],
    inherited: &[String],
) -> Result<(), CliError> {
    let Some(object) = value.as_object() else {
        return rpc_error(Value::Null, -32600, "invalid request", None, config);
    };
    if object.get("jsonrpc").and_then(Value::as_str) != Some(JSONRPC) {
        return rpc_error(
            object.get("id").cloned().unwrap_or(Value::Null),
            -32600,
            "invalid JSON-RPC version",
            None,
            config,
        );
    }
    if !object.contains_key("method") {
        return process_response(object, state, config);
    }
    let Some(method) = object.get("method").and_then(Value::as_str) else {
        return rpc_error(
            object.get("id").cloned().unwrap_or(Value::Null),
            -32600,
            "method must be a string",
            None,
            config,
        );
    };
    let id = match object.get("id") {
        Some(id) if valid_rpc_atom(id) => Some(id.clone()),
        Some(_) => {
            return rpc_error(
                Value::Null,
                -32600,
                "request id must be a string or integer",
                None,
                config,
            );
        }
        None => None,
    };
    let params = object.get("params").cloned().unwrap_or_else(|| json!({}));

    if method == "initialize" {
        let Some(id) = id else {
            return rpc_error(
                Value::Null,
                -32600,
                "initialize must be a request",
                None,
                config,
            );
        };
        return initialize(id, &params, state, config);
    }
    if !state.initialize_seen {
        if let Some(id) = id {
            return rpc_error(id, -32002, "server is not initialized", None, config);
        }
        return Ok(());
    }
    if method == "notifications/initialized" {
        if id.is_some() || !params.is_object() {
            return Ok(());
        }
        if state.initialized {
            return Ok(());
        }
        state.initialized = true;
        if state.client_roots {
            request_roots(state, config)?;
        }
        return Ok(());
    }
    if method == "notifications/cancelled" {
        cancel_request(&params, state);
        return Ok(());
    }
    if method == "notifications/roots/list_changed" {
        if state.client_roots && state.initialized {
            request_roots(state, config)?;
        }
        return Ok(());
    }
    if !state.initialized {
        if let Some(id) = id {
            return rpc_error(
                id,
                -32002,
                "initialized notification has not been received",
                None,
                config,
            );
        }
        return Ok(());
    }
    let Some(id) = id else {
        return Ok(());
    };
    match method {
        "ping" => rpc_result(id, json!({}), config),
        "tools/list" => list_tools(id, &params, tools, config),
        "tools/call" => enqueue_tool(id, &params, state, config, tools, inherited),
        "resources/list" => list_resources(id, &params, config),
        "resources/templates/list" => list_resource_templates(id, &params, config),
        "resources/read" => read_resource_request(id, &params, config),
        _ => rpc_error(id, -32601, "method not found", None, config),
    }
}

fn initialize(
    id: Value,
    params: &Value,
    state: &mut State,
    config: &Config,
) -> Result<(), CliError> {
    if state.initialize_seen {
        return rpc_error(id, -32600, "initialize may only be sent once", None, config);
    }
    let Some(params) = params.as_object() else {
        return rpc_error(
            id,
            -32602,
            "initialize params must be an object",
            None,
            config,
        );
    };
    if !params.get("protocolVersion").is_some_and(Value::is_string)
        || !params.get("capabilities").is_some_and(Value::is_object)
        || !params.get("clientInfo").is_some_and(Value::is_object)
    {
        return rpc_error(id, -32602, "initialize params are incomplete", None, config);
    }
    state.initialize_seen = true;
    state.client_roots = params["capabilities"]
        .get("roots")
        .is_some_and(Value::is_object);
    if state.client_roots {
        state.roots.clear();
        state.roots_ready = false;
    }
    rpc_result(
        id,
        json!({
            "protocolVersion": MCP_PROTOCOL_VERSION,
            "capabilities": {
                "tools": {"listChanged": false},
                "resources": {"subscribe": false, "listChanged": false}
            },
            "serverInfo": {
                "name": "genesiscode",
                "title": "GenesisCode Agent Toolchain",
                "version": env!("CARGO_PKG_VERSION")
            },
            "instructions": "Use generated tools and genesis:// resources. Paths are root-relative; MCP Tasks are not negotiated."
        }),
        config,
    )
}

fn list_tools(
    id: Value,
    params: &Value,
    tools: &[ToolBinding],
    config: &Config,
) -> Result<(), CliError> {
    if has_cursor(params) {
        return rpc_error(
            id,
            -32602,
            "pagination cursor is not supported",
            None,
            config,
        );
    }
    rpc_result(
        id,
        json!({"tools": tools.iter().map(|tool| tool.definition.clone()).collect::<Vec<_>>() }),
        config,
    )
}

fn list_resources(id: Value, params: &Value, config: &Config) -> Result<(), CliError> {
    if has_cursor(params) {
        return rpc_error(
            id,
            -32602,
            "pagination cursor is not supported",
            None,
            config,
        );
    }
    rpc_result(id, json!({"resources": resource_definitions()}), config)
}

fn list_resource_templates(id: Value, params: &Value, config: &Config) -> Result<(), CliError> {
    if has_cursor(params) {
        return rpc_error(
            id,
            -32602,
            "pagination cursor is not supported",
            None,
            config,
        );
    }
    rpc_result(id, json!({"resourceTemplates": []}), config)
}

fn read_resource_request(id: Value, params: &Value, config: &Config) -> Result<(), CliError> {
    let Some(uri) = params.get("uri").and_then(Value::as_str) else {
        return rpc_error(id, -32602, "resource URI is required", None, config);
    };
    match read_resource(uri, runtime_profile()) {
        Ok(result) => rpc_result(id, result, config),
        Err(message) => rpc_error(
            id,
            -32002,
            "resource read failed",
            Some(json!({"reason": message})),
            config,
        ),
    }
}

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
    });
    Ok(())
}

fn start_next(
    state: &mut State,
    flavor: Flavor,
    worker_tx: &mpsc::Sender<WorkerResult>,
    config: &Config,
) -> Result<(), CliError> {
    if state.running.is_some() || state.input_eof {
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
    };
    state.running = Some(RunningCall {
        id: pending.id,
        key: pending.key,
        progress_token: pending.progress_token,
        cancelled: false,
    });
    if matches!(flavor, Flavor::Wasi) {
        let result = run_worker_inline(job);
        return finish_worker(result, state, config);
    }
    if spawn_worker(job, worker_tx.clone()).is_err()
        && let Some(running) = state.running.take()
    {
        state.active_ids.remove(&running.key);
        rpc_error(
            running.id,
            -32603,
            "failed to start tool worker",
            None,
            config,
        )?;
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
        | WorkerResult::WorkspaceError { request_id, .. }
        | WorkerResult::Crashed { request_id } => request_id,
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
    if running.cancelled || state.input_eof {
        return Ok(());
    }
    if let Some(token) = &running.progress_token {
        progress(token, 1, "completed", config)?;
    }
    match result {
        WorkerResult::Completed {
            result: Ok(output), ..
        } => tool_result(running.id, output.json, output.exit_code != EX_OK, config),
        WorkerResult::Completed {
            result: Err(error), ..
        } => {
            let envelope = command_error_envelope(error);
            tool_result(running.id, envelope, true, config)
        }
        WorkerResult::WorkspaceError { .. } => rpc_error(
            running.id,
            -32603,
            "tool worker could not enter the selected workspace",
            None,
            config,
        ),
        WorkerResult::Crashed { .. } => rpc_error(
            running.id,
            -32603,
            "tool worker terminated unexpectedly",
            None,
            config,
        ),
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
    config: &Config,
) -> Result<(), CliError> {
    let text = json_canonical_string(&structured);
    rpc_result(
        id,
        json!({
            "content": [{"type": "text", "text": text}],
            "structuredContent": structured,
            "isError": is_error
        }),
        config,
    )
}
