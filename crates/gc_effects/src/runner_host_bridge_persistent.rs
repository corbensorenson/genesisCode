use super::*;

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
struct PersistentBridgeSession {
    child: Child,
    stdin: ChildStdin,
    stdout: BufReader<ChildStdout>,
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
fn spawn_persistent_bridge_session(
    family: &str,
    op: &str,
    base_dir: &std::path::Path,
    cmd_path: &std::path::Path,
    args: &[String],
) -> Result<PersistentBridgeSession, BridgeError> {
    let mut cmd = Command::new(cmd_path);
    cmd.current_dir(base_dir);
    for arg in args {
        cmd.arg(arg);
    }
    cmd.arg(op);
    cmd.env("GENESIS_HOST_BRIDGE_OP", op);
    cmd.env("GENESIS_HOST_BRIDGE_FAMILY", family);
    cmd.env("GENESIS_HOST_BRIDGE_TRANSPORT", "persistent-stdio");
    cmd.stdin(Stdio::piped());
    cmd.stdout(Stdio::piped());
    cmd.stderr(Stdio::null());
    let mut child = cmd.spawn().map_err(|e| BridgeError {
        code: format!("{family}/bridge-spawn"),
        message: e.to_string(),
    })?;
    let Some(stdin) = child.stdin.take() else {
        return Err(BridgeError {
            code: format!("{family}/bridge-spawn"),
            message: "bridge process missing stdin pipe".to_string(),
        });
    };
    let Some(stdout) = child.stdout.take() else {
        return Err(BridgeError {
            code: format!("{family}/bridge-spawn"),
            message: "bridge process missing stdout pipe".to_string(),
        });
    };
    Ok(PersistentBridgeSession {
        child,
        stdin,
        stdout: BufReader::new(stdout),
    })
}

#[cfg(not(target_os = "wasi"))]
fn with_persistent_bridge_session<R, F>(
    key: &PersistentBridgeSessionKey,
    family: &str,
    op: &str,
    action: F,
) -> Result<R, BridgeError>
where
    F: FnOnce(&mut PersistentBridgeSession) -> Result<R, BridgeError>,
{
    let session = {
        let map = persistent_bridge_session_map();
        let guard = map.lock().map_err(|_| BridgeError {
            code: format!("{family}/bridge-session"),
            message: "persistent bridge session map lock poisoned".to_string(),
        })?;
        guard.get(key).cloned().ok_or_else(|| BridgeError {
            code: format!("{family}/bridge-session"),
            message: format!("persistent bridge session missing for op `{op}`"),
        })?
    };

    let mut guard = session.lock().map_err(|_| BridgeError {
        code: format!("{family}/bridge-session"),
        message: "persistent bridge session lock poisoned".to_string(),
    })?;
    action(&mut guard)
}

#[cfg(not(target_os = "wasi"))]
fn ensure_persistent_bridge_session(
    key: &PersistentBridgeSessionKey,
    family: &str,
    op: &str,
) -> Result<(), BridgeError> {
    let map = persistent_bridge_session_map();
    let mut guard = map.lock().map_err(|_| BridgeError {
        code: format!("{family}/bridge-session"),
        message: "persistent bridge session map lock poisoned".to_string(),
    })?;
    if guard.contains_key(key) {
        return Ok(());
    }
    let spawned =
        spawn_persistent_bridge_session(family, op, &key.base_dir, &key.cmd_path, &key.args)?;
    guard.insert(key.clone(), Arc::new(Mutex::new(spawned)));
    Ok(())
}

#[cfg(not(target_os = "wasi"))]
fn clear_persistent_bridge_session(key: &PersistentBridgeSessionKey, family: &str) {
    let map = persistent_bridge_session_map();
    let Ok(mut guard) = map.lock() else {
        return;
    };
    let Some(session) = guard.remove(key) else {
        return;
    };
    if let Ok(mut locked) = session.lock() {
        let _ = locked.child.kill().map_err(|e| BridgeError {
            code: format!("{family}/bridge-session"),
            message: format!("kill persistent bridge session failed: {e}"),
        });
        let _ = locked.child.wait();
    }
}

#[cfg(all(test, not(target_os = "wasi")))]
pub(super) fn reset_persistent_bridge_sessions_for_tests() {
    let map = persistent_bridge_session_map();
    let Ok(mut guard) = map.lock() else {
        return;
    };
    let sessions: Vec<_> = guard.drain().map(|(_, session)| session).collect();
    drop(guard);
    for session in sessions {
        if let Ok(mut locked) = session.lock() {
            let _ = locked.child.kill();
            let _ = locked.child.wait();
        }
    }
}

#[cfg(not(target_os = "wasi"))]
fn read_framed_response(
    family: &str,
    stdout: &mut BufReader<ChildStdout>,
    max_bytes: Option<usize>,
) -> Result<Vec<u8>, BridgeError> {
    let mut header = String::new();
    let read = stdout.read_line(&mut header).map_err(|e| BridgeError {
        code: format!("{family}/bridge-stdout-read"),
        message: e.to_string(),
    })?;
    if read == 0 {
        return Err(BridgeError {
            code: format!("{family}/bridge-exit"),
            message: "persistent bridge session closed stdout".to_string(),
        });
    }
    let body_len = header.trim().parse::<usize>().map_err(|e| BridgeError {
        code: format!("{family}/bridge-parse"),
        message: format!(
            "invalid framed response length header `{}`: {e}",
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
    stdout.read_exact(&mut buf).map_err(|e| BridgeError {
        code: format!("{family}/bridge-stdout-read"),
        message: e.to_string(),
    })?;
    Ok(buf)
}

#[cfg(not(target_os = "wasi"))]
fn run_persistent_bridge_process_once(
    key: &PersistentBridgeSessionKey,
    family: &str,
    op: &str,
    payload_frame: &str,
    max_bytes: Option<usize>,
) -> Result<Term, BridgeError> {
    ensure_persistent_bridge_session(key, family, op)?;
    let result = with_persistent_bridge_session(key, family, op, |session| {
        session
            .stdin
            .write_all(payload_frame.as_bytes())
            .map_err(|e| BridgeError {
                code: format!("{family}/bridge-stdin-write"),
                message: e.to_string(),
            })?;
        session.stdin.flush().map_err(|e| BridgeError {
            code: format!("{family}/bridge-stdin-write"),
            message: e.to_string(),
        })?;
        let body = read_framed_response(family, &mut session.stdout, max_bytes)?;
        decode_bridge_stdout(family, &body, max_bytes)
    });
    if result.is_ok() {
        return result;
    }
    clear_persistent_bridge_session(key, family);
    ensure_persistent_bridge_session(key, family, op)?;
    with_persistent_bridge_session(key, family, op, |session| {
        session
            .stdin
            .write_all(payload_frame.as_bytes())
            .map_err(|e| BridgeError {
                code: format!("{family}/bridge-stdin-write"),
                message: e.to_string(),
            })?;
        session.stdin.flush().map_err(|e| BridgeError {
            code: format!("{family}/bridge-stdin-write"),
            message: e.to_string(),
        })?;
        let body = read_framed_response(family, &mut session.stdout, max_bytes)?;
        decode_bridge_stdout(family, &body, max_bytes)
    })
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
    if timeout_ms.is_some() {
        return Err(BridgeError {
            code: format!("{family}/bridge-policy"),
            message:
                "timeout_ms is not supported for bridge_transport `persistent-stdio`; use `spawn-per-op` for hard timeout enforcement".to_string(),
        });
    }
    run_persistent_bridge_process_once(&key, family, op, &payload_frame, max_bytes)
}
