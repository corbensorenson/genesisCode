static FORCE_WASI_REMOTE_PROFILE: AtomicBool = AtomicBool::new(false);

type SyncBytesResult = Result<Vec<u8>, gc_registry::RegistryError>;
type SyncHasResult = Result<BTreeMap<String, bool>, gc_registry::RegistryError>;
type SyncUploadResult = Result<(), String>;

pub(crate) fn set_force_wasi_remote_profile(enabled: bool) {
    FORCE_WASI_REMOTE_PROFILE.store(enabled, Ordering::Relaxed);
}

pub(super) struct SyncPolicy {
    pub(super) remote_allow: Vec<String>,
    pub(super) allow_http: bool,
    pub(super) wasi_network_profile: Option<String>,
    pub(super) auth_token: Option<String>,
    pub(super) auth_token_env: Option<String>,
    pub(super) basic_username: Option<String>,
    pub(super) basic_password: Option<String>,
    pub(super) basic_password_env: Option<String>,
    pub(super) mtls_ca_pem: Option<std::path::PathBuf>,
    pub(super) mtls_identity_pem: Option<std::path::PathBuf>,
    pub(super) transfer_workers: usize,
    pub(super) max_artifact_bytes: usize,
    pub(super) max_batch_bytes: usize,
}

fn parse_wasi_network_profile(pol: Option<&OpPolicy>) -> Result<Option<String>, String> {
    let Some(pol) = pol else {
        return Ok(None);
    };
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
    let mut auth_token: Option<String> = None;
    let mut auth_token_env: Option<String> = None;
    let mut basic_username: Option<String> = None;
    let mut basic_password: Option<String> = None;
    let mut basic_password_env: Option<String> = None;
    let mut mtls_ca_pem: Option<std::path::PathBuf> = None;
    let mut mtls_identity_pem: Option<std::path::PathBuf> = None;
    let mut transfer_workers: usize = 4;
    let mut max_artifact_bytes: usize = HARD_REMOTE_ARTIFACT_MAX_BYTES;
    let mut max_batch_bytes: usize = HARD_SYNC_PULL_BATCH_MAX_BYTES;
    if let Some(pol) = pol {
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
        if let Some(v) = pol.extra.get("auth_token")
            && let Some(s) = v.as_str()
        {
            auth_token = Some(s.to_string());
        }
        if let Some(v) = pol.extra.get("auth_token_env")
            && let Some(s) = v.as_str()
        {
            auth_token_env = Some(s.to_string());
        }
        if let Some(v) = pol.extra.get("basic_username")
            && let Some(s) = v.as_str()
        {
            basic_username = Some(s.to_string());
        }
        if let Some(v) = pol.extra.get("basic_password")
            && let Some(s) = v.as_str()
        {
            basic_password = Some(s.to_string());
        }
        if let Some(v) = pol.extra.get("basic_password_env")
            && let Some(s) = v.as_str()
        {
            basic_password_env = Some(s.to_string());
        }
        if let Some(v) = pol.extra.get("mtls_ca_pem")
            && let Some(s) = v.as_str()
        {
            mtls_ca_pem = Some(std::path::PathBuf::from(s));
        }
        if let Some(v) = pol.extra.get("mtls_identity_pem")
            && let Some(s) = v.as_str()
        {
            mtls_identity_pem = Some(std::path::PathBuf::from(s));
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
        auth_token,
        auth_token_env,
        basic_username,
        basic_password,
        basic_password_env,
        mtls_ca_pem,
        mtls_identity_pem,
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
    if base.scheme() == "http" && !policy.store.allow_http {
        return Err("http remotes are disabled by policy (set store.allow_http=true)".to_string());
    }
    if policy.store.remote_allow.is_empty() {
        return Err("store remote requires store.remote_allow allowlist in caps.toml".to_string());
    }
    for p in &policy.store.remote_allow {
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
    Err("store remote is not in policy store.remote_allow allowlist".to_string())
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

pub(super) fn store_registry_auth(
    policy: &CapsPolicy,
) -> Result<gc_registry::RegistryAuth, String> {
    let bearer_token = resolve_auth_token(
        policy.store.auth_token.as_deref(),
        policy.store.auth_token_env.as_deref(),
    )?;
    let basic_password = resolve_basic_password(
        policy.store.basic_password.as_deref(),
        policy.store.basic_password_env.as_deref(),
    )?;
    let basic_username = policy.store.basic_username.clone();
    if bearer_token.is_some() && basic_username.is_some() {
        return Err(
            "auth_token/auth_token_env and basic_username are mutually exclusive".to_string(),
        );
    }
    if basic_username.is_none() && basic_password.is_some() {
        return Err("basic_password/basic_password_env requires basic_username".to_string());
    }
    let basic_password = if basic_username.is_some() {
        Some(basic_password.unwrap_or_default())
    } else {
        None
    };
    let mtls_ca_pem = match policy.store.mtls_ca_pem.as_deref() {
        Some(path) => Some(read_pem_path(path)?),
        None => None,
    };
    let mtls_identity_pem = match policy.store.mtls_identity_pem.as_deref() {
        Some(path) => Some(read_pem_path(path)?),
        None => None,
    };
    Ok(gc_registry::RegistryAuth {
        bearer_token,
        basic_username,
        basic_password,
        mtls_ca_pem,
        mtls_identity_pem,
    })
}

pub(super) fn sync_registry_auth(sp: &SyncPolicy) -> Result<gc_registry::RegistryAuth, String> {
    let bearer_token = resolve_auth_token(sp.auth_token.as_deref(), sp.auth_token_env.as_deref())?;
    let basic_password = resolve_basic_password(
        sp.basic_password.as_deref(),
        sp.basic_password_env.as_deref(),
    )?;
    let basic_username = sp.basic_username.clone();
    if bearer_token.is_some() && basic_username.is_some() {
        return Err(
            "auth_token/auth_token_env and basic_username are mutually exclusive".to_string(),
        );
    }
    if basic_username.is_none() && basic_password.is_some() {
        return Err("basic_password/basic_password_env requires basic_username".to_string());
    }
    let basic_password = if basic_username.is_some() {
        Some(basic_password.unwrap_or_default())
    } else {
        None
    };
    let mtls_ca_pem = match sp.mtls_ca_pem.as_deref() {
        Some(path) => Some(read_pem_path(path)?),
        None => None,
    };
    let mtls_identity_pem = match sp.mtls_identity_pem.as_deref() {
        Some(path) => Some(read_pem_path(path)?),
        None => None,
    };
    Ok(gc_registry::RegistryAuth {
        bearer_token,
        basic_username,
        basic_password,
        mtls_ca_pem,
        mtls_identity_pem,
    })
}

pub(super) fn store_remote_client(
    policy: &CapsPolicy,
    op_pol: Option<&OpPolicy>,
    timeout_ms: Option<u64>,
    error_tok: SealId,
    op: &str,
) -> Result<Option<(gc_registry::RegistryClient, String)>, Value> {
    let Some(remote) = &policy.store.remote else {
        return Ok(None);
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
