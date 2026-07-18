use std::panic::{AssertUnwindSafe, catch_unwind};
use std::path::PathBuf;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc::Sender;
#[cfg(not(target_os = "wasi"))]
use std::thread;
use std::time::Instant;

use super::*;
use crate::session_resources::{SessionAudit, SessionResourceLimits};

#[cfg_attr(target_os = "wasi", allow(dead_code))]
pub(super) struct WorkerJob {
    pub(super) request_id: String,
    pub(super) cli: Cli,
    pub(super) flavor: Flavor,
    pub(super) workspace_root: PathBuf,
    pub(super) inherited: Vec<String>,
    pub(super) argv: Vec<String>,
    pub(super) limits: SessionResourceLimits,
}

#[derive(Clone)]
pub(super) struct WorkerControl {
    cancelled: Arc<AtomicBool>,
}

impl WorkerControl {
    pub(super) fn cancel(&self) {
        self.cancelled.store(true, Ordering::SeqCst);
    }
}

#[derive(Debug)]
#[cfg_attr(target_os = "wasi", allow(dead_code))]
pub(super) enum WorkerResult {
    Completed {
        request_id: String,
        result: Result<CmdOut, CliError>,
        audit: SessionAudit,
    },
    CommandError {
        request_id: String,
        exit_code: u8,
        envelope: serde_json::Value,
        audit: SessionAudit,
    },
    WorkspaceError {
        request_id: String,
        message: String,
        audit: Option<SessionAudit>,
    },
    Crashed {
        request_id: String,
        audit: SessionAudit,
    },
    Aborted {
        request_id: String,
        signal: i32,
        audit: SessionAudit,
    },
    Cancelled {
        request_id: String,
        audit: SessionAudit,
    },
    ResourceExceeded {
        request_id: String,
        resource: &'static str,
        command_envelope: Option<serde_json::Value>,
        audit: SessionAudit,
    },
}

fn run_job(job: WorkerJob) -> WorkerResult {
    let request_id = job.request_id.clone();
    let started = Instant::now();
    let original = match std::env::current_dir() {
        Ok(path) => path,
        Err(error) => {
            return WorkerResult::WorkspaceError {
                request_id,
                message: error.to_string(),
                audit: Some(SessionAudit::not_started(
                    &job.limits,
                    "workspace-state-unavailable",
                )),
            };
        }
    };
    if let Err(error) = std::env::set_current_dir(&job.workspace_root) {
        return WorkerResult::WorkspaceError {
            request_id,
            message: error.to_string(),
            audit: Some(SessionAudit::not_started(
                &job.limits,
                "workspace-transition-failed",
            )),
        };
    }
    let guarded = catch_unwind(AssertUnwindSafe(|| dispatch(&job.cli, job.flavor)));
    if let Err(error) = std::env::set_current_dir(original) {
        return WorkerResult::WorkspaceError {
            request_id,
            message: error.to_string(),
            audit: Some(SessionAudit {
                worker_profile: "wasi-inline-v0.1",
                limits_identity: job.limits.identity(),
                wall_ms: u64::try_from(started.elapsed().as_millis()).unwrap_or(u64::MAX),
                cpu_ms: None,
                output_bytes: 0,
                disk_delta_bytes: 0,
                effect_ops: None,
                peak_heap_bytes: None,
                peak_processes: None,
                termination: "workspace-restore-failed",
                exceeded: None,
            }),
        };
    }
    let result = match guarded {
        Ok(result) => result,
        Err(_) => {
            return WorkerResult::Crashed {
                request_id,
                audit: SessionAudit {
                    worker_profile: "wasi-inline-v0.1",
                    limits_identity: job.limits.identity(),
                    wall_ms: u64::try_from(started.elapsed().as_millis()).unwrap_or(u64::MAX),
                    cpu_ms: None,
                    output_bytes: 0,
                    disk_delta_bytes: 0,
                    effect_ops: None,
                    peak_heap_bytes: None,
                    peak_processes: None,
                    termination: "panic-contained",
                    exceeded: None,
                },
            };
        }
    };
    let output_bytes = result
        .as_ref()
        .map(|output| {
            json_canonical_string(&output.json)
                .len()
                .saturating_add(output.stdout.len()) as u64
        })
        .unwrap_or(0);
    let audit = SessionAudit {
        worker_profile: "wasi-inline-v0.1",
        limits_identity: job.limits.identity(),
        wall_ms: u64::try_from(started.elapsed().as_millis()).unwrap_or(u64::MAX),
        cpu_ms: None,
        output_bytes,
        disk_delta_bytes: 0,
        effect_ops: None,
        peak_heap_bytes: None,
        peak_processes: None,
        termination: "completed",
        exceeded: None,
    };
    if output_bytes > job.limits.max_output_bytes as u64 {
        return WorkerResult::ResourceExceeded {
            request_id,
            resource: "output",
            command_envelope: None,
            audit: SessionAudit {
                termination: "resource-rejected",
                exceeded: Some("output"),
                ..audit
            },
        };
    }
    WorkerResult::Completed {
        request_id,
        result,
        audit,
    }
}

#[cfg(all(not(target_os = "wasi"), any(target_os = "macos", target_os = "linux")))]
pub(super) fn spawn_worker(
    job: WorkerJob,
    sender: Sender<WorkerResult>,
) -> Result<WorkerControl, String> {
    let cancelled = Arc::new(AtomicBool::new(false));
    let worker_cancelled = Arc::clone(&cancelled);
    thread::Builder::new()
        .name("genesis-warm-worker".to_string())
        .spawn(move || {
            let _ = sender.send(crate::warm_worker_process::run_isolated(
                job,
                worker_cancelled,
            ));
        })
        .map(|_| WorkerControl { cancelled })
        .map_err(|error| error.to_string())
}

#[cfg(all(
    not(target_os = "wasi"),
    not(any(target_os = "macos", target_os = "linux"))
))]
pub(super) fn spawn_worker(
    _job: WorkerJob,
    _sender: Sender<WorkerResult>,
) -> Result<WorkerControl, String> {
    Err("native session isolation requires killable process-tree support".to_string())
}

#[cfg(target_os = "wasi")]
pub(super) fn spawn_worker(
    _job: WorkerJob,
    _sender: Sender<WorkerResult>,
) -> Result<WorkerControl, String> {
    Err("WASI sessions execute through the bounded inline worker".to_string())
}

pub(super) fn run_worker_inline(job: WorkerJob) -> WorkerResult {
    run_job(job)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn worker_panic_boundary_is_recoverable() {
        let result = catch_unwind(AssertUnwindSafe(|| -> Result<(), String> {
            panic!("test-only worker crash")
        }));
        assert!(result.is_err());
    }
}
