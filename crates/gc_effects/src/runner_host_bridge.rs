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
#[path = "runner_host_bridge_persistent.rs"]
mod runner_host_bridge_persistent;

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
        BridgeTransport::PersistentStdio => {
            runner_host_bridge_persistent::run_bridge_process_persistent(
                family, op, payload, &base_dir, &cmd_path, &args, timeout_ms, max_bytes,
            )
        }
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
#[path = "runner_host_bridge_tests.rs"]
mod tests;
