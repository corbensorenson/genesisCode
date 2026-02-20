use gc_coreform::{Term, TermOrdKey, parse_term, print_term};
use sha2::{Digest, Sha256};
#[cfg(not(target_os = "wasi"))]
use std::collections::HashMap;
#[cfg(not(target_os = "wasi"))]
use std::io::{BufRead as _, BufReader, Read as _, Write as _};
#[cfg(not(target_os = "wasi"))]
use std::process::{Child, ChildStdin, ChildStdout, Command, Stdio};
#[cfg(not(target_os = "wasi"))]
use std::sync::{Arc, Mutex, OnceLock};

use crate::EffectsError;
use crate::policy::OpPolicy;
use crate::runner_io_ops::{effective_base_dir, sandbox_path_read};
use crate::runner_timeout::with_timeout;

#[derive(Debug, Clone)]
pub(crate) struct BridgeError {
    pub code: String,
    pub message: String,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum BridgeTransport {
    SpawnPerOp,
    PersistentStdio,
}

fn wasi_bridge_profile_enabled(pol: Option<&OpPolicy>) -> bool {
    cfg!(target_os = "wasi")
        || pol
            .and_then(|p| p.extra.get("wasi_bridge_profile"))
            .and_then(|v| v.as_bool())
            .unwrap_or(false)
}

pub(crate) fn call_host_bridge(
    family: &str,
    op: &str,
    payload: &Term,
    pol: Option<&OpPolicy>,
) -> Result<Term, BridgeError> {
    let max_bytes = bridge_max_bytes(pol, family)?;
    let transport = bridge_transport(pol, family)?;
    if wasi_bridge_profile_enabled(pol) {
        return run_wasi_bridge_profile(family, op, payload, pol, max_bytes);
    }

    let Some(cmd_raw) = bridge_cmd(pol) else {
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
    enforce_bridge_identity(family, &cmd_raw, &cmd_path, pol)?;
    let args = bridge_args(pol);
    let timeout_ms = pol.and_then(|p| p.timeout_ms).filter(|ms| *ms > 0);
    match transport {
        BridgeTransport::SpawnPerOp => run_bridge_process(
            family, op, payload, &base_dir, &cmd_path, &args, timeout_ms, max_bytes,
        ),
        BridgeTransport::PersistentStdio => run_bridge_process_persistent(
            family, op, payload, &base_dir, &cmd_path, &args, timeout_ms, max_bytes,
        ),
    }
}

fn bridge_cmd(pol: Option<&OpPolicy>) -> Option<String> {
    pol.and_then(|p| p.extra.get("bridge_cmd"))
        .and_then(|v| v.as_str())
        .map(ToString::to_string)
}

fn bridge_args(pol: Option<&OpPolicy>) -> Vec<String> {
    pol.and_then(|p| p.extra.get("bridge_args"))
        .and_then(|v| v.as_array())
        .map(|arr| {
            arr.iter()
                .filter_map(|x| x.as_str().map(ToString::to_string))
                .collect::<Vec<_>>()
        })
        .unwrap_or_default()
}

fn bridge_transport(pol: Option<&OpPolicy>, family: &str) -> Result<BridgeTransport, BridgeError> {
    let Some(raw) = pol
        .and_then(|p| p.extra.get("bridge_transport"))
        .and_then(|v| v.as_str())
    else {
        return Ok(BridgeTransport::SpawnPerOp);
    };
    match raw.trim() {
        "" | "spawn-per-op" => Ok(BridgeTransport::SpawnPerOp),
        "persistent-stdio" => Ok(BridgeTransport::PersistentStdio),
        other => Err(BridgeError {
            code: format!("{family}/bridge-policy"),
            message: format!(
                "bridge_transport must be one of: spawn-per-op, persistent-stdio (got `{other}`)"
            ),
        }),
    }
}

fn normalize_sha256_hex(raw: &str) -> Option<String> {
    let trimmed = raw.trim();
    let hex = trimmed
        .strip_prefix("sha256:")
        .or_else(|| trimmed.strip_prefix("SHA256:"))
        .unwrap_or(trimmed);
    if hex.len() != 64 || !hex.chars().all(|c| c.is_ascii_hexdigit()) {
        return None;
    }
    Some(hex.to_ascii_lowercase())
}

fn bridge_cmd_allowlist(
    pol: Option<&OpPolicy>,
    family: &str,
) -> Result<Option<Vec<String>>, BridgeError> {
    let Some(v) = pol.and_then(|p| p.extra.get("bridge_cmd_allowlist")) else {
        return Ok(None);
    };
    let Some(arr) = v.as_array() else {
        return Err(BridgeError {
            code: format!("{family}/bridge-policy"),
            message: "bridge_cmd_allowlist must be an array of strings".to_string(),
        });
    };
    let mut out = Vec::with_capacity(arr.len());
    for item in arr {
        let Some(s) = item.as_str() else {
            return Err(BridgeError {
                code: format!("{family}/bridge-policy"),
                message: "bridge_cmd_allowlist must contain only strings".to_string(),
            });
        };
        let trimmed = s.trim();
        if trimmed.is_empty() {
            return Err(BridgeError {
                code: format!("{family}/bridge-policy"),
                message: "bridge_cmd_allowlist entries must be non-empty".to_string(),
            });
        }
        out.push(trimmed.to_string());
    }
    Ok(Some(out))
}

fn bridge_cmd_sha256(pol: Option<&OpPolicy>, family: &str) -> Result<Option<String>, BridgeError> {
    let Some(raw) = pol
        .and_then(|p| p.extra.get("bridge_cmd_sha256"))
        .and_then(|v| v.as_str())
    else {
        return Ok(None);
    };
    let Some(hex) = normalize_sha256_hex(raw) else {
        return Err(BridgeError {
            code: format!("{family}/bridge-policy"),
            message:
                "bridge_cmd_sha256 must be a 64-hex digest (optionally prefixed with `sha256:`)"
                    .to_string(),
        });
    };
    Ok(Some(hex))
}

fn bridge_cmd_matches_allowlist(
    cmd_raw: &str,
    cmd_path: &std::path::Path,
    allowlist: &[String],
) -> bool {
    let cmd_path_s = cmd_path.to_string_lossy();
    let cmd_name = cmd_path.file_name().and_then(|n| n.to_str());
    allowlist.iter().any(|allowed| {
        let token = allowed.trim();
        token == cmd_raw || token == cmd_path_s || cmd_name.is_some_and(|n| n == token)
    })
}

fn file_sha256_hex(path: &std::path::Path) -> Result<String, std::io::Error> {
    use std::io::Read as _;

    let mut f = std::fs::File::open(path)?;
    let mut hasher = Sha256::new();
    let mut buf = [0u8; 8 * 1024];
    loop {
        let n = f.read(&mut buf)?;
        if n == 0 {
            break;
        }
        hasher.update(&buf[..n]);
    }
    Ok(format!("{:x}", hasher.finalize()))
}

fn enforce_bridge_identity(
    family: &str,
    cmd_raw: &str,
    cmd_path: &std::path::Path,
    pol: Option<&OpPolicy>,
) -> Result<(), BridgeError> {
    if let Some(allowlist) = bridge_cmd_allowlist(pol, family)?
        && !bridge_cmd_matches_allowlist(cmd_raw, cmd_path, &allowlist)
    {
        return Err(BridgeError {
            code: format!("{family}/bridge-identity-denied"),
            message: format!(
                "bridge command `{}` is not in bridge_cmd_allowlist",
                cmd_path.display()
            ),
        });
    }

    if let Some(expected_sha256) = bridge_cmd_sha256(pol, family)? {
        let observed_sha256 = file_sha256_hex(cmd_path).map_err(|e| BridgeError {
            code: format!("{family}/bridge-identity-denied"),
            message: format!(
                "failed to hash bridge command `{}`: {e}",
                cmd_path.display()
            ),
        })?;
        if observed_sha256 != expected_sha256 {
            return Err(BridgeError {
                code: format!("{family}/bridge-identity-denied"),
                message: format!(
                    "bridge command digest mismatch for `{}` (expected {expected_sha256}, got {observed_sha256})",
                    cmd_path.display()
                ),
            });
        }
    }
    Ok(())
}

fn bridge_max_bytes(pol: Option<&OpPolicy>, family: &str) -> Result<Option<usize>, BridgeError> {
    let Some(v) = pol.and_then(|p| p.extra.get("max_bytes")) else {
        return Ok(None);
    };
    let Some(raw) = v.as_integer() else {
        return Err(BridgeError {
            code: format!("{family}/bridge-policy"),
            message: "max_bytes must be a positive integer".to_string(),
        });
    };
    if raw <= 0 {
        return Err(BridgeError {
            code: format!("{family}/bridge-policy"),
            message: "max_bytes must be > 0".to_string(),
        });
    }
    let Some(max) = usize::try_from(raw).ok() else {
        return Err(BridgeError {
            code: format!("{family}/bridge-policy"),
            message: "max_bytes is too large".to_string(),
        });
    };
    Ok(Some(max))
}

fn enforce_payload_limit(
    family: &str,
    payload: &Term,
    max_bytes: Option<usize>,
) -> Result<(), BridgeError> {
    let payload_src = print_term(payload);
    if let Some(limit) = max_bytes
        && payload_src.len() > limit
    {
        return Err(BridgeError {
            code: format!("{family}/bridge-payload-too-large"),
            message: format!(
                "bridge payload exceeds max_bytes ({} > {})",
                payload_src.len(),
                limit
            ),
        });
    }
    Ok(())
}

fn enforce_response_limit(
    family: &str,
    response: &Term,
    max_bytes: Option<usize>,
) -> Result<(), BridgeError> {
    if let Some(limit) = max_bytes {
        let response_src = print_term(response);
        if response_src.len() > limit {
            return Err(BridgeError {
                code: format!("{family}/bridge-response-too-large"),
                message: format!(
                    "bridge response exceeds max_bytes ({} > {limit})",
                    response_src.len()
                ),
            });
        }
    }
    Ok(())
}

fn map_lookup_str_or_sym(
    map: &std::collections::BTreeMap<TermOrdKey, Term>,
    key: &str,
) -> Option<Term> {
    map.get(&TermOrdKey(Term::symbol(key)))
        .or_else(|| map.get(&TermOrdKey(Term::Str(key.to_string()))))
        .cloned()
}

fn wasi_bridge_response_for_op(
    pol: Option<&OpPolicy>,
    op: &str,
) -> Result<Option<Term>, BridgeError> {
    let Some(pol) = pol else {
        return Ok(None);
    };

    if let Some(raw) = pol
        .extra
        .get("wasi_bridge_response")
        .and_then(|v| v.as_str())
    {
        let parsed = parse_term(raw).map_err(|e| BridgeError {
            code: "wasi/bridge-response-parse".to_string(),
            message: format!("wasi_bridge_response parse error: {e}"),
        })?;
        return Ok(Some(parsed));
    }

    if let Some(raw) = pol
        .extra
        .get("wasi_bridge_responses")
        .and_then(|v| v.as_str())
    {
        let parsed = parse_term(raw).map_err(|e| BridgeError {
            code: "wasi/bridge-responses-parse".to_string(),
            message: format!("wasi_bridge_responses parse error: {e}"),
        })?;
        if let Term::Map(m) = parsed
            && let Some(resp) = map_lookup_str_or_sym(&m, op)
        {
            return Ok(Some(resp));
        }
    }

    if let Some(file_raw) = pol
        .extra
        .get("wasi_bridge_response_file")
        .and_then(|v| v.as_str())
    {
        let base_dir = effective_base_dir(Some(pol)).map_err(|e| BridgeError {
            code: "wasi/bridge-response-file-path".to_string(),
            message: e.to_string(),
        })?;
        let file = sandbox_path_read(&base_dir, file_raw).map_err(|e| BridgeError {
            code: "wasi/bridge-response-file-path".to_string(),
            message: e.to_string(),
        })?;
        let bytes = std::fs::read(&file).map_err(|e| BridgeError {
            code: "wasi/bridge-response-file-read".to_string(),
            message: e.to_string(),
        })?;
        let parsed = decode_bridge_stdout("wasi", &bytes, None)?;
        if let Term::Map(m) = parsed.clone()
            && let Some(resp) = map_lookup_str_or_sym(&m, op)
        {
            return Ok(Some(resp));
        }
        return Ok(Some(parsed));
    }

    if let Ok(raw) = std::env::var("GENESIS_WASI_BRIDGE_RESPONSES") {
        let parsed = parse_term(&raw).map_err(|e| BridgeError {
            code: "wasi/bridge-env-parse".to_string(),
            message: e.to_string(),
        })?;
        if let Term::Map(m) = parsed
            && let Some(resp) = map_lookup_str_or_sym(&m, op)
        {
            return Ok(Some(resp));
        }
    }

    Ok(None)
}

fn run_wasi_bridge_profile(
    family: &str,
    op: &str,
    payload: &Term,
    pol: Option<&OpPolicy>,
    max_bytes: Option<usize>,
) -> Result<Term, BridgeError> {
    enforce_payload_limit(family, payload, max_bytes)?;
    let Some(response) = wasi_bridge_response_for_op(pol, op)? else {
        return Err(BridgeError {
            code: format!("{family}/bridge-wasi-profile-required"),
            message: format!(
                "{op} requires wasi bridge profile data (set per-op `wasi_bridge_response`/`wasi_bridge_response_file` or GENESIS_WASI_BRIDGE_RESPONSES)"
            ),
        });
    };
    enforce_response_limit(family, &response, max_bytes)?;
    Ok(response)
}

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
fn reset_persistent_bridge_sessions_for_tests() {
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
fn run_bridge_process_persistent(
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
    enforce_payload_limit(family, payload, max_bytes)?;
    let payload_frame = format!("{}\n{}", payload_src.len(), payload_src);
    let key = PersistentBridgeSessionKey {
        family: family.to_string(),
        op: op.to_string(),
        base_dir: base_dir.to_path_buf(),
        cmd_path: cmd_path.to_path_buf(),
        args: args.to_vec(),
    };
    if let Some(ms) = timeout_ms {
        let key2 = key.clone();
        let payload_frame2 = payload_frame.clone();
        let family_s = family.to_string();
        let op_s = op.to_string();
        match with_timeout(ms, move || {
            run_persistent_bridge_process_once(&key2, &family_s, &op_s, &payload_frame2, max_bytes)
                .map_err(|e| EffectsError::Log(format!("{}: {}", e.code, e.message)))
        })
        .map_err(|e| BridgeError {
            code: format!("{family}/bridge-timeout-runtime"),
            message: e.to_string(),
        })? {
            Some(resp) => Ok(resp),
            None => Err(BridgeError {
                code: format!("{family}/bridge-timeout"),
                message: format!("persistent bridge command timed out after {ms}ms"),
            }),
        }
    } else {
        run_persistent_bridge_process_once(&key, family, op, &payload_frame, max_bytes)
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
    enforce_payload_limit(family, payload, max_bytes)?;
    let payload_frame = format!("{}\n{}", payload_src.len(), payload_src);
    let output = if let Some(ms) = timeout_ms {
        let base_dir = base_dir.to_path_buf();
        let cmd_path = cmd_path.to_path_buf();
        let args = args.to_vec();
        let op_s = op.to_string();
        let payload_frame = payload_frame.clone();
        let family_s = family.to_string();
        match with_timeout(ms, move || {
            run_bridge_process_once(
                &family_s,
                &op_s,
                &payload_frame,
                &base_dir,
                &cmd_path,
                &args,
            )
            .map_err(|e| EffectsError::Log(format!("{}: {}", e.code, e.message)))
        })
        .map_err(|e| BridgeError {
            code: format!("{family}/bridge-timeout-runtime"),
            message: e.to_string(),
        })? {
            Some(out) => out,
            None => {
                return Err(BridgeError {
                    code: format!("{family}/bridge-timeout"),
                    message: format!("bridge command timed out after {ms}ms"),
                });
            }
        }
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
mod tests {
    use super::{call_host_bridge, decode_bridge_stdout, normalize_sha256_hex};
    use crate::policy::CapsPolicy;
    use gc_coreform::{Term, TermOrdKey};
    #[cfg(not(target_os = "wasi"))]
    use std::time::Instant;

    #[test]
    fn framed_response_decodes() {
        let body = "{:ok true :id \"x\"}";
        let out = format!("{}\n{}", body.len(), body);
        let term = decode_bridge_stdout("test", out.as_bytes(), None).expect("decode");
        let Term::Map(m) = term else {
            panic!("expected map");
        };
        assert_eq!(
            m.get(&TermOrdKey(Term::symbol(":id"))),
            Some(&Term::Str("x".to_string()))
        );
    }

    #[test]
    fn response_limit_is_enforced() {
        let body = "{:ok true :id \"x\"}";
        let out = format!("{}\n{}", body.len(), body);
        let err = decode_bridge_stdout("test", out.as_bytes(), Some(4)).expect_err("must fail");
        assert_eq!(err.code, "test/bridge-response-too-large");
    }

    #[test]
    fn forced_wasi_profile_supports_inline_response() {
        let policy = CapsPolicy::from_toml_str(
            r#"
allow = ["gpu/compute::limits"]

[op."gpu/compute::limits"]
wasi_bridge_profile = true
wasi_bridge_response = "{:ok true :transport :wasi}"
"#,
        )
        .expect("caps");
        let resp = call_host_bridge(
            "gpu",
            "gpu/compute::limits",
            &Term::Map(
                [(
                    TermOrdKey(Term::symbol(":payload")),
                    Term::Str("x".to_string()),
                )]
                .into_iter()
                .collect(),
            ),
            policy.op_policy("gpu/compute::limits"),
        )
        .expect("wasi bridge");
        let Term::Map(mm) = resp else {
            panic!("map response expected");
        };
        assert_eq!(
            mm.get(&TermOrdKey(Term::symbol(":transport"))),
            Some(&Term::symbol(":wasi"))
        );
    }

    #[test]
    fn forced_wasi_profile_reports_missing_profile_data() {
        let policy = CapsPolicy::from_toml_str(
            r#"
allow = ["gpu/compute::limits"]

[op."gpu/compute::limits"]
wasi_bridge_profile = true
"#,
        )
        .expect("caps");
        let err = call_host_bridge(
            "gpu",
            "gpu/compute::limits",
            &Term::Nil,
            policy.op_policy("gpu/compute::limits"),
        )
        .expect_err("missing wasi profile data should fail");
        assert_eq!(err.code, "gpu/bridge-wasi-profile-required");
    }

    #[test]
    fn normalize_sha256_hex_accepts_prefixed_and_plain_hex() {
        let raw = "abcdefabcdefabcdefabcdefabcdefabcdefabcdefabcdefabcdefabcdefabcd";
        assert_eq!(normalize_sha256_hex(raw), Some(raw.to_string()));
        assert_eq!(
            normalize_sha256_hex(&format!("sha256:{raw}")),
            Some(raw.to_string())
        );
        assert!(normalize_sha256_hex("sha256:not-a-hex").is_none());
        assert!(normalize_sha256_hex("abc").is_none());
    }

    #[cfg(not(target_os = "wasi"))]
    fn write_persistent_bridge_script(path: &std::path::Path) {
        let src = r#"#!/usr/bin/env sh
set -eu
op="$1"
startup_delay_ms=35
python3 - "$startup_delay_ms" <<'PY'
import sys, time
time.sleep(int(sys.argv[1]) / 1000.0)
PY
if [ "${GENESIS_HOST_BRIDGE_TRANSPORT:-}" = "persistent-stdio" ]; then
  persistent=1
else
  persistent=0
fi
while IFS= read -r req_len; do
  if [ -z "${req_len:-}" ]; then
    exit 0
  fi
  dd bs=1 count="$req_len" status=none >/dev/null 2>/dev/null || true
  resp="{:ok true :kind :bridge-ok :op \"$op\"}"
  resp_len="$(printf '%s' "$resp" | wc -c | tr -d '[:space:]')"
  printf '%s\n%s' "$resp_len" "$resp"
  if [ "$persistent" != "1" ]; then
    exit 0
  fi
done
"#;
        std::fs::write(path, src).expect("write persistent bridge script");
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            let mut perms = std::fs::metadata(path)
                .expect("bridge metadata")
                .permissions();
            perms.set_mode(0o755);
            std::fs::set_permissions(path, perms).expect("bridge chmod");
        }
    }

    #[cfg(not(target_os = "wasi"))]
    fn p95_ms(samples: &[u128]) -> u128 {
        let mut s = samples.to_vec();
        s.sort_unstable();
        let n = s.len();
        assert!(n > 0, "samples must be non-empty");
        let rank = (95 * n).div_ceil(100);
        s[rank.saturating_sub(1)]
    }

    #[cfg(not(target_os = "wasi"))]
    #[test]
    fn persistent_stdio_transport_reduces_bridge_p95_latency_vs_spawn_per_op() {
        super::reset_persistent_bridge_sessions_for_tests();
        let td = tempfile::tempdir().expect("tempdir");
        let bridge = td.path().join("persistent_bridge.sh");
        write_persistent_bridge_script(&bridge);
        let base_dir = td.path().display().to_string();

        let spawn_policy = CapsPolicy::from_toml_str(&format!(
            r#"
allow = ["gpu/compute::limits"]
[op."gpu/compute::limits"]
base_dir = "{base_dir}"
bridge_cmd = "persistent_bridge.sh"
bridge_transport = "spawn-per-op"
max_bytes = 4096
"#
        ))
        .expect("spawn policy");

        let persistent_policy = CapsPolicy::from_toml_str(&format!(
            r#"
allow = ["gpu/compute::limits"]
[op."gpu/compute::limits"]
base_dir = "{base_dir}"
bridge_cmd = "persistent_bridge.sh"
bridge_transport = "persistent-stdio"
max_bytes = 4096
"#
        ))
        .expect("persistent policy");

        let mut spawn_samples = Vec::new();
        for _ in 0..12 {
            let t0 = Instant::now();
            let _ = call_host_bridge(
                "gpu",
                "gpu/compute::limits",
                &Term::Nil,
                spawn_policy.op_policy("gpu/compute::limits"),
            )
            .expect("spawn transport call");
            spawn_samples.push(t0.elapsed().as_millis());
        }

        let mut persistent_samples = Vec::new();
        for _ in 0..12 {
            let t0 = Instant::now();
            let _ = call_host_bridge(
                "gpu",
                "gpu/compute::limits",
                &Term::Nil,
                persistent_policy.op_policy("gpu/compute::limits"),
            )
            .expect("persistent transport call");
            persistent_samples.push(t0.elapsed().as_millis());
        }

        let spawn_p95 = p95_ms(&spawn_samples);
        let persistent_p95 = p95_ms(&persistent_samples);
        assert!(
            persistent_p95 + 10 < spawn_p95,
            "expected persistent p95 ({persistent_p95}ms) to beat spawn-per-op p95 ({spawn_p95}ms)"
        );
        super::reset_persistent_bridge_sessions_for_tests();
    }

    #[cfg(not(target_os = "wasi"))]
    #[test]
    fn rejects_invalid_bridge_transport_policy_value() {
        let td = tempfile::tempdir().expect("tempdir");
        let bridge = td.path().join("persistent_bridge.sh");
        write_persistent_bridge_script(&bridge);
        let base_dir = td.path().display().to_string();
        let policy = CapsPolicy::from_toml_str(&format!(
            r#"
allow = ["gpu/compute::limits"]
[op."gpu/compute::limits"]
base_dir = "{base_dir}"
bridge_cmd = "persistent_bridge.sh"
bridge_transport = "udp-magic"
max_bytes = 4096
"#
        ))
        .expect("parse caps policy");
        let err = call_host_bridge(
            "gpu",
            "gpu/compute::limits",
            &Term::Nil,
            policy.op_policy("gpu/compute::limits"),
        )
        .expect_err("invalid bridge_transport must fail");
        assert_eq!(err.code, "gpu/bridge-policy");
        assert!(err.message.contains("bridge_transport must be one of"));
    }
}
