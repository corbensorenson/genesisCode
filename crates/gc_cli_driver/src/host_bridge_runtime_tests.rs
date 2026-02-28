use std::collections::BTreeMap;
#[cfg(unix)]
use std::os::unix::fs::PermissionsExt;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Mutex, OnceLock};

use base64ct::{Base64, Encoding};
use ed25519_dalek::SigningKey;
use gc_coreform::{Term, TermOrdKey};

use super::*;

fn term_map(entries: Vec<(&str, Term)>) -> Term {
    let mut mm = BTreeMap::new();
    for (k, v) in entries {
        mm.insert(TermOrdKey(Term::symbol(k)), v);
    }
    Term::Map(mm)
}

fn test_lock() -> &'static Mutex<()> {
    static LOCK: OnceLock<Mutex<()>> = OnceLock::new();
    LOCK.get_or_init(|| Mutex::new(()))
}

fn next_temp_root() -> PathBuf {
    static COUNTER: AtomicU64 = AtomicU64::new(0);
    let n = COUNTER.fetch_add(1, Ordering::Relaxed);
    std::env::temp_dir().join(format!(
        "genesis_host_bridge_runtime_tests_{}_{}",
        std::process::id(),
        n
    ))
}

fn with_test_workspace<F>(f: F)
where
    F: FnOnce(&Path),
{
    let _guard = test_lock().lock().expect("lock test cwd");
    let old = std::env::current_dir().expect("current dir");
    let root = next_temp_root();
    if root.exists() {
        std::fs::remove_dir_all(&root).expect("cleanup previous temp dir");
    }
    std::fs::create_dir_all(&root).expect("create temp root");
    std::env::set_current_dir(&root).expect("set current dir");
    f(&root);
    std::env::set_current_dir(&old).expect("restore current dir");
    if root.exists() {
        std::fs::remove_dir_all(&root).expect("remove temp dir");
    }
}

fn tcp_stream_pair() -> (std::net::TcpStream, std::net::TcpStream) {
    let listener = std::net::TcpListener::bind("127.0.0.1:0").expect("bind loopback listener");
    let addr = listener.local_addr().expect("listener addr");
    let client_thread =
        std::thread::spawn(move || std::net::TcpStream::connect(addr).expect("connect"));
    let (server, _) = listener.accept().expect("accept loopback connection");
    let client = client_thread.join().expect("join client connector");
    (server, client)
}

fn write_key_file(root: &Path, key_id: &str, body: &str) {
    let keys_dir = root
        .join(".genesis")
        .join("runtime")
        .join("backend")
        .join("keys");
    std::fs::create_dir_all(&keys_dir).expect("create keys dir");
    std::fs::write(keys_dir.join(format!("{key_id}.toml")), body).expect("write key file");
}

#[cfg(unix)]
fn write_executable_script(root: &Path, rel: &str, body: &str) -> PathBuf {
    let path = root.join(rel);
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent).expect("create script parent dir");
    }
    std::fs::write(&path, body).expect("write script");
    let mut perms = std::fs::metadata(&path)
        .expect("script metadata")
        .permissions();
    perms.set_mode(0o755);
    std::fs::set_permissions(&path, perms).expect("set script permissions");
    path
}

fn map_get_str<'a>(mm: &'a BTreeMap<TermOrdKey, Term>, key: &str) -> Option<&'a str> {
    match mm.get(&TermOrdKey(Term::symbol(key))) {
        Some(Term::Str(s)) => Some(s.as_str()),
        _ => None,
    }
}

fn map_get_bool(mm: &BTreeMap<TermOrdKey, Term>, key: &str) -> Option<bool> {
    match mm.get(&TermOrdKey(Term::symbol(key))) {
        Some(Term::Bool(v)) => Some(*v),
        _ => None,
    }
}

fn map_get_i64(mm: &BTreeMap<TermOrdKey, Term>, key: &str) -> Option<i64> {
    match mm.get(&TermOrdKey(Term::symbol(key))) {
        Some(Term::Int(v)) => num_traits::cast::ToPrimitive::to_i64(v),
        _ => None,
    }
}

fn map_get_bytes(mm: &BTreeMap<TermOrdKey, Term>, key: &str) -> Option<Vec<u8>> {
    match mm.get(&TermOrdKey(Term::symbol(key))) {
        Some(Term::Bytes(v)) => Some(v.to_vec()),
        Some(Term::Str(s)) => Some(s.as_bytes().to_vec()),
        _ => None,
    }
}

fn map_get_map<'a>(
    mm: &'a BTreeMap<TermOrdKey, Term>,
    key: &str,
) -> Option<&'a BTreeMap<TermOrdKey, Term>> {
    match mm.get(&TermOrdKey(Term::symbol(key))) {
        Some(Term::Map(inner)) => Some(inner),
        _ => None,
    }
}

#[test]
fn http_request_reader_returns_error_when_peer_closes_before_headers() {
    let (mut server, client) = tcp_stream_pair();
    server
        .set_read_timeout(Some(std::time::Duration::from_millis(200)))
        .expect("set read timeout");
    drop(client);
    let err = read_http_request_from_stream(&mut server, 1024).expect_err("request read must fail");
    assert!(
        err.contains("closed before headers"),
        "unexpected error: {err}"
    );
}

#[test]
fn http_response_reader_returns_error_when_peer_closes_before_headers() {
    let (mut server, client) = tcp_stream_pair();
    server
        .set_read_timeout(Some(std::time::Duration::from_millis(200)))
        .expect("set read timeout");
    drop(client);
    let err = parse_http_response_headers(&mut server).expect_err("response read must fail");
    assert!(
        err.contains("closed before headers"),
        "unexpected error: {err}"
    );
}

#[cfg(unix)]
#[test]
fn plugin_command_exec_schema_supports_external_plugin_bytes_response() {
    with_test_workspace(|root| {
        let plugin_path = write_executable_script(
            root,
            "bin/plugin_exec.sh",
            "#!/bin/sh\ncmd=\"$1\"\nif [ \"$cmd\" = \"run\" ]; then\n  printf '{:ok true :data \"plugin-bytes\"}'\nelse\n  printf '{:ok false :error {:code \"plugin/cmd\" :message \"unsupported command\"}}'\nfi\n",
        );
        let payload = term_map(vec![
            (
                ":plugin",
                Term::Str(plugin_path.to_string_lossy().to_string()),
            ),
            (":command", Term::Str("run".to_string())),
            (
                ":request-schema-id",
                Term::Str("genesis/plugin.request.exec.v1".to_string()),
            ),
            (
                ":response-schema-id",
                Term::Str("genesis/plugin.response.bytes.v1".to_string()),
            ),
            (
                ":payload",
                term_map(vec![(
                    ":args",
                    Term::Vector(vec![Term::Str("--fast".to_string())].into()),
                )]),
            ),
        ]);
        let resp = plugin_command("host/plugin::command", &payload).expect("plugin command");
        let Term::Map(mm) = resp else {
            panic!("plugin response should be map");
        };
        assert_eq!(map_get_bool(&mm, ":ok"), Some(true));
        assert_eq!(map_get_str(&mm, ":data"), Some("plugin-bytes"));
    });
}

#[cfg(unix)]
#[test]
fn ffi_call_schema_driven_external_command_executes_without_unsupported() {
    with_test_workspace(|root| {
        let ffi_path = write_executable_script(
            root,
            "bin/ffi_sum.sh",
            "#!/bin/sh\nsym=\"$1\"\nif [ \"$sym\" = \"sum\" ]; then\n  printf '{:ok true :result 7}'\nelse\n  printf '{:ok false :error {:code \"ffi/symbol\" :message \"unknown symbol\"}}'\nfi\n",
        );
        let payload = term_map(vec![
            (":abi-id", Term::Str("abi.math.v1".to_string())),
            (
                ":library",
                Term::Str(ffi_path.to_string_lossy().to_string()),
            ),
            (":symbol", Term::Str("sum".to_string())),
            (
                ":args",
                Term::Vector(vec![Term::Int(3_i64.into()), Term::Int(4_i64.into())].into()),
            ),
            (
                ":request-schema-id",
                Term::Str("genesis/ffi.request.call.v1".to_string()),
            ),
            (
                ":response-schema-id",
                Term::Str("genesis/ffi.response.call.v1".to_string()),
            ),
        ]);
        let resp = ffi_call(&payload).expect("ffi call response");
        let Term::Map(mm) = resp else {
            panic!("ffi response should be map");
        };
        assert_eq!(map_get_bool(&mm, ":ok"), Some(true));
        assert_eq!(map_get_i64(&mm, ":result"), Some(7));
    });
}

#[cfg(unix)]
#[test]
fn ffi_call_external_spawn_failure_returns_structured_error_map() {
    with_test_workspace(|_root| {
        let payload = term_map(vec![
            (":abi-id", Term::Str("abi.math.v1".to_string())),
            (
                ":library",
                Term::Str("/definitely/not/a/real/ffi/bridge".to_string()),
            ),
            (":symbol", Term::Str("sum".to_string())),
            (
                ":request-schema-id",
                Term::Str("genesis/ffi.request.call.v1".to_string()),
            ),
            (
                ":response-schema-id",
                Term::Str("genesis/ffi.response.call.v1".to_string()),
            ),
        ]);
        let resp = ffi_call(&payload).expect("ffi call should not throw bridge error");
        let Term::Map(mm) = resp else {
            panic!("ffi error response should be map");
        };
        assert_eq!(map_get_bool(&mm, ":ok"), Some(false));
        let err = map_get_map(&mm, ":error").expect("error map");
        assert_eq!(map_get_str(err, ":code"), Some("ffi/exec-failed"));
    });
}

#[cfg(unix)]
#[test]
fn net_http_listen_and_respond_roundtrip() {
    with_test_workspace(|_root| {
        let listen_payload = term_map(vec![
            (":local", Term::Str("http://127.0.0.1:0".to_string())),
            (":max-request-bytes", Term::Int(16384_i64.into())),
        ]);
        let listen_resp = net_http_listen(&listen_payload).expect("http listen");
        let Term::Map(listen_map) = listen_resp else {
            panic!("listen response must be map");
        };
        let listener_id = map_get_str(&listen_map, ":listener-id")
            .expect("listener id")
            .to_string();
        let local_uri = map_get_str(&listen_map, ":local")
            .expect("local uri")
            .to_string();
        let addr = local_uri
            .strip_prefix("http://")
            .expect("http:// local prefix")
            .to_string();

        let client = std::thread::spawn(move || -> String {
            let mut stream = std::net::TcpStream::connect(&addr).expect("connect http listener");
            let request =
                b"GET /ready HTTP/1.1\r\nHost: localhost\r\nConnection: close\r\n\r\n".to_vec();
            std::io::Write::write_all(&mut stream, &request).expect("write request");
            let mut response = String::new();
            std::io::Read::read_to_string(&mut stream, &mut response).expect("read response");
            response
        });

        let mut accepted_map = None;
        for _ in 0..50 {
            let resp = net_http_listen(&term_map(vec![
                (":local", Term::Str(local_uri.clone())),
                (":max-request-bytes", Term::Int(16384_i64.into())),
            ]))
            .expect("poll http listen");
            let Term::Map(mm) = resp else {
                panic!("listen poll response must be map");
            };
            if map_get_bool(&mm, ":accepted") == Some(true) {
                accepted_map = Some(mm);
                break;
            }
            std::thread::sleep(std::time::Duration::from_millis(10));
        }
        let accepted_map = accepted_map.expect("listener should accept request");
        let request_id = map_get_str(&accepted_map, ":request-id")
            .expect("request id")
            .to_string();
        assert_eq!(map_get_str(&accepted_map, ":method"), Some("GET"));
        assert_eq!(map_get_str(&accepted_map, ":path"), Some("/ready"));

        let respond_resp = net_http_respond(&term_map(vec![
            (":listener-id", Term::Str(listener_id)),
            (":request-id", Term::Str(request_id)),
            (":status", Term::Int(200_i64.into())),
            (
                ":headers",
                Term::Vector(vec![Term::Str("Content-Type: text/plain".to_string())].into()),
            ),
            (":body", Term::Bytes(b"ok".to_vec().into())),
        ]))
        .expect("http respond");
        let Term::Map(respond_map) = respond_resp else {
            panic!("respond response must be map");
        };
        assert_eq!(map_get_bool(&respond_map, ":responded"), Some(true));

        let response = client.join().expect("client join");
        assert!(response.starts_with("HTTP/1.1 200 OK\r\n"));
        assert!(response.contains("Content-Type: text/plain"));
        assert!(response.ends_with("\r\n\r\nok"));
    });
}

#[cfg(unix)]
#[test]
fn net_ws_open_accept_send_recv_close_roundtrip() {
    with_test_workspace(|_root| {
        let listen_payload = term_map(vec![
            (":local", Term::Str("http://127.0.0.1:0".to_string())),
            (":max-request-bytes", Term::Int(16384_i64.into())),
        ]);
        let listen_resp = net_http_listen(&listen_payload).expect("http listen");
        let Term::Map(listen_map) = listen_resp else {
            panic!("listen response must be map");
        };
        let listener_id = map_get_str(&listen_map, ":listener-id")
            .expect("listener id")
            .to_string();
        let local_uri = map_get_str(&listen_map, ":local")
            .expect("local uri")
            .to_string();
        let ws_url = format!(
            "ws://{}/chat",
            local_uri.strip_prefix("http://").expect("http:// local")
        );

        let client = std::thread::spawn(move || {
            let open_resp =
                net_ws_open(&term_map(vec![(":url", Term::Str(ws_url.clone()))])).expect("ws open");
            let Term::Map(open_map) = open_resp else {
                panic!("ws open response must be map");
            };
            let client_stream_id = map_get_str(&open_map, ":stream-id")
                .expect("client stream id")
                .to_string();

            net_ws_send(&term_map(vec![
                (":stream-id", Term::Str(client_stream_id.clone())),
                (":data", Term::Bytes(b"client-msg".to_vec().into())),
            ]))
            .expect("client ws send");

            let mut received = None;
            for _ in 0..50 {
                let recv_resp = net_ws_recv(&term_map(vec![
                    (":stream-id", Term::Str(client_stream_id.clone())),
                    (":timeout-ms", Term::Int(100_i64.into())),
                ]))
                .expect("client ws recv");
                let Term::Map(recv_map) = recv_resp else {
                    panic!("client recv response must be map");
                };
                if let Some(bytes) = map_get_bytes(&recv_map, ":data")
                    && !bytes.is_empty()
                {
                    received = Some(bytes);
                    break;
                }
            }
            assert_eq!(received, Some(b"server-msg".to_vec()));

            net_ws_close(&term_map(vec![(":stream-id", Term::Str(client_stream_id))]))
                .expect("client ws close");
        });

        let mut accepted = None;
        for _ in 0..50 {
            let listen_poll = net_http_listen(&term_map(vec![
                (":local", Term::Str(local_uri.clone())),
                (":max-request-bytes", Term::Int(16384_i64.into())),
            ]))
            .expect("http poll");
            let Term::Map(mm) = listen_poll else {
                panic!("listen poll map");
            };
            if map_get_bool(&mm, ":accepted") == Some(true) {
                accepted = Some(mm);
                break;
            }
            std::thread::sleep(std::time::Duration::from_millis(10));
        }
        let accepted = accepted.expect("ws request should be accepted");
        let request_id = map_get_str(&accepted, ":request-id")
            .expect("request id")
            .to_string();

        let ws_accept_resp = net_ws_accept(&term_map(vec![
            (":listener-id", Term::Str(listener_id)),
            (":request-id", Term::Str(request_id)),
            (":max-request-bytes", Term::Int(16384_i64.into())),
        ]))
        .expect("ws accept");
        let Term::Map(ws_accept_map) = ws_accept_resp else {
            panic!("ws accept response map");
        };
        let server_stream_id = map_get_str(&ws_accept_map, ":stream-id")
            .expect("server stream id")
            .to_string();

        let mut received = None;
        for _ in 0..50 {
            let recv_resp = net_ws_recv(&term_map(vec![
                (":stream-id", Term::Str(server_stream_id.clone())),
                (":timeout-ms", Term::Int(100_i64.into())),
            ]))
            .expect("server ws recv");
            let Term::Map(recv_map) = recv_resp else {
                panic!("server recv response map");
            };
            if let Some(bytes) = map_get_bytes(&recv_map, ":data")
                && !bytes.is_empty()
            {
                received = Some(bytes);
                break;
            }
        }
        assert_eq!(received, Some(b"client-msg".to_vec()));

        net_ws_send(&term_map(vec![
            (":stream-id", Term::Str(server_stream_id.clone())),
            (":data", Term::Bytes(b"server-msg".to_vec().into())),
        ]))
        .expect("server ws send");
        net_ws_close(&term_map(vec![(":stream-id", Term::Str(server_stream_id))]))
            .expect("server ws close");

        client.join().expect("client join");
    });
}

#[cfg(unix)]
#[test]
fn net_tcp_lifecycle_open_send_recv_close_roundtrip() {
    with_test_workspace(|_root| {
        let listener = std::net::TcpListener::bind("127.0.0.1:0").expect("bind tcp test server");
        let server_addr = listener.local_addr().expect("tcp server addr");
        let server = std::thread::spawn(move || {
            let (mut sock, _) = listener.accept().expect("accept client");
            let mut buf = [0u8; 16];
            let n = std::io::Read::read(&mut sock, &mut buf).expect("read client payload");
            assert_eq!(&buf[..n], b"ping");
            std::io::Write::write_all(&mut sock, b"pong").expect("write server payload");
        });

        let open_payload = term_map(vec![(
            ":remote",
            Term::Str(format!("tcp://{}", server_addr)),
        )]);
        let open_resp = net_tcp_open(&open_payload).expect("tcp open");
        let Term::Map(open_map) = open_resp else {
            panic!("open response must be map");
        };
        let stream_id = map_get_str(&open_map, ":stream-id")
            .expect("stream-id")
            .to_string();

        let send_resp = net_tcp_send(&term_map(vec![
            (":stream-id", Term::Str(stream_id.clone())),
            (":data", Term::Bytes(b"ping".to_vec().into())),
        ]))
        .expect("tcp send");
        let Term::Map(send_map) = send_resp else {
            panic!("send response must be map");
        };
        assert_eq!(map_get_i64(&send_map, ":sent-bytes"), Some(4));

        let recv_resp = net_tcp_recv(&term_map(vec![
            (":stream-id", Term::Str(stream_id.clone())),
            (":max-bytes", Term::Int(16_i64.into())),
            (":timeout-ms", Term::Int(2000_i64.into())),
        ]))
        .expect("tcp recv");
        let Term::Map(recv_map) = recv_resp else {
            panic!("recv response must be map");
        };
        assert_eq!(map_get_bytes(&recv_map, ":data"), Some(b"pong".to_vec()));

        let close_resp = net_tcp_close(&term_map(vec![(":stream-id", Term::Str(stream_id))]))
            .expect("tcp close");
        let Term::Map(close_map) = close_resp else {
            panic!("close response must be map");
        };
        assert_eq!(map_get_bool(&close_map, ":closed"), Some(true));

        server.join().expect("tcp server join");
    });
}

#[cfg(unix)]
#[test]
fn net_udp_lifecycle_bind_send_recv_close_roundtrip() {
    with_test_workspace(|_root| {
        let peer = std::net::UdpSocket::bind("127.0.0.1:0").expect("bind udp peer");
        peer.set_read_timeout(Some(std::time::Duration::from_secs(2)))
            .expect("set peer timeout");
        let peer_addr = peer.local_addr().expect("peer addr");

        let bind_resp = net_udp_bind(&term_map(vec![(
            ":local",
            Term::Str("udp://127.0.0.1:0".to_string()),
        )]))
        .expect("udp bind");
        let Term::Map(bind_map) = bind_resp else {
            panic!("bind response must be map");
        };
        let socket_id = map_get_str(&bind_map, ":socket-id")
            .expect("socket-id")
            .to_string();
        let bridge_local = map_get_str(&bind_map, ":local")
            .expect("bridge local")
            .to_string();

        let send_resp = net_udp_send(&term_map(vec![
            (":socket-id", Term::Str(socket_id.clone())),
            (":remote", Term::Str(format!("udp://{}", peer_addr))),
            (":data", Term::Bytes(b"hello".to_vec().into())),
        ]))
        .expect("udp send");
        let Term::Map(send_map) = send_resp else {
            panic!("send response must be map");
        };
        assert_eq!(map_get_i64(&send_map, ":sent-bytes"), Some(5));

        let mut peer_buf = [0u8; 32];
        let (peer_n, _from) = peer.recv_from(&mut peer_buf).expect("peer recv");
        assert_eq!(&peer_buf[..peer_n], b"hello");

        peer.send_to(b"world", bridge_local)
            .expect("peer send to bridge");

        let recv_resp = net_udp_recv(&term_map(vec![
            (":socket-id", Term::Str(socket_id.clone())),
            (":max-bytes", Term::Int(32_i64.into())),
            (":timeout-ms", Term::Int(2000_i64.into())),
        ]))
        .expect("udp recv");
        let Term::Map(recv_map) = recv_resp else {
            panic!("recv response must be map");
        };
        assert_eq!(map_get_bytes(&recv_map, ":data"), Some(b"world".to_vec()));

        let close_resp = net_udp_close(&term_map(vec![(":socket-id", Term::Str(socket_id))]))
            .expect("udp close");
        let Term::Map(close_map) = close_resp else {
            panic!("close response must be map");
        };
        assert_eq!(map_get_bool(&close_map, ":closed"), Some(true));
    });
}

#[cfg(unix)]
#[test]
fn process_spawn_wait_and_stdout_read_use_real_lifecycle() {
    with_test_workspace(|_root| {
        let spawn_payload = term_map(vec![
            (":program", Term::Str("sh".to_string())),
            (
                ":args",
                Term::Vector(
                    vec![
                        Term::Str("-c".to_string()),
                        Term::Str("printf ready; sleep 0.1; printf done".to_string()),
                    ]
                    .into(),
                ),
            ),
        ]);
        let spawn_resp = process_spawn(&spawn_payload).expect("spawn process");
        let Term::Map(spawn_map) = spawn_resp else {
            panic!("spawn response must be map");
        };
        let process_id = map_get_str(&spawn_map, ":process-id")
            .expect("spawn response must include :process-id")
            .to_string();

        let wait_payload = term_map(vec![(":process-id", Term::Str(process_id.clone()))]);
        let wait_resp = process_wait(&wait_payload).expect("wait process");
        let Term::Map(wait_map) = wait_resp else {
            panic!("wait response must be map");
        };
        assert_eq!(map_get_i64(&wait_map, ":exit"), Some(0));
        assert_eq!(map_get_bool(&wait_map, ":killed"), Some(false));

        let stdout_resp = process_read_stream(
            &term_map(vec![(":process-id", Term::Str(process_id))]),
            true,
        )
        .expect("stdout read");
        let Term::Map(stdout_map) = stdout_resp else {
            panic!("stdout response must be map");
        };
        let stdout = map_get_str(&stdout_map, ":stdout").expect("stdout field");
        assert!(stdout.contains("ready"));
        assert!(stdout.contains("done"));
    });
}

#[cfg(unix)]
#[test]
fn process_kill_sets_killed_flag_and_wait_returns_terminated_exit() {
    with_test_workspace(|_root| {
        let spawn_payload = term_map(vec![
            (":program", Term::Str("sh".to_string())),
            (
                ":args",
                Term::Vector(
                    vec![
                        Term::Str("-c".to_string()),
                        Term::Str("sleep 5".to_string()),
                    ]
                    .into(),
                ),
            ),
        ]);
        let spawn_resp = process_spawn(&spawn_payload).expect("spawn process");
        let Term::Map(spawn_map) = spawn_resp else {
            panic!("spawn response must be map");
        };
        let process_id = map_get_str(&spawn_map, ":process-id")
            .expect("spawn response must include :process-id")
            .to_string();

        let kill_resp = process_kill(&term_map(vec![(
            ":process-id",
            Term::Str(process_id.clone()),
        )]))
        .expect("kill process");
        let Term::Map(kill_map) = kill_resp else {
            panic!("kill response must be map");
        };
        assert_eq!(map_get_bool(&kill_map, ":killed"), Some(true));

        let wait_resp = process_wait(&term_map(vec![(":process-id", Term::Str(process_id))]))
            .expect("wait process");
        let Term::Map(wait_map) = wait_resp else {
            panic!("wait response must be map");
        };
        assert_eq!(map_get_bool(&wait_map, ":killed"), Some(true));
        assert_eq!(map_get_i64(&wait_map, ":exit"), Some(137));
    });
}

#[test]
fn crypto_ed25519_sign_verify_roundtrip_uses_key_provider_file() {
    with_test_workspace(|root| {
        let signing = SigningKey::from_bytes(&[9u8; 32]);
        let sk_b64 = Base64::encode_string(&signing.to_bytes());
        let pk_b64 = Base64::encode_string(&signing.verifying_key().to_bytes());
        write_key_file(
            root,
            "key-main",
            &format!("alg = \"ed25519\"\nsk_b64 = \"{sk_b64}\"\npk_b64 = \"{pk_b64}\"\n"),
        );

        let sign_payload = term_map(vec![
            (":algorithm", Term::Str("ed25519".to_string())),
            (":key-id", Term::Str("key-main".to_string())),
            (":message", Term::Bytes(b"hello".to_vec().into())),
            (":context", Term::Bytes(b"ctx".to_vec().into())),
        ]);
        let signed = crypto_sign(&sign_payload).expect("sign");
        let Term::Map(signed_map) = signed else {
            panic!("expected sign response map");
        };
        let Some(Term::Bytes(signature)) = signed_map.get(&TermOrdKey(Term::symbol(":signature")))
        else {
            panic!("missing :signature bytes");
        };
        assert_eq!(signature.len(), 64);

        let verify_payload = term_map(vec![
            (":algorithm", Term::Str("ed25519".to_string())),
            (":key-id", Term::Str("key-main".to_string())),
            (":message", Term::Bytes(b"hello".to_vec().into())),
            (":context", Term::Bytes(b"ctx".to_vec().into())),
            (":signature", Term::Bytes(signature.to_vec().into())),
        ]);
        let verified = crypto_verify(&verify_payload).expect("verify");
        let Term::Map(verified_map) = verified else {
            panic!("expected verify response map");
        };
        assert_eq!(
            verified_map.get(&TermOrdKey(Term::symbol(":valid"))),
            Some(&Term::Bool(true))
        );
    });
}

#[test]
fn crypto_hkdf_and_aead_roundtrip_with_packed_ciphertext() {
    with_test_workspace(|root| {
        let key_b64 = Base64::encode_string(b"this-is-a-test-symmetric-key-material");
        write_key_file(
            root,
            "sym-main",
            &format!("alg = \"symmetric\"\nkey_b64 = \"{key_b64}\"\n"),
        );

        let kdf_payload = term_map(vec![
            (":algorithm", Term::Str("hkdf-sha256".to_string())),
            (":key-id", Term::Str("sym-main".to_string())),
            (":info", Term::Bytes(b"context".to_vec().into())),
            (":salt", Term::Bytes(b"salt".to_vec().into())),
            (":length", Term::Int(32_i64.into())),
        ]);
        let kdf = crypto_kdf(&kdf_payload).expect("kdf");
        let Term::Map(kdf_map) = kdf else {
            panic!("expected kdf response map");
        };
        let Some(Term::Bytes(material)) = kdf_map.get(&TermOrdKey(Term::symbol(":key"))) else {
            panic!("missing :key bytes");
        };
        assert_eq!(material.len(), 32);

        let seal_payload = term_map(vec![
            (":algorithm", Term::Str("aes-256-gcm".to_string())),
            (":key-id", Term::Str("sym-main".to_string())),
            (":plaintext", Term::Bytes(b"payload-data".to_vec().into())),
            (":aad", Term::Bytes(b"aad".to_vec().into())),
            (":nonce", Term::Bytes(vec![7u8; 12].into())),
        ]);
        let sealed = crypto_aead_seal(&seal_payload).expect("seal");
        let Term::Map(sealed_map) = sealed else {
            panic!("expected seal response map");
        };
        let Some(Term::Bytes(packed_ciphertext)) =
            sealed_map.get(&TermOrdKey(Term::symbol(":ciphertext")))
        else {
            panic!("missing :ciphertext bytes");
        };
        assert!(packed_ciphertext.len() > 28);

        let open_payload = term_map(vec![
            (":algorithm", Term::Str("aes-256-gcm".to_string())),
            (":key-id", Term::Str("sym-main".to_string())),
            (
                ":ciphertext",
                Term::Bytes(packed_ciphertext.to_vec().into()),
            ),
            (":aad", Term::Bytes(b"aad".to_vec().into())),
        ]);
        let opened = crypto_aead_open(&open_payload).expect("open");
        let Term::Map(opened_map) = opened else {
            panic!("expected open response map");
        };
        assert_eq!(
            opened_map.get(&TermOrdKey(Term::symbol(":plaintext"))),
            Some(&Term::Bytes(b"payload-data".to_vec().into()))
        );

        let mut tampered = packed_ciphertext.to_vec();
        *tampered.last_mut().expect("non-empty ciphertext") ^= 0x01;
        let tampered_open_payload = term_map(vec![
            (":algorithm", Term::Str("aes-256-gcm".to_string())),
            (":key-id", Term::Str("sym-main".to_string())),
            (":ciphertext", Term::Bytes(tampered.into())),
            (":aad", Term::Bytes(b"aad".to_vec().into())),
        ]);
        let opened_tampered = crypto_aead_open(&tampered_open_payload).expect("open tampered");
        let Term::Map(opened_tampered_map) = opened_tampered else {
            panic!("expected tampered open response map");
        };
        assert_eq!(
            opened_tampered_map.get(&TermOrdKey(Term::symbol(":ok"))),
            Some(&Term::Bool(false))
        );
    });
}
