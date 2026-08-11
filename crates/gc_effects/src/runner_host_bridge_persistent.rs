#[cfg(not(target_os = "wasi"))]
use super::*;

#[cfg(not(target_os = "wasi"))]
use crate::runner_process_control::{
    configure_killable_process, signal_process_tree, terminate_and_reap, terminate_descendants,
};

#[cfg(not(target_os = "wasi"))]
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash)]
struct PersistentBridgeSessionKey {
    family: String,
    op: String,
    base_dir: std::path::PathBuf,
    cmd_path: std::path::PathBuf,
    args: Vec<String>,
}

#[cfg(not(target_os = "wasi"))]
struct PersistentBridgeRequest {
    payload_frame: String,
    max_bytes: Option<usize>,
    response: std::sync::mpsc::Sender<Result<Term, BridgeError>>,
}

#[cfg(not(target_os = "wasi"))]
struct PersistentBridgeSession {
    process_id: u32,
    requests: Option<std::sync::mpsc::SyncSender<PersistentBridgeRequest>>,
    worker: Option<std::thread::JoinHandle<std::io::Result<()>>>,
    worker_done: std::sync::mpsc::Receiver<()>,
}

#[cfg(not(target_os = "wasi"))]
#[derive(Default)]
pub(super) struct PersistentBridgeRuntime {
    sessions: std::collections::HashMap<PersistentBridgeSessionKey, PersistentBridgeSession>,
}

#[cfg(not(target_os = "wasi"))]
impl PersistentBridgeRuntime {
    fn clear(&mut self) -> Result<(), BridgeError> {
        let mut sessions = self.sessions.drain().collect::<Vec<_>>();
        sessions.sort_by(|(left, _), (right, _)| left.cmp(right));
        let mut failures = Vec::new();
        for (key, mut session) in sessions {
            if let Err(error) = session.stop() {
                failures.push((key.family, error));
            }
        }
        let Some((family, first)) = failures.first() else {
            return Ok(());
        };
        let details = failures
            .iter()
            .map(|(failed_family, error)| format!("{failed_family}: {error}"))
            .collect::<Vec<_>>()
            .join("; ");
        Err(BridgeError {
            code: format!("{family}/bridge-reap"),
            message: format!(
                "failed to stop {} persistent bridge session(s): {details}; first failure: {first}",
                failures.len()
            ),
        })
    }

    pub(super) fn shutdown(&mut self) -> Result<(), BridgeError> {
        self.clear()
    }
}

#[cfg(not(target_os = "wasi"))]
fn cleanup_after_error(
    runtime: &mut PersistentBridgeRuntime,
    key: &PersistentBridgeSessionKey,
    primary: BridgeError,
) -> BridgeError {
    match clear_persistent_bridge_session(runtime, key) {
        Ok(()) => primary,
        Err(mut cleanup) => {
            cleanup.message = format!(
                "{}; cleanup followed {}: {}",
                cleanup.message, primary.code, primary.message
            );
            cleanup
        }
    }
}

#[cfg(not(target_os = "wasi"))]
impl Drop for PersistentBridgeRuntime {
    fn drop(&mut self) {
        let _ = self.clear();
    }
}

#[cfg(not(target_os = "wasi"))]
static ACTIVE_PERSISTENT_BRIDGE_WORKERS: std::sync::atomic::AtomicUsize =
    std::sync::atomic::AtomicUsize::new(0);
#[cfg(not(target_os = "wasi"))]
static JOINED_PERSISTENT_BRIDGE_WORKERS: std::sync::atomic::AtomicUsize =
    std::sync::atomic::AtomicUsize::new(0);

#[cfg(not(target_os = "wasi"))]
struct ActivePersistentBridgeWorker;

#[cfg(not(target_os = "wasi"))]
impl ActivePersistentBridgeWorker {
    fn enter() -> Self {
        ACTIVE_PERSISTENT_BRIDGE_WORKERS.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
        Self
    }
}

#[cfg(not(target_os = "wasi"))]
impl Drop for ActivePersistentBridgeWorker {
    fn drop(&mut self) {
        ACTIVE_PERSISTENT_BRIDGE_WORKERS.fetch_sub(1, std::sync::atomic::Ordering::SeqCst);
    }
}

#[cfg(all(test, not(target_os = "wasi")))]
pub(super) fn joined_persistent_bridge_workers_for_tests() -> usize {
    JOINED_PERSISTENT_BRIDGE_WORKERS.load(std::sync::atomic::Ordering::SeqCst)
}

#[cfg(not(target_os = "wasi"))]
impl PersistentBridgeSession {
    fn stop(&mut self) -> std::io::Result<()> {
        const WORKER_JOIN_TIMEOUT: std::time::Duration = std::time::Duration::from_millis(250);

        // The worker exclusively owns Child. Stop admission, drive the process
        // group to execution quiescence, then join only after bounded completion.
        let signal_result = signal_process_tree(self.process_id);
        self.requests.take();
        let termination_result = terminate_descendants(self.process_id);
        let completion_result = if self.worker.is_some() {
            match self.worker_done.recv_timeout(WORKER_JOIN_TIMEOUT) {
                Ok(()) | Err(std::sync::mpsc::RecvTimeoutError::Disconnected) => Ok(()),
                Err(std::sync::mpsc::RecvTimeoutError::Timeout) => Err(std::io::Error::new(
                    std::io::ErrorKind::TimedOut,
                    format!(
                        "persistent bridge worker did not quiesce within {}ms; initial signal: {}; process tree: {}",
                        WORKER_JOIN_TIMEOUT.as_millis(),
                        io_result_summary(&signal_result),
                        io_result_summary(&termination_result),
                    ),
                )),
            }
        } else {
            Ok(())
        };
        completion_result?;
        let join_result = if let Some(worker) = self.worker.take() {
            let result = worker
                .join()
                .map_err(|_| {
                    std::io::Error::other("persistent bridge worker panicked during teardown")
                })
                .and_then(|result| result);
            if result.is_ok() {
                JOINED_PERSISTENT_BRIDGE_WORKERS.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
            }
            result
        } else {
            Ok(())
        };
        let residual_result = terminate_descendants(self.process_id);
        join_result.and(residual_result)
    }

    fn call(
        &mut self,
        family: &str,
        payload_frame: &str,
        max_bytes: Option<usize>,
        timeout_ms: Option<u64>,
    ) -> Result<Term, BridgeError> {
        let Some(requests) = self.requests.as_ref() else {
            return Err(BridgeError {
                code: format!("{family}/bridge-session"),
                message: "persistent bridge worker is not available".to_string(),
            });
        };
        let (response, result) = std::sync::mpsc::channel();
        requests
            .send(PersistentBridgeRequest {
                payload_frame: payload_frame.to_string(),
                max_bytes,
                response,
            })
            .map_err(|_| BridgeError {
                code: format!("{family}/bridge-session"),
                message: "persistent bridge worker disconnected".to_string(),
            })?;
        let Some(timeout_ms) = timeout_ms else {
            return result.recv().map_err(|_| BridgeError {
                code: format!("{family}/bridge-session"),
                message: "persistent bridge worker disconnected".to_string(),
            })?;
        };
        match result.recv_timeout(std::time::Duration::from_millis(timeout_ms)) {
            Ok(result) => result,
            Err(std::sync::mpsc::RecvTimeoutError::Timeout) => {
                let termination = self.stop();
                if let Err(error) = termination {
                    return Err(BridgeError {
                        code: format!("{family}/bridge-reap"),
                        message: format!(
                            "persistent bridge timeout failed to terminate process tree: {error}"
                        ),
                    });
                }
                Err(BridgeError {
                    code: format!("{family}/bridge-timeout"),
                    message: format!("persistent bridge request timed out after {timeout_ms}ms"),
                })
            }
            Err(std::sync::mpsc::RecvTimeoutError::Disconnected) => {
                let primary = BridgeError {
                    code: format!("{family}/bridge-session"),
                    message: "persistent bridge worker disconnected".to_string(),
                };
                match self.stop() {
                    Ok(()) => Err(primary),
                    Err(error) => Err(BridgeError {
                        code: format!("{family}/bridge-reap"),
                        message: format!(
                            "persistent bridge disconnect cleanup failed: {error}; prior error: {}",
                            primary.message
                        ),
                    }),
                }
            }
        }
    }
}

#[cfg(not(target_os = "wasi"))]
fn io_result_summary(result: &std::io::Result<()>) -> String {
    match result {
        Ok(()) => "ok".to_string(),
        Err(error) => error.to_string(),
    }
}

#[cfg(not(target_os = "wasi"))]
impl Drop for PersistentBridgeSession {
    fn drop(&mut self) {
        let _ = self.stop();
    }
}

#[cfg(not(target_os = "wasi"))]
fn read_framed_response(
    family: &str,
    stdout: &mut BufReader<ChildStdout>,
    max_bytes: Option<usize>,
) -> Result<Vec<u8>, BridgeError> {
    let mut header = String::new();
    let read = stdout.read_line(&mut header).map_err(|error| BridgeError {
        code: format!("{family}/bridge-stdout-read"),
        message: error.to_string(),
    })?;
    if read == 0 {
        return Err(BridgeError {
            code: format!("{family}/bridge-exit"),
            message: "persistent bridge session closed stdout".to_string(),
        });
    }
    let body_len = header
        .trim()
        .parse::<usize>()
        .map_err(|error| BridgeError {
            code: format!("{family}/bridge-parse"),
            message: format!(
                "invalid framed response length header `{}`: {error}",
                header.trim()
            ),
        })?;
    if let Some(limit) = max_bytes
        && body_len > limit
    {
        return Err(BridgeError {
            code: format!("{family}/bridge-response-too-large"),
            message: format!("bridge response exceeds max_bytes ({body_len} > {limit})"),
        });
    }
    let mut buf = vec![0u8; body_len];
    stdout.read_exact(&mut buf).map_err(|error| BridgeError {
        code: format!("{family}/bridge-stdout-read"),
        message: error.to_string(),
    })?;
    Ok(buf)
}

#[cfg(not(target_os = "wasi"))]
fn persistent_worker(
    key: PersistentBridgeSessionKey,
    requests: std::sync::mpsc::Receiver<PersistentBridgeRequest>,
    startup: std::sync::mpsc::SyncSender<Result<u32, BridgeError>>,
) -> std::io::Result<()> {
    let _active = ActivePersistentBridgeWorker::enter();
    let mut command = Command::new(&key.cmd_path);
    command.current_dir(&key.base_dir);
    command.args(&key.args);
    command.arg(&key.op);
    command.env("GENESIS_HOST_BRIDGE_OP", &key.op);
    command.env("GENESIS_HOST_BRIDGE_FAMILY", &key.family);
    command.env("GENESIS_HOST_BRIDGE_TRANSPORT", "persistent-stdio");
    command.stdin(Stdio::piped());
    command.stdout(Stdio::piped());
    command.stderr(Stdio::null());
    configure_killable_process(&mut command);
    let mut child = match command.spawn() {
        Ok(child) => child,
        Err(error) => {
            let _ = startup.send(Err(BridgeError {
                code: format!("{}/bridge-spawn", key.family),
                message: error.to_string(),
            }));
            return Ok(());
        }
    };
    let process_id = child.id();
    let Some(mut stdin) = child.stdin.take() else {
        let cleanup = terminate_and_reap(&mut child).map(|_| ());
        let _ = startup.send(Err(match &cleanup {
            Ok(()) => BridgeError {
                code: format!("{}/bridge-spawn", key.family),
                message: "bridge process missing stdin pipe".to_string(),
            },
            Err(error) => BridgeError {
                code: format!("{}/bridge-reap", key.family),
                message: format!("missing stdin pipe and process cleanup failed: {error}"),
            },
        }));
        return cleanup;
    };
    let Some(stdout) = child.stdout.take() else {
        let cleanup = terminate_and_reap(&mut child).map(|_| ());
        let _ = startup.send(Err(match &cleanup {
            Ok(()) => BridgeError {
                code: format!("{}/bridge-spawn", key.family),
                message: "bridge process missing stdout pipe".to_string(),
            },
            Err(error) => BridgeError {
                code: format!("{}/bridge-reap", key.family),
                message: format!("missing stdout pipe and process cleanup failed: {error}"),
            },
        }));
        return cleanup;
    };
    let mut stdout = BufReader::new(stdout);
    if startup.send(Ok(process_id)).is_err() {
        return terminate_and_reap(&mut child).map(|_| ());
    }
    while let Ok(request) = requests.recv() {
        let result = stdin
            .write_all(request.payload_frame.as_bytes())
            .and_then(|()| stdin.flush())
            .map_err(|error| BridgeError {
                code: format!("{}/bridge-stdin-write", key.family),
                message: error.to_string(),
            })
            .and_then(|()| read_framed_response(&key.family, &mut stdout, request.max_bytes))
            .and_then(|body| decode_bridge_stdout(&key.family, &body, request.max_bytes));
        let failed = result.is_err();
        let _ = request.response.send(result);
        if failed {
            break;
        }
    }
    terminate_and_reap(&mut child).map(|_| ())
}

#[cfg(not(target_os = "wasi"))]
fn spawn_persistent_bridge_session(
    key: &PersistentBridgeSessionKey,
) -> Result<PersistentBridgeSession, BridgeError> {
    let (request_sender, request_receiver) = std::sync::mpsc::sync_channel(1);
    let (startup_sender, startup_receiver) = std::sync::mpsc::sync_channel(1);
    let (worker_done_sender, worker_done_receiver) = std::sync::mpsc::sync_channel(1);
    let worker_key = key.clone();
    let worker = std::thread::Builder::new()
        .name("gc-persistent-bridge".to_string())
        .spawn(move || {
            let result = persistent_worker(worker_key, request_receiver, startup_sender);
            let _ = worker_done_sender.send(());
            result
        })
        .map_err(|error| BridgeError {
            code: format!("{}/bridge-thread", key.family),
            message: error.to_string(),
        })?;
    match startup_receiver.recv() {
        Ok(Ok(process_id)) => Ok(PersistentBridgeSession {
            process_id,
            requests: Some(request_sender),
            worker: Some(worker),
            worker_done: worker_done_receiver,
        }),
        Ok(Err(error)) => match worker.join() {
            Ok(Ok(())) => Err(error),
            Ok(Err(cleanup)) => Err(BridgeError {
                code: format!("{}/bridge-reap", key.family),
                message: format!(
                    "persistent bridge startup cleanup failed: {cleanup}; prior error: {}",
                    error.message
                ),
            }),
            Err(_) => Err(BridgeError {
                code: format!("{}/bridge-reap", key.family),
                message: format!(
                    "persistent bridge worker panicked during startup cleanup; prior error: {}",
                    error.message
                ),
            }),
        },
        Err(_) => {
            let primary = "persistent bridge worker disconnected during startup";
            match worker.join() {
                Ok(Ok(())) => Err(BridgeError {
                    code: format!("{}/bridge-session", key.family),
                    message: primary.to_string(),
                }),
                Ok(Err(cleanup)) => Err(BridgeError {
                    code: format!("{}/bridge-reap", key.family),
                    message: format!("{primary}; cleanup failed: {cleanup}"),
                }),
                Err(_) => Err(BridgeError {
                    code: format!("{}/bridge-reap", key.family),
                    message: format!("{primary}; worker panicked during cleanup"),
                }),
            }
        }
    }
}

#[cfg(not(target_os = "wasi"))]
fn ensure_persistent_bridge_session(
    runtime: &mut PersistentBridgeRuntime,
    key: &PersistentBridgeSessionKey,
) -> Result<(), BridgeError> {
    if runtime.sessions.contains_key(key) {
        return Ok(());
    }
    let session = spawn_persistent_bridge_session(key)?;
    runtime.sessions.insert(key.clone(), session);
    Ok(())
}

#[cfg(not(target_os = "wasi"))]
fn clear_persistent_bridge_session(
    runtime: &mut PersistentBridgeRuntime,
    key: &PersistentBridgeSessionKey,
) -> Result<(), BridgeError> {
    if let Some(mut session) = runtime.sessions.remove(key) {
        session.stop().map_err(|error| BridgeError {
            code: format!("{}/bridge-reap", key.family),
            message: format!("persistent bridge session cleanup failed: {error}"),
        })?;
    }
    Ok(())
}

#[cfg(not(target_os = "wasi"))]
fn run_persistent_bridge_process_once(
    runtime: &mut PersistentBridgeRuntime,
    key: &PersistentBridgeSessionKey,
    payload_frame: &str,
    timeout_ms: Option<u64>,
    max_bytes: Option<usize>,
) -> Result<Term, BridgeError> {
    ensure_persistent_bridge_session(runtime, key)?;
    let session = runtime.sessions.get_mut(key).ok_or_else(|| BridgeError {
        code: format!("{}/bridge-session", key.family),
        message: "persistent bridge session disappeared".to_string(),
    })?;
    match session.call(&key.family, payload_frame, max_bytes, timeout_ms) {
        Ok(response) => Ok(response),
        Err(primary) => Err(cleanup_after_error(runtime, key, primary)),
    }
}

#[cfg(not(target_os = "wasi"))]
#[expect(
    clippy::too_many_arguments,
    reason = "bridge process runner requires explicit io/time/resource limits for deterministic envelopes"
)]
pub(super) fn run_bridge_process_persistent(
    runtime: &mut PersistentBridgeRuntime,
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
    let key = PersistentBridgeSessionKey {
        family: family.to_string(),
        op: op.to_string(),
        base_dir: base_dir.to_path_buf(),
        cmd_path: cmd_path.to_path_buf(),
        args: args.to_vec(),
    };
    run_persistent_bridge_process_once(runtime, &key, &payload_frame, timeout_ms, max_bytes)
}

#[cfg(all(test, not(target_os = "wasi")))]
mod tests {
    use super::*;

    fn synthetic_key(family: &str) -> PersistentBridgeSessionKey {
        PersistentBridgeSessionKey {
            family: family.to_string(),
            op: "test/bridge::call".to_string(),
            base_dir: std::path::PathBuf::from("."),
            cmd_path: std::path::PathBuf::from("fixture"),
            args: Vec::new(),
        }
    }

    #[test]
    fn runtime_shutdown_reports_worker_reap_failure() {
        let (done_sender, done_receiver) = std::sync::mpsc::sync_channel(1);
        let worker = std::thread::spawn(move || -> std::io::Result<()> {
            done_sender.send(()).expect("signal synthetic completion");
            Err(std::io::Error::other("injected worker reap failure"))
        });
        let mut runtime = PersistentBridgeRuntime::default();
        runtime.sessions.insert(
            synthetic_key("model"),
            PersistentBridgeSession {
                process_id: u32::MAX,
                requests: None,
                worker: Some(worker),
                worker_done: done_receiver,
            },
        );

        let error = runtime
            .shutdown()
            .expect_err("worker reap failure must cross explicit owner shutdown");
        assert_eq!(error.code, "model/bridge-reap");
        assert!(error.message.contains("injected worker reap failure"));
        assert!(runtime.sessions.is_empty());
    }

    #[test]
    fn persistent_stop_is_bounded_when_signal_and_reap_fail() {
        let (release_sender, release_receiver) = std::sync::mpsc::sync_channel(1);
        let (done_sender, done_receiver) = std::sync::mpsc::sync_channel(1);
        let worker = std::thread::spawn(move || -> std::io::Result<()> {
            let _ = release_receiver.recv();
            let _ = done_sender.send(());
            Ok(())
        });
        let joined_before = joined_persistent_bridge_workers_for_tests();
        let mut session = PersistentBridgeSession {
            process_id: u32::MAX,
            requests: None,
            worker: Some(worker),
            worker_done: done_receiver,
        };

        let started = std::time::Instant::now();
        let error = session
            .stop()
            .expect_err("blocked worker with invalid process group must fail boundedly");
        assert_eq!(error.kind(), std::io::ErrorKind::TimedOut);
        assert!(
            started.elapsed() >= std::time::Duration::from_millis(200)
                && started.elapsed() < std::time::Duration::from_millis(750),
            "persistent stop did not honor its bounded worker deadline: {:?}",
            started.elapsed()
        );
        assert!(session.worker.is_some(), "timed-out worker handle was lost");

        release_sender.send(()).expect("release synthetic worker");
        let _ = session.stop();
        assert!(session.worker.is_none(), "released worker was not joined");
        assert!(
            joined_persistent_bridge_workers_for_tests() > joined_before,
            "released synthetic worker was not included in the monotonic join count"
        );
    }
}
