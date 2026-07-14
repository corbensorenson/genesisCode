use super::*;

#[cfg(not(target_os = "wasi"))]
use crate::runner_process_control::{
    configure_killable_process, terminate_and_reap, terminate_descendants,
};

#[cfg(not(target_os = "wasi"))]
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
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
    worker: Option<std::thread::JoinHandle<()>>,
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
    fn stop(&mut self) {
        let _ = terminate_descendants(self.process_id);
        self.requests.take();
        if let Some(worker) = self.worker.take() {
            let _ = worker.join();
            JOINED_PERSISTENT_BRIDGE_WORKERS.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
        }
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
                let termination = terminate_descendants(self.process_id);
                let _ = result.recv();
                self.stop();
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
                self.stop();
                Err(BridgeError {
                    code: format!("{family}/bridge-session"),
                    message: "persistent bridge worker disconnected".to_string(),
                })
            }
        }
    }
}

#[cfg(not(target_os = "wasi"))]
impl Drop for PersistentBridgeSession {
    fn drop(&mut self) {
        self.stop();
    }
}

#[cfg(not(target_os = "wasi"))]
fn persistent_bridge_session_map()
-> &'static Mutex<HashMap<PersistentBridgeSessionKey, Arc<Mutex<PersistentBridgeSession>>>> {
    static SESSIONS: OnceLock<
        Mutex<HashMap<PersistentBridgeSessionKey, Arc<Mutex<PersistentBridgeSession>>>>,
    > = OnceLock::new();
    SESSIONS.get_or_init(|| Mutex::new(HashMap::new()))
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
) {
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
            return;
        }
    };
    let process_id = child.id();
    let Some(mut stdin) = child.stdin.take() else {
        let _ = terminate_and_reap(&mut child);
        let _ = startup.send(Err(BridgeError {
            code: format!("{}/bridge-spawn", key.family),
            message: "bridge process missing stdin pipe".to_string(),
        }));
        return;
    };
    let Some(stdout) = child.stdout.take() else {
        let _ = terminate_and_reap(&mut child);
        let _ = startup.send(Err(BridgeError {
            code: format!("{}/bridge-spawn", key.family),
            message: "bridge process missing stdout pipe".to_string(),
        }));
        return;
    };
    let mut stdout = BufReader::new(stdout);
    if startup.send(Ok(process_id)).is_err() {
        let _ = terminate_and_reap(&mut child);
        return;
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
    let _ = terminate_and_reap(&mut child);
}

#[cfg(not(target_os = "wasi"))]
fn spawn_persistent_bridge_session(
    key: &PersistentBridgeSessionKey,
) -> Result<PersistentBridgeSession, BridgeError> {
    let (request_sender, request_receiver) = std::sync::mpsc::sync_channel(1);
    let (startup_sender, startup_receiver) = std::sync::mpsc::sync_channel(1);
    let worker_key = key.clone();
    let worker = std::thread::Builder::new()
        .name("gc-persistent-bridge".to_string())
        .spawn(move || persistent_worker(worker_key, request_receiver, startup_sender))
        .map_err(|error| BridgeError {
            code: format!("{}/bridge-thread", key.family),
            message: error.to_string(),
        })?;
    match startup_receiver.recv() {
        Ok(Ok(process_id)) => Ok(PersistentBridgeSession {
            process_id,
            requests: Some(request_sender),
            worker: Some(worker),
        }),
        Ok(Err(error)) => {
            let _ = worker.join();
            Err(error)
        }
        Err(_) => {
            let _ = worker.join();
            Err(BridgeError {
                code: format!("{}/bridge-session", key.family),
                message: "persistent bridge worker disconnected during startup".to_string(),
            })
        }
    }
}

#[cfg(not(target_os = "wasi"))]
fn ensure_persistent_bridge_session(key: &PersistentBridgeSessionKey) -> Result<(), BridgeError> {
    let map = persistent_bridge_session_map();
    let mut sessions = map.lock().map_err(|_| BridgeError {
        code: format!("{}/bridge-session", key.family),
        message: "persistent bridge session map lock poisoned".to_string(),
    })?;
    if sessions.contains_key(key) {
        return Ok(());
    }
    let session = spawn_persistent_bridge_session(key)?;
    sessions.insert(key.clone(), Arc::new(Mutex::new(session)));
    Ok(())
}

#[cfg(not(target_os = "wasi"))]
fn clear_persistent_bridge_session(key: &PersistentBridgeSessionKey) {
    let session = persistent_bridge_session_map()
        .lock()
        .ok()
        .and_then(|mut sessions| sessions.remove(key));
    if let Some(session) = session
        && let Ok(mut session) = session.lock()
    {
        session.stop();
    }
}

#[cfg(all(test, not(target_os = "wasi")))]
pub(super) fn reset_persistent_bridge_sessions_for_tests() {
    let sessions = persistent_bridge_session_map()
        .lock()
        .map(|mut sessions| {
            sessions
                .drain()
                .map(|(_, session)| session)
                .collect::<Vec<_>>()
        })
        .unwrap_or_default();
    for session in sessions {
        if let Ok(mut session) = session.lock() {
            session.stop();
        }
    }
}

#[cfg(not(target_os = "wasi"))]
fn run_persistent_bridge_process_once(
    key: &PersistentBridgeSessionKey,
    payload_frame: &str,
    timeout_ms: Option<u64>,
    max_bytes: Option<usize>,
) -> Result<Term, BridgeError> {
    ensure_persistent_bridge_session(key)?;
    let session = persistent_bridge_session_map()
        .lock()
        .map_err(|_| BridgeError {
            code: format!("{}/bridge-session", key.family),
            message: "persistent bridge session map lock poisoned".to_string(),
        })?
        .get(key)
        .cloned()
        .ok_or_else(|| BridgeError {
            code: format!("{}/bridge-session", key.family),
            message: "persistent bridge session disappeared".to_string(),
        })?;
    let result = session
        .lock()
        .map_err(|_| BridgeError {
            code: format!("{}/bridge-session", key.family),
            message: "persistent bridge session lock poisoned".to_string(),
        })?
        .call(&key.family, payload_frame, max_bytes, timeout_ms);
    if result.is_err() {
        clear_persistent_bridge_session(key);
    }
    result
}

#[cfg(not(target_os = "wasi"))]
#[expect(
    clippy::too_many_arguments,
    reason = "bridge process runner requires explicit io/time/resource limits for deterministic envelopes"
)]
pub(super) fn run_bridge_process_persistent(
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
    run_persistent_bridge_process_once(&key, &payload_frame, timeout_ms, max_bytes)
}
