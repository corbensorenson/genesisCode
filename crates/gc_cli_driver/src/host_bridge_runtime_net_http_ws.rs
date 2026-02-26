use super::*;
use sha1::{Digest as Sha1Digest, Sha1};

const WS_ACCEPT_GUID: &str = "258EAFA5-E914-47DA-95CA-C5AB0DC85B11";
const WS_FRAME_MAX_BYTES: usize = 4 * 1024 * 1024;

fn opt_string(payload: &Term, key: &str) -> Result<Option<String>, String> {
    let mm = as_map(payload)?;
    let Some(value) = mm.get(&map_key(key)) else {
        return Ok(None);
    };
    match value {
        Term::Nil => Ok(None),
        Term::Str(s) | Term::Symbol(s) => {
            let trimmed = s.trim();
            if trimmed.is_empty() {
                Ok(None)
            } else {
                Ok(Some(trimmed.to_string()))
            }
        }
        _ => Err(format!("`{key}` must be string|symbol")),
    }
}

fn parse_http_local_target(local: &str) -> Result<SocketAddr, String> {
    parse_bound_socket_addr(local, "http")
}

fn read_http_request_from_stream(
    stream: &mut TcpStream,
    max_request_bytes: usize,
) -> Result<(String, String, Vec<String>, Vec<u8>), String> {
    let mut buf = Vec::<u8>::new();
    let mut hdr_end = None;
    while hdr_end.is_none() {
        let mut byte = [0u8; 1];
        match stream.read(&mut byte) {
            Ok(0) => return Err("http request closed before headers".to_string()),
            Ok(1) => {
                buf.push(byte[0]);
                if buf.len() > max_request_bytes {
                    return Err(format!(
                        "http request exceeds max-request-bytes ({} > {})",
                        buf.len(),
                        max_request_bytes
                    ));
                }
                if buf.len() >= 4 && &buf[buf.len() - 4..] == b"\r\n\r\n" {
                    hdr_end = Some(buf.len());
                }
            }
            Ok(_) => unreachable!(),
            Err(e)
                if matches!(
                    e.kind(),
                    std::io::ErrorKind::WouldBlock | std::io::ErrorKind::TimedOut
                ) =>
            {
                return Err("http request timed out while reading headers".to_string());
            }
            Err(e) => return Err(format!("http request read: {e}")),
        }
    }
    let hdr_end = hdr_end.ok_or_else(|| "missing http header terminator".to_string())?;
    let head_bytes = &buf[..hdr_end];
    let head_src = String::from_utf8(head_bytes.to_vec()).map_err(|e| format!("http utf8: {e}"))?;
    let mut lines = head_src.split("\r\n").filter(|line| !line.is_empty());
    let request_line = lines
        .next()
        .ok_or_else(|| "missing http request line".to_string())?;
    let mut req_parts = request_line.split_whitespace();
    let method = req_parts
        .next()
        .ok_or_else(|| "missing http method".to_string())?
        .to_string();
    let path = req_parts
        .next()
        .ok_or_else(|| "missing http path".to_string())?
        .to_string();
    let headers = lines.map(ToString::to_string).collect::<Vec<_>>();

    let content_len = headers
        .iter()
        .find_map(|line| {
            let (k, v) = line.split_once(':')?;
            if k.trim().eq_ignore_ascii_case("content-length") {
                v.trim().parse::<usize>().ok()
            } else {
                None
            }
        })
        .unwrap_or(0usize);

    let mut body = buf[hdr_end..].to_vec();
    while body.len() < content_len {
        let mut chunk = vec![0u8; content_len - body.len()];
        let n = stream
            .read(&mut chunk)
            .map_err(|e| format!("http request body read: {e}"))?;
        if n == 0 {
            break;
        }
        body.extend_from_slice(&chunk[..n]);
        if hdr_end + body.len() > max_request_bytes {
            return Err(format!(
                "http request exceeds max-request-bytes ({} > {})",
                hdr_end + body.len(),
                max_request_bytes
            ));
        }
    }
    Ok((method, path, headers, body))
}

fn parse_http_response_headers(stream: &mut TcpStream) -> Result<(i64, Vec<String>), String> {
    let mut buf = Vec::<u8>::new();
    let mut hdr_end = None;
    while hdr_end.is_none() {
        let mut byte = [0u8; 1];
        match stream.read(&mut byte) {
            Ok(0) => return Err("http response closed before headers".to_string()),
            Ok(1) => {
                buf.push(byte[0]);
                if buf.len() > 64 * 1024 {
                    return Err("http response headers exceeded 65536 bytes".to_string());
                }
                if buf.len() >= 4 && &buf[buf.len() - 4..] == b"\r\n\r\n" {
                    hdr_end = Some(buf.len());
                }
            }
            Ok(_) => unreachable!(),
            Err(e)
                if matches!(
                    e.kind(),
                    std::io::ErrorKind::WouldBlock | std::io::ErrorKind::TimedOut
                ) =>
            {
                return Err("http response timed out while reading headers".to_string());
            }
            Err(e) => return Err(format!("http response read: {e}")),
        }
    }
    let head_src =
        String::from_utf8(buf).map_err(|e| format!("http response utf8 decode error: {e}"))?;
    let mut lines = head_src
        .split("\r\n")
        .filter(|line| !line.is_empty())
        .collect::<Vec<_>>();
    if lines.is_empty() {
        return Err("missing http status line".to_string());
    }
    let status_line = lines.remove(0);
    let status = status_line
        .split_whitespace()
        .nth(1)
        .ok_or_else(|| "missing status code".to_string())?
        .parse::<i64>()
        .map_err(|e| format!("status parse: {e}"))?;
    Ok((status, lines.into_iter().map(ToString::to_string).collect()))
}

fn http_header_value<'a>(headers: &'a [String], name: &str) -> Option<&'a str> {
    headers.iter().find_map(|line| {
        let (k, v) = line.split_once(':')?;
        if k.trim().eq_ignore_ascii_case(name) {
            Some(v.trim())
        } else {
            None
        }
    })
}

fn header_has_token_ci(headers: &[String], name: &str, token: &str) -> bool {
    let Some(value) = http_header_value(headers, name) else {
        return false;
    };
    value
        .split(',')
        .any(|part| part.trim().eq_ignore_ascii_case(token))
}

fn normalize_http_headers_for_response(term: Option<&Term>) -> Result<Vec<String>, String> {
    let Some(term) = term else {
        return Ok(Vec::new());
    };
    match term {
        Term::Nil => Ok(Vec::new()),
        Term::Vector(values) => {
            let mut out = Vec::with_capacity(values.len());
            for value in values {
                match value {
                    Term::Str(line) | Term::Symbol(line) => {
                        let trimmed = line.trim();
                        if !trimmed.is_empty() {
                            out.push(trimmed.to_string());
                        }
                    }
                    _ => return Err("`:headers` vector entries must be string|symbol".to_string()),
                }
            }
            Ok(out)
        }
        Term::Map(map) => {
            let mut out = Vec::with_capacity(map.len());
            for (k, v) in map {
                let key = match &k.0 {
                    Term::Str(s) | Term::Symbol(s) => s.trim(),
                    _ => return Err("`:headers` map keys must be string|symbol".to_string()),
                };
                let value = match v {
                    Term::Str(s) | Term::Symbol(s) => s.trim(),
                    Term::Int(i) => {
                        let rendered = i.to_string();
                        out.push(format!("{key}: {rendered}"));
                        continue;
                    }
                    _ => return Err("`:headers` map values must be string|symbol|int".to_string()),
                };
                out.push(format!("{key}: {value}"));
            }
            Ok(out)
        }
        _ => Err("`:headers` must be map|vector|nil".to_string()),
    }
}

fn http_reason_phrase(status: i64) -> &'static str {
    match status {
        101 => "Switching Protocols",
        200 => "OK",
        201 => "Created",
        202 => "Accepted",
        204 => "No Content",
        400 => "Bad Request",
        401 => "Unauthorized",
        403 => "Forbidden",
        404 => "Not Found",
        409 => "Conflict",
        500 => "Internal Server Error",
        502 => "Bad Gateway",
        503 => "Service Unavailable",
        _ => "OK",
    }
}

fn parse_ws_url(url: &str) -> Result<(String, u16, String), String> {
    if !url.starts_with("ws://") {
        return Err("io/net::ws-open currently supports `ws://` urls only".to_string());
    }
    let no_scheme = &url["ws://".len()..];
    let (host_port, path) = match no_scheme.split_once('/') {
        Some((hp, p)) => (hp, format!("/{}", p)),
        None => (no_scheme, "/".to_string()),
    };
    let (host, port) = match host_port.split_once(':') {
        Some((h, p)) => (
            h.to_string(),
            p.parse::<u16>()
                .map_err(|e| format!("invalid ws port `{p}`: {e}"))?,
        ),
        None => (host_port.to_string(), 80u16),
    };
    Ok((host, port, path))
}

fn ws_accept_for_key(key: &str) -> String {
    let mut hasher = Sha1::new();
    hasher.update(key.as_bytes());
    hasher.update(WS_ACCEPT_GUID.as_bytes());
    let digest = hasher.finalize();
    Base64::encode_string(&digest)
}

fn read_ws_frame(stream: &mut TcpStream) -> Result<Option<(u8, Vec<u8>)>, String> {
    let mut first2 = [0u8; 2];
    match stream.read_exact(&mut first2) {
        Ok(()) => {}
        Err(e)
            if matches!(
                e.kind(),
                std::io::ErrorKind::WouldBlock | std::io::ErrorKind::TimedOut
            ) =>
        {
            return Ok(None);
        }
        Err(e) => return Err(format!("ws read header: {e}")),
    }
    let opcode = first2[0] & 0x0f;
    let masked = (first2[1] & 0x80) != 0;
    let mut payload_len = (first2[1] & 0x7f) as usize;
    if payload_len == 126 {
        let mut ext = [0u8; 2];
        stream
            .read_exact(&mut ext)
            .map_err(|e| format!("ws read ext-len16: {e}"))?;
        payload_len = u16::from_be_bytes(ext) as usize;
    } else if payload_len == 127 {
        let mut ext = [0u8; 8];
        stream
            .read_exact(&mut ext)
            .map_err(|e| format!("ws read ext-len64: {e}"))?;
        let wide = u64::from_be_bytes(ext);
        payload_len = usize::try_from(wide).map_err(|_| "ws payload too large".to_string())?;
    }
    if payload_len > WS_FRAME_MAX_BYTES {
        return Err(format!(
            "ws frame payload exceeds limit ({} > {})",
            payload_len, WS_FRAME_MAX_BYTES
        ));
    }
    let mut mask = [0u8; 4];
    if masked {
        stream
            .read_exact(&mut mask)
            .map_err(|e| format!("ws read mask: {e}"))?;
    }
    let mut payload = vec![0u8; payload_len];
    if payload_len > 0 {
        stream
            .read_exact(&mut payload)
            .map_err(|e| format!("ws read payload: {e}"))?;
    }
    if masked {
        for (idx, byte) in payload.iter_mut().enumerate() {
            *byte ^= mask[idx % 4];
        }
    }
    Ok(Some((opcode, payload)))
}

fn write_ws_frame(
    stream: &mut TcpStream,
    opcode: u8,
    data: &[u8],
    mask_outgoing: bool,
) -> Result<(), String> {
    if data.len() > WS_FRAME_MAX_BYTES {
        return Err(format!(
            "ws frame payload exceeds limit ({} > {})",
            data.len(),
            WS_FRAME_MAX_BYTES
        ));
    }
    let mut frame = Vec::<u8>::with_capacity(data.len() + 14);
    frame.push(0x80 | (opcode & 0x0f));
    let mask_bit = if mask_outgoing { 0x80 } else { 0x00 };
    match data.len() {
        n if n < 126 => frame.push(mask_bit | (n as u8)),
        n if n <= 0xffff => {
            frame.push(mask_bit | 126);
            frame.extend_from_slice(&(n as u16).to_be_bytes());
        }
        n => {
            frame.push(mask_bit | 127);
            frame.extend_from_slice(&(n as u64).to_be_bytes());
        }
    }
    if mask_outgoing {
        let mask = [0x13u8, 0x57, 0x9b, 0xdf];
        frame.extend_from_slice(&mask);
        for (idx, byte) in data.iter().copied().enumerate() {
            frame.push(byte ^ mask[idx % 4]);
        }
    } else {
        frame.extend_from_slice(data);
    }
    stream
        .write_all(&frame)
        .map_err(|e| format!("ws frame write: {e}"))?;
    stream.flush().map_err(|e| format!("ws frame flush: {e}"))?;
    Ok(())
}

pub(super) fn net_http_listen(payload: &Term) -> Result<Term, String> {
    let local = req_string(payload, ":local")?;
    let max_request_bytes = req_int(payload, ":max-request-bytes")?;
    if max_request_bytes <= 0 {
        return Err("`:max-request-bytes` must be > 0".to_string());
    }
    let requested_listener_id = opt_string(payload, ":listener-id")?;
    let (listener_id, local_uri, accepted) = {
        let mut state = net_bridge_state()
            .lock()
            .map_err(|_| "net bridge state lock poisoned".to_string())?;
        let resolved_id = if let Some(listener_id) = requested_listener_id.as_ref() {
            if !state.http_listeners.contains_key(listener_id) {
                return Err(format!("unknown `:listener-id` `{listener_id}`"));
            }
            listener_id.clone()
        } else if let Some(existing) = state.http_listener_by_local.get(&local) {
            existing.clone()
        } else {
            let addr = parse_http_local_target(&local)?;
            let listener =
                TcpListener::bind(addr).map_err(|e| format!("http bind `{addr}`: {e}"))?;
            listener
                .set_nonblocking(true)
                .map_err(|e| format!("http listener nonblocking `{addr}`: {e}"))?;
            let local_uri = format!(
                "http://{}",
                listener.local_addr().map_err(|e| e.to_string())?
            );
            let listener_id = format!("http-{}", next_counter("net_http_listener")?);
            state
                .http_listener_by_local
                .insert(local.clone(), listener_id.clone());
            state
                .http_listener_by_local
                .insert(local_uri.clone(), listener_id.clone());
            state.http_listeners.insert(listener_id.clone(), listener);
            listener_id
        };
        let listener = state
            .http_listeners
            .get(&resolved_id)
            .ok_or_else(|| "resolved listener missing".to_string())?;
        let local_uri = format!(
            "http://{}",
            listener.local_addr().map_err(|e| e.to_string())?
        );
        let accepted = match listener.accept() {
            Ok((stream, remote)) => Some((stream, remote)),
            Err(e) if e.kind() == std::io::ErrorKind::WouldBlock => None,
            Err(e) => return Err(format!("http accept `{resolved_id}`: {e}")),
        };
        (resolved_id, local_uri, accepted)
    };

    let Some((mut stream, remote)) = accepted else {
        return Ok(ok_term(vec![
            (":listener-id", Term::Str(listener_id)),
            (":local", Term::Str(local_uri)),
            (":accepted", Term::Bool(false)),
            (":request-id", Term::Nil),
        ]));
    };
    stream
        .set_nonblocking(false)
        .map_err(|e| format!("http accepted stream blocking mode: {e}"))?;
    stream
        .set_read_timeout(Some(Duration::from_secs(2)))
        .map_err(|e| format!("http accepted stream timeout: {e}"))?;
    let (method, path, headers, body) =
        read_http_request_from_stream(&mut stream, max_request_bytes as usize)?;
    let request_id = format!("http-req-{}", next_counter("net_http_request")?);
    let pending = HttpPendingRequest {
        listener_id: listener_id.clone(),
        stream,
        headers: headers.clone(),
    };
    net_bridge_state()
        .lock()
        .map_err(|_| "net bridge state lock poisoned".to_string())?
        .http_requests
        .insert(request_id.clone(), pending);
    Ok(ok_term(vec![
        (":listener-id", Term::Str(listener_id)),
        (":local", Term::Str(local_uri)),
        (":accepted", Term::Bool(true)),
        (":request-id", Term::Str(request_id)),
        (":method", Term::Str(method)),
        (":path", Term::Str(path)),
        (
            ":headers",
            Term::Vector(headers.into_iter().map(Term::Str).collect()),
        ),
        (":body", Term::Bytes(body.into())),
        (":remote", Term::Str(remote.to_string())),
    ]))
}

pub(super) fn net_http_respond(payload: &Term) -> Result<Term, String> {
    let listener_id = req_string(payload, ":listener-id")?;
    let request_id = req_string(payload, ":request-id")?;
    let status = req_int(payload, ":status")?;
    let body = opt_bytes(payload, ":body")?.unwrap_or_default();
    let header_term = as_map(payload)?
        .get(&map_key(":headers"))
        .cloned()
        .unwrap_or(Term::Nil);
    let mut headers = normalize_http_headers_for_response(Some(&header_term))?;
    let mut pending = net_bridge_state()
        .lock()
        .map_err(|_| "net bridge state lock poisoned".to_string())?
        .http_requests
        .remove(&request_id)
        .ok_or_else(|| format!("unknown `:request-id` `{request_id}`"))?;
    if pending.listener_id != listener_id {
        return Err(format!(
            "request `{request_id}` does not belong to listener `{listener_id}`"
        ));
    }
    if !headers.iter().any(|line| {
        line.split(':')
            .next()
            .is_some_and(|k| k.trim().eq_ignore_ascii_case("content-length"))
    }) {
        headers.push(format!("Content-Length: {}", body.len()));
    }
    if !headers.iter().any(|line| {
        line.split(':')
            .next()
            .is_some_and(|k| k.trim().eq_ignore_ascii_case("connection"))
    }) {
        headers.push("Connection: close".to_string());
    }
    let mut response =
        format!("HTTP/1.1 {} {}\r\n", status, http_reason_phrase(status)).into_bytes();
    for line in headers {
        response.extend_from_slice(line.as_bytes());
        response.extend_from_slice(b"\r\n");
    }
    response.extend_from_slice(b"\r\n");
    response.extend_from_slice(&body);
    pending
        .stream
        .write_all(&response)
        .map_err(|e| format!("http respond write `{request_id}`: {e}"))?;
    let _ = pending.stream.flush();
    let _ = pending.stream.shutdown(Shutdown::Both);
    Ok(ok_term(vec![
        (":responded", Term::Bool(true)),
        (":status", Term::Int(status.into())),
        (":bytes", Term::Int((response.len() as i64).into())),
    ]))
}

pub(super) fn net_ws_accept(payload: &Term) -> Result<Term, String> {
    let listener_id = req_string(payload, ":listener-id")?;
    let request_id = req_string(payload, ":request-id")?;
    let max_request_bytes = req_int(payload, ":max-request-bytes")?;
    if max_request_bytes <= 0 {
        return Err("`:max-request-bytes` must be > 0".to_string());
    }
    let mut pending = net_bridge_state()
        .lock()
        .map_err(|_| "net bridge state lock poisoned".to_string())?
        .http_requests
        .remove(&request_id)
        .ok_or_else(|| format!("unknown `:request-id` `{request_id}`"))?;
    if pending.listener_id != listener_id {
        return Err(format!(
            "request `{request_id}` does not belong to listener `{listener_id}`"
        ));
    }
    if pending
        .headers
        .iter()
        .map(|line| line.len() + 2)
        .sum::<usize>()
        > max_request_bytes as usize
    {
        return Err("ws accept request exceeded max-request-bytes".to_string());
    }
    if !header_has_token_ci(&pending.headers, "connection", "upgrade")
        || !http_header_value(&pending.headers, "upgrade")
            .is_some_and(|v| v.eq_ignore_ascii_case("websocket"))
    {
        return Err("ws accept requires Upgrade: websocket request".to_string());
    }
    let key = http_header_value(&pending.headers, "sec-websocket-key")
        .ok_or_else(|| "missing Sec-WebSocket-Key".to_string())?;
    let accept = ws_accept_for_key(key);
    let response = format!(
        "HTTP/1.1 101 Switching Protocols\r\nUpgrade: websocket\r\nConnection: Upgrade\r\nSec-WebSocket-Accept: {accept}\r\n\r\n"
    );
    pending
        .stream
        .write_all(response.as_bytes())
        .map_err(|e| format!("ws accept write handshake: {e}"))?;
    pending
        .stream
        .flush()
        .map_err(|e| format!("ws accept flush handshake: {e}"))?;
    pending
        .stream
        .set_read_timeout(Some(Duration::from_secs(5)))
        .map_err(|e| e.to_string())?;
    pending
        .stream
        .set_write_timeout(Some(Duration::from_secs(5)))
        .map_err(|e| e.to_string())?;
    let stream_id = format!("ws-{}", next_counter("net_ws_stream")?);
    net_bridge_state()
        .lock()
        .map_err(|_| "net bridge state lock poisoned".to_string())?
        .ws_streams
        .insert(
            stream_id.clone(),
            WsStream {
                stream: pending.stream,
                mask_outgoing: false,
            },
        );
    Ok(ok_term(vec![
        (":accepted", Term::Bool(true)),
        (":stream-id", Term::Str(stream_id)),
        (":request-id", Term::Str(request_id)),
    ]))
}

pub(super) fn net_ws_open(payload: &Term) -> Result<Term, String> {
    let url = req_string(payload, ":url")?;
    let (host, port, path) = parse_ws_url(&url)?;
    let mut addrs = (host.as_str(), port)
        .to_socket_addrs()
        .map_err(|e| format!("resolve `{host}:{port}`: {e}"))?;
    let addr = addrs
        .next()
        .ok_or_else(|| format!("resolve `{host}:{port}` produced no addresses"))?;
    let mut stream = TcpStream::connect_timeout(&addr, Duration::from_secs(5))
        .map_err(|e| format!("connect ws `{addr}`: {e}"))?;
    stream
        .set_read_timeout(Some(Duration::from_secs(5)))
        .map_err(|e| e.to_string())?;
    stream
        .set_write_timeout(Some(Duration::from_secs(5)))
        .map_err(|e| e.to_string())?;
    let nonce = format!("genesis-ws-key-{}", next_counter("net_ws_open_key")?);
    let digest = Sha256::digest(nonce.as_bytes());
    let key = Base64::encode_string(&digest[..16]);
    let request = format!(
        "GET {path} HTTP/1.1\r\nHost: {host}:{port}\r\nUpgrade: websocket\r\nConnection: Upgrade\r\nSec-WebSocket-Key: {key}\r\nSec-WebSocket-Version: 13\r\n\r\n"
    );
    stream
        .write_all(request.as_bytes())
        .map_err(|e| format!("ws open write handshake: {e}"))?;
    let (status, headers) = parse_http_response_headers(&mut stream)?;
    if status != 101 {
        return Err(format!("ws open expected HTTP 101, got {status}"));
    }
    let expected_accept = ws_accept_for_key(&key);
    let observed_accept = http_header_value(&headers, "sec-websocket-accept")
        .ok_or_else(|| "ws open missing Sec-WebSocket-Accept".to_string())?;
    if observed_accept != expected_accept {
        return Err("ws open Sec-WebSocket-Accept mismatch".to_string());
    }
    let stream_id = format!("ws-{}", next_counter("net_ws_stream")?);
    net_bridge_state()
        .lock()
        .map_err(|_| "net bridge state lock poisoned".to_string())?
        .ws_streams
        .insert(
            stream_id.clone(),
            WsStream {
                stream,
                mask_outgoing: true,
            },
        );
    Ok(ok_term(vec![
        (":stream-id", Term::Str(stream_id)),
        (":status", Term::Int(status.into())),
    ]))
}

pub(super) fn net_ws_send(payload: &Term) -> Result<Term, String> {
    let stream_id = req_string(payload, ":stream-id")?;
    let data = req_bytes(payload, ":data")?;
    let mut state = net_bridge_state()
        .lock()
        .map_err(|_| "net bridge state lock poisoned".to_string())?;
    let Some(ws) = state.ws_streams.get_mut(&stream_id) else {
        return Err(
            "unknown `:stream-id`; use persistent-stdio bridge transport for ws lifecycle"
                .to_string(),
        );
    };
    write_ws_frame(&mut ws.stream, 0x2, &data, ws.mask_outgoing)?;
    Ok(ok_term(vec![(
        ":sent-bytes",
        Term::Int((data.len() as i64).into()),
    )]))
}

pub(super) fn net_ws_recv(payload: &Term) -> Result<Term, String> {
    let stream_id = req_string(payload, ":stream-id")?;
    let timeout_ms = opt_int(payload, ":timeout-ms")?.unwrap_or(1000);
    if timeout_ms < 0 {
        return Err("`:timeout-ms` must be >= 0".to_string());
    }
    let mut state = net_bridge_state()
        .lock()
        .map_err(|_| "net bridge state lock poisoned".to_string())?;
    let Some(ws) = state.ws_streams.get_mut(&stream_id) else {
        return Err(
            "unknown `:stream-id`; use persistent-stdio bridge transport for ws lifecycle"
                .to_string(),
        );
    };
    ws.stream
        .set_read_timeout(Some(Duration::from_millis(timeout_ms as u64)))
        .map_err(|e| e.to_string())?;
    for _ in 0..4 {
        let Some((opcode, data)) = read_ws_frame(&mut ws.stream)? else {
            return Ok(ok_term(vec![
                (":data", Term::Bytes(Vec::<u8>::new().into())),
                (":eof", Term::Bool(false)),
            ]));
        };
        match opcode {
            0x8 => {
                return Ok(ok_term(vec![
                    (":data", Term::Bytes(Vec::<u8>::new().into())),
                    (":eof", Term::Bool(true)),
                ]));
            }
            0x9 => {
                write_ws_frame(&mut ws.stream, 0xA, &data, ws.mask_outgoing)?;
            }
            0x1 | 0x2 => {
                return Ok(ok_term(vec![
                    (":data", Term::Bytes(data.into())),
                    (":eof", Term::Bool(false)),
                ]));
            }
            _ => {}
        }
    }
    Ok(ok_term(vec![
        (":data", Term::Bytes(Vec::<u8>::new().into())),
        (":eof", Term::Bool(false)),
    ]))
}

pub(super) fn net_ws_close(payload: &Term) -> Result<Term, String> {
    let stream_id = req_string(payload, ":stream-id")?;
    let mut stream = net_bridge_state()
        .lock()
        .map_err(|_| "net bridge state lock poisoned".to_string())?
        .ws_streams
        .remove(&stream_id);
    if let Some(ref mut ws) = stream {
        let _ = write_ws_frame(&mut ws.stream, 0x8, &[], ws.mask_outgoing);
        let _ = ws.stream.shutdown(Shutdown::Both);
    }
    Ok(ok_term(vec![(":closed", Term::Bool(true))]))
}
