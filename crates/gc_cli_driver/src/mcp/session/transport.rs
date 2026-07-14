use super::*;

const POLL_INTERVAL: Duration = Duration::from_millis(5);

pub(super) fn serve(cli: &Cli, flavor: Flavor, config: &Config) -> Result<(), CliError> {
    let tools = bindings(runtime_profile())
        .map_err(|message| cli_err(EX_INTERNAL, "mcp/interface-invalid", message))?;
    let inherited = inherited_global_args(cli, &config.resources);
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
        enforce_drain_deadline(&mut state, config)?;
        start_next(&mut state, flavor, &worker_tx, config)?;
        if state.input_eof && state.running.is_none() && state.pending.is_empty() {
            return Ok(());
        }
        match input.recv_timeout(POLL_INTERVAL) {
            Ok(event) => process_input(event, &mut state, config, &tools, &inherited, flavor)?,
            Err(mpsc::RecvTimeoutError::Timeout) => {}
            Err(mpsc::RecvTimeoutError::Disconnected) => {
                begin_drain(&mut state, config, "input-disconnected")?
            }
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
            begin_drain(state, config, "session-frame-limit")?;
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
        InputEvent::IoError(_) => begin_drain(state, config, "input-io")?,
        InputEvent::Eof => begin_drain(state, config, "eof")?,
    }
    Ok(())
}

fn begin_drain(state: &mut State, config: &Config, reason: &'static str) -> Result<(), CliError> {
    if state.drain_deadline.is_some() {
        state.input_eof = true;
        return Ok(());
    }
    state.input_eof = true;
    state.drain_reason = Some(reason);
    state.drain_deadline = std::time::Instant::now().checked_add(config.resources.drain_timeout);
    let running_slots = usize::from(state.running.is_some());
    let keep_pending = config
        .resources
        .max_drain_requests
        .saturating_sub(running_slots)
        .min(state.pending.len());
    let cancelled = state.pending.split_off(keep_pending);
    for pending in cancelled {
        state.active_ids.remove(&pending.key);
        state.cancelled_calls = state.cancelled_calls.saturating_add(1);
        let audit = SessionAudit::not_started(&config.resources, "bounded-drain-cancelled");
        rpc_error(
            pending.id,
            -32005,
            "accepted tool call exceeded the bounded disconnect drain set",
            Some(json!({"reason": reason, "audit": audit.as_json()})),
            config,
        )?;
    }
    if config.resources.max_drain_requests == 0
        && let Some(running) = state.running.as_mut()
    {
        running.drain_timeout = true;
        if let Some(control) = &running.control {
            control.cancel();
        }
    }
    Ok(())
}

fn enforce_drain_deadline(state: &mut State, config: &Config) -> Result<(), CliError> {
    if state
        .drain_deadline
        .is_none_or(|deadline| std::time::Instant::now() < deadline)
    {
        return Ok(());
    }
    state.drain_deadline = None;
    let reason = state.drain_reason;
    if let Some(running) = state.running.as_mut() {
        running.drain_timeout = true;
        if let Some(control) = &running.control {
            control.cancel();
        }
    }
    while let Some(pending) = state.pending.pop_front() {
        state.active_ids.remove(&pending.key);
        state.cancelled_calls = state.cancelled_calls.saturating_add(1);
        let audit = SessionAudit::not_started(&config.resources, "drain-timeout-cancelled");
        rpc_error(
            pending.id,
            -32006,
            "accepted tool call was cancelled when the disconnect drain expired",
            Some(json!({"reason": reason, "audit": audit.as_json()})),
            config,
        )?;
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
        if let Some(pending) = cancel_request(&params, state) {
            state.cancelled_calls = state.cancelled_calls.saturating_add(1);
            let audit = SessionAudit::not_started(&config.resources, "explicit-queue-cancelled");
            rpc_error(
                pending.id,
                -32800,
                "accepted queued tool call was cancelled before execution",
                Some(json!({"audit": audit.as_json()})),
                config,
            )?;
        }
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
                "resources": {"subscribe": false, "listChanged": false},
                "experimental": {
                    "genesis/sessionResources": {
                        "kind": "genesis/agent-session-resources-v0.1",
                        "identity": config.resources.identity(),
                        "limits": config.resources.as_json(),
                        "nativeHardTermination": cfg!(any(target_os = "macos", target_os = "linux")),
                    }
                }
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
