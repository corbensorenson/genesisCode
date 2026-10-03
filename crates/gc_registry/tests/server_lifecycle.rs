#![cfg(not(target_os = "wasi"))]

use std::io::{Read, Write};
use std::net::{Shutdown, TcpStream};
use std::path::PathBuf;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::mpsc;
use std::time::Duration;

use gc_registry::{HttpRegistryServerConfig, RegistryClient, spawn_http_file_registry_server};

struct TempDir(PathBuf);

impl TempDir {
    fn new() -> Self {
        static NEXT: AtomicUsize = AtomicUsize::new(0);
        let path = std::env::temp_dir().join(format!(
            "gc-registry-lifecycle-{}-{}",
            std::process::id(),
            NEXT.fetch_add(1, Ordering::SeqCst)
        ));
        std::fs::create_dir(&path).unwrap();
        Self(path)
    }

    fn config(&self, max_requests: Option<u64>) -> HttpRegistryServerConfig {
        HttpRegistryServerConfig {
            root: self.0.clone(),
            max_requests,
            ..HttpRegistryServerConfig::default()
        }
    }
}

impl Drop for TempDir {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

fn client(address: &str) -> RegistryClient {
    RegistryClient::new(&format!("http://{address}"), Some(Duration::from_secs(2))).unwrap()
}

#[test]
fn natural_join_waits_for_real_request_and_policy_completion() {
    let root = TempDir::new();
    let handle = spawn_http_file_registry_server(root.config(Some(1))).unwrap();
    let client = client(handle.bound_addr());
    let (completed, observed) = mpsc::channel();
    let waiter = std::thread::spawn(move || completed.send(handle.join()).unwrap());

    match observed.recv_timeout(Duration::from_millis(250)) {
        Err(mpsc::RecvTimeoutError::Timeout) => {}
        early => {
            waiter.join().unwrap();
            panic!("natural join stopped before any request: {early:?}");
        }
    }
    assert!(client.ping().unwrap().ok);
    observed
        .recv_timeout(Duration::from_secs(2))
        .unwrap()
        .unwrap();
    waiter.join().unwrap();
}

#[test]
fn owner_drop_stops_listener_after_serving_real_request() {
    let root = TempDir::new();
    let handle = spawn_http_file_registry_server(root.config(Some(2))).unwrap();
    let client = client(handle.bound_addr());
    assert!(client.ping().unwrap().ok);
    drop(handle);
    assert!(
        client.ping().is_err(),
        "dropped owner left serving thread alive"
    );
}

#[test]
fn explicit_shutdown_then_join_reaps_idle_listener() {
    let root = TempDir::new();
    let handle = spawn_http_file_registry_server(root.config(None)).unwrap();
    let address = handle.bound_addr().to_string();
    handle.shutdown();
    handle.join().unwrap();
    assert!(client(&address).ping().is_err());
}

#[test]
fn join_propagates_worker_initialization_error() {
    let root = TempDir::new();
    let blocked_root = root.0.join("file-root");
    std::fs::write(&blocked_root, b"not a directory").unwrap();
    let config = HttpRegistryServerConfig {
        root: blocked_root,
        ..root.config(None)
    };
    let handle = spawn_http_file_registry_server(config).unwrap();
    assert!(
        handle.join().is_err(),
        "worker failure was reported as success"
    );
}

#[test]
fn shutdown_interrupts_body_after_continue_handshake() {
    let root = TempDir::new();
    let handle = spawn_http_file_registry_server(root.config(None)).unwrap();
    let mut stream = TcpStream::connect(handle.bound_addr()).unwrap();
    stream
        .set_read_timeout(Some(Duration::from_secs(2)))
        .unwrap();
    write!(
        stream,
        "POST /v1/store/has HTTP/1.1\r\nHost: localhost\r\nContent-Length: 4096\r\nExpect: 100-continue\r\nConnection: close\r\n\r\n"
    )
    .unwrap();
    let mut interim = Vec::new();
    while !interim.ends_with(b"\r\n\r\n") {
        assert!(interim.len() < 256, "unbounded interim response");
        let mut byte = [0];
        stream.read_exact(&mut byte).unwrap();
        interim.push(byte[0]);
    }
    assert!(interim.starts_with(b"HTTP/1.1 100 "), "{interim:?}");
    // The actual Continue response witnesses the server entering body admission.
    // Keep the peer open with all declared body bytes withheld during shutdown.
    handle.shutdown();
    let (completed, observed) = mpsc::channel();
    let waiter = std::thread::spawn(move || completed.send(handle.join()).unwrap());
    let stopped_without_peer_help = observed.recv_timeout(Duration::from_secs(2));
    // Always release the stalled peer before asserting, including on old source.
    if let Err(error) = stream.shutdown(Shutdown::Both) {
        assert_eq!(error.kind(), std::io::ErrorKind::NotConnected);
    }
    if stopped_without_peer_help.is_err() {
        observed
            .recv_timeout(Duration::from_secs(2))
            .unwrap()
            .unwrap();
    }
    waiter.join().unwrap();
    assert!(
        stopped_without_peer_help.is_ok_and(|result| result.is_ok()),
        "shutdown depended on the stalled client releasing its body"
    );
}

#[test]
fn separate_stop_handle_can_interrupt_natural_wait() {
    let root = TempDir::new();
    let handle = spawn_http_file_registry_server(root.config(None)).unwrap();
    let address = handle.bound_addr().to_string();
    let stop = handle.shutdown_handle();
    let (completed, observed) = mpsc::channel();
    let waiter = std::thread::spawn(move || completed.send(handle.join()).unwrap());
    assert!(client(&address).ping().unwrap().ok);
    assert!(matches!(
        observed.try_recv(),
        Err(mpsc::TryRecvError::Empty)
    ));
    stop.shutdown();
    stop.shutdown();
    observed
        .recv_timeout(Duration::from_secs(2))
        .unwrap()
        .unwrap();
    waiter.join().unwrap();
    assert!(client(&address).ping().is_err());
}

#[test]
fn zero_request_policy_stops_without_admitting_a_request() {
    let root = TempDir::new();
    let handle = spawn_http_file_registry_server(root.config(Some(0))).unwrap();
    let address = handle.bound_addr().to_string();
    handle.join().unwrap();
    assert!(client(&address).ping().is_err());
}

#[test]
fn owner_drop_reaps_stalled_headers_and_admitted_upload_body() {
    for headers in [false, true] {
        let root = TempDir::new();
        let handle = spawn_http_file_registry_server(root.config(None)).unwrap();
        let address = handle.bound_addr().to_string();
        let mut stream = TcpStream::connect(&address).unwrap();
        stream
            .set_read_timeout(Some(Duration::from_secs(2)))
            .unwrap();
        if headers {
            stream
                .write_all(b"POST /v1/store/has HTTP/1.1\r\nHost: localhost\r\n")
                .unwrap();
        } else {
            stream.write_all(b"POST /v1/store/has HTTP/1.1\r\nHost: localhost\r\nContent-Length: 4096\r\nExpect: 100-continue\r\n\r\n").unwrap();
            let mut response = Vec::new();
            while !response.ends_with(b"\r\n\r\n") {
                assert!(response.len() < 256);
                let mut byte = [0];
                stream.read_exact(&mut byte).unwrap();
                response.push(byte[0]);
            }
            assert!(response.starts_with(b"HTTP/1.1 100 "));
        }
        let (completed, observed) = mpsc::channel();
        let owner = std::thread::spawn(move || {
            drop(handle);
            completed.send(()).unwrap();
        });
        let result = observed.recv_timeout(Duration::from_secs(2));
        // Release fixture peer even on regression; the outer process watchdog owns the test.
        drop(stream);
        owner.join().unwrap();
        assert!(
            result.is_ok(),
            "owner drop needed peer assistance (headers={headers})"
        );
        assert!(client(&address).ping().is_err());
    }
}

#[test]
fn malformed_connection_does_not_spend_request_policy() {
    let root = TempDir::new();
    let handle = spawn_http_file_registry_server(root.config(Some(1))).unwrap();
    let address = handle.bound_addr().to_string();
    let mut peer = TcpStream::connect(&address).unwrap();
    peer.set_read_timeout(Some(Duration::from_secs(2))).unwrap();
    peer.write_all(b"invalid request\r\n\r\n").unwrap();
    let mut response = Vec::new();
    peer.take(4096).read_to_end(&mut response).unwrap();
    assert!(response.starts_with(b"HTTP/1.1 400 "));
    assert!(client(&address).ping().unwrap().ok);
    handle.join().unwrap();
    assert!(client(&address).ping().is_err());
}
