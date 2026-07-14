use super::*;

pub(super) fn handle_frame(
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
                            "resources": config.resources.as_json(),
                            "resource_identity": config.resources.identity(),
                        },
                        "capabilities": {
                            "concurrent_control": matches!(flavor, Flavor::Native),
                            "queued_cancellation": true,
                            "running_cancellation": if matches!(flavor, Flavor::Native) && cfg!(any(target_os = "macos", target_os = "linux")) {
                                "process-tree-kill-and-reap"
                            } else {
                                "cooperative-result-suppression"
                            },
                            "hard_termination": matches!(flavor, Flavor::Native) && cfg!(any(target_os = "macos", target_os = "linux")),
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
            let argv = normalize_session_argv(argv);
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
                            argv,
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
                    state.cancelled_requests = state.cancelled_requests.saturating_add(1);
                    let audit =
                        SessionAudit::not_started(&config.resources, "explicit-queue-cancelled");
                    state.protocol_error(
                        Some(target.id),
                        "warm/cancelled",
                        "queued request was cancelled before execution",
                        false,
                        json!({
                            "accepted_index": target.accepted_index,
                            "hard_termination": false,
                            "audit": audit.as_json(),
                        }),
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
                    if let Some(control) = &running.control {
                        control.cancel();
                    }
                }
                state.emit_success(
                    &frame.id,
                    "cancellation-requested",
                    json!({
                        "target_id": target_id,
                        "target_state": "running",
                        "hard_termination": matches!(flavor, Flavor::Native) && cfg!(any(target_os = "macos", target_os = "linux")),
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
            state.emit_success(
                &frame.id,
                "draining",
                json!({
                    "running": state.running.is_some(),
                    "queued": state.pending.len(),
                }),
                evicted_workspaces,
            )?;
            begin_drain(state, config, "shutdown", false)?;
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

pub(super) fn begin_drain(
    state: &mut SessionState,
    config: &WarmConfig,
    reason: &'static str,
    input_closed: bool,
) -> Result<(), CliError> {
    if state.drain_deadline.is_some() {
        state.input_eof |= input_closed;
        return Ok(());
    }
    state.shutting_down = true;
    state.input_eof |= input_closed;
    state.drain_reason = Some(reason);
    state.drain_deadline = Instant::now().checked_add(config.resources.drain_timeout);
    let running_slots = usize::from(state.running.is_some());
    let keep_pending = config
        .resources
        .max_drain_requests
        .saturating_sub(running_slots)
        .min(state.pending.len());
    let cancelled = state.pending.split_off(keep_pending);
    for request in cancelled {
        state.cancelled_requests = state.cancelled_requests.saturating_add(1);
        let audit = SessionAudit::not_started(&config.resources, "bounded-drain-cancelled");
        state.protocol_error(
            Some(request.id),
            "warm/drain-bounded",
            "accepted request exceeded the bounded disconnect drain set",
            false,
            json!({
                "accepted_index": request.accepted_index,
                "reason": reason,
                "audit": audit.as_json(),
            }),
            0,
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

pub(super) fn enforce_drain_deadline(
    state: &mut SessionState,
    config: &WarmConfig,
) -> Result<(), CliError> {
    if state
        .drain_deadline
        .is_none_or(|deadline| Instant::now() < deadline)
    {
        return Ok(());
    }
    state.drain_deadline = None;
    if let Some(running) = state.running.as_mut() {
        running.drain_timeout = true;
        if let Some(control) = &running.control {
            control.cancel();
        }
    }
    while let Some(request) = state.pending.pop_front() {
        state.cancelled_requests = state.cancelled_requests.saturating_add(1);
        let audit = SessionAudit::not_started(&config.resources, "drain-timeout-cancelled");
        state.protocol_error(
            Some(request.id),
            "warm/drain-timeout",
            "accepted request was cancelled when the disconnect drain deadline expired",
            false,
            json!({
                "accepted_index": request.accepted_index,
                "reason": state.drain_reason,
                "audit": audit.as_json(),
            }),
            0,
        )?;
    }
    Ok(())
}

pub(super) fn expire_pending(
    state: &mut SessionState,
    config: &WarmConfig,
) -> Result<(), CliError> {
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
        } else {
            retained.push_back(request);
        }
    }
    state.pending = retained;
    Ok(())
}
