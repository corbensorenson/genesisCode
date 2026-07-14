use std::collections::{HashMap, HashSet, VecDeque};
use std::io::{self, Write};
use std::path::PathBuf;
use std::time::Instant;

use serde_json::{Value, json};

use super::*;
use crate::session_resources::{SessionAudit, SessionResourceLimits};
use crate::warm_protocol::{ProtocolError, WARM_ERROR_V02, WARM_PROTOCOL_V02, WARM_RESPONSE_V02};
use crate::warm_worker::WorkerControl;
use crate::warm_workspace::WorkspaceEntry;

pub(super) struct PendingRequest {
    pub(super) id: String,
    pub(super) cli: Cli,
    pub(super) argv: Vec<String>,
    pub(super) workspace_id: String,
    pub(super) workspace_root: PathBuf,
    pub(super) deadline: Option<Instant>,
    pub(super) accepted_index: u64,
}

pub(super) struct RunningRequest {
    pub(super) id: String,
    pub(super) workspace_id: String,
    pub(super) deadline: Option<Instant>,
    pub(super) accepted_index: u64,
    pub(super) cancellation_requested: bool,
    pub(super) deadline_expired: bool,
    pub(super) drain_timeout: bool,
    pub(super) control: Option<WorkerControl>,
}

pub(super) struct SessionState {
    pub(super) initialized: bool,
    pub(super) generation: u64,
    pub(super) handled_frames: u64,
    pub(super) accepted_requests: u64,
    pub(super) response_sequence: u64,
    pub(super) crash_count: u64,
    pub(super) completed_requests: u64,
    pub(super) cancelled_requests: u64,
    pub(super) resource_exceeded_requests: u64,
    pub(super) shutting_down: bool,
    pub(super) input_eof: bool,
    pub(super) drain_deadline: Option<Instant>,
    pub(super) drain_reason: Option<&'static str>,
    pub(super) session_cache_key: String,
    pub(super) seen_ids: HashSet<String>,
    pub(super) workspaces: HashMap<String, WorkspaceEntry>,
    pub(super) pending: VecDeque<PendingRequest>,
    pub(super) running: Option<RunningRequest>,
}

fn emit_warm_line(value: &Value) -> Result<(), CliError> {
    let mut stdout = io::stdout().lock();
    writeln!(stdout, "{}", json_canonical_string(value))
        .map_err(|error| cli_err(EX_IO, "io/error", error.to_string()))?;
    stdout
        .flush()
        .map_err(|error| cli_err(EX_IO, "io/error", error.to_string()))
}

impl SessionState {
    fn metadata(&self, evicted_workspaces: usize) -> Value {
        json!({
            "generation": self.generation,
            "sequence": self.response_sequence,
            "session_cache_key": self.session_cache_key,
            "queue_depth": self.pending.len(),
            "workspace_count": self.workspaces.len(),
            "evicted_workspace_count": evicted_workspaces,
            "crash_count": self.crash_count,
        })
    }

    pub(super) fn emit_success(
        &mut self,
        request_id: &str,
        status: &'static str,
        data: Value,
        evicted_workspaces: usize,
    ) -> Result<(), CliError> {
        let value = json!({
            "protocol": WARM_PROTOCOL_V02,
            "id": request_id,
            "kind": WARM_RESPONSE_V02,
            "ok": true,
            "status": status,
            "data": data,
            "error": null,
            "meta": self.metadata(evicted_workspaces),
        });
        self.response_sequence = self.response_sequence.saturating_add(1);
        emit_warm_line(&value)
    }

    pub(super) fn emit_error(
        &mut self,
        error: ProtocolError,
        evicted_workspaces: usize,
    ) -> Result<(), CliError> {
        let value = json!({
            "protocol": WARM_PROTOCOL_V02,
            "id": error.request_id,
            "kind": WARM_RESPONSE_V02,
            "ok": false,
            "status": "error",
            "data": null,
            "error": {
                "schema": WARM_ERROR_V02,
                "code": error.code,
                "message": error.message,
                "retryable": error.retryable,
                "details": error.details,
            },
            "meta": self.metadata(evicted_workspaces),
        });
        self.response_sequence = self.response_sequence.saturating_add(1);
        emit_warm_line(&value)
    }

    pub(super) fn protocol_error(
        &mut self,
        request_id: Option<String>,
        code: &'static str,
        message: impl Into<String>,
        retryable: bool,
        details: Value,
        evicted_workspaces: usize,
    ) -> Result<(), CliError> {
        self.emit_error(
            ProtocolError {
                request_id,
                code,
                message: message.into(),
                retryable,
                details,
            },
            evicted_workspaces,
        )
    }

    pub(super) fn discard_pending_after_crash(
        &mut self,
        limits: &SessionResourceLimits,
    ) -> Result<(), CliError> {
        while let Some(request) = self.pending.pop_front() {
            self.cancelled_requests = self.cancelled_requests.saturating_add(1);
            let audit = SessionAudit::not_started(limits, "worker-crash-cancelled");
            self.protocol_error(
                Some(request.id),
                "warm/worker-restarted",
                "accepted request was cancelled during worker crash recovery",
                true,
                json!({
                    "accepted_index": request.accepted_index,
                    "requires_initialize": true,
                    "audit": audit.as_json(),
                }),
                0,
            )?;
        }
        Ok(())
    }
}
