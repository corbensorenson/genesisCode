static FORCE_WASI_REMOTE_PROFILE: AtomicBool = AtomicBool::new(false);

use crate::policy::{
    AuthorizedOptionalBool, AuthorizedOptionalString, AuthorizedStoreCredentials,
    AuthorizedStringList,
};

#[path = "policy_auth_store.rs"]
mod policy_auth_store;
pub(super) use policy_auth_store::store_registry_auth;

type SyncBytesResult = Result<Vec<u8>, gc_registry::RegistryError>;
type SyncHasResult = Result<BTreeMap<String, bool>, gc_registry::RegistryError>;
type SyncUploadResult = Result<(), String>;

pub(crate) fn set_force_wasi_remote_profile(enabled: bool) {
    FORCE_WASI_REMOTE_PROFILE.store(enabled, Ordering::Relaxed);
}

#[derive(Debug)]
pub(super) struct SyncPolicy {
    pub(super) remote_allow: Vec<String>,
    pub(super) allow_http: bool,
    pub(super) wasi_network_profile: Option<String>,
    pub(super) credentials: AuthorizedStoreCredentials,
    pub(super) transfer_workers: usize,
    pub(super) max_artifact_bytes: usize,
    pub(super) max_batch_bytes: usize,
}

fn parse_wasi_network_profile(pol: Option<&OpPolicy>) -> Result<Option<String>, String> {
    let Some(pol) = pol else {
        return Ok(None);
    };
    if let Some(authorized) = &pol.authorized_network {
        return match &authorized.wasi_network_profile {
            AuthorizedOptionalString::Absent => Ok(None),
            AuthorizedOptionalString::InvalidType => {
                Err("wasi_network_profile must be a string".to_string())
            }
            AuthorizedOptionalString::Empty => {
                Err("wasi_network_profile must not be empty".to_string())
            }
            AuthorizedOptionalString::Valid(value) => Ok(Some(value.clone())),
        };
    }
    let Some(v) = pol.extra.get("wasi_network_profile") else {
        return Ok(None);
    };
    let s = v
        .as_str()
        .ok_or_else(|| "wasi_network_profile must be a string".to_string())?
        .trim()
        .to_string();
    if s.is_empty() {
        return Err("wasi_network_profile must not be empty".to_string());
    }
    Ok(Some(s))
}

fn validate_wasi_remote_profile(
    profile: Option<&str>,
    scheme: &str,
    capability_scope: &str,
) -> Result<(), String> {
    let enforce = cfg!(target_os = "wasi") || FORCE_WASI_REMOTE_PROFILE.load(Ordering::Relaxed);
    if !enforce {
        return Ok(());
    }
    let profile = profile.unwrap_or("none");
    match profile {
        "none" => Err(format!(
            "WASI remote {capability_scope} access is disabled; set wasi_network_profile to `local` or `preview2` in caps.toml op policy"
        )),
        "local" => {
            if matches!(scheme, "file" | "inproc")
                || (matches!(scheme, "http" | "https")
                    && gc_registry::wasi_http_bridge_configured())
            {
                Ok(())
            } else {
                Err(format!(
                    "wasi_network_profile=local only allows file:// or inproc:// remotes (got scheme `{scheme}`)"
                ))
            }
        }
        "preview2" => Ok(()),
        other => Err(format!(
            "invalid wasi_network_profile `{other}`; expected `none`, `local`, or `preview2`"
        )),
    }
}

pub(super) fn sync_policy_from_op(pol: Option<&OpPolicy>) -> Result<SyncPolicy, String> {
    let mut remote_allow: Vec<String> = Vec::new();
    let mut allow_http = false;
    let wasi_network_profile: Option<String> = parse_wasi_network_profile(pol)?;
    let credentials = pol
        .and_then(|policy| policy.authorized_sync_credentials.clone())
        .ok_or_else(|| "per-operation sync credential policy authority is missing".to_string())?;
    let mut transfer_workers: usize = 4;
    let mut max_artifact_bytes: usize = HARD_REMOTE_ARTIFACT_MAX_BYTES;
    let mut max_batch_bytes: usize = HARD_SYNC_PULL_BATCH_MAX_BYTES;
    if let Some(pol) = pol {
        if let Some(authorized) = &pol.authorized_network {
            match &authorized.remote_allow {
                AuthorizedStringList::Absent
                | AuthorizedStringList::InvalidType
                | AuthorizedStringList::Empty => {}
                AuthorizedStringList::InvalidEntry => {
                    return Err("remote_allow entries must be strings".to_string());
                }
                AuthorizedStringList::Valid(values) => remote_allow.clone_from(values),
            }
            allow_http = match &authorized.allow_http {
                AuthorizedOptionalBool::Valid(value) => *value,
                AuthorizedOptionalBool::Absent | AuthorizedOptionalBool::InvalidType => false,
            };
        } else {
            if let Some(v) = pol.extra.get("remote_allow")
                && let Some(arr) = v.as_array()
            {
                for x in arr {
                    let s = x
                        .as_str()
                        .ok_or_else(|| "remote_allow entries must be strings".to_string())?;
                    let t = s.trim();
                    if !t.is_empty() {
                        remote_allow.push(t.to_string());
                    }
                }
            }
            if let Some(v) = pol.extra.get("allow_http")
                && let Some(b) = v.as_bool()
            {
                allow_http = b;
            }
        }
        if let Some(v) = pol.extra.get("transfer_workers")
            && let Some(n) = v.as_integer()
            && n > 0
            && let Ok(nn) = usize::try_from(n)
        {
            transfer_workers = nn.clamp(1, 64);
        }
        if let Some(v) = pol.extra.get("max_artifact_bytes") {
            let n = v
                .as_integer()
                .ok_or_else(|| "max_artifact_bytes must be a positive integer".to_string())?;
            if n <= 0 {
                return Err("max_artifact_bytes must be > 0".to_string());
            }
            let nn = usize::try_from(n)
                .map_err(|_| "max_artifact_bytes is too large for this platform".to_string())?;
            max_artifact_bytes = nn.min(HARD_REMOTE_ARTIFACT_MAX_BYTES);
        }
        if let Some(v) = pol.extra.get("max_batch_bytes") {
            let n = v
                .as_integer()
                .ok_or_else(|| "max_batch_bytes must be a positive integer".to_string())?;
            if n <= 0 {
                return Err("max_batch_bytes must be > 0".to_string());
            }
            let nn = usize::try_from(n)
                .map_err(|_| "max_batch_bytes is too large for this platform".to_string())?;
            max_batch_bytes = nn.min(HARD_SYNC_PULL_BATCH_MAX_BYTES);
        }
    }
    if remote_allow.is_empty() {
        return Err("sync requires per-op remote_allow allowlist in caps.toml".to_string());
    }
    if max_batch_bytes < max_artifact_bytes {
        max_batch_bytes = max_artifact_bytes;
    }
    Ok(SyncPolicy {
        remote_allow,
        allow_http,
        wasi_network_profile,
        credentials,
        transfer_workers,
        max_artifact_bytes,
        max_batch_bytes,
    })
}

pub(super) fn sync_normalize_and_check_remote(
    sp: &SyncPolicy,
    remote: &str,
) -> Result<String, String> {
    let base = gc_registry::normalize_remote_base(remote).map_err(|e| format!("{e}"))?;
    let base_s = base.as_str().to_string();
    validate_wasi_remote_profile(sp.wasi_network_profile.as_deref(), base.scheme(), "sync")?;
    if base.scheme() == "http" && !sp.allow_http {
        return Err("http remotes are disabled by policy (set allow_http=true)".to_string());
    }
    for p in &sp.remote_allow {
        let t = p.trim();
        if t.ends_with("://") {
            if base.scheme() == t.trim_end_matches("://") {
                return Ok(base_s.clone());
            }
            continue;
        }
        if remote_allow_matches(&base_s, t).map_err(|e| format!("bad remote_allow: {e}"))? {
            return Ok(base_s.clone());
        }
    }
    Err("remote is not in policy remote_allow allowlist".to_string())
}

pub(super) fn store_normalize_and_check_remote(
    policy: &CapsPolicy,
    op_pol: Option<&OpPolicy>,
    remote: &str,
) -> Result<String, String> {
    let base = gc_registry::normalize_remote_base(remote).map_err(|e| format!("{e}"))?;
    let wasi_profile = parse_wasi_network_profile(op_pol)?;
    validate_wasi_remote_profile(wasi_profile.as_deref(), base.scheme(), "store")?;
    let base_s = base.as_str().to_string();
    let authorized = policy
        .authorized_store_remote()
        .ok_or_else(|| "global store remote policy authority is missing".to_string())?;
    let allow_http = match &authorized.allow_http {
        AuthorizedOptionalBool::Absent => false,
        AuthorizedOptionalBool::InvalidType => {
            return Err("store.allow_http must be a boolean".to_string());
        }
        AuthorizedOptionalBool::Valid(value) => *value,
    };
    if base.scheme() == "http" && !allow_http {
        return Err("http remotes are disabled by policy (set store.allow_http=true)".to_string());
    }
    let remote_allow = match &authorized.remote_allow {
        AuthorizedStringList::Valid(values) => values,
        AuthorizedStringList::InvalidType => {
            return Err("store.remote_allow must be an array".to_string());
        }
        AuthorizedStringList::InvalidEntry => {
            return Err("store.remote_allow entries must be strings".to_string());
        }
        AuthorizedStringList::Absent | AuthorizedStringList::Empty => {
            return Err("store remote requires store.remote_allow allowlist in caps.toml".to_string());
        }
    };
    for t in remote_allow {
        if t.ends_with("://") {
            if base.scheme() == t.trim_end_matches("://") {
                return Ok(base_s.clone());
            }
            continue;
        }
        if remote_allow_matches(&base_s, t).map_err(|e| format!("bad remote_allow: {e}"))? {
            return Ok(base_s.clone());
        }
    }
    Err("store remote is not in policy store.remote_allow allowlist".to_string())
}

pub(super) fn store_remote_from_policy(policy: &CapsPolicy) -> Result<Option<&str>, String> {
    let authorized = policy
        .authorized_store_remote()
        .ok_or_else(|| "global store remote policy authority is missing".to_string())?;
    match &authorized.remote {
        AuthorizedOptionalString::Absent => Ok(None),
        AuthorizedOptionalString::InvalidType => Err("store.remote must be a string".to_string()),
        AuthorizedOptionalString::Empty => Err("store.remote must not be empty".to_string()),
        AuthorizedOptionalString::Valid(value) => Ok(Some(value)),
    }
}

pub(super) fn remote_allow_matches(
    base: &str,
    allow: &str,
) -> Result<bool, gc_registry::RegistryError> {
    let base = gc_registry::normalize_remote_base(base)?;
    let allow = gc_registry::normalize_remote_base(allow)?;
    if base.scheme() != allow.scheme() {
        return Ok(false);
    }
    if base.host_str() != allow.host_str() {
        return Ok(false);
    }
    if base.port_or_known_default() != allow.port_or_known_default() {
        return Ok(false);
    }
    let base_path = ensure_trailing_slash(base.path());
    let allow_path = ensure_trailing_slash(allow.path());
    Ok(base_path.starts_with(&allow_path))
}

fn ensure_trailing_slash(path: &str) -> String {
    if path.ends_with('/') {
        path.to_string()
    } else {
        format!("{path}/")
    }
}

pub(super) fn registry_error_code(
    err: &gc_registry::RegistryError,
    auth_code: &'static str,
) -> &'static str {
    match err {
        gc_registry::RegistryError::Auth(_) => auth_code,
        _ => "core/sync/remote-error",
    }
}

fn resolve_auth_token(
    inline: Option<&str>,
    env_name: Option<&str>,
) -> Result<Option<String>, String> {
    if inline.is_some() && env_name.is_some() {
        return Err("auth_token and auth_token_env are mutually exclusive".to_string());
    }
    if let Some(token) = inline {
        return Ok(Some(token.to_string()));
    }
    if let Some(name) = env_name {
        let v = std::env::var(name)
            .map_err(|_| format!("auth_token_env `{name}` is not set in environment"))?;
        if v.trim().is_empty() {
            return Err(format!(
                "auth_token_env `{name}` resolved to an empty token"
            ));
        }
        return Ok(Some(v));
    }
    Ok(None)
}

fn resolve_basic_password(
    inline: Option<&str>,
    env_name: Option<&str>,
) -> Result<Option<String>, String> {
    if inline.is_some() && env_name.is_some() {
        return Err("basic_password and basic_password_env are mutually exclusive".to_string());
    }
    if let Some(password) = inline {
        return Ok(Some(password.to_string()));
    }
    if let Some(name) = env_name {
        let v = std::env::var(name)
            .map_err(|_| format!("basic_password_env `{name}` is not set in environment"))?;
        return Ok(Some(v));
    }
    Ok(None)
}

fn read_pem_path(path: &std::path::Path) -> Result<Vec<u8>, String> {
    std::fs::read(path).map_err(|e| format!("failed reading PEM `{}`: {e}", path.display()))
}

pub(super) fn sync_registry_auth(sp: &SyncPolicy) -> Result<gc_registry::RegistryAuth, String> {
    policy_auth_store::registry_auth_from_authority(&sp.credentials, "")
}

pub(super) fn store_remote_client(
    policy: &CapsPolicy,
    op_pol: Option<&OpPolicy>,
    timeout_ms: Option<u64>,
    error_tok: SealId,
    op: &str,
) -> Result<Option<(gc_registry::RegistryClient, String)>, Value> {
    let remote = match store_remote_from_policy(policy) {
        Ok(Some(remote)) => remote,
        Ok(None) => return Ok(None),
        Err(error) => {
            return Err(mk_error(
                error_tok,
                "core/caps/policy-error",
                error,
                Some(op),
            ));
        }
    };
    let base = match store_normalize_and_check_remote(policy, op_pol, remote) {
        Ok(b) => b,
        Err(e) => {
            return Err(mk_error(error_tok, "core/store/remote-denied", e, Some(op)));
        }
    };
    let auth = match store_registry_auth(policy) {
        Ok(a) => a,
        Err(e) => {
            return Err(mk_error(error_tok, "core/caps/policy-error", e, Some(op)));
        }
    };
    let client = match gc_registry::RegistryClient::new_with_auth(
        &base,
        timeout_ms.map(std::time::Duration::from_millis),
        auth,
    ) {
        Ok(c) => c,
        Err(e) => {
            let code = match &e {
                gc_registry::RegistryError::Auth(_) => "core/store/remote-auth",
                _ => "core/store/remote-error",
            };
            return Err(mk_error(error_tok, code, format!("{e}"), Some(op)));
        }
    };
    Ok(Some((client, base)))
}

#[cfg(test)]
mod network_authority_tests {
    use toml::Value as TomlValue;

    use super::*;
    use crate::policy::{
        AuthorizedBindPorts, AuthorizedMaxBytes, AuthorizedNetworkPolicy,
        AuthorizedSecretSource, AuthorizedStoreCredentialError,
    };

    fn make_policy(network: AuthorizedNetworkPolicy) -> OpPolicy {
        OpPolicy {
            base_dir: None,
            create_dirs: false,
            timeout_ms: None,
            log_inline_max_bytes: None,
            extra: BTreeMap::from([
                (
                    "remote_allow".to_string(),
                    TomlValue::String("invalid raw fallback".to_string()),
                ),
                (
                    "allow_http".to_string(),
                    TomlValue::String("invalid raw fallback".to_string()),
                ),
                (
                    "wasi_network_profile".to_string(),
                    TomlValue::Integer(7),
                ),
            ]),
            authorized_cap: None,
            authorized_max_bytes: None,
            authorized_process_programs: None,
            authorized_database: None,
            authorized_network: Some(network),
            authorized_crypto: None,
            authorized_gpu: None,
            authorized_gfx_profile: None,
            authorized_xr_policy: None,
            authorized_bridge_identity: None,
            authorized_plugin: None,
            authorized_ffi: None,
            authorized_sync_credentials: Some(AuthorizedStoreCredentials::Valid {
                bearer: AuthorizedSecretSource::Absent,
                basic_username: None,
                basic_password: AuthorizedSecretSource::Absent,
                mtls_ca_pem: None,
                mtls_identity_pem: None,
            }),
        }
    }

    fn base_network() -> AuthorizedNetworkPolicy {
        AuthorizedNetworkPolicy {
            url_allow: AuthorizedStringList::Absent,
            remote_allow: AuthorizedStringList::Absent,
            allow_http: AuthorizedOptionalBool::Absent,
            wasi_network_profile: AuthorizedOptionalString::Absent,
            bind_hosts: AuthorizedStringList::Absent,
            bind_ports: AuthorizedBindPorts::Absent,
            max_request_bytes: AuthorizedMaxBytes::Absent,
        }
    }

    #[test]
    fn remote_dispatch_consumes_authorized_network_policy_before_raw_policy() {
        let mut network = base_network();
        network.remote_allow =
            AuthorizedStringList::Valid(vec!["https://safe.example/v1/".to_string()]);
        network.allow_http = AuthorizedOptionalBool::Valid(false);
        network.wasi_network_profile = AuthorizedOptionalString::Valid("preview2".to_string());
        let policy = make_policy(network);
        let selected = sync_policy_from_op(Some(&policy)).unwrap();
        assert_eq!(selected.remote_allow, vec!["https://safe.example/v1/"]);
        assert!(!selected.allow_http);
        assert_eq!(selected.wasi_network_profile.as_deref(), Some("preview2"));
    }

    #[test]
    fn remote_dispatch_preserves_authorized_network_policy_errors() {
        let mut network = base_network();
        network.remote_allow = AuthorizedStringList::InvalidEntry;
        let policy = make_policy(network);
        assert_eq!(
            sync_policy_from_op(Some(&policy)).unwrap_err(),
            "remote_allow entries must be strings"
        );

        let mut network = base_network();
        network.wasi_network_profile = AuthorizedOptionalString::InvalidType;
        let policy = make_policy(network);
        assert_eq!(
            parse_wasi_network_profile(Some(&policy)).unwrap_err(),
            "wasi_network_profile must be a string"
        );
    }

    #[test]
    fn sync_auth_consumes_authority_before_poisoned_raw_fields() {
        let mut network = base_network();
        network.remote_allow =
            AuthorizedStringList::Valid(vec!["https://safe.example/v1/".to_string()]);
        let mut policy = make_policy(network);
        policy.extra.insert(
            "auth_token_env".to_string(),
            TomlValue::String("POISONED_ENV".to_string()),
        );
        policy.extra.insert(
            "basic_username".to_string(),
            TomlValue::String("poisoned-user".to_string()),
        );
        policy.authorized_sync_credentials = Some(AuthorizedStoreCredentials::Valid {
            bearer: AuthorizedSecretSource::Inline("authorized-secret".to_string()),
            basic_username: None,
            basic_password: AuthorizedSecretSource::Absent,
            mtls_ca_pem: None,
            mtls_identity_pem: None,
        });

        let selected = sync_policy_from_op(Some(&policy)).unwrap();
        let auth = sync_registry_auth(&selected).unwrap();
        assert_eq!(auth.bearer_token.as_deref(), Some("authorized-secret"));
        assert_eq!(auth.basic_username, None);
    }

    #[test]
    fn sync_auth_fails_closed_without_or_with_rejected_authority() {
        let mut network = base_network();
        network.remote_allow =
            AuthorizedStringList::Valid(vec!["https://safe.example/v1/".to_string()]);
        let mut policy = make_policy(network);
        policy.authorized_sync_credentials = None;
        assert_eq!(
            sync_policy_from_op(Some(&policy)).unwrap_err(),
            "per-operation sync credential policy authority is missing"
        );

        policy.authorized_sync_credentials = Some(AuthorizedStoreCredentials::Invalid(
            AuthorizedStoreCredentialError::AuthTokenInvalidType,
        ));
        let selected = sync_policy_from_op(Some(&policy)).unwrap();
        assert_eq!(
            sync_registry_auth(&selected).unwrap_err(),
            "auth_token must be a string"
        );
    }

    #[test]
    fn store_remote_dispatch_consumes_authority_before_raw_fields() {
        let mut policy = CapsPolicy::from_toml_str(
            r#"
[store]
remote = "https://safe.example/v1/"
remote_allow = ["https://safe.example/v1/"]
allow_http = false
"#,
        )
        .unwrap();
        policy.store.remote = Some("http://raw-unsafe.example/".to_string());
        policy.store.remote_allow = vec!["http://raw-unsafe.example/".to_string()];
        policy.store.allow_http = true;

        assert_eq!(
            store_remote_from_policy(&policy).unwrap(),
            Some("https://safe.example/v1/")
        );
        assert_eq!(
            store_normalize_and_check_remote(
                &policy,
                None,
                "https://safe.example/v1/objects"
            )
            .unwrap(),
            "https://safe.example/v1/objects/v1/"
        );
        assert!(
            store_normalize_and_check_remote(&policy, None, "http://raw-unsafe.example/")
                .is_err()
        );
    }

    #[test]
    fn store_remote_dispatch_preserves_authorized_type_errors() {
        let policy = CapsPolicy::from_toml_str(
            r#"
[store]
remote = 7
remote_allow = "https://safe.example/v1/"
allow_http = "yes"
"#,
        )
        .unwrap();
        assert_eq!(
            store_remote_from_policy(&policy).unwrap_err(),
            "store.remote must be a string"
        );
        assert_eq!(
            store_normalize_and_check_remote(&policy, None, "https://safe.example/v1/")
                .unwrap_err(),
            "store.allow_http must be a boolean"
        );
    }

    #[test]
    fn store_auth_consumes_authorized_credentials_before_poisoned_raw_fields() {
        let mut policy = CapsPolicy::from_toml_str(
            r#"
[store]
auth_token = "authorized-secret"
"#,
        )
        .unwrap();
        policy.store.auth_token = Some("poisoned-raw-secret".to_string());
        policy.store.auth_token_env = Some("POISONED_ENV".to_string());
        policy.store.basic_username = Some("poisoned-user".to_string());
        policy.store.basic_password = Some("poisoned-password".to_string());

        let auth = store_registry_auth(&policy).unwrap();
        assert_eq!(auth.bearer_token.as_deref(), Some("authorized-secret"));
        assert_eq!(auth.basic_username, None);
        assert_eq!(auth.basic_password, None);
    }

    #[test]
    fn store_auth_fails_closed_without_credential_authority() {
        let mut policy = CapsPolicy::empty();
        policy.store.authorized_credentials = None;
        assert_eq!(
            store_registry_auth(&policy).unwrap_err(),
            "global store credential policy authority is missing"
        );
    }
}
