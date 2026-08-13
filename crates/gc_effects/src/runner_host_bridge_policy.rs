#[cfg(not(target_os = "wasi"))]
use sha2::{Digest, Sha256};

use super::*;
use crate::policy::{
    AuthorizedBridgeAllowlist, AuthorizedBridgeDigest, AuthorizedBridgeIdentityPolicy,
    AuthorizedBridgeTransport,
};

#[cfg(not(target_os = "wasi"))]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum BridgeTransport {
    SpawnPerOp,
    PersistentStdio,
}

fn bridge_authority<'a>(
    pol: Option<&'a OpPolicy>,
    family: &str,
) -> Result<Option<&'a AuthorizedBridgeIdentityPolicy>, BridgeError> {
    let Some(policy) = pol else {
        return Ok(None);
    };
    policy
        .authorized_bridge_identity
        .as_ref()
        .map(Some)
        .ok_or_else(|| BridgeError {
            code: format!("{family}/bridge-policy"),
            message: "bridge identity authority state is unavailable".to_string(),
        })
}

pub(crate) fn bridge_profile_active(pol: Option<&OpPolicy>) -> bool {
    let Some(policy) = pol else {
        return false;
    };
    policy
        .authorized_bridge_identity
        .as_ref()
        .map(|authority| authority.active)
        // A present policy without authority must enter the bridge path, where
        // the existing accessors return a sealed bridge-policy error.
        .unwrap_or(true)
}

pub(crate) fn wasi_bridge_profile_enabled(
    pol: Option<&OpPolicy>,
    family: &str,
) -> Result<bool, BridgeError> {
    if cfg!(target_os = "wasi") {
        return Ok(true);
    }
    Ok(bridge_authority(pol, family)?.is_some_and(|authority| authority.wasi_profile))
}

#[cfg(not(target_os = "wasi"))]
pub(crate) fn bridge_cmd(
    pol: Option<&OpPolicy>,
    family: &str,
) -> Result<Option<String>, BridgeError> {
    Ok(bridge_authority(pol, family)?.and_then(|authority| authority.command.clone()))
}

#[cfg(not(target_os = "wasi"))]
pub(crate) fn bridge_args(
    pol: Option<&OpPolicy>,
    family: &str,
) -> Result<Vec<String>, BridgeError> {
    Ok(bridge_authority(pol, family)?
        .map(|authority| authority.args.clone())
        .unwrap_or_default())
}

#[cfg(not(target_os = "wasi"))]
pub(crate) fn bridge_transport(
    pol: Option<&OpPolicy>,
    family: &str,
) -> Result<BridgeTransport, BridgeError> {
    match bridge_authority(pol, family)?.map(|authority| &authority.transport) {
        None | Some(AuthorizedBridgeTransport::SpawnPerOp) => Ok(BridgeTransport::SpawnPerOp),
        Some(AuthorizedBridgeTransport::PersistentStdio) => Ok(BridgeTransport::PersistentStdio),
        Some(AuthorizedBridgeTransport::Invalid(value)) => Err(BridgeError {
            code: format!("{family}/bridge-policy"),
            message: format!(
                "bridge_transport must be one of: spawn-per-op, persistent-stdio (got `{value}`)"
            ),
        }),
    }
}

pub(crate) fn bridge_digest_pin_is_missing(pol: Option<&OpPolicy>) -> Result<bool, String> {
    let authority = pol
        .and_then(|policy| policy.authorized_bridge_identity.as_ref())
        .ok_or_else(|| "bridge identity authority state is unavailable".to_string())?;
    Ok(authority.pin_required
        && matches!(
            authority.digest,
            AuthorizedBridgeDigest::Absent
                | AuthorizedBridgeDigest::InvalidType
                | AuthorizedBridgeDigest::Empty
        ))
}

#[cfg(not(target_os = "wasi"))]
fn bridge_cmd_allowlist(
    pol: Option<&OpPolicy>,
    family: &str,
) -> Result<Option<Vec<String>>, BridgeError> {
    let authority = pol
        .and_then(|policy| policy.authorized_bridge_identity.as_ref())
        .ok_or_else(|| BridgeError {
            code: format!("{family}/bridge-policy"),
            message: "bridge identity authority state is unavailable".to_string(),
        })?;
    match &authority.allowlist {
        AuthorizedBridgeAllowlist::Absent => Ok(None),
        AuthorizedBridgeAllowlist::InvalidType => Err(BridgeError {
            code: format!("{family}/bridge-policy"),
            message: "bridge_cmd_allowlist must be an array of strings".to_string(),
        }),
        AuthorizedBridgeAllowlist::InvalidEntry => Err(BridgeError {
            code: format!("{family}/bridge-policy"),
            message: "bridge_cmd_allowlist must contain only strings".to_string(),
        }),
        AuthorizedBridgeAllowlist::EmptyEntry => Err(BridgeError {
            code: format!("{family}/bridge-policy"),
            message: "bridge_cmd_allowlist entries must be non-empty".to_string(),
        }),
        AuthorizedBridgeAllowlist::Valid(values) => Ok(Some(values.clone())),
    }
}

#[cfg(not(target_os = "wasi"))]
fn bridge_cmd_sha256(pol: Option<&OpPolicy>, family: &str) -> Result<Option<String>, BridgeError> {
    let authority = pol
        .and_then(|policy| policy.authorized_bridge_identity.as_ref())
        .ok_or_else(|| BridgeError {
            code: format!("{family}/bridge-policy"),
            message: "bridge identity authority state is unavailable".to_string(),
        })?;
    match &authority.digest {
        AuthorizedBridgeDigest::Absent | AuthorizedBridgeDigest::InvalidType => Ok(None),
        AuthorizedBridgeDigest::Empty | AuthorizedBridgeDigest::InvalidDigest => Err(BridgeError {
            code: format!("{family}/bridge-policy"),
            message:
                "bridge_cmd_sha256 must be a 64-hex digest (optionally prefixed with `sha256:`)"
                    .to_string(),
        }),
        AuthorizedBridgeDigest::Valid(hex) => Ok(Some(hex.clone())),
    }
}

#[cfg(not(target_os = "wasi"))]
fn bridge_cmd_matches_allowlist(
    cmd_raw: &str,
    cmd_path: &std::path::Path,
    allowlist: &[String],
) -> bool {
    let cmd_path_s = cmd_path.to_str();
    let cmd_name = cmd_path.file_name().and_then(|n| n.to_str());
    allowlist.iter().any(|allowed| {
        let token = allowed.trim();
        token == cmd_raw
            || cmd_path_s.is_some_and(|path| token == path)
            || cmd_name.is_some_and(|name| name == token)
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
    let Some(pol) = pol else {
        return Ok(None);
    };
    if let Some(authorized) = &pol.authorized_max_bytes {
        return match authorized {
            AuthorizedMaxBytes::Absent => Ok(None),
            AuthorizedMaxBytes::InvalidType => Err(BridgeError {
                code: format!("{family}/bridge-policy"),
                message: "max_bytes must be a positive integer".to_string(),
            }),
            AuthorizedMaxBytes::NonPositive => Err(BridgeError {
                code: format!("{family}/bridge-policy"),
                message: "max_bytes must be > 0".to_string(),
            }),
            AuthorizedMaxBytes::PlatformOverflow => Err(BridgeError {
                code: format!("{family}/bridge-policy"),
                message: "max_bytes is too large".to_string(),
            }),
            AuthorizedMaxBytes::Valid(limit) => Ok(Some(*limit)),
        };
    }
    let Some(v) = pol.extra.get("max_bytes") else {
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

#[cfg(test)]
mod authority_tests {
    use super::*;
    use crate::policy::{AuthorizedBridgeAllowlist, AuthorizedBridgeIdentityPolicy};
    use std::collections::BTreeMap;

    fn policy(digest: AuthorizedBridgeDigest, raw: &str) -> OpPolicy {
        OpPolicy {
            base_dir: None,
            create_dirs: false,
            timeout_ms: None,
            log_inline_max_bytes: None,
            extra: BTreeMap::from([(
                "bridge_cmd_sha256".to_string(),
                toml::Value::String(raw.to_string()),
            )]),
            authorized_cap: None,
            authorized_max_bytes: None,
            authorized_process_programs: None,
            authorized_database: None,
            authorized_network: None,
            authorized_crypto: None,
            authorized_gpu: None,
            authorized_gfx_profile: None,
            authorized_bridge_identity: Some(AuthorizedBridgeIdentityPolicy {
                active: false,
                allowlist: AuthorizedBridgeAllowlist::Absent,
                args: Vec::new(),
                command: None,
                pin_required: true,
                digest,
                transport: AuthorizedBridgeTransport::SpawnPerOp,
                wasi_profile: false,
            }),
            authorized_plugin: None,
            authorized_ffi: None,
        }
    }

    #[cfg(not(target_os = "wasi"))]
    #[test]
    fn bridge_identity_enforcement_consumes_authority_before_raw_policy() {
        let canonical = "a".repeat(64);
        let authorized = policy(
            AuthorizedBridgeDigest::Valid(canonical.clone()),
            "raw fallback must not be used",
        );
        assert_eq!(
            bridge_cmd_sha256(Some(&authorized), "host/ffi").unwrap(),
            Some(canonical)
        );

        let rejected = policy(AuthorizedBridgeDigest::InvalidDigest, &"b".repeat(64));
        assert!(bridge_cmd_sha256(Some(&rejected), "host/ffi").is_err());
    }

    #[cfg(not(target_os = "wasi"))]
    #[test]
    fn bridge_allowlist_enforcement_consumes_authority_before_raw_policy() {
        let mut authorized = policy(AuthorizedBridgeDigest::Absent, "unused");
        authorized.extra.insert(
            "bridge_cmd_allowlist".to_string(),
            toml::Value::String("raw malformed fallback".to_string()),
        );
        authorized
            .authorized_bridge_identity
            .as_mut()
            .expect("test authority")
            .allowlist = AuthorizedBridgeAllowlist::Valid(vec!["approved".to_string()]);
        assert_eq!(
            bridge_cmd_allowlist(Some(&authorized), "host/plugin").unwrap(),
            Some(vec!["approved".to_string()])
        );

        authorized
            .authorized_bridge_identity
            .as_mut()
            .expect("test authority")
            .allowlist = AuthorizedBridgeAllowlist::EmptyEntry;
        assert!(bridge_cmd_allowlist(Some(&authorized), "host/plugin").is_err());
    }

    #[cfg(not(target_os = "wasi"))]
    #[test]
    fn bridge_invocation_consumes_authority_before_raw_policy() {
        let mut authorized = policy(AuthorizedBridgeDigest::Absent, "unused");
        authorized.extra.extend([
            (
                "bridge_cmd".to_string(),
                toml::Value::String("raw-command".to_string()),
            ),
            (
                "bridge_args".to_string(),
                toml::Value::Array(vec![toml::Value::String("raw-arg".to_string())]),
            ),
            (
                "bridge_transport".to_string(),
                toml::Value::String("udp-magic".to_string()),
            ),
            (
                "wasi_bridge_profile".to_string(),
                toml::Value::Boolean(true),
            ),
        ]);
        let authority = authorized
            .authorized_bridge_identity
            .as_mut()
            .expect("test authority");
        authority.command = Some("authorized-command".to_string());
        authority.args = vec!["authorized-arg".to_string()];
        authority.transport = AuthorizedBridgeTransport::PersistentStdio;
        authority.wasi_profile = false;
        authority.active = false;

        assert_eq!(
            bridge_cmd(Some(&authorized), "host/plugin").unwrap(),
            Some("authorized-command".to_string())
        );
        assert_eq!(
            bridge_args(Some(&authorized), "host/plugin").unwrap(),
            vec!["authorized-arg".to_string()]
        );
        assert_eq!(
            bridge_transport(Some(&authorized), "host/plugin").unwrap(),
            BridgeTransport::PersistentStdio
        );
        assert!(!wasi_bridge_profile_enabled(Some(&authorized), "host/plugin").unwrap());
        assert!(!bridge_profile_active(Some(&authorized)));

        authorized
            .authorized_bridge_identity
            .as_mut()
            .expect("test authority")
            .active = true;
        assert!(bridge_profile_active(Some(&authorized)));
    }
}
