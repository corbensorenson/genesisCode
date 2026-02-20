#[cfg(not(target_os = "wasi"))]
use std::collections::BTreeMap;
#[cfg(not(target_os = "wasi"))]
#[cfg(not(target_os = "wasi"))]
use std::path::PathBuf;
#[cfg(not(target_os = "wasi"))]
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
#[cfg(not(target_os = "wasi"))]
use std::sync::{Arc, Mutex};
#[cfg(not(target_os = "wasi"))]
use std::thread;
#[cfg(not(target_os = "wasi"))]
use std::time::Duration;

#[cfg(not(target_os = "wasi"))]
use reqwest::Url;
#[cfg(not(target_os = "wasi"))]
use serde::{Deserialize, Serialize};
#[cfg(not(target_os = "wasi"))]
use tiny_http::{Header, Method, Request, Response, Server, StatusCode};

#[cfg(not(target_os = "wasi"))]
use crate::{
    RefsSetReq, RegistryClient, RegistryError, StoreUploadChunkResp, StoreUploadFinishResp,
    StoreUploadStartResp, StoreUploadStatusResp,
};

#[cfg(not(target_os = "wasi"))]
#[derive(Debug, Clone)]
pub struct HttpRegistryServerConfig {
    pub addr: String,
    pub root: PathBuf,
    pub max_chunk_bytes: u64,
    pub max_requests: Option<u64>,
}

#[cfg(not(target_os = "wasi"))]
impl Default for HttpRegistryServerConfig {
    fn default() -> Self {
        Self {
            addr: "127.0.0.1:0".to_string(),
            root: PathBuf::from("."),
            max_chunk_bytes: 4_194_304,
            max_requests: None,
        }
    }
}

#[cfg(not(target_os = "wasi"))]
#[derive(Debug)]
pub struct HttpRegistryServerHandle {
    shutdown: Arc<AtomicBool>,
    join: Option<thread::JoinHandle<Result<(), RegistryError>>>,
    bound_addr: String,
}

#[cfg(not(target_os = "wasi"))]
impl HttpRegistryServerHandle {
    pub fn bound_addr(&self) -> &str {
        &self.bound_addr
    }

    pub fn shutdown(&self) {
        self.shutdown.store(true, Ordering::Relaxed);
    }

    pub fn join(mut self) -> Result<(), RegistryError> {
        self.shutdown();
        if let Some(j) = self.join.take() {
            return j.join().map_err(|_| {
                RegistryError::Protocol("registry server thread panicked".to_string())
            })?;
        }
        Ok(())
    }
}

#[cfg(not(target_os = "wasi"))]
#[derive(Debug)]
struct UploadSession {
    hash: String,
    size_bytes: u64,
    chunks: BTreeMap<u64, Vec<u8>>,
}

#[cfg(not(target_os = "wasi"))]
#[derive(Debug)]
struct UploadState {
    next_upload_id: AtomicU64,
    sessions: Mutex<BTreeMap<String, UploadSession>>,
}

#[cfg(not(target_os = "wasi"))]
impl Default for UploadState {
    fn default() -> Self {
        Self {
            next_upload_id: AtomicU64::new(1),
            sessions: Mutex::new(BTreeMap::new()),
        }
    }
}

#[cfg(not(target_os = "wasi"))]
#[derive(Debug, Clone, Deserialize)]
struct StoreHasReqOwned {
    hashes: Vec<String>,
}

#[cfg(not(target_os = "wasi"))]
#[derive(Debug, Clone, Serialize)]
struct StoreHasRespOwned {
    present: BTreeMap<String, bool>,
}

#[cfg(not(target_os = "wasi"))]
#[derive(Debug, Clone, Deserialize)]
struct StoreUploadStartReqOwned {
    hash: String,
    size_bytes: u64,
}

#[cfg(not(target_os = "wasi"))]
#[derive(Debug, Clone, Deserialize)]
struct StoreUploadFinishReqOwned {
    upload_id: String,
}

#[cfg(not(target_os = "wasi"))]
#[derive(Debug, Clone, Serialize)]
struct RefsGetRespOwned {
    name: String,
    hash: Option<String>,
}

#[cfg(not(target_os = "wasi"))]
#[derive(Debug, Clone, Serialize)]
struct RefsListRespOwned {
    refs: Vec<crate::RefsListEntry>,
}

#[cfg(not(target_os = "wasi"))]
#[derive(Debug, Clone, Deserialize)]
struct RefsSetReqOwned {
    name: String,
    hash: String,
    policy: String,
    expected_old: Option<String>,
}

#[cfg(not(target_os = "wasi"))]
#[derive(Debug, Clone, Serialize)]
struct ErrorEnvelope {
    error: ErrorBody,
}

#[cfg(not(target_os = "wasi"))]
#[derive(Debug, Clone, Serialize)]
struct ErrorBody {
    code: String,
    message: String,
}

#[cfg(not(target_os = "wasi"))]
pub fn spawn_http_file_registry_server(
    cfg: HttpRegistryServerConfig,
) -> Result<HttpRegistryServerHandle, RegistryError> {
    let server = Server::http(&cfg.addr)
        .map_err(|e| RegistryError::Http(format!("registry/serve bind {}: {e}", cfg.addr)))?;
    let bound_addr = server.server_addr().to_string();

    let shutdown = Arc::new(AtomicBool::new(false));
    let shutdown_worker = Arc::clone(&shutdown);
    let join = thread::spawn(move || run_server_loop(server, cfg, shutdown_worker));

    Ok(HttpRegistryServerHandle {
        shutdown,
        join: Some(join),
        bound_addr,
    })
}

#[cfg(not(target_os = "wasi"))]
fn run_server_loop(
    server: Server,
    cfg: HttpRegistryServerConfig,
    shutdown: Arc<AtomicBool>,
) -> Result<(), RegistryError> {
    let root = cfg.root.canonicalize().unwrap_or(cfg.root.clone());
    std::fs::create_dir_all(root.join("v1").join("store"))
        .map_err(|e| RegistryError::Http(format!("registry/serve mkdir: {e}")))?;
    let remote = format!("file://{}/", root.display());
    let client = RegistryClient::new(&remote, None)?;
    let uploads = UploadState::default();
    let mut handled: u64 = 0;

    loop {
        if shutdown.load(Ordering::Relaxed) {
            break;
        }
        if let Some(max) = cfg.max_requests
            && handled >= max
        {
            break;
        }

        let req_opt = server
            .recv_timeout(Duration::from_millis(100))
            .map_err(|e| RegistryError::Http(format!("registry/serve recv: {e}")))?;
        let Some(req) = req_opt else {
            continue;
        };

        let _ = handle_request(req, &client, &uploads, cfg.max_chunk_bytes);
        handled = handled.saturating_add(1);
    }
    Ok(())
}

#[cfg(not(target_os = "wasi"))]
fn handle_request(
    mut req: Request,
    client: &RegistryClient,
    uploads: &UploadState,
    max_chunk_bytes: u64,
) -> Result<(), RegistryError> {
    let method = req.method().clone();
    let parsed = parse_req_url(req.url())?;
    let plan: Result<(u16, &'static str, Vec<u8>), RegistryError> = (|| {
        if parsed.path == "v1/ping" && method == Method::Get {
            let body = serde_json::to_vec(&serde_json::json!({
                "ok": true,
                "version": "0.1",
                "hash": "blake3-256",
                "max_chunk_bytes": max_chunk_bytes
            }))
            .map_err(|e| RegistryError::Protocol(format!("json encode: {e}")))?;
            return Ok((200, "application/json", body));
        }

        match (method, parsed.path.as_str()) {
            (Method::Post, "v1/store/has") => {
                let in_req: StoreHasReqOwned = read_json(&mut req)?;
                let present = client.store_has(&in_req.hashes)?;
                let body = serde_json::to_vec(&StoreHasRespOwned { present })
                    .map_err(|e| RegistryError::Protocol(format!("json encode: {e}")))?;
                Ok((200, "application/json", body))
            }
            (Method::Get, p) if p.starts_with("v1/store/get/") => {
                let hash = p.trim_start_matches("v1/store/get/");
                let bytes = client.store_get(hash)?;
                Ok((200, "application/octet-stream", bytes))
            }
            (Method::Put, p) if p.starts_with("v1/store/put/") => {
                let hash = p.trim_start_matches("v1/store/put/");
                let mut bytes = Vec::new();
                req.as_reader()
                    .read_to_end(&mut bytes)
                    .map_err(|e| RegistryError::Http(format!("store/put read: {e}")))?;
                client.store_put(hash, &bytes)?;
                Ok((200, "application/json", b"{}".to_vec()))
            }
            (Method::Post, "v1/store/upload/start") => {
                let in_req: StoreUploadStartReqOwned = read_json(&mut req)?;
                let upload_id = format!(
                    "u_{}",
                    uploads.next_upload_id.fetch_add(1, Ordering::Relaxed)
                );
                let mut g = uploads
                    .sessions
                    .lock()
                    .map_err(|_| RegistryError::Protocol("upload lock poisoned".to_string()))?;
                g.insert(
                    upload_id.clone(),
                    UploadSession {
                        hash: in_req.hash,
                        size_bytes: in_req.size_bytes,
                        chunks: BTreeMap::new(),
                    },
                );
                let body = serde_json::to_vec(&StoreUploadStartResp {
                    upload_id,
                    chunk_bytes: max_chunk_bytes,
                })
                .map_err(|e| RegistryError::Protocol(format!("json encode: {e}")))?;
                Ok((200, "application/json", body))
            }
            (Method::Put, p) if p.starts_with("v1/store/upload/chunk/") => {
                let suffix = p.trim_start_matches("v1/store/upload/chunk/");
                let mut segs = suffix.splitn(2, '/');
                let upload_id = segs.next().unwrap_or_default();
                let idx = segs
                    .next()
                    .ok_or_else(|| {
                        RegistryError::Protocol(
                            "store/upload/chunk: missing index path segment".to_string(),
                        )
                    })?
                    .parse::<u64>()
                    .map_err(|e| {
                        RegistryError::Protocol(format!("store/upload/chunk: invalid index: {e}"))
                    })?;
                let mut bytes = Vec::new();
                req.as_reader()
                    .read_to_end(&mut bytes)
                    .map_err(|e| RegistryError::Http(format!("store/upload/chunk read: {e}")))?;
                if bytes.len() as u64 > max_chunk_bytes {
                    return Err(RegistryError::Protocol(
                        "store/upload/chunk: exceeds max_chunk_bytes".to_string(),
                    ));
                }
                let mut g = uploads
                    .sessions
                    .lock()
                    .map_err(|_| RegistryError::Protocol("upload lock poisoned".to_string()))?;
                let session = g.get_mut(upload_id).ok_or_else(|| {
                    RegistryError::Http("store/upload/chunk: status 404".to_string())
                })?;
                session.chunks.insert(idx, bytes.clone());
                let body = serde_json::to_vec(&StoreUploadChunkResp {
                    ok: true,
                    received: bytes.len() as u64,
                })
                .map_err(|e| RegistryError::Protocol(format!("json encode: {e}")))?;
                Ok((200, "application/json", body))
            }
            (Method::Post, "v1/store/upload/finish") => {
                let in_req: StoreUploadFinishReqOwned = read_json(&mut req)?;
                let session = {
                    let mut g = uploads
                        .sessions
                        .lock()
                        .map_err(|_| RegistryError::Protocol("upload lock poisoned".to_string()))?;
                    g.remove(&in_req.upload_id).ok_or_else(|| {
                        RegistryError::Http("store/upload/finish: status 404".to_string())
                    })?
                };
                let mut keys: Vec<u64> = session.chunks.keys().copied().collect();
                keys.sort_unstable();
                for (i, k) in keys.iter().enumerate() {
                    if *k != i as u64 {
                        return Err(RegistryError::Protocol(
                            "store/upload/finish: missing chunk index".to_string(),
                        ));
                    }
                }
                let mut payload = Vec::new();
                for idx in keys {
                    let chunk = session.chunks.get(&idx).ok_or_else(|| {
                        RegistryError::Protocol("upload chunk missing".to_string())
                    })?;
                    payload.extend_from_slice(chunk);
                }
                if payload.len() as u64 != session.size_bytes {
                    return Err(RegistryError::Protocol(
                        "store/upload/finish: size mismatch".to_string(),
                    ));
                }
                client.store_put(&session.hash, &payload)?;
                let body = serde_json::to_vec(&StoreUploadFinishResp { ok: true })
                    .map_err(|e| RegistryError::Protocol(format!("json encode: {e}")))?;
                Ok((200, "application/json", body))
            }
            (Method::Get, p) if p.starts_with("v1/store/upload/status/") => {
                let upload_id = p.trim_start_matches("v1/store/upload/status/");
                let g = uploads
                    .sessions
                    .lock()
                    .map_err(|_| RegistryError::Protocol("upload lock poisoned".to_string()))?;
                let sess = g.get(upload_id).ok_or_else(|| {
                    RegistryError::Http("store/upload/status: status 404".to_string())
                })?;
                let mut received: Vec<u64> = sess.chunks.keys().copied().collect();
                received.sort_unstable();
                let body = serde_json::to_vec(&StoreUploadStatusResp {
                    received_chunks: received,
                })
                .map_err(|e| RegistryError::Protocol(format!("json encode: {e}")))?;
                Ok((200, "application/json", body))
            }
            (Method::Get, "v1/refs/get") => {
                let name = parsed.query.get("name").cloned().ok_or_else(|| {
                    RegistryError::Protocol("refs/get: missing query parameter `name`".to_string())
                })?;
                let hash = client.refs_get(&name)?;
                let body = serde_json::to_vec(&RefsGetRespOwned { name, hash })
                    .map_err(|e| RegistryError::Protocol(format!("json encode: {e}")))?;
                Ok((200, "application/json", body))
            }
            (Method::Get, "v1/refs/list") => {
                let prefix = parsed.query.get("prefix").map(String::as_str);
                let refs = client.refs_list(prefix)?;
                let body = serde_json::to_vec(&RefsListRespOwned { refs })
                    .map_err(|e| RegistryError::Protocol(format!("json encode: {e}")))?;
                Ok((200, "application/json", body))
            }
            (Method::Post, "v1/refs/set") => {
                let in_req: RefsSetReqOwned = read_json(&mut req)?;
                let out = client.refs_set(&RefsSetReq {
                    name: &in_req.name,
                    hash: &in_req.hash,
                    policy: &in_req.policy,
                    expected_old: in_req.expected_old.as_deref(),
                })?;
                let body = serde_json::to_vec(&out)
                    .map_err(|e| RegistryError::Protocol(format!("json encode: {e}")))?;
                Ok((200, "application/json", body))
            }
            _ => Err(RegistryError::Http("route: status 404".to_string())),
        }
    })();

    let (status, content_type, body) = match plan {
        Ok(ok) => ok,
        Err(err) => {
            let (status, code, message) = registry_error_http(err);
            let body = serde_json::to_vec(&ErrorEnvelope {
                error: ErrorBody { code, message },
            })
            .map_err(|e| RegistryError::Protocol(format!("json encode: {e}")))?;
            (status, "application/json", body)
        }
    };
    respond_bytes(req, status, content_type, body)
}

#[cfg(not(target_os = "wasi"))]
fn read_json<T: for<'de> Deserialize<'de>>(req: &mut Request) -> Result<T, RegistryError> {
    let mut buf = Vec::new();
    req.as_reader()
        .read_to_end(&mut buf)
        .map_err(|e| RegistryError::Http(format!("read body: {e}")))?;
    serde_json::from_slice(&buf).map_err(|e| RegistryError::Protocol(format!("json decode: {e}")))
}

#[cfg(not(target_os = "wasi"))]
fn respond_bytes(
    req: Request,
    status: u16,
    content_type: &str,
    body: Vec<u8>,
) -> Result<(), RegistryError> {
    let content_type_header = Header::from_bytes(b"Content-Type", content_type.as_bytes())
        .map_err(|e| RegistryError::Protocol(format!("bad response header: {e:?}")))?;
    let resp = Response::from_data(body)
        .with_status_code(StatusCode(status))
        .with_header(content_type_header);
    req.respond(resp)
        .map_err(|e| RegistryError::Http(format!("respond: {e}")))
}

#[cfg(not(target_os = "wasi"))]
fn registry_error_http(err: RegistryError) -> (u16, String, String) {
    match err {
        RegistryError::Auth(msg) => (401, "unauthorized".to_string(), msg),
        RegistryError::RemoteSpec(msg) => (400, "bad_request".to_string(), msg),
        RegistryError::Protocol(msg) => (400, "protocol".to_string(), msg),
        RegistryError::Http(msg) => {
            let status = parse_status_code_hint(&msg).unwrap_or(500);
            let code = match status {
                400 => "bad_request",
                401 => "unauthorized",
                403 => "forbidden",
                404 => "not_found",
                409 => "conflict",
                413 => "payload_too_large",
                _ => "internal",
            };
            (status, code.to_string(), msg)
        }
    }
}

#[cfg(not(target_os = "wasi"))]
fn parse_status_code_hint(msg: &str) -> Option<u16> {
    let needle = "status ";
    let idx = msg.find(needle)?;
    let raw = msg[idx + needle.len()..]
        .chars()
        .take_while(|c| c.is_ascii_digit())
        .collect::<String>();
    raw.parse::<u16>().ok()
}

#[cfg(not(target_os = "wasi"))]
#[derive(Debug, Clone)]
struct ParsedReqUrl {
    path: String,
    query: BTreeMap<String, String>,
}

#[cfg(not(target_os = "wasi"))]
fn parse_req_url(raw: &str) -> Result<ParsedReqUrl, RegistryError> {
    let u = Url::parse(&format!("http://localhost{raw}"))
        .map_err(|e| RegistryError::Protocol(format!("bad request url `{raw}`: {e}")))?;
    let mut query = BTreeMap::new();
    for (k, v) in u.query_pairs() {
        query.insert(k.to_string(), v.to_string());
    }
    Ok(ParsedReqUrl {
        path: u.path().trim_start_matches('/').to_string(),
        query,
    })
}
