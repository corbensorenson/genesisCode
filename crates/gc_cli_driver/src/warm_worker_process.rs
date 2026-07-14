#![cfg(any(target_os = "macos", target_os = "linux"))]

use std::fs;
use std::io::Read;
use std::path::Path;
use std::process::{Command, ExitStatus, Stdio};
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::thread;
use std::time::{Duration, Instant};

use serde_json::Value;

use super::*;
use crate::session_resources::SessionAudit;
use crate::warm_worker::{WorkerJob, WorkerResult};

const POLL_INTERVAL: Duration = Duration::from_millis(5);
const DISK_POLL_INTERVAL: Duration = Duration::from_millis(50);
const MAX_ACCOUNTED_FILES: usize = 200_000;

struct CapturedPipe {
    bytes: Vec<u8>,
    observed: u64,
    exceeded: bool,
}

enum CommandResult {
    Output(CmdOut),
    Error { exit_code: u8, envelope: Value },
}

fn capture_pipe<R: Read + Send + 'static>(
    mut pipe: R,
    limit: usize,
    total_observed: Arc<AtomicU64>,
    total_exceeded: Arc<AtomicBool>,
) -> thread::JoinHandle<CapturedPipe> {
    thread::spawn(move || {
        let mut bytes = Vec::with_capacity(limit.min(64 * 1024));
        let mut observed = 0_u64;
        let mut chunk = [0_u8; 16 * 1024];
        loop {
            let read = match pipe.read(&mut chunk) {
                Ok(0) | Err(_) => break,
                Ok(read) => read,
            };
            observed = observed.saturating_add(read as u64);
            let total = total_observed
                .fetch_add(read as u64, Ordering::SeqCst)
                .saturating_add(read as u64);
            if total > limit as u64 {
                total_exceeded.store(true, Ordering::SeqCst);
            }
            let remaining = limit.saturating_sub(bytes.len());
            bytes.extend_from_slice(&chunk[..read.min(remaining)]);
        }
        CapturedPipe {
            bytes,
            observed,
            exceeded: total_exceeded.load(Ordering::SeqCst),
        }
    })
}

fn workspace_bytes(root: &Path) -> Result<u64, ()> {
    let mut pending = vec![root.to_path_buf()];
    let mut files = 0_usize;
    let mut bytes = 0_u64;
    while let Some(path) = pending.pop() {
        let entries = fs::read_dir(path).map_err(|_| ())?;
        for entry in entries {
            let entry = entry.map_err(|_| ())?;
            let file_type = entry.file_type().map_err(|_| ())?;
            if file_type.is_symlink() {
                continue;
            }
            if file_type.is_dir() {
                pending.push(entry.path());
            } else if file_type.is_file() {
                let metadata = entry.metadata().map_err(|_| ())?;
                files = files.saturating_add(1);
                if files > MAX_ACCOUNTED_FILES {
                    return Err(());
                }
                bytes = bytes.saturating_add(metadata.len());
            }
        }
    }
    Ok(bytes)
}

mod platform;
use platform::{configure_process, cpu_millis_snapshot, kill_process_tree, process_tree_usage};

fn effect_ops(value: &Value) -> Option<u64> {
    match value.get("kind").and_then(Value::as_str) {
        Some("genesis/run-v0.2") => value
            .get("data")
            .and_then(|data| data.get("entries"))
            .and_then(Value::as_u64),
        _ => None,
    }
}

fn semantic_resource(
    result: &Result<CommandResult, CliError>,
    max_effects: u64,
) -> Option<&'static str> {
    match result {
        Ok(CommandResult::Output(output))
            if effect_ops(&output.json).is_some_and(|observed| observed > max_effects) =>
        {
            Some("effects")
        }
        Ok(CommandResult::Error { envelope, .. })
            if envelope
                .pointer("/error/context/kind")
                .and_then(Value::as_str)
                == Some("step-limit") =>
        {
            Some("steps")
        }
        _ => None,
    }
}

fn command_result(status: ExitStatus, stdout: &[u8]) -> Result<CommandResult, CliError> {
    let value: Value = serde_json::from_slice(stdout).map_err(|_| {
        cli_err(
            EX_INTERNAL,
            "session/worker-output",
            "isolated worker did not emit one canonical JSON envelope",
        )
    })?;
    let exit_code = status
        .code()
        .and_then(|code| u8::try_from(code).ok())
        .unwrap_or(EX_INTERNAL);
    if value.get("kind").and_then(Value::as_str) == Some("genesis/error-v0.2") {
        return Ok(CommandResult::Error {
            exit_code,
            envelope: value,
        });
    }
    Ok(CommandResult::Output(CmdOut {
        exit_code,
        stdout: String::new(),
        json: value,
    }))
}

#[derive(Clone, Copy)]
struct AuditMeasurements {
    started: Instant,
    cpu_before: Option<u64>,
    output_bytes: u64,
    disk_before: u64,
    disk_after: u64,
    peak_heap_bytes: Option<u64>,
    peak_processes: Option<u64>,
    observed_cpu_ms: Option<u64>,
}

fn audit(
    job: &WorkerJob,
    measurements: AuditMeasurements,
    result: Option<&Result<CommandResult, CliError>>,
    termination: &'static str,
    exceeded: Option<&'static str>,
) -> SessionAudit {
    SessionAudit {
        worker_profile: "native-isolated-v0.1",
        limits_identity: job.limits.identity(),
        wall_ms: u64::try_from(measurements.started.elapsed().as_millis()).unwrap_or(u64::MAX),
        cpu_ms: measurements.observed_cpu_ms.or_else(|| {
            measurements
                .cpu_before
                .zip(cpu_millis_snapshot())
                .map(|(before, after)| after.saturating_sub(before))
        }),
        output_bytes: measurements.output_bytes,
        disk_delta_bytes: i64::try_from(measurements.disk_after).unwrap_or(i64::MAX)
            - i64::try_from(measurements.disk_before).unwrap_or(i64::MAX),
        effect_ops: result.and_then(|result| result.as_ref().ok()).and_then(
            |result| match result {
                CommandResult::Output(output) => effect_ops(&output.json),
                CommandResult::Error { envelope, .. } => effect_ops(envelope),
            },
        ),
        peak_heap_bytes: measurements.peak_heap_bytes,
        peak_processes: measurements.peak_processes,
        termination,
        exceeded,
    }
}

pub(super) fn run_isolated(job: WorkerJob, cancelled: Arc<AtomicBool>) -> WorkerResult {
    let request_id = job.request_id.clone();
    let started = Instant::now();
    let cpu_before = cpu_millis_snapshot();
    let disk_before = match workspace_bytes(&job.workspace_root) {
        Ok(bytes) => bytes,
        Err(()) => {
            return WorkerResult::WorkspaceError {
                request_id,
                message: "workspace disk accounting exceeded its bounded input profile".to_string(),
                audit: Some(SessionAudit::not_started(
                    &job.limits,
                    "workspace-accounting-failed",
                )),
            };
        }
    };
    let executable = match std::env::current_exe() {
        Ok(executable) => executable,
        Err(_) => {
            return WorkerResult::WorkspaceError {
                request_id,
                message: "isolated worker executable is unavailable".to_string(),
                audit: Some(SessionAudit::not_started(
                    &job.limits,
                    "worker-executable-unavailable",
                )),
            };
        }
    };
    let mut command = Command::new(executable);
    command.current_dir(&job.workspace_root);
    command.args(&job.inherited);
    command.args(&job.argv);
    command.stdin(Stdio::null());
    command.stdout(Stdio::piped());
    command.stderr(Stdio::piped());
    configure_process(&mut command, &job);
    let mut child = match command.spawn() {
        Ok(child) => child,
        Err(_) => {
            return WorkerResult::WorkspaceError {
                request_id,
                message: "isolated worker process could not be started".to_string(),
                audit: Some(SessionAudit::not_started(
                    &job.limits,
                    "worker-spawn-failed",
                )),
            };
        }
    };
    let process_id = child.id();
    let output_observed = Arc::new(AtomicU64::new(0));
    let output_exceeded = Arc::new(AtomicBool::new(false));
    let stdout = match child.stdout.take() {
        Some(stdout) => capture_pipe(
            stdout,
            job.limits.max_output_bytes,
            Arc::clone(&output_observed),
            Arc::clone(&output_exceeded),
        ),
        None => {
            let _ = kill_process_tree(process_id);
            let _ = child.wait();
            let disk_after = workspace_bytes(&job.workspace_root).unwrap_or(disk_before);
            return WorkerResult::WorkspaceError {
                request_id,
                message: "isolated worker stdout was unavailable".to_string(),
                audit: Some(audit(
                    &job,
                    AuditMeasurements {
                        started,
                        cpu_before,
                        output_bytes: 0,
                        disk_before,
                        disk_after,
                        peak_heap_bytes: None,
                        peak_processes: Some(1),
                        observed_cpu_ms: None,
                    },
                    None,
                    "worker-stdout-unavailable",
                    None,
                )),
            };
        }
    };
    let stderr = match child.stderr.take() {
        Some(stderr) => capture_pipe(
            stderr,
            job.limits.max_output_bytes,
            Arc::clone(&output_observed),
            Arc::clone(&output_exceeded),
        ),
        None => {
            let _ = kill_process_tree(process_id);
            let _ = child.wait();
            let disk_after = workspace_bytes(&job.workspace_root).unwrap_or(disk_before);
            return WorkerResult::WorkspaceError {
                request_id,
                message: "isolated worker stderr was unavailable".to_string(),
                audit: Some(audit(
                    &job,
                    AuditMeasurements {
                        started,
                        cpu_before,
                        output_bytes: 0,
                        disk_before,
                        disk_after,
                        peak_heap_bytes: None,
                        peak_processes: Some(1),
                        observed_cpu_ms: None,
                    },
                    None,
                    "worker-stderr-unavailable",
                    None,
                )),
            };
        }
    };

    let mut termination = "completed";
    let mut exceeded = None;
    let initial_usage = process_tree_usage(process_id);
    let mut peak_heap_bytes = initial_usage.map(|(_, heap_bytes, _)| heap_bytes);
    let mut peak_processes = Some(
        initial_usage
            .map(|(processes, _, _)| processes)
            .unwrap_or(1),
    );
    let mut observed_cpu_ms = initial_usage.map(|(_, _, cpu_ms)| cpu_ms);
    let mut next_disk_poll = Instant::now();
    let status = loop {
        if cancelled.load(Ordering::SeqCst) {
            termination = "cancelled-and-reaped";
            let _ = kill_process_tree(process_id);
            break child.wait();
        }
        if started.elapsed() >= job.limits.max_wall {
            termination = "resource-killed-and-reaped";
            exceeded = Some("wall");
            let _ = kill_process_tree(process_id);
            break child.wait();
        }
        if output_exceeded.load(Ordering::SeqCst) {
            termination = "resource-killed-and-reaped";
            exceeded = Some("output");
            let _ = kill_process_tree(process_id);
            break child.wait();
        }
        if let Some((processes, heap_bytes, cpu_ms)) = process_tree_usage(process_id) {
            peak_processes = Some(peak_processes.unwrap_or(0).max(processes));
            peak_heap_bytes = Some(peak_heap_bytes.unwrap_or(0).max(heap_bytes));
            observed_cpu_ms = Some(observed_cpu_ms.unwrap_or(0).max(cpu_ms));
            let resource = if processes > job.limits.max_processes {
                Some("processes")
            } else if heap_bytes > job.limits.max_heap_bytes {
                Some("heap")
            } else if cpu_ms > job.limits.max_cpu.as_millis() as u64 {
                Some("cpu")
            } else {
                None
            };
            if let Some(resource) = resource {
                termination = "resource-killed-and-reaped";
                exceeded = Some(resource);
                let _ = kill_process_tree(process_id);
                break child.wait();
            }
        }
        if Instant::now() >= next_disk_poll {
            next_disk_poll = Instant::now() + DISK_POLL_INTERVAL;
            if workspace_bytes(&job.workspace_root)
                .map(|bytes| bytes.saturating_sub(disk_before) > job.limits.max_disk_bytes)
                .unwrap_or(true)
            {
                termination = "resource-killed-and-reaped";
                exceeded = Some("disk");
                let _ = kill_process_tree(process_id);
                break child.wait();
            }
        }
        match child.try_wait() {
            Ok(Some(status)) => break Ok(status),
            Ok(None) => thread::sleep(POLL_INTERVAL),
            Err(error) => break Err(error),
        }
    };
    #[cfg(unix)]
    let file_size_signal = {
        use std::os::unix::process::ExitStatusExt;
        status
            .as_ref()
            .ok()
            .and_then(ExitStatusExt::signal)
            .is_some_and(|signal| signal == libc::SIGXFSZ)
    };
    #[cfg(not(unix))]
    let file_size_signal = false;
    let _ = kill_process_tree(process_id);
    let stdout = stdout.join().unwrap_or(CapturedPipe {
        bytes: Vec::new(),
        observed: 0,
        exceeded: true,
    });
    let stderr = stderr.join().unwrap_or(CapturedPipe {
        bytes: Vec::new(),
        observed: 0,
        exceeded: true,
    });
    let output_bytes = stdout.observed.saturating_add(stderr.observed);
    let disk_after = workspace_bytes(&job.workspace_root).unwrap_or(u64::MAX);
    let measurements = AuditMeasurements {
        started,
        cpu_before,
        output_bytes,
        disk_before,
        disk_after,
        peak_heap_bytes,
        peak_processes,
        observed_cpu_ms,
    };

    if termination == "cancelled-and-reaped" {
        let audit = audit(&job, measurements, None, termination, None);
        return WorkerResult::Cancelled { request_id, audit };
    }
    if exceeded.is_none() && (stdout.exceeded || stderr.exceeded) {
        exceeded = Some("output");
        termination = "resource-rejected";
    }
    if exceeded.is_none()
        && (file_size_signal || disk_after.saturating_sub(disk_before) > job.limits.max_disk_bytes)
    {
        exceeded = Some("disk");
        termination = "resource-rejected";
    }
    let result = status
        .map_err(|_| {
            cli_err(
                EX_INTERNAL,
                "session/worker-wait",
                "isolated worker could not be reaped",
            )
        })
        .and_then(|status| command_result(status, &stdout.bytes));
    if exceeded.is_none()
        && let Some(resource) = semantic_resource(&result, job.limits.max_effects)
    {
        exceeded = Some(resource);
        termination = "resource-rejected";
    }
    let audit = audit(&job, measurements, Some(&result), termination, exceeded);
    if let Some(resource) = exceeded {
        let command_envelope = match &result {
            Ok(CommandResult::Output(output)) => Some(output.json.clone()),
            Ok(CommandResult::Error { envelope, .. }) => Some(envelope.clone()),
            Err(_) => None,
        };
        return WorkerResult::ResourceExceeded {
            request_id,
            resource,
            command_envelope,
            audit,
        };
    }
    match result {
        Ok(CommandResult::Output(output)) => WorkerResult::Completed {
            request_id,
            result: Ok(output),
            audit,
        },
        Ok(CommandResult::Error {
            exit_code,
            envelope,
        }) => WorkerResult::CommandError {
            request_id,
            exit_code,
            envelope,
            audit,
        },
        Err(error) => WorkerResult::Completed {
            request_id,
            result: Err(error),
            audit,
        },
    }
}
