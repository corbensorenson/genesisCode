pub fn wasi_http_bridge_configured() -> bool {
    wasi_http_bridge_root_from_env().is_some() || discover_workspace_runtime_wasi_bridge_root().is_some()
}

fn wasi_http_bridge_root_for_base(base: &Url) -> Option<PathBuf> {
    if let Some(root) = wasi_http_bridge_root_from_env() {
        return Some(resolve_wasi_http_bridge_root_for_remote(&root, base));
    }
    discover_workspace_runtime_wasi_bridge_root()
        .map(|root| resolve_wasi_http_bridge_root_for_remote(&root, base))
}

fn wasi_http_bridge_root_from_env() -> Option<PathBuf> {
    let raw = std::env::var(WASI_HTTP_BRIDGE_ROOT_ENV).ok()?;
    let trimmed = raw.trim();
    if trimmed.is_empty() {
        return None;
    }
    Some(PathBuf::from(trimmed))
}

fn discover_workspace_runtime_wasi_bridge_root() -> Option<PathBuf> {
    let mut cur = std::env::current_dir().ok()?;
    loop {
        let candidate = cur
            .join(".genesis")
            .join("runtime")
            .join("wasi-http-bridge");
        if candidate.is_dir() {
            return Some(candidate);
        }
        if !cur.pop() {
            return None;
        }
    }
}

fn resolve_wasi_http_bridge_root_for_remote(root: &Path, base: &Url) -> PathBuf {
    if root.file_name().map(|n| n == "v1").unwrap_or(false) {
        return root.to_path_buf();
    }

    if let Some(host_token) = bridge_host_token(base)
        && (root.join(base.scheme()).is_dir()
            || root.file_name().map(|n| n == "wasi-http-bridge").unwrap_or(false))
    {
        return root.join(base.scheme()).join(host_token).join("v1");
    }

    let legacy_candidate = root.join("v1");
    if legacy_candidate.exists() {
        return legacy_candidate;
    }
    root.to_path_buf()
}

fn bridge_host_token(base: &Url) -> Option<String> {
    let host = base.host_str()?;
    let port = base.port_or_known_default().unwrap_or_else(|| {
        if base.scheme() == "https" {
            443
        } else {
            80
        }
    });
    Some(format!(
        "{}_{}",
        sanitize_bridge_token_segment(host),
        port
    ))
}

fn sanitize_bridge_token_segment(raw: &str) -> String {
    raw.chars()
        .map(|ch| {
            if ch.is_ascii_alphanumeric() || matches!(ch, '.' | '-' | '_') {
                ch
            } else {
                '_'
            }
        })
        .collect()
}

pub fn wasi_http_bridge_resolve_remote_root(
    bridge_root: &Path,
    remote: &str,
) -> Result<PathBuf, RegistryError> {
    let base = normalize_remote_base(remote)?;
    Ok(resolve_wasi_http_bridge_root_for_remote(
        bridge_root,
        &base,
    ))
}

#[cfg(target_os = "wasi")]
fn wasi_http_unsupported(op: &str) -> RegistryError {
    RegistryError::Http(format!(
        "{op}: http(s) registry remotes are not supported in WASI builds; use file:// or inproc://"
    ))
}

#[cfg(not(target_os = "wasi"))]
fn status_error(op: &str, status: StatusCode) -> RegistryError {
    if status == StatusCode::UNAUTHORIZED || status == StatusCode::FORBIDDEN {
        RegistryError::Auth(format!("{op}: status {status}"))
    } else {
        RegistryError::Http(format!("{op}: status {status}"))
    }
}

fn enforce_body_limit(
    op: &str,
    max_bytes: Option<usize>,
    observed: u64,
) -> Result<(), RegistryError> {
    let Some(max) = max_bytes else {
        return Ok(());
    };
    if observed > max as u64 {
        return Err(RegistryError::Protocol(format!(
            "resource-limit: {op}: response exceeds configured limit ({observed} > {max} bytes)"
        )));
    }
    Ok(())
}

fn chunk_upload_not_supported(err: &RegistryError) -> bool {
    match err {
        RegistryError::Protocol(s) => s.contains("not supported"),
        RegistryError::Http(s) => s.contains("status 404") || s.contains("status 405"),
        _ => false,
    }
}

#[cfg(not(target_os = "wasi"))]
fn read_response_bytes_limited(
    op: &str,
    mut r: reqwest::blocking::Response,
    max_bytes: Option<usize>,
) -> Result<Vec<u8>, RegistryError> {
    if let Some(max) = max_bytes {
        if let Some(cl) = r.content_length() {
            enforce_body_limit(op, Some(max), cl)?;
        }
        let mut out: Vec<u8> = Vec::new();
        let mut buf = [0u8; 8 * 1024];
        loop {
            let n = r
                .read(&mut buf)
                .map_err(|e| RegistryError::Http(format!("{op} read: {e}")))?;
            if n == 0 {
                break;
            }
            if out.len().saturating_add(n) > max {
                return Err(RegistryError::Protocol(format!(
                    "resource-limit: {op}: response exceeds configured limit (> {max} bytes)"
                )));
            }
            out.extend_from_slice(&buf[..n]);
        }
        Ok(out)
    } else {
        r.bytes()
            .map(|b| b.to_vec())
            .map_err(|e| RegistryError::Http(format!("{op} bytes: {e}")))
    }
}

pub fn normalize_remote_base(remote: &str) -> Result<Url, RegistryError> {
    let t = remote.trim();
    if t.is_empty() {
        return Err(RegistryError::RemoteSpec("remote is empty".to_string()));
    }
    let mut u = if t.starts_with("gen://") {
        let rest = t.strip_prefix("gen://").unwrap_or("");
        Url::parse(&format!("https://{rest}"))
            .map_err(|e| RegistryError::RemoteSpec(format!("bad gen:// url: {e}")))?
    } else {
        Url::parse(t).map_err(|e| RegistryError::RemoteSpec(format!("bad url: {e}")))?
    };
    if u.scheme() != "https"
        && u.scheme() != "http"
        && u.scheme() != "inproc"
        && u.scheme() != "file"
    {
        return Err(RegistryError::RemoteSpec(format!(
            "unsupported scheme {}",
            u.scheme()
        )));
    }

    // Normalize to .../v1/ base.
    let path = u.path().to_string();
    let base_path = if path.ends_with("/v1/") {
        path
    } else if path.ends_with("/v1") {
        format!("{path}/")
    } else if path.ends_with('/') || path.is_empty() {
        format!("{path}v1/")
    } else {
        format!("{path}/v1/")
    };
    u.set_path(&base_path);
    u.set_query(None);
    Ok(u)
}
