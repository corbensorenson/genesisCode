use gc_coreform::{Term, TermOrdKey, parse_term, print_term};
#[cfg(not(target_os = "wasi"))]
use std::collections::HashMap;
#[cfg(not(target_os = "wasi"))]
use std::io::{BufRead as _, BufReader, Read as _, Write as _};
#[cfg(not(target_os = "wasi"))]
use std::process::{ChildStdout, Command, Stdio};
#[cfg(not(target_os = "wasi"))]
use std::sync::{Arc, Mutex, OnceLock};

use crate::policy::OpPolicy;
use crate::runner_io_ops::{effective_base_dir, sandbox_path_read};
#[cfg(not(target_os = "wasi"))]
use crate::runner_process_control::{
    configure_killable_process, hard_process_tree_termination_supported, terminate_and_reap,
    terminate_descendants,
};

#[cfg(not(target_os = "wasi"))]
static ACTIVE_BRIDGE_IO_PUMPS: std::sync::atomic::AtomicUsize =
    std::sync::atomic::AtomicUsize::new(0);

#[cfg(not(target_os = "wasi"))]
struct ActiveBridgeIoPump;

#[cfg(not(target_os = "wasi"))]
impl ActiveBridgeIoPump {
    fn enter() -> Self {
        ACTIVE_BRIDGE_IO_PUMPS.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
        Self
    }
}

#[cfg(not(target_os = "wasi"))]
impl Drop for ActiveBridgeIoPump {
    fn drop(&mut self) {
        ACTIVE_BRIDGE_IO_PUMPS.fetch_sub(1, std::sync::atomic::Ordering::SeqCst);
    }
}

#[cfg(all(test, not(target_os = "wasi")))]
fn active_bridge_io_pumps_for_tests() -> usize {
    ACTIVE_BRIDGE_IO_PUMPS.load(std::sync::atomic::Ordering::SeqCst)
}
#[path = "runner_host_bridge_persistent.rs"]
mod runner_host_bridge_persistent;
#[path = "runner_host_bridge_policy.rs"]
mod runner_host_bridge_policy;
#[path = "runner_host_bridge_wasi.rs"]
mod runner_host_bridge_wasi;

#[derive(Debug, Clone)]
pub(crate) struct BridgeError {
    pub code: String,
    pub message: String,
}

pub(crate) fn call_host_bridge(
    family: &str,
    op: &str,
    payload: &Term,
    pol: Option<&OpPolicy>,
) -> Result<Term, BridgeError> {
    let max_bytes = runner_host_bridge_policy::bridge_max_bytes(pol, family)?;
    if runner_host_bridge_policy::wasi_bridge_profile_enabled(pol) {
        return runner_host_bridge_wasi::run_wasi_bridge_profile(
            family, op, payload, pol, max_bytes,
        );
    }

    #[cfg(target_os = "wasi")]
    return Err(BridgeError {
        code: format!("{family}/bridge-profile-required"),
        message: format!(
            "{op} requires the deny-by-default WASI bridge profile; process bridges are unavailable"
        ),
    });

    #[cfg(not(target_os = "wasi"))]
    {
        let transport = runner_host_bridge_policy::bridge_transport(pol, family)?;
        let Some(cmd_raw) = runner_host_bridge_policy::bridge_cmd(pol) else {
            return Err(BridgeError {
                code: format!("{family}/bridge-required"),
                message: format!("{op} requires `{}` in caps.toml op policy", "bridge_cmd"),
            });
        };
        let base_dir = effective_base_dir(pol).map_err(|e| BridgeError {
            code: format!("{family}/bridge-path"),
            message: e.to_string(),
        })?;
        let cmd_path = sandbox_path_read(&base_dir, &cmd_raw).map_err(|e| BridgeError {
            code: format!("{family}/bridge-path"),
            message: e.to_string(),
        })?;
        runner_host_bridge_policy::enforce_bridge_identity(family, &cmd_raw, &cmd_path, pol)?;
        let args = runner_host_bridge_policy::bridge_args(pol);
        let timeout_ms = pol.and_then(|p| p.timeout_ms).filter(|ms| *ms > 0);
        #[cfg(not(target_os = "wasi"))]
        if timeout_ms.is_some() && !hard_process_tree_termination_supported() {
            return Err(BridgeError {
                code: format!("{family}/bridge-policy"),
                message: "timeout_ms requires platform process-tree termination support"
                    .to_string(),
            });
        }
        match transport {
            runner_host_bridge_policy::BridgeTransport::SpawnPerOp => run_bridge_process(
                family, op, payload, &base_dir, &cmd_path, &args, timeout_ms, max_bytes,
            ),
            runner_host_bridge_policy::BridgeTransport::PersistentStdio => {
                runner_host_bridge_persistent::run_bridge_process_persistent(
                    family, op, payload, &base_dir, &cmd_path, &args, timeout_ms, max_bytes,
                )
            }
        }
    }
}

#[cfg(all(test, not(target_os = "wasi")))]
fn reset_persistent_bridge_sessions_for_tests() {
    runner_host_bridge_persistent::reset_persistent_bridge_sessions_for_tests();
}

#[cfg(not(target_os = "wasi"))]
#[expect(
    clippy::too_many_arguments,
    reason = "bridge process runner requires explicit io/time/resource limits for deterministic envelopes"
)]
fn run_bridge_process(
    family: &str,
    op: &str,
    payload: &Term,
    base_dir: &std::path::Path,
    cmd_path: &std::path::Path,
    args: &[String],
    timeout_ms: Option<u64>,
    max_bytes: Option<usize>,
) -> Result<Term, BridgeError> {
    let payload_src = print_term(payload);
    runner_host_bridge_policy::enforce_payload_limit(family, payload, max_bytes)?;
    let payload_frame = format!("{}\n{}", payload_src.len(), payload_src);
    let output = if let Some(ms) = timeout_ms {
        run_bridge_process_once_with_timeout(
            family,
            op,
            &payload_frame,
            base_dir,
            cmd_path,
            args,
            ms,
        )?
    } else {
        run_bridge_process_once(family, op, &payload_frame, base_dir, cmd_path, args)?
    };
    decode_bridge_stdout(family, &output.stdout, max_bytes)
}

#[cfg(not(target_os = "wasi"))]
fn run_bridge_process_once(
    family: &str,
    op: &str,
    payload_frame: &str,
    base_dir: &std::path::Path,
    cmd_path: &std::path::Path,
    args: &[String],
) -> Result<std::process::Output, BridgeError> {
    use std::io::Write as _;
    use std::process::{Command, Stdio};

    let mut cmd = Command::new(cmd_path);
    cmd.current_dir(base_dir);
    for arg in args {
        cmd.arg(arg);
    }
    cmd.arg(op);
    cmd.env("GENESIS_HOST_BRIDGE_OP", op);
    cmd.env("GENESIS_HOST_BRIDGE_FAMILY", family);
    cmd.stdin(Stdio::piped());
    cmd.stdout(Stdio::piped());
    cmd.stderr(Stdio::piped());
    let mut child = cmd.spawn().map_err(|e| BridgeError {
        code: format!("{family}/bridge-spawn"),
        message: e.to_string(),
    })?;
    let write_result = child
        .stdin
        .take()
        .map(|mut stdin| stdin.write_all(payload_frame.as_bytes()))
        .unwrap_or(Ok(()));
    let out = child.wait_with_output().map_err(|e| BridgeError {
        code: format!("{family}/bridge-exec"),
        message: e.to_string(),
    })?;
    if !out.status.success() {
        let stderr = String::from_utf8_lossy(&out.stderr).trim().to_string();
        let msg = if stderr.is_empty() {
            format!("bridge command exited with status {}", out.status)
        } else {
            format!("bridge command exited with status {}: {stderr}", out.status)
        };
        return Err(BridgeError {
            code: format!("{family}/bridge-exit"),
            message: msg,
        });
    }
    validate_bridge_stdin_write(family, write_result)?;
    Ok(out)
}

#[cfg(not(target_os = "wasi"))]
fn validate_bridge_stdin_write(
    family: &str,
    result: std::io::Result<()>,
) -> Result<(), BridgeError> {
    match result {
        Ok(()) => Ok(()),
        // A bridge may not need its payload and can close stdin after producing a
        // successful response. The child status is validated before this helper.
        Err(error) if error.kind() == std::io::ErrorKind::BrokenPipe => Ok(()),
        Err(error) => Err(BridgeError {
            code: format!("{family}/bridge-stdin-write"),
            message: error.to_string(),
        }),
    }
}

#[cfg(not(target_os = "wasi"))]
fn run_bridge_process_once_with_timeout(
    family: &str,
    op: &str,
    payload_frame: &str,
    base_dir: &std::path::Path,
    cmd_path: &std::path::Path,
    args: &[String],
    timeout_ms: u64,
) -> Result<std::process::Output, BridgeError> {
    use std::io::{Read as _, Write as _};
    use std::process::{Command, Stdio};
    use std::time::{Duration, Instant};

    let mut cmd = Command::new(cmd_path);
    cmd.current_dir(base_dir);
    for arg in args {
        cmd.arg(arg);
    }
    cmd.arg(op);
    cmd.env("GENESIS_HOST_BRIDGE_OP", op);
    cmd.env("GENESIS_HOST_BRIDGE_FAMILY", family);
    cmd.stdin(Stdio::piped());
    cmd.stdout(Stdio::piped());
    cmd.stderr(Stdio::piped());
    configure_killable_process(&mut cmd);
    let mut child = cmd.spawn().map_err(|e| BridgeError {
        code: format!("{family}/bridge-spawn"),
        message: e.to_string(),
    })?;
    let process_id = child.id();
    let stdin = child.stdin.take();
    let stdout = child.stdout.take();
    let stderr = child.stderr.take();
    let payload = payload_frame.as_bytes().to_vec();
    let writer = std::thread::Builder::new()
        .name("gc-bridge-stdin".to_string())
        .spawn(move || {
            let _active = ActiveBridgeIoPump::enter();
            let Some(mut stdin) = stdin else {
                return Ok(());
            };
            stdin.write_all(&payload)
        });
    let writer = match writer {
        Ok(writer) => writer,
        Err(error) => {
            let _ = terminate_and_reap(&mut child);
            return Err(BridgeError {
                code: format!("{family}/bridge-thread"),
                message: error.to_string(),
            });
        }
    };
    let reader = std::thread::Builder::new()
        .name("gc-bridge-stdout".to_string())
        .spawn(move || {
            let _active = ActiveBridgeIoPump::enter();
            let mut bytes = Vec::new();
            if let Some(mut stdout) = stdout {
                stdout.read_to_end(&mut bytes)?;
            }
            Ok::<_, std::io::Error>(bytes)
        });
    let reader = match reader {
        Ok(reader) => reader,
        Err(error) => {
            let _ = terminate_and_reap(&mut child);
            let _ = writer.join();
            return Err(BridgeError {
                code: format!("{family}/bridge-thread"),
                message: error.to_string(),
            });
        }
    };
    let error_reader = std::thread::Builder::new()
        .name("gc-bridge-stderr".to_string())
        .spawn(move || {
            let _active = ActiveBridgeIoPump::enter();
            let mut bytes = Vec::new();
            if let Some(mut stderr) = stderr {
                stderr.read_to_end(&mut bytes)?;
            }
            Ok::<_, std::io::Error>(bytes)
        });
    let error_reader = match error_reader {
        Ok(error_reader) => error_reader,
        Err(error) => {
            let _ = terminate_and_reap(&mut child);
            let _ = writer.join();
            let _ = reader.join();
            return Err(BridgeError {
                code: format!("{family}/bridge-thread"),
                message: error.to_string(),
            });
        }
    };
    let deadline = Instant::now()
        .checked_add(Duration::from_millis(timeout_ms))
        .unwrap_or_else(Instant::now);
    let status = loop {
        match child.try_wait() {
            Ok(Some(status)) => break status,
            Ok(None) => {
                if Instant::now() >= deadline {
                    let termination = terminate_and_reap(&mut child);
                    let _ = writer.join();
                    let _ = reader.join();
                    let _ = error_reader.join();
                    if let Err(error) = termination {
                        return Err(BridgeError {
                            code: format!("{family}/bridge-reap"),
                            message: format!(
                                "bridge timeout failed to terminate and reap process tree: {error}"
                            ),
                        });
                    }
                    return Err(BridgeError {
                        code: format!("{family}/bridge-timeout"),
                        message: format!("bridge command timed out after {timeout_ms}ms"),
                    });
                }
                std::thread::sleep(Duration::from_millis(2));
            }
            Err(e) => {
                let _ = terminate_and_reap(&mut child);
                let _ = writer.join();
                let _ = reader.join();
                let _ = error_reader.join();
                return Err(BridgeError {
                    code: format!("{family}/bridge-exec"),
                    message: e.to_string(),
                });
            }
        }
    };
    terminate_descendants(process_id).map_err(|error| BridgeError {
        code: format!("{family}/bridge-reap"),
        message: format!("failed to terminate residual bridge descendants: {error}"),
    })?;
    let write_result = writer.join().map_err(|_| BridgeError {
        code: format!("{family}/bridge-thread"),
        message: "bridge stdin pump panicked".to_string(),
    })?;
    let stdout = reader
        .join()
        .map_err(|_| BridgeError {
            code: format!("{family}/bridge-thread"),
            message: "bridge stdout pump panicked".to_string(),
        })?
        .map_err(|error| BridgeError {
            code: format!("{family}/bridge-stdout-read"),
            message: error.to_string(),
        })?;
    let stderr = error_reader
        .join()
        .map_err(|_| BridgeError {
            code: format!("{family}/bridge-thread"),
            message: "bridge stderr pump panicked".to_string(),
        })?
        .map_err(|error| BridgeError {
            code: format!("{family}/bridge-stderr-read"),
            message: error.to_string(),
        })?;
    let out = std::process::Output {
        status,
        stdout,
        stderr,
    };
    if !out.status.success() {
        let stderr = String::from_utf8_lossy(&out.stderr).trim().to_string();
        let msg = if stderr.is_empty() {
            format!("bridge command exited with status {}", out.status)
        } else {
            format!("bridge command exited with status {}: {stderr}", out.status)
        };
        return Err(BridgeError {
            code: format!("{family}/bridge-exit"),
            message: msg,
        });
    }
    validate_bridge_stdin_write(family, write_result)?;
    Ok(out)
}

fn decode_bridge_stdout(
    family: &str,
    stdout: &[u8],
    max_bytes: Option<usize>,
) -> Result<Term, BridgeError> {
    if stdout.is_empty() {
        return Ok(Term::Map(
            [((TermOrdKey(Term::symbol(":ok"))), Term::Bool(true))]
                .into_iter()
                .collect(),
        ));
    }

    let stdout_s = String::from_utf8(stdout.to_vec()).map_err(|e| BridgeError {
        code: format!("{family}/bridge-stdout-utf8"),
        message: e.to_string(),
    })?;
    if let Some((header, body)) = stdout_s.split_once('\n')
        && let Ok(body_len) = header.trim().parse::<usize>()
        && body_len == body.len()
    {
        if let Some(limit) = max_bytes
            && body_len > limit
        {
            return Err(BridgeError {
                code: format!("{family}/bridge-response-too-large"),
                message: format!("bridge response exceeds max_bytes ({body_len} > {limit})"),
            });
        }
        return parse_term(body).map_err(|e| BridgeError {
            code: format!("{family}/bridge-parse"),
            message: e.to_string(),
        });
    }

    let trimmed = stdout_s.trim();
    if let Some(limit) = max_bytes
        && trimmed.len() > limit
    {
        return Err(BridgeError {
            code: format!("{family}/bridge-response-too-large"),
            message: format!(
                "bridge response exceeds max_bytes ({} > {limit})",
                trimmed.len()
            ),
        });
    }
    parse_term(trimmed).map_err(|e| BridgeError {
        code: format!("{family}/bridge-parse"),
        message: e.to_string(),
    })
}

#[cfg(test)]
#[path = "runner_host_bridge_tests.rs"]
mod tests;
