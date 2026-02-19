use super::*;

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

#[cfg(target_os = "wasi")]
fn validate_wasi_remote_profile(
    profile: Option<&str>,
    scheme: &str,
    capability_scope: &str,
) -> Result<(), String> {
    let profile = profile.unwrap_or("none");
    match profile {
        "none" => Err(format!(
            "WASI remote {capability_scope} access is disabled; set wasi_network_profile to `local` or `preview2` in caps.toml op policy"
        )),
        "local" => {
            if matches!(scheme, "file" | "inproc") {
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

#[cfg(not(target_os = "wasi"))]
fn validate_wasi_remote_profile(
    _profile: Option<&str>,
    _scheme: &str,
    _capability_scope: &str,
) -> Result<(), String> {
    Ok(())
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
        {
            if let Ok(nn) = usize::try_from(n) {
                transfer_workers = nn.clamp(1, 64);
            }
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

pub(super) struct SyncPullStats<'a> {
    pub(super) pulled: &'a mut u64,
    pub(super) already: &'a mut u64,
    pub(super) store_written_bytes: &'a mut usize,
    pub(super) store_max_run_bytes: Option<usize>,
    pub(super) error_tok: SealId,
    pub(super) op: &'a str,
    pub(super) transfer_workers: usize,
    pub(super) max_artifact_bytes: usize,
    pub(super) max_batch_bytes: usize,
}

pub(super) fn sync_pull_closure(
    client: &gc_registry::RegistryClient,
    store: &ArtifactStore,
    root: &str,
    depth: u64,
    stats: &mut SyncPullStats<'_>,
) -> Result<(), Value> {
    use std::collections::{HashSet, VecDeque};

    let mut q: VecDeque<(String, u64)> = VecDeque::new();
    q.push_back((root.to_string(), depth));
    let mut seen: HashSet<String> = HashSet::new();
    let mut obj_count: u64 = 0;
    let base_batch_cap = (stats.transfer_workers.max(1) * 8).max(8);
    let by_budget = (stats.max_batch_bytes / stats.max_artifact_bytes.max(1)).max(1);
    let batch_cap = base_batch_cap.min(by_budget);

    while !q.is_empty() {
        let mut batch: Vec<(String, u64)> = Vec::new();
        while batch.len() < batch_cap {
            let Some((h, dleft)) = q.pop_front() else {
                break;
            };
            if !seen.insert(h.clone()) {
                continue;
            }
            obj_count = obj_count.saturating_add(1);
            if obj_count > 50_000 {
                return Err(mk_error(
                    stats.error_tok,
                    "core/sync/too-many-objects",
                    "closure exceeded 50k objects".to_string(),
                    Some(stats.op),
                ));
            }
            batch.push((h, dleft));
        }
        if batch.is_empty() {
            continue;
        }

        let mut missing_hashes: Vec<String> = Vec::new();
        for (h, _) in &batch {
            if store.path_for(h).exists() {
                if store.verify_hex(h).is_err() {
                    return Err(mk_error(
                        stats.error_tok,
                        "core/store/corruption",
                        format!("artifact store corruption: {h}"),
                        Some(stats.op),
                    ));
                }
                *stats.already = stats.already.saturating_add(1);
            } else {
                missing_hashes.push(h.clone());
            }
        }

        if !missing_hashes.is_empty() {
            let dl_results = sync_parallel_store_get_bytes(
                client,
                &missing_hashes,
                stats.transfer_workers,
                stats.max_artifact_bytes,
                stats.max_batch_bytes,
            );
            for (i, h) in missing_hashes.iter().enumerate() {
                let bytes = match &dl_results[i] {
                    Ok(b) => b,
                    Err(e) => {
                        if let gc_registry::RegistryError::Protocol(msg) = e
                            && msg.contains("resource-limit:")
                        {
                            return Err(mk_error(
                                stats.error_tok,
                                "core/caps/resource-limit",
                                msg.split("resource-limit:")
                                    .nth(1)
                                    .unwrap_or(msg)
                                    .trim()
                                    .to_string(),
                                Some(stats.op),
                            ));
                        }
                        let code = registry_error_code(e, "core/sync/remote-auth");
                        return Err(mk_error(
                            stats.error_tok,
                            code,
                            format!("{e}"),
                            Some(stats.op),
                        ));
                    }
                };
                if let Some(limit) = stats.store_max_run_bytes {
                    let observed = (*stats.store_written_bytes).saturating_add(bytes.len());
                    if observed > limit {
                        return Err(mk_resource_limit_error(
                            stats.error_tok,
                            stats.op,
                            "store artifact bytes",
                            observed,
                            limit,
                        ));
                    }
                }
                let got = store.put_bytes(bytes).map_err(|e| {
                    mk_error(
                        stats.error_tok,
                        "core/store/io-error",
                        e.to_string(),
                        Some(stats.op),
                    )
                })?;
                if got != *h {
                    return Err(mk_error(
                        stats.error_tok,
                        "core/sync/hash-mismatch",
                        "remote bytes hash mismatch".to_string(),
                        Some(stats.op),
                    ));
                }
                *stats.store_written_bytes =
                    (*stats.store_written_bytes).saturating_add(bytes.len());
                *stats.pulled = stats.pulled.saturating_add(1);
            }
        }

        for (h, dleft) in batch {
            let t = match store_get_term(store, &h) {
                Ok(t) => t,
                Err(_) => continue,
            };

            // Commit closure: commit, base, patch, result snapshot, evidence, attestations, parents.
            if let Ok(c) = gc_vcs::Commit::from_term(&t) {
                if let Some(b) = c.base {
                    q.push_back((b, dleft));
                }
                q.push_back((c.patch, dleft));
                q.push_back((c.result, dleft));
                for x in c.evidence {
                    q.push_back((x, dleft));
                }
                for x in c.attestations {
                    q.push_back((x, dleft));
                }
                if dleft > 0 {
                    for p in c.parents {
                        q.push_back((p, dleft - 1));
                    }
                }
                continue;
            }

            // Patch closure: follow referenced values.
            if let Ok(p) = gc_vcs::Patch::from_term(&t) {
                for x in p.refs() {
                    q.push_back((x, dleft));
                }
                continue;
            }

            // Evidence closure: follow any referenced inputs/outputs/data.
            if let Ok(e) = gc_vcs::Evidence::from_term(&t) {
                for x in e.refs() {
                    q.push_back((x, dleft));
                }
                continue;
            }

            // Conflict closure: follow referenced snapshots and referenced handler/value hashes.
            if let Ok(c) = gc_vcs::Conflict::from_term(&t) {
                for x in c.refs() {
                    q.push_back((x, dleft));
                }
                continue;
            }

            // Snapshot closure: shallow refs.
            if let Ok(s) = gc_vcs::Snapshot::from_term(&t) {
                for x in s.shallow_refs() {
                    q.push_back((x, dleft));
                }
            }
        }
    }

    Ok(())
}

pub(super) fn sync_parallel_store_get_bytes(
    client: &gc_registry::RegistryClient,
    hashes: &[String],
    workers: usize,
    max_artifact_bytes: usize,
    max_batch_bytes: usize,
) -> Vec<Result<Vec<u8>, gc_registry::RegistryError>> {
    use std::sync::atomic::{AtomicUsize, Ordering};
    use std::sync::{Arc, Mutex};

    if hashes.is_empty() {
        return Vec::new();
    }
    let workers = workers.clamp(1, 64).min(hashes.len());
    if workers <= 1 {
        let mut total: usize = 0;
        return hashes
            .iter()
            .map(|h| {
                client
                    .store_get_bounded(h, Some(max_artifact_bytes))
                    .map_err(|e| e)
                    .and_then(|b| {
                        total = total.saturating_add(b.len());
                        if total > max_batch_bytes {
                            return Err(gc_registry::RegistryError::Protocol(format!(
                                "resource-limit: sync pull batch exceeded limit ({total} > {max_batch_bytes} bytes)"
                            )));
                        }
                        Ok(b)
                    })
            })
            .collect();
    }

    let next = Arc::new(AtomicUsize::new(0));
    let out: Arc<Mutex<Vec<Option<Result<Vec<u8>, gc_registry::RegistryError>>>>> =
        Arc::new(Mutex::new((0..hashes.len()).map(|_| None).collect()));
    std::thread::scope(|scope| {
        for _ in 0..workers {
            let out = Arc::clone(&out);
            let next = Arc::clone(&next);
            let c = client.clone();
            scope.spawn(move || {
                loop {
                    let i = next.fetch_add(1, Ordering::Relaxed);
                    if i >= hashes.len() {
                        break;
                    }
                    let res = c
                        .store_get_bounded(&hashes[i], Some(max_artifact_bytes))
                        .map_err(|e| e);
                    if let Ok(mut g) = out.lock() {
                        g[i] = Some(res);
                    } else {
                        return;
                    }
                }
            });
        }
    });
    let mut g = match out.lock() {
        Ok(g) => g,
        Err(_) => {
            return (0..hashes.len())
                .map(|_| {
                    Err(gc_registry::RegistryError::Protocol(
                        "sync get results lock poisoned".to_string(),
                    ))
                })
                .collect();
        }
    };
    let mut total: usize = 0;
    g.drain(..)
        .map(|x| {
            x.unwrap_or_else(|| {
                Err(gc_registry::RegistryError::Protocol(
                    "sync get worker produced no result".to_string(),
                ))
            })
                .and_then(|b| {
                    total = total.saturating_add(b.len());
                    if total > max_batch_bytes {
                        return Err(gc_registry::RegistryError::Protocol(format!(
                            "resource-limit: sync pull batch exceeded limit ({total} > {max_batch_bytes} bytes)"
                        )));
                    }
                    Ok(b)
                })
        })
        .collect()
}

pub(super) fn sync_parallel_store_has_chunks(
    client: &gc_registry::RegistryClient,
    chunks: &[Vec<String>],
    workers: usize,
) -> Vec<Result<BTreeMap<String, bool>, gc_registry::RegistryError>> {
    use std::sync::atomic::{AtomicUsize, Ordering};
    use std::sync::{Arc, Mutex};

    if chunks.is_empty() {
        return Vec::new();
    }
    let workers = workers.clamp(1, 64).min(chunks.len());
    if workers <= 1 {
        return chunks.iter().map(|chunk| client.store_has(chunk)).collect();
    }

    let next = Arc::new(AtomicUsize::new(0));
    let out: Arc<Mutex<Vec<Option<Result<BTreeMap<String, bool>, gc_registry::RegistryError>>>>> =
        Arc::new(Mutex::new((0..chunks.len()).map(|_| None).collect()));
    std::thread::scope(|scope| {
        for _ in 0..workers {
            let out = Arc::clone(&out);
            let next = Arc::clone(&next);
            let c = client.clone();
            scope.spawn(move || {
                loop {
                    let i = next.fetch_add(1, Ordering::Relaxed);
                    if i >= chunks.len() {
                        break;
                    }
                    let res = c.store_has(&chunks[i]);
                    if let Ok(mut g) = out.lock() {
                        g[i] = Some(res);
                    } else {
                        return;
                    }
                }
            });
        }
    });
    let mut g = match out.lock() {
        Ok(g) => g,
        Err(_) => {
            return (0..chunks.len())
                .map(|_| {
                    Err(gc_registry::RegistryError::Protocol(
                        "sync has results lock poisoned".to_string(),
                    ))
                })
                .collect();
        }
    };
    g.drain(..)
        .map(|x| {
            x.unwrap_or_else(|| {
                Err(gc_registry::RegistryError::Protocol(
                    "sync has worker produced no result".to_string(),
                ))
            })
        })
        .collect()
}

pub(super) fn sync_parallel_upload_missing(
    client: &gc_registry::RegistryClient,
    store: &ArtifactStore,
    missing: &[String],
    workers: usize,
) -> Vec<Result<(), String>> {
    use std::sync::atomic::{AtomicUsize, Ordering};
    use std::sync::{Arc, Mutex};

    if missing.is_empty() {
        return Vec::new();
    }
    let workers = workers.clamp(1, 64).min(missing.len());
    if workers <= 1 {
        return missing
            .iter()
            .map(|h| {
                let bytes = store.get_bytes(h).map_err(|e| format!("store-read:{e}"))?;
                client.store_put(h, &bytes).map_err(|e| format!("{e}"))
            })
            .collect();
    }

    let next = Arc::new(AtomicUsize::new(0));
    let out: Arc<Mutex<Vec<Option<Result<(), String>>>>> =
        Arc::new(Mutex::new((0..missing.len()).map(|_| None).collect()));
    std::thread::scope(|scope| {
        for _ in 0..workers {
            let out = Arc::clone(&out);
            let next = Arc::clone(&next);
            let c = client.clone();
            let s = store.clone();
            scope.spawn(move || {
                loop {
                    let i = next.fetch_add(1, Ordering::Relaxed);
                    if i >= missing.len() {
                        break;
                    }
                    let h = &missing[i];
                    let res = s
                        .get_bytes(h)
                        .map_err(|e| format!("store-read:{e}"))
                        .and_then(|bytes| c.store_put(h, &bytes).map_err(|e| format!("{e}")));
                    if let Ok(mut g) = out.lock() {
                        g[i] = Some(res);
                    } else {
                        return;
                    }
                }
            });
        }
    });
    let mut g = match out.lock() {
        Ok(g) => g,
        Err(_) => {
            return (0..missing.len())
                .map(|_| Err("sync put results lock poisoned".to_string()))
                .collect();
        }
    };
    g.drain(..)
        .map(|x| x.unwrap_or_else(|| Err("sync put worker produced no result".to_string())))
        .collect()
}

pub(super) fn capability_sync_pull(
    payload: &Term,
    pol: Option<&OpPolicy>,
    policy: &CapsPolicy,
    store: Option<&ArtifactStore>,
    refs: Option<&RefsDb>,
    budget: &mut ArtifactBudgetState,
    error_tok: SealId,
    op: &str,
    timeout_ms: Option<u64>,
) -> Result<Value, EffectsError> {
    let store = store.ok_or_else(|| {
        EffectsError::Log("missing artifact store for core/sync::pull".to_string())
    })?;
    let refs =
        refs.ok_or_else(|| EffectsError::Log("missing refs db for core/sync::pull".to_string()))?;

    let remote_s = match payload_sync_remote(payload) {
        Ok(s) => s,
        Err(e) => {
            return Ok(mk_error(error_tok, "core/sync/bad-payload", e, Some(op)));
        }
    };
    let depth = payload_sync_depth(payload).unwrap_or(0);
    let force = payload_sync_force(payload).unwrap_or(false);
    let refnames = match payload_sync_refs(payload) {
        Ok(rs) => rs,
        Err(e) => {
            return Ok(mk_error(error_tok, "core/sync/bad-payload", e, Some(op)));
        }
    };
    let roots = match payload_sync_roots(payload) {
        Ok(rs) => rs,
        Err(e) => {
            return Ok(mk_error(error_tok, "core/sync/bad-payload", e, Some(op)));
        }
    };
    if refnames.is_empty() && roots.is_empty() {
        return Ok(mk_error(
            error_tok,
            "core/sync/bad-payload",
            "pull requires :refs and/or :roots".to_string(),
            Some(op),
        ));
    }

    let sp = match sync_policy_from_op(pol) {
        Ok(p) => p,
        Err(e) => {
            return Ok(mk_error(error_tok, "core/caps/policy-error", e, Some(op)));
        }
    };
    let base = match sync_normalize_and_check_remote(&sp, &remote_s) {
        Ok(b) => b,
        Err(e) => {
            return Ok(mk_error(error_tok, "core/sync/remote-denied", e, Some(op)));
        }
    };
    let auth = match sync_registry_auth(&sp) {
        Ok(a) => a,
        Err(e) => {
            return Ok(mk_error(error_tok, "core/caps/policy-error", e, Some(op)));
        }
    };
    let client = match gc_registry::RegistryClient::new_with_auth(
        &base,
        timeout_ms.map(std::time::Duration::from_millis),
        auth,
    ) {
        Ok(c) => c,
        Err(e) => {
            let code = registry_error_code(&e, "core/sync/remote-auth");
            return Ok(mk_error(error_tok, code, format!("{e}"), Some(op)));
        }
    };

    let mut pulled: u64 = 0;
    let mut already: u64 = 0;
    let mut heads: Vec<Term> = Vec::new();

    for h in &roots {
        let mut stats = SyncPullStats {
            pulled: &mut pulled,
            already: &mut already,
            store_written_bytes: &mut budget.store_written_bytes,
            store_max_run_bytes: policy.store.max_run_bytes,
            error_tok,
            op,
            transfer_workers: sp.transfer_workers,
            max_artifact_bytes: sp.max_artifact_bytes,
            max_batch_bytes: sp.max_batch_bytes,
        };
        match sync_pull_closure(&client, store, h, depth, &mut stats) {
            Ok(()) => {}
            Err(v) => return Ok(v),
        }
    }

    for rname in &refnames {
        let h = match client.refs_get(rname) {
            Ok(Some(h)) => h,
            Ok(None) => {
                return Ok(mk_error(
                    error_tok,
                    "core/sync/ref-not-found",
                    format!("remote ref not found: {rname}"),
                    Some(op),
                ));
            }
            Err(e) => {
                let code = registry_error_code(&e, "core/sync/remote-auth");
                return Ok(mk_error(error_tok, code, format!("{e}"), Some(op)));
            }
        };
        let mut stats = SyncPullStats {
            pulled: &mut pulled,
            already: &mut already,
            store_written_bytes: &mut budget.store_written_bytes,
            store_max_run_bytes: policy.store.max_run_bytes,
            error_tok,
            op,
            transfer_workers: sp.transfer_workers,
            max_artifact_bytes: sp.max_artifact_bytes,
            max_batch_bytes: sp.max_batch_bytes,
        };
        match sync_pull_closure(&client, store, &h, depth, &mut stats) {
            Ok(()) => {}
            Err(v) => return Ok(v),
        }

        let cur = refs.get(rname)?;
        if !force
            && let Some(curh) = &cur
            && curh != &h
        {
            return Ok(mk_error_with_ctx(
                error_tok,
                "core/refs/conflict",
                "local ref differs; use force to overwrite".to_string(),
                Some(op),
                Term::Map(
                    [
                        (
                            TermOrdKey(Term::symbol(":refs/name")),
                            Term::Str(rname.clone()),
                        ),
                        (
                            TermOrdKey(Term::symbol(":refs/current")),
                            cur.clone().map(Term::Str).unwrap_or(Term::Nil),
                        ),
                        (
                            TermOrdKey(Term::symbol(":refs/remote")),
                            Term::Str(h.clone()),
                        ),
                    ]
                    .into_iter()
                    .collect(),
                ),
            ));
        }
        let _ = refs.set(rname, Some(&h), None)?;

        heads.push(Term::Map(
            [
                (TermOrdKey(Term::symbol(":name")), Term::Str(rname.clone())),
                (TermOrdKey(Term::symbol(":hash")), Term::Str(h)),
            ]
            .into_iter()
            .collect(),
        ));
    }

    heads.sort_by_cached_key(print_term);

    let mut m = BTreeMap::new();
    m.insert(TermOrdKey(Term::symbol(":ok")), Term::Bool(true));
    m.insert(TermOrdKey(Term::symbol(":remote")), Term::Str(base));
    m.insert(
        TermOrdKey(Term::symbol(":pulled")),
        Term::Int((pulled as i64).into()),
    );
    m.insert(
        TermOrdKey(Term::symbol(":present")),
        Term::Int((already as i64).into()),
    );
    m.insert(TermOrdKey(Term::symbol(":heads")), Term::Vector(heads));
    Ok(Value::Data(Term::Map(m)))
}

pub(super) fn capability_sync_push(
    payload: &Term,
    pol: Option<&OpPolicy>,
    store: Option<&ArtifactStore>,
    error_tok: SealId,
    op: &str,
    timeout_ms: Option<u64>,
) -> Result<Value, EffectsError> {
    let store = store.ok_or_else(|| {
        EffectsError::Log("missing artifact store for core/sync::push".to_string())
    })?;

    let remote_s = match payload_sync_remote(payload) {
        Ok(s) => s,
        Err(e) => {
            return Ok(mk_error(error_tok, "core/sync/bad-payload", e, Some(op)));
        }
    };
    let depth = payload_sync_depth(payload).unwrap_or(0);
    let roots = match payload_sync_roots(payload) {
        Ok(rs) => rs,
        Err(e) => {
            return Ok(mk_error(error_tok, "core/sync/bad-payload", e, Some(op)));
        }
    };
    if roots.is_empty() {
        return Ok(mk_error(
            error_tok,
            "core/sync/bad-payload",
            "push requires :roots".to_string(),
            Some(op),
        ));
    }
    let set_refs = match payload_sync_set_refs(payload) {
        Ok(v) => v,
        Err(e) => {
            return Ok(mk_error(error_tok, "core/sync/bad-payload", e, Some(op)));
        }
    };
    for sr in &set_refs {
        if let Err(v) = local_refs_validate_policy_gate(
            store,
            &sr.name,
            Some(&sr.hash),
            &sr.policy,
            error_tok,
            op,
        ) {
            return Ok(v);
        }
    }

    let sp = match sync_policy_from_op(pol) {
        Ok(p) => p,
        Err(e) => {
            return Ok(mk_error(error_tok, "core/caps/policy-error", e, Some(op)));
        }
    };
    let base = match sync_normalize_and_check_remote(&sp, &remote_s) {
        Ok(b) => b,
        Err(e) => {
            return Ok(mk_error(error_tok, "core/sync/remote-denied", e, Some(op)));
        }
    };
    let auth = match sync_registry_auth(&sp) {
        Ok(a) => a,
        Err(e) => {
            return Ok(mk_error(error_tok, "core/caps/policy-error", e, Some(op)));
        }
    };
    let client = match gc_registry::RegistryClient::new_with_auth(
        &base,
        timeout_ms.map(std::time::Duration::from_millis),
        auth,
    ) {
        Ok(c) => c,
        Err(e) => {
            let code = registry_error_code(&e, "core/sync/remote-auth");
            return Ok(mk_error(error_tok, code, format!("{e}"), Some(op)));
        }
    };

    let mut all: std::collections::BTreeSet<String> = std::collections::BTreeSet::new();
    for h in &roots {
        match sync_closure_local(store, h, depth, &mut all, error_tok, op) {
            Ok(()) => {}
            Err(v) => return Ok(v),
        }
    }
    let hashes: Vec<String> = all.into_iter().collect();

    let mut missing: Vec<String> = Vec::new();
    let mut present: u64 = 0;
    let has_chunks: Vec<Vec<String>> = hashes.chunks(512).map(|chunk| chunk.to_vec()).collect();
    let has_results = sync_parallel_store_has_chunks(&client, &has_chunks, sp.transfer_workers);
    for (chunk_i, chunk) in hashes.chunks(512).enumerate() {
        let mp = match &has_results[chunk_i] {
            Ok(m) => m,
            Err(e) => {
                let code = registry_error_code(e, "core/sync/remote-auth");
                return Ok(mk_error(error_tok, code, format!("{e}"), Some(op)));
            }
        };
        for h in chunk {
            match mp.get(h) {
                Some(true) => present = present.saturating_add(1),
                _ => missing.push(h.clone()),
            }
        }
    }
    missing.sort();
    missing.dedup();

    let upload_results =
        sync_parallel_upload_missing(&client, store, &missing, sp.transfer_workers);
    let mut uploaded: u64 = 0;
    for r in upload_results {
        match r {
            Ok(()) => uploaded = uploaded.saturating_add(1),
            Err(e) => {
                let (code, msg) = if e.starts_with("store-read:") {
                    ("core/store/not-found", e)
                } else if e.starts_with("auth error:") {
                    ("core/sync/remote-auth", e)
                } else {
                    ("core/sync/remote-error", e)
                };
                return Ok(mk_error(error_tok, code, msg, Some(op)));
            }
        }
    }

    let mut refs_updated: u64 = 0;
    if !set_refs.is_empty() {
        let mut set_refs_sorted = set_refs;
        set_refs_sorted.sort_by(|a, b| a.name.cmp(&b.name));
        for sr in &set_refs_sorted {
            let req = gc_registry::RefsSetReq {
                name: &sr.name,
                hash: &sr.hash,
                policy: &sr.policy,
                expected_old: sr.expected_old.as_deref(),
            };
            match client.refs_set(&req) {
                Ok(r) => {
                    if !r.ok {
                        return Ok(mk_error(
                            error_tok,
                            "core/sync/refs-set-failed",
                            "remote refs/set returned ok=false".to_string(),
                            Some(op),
                        ));
                    }
                    refs_updated = refs_updated.saturating_add(1);
                }
                Err(e) => {
                    let code = registry_error_code(&e, "core/sync/remote-auth");
                    return Ok(mk_error(error_tok, code, format!("{e}"), Some(op)));
                }
            }
        }
    }

    let mut m = BTreeMap::new();
    m.insert(TermOrdKey(Term::symbol(":ok")), Term::Bool(true));
    m.insert(TermOrdKey(Term::symbol(":remote")), Term::Str(base));
    m.insert(
        TermOrdKey(Term::symbol(":total")),
        Term::Int((hashes.len() as i64).into()),
    );
    m.insert(
        TermOrdKey(Term::symbol(":present")),
        Term::Int((present as i64).into()),
    );
    m.insert(
        TermOrdKey(Term::symbol(":uploaded")),
        Term::Int((uploaded as i64).into()),
    );
    m.insert(
        TermOrdKey(Term::symbol(":refs-updated")),
        Term::Int((refs_updated as i64).into()),
    );
    Ok(Value::Data(Term::Map(m)))
}

pub(super) fn sync_closure_local(
    store: &ArtifactStore,
    root: &str,
    depth: u64,
    out: &mut std::collections::BTreeSet<String>,
    error_tok: SealId,
    op: &str,
) -> Result<(), Value> {
    use std::collections::{HashSet, VecDeque};
    let mut q: VecDeque<(String, u64)> = VecDeque::new();
    q.push_back((root.to_string(), depth));
    let mut seen: HashSet<String> = HashSet::new();
    let mut obj_count: u64 = 0;

    while let Some((h, dleft)) = q.pop_front() {
        if !seen.insert(h.clone()) {
            continue;
        }
        obj_count = obj_count.saturating_add(1);
        if obj_count > 50_000 {
            return Err(mk_error(
                error_tok,
                "core/sync/too-many-objects",
                "closure exceeded 50k objects".to_string(),
                Some(op),
            ));
        }
        if !store.path_for(&h).exists() {
            return Err(mk_error(
                error_tok,
                "core/store/not-found",
                format!("artifact not found: {h}"),
                Some(op),
            ));
        }
        if store.verify_hex(&h).is_err() {
            return Err(mk_error(
                error_tok,
                "core/store/corruption",
                format!("artifact store corruption: {h}"),
                Some(op),
            ));
        }
        out.insert(h.clone());

        let t = match store_get_term(store, &h) {
            Ok(t) => t,
            Err(_) => continue,
        };
        if let Ok(c) = gc_vcs::Commit::from_term(&t) {
            if let Some(b) = c.base {
                q.push_back((b, dleft));
            }
            q.push_back((c.patch, dleft));
            q.push_back((c.result, dleft));
            for x in c.evidence {
                q.push_back((x, dleft));
            }
            for x in c.attestations {
                q.push_back((x, dleft));
            }
            if dleft > 0 {
                for p in c.parents {
                    q.push_back((p, dleft - 1));
                }
            }
            continue;
        }
        if let Ok(p) = gc_vcs::Patch::from_term(&t) {
            for x in p.refs() {
                q.push_back((x, dleft));
            }
            continue;
        }
        if let Ok(e) = gc_vcs::Evidence::from_term(&t) {
            for x in e.refs() {
                q.push_back((x, dleft));
            }
            continue;
        }
        if let Ok(c) = gc_vcs::Conflict::from_term(&t) {
            for x in c.refs() {
                q.push_back((x, dleft));
            }
            continue;
        }
        if let Ok(s) = gc_vcs::Snapshot::from_term(&t) {
            for x in s.shallow_refs() {
                q.push_back((x, dleft));
            }
        }
    }
    Ok(())
}

pub(super) fn resolve_gpk_root_for_export(
    store: &ArtifactStore,
    refs: Option<&RefsDb>,
    root_spec: &str,
    mode: GpkMode,
    error_tok: SealId,
    op: &str,
) -> Result<String, Value> {
    let mut root = root_spec.trim().to_string();
    if let Some(s) = root.strip_prefix("h:") {
        root = s.to_string();
    }
    if gc_vcs::validate_hex_hash(&root).is_ok() {
        return Ok(root.to_ascii_lowercase());
    }
    if let Some(s) = root.strip_prefix("ref:") {
        root = s.to_string();
    }
    if !root.starts_with("refs/") {
        return Err(mk_error(
            error_tok,
            "core/gpk/bad-root",
            "root must be a hash or refs/...".to_string(),
            Some(op),
        ));
    }
    let refs = refs.ok_or_else(|| {
        mk_error(
            error_tok,
            "core/gpk/missing-refs-db",
            "refs db required when root is a ref".to_string(),
            Some(op),
        )
    })?;
    let resolved = refs
        .get(&root)
        .map_err(|e| mk_error(error_tok, "core/gpk/refs-io-error", e.to_string(), Some(op)))?;
    let Some(hash) = resolved else {
        return Err(mk_error(
            error_tok,
            "core/gpk/ref-not-found",
            format!("ref not found: {root}"),
            Some(op),
        ));
    };
    let hash = hash.to_ascii_lowercase();
    let root_term = match store_get_term(store, &hash) {
        Ok(t) => t,
        Err(_) => {
            return Err(mk_error(
                error_tok,
                "core/store/not-found",
                format!("artifact not found: {hash}"),
                Some(op),
            ));
        }
    };
    if mode == GpkMode::Shallow && gc_vcs::Snapshot::from_term(&root_term).is_err() {
        return Err(mk_error(
            error_tok,
            "core/gpk/bad-root",
            "shallow export root must resolve to a :vcs/snapshot".to_string(),
            Some(op),
        ));
    }
    Ok(hash)
}

#[derive(Copy, Clone, Debug)]
pub(super) struct GpkClosureOptions<'a> {
    pub(super) depth: u64,
    pub(super) mode: GpkMode,
    pub(super) include_evidence: GpkIncludeEvidence,
    pub(super) include_deps: GpkIncludeDeps,
    pub(super) root_snapshot_for_locked_deps: Option<&'a str>,
}
