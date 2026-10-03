#![cfg(not(target_os = "wasi"))]

use std::io::{Read, Write};
use std::net::TcpStream;
use std::path::PathBuf;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::time::Duration;

use gc_registry::{
    HttpRegistryServerConfig, HttpRegistryServerHandle, HttpRegistryServerLimits, RegistryClient,
    spawn_http_file_registry_server_with_limits,
};
use serde_json::{Value, json};

struct Fixture {
    server: Option<HttpRegistryServerHandle>,
    root: PathBuf,
    url: String,
    http: reqwest::blocking::Client,
}
impl Fixture {
    fn new(limits: HttpRegistryServerLimits) -> Self {
        static NEXT: AtomicUsize = AtomicUsize::new(0);
        let root = std::env::temp_dir().join(format!(
            "gc-registry-limits-{}-{}",
            std::process::id(),
            NEXT.fetch_add(1, Ordering::SeqCst)
        ));
        std::fs::create_dir(&root).unwrap();
        let server = spawn_http_file_registry_server_with_limits(
            HttpRegistryServerConfig {
                root: root.clone(),
                max_chunk_bytes: 4,
                ..HttpRegistryServerConfig::default()
            },
            limits,
        )
        .unwrap();
        let url = format!("http://{}", server.bound_addr());
        Self {
            server: Some(server),
            root,
            url,
            http: reqwest::blocking::Client::builder()
                .timeout(Duration::from_secs(2))
                .no_proxy()
                .build()
                .unwrap(),
        }
    }
    fn standard() -> Self {
        Self::new(limits())
    }
    fn post(&self, path: &str, body: Value) -> reqwest::blocking::Response {
        self.http
            .post(format!("{}{path}", self.url))
            .json(&body)
            .send()
            .unwrap()
    }
    fn start(&self, size: u64, hash: &str) -> String {
        let response = self.post(
            "/v1/store/upload/start",
            json!({"hash":hash,"size_bytes":size}),
        );
        assert_eq!(response.status(), 200);
        response.json::<Value>().unwrap()["upload_id"]
            .as_str()
            .unwrap()
            .to_string()
    }
    fn chunk(&self, id: &str, index: &str, bytes: &[u8]) -> u16 {
        self.http
            .put(format!("{}/v1/store/upload/chunk/{id}/{index}", self.url))
            .body(bytes.to_vec())
            .send()
            .unwrap()
            .status()
            .as_u16()
    }
    fn raw(&self, request: &[u8]) -> Vec<u8> {
        let address = self.server.as_ref().unwrap().bound_addr();
        let mut stream = TcpStream::connect(address).unwrap();
        stream
            .set_read_timeout(Some(Duration::from_secs(2)))
            .unwrap();
        stream.write_all(request).unwrap();
        let mut response = Vec::new();
        stream.take(4096).read_to_end(&mut response).unwrap();
        response
    }
}
impl Drop for Fixture {
    fn drop(&mut self) {
        self.server.take().unwrap().stop_and_join().unwrap();
        std::fs::remove_dir_all(&self.root).unwrap();
    }
}
fn limits() -> HttpRegistryServerLimits {
    HttpRegistryServerLimits {
        max_json_bytes: 256,
        max_object_bytes: 8,
        max_upload_bytes: 8,
        max_sessions: 2,
        max_chunks_per_session: 4,
        request_timeout: Duration::from_secs(2),
        ..HttpRegistryServerLimits::default()
    }
}
fn hash(bytes: &[u8]) -> String {
    blake3::hash(bytes).to_hex().to_string()
}

#[test]
fn honest_direct_and_chunked_objects_keep_identity_and_wire_contract() {
    let fixture = Fixture::standard();
    let client = RegistryClient::new(&fixture.url, Some(Duration::from_secs(2))).unwrap();
    client.store_put(&hash(b"12345678"), b"12345678").unwrap();
    assert_eq!(client.store_get(&hash(b"12345678")).unwrap(), b"12345678");
    let id = fixture.start(8, &hash(b"abcdefgh"));
    assert_eq!(fixture.chunk(&id, "0", b"abcd"), 200);
    assert_eq!(fixture.chunk(&id, "1", b"efgh"), 200);
    assert_eq!(
        fixture
            .post("/v1/store/upload/finish", json!({"upload_id":id}))
            .status(),
        200
    );
    assert_eq!(client.store_get(&hash(b"abcdefgh")).unwrap(), b"abcdefgh");
}

#[test]
fn session_object_reservation_and_chunk_bounds_reject_before_mutation() {
    let fixture = Fixture::standard();
    assert_eq!(
        fixture
            .post(
                "/v1/store/upload/start",
                json!({"hash":"bad","size_bytes":1})
            )
            .status(),
        400
    );
    assert_eq!(
        fixture
            .post(
                "/v1/store/upload/start",
                json!({"hash":hash(b"x"),"size_bytes":9})
            )
            .status(),
        413
    );
    let first = fixture.start(4, &hash(b"1234"));
    let second = fixture.start(4, &hash(b"abcd"));
    assert_eq!(first, "u_1");
    assert_eq!(second, "u_2");
    assert_eq!(
        fixture
            .post(
                "/v1/store/upload/start",
                json!({"hash":hash(b""),"size_bytes":0})
            )
            .status(),
        413
    );
    assert_eq!(fixture.chunk(&first, "18446744073709551615", b"x"), 413);
    assert_eq!(fixture.chunk(&first, "not-a-number", b"x"), 400);
    assert_eq!(fixture.chunk(&first, "0", b"12345"), 413);
    assert_eq!(fixture.chunk(&first, "0", b"1234"), 200);
    assert_eq!(fixture.chunk(&first, "1", b"x"), 413);
    assert_eq!(fixture.chunk(&first, "0", b"12"), 200);
    assert_eq!(fixture.chunk(&first, "1", b"34"), 200);
    assert_eq!(
        fixture
            .post("/v1/store/upload/finish", json!({"upload_id":first}))
            .status(),
        200
    );
    assert_eq!(fixture.chunk(&second, "0", b"abcd"), 200);
    assert_eq!(
        fixture
            .post("/v1/store/upload/finish", json!({"upload_id":second}))
            .status(),
        200
    );
    assert_eq!(fixture.start(0, &hash(b"")), "u_3");
}

#[test]
fn declared_and_chunked_oversize_and_truncation_never_install_objects() {
    let fixture = Fixture::new(HttpRegistryServerLimits {
        request_timeout: Duration::from_millis(100),
        ..limits()
    });
    let identity = hash(b"123456789");
    let response = fixture.raw(format!("PUT /v1/store/put/{identity} HTTP/1.1\r\nHost: localhost\r\nContent-Length: 9\r\nConnection: close\r\n\r\n123456789").as_bytes());
    assert!(response.starts_with(b"HTTP/1.1 413"));
    let response = fixture.raw(format!("PUT /v1/store/put/{identity} HTTP/1.1\r\nHost: localhost\r\nTransfer-Encoding: chunked\r\nConnection: close\r\n\r\n9\r\n123456789\r\n0\r\n\r\n").as_bytes());
    assert!(response.starts_with(b"HTTP/1.1 413"));
    assert!(!fixture.root.join("v1/store").join(identity).exists());
    let response = fixture.raw(format!("PUT /v1/store/put/{} HTTP/1.1\r\nHost: localhost\r\nContent-Length: 8\r\nConnection: close\r\n\r\n123",hash(b"12345678")).as_bytes());
    // The peer remains open: the absolute deadline closes the incomplete body.
    assert!(response.is_empty() || response.starts_with(b"HTTP/1.1 4"));
    assert!(
        !fixture
            .root
            .join("v1/store")
            .join(hash(b"12345678"))
            .exists()
    );
}

#[test]
fn json_limit_and_false_framing_are_enforced_before_dispatch() {
    let fixture = Fixture::standard();
    let mut body = b"{\"hashes\":[]}".to_vec();
    body.resize(256, b' ');
    assert_eq!(
        fixture
            .http
            .post(format!("{}/v1/store/has", fixture.url))
            .body(body.clone())
            .send()
            .unwrap()
            .status(),
        200
    );
    body.push(b' ');
    assert_eq!(
        fixture
            .http
            .post(format!("{}/v1/store/has", fixture.url))
            .body(body)
            .send()
            .unwrap()
            .status(),
        413
    );
    let response = fixture.raw(b"POST /v1/store/has HTTP/1.1\r\nHost: localhost\r\nContent-Length: 1\r\nContent-Length: 2\r\nConnection: close\r\n\r\n{}");
    assert!(response.starts_with(b"HTTP/1.1 400"));
}

#[test]
fn sessions_expire_on_the_real_clock_and_release_reservations() {
    let fixture = Fixture::new(HttpRegistryServerLimits {
        session_lifetime: Duration::from_millis(300),
        ..limits()
    });
    let id = fixture.start(8, &hash(b"12345678"));
    assert_eq!(fixture.chunk(&id, "0", b"1234"), 200);
    std::thread::sleep(Duration::from_millis(350));
    assert_eq!(
        fixture
            .http
            .get(format!("{}/v1/store/upload/status/{id}", fixture.url))
            .send()
            .unwrap()
            .status(),
        404
    );
    assert_eq!(fixture.start(8, &hash(b"abcdefgh")), "u_2");
}

#[test]
fn aggregate_reservation_and_minimum_chunk_count_are_independent_limits() {
    let fixture = Fixture::new(HttpRegistryServerLimits {
        max_sessions: 4,
        ..limits()
    });
    fixture.start(8, &hash(b"12345678"));
    assert_eq!(
        fixture
            .post(
                "/v1/store/upload/start",
                json!({"hash":hash(b"x"),"size_bytes":1})
            )
            .status(),
        413
    );
    let fixture = Fixture::new(HttpRegistryServerLimits {
        max_chunks_per_session: 1,
        ..limits()
    });
    assert_eq!(
        fixture
            .post(
                "/v1/store/upload/start",
                json!({"hash":hash(b"12345678"),"size_bytes":8})
            )
            .status(),
        413
    );
    assert_eq!(fixture.start(4, &hash(b"1234")), "u_1");
}

#[test]
fn invalid_policy_is_rejected_before_binding_or_filesystem_mutation() {
    let root = std::env::temp_dir().join(format!("gc-invalid-policy-{}", std::process::id()));
    assert!(!root.exists());
    for policy in [
        HttpRegistryServerLimits {
            max_sessions: 0,
            ..limits()
        },
        HttpRegistryServerLimits {
            request_timeout: Duration::ZERO,
            ..limits()
        },
        HttpRegistryServerLimits {
            max_upload_bytes: 7,
            ..limits()
        },
        HttpRegistryServerLimits {
            max_object_bytes: usize::MAX,
            ..limits()
        },
    ] {
        let result = spawn_http_file_registry_server_with_limits(
            HttpRegistryServerConfig {
                root: root.clone(),
                max_chunk_bytes: 4,
                ..HttpRegistryServerConfig::default()
            },
            policy,
        );
        assert!(result.is_err());
        assert!(!root.exists());
    }
}

#[test]
fn header_count_and_absolute_body_deadline_preserve_server_liveness() {
    let fixture = Fixture::new(HttpRegistryServerLimits {
        request_timeout: Duration::from_millis(100),
        ..limits()
    });
    let mut headers =
        String::from("GET /v1/ping HTTP/1.1\r\nHost: localhost\r\nConnection: close\r\n");
    for i in 0..65 {
        headers.push_str(&format!("X-{i}: a\r\n"));
    }
    headers.push_str("\r\n");
    let response = fixture.raw(headers.as_bytes());
    assert!(response.starts_with(b"HTTP/1.1 431") || response.starts_with(b"HTTP/1.1 400"));
    let identity = hash(b"12345678");
    let mut stream = TcpStream::connect(fixture.server.as_ref().unwrap().bound_addr()).unwrap();
    stream
        .set_read_timeout(Some(Duration::from_secs(2)))
        .unwrap();
    write!(stream,"PUT /v1/store/put/{identity} HTTP/1.1\r\nHost: localhost\r\nContent-Length: 8\r\nConnection: close\r\n\r\n").unwrap();
    for byte in b"12345678" {
        if stream.write_all(&[*byte]).is_err() {
            break;
        }
        std::thread::sleep(Duration::from_millis(40));
    }
    let mut response = Vec::new();
    match stream.take(4096).read_to_end(&mut response) {
        Ok(_) => {}
        Err(e) => assert_eq!(e.kind(), std::io::ErrorKind::ConnectionReset),
    }
    assert!(!fixture.root.join("v1/store").join(identity).exists());
    let client = RegistryClient::new(&fixture.url, Some(Duration::from_secs(2))).unwrap();
    assert!(client.ping().unwrap().ok);
}

#[test]
fn hash_array_count_is_admitted_during_decode() {
    let fixture = Fixture::new(HttpRegistryServerLimits {
        max_json_bytes: 1 << 20,
        ..limits()
    });
    let mut hashes = vec![hash(b""); 1024];
    assert_eq!(
        fixture
            .post("/v1/store/has", json!({"hashes":hashes}))
            .status(),
        200
    );
    hashes.push(hash(b""));
    let response = fixture.post("/v1/store/has", json!({"hashes":hashes}));
    assert_eq!(response.status(), 413);
    assert_eq!(
        response.json::<Value>().unwrap()["error"]["code"],
        "payload_too_large"
    );
}

#[test]
fn bounded_get_refuses_preexisting_oversize_object() {
    let fixture = Fixture::standard();
    let identity = hash(b"123456789");
    std::fs::write(fixture.root.join("v1/store").join(&identity), b"123456789").unwrap();
    assert_eq!(
        fixture
            .http
            .get(format!("{}/v1/store/get/{identity}", fixture.url))
            .send()
            .unwrap()
            .status(),
        413
    );
    assert_eq!(
        std::fs::read(fixture.root.join("v1/store").join(identity)).unwrap(),
        b"123456789"
    );
}
