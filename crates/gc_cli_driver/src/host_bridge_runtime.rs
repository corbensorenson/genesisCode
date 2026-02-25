use std::collections::{BTreeMap, BTreeSet};
use std::io::{BufRead, BufReader, Read, Write};
use std::net::{SocketAddr, TcpStream, ToSocketAddrs};
use std::path::{Path, PathBuf};
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use gc_coreform::{Term, TermOrdKey, parse_term, print_term};
use num_traits::cast::ToPrimitive;
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256, Sha512};

#[derive(Debug, Serialize, Deserialize)]
struct ProcessRecord {
    exit: i64,
    stdout: String,
    stderr: String,
    killed: bool,
    stdin_writes: Vec<String>,
}

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
        "io/net::tcp-open" => net_tcp_open(payload),
        "io/net::tcp-send"
        | "io/net::tcp-recv"
        | "io/net::tcp-close"
        | "io/net::tcp-listen"
        | "io/net::tcp-accept"
        | "io/net::udp-bind"
        | "io/net::udp-send"
        | "io/net::udp-recv"
        | "io/net::udp-close"
        | "io/net::ws-open"
        | "io/net::ws-send"
        | "io/net::ws-recv"
        | "io/net::ws-close"
        | "io/net::http-listen"
        | "io/net::http-respond"
        | "io/net::ws-accept" => Err(format!("op `{op}` is not supported by first-party backend bridge")),
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

fn now_stamp() -> u128 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_nanos()
}

fn state_root() -> Result<PathBuf, String> {
    let cwd = std::env::current_dir().map_err(|e| e.to_string())?;
    let root = cwd.join(".genesis").join("runtime").join("backend").join("state");
    std::fs::create_dir_all(&root).map_err(|e| e.to_string())?;
    Ok(root)
}

fn next_counter(name: &str) -> Result<u64, String> {
    let root = state_root()?;
    let counter_dir = root.join("counters");
    std::fs::create_dir_all(&counter_dir).map_err(|e| e.to_string())?;
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
    let status_line = lines.next().ok_or_else(|| "missing status line".to_string())?;
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
    let local = stream.local_addr().map_err(|e| e.to_string())?;
    drop(stream);
    let stream_id = format!("tcp-{}", next_counter("net_stream")?);
    Ok(ok_term(vec![
        (":stream-id", Term::Str(stream_id)),
        (":remote", Term::Str(addr.to_string())),
        (":local", Term::Str(local.to_string())),
    ]))
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
        for idx in 0..col_count {
            let key = format!(":{}", col_names[idx].clone());
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
    let (exit, stdout, stderr) = run_process(&program, &args)?;
    let process_id = format!("proc-{}", next_counter("process_id")?);
    let record = ProcessRecord {
        exit,
        stdout,
        stderr,
        killed: false,
        stdin_writes: Vec::new(),
    };
    save_process_record(&process_id, &record)?;
    Ok(ok_term(vec![(":process-id", Term::Str(process_id))]))
}

fn process_wait(payload: &Term) -> Result<Term, String> {
    let process_id = req_string(payload, ":process-id")?;
    let rec = load_process_record(&process_id)?;
    Ok(ok_term(vec![
        (":exit", Term::Int(rec.exit.into())),
        (":killed", Term::Bool(rec.killed)),
    ]))
}

fn process_kill(payload: &Term) -> Result<Term, String> {
    let process_id = req_string(payload, ":process-id")?;
    let mut rec = load_process_record(&process_id)?;
    rec.killed = true;
    save_process_record(&process_id, &rec)?;
    Ok(ok_term(vec![(":killed", Term::Bool(true))]))
}

fn process_read_stream(payload: &Term, stdout_stream: bool) -> Result<Term, String> {
    let process_id = req_string(payload, ":process-id")?;
    let rec = load_process_record(&process_id)?;
    let data = if stdout_stream { rec.stdout } else { rec.stderr };
    Ok(ok_term(vec![(
        if stdout_stream { ":stdout" } else { ":stderr" },
        Term::Str(data),
    )]))
}

fn process_stdin_write(payload: &Term) -> Result<Term, String> {
    let process_id = req_string(payload, ":process-id")?;
    let data = req_bytes(payload, ":data")?;
    let mut rec = load_process_record(&process_id)?;
    rec.stdin_writes
        .push(String::from_utf8_lossy(&data).to_string());
    save_process_record(&process_id, &rec)?;
    Ok(ok_term(vec![
        (":written-bytes", Term::Int((data.len() as i64).into())),
        (":ok", Term::Bool(true)),
    ]))
}

fn crypto_key(key_id: &str) -> [u8; 32] {
    *blake3::hash(key_id.as_bytes()).as_bytes()
}

fn bytes_xor(input: &[u8], key_stream: &[u8]) -> Vec<u8> {
    input
        .iter()
        .zip(key_stream.iter().cycle())
        .map(|(a, b)| a ^ b)
        .collect()
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
    let message = req_bytes(payload, ":message")?;
    let mut ctx = Sha256::new();
    match algorithm.as_str() {
        "hmac-sha256" | "sha256" => {
            ctx.update(crypto_key(&key_id));
            ctx.update(&message);
            if let Some(extra) = opt_bytes(payload, ":context")? {
                ctx.update(extra);
            }
        }
        other => return Err(format!("unsupported sign algorithm `{other}`")),
    }
    let signature = ctx.finalize().to_vec();
    Ok(ok_term(vec![
        (":algorithm", Term::Str(algorithm)),
        (":signature", Term::Bytes(signature.into())),
    ]))
}

fn crypto_verify(payload: &Term) -> Result<Term, String> {
    let algorithm = req_string(payload, ":algorithm")?.to_ascii_lowercase();
    let key_id = req_string(payload, ":key-id")?;
    let message = req_bytes(payload, ":message")?;
    let signature = req_bytes(payload, ":signature")?;
    let mut ctx = Sha256::new();
    match algorithm.as_str() {
        "hmac-sha256" | "sha256" => {
            ctx.update(crypto_key(&key_id));
            ctx.update(&message);
            if let Some(extra) = opt_bytes(payload, ":context")? {
                ctx.update(extra);
            }
        }
        other => return Err(format!("unsupported verify algorithm `{other}`")),
    }
    let expected = ctx.finalize().to_vec();
    Ok(ok_term(vec![(":valid", Term::Bool(expected == signature))]))
}

fn crypto_kdf(payload: &Term) -> Result<Term, String> {
    let algorithm = req_string(payload, ":algorithm")?.to_ascii_lowercase();
    if algorithm != "blake3-kdf" && algorithm != "sha256-kdf" {
        return Err(format!("unsupported kdf algorithm `{algorithm}`"));
    }
    let key_id = req_string(payload, ":key-id")?;
    let info = req_bytes(payload, ":info")?;
    let out_len = req_int(payload, ":length")?;
    if out_len <= 0 {
        return Err("`:length` must be > 0".to_string());
    }
    let out_len = out_len as usize;
    let mut out = Vec::with_capacity(out_len);
    let mut counter: u64 = 0;
    while out.len() < out_len {
        let mut block = Sha256::new();
        block.update(crypto_key(&key_id));
        block.update(&info);
        if let Some(salt) = opt_bytes(payload, ":salt")? {
            block.update(&salt);
        }
        block.update(counter.to_le_bytes());
        out.extend_from_slice(&block.finalize());
        counter = counter.saturating_add(1);
    }
    out.truncate(out_len);
    Ok(ok_term(vec![(":key", Term::Bytes(out.into()))]))
}

fn crypto_stream_material(key: &[u8], nonce: &[u8], length: usize) -> Vec<u8> {
    let mut out = Vec::with_capacity(length);
    let mut counter: u64 = 0;
    while out.len() < length {
        let mut hasher = Sha256::new();
        hasher.update(key);
        hasher.update(nonce);
        hasher.update(counter.to_le_bytes());
        out.extend_from_slice(&hasher.finalize());
        counter = counter.saturating_add(1);
    }
    out.truncate(length);
    out
}

fn crypto_aead_seal(payload: &Term) -> Result<Term, String> {
    let algorithm = req_string(payload, ":algorithm")?.to_ascii_lowercase();
    if algorithm != "xor-sha256" {
        return Err(format!("unsupported aead algorithm `{algorithm}`"));
    }
    let key_id = req_string(payload, ":key-id")?;
    let plaintext = req_bytes(payload, ":plaintext")?;
    let aad = opt_bytes(payload, ":aad")?.unwrap_or_default();
    let key = crypto_key(&key_id);
    let mut nonce_hasher = Sha256::new();
    nonce_hasher.update(key);
    nonce_hasher.update(now_stamp().to_le_bytes());
    nonce_hasher.update(&plaintext);
    let nonce = nonce_hasher.finalize()[..12].to_vec();
    let ks = crypto_stream_material(&key, &nonce, plaintext.len());
    let cipher = bytes_xor(&plaintext, &ks);
    let mut tag_hasher = Sha256::new();
    tag_hasher.update(key);
    tag_hasher.update(&nonce);
    tag_hasher.update(&aad);
    tag_hasher.update(&cipher);
    let tag = tag_hasher.finalize()[..16].to_vec();
    let mut packed = Vec::with_capacity(12 + 16 + cipher.len());
    packed.extend_from_slice(&nonce);
    packed.extend_from_slice(&tag);
    packed.extend_from_slice(&cipher);
    Ok(ok_term(vec![(":ciphertext", Term::Bytes(packed.into()))]))
}

fn crypto_aead_open(payload: &Term) -> Result<Term, String> {
    let algorithm = req_string(payload, ":algorithm")?.to_ascii_lowercase();
    if algorithm != "xor-sha256" {
        return Err(format!("unsupported aead algorithm `{algorithm}`"));
    }
    let key_id = req_string(payload, ":key-id")?;
    let packed = req_bytes(payload, ":ciphertext")?;
    if packed.len() < 28 {
        return Err("ciphertext is too small".to_string());
    }
    let nonce = &packed[..12];
    let tag = &packed[12..28];
    let cipher = &packed[28..];
    let aad = opt_bytes(payload, ":aad")?.unwrap_or_default();
    let key = crypto_key(&key_id);
    let mut tag_hasher = Sha256::new();
    tag_hasher.update(key);
    tag_hasher.update(nonce);
    tag_hasher.update(&aad);
    tag_hasher.update(cipher);
    let expected = tag_hasher.finalize();
    if expected[..16] != *tag {
        return Ok(error_term("crypto/aead-auth-failed", "aead tag verification failed"));
    }
    let ks = crypto_stream_material(&key, nonce, cipher.len());
    let plaintext = bytes_xor(cipher, &ks);
    Ok(ok_term(vec![(":plaintext", Term::Bytes(plaintext.into()))]))
}

fn plugin_command(op: &str, payload: &Term) -> Result<Term, String> {
    let plugin = req_string(payload, ":plugin")?;
    let command = req_string(payload, ":command")?;
    if plugin == "demo" {
        let status = if op.starts_with("editor/") {
            "editor-ok"
        } else {
            "host-ok"
        };
        return Ok(ok_term(vec![
            (":plugin", Term::Str(plugin)),
            (":command", Term::Str(command)),
            (":status", Term::Str(status.to_string())),
        ]));
    }
    let payload_term = as_map(payload)?
        .get(&map_key(":payload"))
        .cloned()
        .unwrap_or(Term::Nil);
    let payload_src = print_term(&payload_term);
    let output = std::process::Command::new(&plugin)
        .arg(&command)
        .arg(payload_src)
        .output()
        .map_err(|e| format!("spawn plugin `{plugin}` failed: {e}"))?;
    let status = output.status.code().unwrap_or(1);
    Ok(ok_term(vec![
        (":plugin", Term::Str(plugin)),
        (":command", Term::Str(command)),
        (":status-code", Term::Int((status as i64).into())),
        (
            ":stdout",
            Term::Str(String::from_utf8_lossy(&output.stdout).to_string()),
        ),
        (
            ":stderr",
            Term::Str(String::from_utf8_lossy(&output.stderr).to_string()),
        ),
    ]))
}

fn ffi_buffer_dir() -> Result<PathBuf, String> {
    let dir = state_root()?.join("ffi").join("buffers");
    std::fs::create_dir_all(&dir).map_err(|e| e.to_string())?;
    Ok(dir)
}

fn ffi_buffer_pin(payload: &Term) -> Result<Term, String> {
    let _abi_id = req_string(payload, ":abi-id")?;
    let bytes = req_bytes(payload, ":bytes")?;
    let digest = format!("{:x}", Sha256::digest(&bytes));
    let handle = format!("ffi-buffer-{}-{}", &digest[..12], next_counter("ffi_buffer")?);
    let path = ffi_buffer_dir()?.join(format!("{handle}.bin"));
    std::fs::write(&path, bytes).map_err(|e| e.to_string())?;
    Ok(ok_term(vec![(":handle", Term::Str(handle))]))
}

fn ffi_buffer_unpin(payload: &Term) -> Result<Term, String> {
    let _abi_id = req_string(payload, ":abi-id")?;
    let handle = req_string(payload, ":handle")?;
    let path = ffi_buffer_dir()?.join(format!("{handle}.bin"));
    if path.exists() {
        std::fs::remove_file(path).map_err(|e| e.to_string())?;
    }
    Ok(ok_term(vec![(":status", Term::Str("unpinned".to_string()))]))
}

fn ffi_call(payload: &Term) -> Result<Term, String> {
    let abi_id = req_string(payload, ":abi-id")?;
    let _library = req_string(payload, ":library")?;
    let symbol = req_string(payload, ":symbol")?;
    let mm = as_map(payload)?;
    let args = match mm.get(&map_key(":args")) {
        Some(Term::Vector(values)) => values.clone(),
        Some(Term::Nil) | None => Vec::new(),
        _ => return Err("`:args` must be vector|nil".to_string()),
    };

    if abi_id == "libc.v1" && symbol == "strlen" {
        let Some(arg0) = args.first() else {
            return Err("ffi strlen requires one argument".to_string());
        };
        let len = match arg0 {
            Term::Bytes(bytes) => bytes.len() as i64,
            Term::Str(s) => s.len() as i64,
            _ => return Err("ffi strlen arg must be bytes|string".to_string()),
        };
        return Ok(ok_term(vec![(":result", Term::Int(len.into()))]));
    }

    if abi_id == "genesis/ffi.memory.v1" && symbol == "buffer-len" {
        let Some(Term::Str(handle)) = args.first() else {
            return Err("ffi buffer-len requires handle string arg".to_string());
        };
        let path = ffi_buffer_dir()?.join(format!("{handle}.bin"));
        let len = std::fs::metadata(path).map_err(|e| e.to_string())?.len() as i64;
        return Ok(ok_term(vec![(":result", Term::Int(len.into()))]));
    }

    if abi_id == "genesis/ffi.memory.v1" && symbol == "buffer-read" {
        let Some(Term::Str(handle)) = args.first() else {
            return Err("ffi buffer-read requires handle string arg".to_string());
        };
        let path = ffi_buffer_dir()?.join(format!("{handle}.bin"));
        let bytes = std::fs::read(path).map_err(|e| e.to_string())?;
        return Ok(ok_term(vec![(":result", Term::Bytes(bytes.into()))]));
    }

    Err(format!(
        "unsupported ffi call for abi `{abi_id}` symbol `{symbol}`"
    ))
}
