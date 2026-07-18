use std::collections::{BTreeMap, BTreeSet};
use std::io::{BufRead, BufReader, Read, Write};
use std::net::{Shutdown, SocketAddr, TcpListener, TcpStream, ToSocketAddrs, UdpSocket};
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex, OnceLock};
use std::time::Duration;

use aes_gcm::Aes256Gcm;
use aes_gcm::aead::{AeadInPlace, KeyInit};
use base64ct::{Base64, Encoding};
use chacha20poly1305::ChaCha20Poly1305;
use ed25519_dalek::{Signature, Signer, SigningKey, Verifier, VerifyingKey};
#[cfg(not(target_os = "wasi"))]
use fs2::FileExt;
use gc_coreform::{Term, TermOrdKey, parse_term, print_term};
use hkdf::Hkdf;
use hmac::{Hmac, Mac};
use num_traits::cast::ToPrimitive;
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256, Sha512};

#[derive(Debug, Serialize, Deserialize, Default)]
struct ProcessRecord {
    #[serde(default)]
    exit: Option<i64>,
    #[serde(default)]
    stdout: String,
    #[serde(default)]
    stderr: String,
    #[serde(default)]
    killed: bool,
    #[serde(default)]
    stdin_writes: Vec<String>,
    #[serde(default)]
    child_pid: Option<u32>,
    #[serde(default)]
    supervisor_pid: Option<u32>,
    #[serde(default)]
    stdin_fifo: Option<String>,
    #[serde(default)]
    stdout_path: Option<String>,
    #[serde(default)]
    stderr_path: Option<String>,
    #[serde(default)]
    exit_path: Option<String>,
    #[serde(default)]
    pid_path: Option<String>,
}

#[derive(Default)]
struct NetBridgeState {
    tcp_streams: BTreeMap<String, TcpStream>,
    tcp_listeners: BTreeMap<String, TcpListener>,
    udp_sockets: BTreeMap<String, UdpSocket>,
    http_listeners: BTreeMap<String, TcpListener>,
    http_listener_by_local: BTreeMap<String, String>,
    http_requests: BTreeMap<String, HttpPendingRequest>,
    ws_streams: BTreeMap<String, Arc<Mutex<WsStream>>>,
}

struct HttpPendingRequest {
    listener_id: String,
    stream: TcpStream,
    headers: Vec<String>,
}

struct WsStream {
    stream: TcpStream,
    mask_outgoing: bool,
}

fn net_bridge_state() -> &'static Mutex<NetBridgeState> {
    static STATE: OnceLock<Mutex<NetBridgeState>> = OnceLock::new();
    STATE.get_or_init(|| Mutex::new(NetBridgeState::default()))
}

#[path = "host_bridge_runtime_net_http_ws.rs"]
mod net_http_ws;
use net_http_ws::*;
#[path = "host_bridge_runtime_host_abi.rs"]
mod host_abi;
use host_abi::*;

pub(crate) fn maybe_run_host_bridge_mode() -> Option<std::process::ExitCode> {
    let op = std::env::var("GENESIS_HOST_BRIDGE_OP").ok()?;
    let transport = std::env::var("GENESIS_HOST_BRIDGE_TRANSPORT").unwrap_or_default();
    let persistent = transport.trim() == "persistent-stdio";
    let code = if persistent {
        run_persistent(&op)
    } else {
        run_single(&op)
    };
    Some(std::process::ExitCode::from(code))
}

fn run_single(op: &str) -> u8 {
    let mut stdin = std::io::stdin().lock();
    let mut stdout = std::io::stdout().lock();
    let payload = match read_framed_term(&mut stdin) {
        Ok(Some(payload)) => payload,
        Ok(None) => return 0,
        Err(err) => {
            let _ = write_framed_term(&mut stdout, &error_term("bridge/frame-read", &err));
            return 0;
        }
    };
    let response = dispatch_host_bridge(op, &payload);
    let _ = write_framed_term(&mut stdout, &response);
    0
}

fn run_persistent(op: &str) -> u8 {
    let mut stdin = BufReader::new(std::io::stdin().lock());
    let mut stdout = std::io::stdout().lock();
    loop {
        match read_framed_term(&mut stdin) {
            Ok(Some(payload)) => {
                let response = dispatch_host_bridge(op, &payload);
                if write_framed_term(&mut stdout, &response).is_err() {
                    return 0;
                }
            }
            Ok(None) => return 0,
            Err(err) => {
                let _ = write_framed_term(&mut stdout, &error_term("bridge/frame-read", &err));
                return 0;
            }
        }
    }
}

fn read_framed_term<R: BufRead>(reader: &mut R) -> Result<Option<Term>, String> {
    let mut header = String::new();
    let n = reader.read_line(&mut header).map_err(|e| e.to_string())?;
    if n == 0 {
        return Ok(None);
    }
    let len = header
        .trim()
        .parse::<usize>()
        .map_err(|e| format!("invalid frame header `{}`: {e}", header.trim()))?;
    let mut body = vec![0u8; len];
    reader.read_exact(&mut body).map_err(|e| e.to_string())?;
    let src = String::from_utf8(body).map_err(|e| format!("frame body utf8: {e}"))?;
    let parsed = parse_term(&src).map_err(|e| format!("frame term parse: {e}"))?;
    Ok(Some(parsed))
}

fn write_framed_term<W: Write>(writer: &mut W, term: &Term) -> Result<(), String> {
    let src = print_term(term);
    write!(writer, "{}\n{}", src.len(), src).map_err(|e| e.to_string())?;
    writer.flush().map_err(|e| e.to_string())
}

fn dispatch_host_bridge(op: &str, payload: &Term) -> Term {
    match dispatch_host_bridge_impl(op, payload) {
        Ok(term) => term,
        Err(err) => error_term("bridge/dispatch", &err),
    }
}

fn dispatch_host_bridge_impl(op: &str, payload: &Term) -> Result<Term, String> {
    match op {
        "io/net::http-request" => net_http_request(payload),
        "io/net::dns-resolve" => net_dns_resolve(payload),
        "io/net::tcp-listen" => net_tcp_listen(payload),
        "io/net::tcp-accept" => net_tcp_accept(payload),
        "io/net::tcp-open" => net_tcp_open(payload),
        "io/net::tcp-send" => net_tcp_send(payload),
        "io/net::tcp-recv" => net_tcp_recv(payload),
        "io/net::tcp-close" => net_tcp_close(payload),
        "io/net::udp-bind" => net_udp_bind(payload),
        "io/net::udp-send" => net_udp_send(payload),
        "io/net::udp-recv" => net_udp_recv(payload),
        "io/net::udp-close" => net_udp_close(payload),
        "io/net::http-listen" => net_http_listen(payload),
        "io/net::http-respond" => net_http_respond(payload),
        "io/net::ws-accept" => net_ws_accept(payload),
        "io/net::ws-open" => net_ws_open(payload),
        "io/net::ws-send" => net_ws_send(payload),
        "io/net::ws-recv" => net_ws_recv(payload),
        "io/net::ws-close" => net_ws_close(payload),
        "io/db::connect" => db_connect(payload),
        "io/db::tx-begin" => db_tx_begin(payload),
        "io/db::tx-commit" | "io/db::tx-rollback" => db_tx_finish(op, payload),
        "io/db::query" => db_query(payload),
        "io/db::exec" => db_exec(payload),
        "io/db::kv-open" => db_kv_open(payload),
        "io/db::kv-get" => db_kv_get(payload),
        "io/db::kv-put" => db_kv_put(payload),
        "io/db::kv-delete" => db_kv_delete(payload),
        "sys/process::exec" => process_exec(payload),
        "sys/process::spawn" => process_spawn(payload),
        "sys/process::wait" => process_wait(payload),
        "sys/process::kill" => process_kill(payload),
        "sys/process::stdout-read" => process_read_stream(payload, true),
        "sys/process::stderr-read" => process_read_stream(payload, false),
        "sys/process::stdin-write" => process_stdin_write(payload),
        "core/crypto::hash" => crypto_hash(payload),
        "core/crypto::sign" => crypto_sign(payload),
        "core/crypto::verify" => crypto_verify(payload),
        "core/crypto::kdf" => crypto_kdf(payload),
        "core/crypto::aead-seal" => crypto_aead_seal(payload),
        "core/crypto::aead-open" => crypto_aead_open(payload),
        "host/plugin::command" | "editor/plugin::command" => plugin_command(op, payload),
        "host/ffi::call" => ffi_call(payload),
        "host/ffi::buffer-pin" => ffi_buffer_pin(payload),
        "host/ffi::buffer-unpin" => ffi_buffer_unpin(payload),
        other => Err(format!("unsupported bridge op `{other}`")),
    }
}

fn map_key(key: &str) -> TermOrdKey {
    TermOrdKey(Term::symbol(key))
}

fn as_map(payload: &Term) -> Result<&BTreeMap<TermOrdKey, Term>, String> {
    match payload {
        Term::Map(mm) => Ok(mm),
        _ => Err("payload must be a map".to_string()),
    }
}

fn req_string(payload: &Term, key: &str) -> Result<String, String> {
    let mm = as_map(payload)?;
    let Some(value) = mm.get(&map_key(key)) else {
        return Err(format!("missing `{key}`"));
    };
    match value {
        Term::Str(s) | Term::Symbol(s) => {
            let trimmed = s.trim();
            if trimmed.is_empty() {
                Err(format!("`{key}` must not be empty"))
            } else {
                Ok(trimmed.to_string())
            }
        }
        _ => Err(format!("`{key}` must be string|symbol")),
    }
}

fn req_int(payload: &Term, key: &str) -> Result<i64, String> {
    let mm = as_map(payload)?;
    let Some(value) = mm.get(&map_key(key)) else {
        return Err(format!("missing `{key}`"));
    };
    let Term::Int(value) = value else {
        return Err(format!("`{key}` must be int"));
    };
    value
        .to_i64()
        .ok_or_else(|| format!("`{key}` exceeds i64 range"))
}

fn opt_int(payload: &Term, key: &str) -> Result<Option<i64>, String> {
    let mm = as_map(payload)?;
    let Some(value) = mm.get(&map_key(key)) else {
        return Ok(None);
    };
    match value {
        Term::Nil => Ok(None),
        Term::Int(value) => value
            .to_i64()
            .ok_or_else(|| format!("`{key}` exceeds i64 range"))
            .map(Some),
        _ => Err(format!("`{key}` must be int")),
    }
}

fn req_bytes(payload: &Term, key: &str) -> Result<Vec<u8>, String> {
    let mm = as_map(payload)?;
    let Some(value) = mm.get(&map_key(key)) else {
        return Err(format!("missing `{key}`"));
    };
    match value {
        Term::Bytes(bytes) => Ok(bytes.to_vec()),
        Term::Str(s) => Ok(s.as_bytes().to_vec()),
        _ => Err(format!("`{key}` must be bytes|string")),
    }
}

fn opt_bytes(payload: &Term, key: &str) -> Result<Option<Vec<u8>>, String> {
    let mm = as_map(payload)?;
    let Some(value) = mm.get(&map_key(key)) else {
        return Ok(None);
    };
    match value {
        Term::Nil => Ok(None),
        Term::Bytes(bytes) => Ok(Some(bytes.to_vec())),
        Term::Str(s) => Ok(Some(s.as_bytes().to_vec())),
        _ => Err(format!("`{key}` must be bytes|string")),
    }
}

fn state_root() -> Result<PathBuf, String> {
    let cwd = std::env::current_dir().map_err(|e| e.to_string())?;
    let root = cwd
        .join(".genesis")
        .join("runtime")
        .join("backend")
        .join("state");
    std::fs::create_dir_all(&root).map_err(|e| e.to_string())?;
    Ok(root)
}

struct CounterLock {
    #[cfg(not(target_os = "wasi"))]
    file: std::fs::File,
    marker_file: Option<std::fs::File>,
    marker_path: PathBuf,
}

impl CounterLock {
    fn acquire(path: &Path) -> Result<Self, String> {
        #[cfg(not(target_os = "wasi"))]
        {
            let file = std::fs::OpenOptions::new()
                .create(true)
                .truncate(false)
                .read(true)
                .write(true)
                .open(path)
                .map_err(|e| e.to_string())?;
            file.lock_exclusive().map_err(|e| e.to_string())?;
            let (marker_path, marker_file) = acquire_counter_marker(path)?;
            Ok(Self {
                file,
                marker_file: Some(marker_file),
                marker_path,
            })
        }

        #[cfg(target_os = "wasi")]
        {
            let (marker_path, marker_file) = acquire_counter_marker(path)?;
            Ok(Self {
                marker_file: Some(marker_file),
                marker_path,
            })
        }
    }
}

fn acquire_counter_marker(path: &Path) -> Result<(PathBuf, std::fs::File), String> {
    let mut marker_name = path.as_os_str().to_os_string();
    marker_name.push(".exclusive");
    let marker_path = PathBuf::from(marker_name);
    const RETRIES: u32 = 1_000;
    for _ in 0..RETRIES {
        match std::fs::OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(&marker_path)
        {
            Ok(file) => return Ok((marker_path, file)),
            Err(err) if err.kind() == std::io::ErrorKind::AlreadyExists => {
                std::thread::sleep(Duration::from_millis(10));
            }
            Err(err) => return Err(err.to_string()),
        }
    }
    Err(format!("counter lock busy: {}", path.display()))
}

impl Drop for CounterLock {
    fn drop(&mut self) {
        drop(self.marker_file.take());
        let _ = std::fs::remove_file(&self.marker_path);

        #[cfg(not(target_os = "wasi"))]
        {
            let _ = FileExt::unlock(&self.file);
        }
    }
}

fn next_counter(name: &str) -> Result<u64, String> {
    let root = state_root()?;
    let counter_dir = root.join("counters");
    std::fs::create_dir_all(&counter_dir).map_err(|e| e.to_string())?;
    let _lock = CounterLock::acquire(&counter_dir.join(format!("{name}.lock")))?;
    let path = counter_dir.join(format!("{name}.txt"));
    let current = match std::fs::read_to_string(&path) {
        Ok(src) => src.trim().parse::<u64>().unwrap_or(0),
        Err(_) => 0,
    };
    let next = current.saturating_add(1);
    std::fs::write(&path, format!("{next}\n")).map_err(|e| e.to_string())?;
    Ok(next)
}

fn ok_term(fields: Vec<(&str, Term)>) -> Term {
    let mut mm = BTreeMap::new();
    mm.insert(map_key(":ok"), Term::Bool(true));
    mm.insert(
        map_key(":backend"),
        Term::Str("first-party-backend-bridge".to_string()),
    );
    for (k, v) in fields {
        mm.insert(map_key(k), v);
    }
    Term::Map(mm)
}

fn error_term(code: &str, message: &str) -> Term {
    let mut err = BTreeMap::new();
    err.insert(map_key(":code"), Term::Str(code.to_string()));
    err.insert(map_key(":message"), Term::Str(message.to_string()));
    let mut top = BTreeMap::new();
    top.insert(map_key(":ok"), Term::Bool(false));
    top.insert(map_key(":error"), Term::Map(err));
    Term::Map(top)
}

fn net_http_request(payload: &Term) -> Result<Term, String> {
    let method = req_string(payload, ":method")?;
    let url = req_string(payload, ":url")?;
    let body = opt_bytes(payload, ":body")?.unwrap_or_default();
    if !url.starts_with("http://") {
        return Err("io/net::http-request currently supports `http://` urls only".to_string());
    }
    let no_scheme = &url["http://".len()..];
    let (host_port, path) = match no_scheme.split_once('/') {
        Some((hp, p)) => (hp, format!("/{}", p)),
        None => (no_scheme, "/".to_string()),
    };
    let (host, port) = match host_port.split_once(':') {
        Some((h, p)) => {
            let port = p
                .parse::<u16>()
                .map_err(|e| format!("invalid port in url `{url}`: {e}"))?;
            (h.to_string(), port)
        }
        None => (host_port.to_string(), 80u16),
    };
    let mut addrs = (host.as_str(), port)
        .to_socket_addrs()
        .map_err(|e| format!("resolve `{host}:{port}`: {e}"))?;
    let Some(addr) = addrs.next() else {
        return Err(format!("resolve `{host}:{port}` produced no addresses"));
    };
    let mut stream = TcpStream::connect_timeout(&addr, Duration::from_secs(5))
        .map_err(|e| format!("connect `{addr}`: {e}"))?;
    stream
        .set_read_timeout(Some(Duration::from_secs(5)))
        .map_err(|e| e.to_string())?;
    stream
        .set_write_timeout(Some(Duration::from_secs(5)))
        .map_err(|e| e.to_string())?;
    let request = format!(
        "{} {} HTTP/1.1\r\nHost: {}\r\nConnection: close\r\nContent-Length: {}\r\n\r\n",
        method.to_uppercase(),
        path,
        host,
        body.len()
    );
    stream
        .write_all(request.as_bytes())
        .map_err(|e| format!("write request line+headers: {e}"))?;
    if !body.is_empty() {
        stream
            .write_all(&body)
            .map_err(|e| format!("write request body: {e}"))?;
    }
    let mut response = Vec::new();
    stream
        .read_to_end(&mut response)
        .map_err(|e| format!("read response: {e}"))?;
    let response_src =
        String::from_utf8(response.clone()).map_err(|e| format!("response utf8: {e}"))?;
    let (head, body_src) = response_src
        .split_once("\r\n\r\n")
        .ok_or_else(|| "malformed http response".to_string())?;
    let mut lines = head.lines();
    let status_line = lines
        .next()
        .ok_or_else(|| "missing status line".to_string())?;
    let status = status_line
        .split_whitespace()
        .nth(1)
        .ok_or_else(|| "missing status code".to_string())?
        .parse::<i64>()
        .map_err(|e| format!("status parse: {e}"))?;
    let headers = lines.map(|s| Term::Str(s.to_string())).collect::<Vec<_>>();
    Ok(ok_term(vec![
        (":status", Term::Int(status.into())),
        (":headers", Term::Vector(headers)),
        (":body", Term::Str(body_src.to_string())),
    ]))
}

fn net_dns_resolve(payload: &Term) -> Result<Term, String> {
    let name = req_string(payload, ":name")?;
    let addrs = (name.as_str(), 0)
        .to_socket_addrs()
        .map_err(|e| format!("dns resolve `{name}`: {e}"))?;
    let mut uniq = BTreeSet::new();
    for addr in addrs {
        uniq.insert(addr.ip().to_string());
    }
    Ok(ok_term(vec![(
        ":addrs",
        Term::Vector(uniq.into_iter().map(Term::Str).collect()),
    )]))
}

fn parse_tcp_remote(uri: &str) -> Result<SocketAddr, String> {
    if !uri.starts_with("tcp://") {
        return Err(format!("tcp uri must start with tcp:// (got `{uri}`)"));
    }
    let rest = &uri["tcp://".len()..];
    let mut addrs = rest
        .to_socket_addrs()
        .map_err(|e| format!("resolve `{rest}`: {e}"))?;
    addrs
        .next()
        .ok_or_else(|| format!("resolve `{rest}` produced no address"))
}

fn net_tcp_open(payload: &Term) -> Result<Term, String> {
    let remote = req_string(payload, ":remote")?;
    let addr = parse_tcp_remote(&remote)?;
    let stream =
        TcpStream::connect_timeout(&addr, Duration::from_secs(5)).map_err(|e| e.to_string())?;
    stream
        .set_read_timeout(Some(Duration::from_secs(5)))
        .map_err(|e| e.to_string())?;
    stream
        .set_write_timeout(Some(Duration::from_secs(5)))
        .map_err(|e| e.to_string())?;
    let local = stream.local_addr().map_err(|e| e.to_string())?;
    let stream_id = format!("tcp-{}", next_counter("net_stream")?);
    net_bridge_state()
        .lock()
        .map_err(|_| "net bridge state lock poisoned".to_string())?
        .tcp_streams
        .insert(stream_id.clone(), stream);
    Ok(ok_term(vec![
        (":stream-id", Term::Str(stream_id)),
        (":remote", Term::Str(addr.to_string())),
        (":local", Term::Str(local.to_string())),
    ]))
}

fn parse_bound_socket_addr(uri: &str, scheme: &str) -> Result<SocketAddr, String> {
    let prefix = format!("{scheme}://");
    if !uri.starts_with(&prefix) {
        return Err(format!(
            "{scheme} uri must start with {prefix} (got `{uri}`)"
        ));
    }
    let rest = &uri[prefix.len()..];
    let mut addrs = rest
        .to_socket_addrs()
        .map_err(|e| format!("resolve `{rest}`: {e}"))?;
    addrs
        .next()
        .ok_or_else(|| format!("resolve `{rest}` produced no address"))
}

fn net_tcp_listen(payload: &Term) -> Result<Term, String> {
    let local = req_string(payload, ":local")?;
    let addr = parse_bound_socket_addr(&local, "tcp")?;
    let listener = TcpListener::bind(addr).map_err(|e| format!("tcp bind `{addr}`: {e}"))?;
    let bound_local = listener.local_addr().map_err(|e| e.to_string())?;
    listener
        .set_nonblocking(true)
        .map_err(|e| format!("tcp listener nonblocking `{addr}`: {e}"))?;
    let listener_id = format!("tcpl-{}", next_counter("net_listener")?);
    net_bridge_state()
        .lock()
        .map_err(|_| "net bridge state lock poisoned".to_string())?
        .tcp_listeners
        .insert(listener_id.clone(), listener);
    Ok(ok_term(vec![
        (":listener-id", Term::Str(listener_id)),
        (":local", Term::Str(bound_local.to_string())),
    ]))
}

fn net_tcp_accept(payload: &Term) -> Result<Term, String> {
    let listener_id = req_string(payload, ":listener-id")?;
    let timeout_ms = opt_int(payload, ":timeout-ms")?.unwrap_or(5000);
    if timeout_ms < 0 {
        return Err("`:timeout-ms` must be >= 0".to_string());
    }
    let deadline = std::time::Instant::now()
        .checked_add(Duration::from_millis(timeout_ms as u64))
        .ok_or_else(|| "timeout overflow".to_string())?;
    let mut accepted: Option<(TcpStream, SocketAddr, SocketAddr)> = None;
    while std::time::Instant::now() <= deadline {
        let maybe_accept = {
            let guard = net_bridge_state()
                .lock()
                .map_err(|_| "net bridge state lock poisoned".to_string())?;
            let Some(listener) = guard.tcp_listeners.get(&listener_id) else {
                return Err(
                    "unknown `:listener-id`; use persistent-stdio bridge transport for listener lifecycle"
                        .to_string(),
                );
            };
            match listener.accept() {
                Ok((stream, remote)) => {
                    let local = stream.local_addr().map_err(|e| e.to_string())?;
                    Some((stream, remote, local))
                }
                Err(e) if e.kind() == std::io::ErrorKind::WouldBlock => None,
                Err(e) => return Err(format!("tcp accept `{listener_id}`: {e}")),
            }
        };
        if let Some(tuple) = maybe_accept {
            accepted = Some(tuple);
            break;
        }
        std::thread::sleep(Duration::from_millis(5));
    }
    let Some((stream, remote, local)) = accepted else {
        return Ok(ok_term(vec![
            (":accepted", Term::Bool(false)),
            (":eof", Term::Bool(false)),
        ]));
    };
    stream
        .set_read_timeout(Some(Duration::from_secs(5)))
        .map_err(|e| e.to_string())?;
    stream
        .set_write_timeout(Some(Duration::from_secs(5)))
        .map_err(|e| e.to_string())?;
    let stream_id = format!("tcp-{}", next_counter("net_stream")?);
    net_bridge_state()
        .lock()
        .map_err(|_| "net bridge state lock poisoned".to_string())?
        .tcp_streams
        .insert(stream_id.clone(), stream);
    Ok(ok_term(vec![
        (":accepted", Term::Bool(true)),
        (":stream-id", Term::Str(stream_id)),
        (":remote", Term::Str(remote.to_string())),
        (":local", Term::Str(local.to_string())),
    ]))
}

fn net_tcp_send(payload: &Term) -> Result<Term, String> {
    let stream_id = req_string(payload, ":stream-id")?;
    let data = req_bytes(payload, ":data")?;
    let sent = {
        let mut guard = net_bridge_state()
            .lock()
            .map_err(|_| "net bridge state lock poisoned".to_string())?;
        let Some(stream) = guard.tcp_streams.get_mut(&stream_id) else {
            return Err(
                "unknown `:stream-id`; use persistent-stdio bridge transport for stream lifecycle"
                    .to_string(),
            );
        };
        stream
            .write(&data)
            .map_err(|e| format!("tcp send `{stream_id}`: {e}"))?
    };
    Ok(ok_term(vec![(
        ":sent-bytes",
        Term::Int((sent as i64).into()),
    )]))
}

fn net_tcp_recv(payload: &Term) -> Result<Term, String> {
    let stream_id = req_string(payload, ":stream-id")?;
    let max_bytes = opt_int(payload, ":max-bytes")?.unwrap_or(65536);
    if max_bytes <= 0 {
        return Err("`:max-bytes` must be > 0".to_string());
    }
    let timeout_ms = opt_int(payload, ":timeout-ms")?.unwrap_or(1000);
    if timeout_ms < 0 {
        return Err("`:timeout-ms` must be >= 0".to_string());
    }
    let mut buf = vec![0u8; max_bytes as usize];
    let (read_n, eof) = {
        let mut guard = net_bridge_state()
            .lock()
            .map_err(|_| "net bridge state lock poisoned".to_string())?;
        let Some(stream) = guard.tcp_streams.get_mut(&stream_id) else {
            return Err(
                "unknown `:stream-id`; use persistent-stdio bridge transport for stream lifecycle"
                    .to_string(),
            );
        };
        stream
            .set_read_timeout(Some(Duration::from_millis(timeout_ms as u64)))
            .map_err(|e| format!("tcp recv timeout `{stream_id}`: {e}"))?;
        match stream.read(&mut buf) {
            Ok(0) => (0usize, true),
            Ok(n) => (n, false),
            Err(e)
                if matches!(
                    e.kind(),
                    std::io::ErrorKind::WouldBlock | std::io::ErrorKind::TimedOut
                ) =>
            {
                (0usize, false)
            }
            Err(e) => return Err(format!("tcp recv `{stream_id}`: {e}")),
        }
    };
    buf.truncate(read_n);
    Ok(ok_term(vec![
        (":data", Term::Bytes(buf.into())),
        (":eof", Term::Bool(eof)),
    ]))
}

fn net_tcp_close(payload: &Term) -> Result<Term, String> {
    let stream_id = req_string(payload, ":stream-id")?;
    let stream = net_bridge_state()
        .lock()
        .map_err(|_| "net bridge state lock poisoned".to_string())?
        .tcp_streams
        .remove(&stream_id);
    if let Some(stream) = stream {
        let _ = stream.shutdown(Shutdown::Both);
    }
    Ok(ok_term(vec![(":closed", Term::Bool(true))]))
}

fn net_udp_bind(payload: &Term) -> Result<Term, String> {
    let local = req_string(payload, ":local")?;
    let addr = parse_bound_socket_addr(&local, "udp")?;
    let socket = UdpSocket::bind(addr).map_err(|e| format!("udp bind `{addr}`: {e}"))?;
    let bound_local = socket.local_addr().map_err(|e| e.to_string())?;
    let socket_id = format!("udp-{}", next_counter("net_udp_socket")?);
    net_bridge_state()
        .lock()
        .map_err(|_| "net bridge state lock poisoned".to_string())?
        .udp_sockets
        .insert(socket_id.clone(), socket);
    Ok(ok_term(vec![
        (":socket-id", Term::Str(socket_id)),
        (":local", Term::Str(bound_local.to_string())),
    ]))
}

fn parse_udp_remote(uri: &str) -> Result<SocketAddr, String> {
    parse_bound_socket_addr(uri, "udp")
}

fn net_udp_send(payload: &Term) -> Result<Term, String> {
    let socket_id = req_string(payload, ":socket-id")?;
    let remote = req_string(payload, ":remote")?;
    let remote_addr = parse_udp_remote(&remote)?;
    let data = req_bytes(payload, ":data")?;
    let sent = {
        let mut guard = net_bridge_state()
            .lock()
            .map_err(|_| "net bridge state lock poisoned".to_string())?;
        let Some(socket) = guard.udp_sockets.get_mut(&socket_id) else {
            return Err(
                "unknown `:socket-id`; use persistent-stdio bridge transport for socket lifecycle"
                    .to_string(),
            );
        };
        socket
            .send_to(&data, remote_addr)
            .map_err(|e| format!("udp send `{socket_id}`: {e}"))?
    };
    Ok(ok_term(vec![(
        ":sent-bytes",
        Term::Int((sent as i64).into()),
    )]))
}

fn net_udp_recv(payload: &Term) -> Result<Term, String> {
    let socket_id = req_string(payload, ":socket-id")?;
    let max_bytes = opt_int(payload, ":max-bytes")?.unwrap_or(65536);
    if max_bytes <= 0 {
        return Err("`:max-bytes` must be > 0".to_string());
    }
    let timeout_ms = opt_int(payload, ":timeout-ms")?.unwrap_or(1000);
    if timeout_ms < 0 {
        return Err("`:timeout-ms` must be >= 0".to_string());
    }
    let mut buf = vec![0u8; max_bytes as usize];
    let recv_res = {
        let mut guard = net_bridge_state()
            .lock()
            .map_err(|_| "net bridge state lock poisoned".to_string())?;
        let Some(socket) = guard.udp_sockets.get_mut(&socket_id) else {
            return Err(
                "unknown `:socket-id`; use persistent-stdio bridge transport for socket lifecycle"
                    .to_string(),
            );
        };
        socket
            .set_read_timeout(Some(Duration::from_millis(timeout_ms as u64)))
            .map_err(|e| format!("udp recv timeout `{socket_id}`: {e}"))?;
        socket.recv_from(&mut buf)
    };
    match recv_res {
        Ok((n, from)) => {
            buf.truncate(n);
            Ok(ok_term(vec![
                (":data", Term::Bytes(buf.into())),
                (":from", Term::Str(from.to_string())),
            ]))
        }
        Err(e)
            if matches!(
                e.kind(),
                std::io::ErrorKind::WouldBlock | std::io::ErrorKind::TimedOut
            ) =>
        {
            Ok(ok_term(vec![
                (":data", Term::Bytes(Vec::<u8>::new().into())),
                (":from", Term::Nil),
            ]))
        }
        Err(e) => Err(format!("udp recv `{socket_id}`: {e}")),
    }
}

fn net_udp_close(payload: &Term) -> Result<Term, String> {
    let socket_id = req_string(payload, ":socket-id")?;
    net_bridge_state()
        .lock()
        .map_err(|_| "net bridge state lock poisoned".to_string())?
        .udp_sockets
        .remove(&socket_id);
    Ok(ok_term(vec![(":closed", Term::Bool(true))]))
}

fn db_connection_dir() -> Result<PathBuf, String> {
    let dir = state_root()?.join("db").join("connections");
    std::fs::create_dir_all(&dir).map_err(|e| e.to_string())?;
    Ok(dir)
}

fn sqlite_path_for_target(target: &str) -> Result<PathBuf, String> {
    if let Some(path) = target.strip_prefix("sqlite://") {
        let cwd = std::env::current_dir().map_err(|e| e.to_string())?;
        let p = Path::new(path);
        let full = if p.is_absolute() {
            p.to_path_buf()
        } else {
            cwd.join(p)
        };
        if let Some(parent) = full.parent() {
            std::fs::create_dir_all(parent).map_err(|e| e.to_string())?;
        }
        return Ok(full);
    }
    if let Some(name) = target.strip_prefix("kv://") {
        let root = state_root()?.join("db").join("kv");
        std::fs::create_dir_all(&root).map_err(|e| e.to_string())?;
        let digest = format!("{:x}", Sha256::digest(name.as_bytes()));
        return Ok(root.join(format!("{digest}.sqlite")));
    }
    Err(format!(
        "unsupported db target `{target}` (expected sqlite:// or kv://)"
    ))
}

fn db_connect(payload: &Term) -> Result<Term, String> {
    let target = req_string(payload, ":target")?;
    let sqlite_path = sqlite_path_for_target(&target)?;
    let id_digest = format!(
        "{:x}",
        Sha256::digest(sqlite_path.to_string_lossy().as_bytes())
    );
    let connection_id = format!("db-{}", &id_digest[..12]);
    let conn_dir = db_connection_dir()?;
    std::fs::write(
        conn_dir.join(format!("{connection_id}.path")),
        format!("{}\n", sqlite_path.to_string_lossy()),
    )
    .map_err(|e| e.to_string())?;
    let conn = rusqlite::Connection::open(&sqlite_path).map_err(|e| e.to_string())?;
    conn.execute_batch("PRAGMA journal_mode=WAL; PRAGMA synchronous=NORMAL;")
        .map_err(|e| e.to_string())?;
    Ok(ok_term(vec![
        (":connection-id", Term::Str(connection_id)),
        (":target", Term::Str(target)),
    ]))
}

fn sqlite_path_for_connection(connection_id: &str) -> Result<PathBuf, String> {
    let conn_dir = db_connection_dir()?;
    let path = conn_dir.join(format!("{connection_id}.path"));
    let src = std::fs::read_to_string(&path)
        .map_err(|e| format!("load connection `{connection_id}`: {e}"))?;
    Ok(PathBuf::from(src.trim()))
}

fn db_tx_begin(payload: &Term) -> Result<Term, String> {
    let connection_id = req_string(payload, ":connection-id")?;
    let _ = sqlite_path_for_connection(&connection_id)?;
    let tx_id = format!("tx-{}", next_counter("db_tx")?);
    let tx_dir = state_root()?.join("db").join("tx");
    std::fs::create_dir_all(&tx_dir).map_err(|e| e.to_string())?;
    std::fs::write(tx_dir.join(format!("{tx_id}.connection")), connection_id)
        .map_err(|e| e.to_string())?;
    Ok(ok_term(vec![(":tx-id", Term::Str(tx_id))]))
}

fn db_tx_finish(op: &str, payload: &Term) -> Result<Term, String> {
    let tx_id = req_string(payload, ":tx-id")?;
    let tx_path = state_root()?
        .join("db")
        .join("tx")
        .join(format!("{tx_id}.connection"));
    if tx_path.is_file() {
        let _ = std::fs::remove_file(tx_path);
    }
    let field = if op == "io/db::tx-commit" {
        ":committed"
    } else {
        ":rolled-back"
    };
    Ok(ok_term(vec![(field, Term::Bool(true))]))
}

fn db_query(payload: &Term) -> Result<Term, String> {
    let connection_id = req_string(payload, ":connection-id")?;
    let query = req_string(payload, ":query")?;
    let sqlite_path = sqlite_path_for_connection(&connection_id)?;
    let conn = rusqlite::Connection::open(sqlite_path).map_err(|e| e.to_string())?;
    let mut stmt = conn.prepare(&query).map_err(|e| e.to_string())?;
    let col_count = stmt.column_count();
    let col_names = (0..col_count)
        .map(|idx| {
            stmt.column_name(idx)
                .unwrap_or("col")
                .replace(|c: char| !c.is_ascii_alphanumeric(), "_")
        })
        .collect::<Vec<_>>();
    let mut rows_out = Vec::new();
    let mut rows = stmt.query([]).map_err(|e| e.to_string())?;
    while let Some(row) = rows.next().map_err(|e| e.to_string())? {
        let mut mm = BTreeMap::new();
        for (idx, col_name) in col_names.iter().enumerate().take(col_count) {
            let key = format!(":{}", col_name);
            let value = match row.get_ref(idx).map_err(|e| e.to_string())? {
                rusqlite::types::ValueRef::Null => Term::Nil,
                rusqlite::types::ValueRef::Integer(v) => Term::Int(v.into()),
                rusqlite::types::ValueRef::Real(v) => Term::Str(v.to_string()),
                rusqlite::types::ValueRef::Text(v) => {
                    Term::Str(String::from_utf8_lossy(v).to_string())
                }
                rusqlite::types::ValueRef::Blob(v) => Term::Bytes(v.to_vec().into()),
            };
            mm.insert(map_key(&key), value);
        }
        rows_out.push(Term::Map(mm));
    }
    Ok(ok_term(vec![
        (":rows", Term::Vector(rows_out.clone())),
        (":row-count", Term::Int((rows_out.len() as i64).into())),
    ]))
}

fn db_exec(payload: &Term) -> Result<Term, String> {
    let connection_id = req_string(payload, ":connection-id")?;
    let statement = req_string(payload, ":statement")?;
    let sqlite_path = sqlite_path_for_connection(&connection_id)?;
    let conn = rusqlite::Connection::open(sqlite_path).map_err(|e| e.to_string())?;
    let affected = conn.execute(&statement, []).map_err(|e| e.to_string())?;
    Ok(ok_term(vec![(
        ":affected-rows",
        Term::Int((affected as i64).into()),
    )]))
}

fn kv_store_dir() -> Result<PathBuf, String> {
    let dir = state_root()?.join("db").join("kvstore");
    std::fs::create_dir_all(&dir).map_err(|e| e.to_string())?;
    Ok(dir)
}

fn kv_store_path(store_id: &str) -> Result<PathBuf, String> {
    let store_dir = kv_store_dir()?;
    let marker = store_dir.join(format!("{store_id}.sqlite.path"));
    let src = std::fs::read_to_string(&marker).map_err(|e| e.to_string())?;
    Ok(PathBuf::from(src.trim()))
}

fn ensure_kv_schema(conn: &rusqlite::Connection) -> Result<(), String> {
    conn.execute_batch(
        "CREATE TABLE IF NOT EXISTS kv (
             k TEXT PRIMARY KEY NOT NULL,
             v BLOB NOT NULL
         );",
    )
    .map_err(|e| e.to_string())
}

fn db_kv_open(payload: &Term) -> Result<Term, String> {
    let target = req_string(payload, ":target")?;
    let sqlite_path = sqlite_path_for_target(&target)?;
    let conn = rusqlite::Connection::open(&sqlite_path).map_err(|e| e.to_string())?;
    ensure_kv_schema(&conn)?;
    let digest = format!("{:x}", Sha256::digest(target.as_bytes()));
    let store_id = format!("kv-{}", &digest[..12]);
    let store_dir = kv_store_dir()?;
    std::fs::write(
        store_dir.join(format!("{store_id}.sqlite.path")),
        format!("{}\n", sqlite_path.to_string_lossy()),
    )
    .map_err(|e| e.to_string())?;
    Ok(ok_term(vec![(":store-id", Term::Str(store_id))]))
}

fn db_kv_get(payload: &Term) -> Result<Term, String> {
    let store_id = req_string(payload, ":store-id")?;
    let key = req_string(payload, ":key")?;
    let sqlite_path = kv_store_path(&store_id)?;
    let conn = rusqlite::Connection::open(sqlite_path).map_err(|e| e.to_string())?;
    ensure_kv_schema(&conn)?;
    let mut stmt = conn
        .prepare("SELECT v FROM kv WHERE k = ?1")
        .map_err(|e| e.to_string())?;
    let mut rows = stmt.query([key.clone()]).map_err(|e| e.to_string())?;
    if let Some(row) = rows.next().map_err(|e| e.to_string())? {
        let value: Vec<u8> = row.get(0).map_err(|e| e.to_string())?;
        let value_term = match String::from_utf8(value.clone()) {
            Ok(s) => Term::Str(s),
            Err(_) => Term::Bytes(value.into()),
        };
        return Ok(ok_term(vec![
            (":found", Term::Bool(true)),
            (":value", value_term),
        ]));
    }
    Ok(ok_term(vec![(":found", Term::Bool(false))]))
}

fn db_kv_put(payload: &Term) -> Result<Term, String> {
    let store_id = req_string(payload, ":store-id")?;
    let key = req_string(payload, ":key")?;
    let value = req_bytes(payload, ":value")?;
    let sqlite_path = kv_store_path(&store_id)?;
    let conn = rusqlite::Connection::open(sqlite_path).map_err(|e| e.to_string())?;
    ensure_kv_schema(&conn)?;
    conn.execute(
        "INSERT INTO kv(k,v) VALUES(?1, ?2) ON CONFLICT(k) DO UPDATE SET v=excluded.v",
        rusqlite::params![key, value],
    )
    .map_err(|e| e.to_string())?;
    Ok(ok_term(vec![(":written", Term::Bool(true))]))
}

fn db_kv_delete(payload: &Term) -> Result<Term, String> {
    let store_id = req_string(payload, ":store-id")?;
    let key = req_string(payload, ":key")?;
    let sqlite_path = kv_store_path(&store_id)?;
    let conn = rusqlite::Connection::open(sqlite_path).map_err(|e| e.to_string())?;
    ensure_kv_schema(&conn)?;
    conn.execute("DELETE FROM kv WHERE k = ?1", [key])
        .map_err(|e| e.to_string())?;
    Ok(ok_term(vec![(":deleted", Term::Bool(true))]))
}

fn process_record_dir() -> Result<PathBuf, String> {
    let dir = state_root()?.join("process");
    std::fs::create_dir_all(&dir).map_err(|e| e.to_string())?;
    Ok(dir)
}

fn process_record_path(id: &str) -> Result<PathBuf, String> {
    Ok(process_record_dir()?.join(format!("{id}.json")))
}

#[cfg(unix)]
fn process_runtime_dir(id: &str) -> Result<PathBuf, String> {
    let dir = process_record_dir()?.join("runtime").join(id);
    std::fs::create_dir_all(&dir).map_err(|e| e.to_string())?;
    Ok(dir)
}

fn save_process_record(id: &str, rec: &ProcessRecord) -> Result<(), String> {
    let path = process_record_path(id)?;
    let body = serde_json::to_vec_pretty(rec).map_err(|e| e.to_string())?;
    std::fs::write(path, body).map_err(|e| e.to_string())
}

fn load_process_record(id: &str) -> Result<ProcessRecord, String> {
    let path = process_record_path(id)?;
    let body = std::fs::read(path).map_err(|e| e.to_string())?;
    serde_json::from_slice(&body).map_err(|e| e.to_string())
}

fn process_program_and_args(payload: &Term) -> Result<(String, Vec<String>), String> {
    let program = req_string(payload, ":program")?;
    let mm = as_map(payload)?;
    let args = match mm.get(&map_key(":args")) {
        Some(Term::Vector(v)) => {
            let mut out = Vec::with_capacity(v.len());
            for item in v {
                match item {
                    Term::Str(s) | Term::Symbol(s) => out.push(s.clone()),
                    _ => return Err("`:args` entries must be string|symbol".to_string()),
                }
            }
            out
        }
        Some(Term::Nil) | None => Vec::new(),
        _ => return Err("`:args` must be vector|nil".to_string()),
    };
    Ok((program, args))
}

fn run_process(program: &str, args: &[String]) -> Result<(i64, String, String), String> {
    let output = std::process::Command::new(program)
        .args(args)
        .output()
        .map_err(|e| format!("spawn `{program}`: {e}"))?;
    let exit = output.status.code().unwrap_or(1) as i64;
    let stdout = String::from_utf8_lossy(&output.stdout).to_string();
    let stderr = String::from_utf8_lossy(&output.stderr).to_string();
    Ok((exit, stdout, stderr))
}

#[cfg(unix)]
fn spawn_process_record(
    process_id: &str,
    program: &str,
    args: &[String],
) -> Result<ProcessRecord, String> {
    let runtime_dir = process_runtime_dir(process_id)?;
    let stdout_path = runtime_dir.join("stdout.log");
    let stderr_path = runtime_dir.join("stderr.log");
    let exit_path = runtime_dir.join("exit.code");
    let pid_path = runtime_dir.join("child.pid");
    let stdin_fifo = runtime_dir.join("stdin.fifo");
    let _ = std::fs::remove_file(&stdin_fifo);
    let status = std::process::Command::new("mkfifo")
        .arg(&stdin_fifo)
        .status()
        .map_err(|e| format!("create stdin fifo: {e}"))?;
    if !status.success() {
        return Err("create stdin fifo failed".to_string());
    }

    let shell = r#"child_pid_file="$1"
exit_file="$2"
stdout_file="$3"
stderr_file="$4"
stdin_fifo="$5"
shift 5
exec 3<>"$stdin_fifo"
"$@" <&3 >"$stdout_file" 2>"$stderr_file" &
child="$!"
printf '%s\n' "$child" >"$child_pid_file"
wait "$child"
code="$?"
printf '%s\n' "$code" >"$exit_file"
"#;

    let mut cmd = std::process::Command::new("sh");
    cmd.arg("-c")
        .arg(shell)
        .arg("genesis-bridge-process-wrapper")
        .arg(pid_path.to_string_lossy().to_string())
        .arg(exit_path.to_string_lossy().to_string())
        .arg(stdout_path.to_string_lossy().to_string())
        .arg(stderr_path.to_string_lossy().to_string())
        .arg(stdin_fifo.to_string_lossy().to_string())
        .arg(program);
    for arg in args {
        cmd.arg(arg);
    }
    cmd.stdin(std::process::Stdio::null());
    cmd.stdout(std::process::Stdio::null());
    cmd.stderr(std::process::Stdio::null());
    let supervisor = cmd
        .spawn()
        .map_err(|e| format!("spawn process supervisor for `{program}`: {e}"))?;

    Ok(ProcessRecord {
        exit: None,
        stdout: String::new(),
        stderr: String::new(),
        killed: false,
        stdin_writes: Vec::new(),
        child_pid: None,
        supervisor_pid: Some(supervisor.id()),
        stdin_fifo: Some(stdin_fifo.to_string_lossy().to_string()),
        stdout_path: Some(stdout_path.to_string_lossy().to_string()),
        stderr_path: Some(stderr_path.to_string_lossy().to_string()),
        exit_path: Some(exit_path.to_string_lossy().to_string()),
        pid_path: Some(pid_path.to_string_lossy().to_string()),
    })
}

#[cfg(not(unix))]
fn spawn_process_record(
    _process_id: &str,
    program: &str,
    args: &[String],
) -> Result<ProcessRecord, String> {
    let (exit, stdout, stderr) = run_process(program, args)?;
    Ok(ProcessRecord {
        exit: Some(exit),
        stdout,
        stderr,
        ..ProcessRecord::default()
    })
}

fn read_process_exit_code(rec: &ProcessRecord) -> Result<Option<i64>, String> {
    let Some(path) = rec.exit_path.as_ref() else {
        return Ok(rec.exit);
    };
    if !Path::new(path).is_file() {
        return Ok(rec.exit);
    }
    let src = std::fs::read_to_string(path).map_err(|e| format!("read process exit code: {e}"))?;
    let parsed = src
        .trim()
        .parse::<i64>()
        .map_err(|e| format!("parse process exit code `{}`: {e}", src.trim()))?;
    Ok(Some(parsed))
}

fn read_child_pid_from_file(rec: &ProcessRecord) -> Result<Option<u32>, String> {
    let Some(path) = rec.pid_path.as_ref() else {
        return Ok(rec.child_pid);
    };
    if !Path::new(path).is_file() {
        return Ok(rec.child_pid);
    }
    let src = std::fs::read_to_string(path).map_err(|e| format!("read process pid file: {e}"))?;
    let pid = src
        .trim()
        .parse::<u32>()
        .map_err(|e| format!("parse process pid `{}`: {e}", src.trim()))?;
    Ok(Some(pid))
}

#[cfg(unix)]
fn pid_is_alive(pid: u32) -> bool {
    std::process::Command::new("kill")
        .arg("-0")
        .arg(pid.to_string())
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .status()
        .is_ok_and(|status| status.success())
}

#[cfg(not(unix))]
fn pid_is_alive(_pid: u32) -> bool {
    false
}

#[cfg(unix)]
fn send_term_signal(pid: u32) -> Result<bool, String> {
    let status = std::process::Command::new("kill")
        .arg("-TERM")
        .arg(pid.to_string())
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .status()
        .map_err(|e| format!("kill -TERM {pid}: {e}"))?;
    Ok(status.success())
}

#[cfg(not(unix))]
fn send_term_signal(_pid: u32) -> Result<bool, String> {
    Ok(false)
}

fn refresh_process_runtime_state(rec: &mut ProcessRecord) -> Result<(), String> {
    if rec.child_pid.is_none() {
        rec.child_pid = read_child_pid_from_file(rec)?;
    }
    if rec.exit.is_none() {
        rec.exit = read_process_exit_code(rec)?;
    }
    if rec.exit.is_none()
        && let Some(pid) = rec.child_pid
        && !pid_is_alive(pid)
    {
        rec.exit = read_process_exit_code(rec)?;
        if rec.exit.is_none() && rec.supervisor_pid.is_none_or(|spid| !pid_is_alive(spid)) {
            rec.exit = Some(if rec.killed { 137 } else { 1 });
        }
    }
    Ok(())
}

fn read_process_stream_output(rec: &ProcessRecord, stdout_stream: bool) -> Result<String, String> {
    let path = if stdout_stream {
        rec.stdout_path.as_ref()
    } else {
        rec.stderr_path.as_ref()
    };
    let fallback = if stdout_stream {
        rec.stdout.clone()
    } else {
        rec.stderr.clone()
    };
    let Some(path) = path else {
        return Ok(fallback);
    };
    if !Path::new(path).exists() {
        return Ok(fallback);
    }
    let bytes = std::fs::read(path).map_err(|e| format!("read process stream `{path}`: {e}"))?;
    Ok(String::from_utf8_lossy(&bytes).to_string())
}

fn process_exec(payload: &Term) -> Result<Term, String> {
    let (program, args) = process_program_and_args(payload)?;
    let (exit, stdout, stderr) = run_process(&program, &args)?;
    Ok(ok_term(vec![
        (":exit", Term::Int(exit.into())),
        (":stdout", Term::Str(stdout)),
        (":stderr", Term::Str(stderr)),
    ]))
}

fn process_spawn(payload: &Term) -> Result<Term, String> {
    let (program, args) = process_program_and_args(payload)?;
    let process_id = format!("proc-{}", next_counter("process_id")?);
    let record = spawn_process_record(&process_id, &program, &args)?;
    save_process_record(&process_id, &record)?;
    Ok(ok_term(vec![(":process-id", Term::Str(process_id))]))
}

fn process_wait(payload: &Term) -> Result<Term, String> {
    let process_id = req_string(payload, ":process-id")?;
    let mut rec = load_process_record(&process_id)?;
    while rec.exit.is_none() {
        refresh_process_runtime_state(&mut rec)?;
        if rec.exit.is_some() {
            break;
        }
        std::thread::sleep(Duration::from_millis(20));
    }
    let exit = rec.exit.unwrap_or(1);
    save_process_record(&process_id, &rec)?;
    Ok(ok_term(vec![
        (":exit", Term::Int(exit.into())),
        (":killed", Term::Bool(rec.killed)),
        (":running", Term::Bool(false)),
    ]))
}

fn process_kill(payload: &Term) -> Result<Term, String> {
    let process_id = req_string(payload, ":process-id")?;
    let mut rec = load_process_record(&process_id)?;
    refresh_process_runtime_state(&mut rec)?;
    let mut signalled = false;
    if let Some(pid) = rec.child_pid {
        signalled |= send_term_signal(pid)?;
    }
    if let Some(pid) = rec.supervisor_pid {
        signalled |= send_term_signal(pid)?;
    }
    rec.killed = true;
    if rec.exit.is_none() {
        rec.exit = Some(137);
    }
    save_process_record(&process_id, &rec)?;
    Ok(ok_term(vec![
        (":killed", Term::Bool(true)),
        (":signalled", Term::Bool(signalled)),
    ]))
}

fn process_read_stream(payload: &Term, stdout_stream: bool) -> Result<Term, String> {
    let process_id = req_string(payload, ":process-id")?;
    let mut rec = load_process_record(&process_id)?;
    refresh_process_runtime_state(&mut rec)?;
    let data = read_process_stream_output(&rec, stdout_stream)?;
    save_process_record(&process_id, &rec)?;
    Ok(ok_term(vec![(
        if stdout_stream { ":stdout" } else { ":stderr" },
        Term::Str(data),
    )]))
}

fn process_stdin_write(payload: &Term) -> Result<Term, String> {
    let process_id = req_string(payload, ":process-id")?;
    let data = req_bytes(payload, ":data")?;
    let mut rec = load_process_record(&process_id)?;
    refresh_process_runtime_state(&mut rec)?;
    if rec.exit.is_some() {
        return Err("process already exited".to_string());
    }
    let mut wrote = false;
    if let Some(path) = rec.stdin_fifo.as_ref() {
        let fifo_path = Path::new(path);
        if fifo_path.exists() {
            let mut fifo = std::fs::OpenOptions::new()
                .read(true)
                .write(true)
                .open(fifo_path)
                .map_err(|e| format!("open process stdin fifo `{path}`: {e}"))?;
            fifo.write_all(&data)
                .map_err(|e| format!("write process stdin fifo `{path}`: {e}"))?;
            fifo.flush()
                .map_err(|e| format!("flush process stdin fifo `{path}`: {e}"))?;
            wrote = true;
        }
    }
    rec.stdin_writes
        .push(String::from_utf8_lossy(&data).to_string());
    save_process_record(&process_id, &rec)?;
    Ok(ok_term(vec![
        (":written-bytes", Term::Int((data.len() as i64).into())),
        (":ok", Term::Bool(wrote)),
    ]))
}

#[derive(Debug, Deserialize)]
struct BridgeKeyFile {
    alg: String,
    sk_b64: Option<String>,
    pk_b64: Option<String>,
    key_b64: Option<String>,
}

fn sanitize_key_id(raw: &str) -> String {
    raw.chars()
        .map(|ch| {
            if ch.is_ascii_alphanumeric() || matches!(ch, '_' | '-' | '.') {
                ch
            } else {
                '_'
            }
        })
        .collect()
}

fn decode_b64_vec(raw: &str, field: &str) -> Result<Vec<u8>, String> {
    Base64::decode_vec(raw).map_err(|e| format!("invalid base64 in `{field}`: {e}"))
}

fn decode_b64_fixed<const N: usize>(raw: &str, field: &str) -> Result<[u8; N], String> {
    let bytes = decode_b64_vec(raw, field)?;
    if bytes.len() != N {
        return Err(format!(
            "`{field}` must decode to exactly {N} bytes (got {})",
            bytes.len()
        ));
    }
    let mut out = [0u8; N];
    out.copy_from_slice(&bytes);
    Ok(out)
}

fn crypto_key_dirs() -> Result<Vec<PathBuf>, String> {
    let cwd = std::env::current_dir().map_err(|e| e.to_string())?;
    let mut out = Vec::new();
    if let Ok(raw) = std::env::var("GENESIS_CRYPTO_KEY_DIR") {
        let trimmed = raw.trim();
        if !trimmed.is_empty() {
            let p = PathBuf::from(trimmed);
            out.push(if p.is_absolute() { p } else { cwd.join(p) });
        }
    }
    out.push(
        cwd.join(".genesis")
            .join("runtime")
            .join("backend")
            .join("keys"),
    );
    out.push(cwd.join(".genesis").join("keys"));
    out.push(cwd.join("keys"));
    Ok(out)
}

fn load_key_file(key_id: &str) -> Result<BridgeKeyFile, String> {
    let sanitized = sanitize_key_id(key_id);
    let digest = format!("{:x}", Sha256::digest(key_id.as_bytes()));
    let mut searched = Vec::new();
    for dir in crypto_key_dirs()? {
        let candidates = [
            dir.join(format!("{sanitized}.toml")),
            dir.join(format!("{digest}.toml")),
            dir.join(format!("{sanitized}.key.toml")),
            dir.join(format!("{digest}.key.toml")),
        ];
        for candidate in candidates {
            searched.push(candidate.display().to_string());
            if !candidate.is_file() {
                continue;
            }
            let src = std::fs::read_to_string(&candidate).map_err(|e| {
                format!(
                    "read key file for key-id `{key_id}` at `{}`: {e}",
                    candidate.display()
                )
            })?;
            let parsed: BridgeKeyFile = toml::from_str(&src).map_err(|e| {
                format!(
                    "parse key file for key-id `{key_id}` at `{}`: {e}",
                    candidate.display()
                )
            })?;
            return Ok(parsed);
        }
    }
    Err(format!(
        "missing key material for key-id `{key_id}`; expected one of: {}",
        searched.join(", ")
    ))
}

fn resolve_ed25519_signing_key(key_id: &str) -> Result<SigningKey, String> {
    let key_file = load_key_file(key_id)?;
    if !key_file.alg.eq_ignore_ascii_case("ed25519") {
        return Err(format!(
            "key-id `{key_id}` uses alg `{}` but ed25519 is required",
            key_file.alg
        ));
    }
    let sk_raw = key_file
        .sk_b64
        .as_deref()
        .ok_or_else(|| format!("key-id `{key_id}` is missing `sk_b64`"))?;
    let sk = decode_b64_fixed::<32>(sk_raw, "sk_b64")?;
    Ok(SigningKey::from_bytes(&sk))
}

fn resolve_ed25519_verifying_key(key_id: &str) -> Result<VerifyingKey, String> {
    let key_file = load_key_file(key_id)?;
    if !key_file.alg.eq_ignore_ascii_case("ed25519") {
        return Err(format!(
            "key-id `{key_id}` uses alg `{}` but ed25519 is required",
            key_file.alg
        ));
    }
    if let Some(pk_raw) = key_file.pk_b64.as_deref() {
        let pk = decode_b64_fixed::<32>(pk_raw, "pk_b64")?;
        return VerifyingKey::from_bytes(&pk)
            .map_err(|e| format!("key-id `{key_id}` has invalid ed25519 public key: {e}"));
    }
    let signing = resolve_ed25519_signing_key(key_id)?;
    Ok(signing.verifying_key())
}

fn resolve_symmetric_key_bytes(key_id: &str) -> Result<Vec<u8>, String> {
    let key_file = load_key_file(key_id)?;
    let alg = key_file.alg.to_ascii_lowercase();
    if alg == "ed25519" {
        let signing = resolve_ed25519_signing_key(key_id)?;
        let mut h = Sha256::new();
        h.update(signing.to_bytes());
        return Ok(h.finalize().to_vec());
    }
    let raw = key_file
        .key_b64
        .as_deref()
        .ok_or_else(|| format!("key-id `{key_id}` is missing `key_b64`"))?;
    let decoded = decode_b64_vec(raw, "key_b64")?;
    if decoded.is_empty() {
        return Err(format!("key-id `{key_id}` has empty `key_b64` material"));
    }
    Ok(decoded)
}

fn normalize_32_key(material: &[u8]) -> [u8; 32] {
    if material.len() == 32 {
        let mut out = [0u8; 32];
        out.copy_from_slice(material);
        return out;
    }
    let mut h = Sha256::new();
    h.update(material);
    let digest = h.finalize();
    let mut out = [0u8; 32];
    out.copy_from_slice(&digest[..32]);
    out
}

fn crypto_message_with_context(payload: &Term, key: &str) -> Result<Vec<u8>, String> {
    let message = req_bytes(payload, key)?;
    if let Some(context) = opt_bytes(payload, ":context")? {
        let mut out = Vec::with_capacity(context.len() + 1 + message.len());
        out.extend_from_slice(&context);
        out.push(0);
        out.extend_from_slice(&message);
        Ok(out)
    } else {
        Ok(message)
    }
}

fn crypto_hash(payload: &Term) -> Result<Term, String> {
    let algorithm = req_string(payload, ":algorithm")?.to_ascii_lowercase();
    let data = req_bytes(payload, ":data")?;
    let digest = match algorithm.as_str() {
        "sha256" => Sha256::digest(&data).to_vec(),
        "sha512" => Sha512::digest(&data).to_vec(),
        "blake3" => blake3::hash(&data).as_bytes().to_vec(),
        other => return Err(format!("unsupported hash algorithm `{other}`")),
    };
    Ok(ok_term(vec![
        (":algorithm", Term::Str(algorithm)),
        (":digest", Term::Bytes(digest.into())),
    ]))
}

fn crypto_sign(payload: &Term) -> Result<Term, String> {
    let algorithm = req_string(payload, ":algorithm")?.to_ascii_lowercase();
    let key_id = req_string(payload, ":key-id")?;
    let message = crypto_message_with_context(payload, ":message")?;
    let signature = match algorithm.as_str() {
        "ed25519" => {
            let signing = resolve_ed25519_signing_key(&key_id)?;
            signing.sign(&message).to_bytes().to_vec()
        }
        "hmac-sha256" => {
            let raw = resolve_symmetric_key_bytes(&key_id)?;
            let mut mac = <Hmac<Sha256> as Mac>::new_from_slice(&raw)
                .map_err(|e| format!("hmac key error: {e}"))?;
            mac.update(&message);
            mac.finalize().into_bytes().to_vec()
        }
        other => return Err(format!("unsupported sign algorithm `{other}`")),
    };
    Ok(ok_term(vec![
        (":algorithm", Term::Str(algorithm)),
        (":signature", Term::Bytes(signature.into())),
    ]))
}

fn crypto_verify(payload: &Term) -> Result<Term, String> {
    let algorithm = req_string(payload, ":algorithm")?.to_ascii_lowercase();
    let key_id = req_string(payload, ":key-id")?;
    let message = crypto_message_with_context(payload, ":message")?;
    let signature = req_bytes(payload, ":signature")?;
    let valid = match algorithm.as_str() {
        "ed25519" => {
            if signature.len() != 64 {
                return Err(format!(
                    "ed25519 signature must be 64 bytes (got {})",
                    signature.len()
                ));
            }
            let verify_key = resolve_ed25519_verifying_key(&key_id)?;
            let sig = Signature::from_slice(&signature)
                .map_err(|e| format!("invalid ed25519 signature bytes: {e}"))?;
            verify_key.verify(&message, &sig).is_ok()
        }
        "hmac-sha256" => {
            let raw = resolve_symmetric_key_bytes(&key_id)?;
            let mut mac = <Hmac<Sha256> as Mac>::new_from_slice(&raw)
                .map_err(|e| format!("hmac key error: {e}"))?;
            mac.update(&message);
            mac.verify_slice(&signature).is_ok()
        }
        other => return Err(format!("unsupported verify algorithm `{other}`")),
    };
    Ok(ok_term(vec![(":valid", Term::Bool(valid))]))
}

fn crypto_kdf(payload: &Term) -> Result<Term, String> {
    let algorithm = req_string(payload, ":algorithm")?.to_ascii_lowercase();
    if algorithm != "hkdf-sha256" && algorithm != "sha256-kdf" && algorithm != "blake3-kdf" {
        return Err(format!("unsupported kdf algorithm `{algorithm}`"));
    }
    let key_id = req_string(payload, ":key-id")?;
    let info = req_bytes(payload, ":info")?;
    let out_len = req_int(payload, ":length")?;
    if out_len <= 0 {
        return Err("`:length` must be > 0".to_string());
    }
    let out_len = out_len as usize;
    let ikm = resolve_symmetric_key_bytes(&key_id)?;
    let salt = opt_bytes(payload, ":salt")?;
    let hk = Hkdf::<Sha256>::new(salt.as_deref(), &ikm);
    let mut out = vec![0u8; out_len];
    hk.expand(&info, &mut out)
        .map_err(|_| format!("hkdf output length {out_len} exceeds limits"))?;
    Ok(ok_term(vec![
        (":algorithm", Term::Str(algorithm)),
        (":key", Term::Bytes(out.into())),
        (":length", Term::Int((out_len as i64).into())),
    ]))
}

fn next_crypto_nonce(
    algorithm: &str,
    key_id: &str,
    payload: &[u8],
    aad: &[u8],
) -> Result<[u8; 12], String> {
    let counter = next_counter("crypto_nonce")?;
    let mut h = Sha256::new();
    h.update(algorithm.as_bytes());
    h.update(key_id.as_bytes());
    h.update(counter.to_le_bytes());
    h.update(payload);
    h.update(aad);
    let digest = h.finalize();
    let mut out = [0u8; 12];
    out.copy_from_slice(&digest[..12]);
    Ok(out)
}

fn parse_nonce_12(raw: Vec<u8>) -> Result<[u8; 12], String> {
    if raw.len() != 12 {
        return Err(format!("nonce must be 12 bytes (got {})", raw.len()));
    }
    let mut out = [0u8; 12];
    out.copy_from_slice(&raw);
    Ok(out)
}

fn parse_tag_16(raw: Vec<u8>) -> Result<[u8; 16], String> {
    if raw.len() != 16 {
        return Err(format!("tag must be 16 bytes (got {})", raw.len()));
    }
    let mut out = [0u8; 16];
    out.copy_from_slice(&raw);
    Ok(out)
}

fn crypto_aead_seal(payload: &Term) -> Result<Term, String> {
    let algorithm = req_string(payload, ":algorithm")?.to_ascii_lowercase();
    let key_id = req_string(payload, ":key-id")?;
    let plaintext = req_bytes(payload, ":plaintext")?;
    let aad = opt_bytes(payload, ":aad")?.unwrap_or_default();
    let nonce = match opt_bytes(payload, ":nonce")? {
        Some(raw) => parse_nonce_12(raw)?,
        None => next_crypto_nonce(&algorithm, &key_id, &plaintext, &aad)?,
    };
    let raw_key = resolve_symmetric_key_bytes(&key_id)?;
    let key = normalize_32_key(&raw_key);
    let mut ciphertext = plaintext.clone();
    let tag = match algorithm.as_str() {
        "aes-256-gcm" => {
            let cipher = Aes256Gcm::new_from_slice(&key)
                .map_err(|e| format!("aes-256-gcm key init: {e}"))?;
            cipher
                .encrypt_in_place_detached((&nonce).into(), &aad, &mut ciphertext)
                .map_err(|e| format!("aes-256-gcm seal failed: {e}"))?
                .to_vec()
        }
        "chacha20poly1305" => {
            let cipher = ChaCha20Poly1305::new_from_slice(&key)
                .map_err(|e| format!("chacha20poly1305 key init: {e}"))?;
            cipher
                .encrypt_in_place_detached((&nonce).into(), &aad, &mut ciphertext)
                .map_err(|e| format!("chacha20poly1305 seal failed: {e}"))?
                .to_vec()
        }
        other => return Err(format!("unsupported aead algorithm `{other}`")),
    };
    let mut packed = Vec::with_capacity(12 + 16 + ciphertext.len());
    packed.extend_from_slice(&nonce);
    packed.extend_from_slice(&tag);
    packed.extend_from_slice(&ciphertext);
    Ok(ok_term(vec![
        (":algorithm", Term::Str(algorithm)),
        (":ciphertext", Term::Bytes(packed.into())),
        (":nonce", Term::Bytes(nonce.to_vec().into())),
        (":tag", Term::Bytes(tag.into())),
    ]))
}

fn crypto_aead_open(payload: &Term) -> Result<Term, String> {
    let algorithm = req_string(payload, ":algorithm")?.to_ascii_lowercase();
    let key_id = req_string(payload, ":key-id")?;
    let mut ciphertext = req_bytes(payload, ":ciphertext")?;
    let aad = opt_bytes(payload, ":aad")?.unwrap_or_default();
    let (nonce, tag) = match (opt_bytes(payload, ":nonce")?, opt_bytes(payload, ":tag")?) {
        (Some(nonce_raw), Some(tag_raw)) => (parse_nonce_12(nonce_raw)?, parse_tag_16(tag_raw)?),
        (Some(_), None) | (None, Some(_)) => {
            return Err(
                "`:nonce` and `:tag` must either both be provided or both be omitted".to_string(),
            );
        }
        (None, None) => {
            if ciphertext.len() < 28 {
                return Err(
                    "ciphertext must contain packed nonce+tag+ciphertext when :nonce/:tag are omitted"
                        .to_string(),
                );
            }
            let mut nonce = [0u8; 12];
            nonce.copy_from_slice(&ciphertext[..12]);
            let mut tag = [0u8; 16];
            tag.copy_from_slice(&ciphertext[12..28]);
            ciphertext = ciphertext[28..].to_vec();
            (nonce, tag)
        }
    };
    let raw_key = resolve_symmetric_key_bytes(&key_id)?;
    let key = normalize_32_key(&raw_key);
    let mut plaintext = ciphertext;
    let open_result = match algorithm.as_str() {
        "aes-256-gcm" => {
            let cipher = Aes256Gcm::new_from_slice(&key)
                .map_err(|e| format!("aes-256-gcm key init: {e}"))?;
            cipher.decrypt_in_place_detached((&nonce).into(), &aad, &mut plaintext, (&tag).into())
        }
        "chacha20poly1305" => {
            let cipher = ChaCha20Poly1305::new_from_slice(&key)
                .map_err(|e| format!("chacha20poly1305 key init: {e}"))?;
            cipher.decrypt_in_place_detached((&nonce).into(), &aad, &mut plaintext, (&tag).into())
        }
        other => return Err(format!("unsupported aead algorithm `{other}`")),
    };
    if open_result.is_err() {
        return Ok(error_term(
            "crypto/aead-auth-failed",
            "aead tag verification failed",
        ));
    }
    Ok(ok_term(vec![(":plaintext", Term::Bytes(plaintext.into()))]))
}

#[cfg(test)]
#[path = "host_bridge_runtime_tests.rs"]
mod tests;
