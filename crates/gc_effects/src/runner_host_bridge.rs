use crate::policy::OpPolicy;
use crate::runner_io_ops::{effective_base_dir, sandbox_path_read};
#[cfg(not(target_os = "wasi"))]
use crate::runner_process_control::{
    configure_killable_process, hard_process_tree_termination_supported, terminate_and_reap,
    terminate_descendants,
};
use gc_coreform::{Term, TermOrdKey, parse_term, print_term};
#[cfg(not(target_os = "wasi"))]
use std::io::{BufRead as _, BufReader, Read as _, Write as _};
#[cfg(not(target_os = "wasi"))]
use std::process::{ChildStdout, Command, Stdio};

#[cfg(all(test, not(target_os = "wasi")))]
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum SpawnBridgeCleanupFault {
    Reap,
    ResidualVerification,
    WriterJoin,
}

#[cfg(all(test, not(target_os = "wasi")))]
std::thread_local! {
    static SPAWN_BRIDGE_CLEANUP_FAULT: std::cell::Cell<Option<SpawnBridgeCleanupFault>> =
        const { std::cell::Cell::new(None) };
}

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

#[cfg(not(target_os = "wasi"))]
type BridgeWriterPump = std::thread::JoinHandle<std::io::Result<()>>;

#[cfg(not(target_os = "wasi"))]
type BridgeReaderPump = std::thread::JoinHandle<std::io::Result<Vec<u8>>>;

#[cfg(not(target_os = "wasi"))]
#[derive(Default)]
struct SpawnBridgePumps {
    writer: Option<BridgeWriterPump>,
    stdout: Option<BridgeReaderPump>,
    stderr: Option<BridgeReaderPump>,
}

#[cfg(not(target_os = "wasi"))]
#[derive(Default)]
struct SpawnBridgePumpResults {
    write: Option<std::io::Result<()>>,
    stdout: Option<std::io::Result<Vec<u8>>>,
    stderr: Option<std::io::Result<Vec<u8>>>,
    join_failures: Vec<&'static str>,
}

#[cfg(not(target_os = "wasi"))]
impl SpawnBridgePumps {
    fn join_all(mut self) -> SpawnBridgePumpResults {
        let mut results = SpawnBridgePumpResults::default();
        if let Some(writer) = self.writer.take() {
            match writer.join() {
                Ok(result) => results.write = Some(result),
                Err(_) => results.join_failures.push("stdin pump join failed"),
            }
        }
        if let Some(stdout) = self.stdout.take() {
            match stdout.join() {
                Ok(result) => results.stdout = Some(result),
                Err(_) => results.join_failures.push("stdout pump join failed"),
            }
        }
        if let Some(stderr) = self.stderr.take() {
            match stderr.join() {
                Ok(result) => results.stderr = Some(result),
                Err(_) => results.join_failures.push("stderr pump join failed"),
            }
        }
        #[cfg(test)]
        if SPAWN_BRIDGE_CLEANUP_FAULT
            .get()
            .is_some_and(|fault| fault == SpawnBridgeCleanupFault::WriterJoin)
        {
            results
                .join_failures
                .push("injected stdin pump join failure");
        }
        results
    }
}

#[cfg(not(target_os = "wasi"))]
fn spawn_bridge_reap_error(
    family: &str,
    context: &str,
    failures: &[String],
    prior: Option<&BridgeError>,
) -> BridgeError {
    let details = failures.join("; ");
    let prior = prior
        .map(|error| format!("; prior error: {}: {}", error.code, error.message))
        .unwrap_or_default();
    BridgeError {
        code: format!("{family}/bridge-reap"),
        message: format!("{context}: {details}{prior}"),
    }
}

#[cfg(not(target_os = "wasi"))]
fn terminate_spawn_bridge(
    child: &mut std::process::Child,
) -> std::io::Result<std::process::ExitStatus> {
    let result = terminate_and_reap(child);
    #[cfg(test)]
    if result.is_ok()
        && SPAWN_BRIDGE_CLEANUP_FAULT
            .get()
            .is_some_and(|fault| fault == SpawnBridgeCleanupFault::Reap)
    {
        return Err(std::io::Error::other("injected spawn bridge reap failure"));
    }
    result
}

#[cfg(not(target_os = "wasi"))]
fn terminate_spawn_bridge_descendants(process_id: u32) -> std::io::Result<()> {
    let result = terminate_descendants(process_id);
    #[cfg(test)]
    if result.is_ok()
        && SPAWN_BRIDGE_CLEANUP_FAULT
            .get()
            .is_some_and(|fault| fault == SpawnBridgeCleanupFault::ResidualVerification)
    {
        return Err(std::io::Error::other(
            "injected spawn bridge residual verification failure",
        ));
    }
    result
}

#[cfg(all(test, not(target_os = "wasi")))]
fn with_spawn_bridge_cleanup_fault_for_tests<T>(
    fault: SpawnBridgeCleanupFault,
    operation: impl FnOnce() -> T,
) -> T {
    struct ResetFault(Option<SpawnBridgeCleanupFault>);

    impl Drop for ResetFault {
        fn drop(&mut self) {
            SPAWN_BRIDGE_CLEANUP_FAULT.set(self.0);
        }
    }

    let previous = SPAWN_BRIDGE_CLEANUP_FAULT.replace(Some(fault));
    let _reset = ResetFault(previous);
    operation()
}

#[cfg(not(target_os = "wasi"))]
fn cleanup_failed_spawn_bridge(
    family: &str,
    child: &mut std::process::Child,
    pumps: SpawnBridgePumps,
    prior: BridgeError,
) -> BridgeError {
    let mut failures = Vec::new();
    if let Err(error) = terminate_spawn_bridge(child) {
        failures.push(format!("process termination/reap failed: {error}"));
    }
    let joined = pumps.join_all();
    failures.extend(joined.join_failures.into_iter().map(str::to_string));
    if failures.is_empty() {
        prior
    } else {
        spawn_bridge_reap_error(
            family,
            "spawn-per-operation bridge cleanup failed",
            &failures,
            Some(&prior),
        )
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

#[derive(Default)]
pub(crate) struct HostBridgeRuntime {
    #[cfg(not(target_os = "wasi"))]
    persistent: runner_host_bridge_persistent::PersistentBridgeRuntime,
}

impl HostBridgeRuntime {
    pub(crate) fn shutdown(&mut self) -> Result<(), BridgeError> {
        #[cfg(not(target_os = "wasi"))]
        {
            self.persistent.shutdown()
        }
        #[cfg(target_os = "wasi")]
        {
            Ok(())
        }
    }
}

pub(crate) fn call_host_bridge(
    runtime: &mut HostBridgeRuntime,
    family: &str,
    op: &str,
    payload: &Term,
    pol: Option<&OpPolicy>,
) -> Result<Term, BridgeError> {
    #[cfg(target_os = "wasi")]
    let _ = runtime;

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
                    &mut runtime.persistent,
                    family,
                    op,
                    payload,
                    &base_dir,
                    &cmd_path,
                    &args,
                    timeout_ms,
                    max_bytes,
                )
            }
        }
    }
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
    let output = run_bridge_process_once(
        family,
        op,
        &payload_frame,
        base_dir,
        cmd_path,
        args,
        timeout_ms,
    )?;
    decode_bridge_stdout(family, &output.stdout, max_bytes)
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
fn run_bridge_process_once(
    family: &str,
    op: &str,
    payload_frame: &str,
    base_dir: &std::path::Path,
    cmd_path: &std::path::Path,
    args: &[String],
    timeout_ms: Option<u64>,
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
    let mut pumps = SpawnBridgePumps::default();
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
            let prior = BridgeError {
                code: format!("{family}/bridge-thread"),
                message: error.to_string(),
            };
            return Err(cleanup_failed_spawn_bridge(
                family, &mut child, pumps, prior,
            ));
        }
    };
    pumps.writer = Some(writer);
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
            let prior = BridgeError {
                code: format!("{family}/bridge-thread"),
                message: error.to_string(),
            };
            return Err(cleanup_failed_spawn_bridge(
                family, &mut child, pumps, prior,
            ));
        }
    };
    pumps.stdout = Some(reader);
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
            let prior = BridgeError {
                code: format!("{family}/bridge-thread"),
                message: error.to_string(),
            };
            return Err(cleanup_failed_spawn_bridge(
                family, &mut child, pumps, prior,
            ));
        }
    };
    pumps.stderr = Some(error_reader);
    let deadline = timeout_ms.and_then(|ms| Instant::now().checked_add(Duration::from_millis(ms)));
    let status = loop {
        match child.try_wait() {
            Ok(Some(status)) => break status,
            Ok(None) => {
                if deadline.is_some_and(|deadline| Instant::now() >= deadline) {
                    let timeout_ms = timeout_ms.unwrap_or_default();
                    let prior = BridgeError {
                        code: format!("{family}/bridge-timeout"),
                        message: format!("bridge command timed out after {timeout_ms}ms"),
                    };
                    return Err(cleanup_failed_spawn_bridge(
                        family, &mut child, pumps, prior,
                    ));
                }
                std::thread::sleep(Duration::from_millis(2));
            }
            Err(e) => {
                let prior = BridgeError {
                    code: format!("{family}/bridge-exec"),
                    message: e.to_string(),
                };
                return Err(cleanup_failed_spawn_bridge(
                    family, &mut child, pumps, prior,
                ));
            }
        }
    };
    let residual_result = terminate_spawn_bridge_descendants(process_id);
    let joined = pumps.join_all();
    let mut cleanup_failures = Vec::new();
    if let Err(error) = residual_result {
        cleanup_failures.push(format!(
            "residual process-group verification failed: {error}"
        ));
    }
    cleanup_failures.extend(joined.join_failures.into_iter().map(str::to_string));
    if !cleanup_failures.is_empty() {
        return Err(spawn_bridge_reap_error(
            family,
            "spawn-per-operation bridge finalization failed",
            &cleanup_failures,
            None,
        ));
    }
    let write_result = joined.write.ok_or_else(|| BridgeError {
        code: format!("{family}/bridge-thread"),
        message: "bridge stdin pump result missing after join".to_string(),
    })?;
    let stdout = joined
        .stdout
        .ok_or_else(|| BridgeError {
            code: format!("{family}/bridge-thread"),
            message: "bridge stdout pump result missing after join".to_string(),
        })?
        .map_err(|error| BridgeError {
            code: format!("{family}/bridge-stdout-read"),
            message: error.to_string(),
        })?;
    let stderr = joined
        .stderr
        .ok_or_else(|| BridgeError {
            code: format!("{family}/bridge-thread"),
            message: "bridge stderr pump result missing after join".to_string(),
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
