#[cfg(not(target_os = "wasi"))]
use sha2::{Digest, Sha256};

use super::*;

#[cfg(not(target_os = "wasi"))]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum BridgeTransport {
    SpawnPerOp,
    PersistentStdio,
}

pub(crate) fn wasi_bridge_profile_enabled(pol: Option<&OpPolicy>) -> bool {
    cfg!(target_os = "wasi")
        || pol
            .and_then(|p| p.extra.get("wasi_bridge_profile"))
            .and_then(|v| v.as_bool())
            .unwrap_or(false)
}

#[cfg(not(target_os = "wasi"))]
pub(crate) fn bridge_cmd(pol: Option<&OpPolicy>) -> Option<String> {
    pol.and_then(|p| p.extra.get("bridge_cmd"))
        .and_then(|v| v.as_str())
        .map(ToString::to_string)
}

#[cfg(not(target_os = "wasi"))]
pub(crate) fn bridge_args(pol: Option<&OpPolicy>) -> Vec<String> {
    pol.and_then(|p| p.extra.get("bridge_args"))
        .and_then(|v| v.as_array())
        .map(|arr| {
            arr.iter()
                .filter_map(|x| x.as_str().map(ToString::to_string))
                .collect::<Vec<_>>()
        })
        .unwrap_or_default()
}

#[cfg(not(target_os = "wasi"))]
pub(crate) fn bridge_transport(
    pol: Option<&OpPolicy>,
    family: &str,
) -> Result<BridgeTransport, BridgeError> {
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

#[cfg(not(target_os = "wasi"))]
pub(super) fn normalize_sha256_hex(raw: &str) -> Option<String> {
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

#[cfg(not(target_os = "wasi"))]
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

#[cfg(not(target_os = "wasi"))]
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

#[cfg(not(target_os = "wasi"))]
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

#[cfg(not(target_os = "wasi"))]
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

#[cfg(not(target_os = "wasi"))]
pub(crate) fn enforce_bridge_identity(
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

pub(crate) fn bridge_max_bytes(
    pol: Option<&OpPolicy>,
    family: &str,
) -> Result<Option<usize>, BridgeError> {
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

pub(crate) fn enforce_payload_limit(
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

pub(crate) fn enforce_response_limit(
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
