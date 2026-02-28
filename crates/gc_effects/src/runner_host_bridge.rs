use gc_coreform::{Term, TermOrdKey, parse_term, print_term};
#[cfg(not(target_os = "wasi"))]
use std::collections::HashMap;
#[cfg(not(target_os = "wasi"))]
use std::io::{BufRead as _, BufReader, Read as _, Write as _};
#[cfg(not(target_os = "wasi"))]
use std::process::{Child, ChildStdin, ChildStdout, Command, Stdio};
#[cfg(not(target_os = "wasi"))]
use std::sync::{Arc, Mutex, OnceLock};

use crate::policy::OpPolicy;
use crate::runner_io_ops::{effective_base_dir, sandbox_path_read};
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
    let transport = runner_host_bridge_policy::bridge_transport(pol, family)?;
    if runner_host_bridge_policy::wasi_bridge_profile_enabled(pol) {
        return runner_host_bridge_wasi::run_wasi_bridge_profile(
            family, op, payload, pol, max_bytes,
        );
    }

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
    if timeout_ms.is_some()
        && matches!(
            transport,
            runner_host_bridge_policy::BridgeTransport::PersistentStdio
        )
    {
        return Err(BridgeError {
            code: format!("{family}/bridge-policy"),
            message:
                "timeout_ms is not supported for bridge_transport `persistent-stdio`; use `spawn-per-op` for hard timeout enforcement".to_string(),
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
    if let Some(mut stdin) = child.stdin.take() {
        stdin
            .write_all(payload_frame.as_bytes())
            .map_err(|e| BridgeError {
                code: format!("{family}/bridge-stdin-write"),
                message: e.to_string(),
            })?;
    }
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
    Ok(out)
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
    use std::io::Write as _;
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
    let mut child = cmd.spawn().map_err(|e| BridgeError {
        code: format!("{family}/bridge-spawn"),
        message: e.to_string(),
    })?;
    if let Some(mut stdin) = child.stdin.take() {
        stdin
            .write_all(payload_frame.as_bytes())
            .map_err(|e| BridgeError {
                code: format!("{family}/bridge-stdin-write"),
                message: e.to_string(),
            })?;
    }
    let deadline = Instant::now() + Duration::from_millis(timeout_ms);
    loop {
        match child.try_wait() {
            Ok(Some(_status)) => break,
            Ok(None) => {
                if Instant::now() >= deadline {
                    let _ = child.kill();
                    let _ = child.wait();
                    return Err(BridgeError {
                        code: format!("{family}/bridge-timeout"),
                        message: format!("bridge command timed out after {timeout_ms}ms"),
                    });
                }
                std::thread::sleep(Duration::from_millis(2));
            }
            Err(e) => {
                return Err(BridgeError {
                    code: format!("{family}/bridge-exec"),
                    message: e.to_string(),
                });
            }
        }
    }
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
