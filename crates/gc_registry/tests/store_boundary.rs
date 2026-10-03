#![cfg(not(target_os = "wasi"))]

use std::collections::BTreeMap;
use std::path::PathBuf;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use std::time::Duration;

use gc_registry::*;

struct TempDir(PathBuf);
impl TempDir {
    fn new() -> Self {
        static NEXT: AtomicUsize = AtomicUsize::new(0);
        let path = std::env::temp_dir().join(format!(
            "gc-registry-boundary-{}-{}",
            std::process::id(),
            NEXT.fetch_add(1, Ordering::SeqCst)
        ));
        std::fs::create_dir(&path).unwrap();
        Self(path)
    }
    fn client(&self) -> RegistryClient {
        RegistryClient::new(
            reqwest::Url::from_directory_path(&self.0).unwrap().as_str(),
            None,
        )
        .unwrap()
    }
}
impl Drop for TempDir {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

struct HttpFixture {
    url: String,
    contacts: Arc<AtomicUsize>,
    stop: Arc<AtomicBool>,
    worker: Option<std::thread::JoinHandle<()>>,
}
impl HttpFixture {
    fn new(status: u16, body: &[u8], location: Option<String>) -> Self {
        let server = tiny_http::Server::http("127.0.0.1:0").unwrap();
        let url = format!("http://{}", server.server_addr());
        let contacts = Arc::new(AtomicUsize::new(0));
        let stop = Arc::new(AtomicBool::new(false));
        let observed = contacts.clone();
        let stopped = stop.clone();
        let body = body.to_vec();
        let worker = std::thread::spawn(move || {
            while !stopped.load(Ordering::SeqCst) {
                if let Some(request) = server.recv_timeout(Duration::from_millis(20)).unwrap() {
                    observed.fetch_add(1, Ordering::SeqCst);
                    let mut response = tiny_http::Response::new(
                        tiny_http::StatusCode(status),
                        Vec::new(),
                        std::io::Cursor::new(body.clone()),
                        None,
                        None,
                    )
                    .with_chunked_threshold(0);
                    if let Some(location) = &location {
                        response.add_header(
                            tiny_http::Header::from_bytes("Location", location.as_str()).unwrap(),
                        );
                    }
                    let _ = request.respond(response);
                }
            }
        });
        Self {
            url,
            contacts,
            stop,
            worker: Some(worker),
        }
    }
    fn client(&self) -> RegistryClient {
        RegistryClient::new(&self.url, Some(Duration::from_secs(2))).unwrap()
    }
}
impl Drop for HttpFixture {
    fn drop(&mut self) {
        self.stop.store(true, Ordering::SeqCst);
        if let Some(worker) = self.worker.take() {
            worker.join().unwrap();
        }
    }
}

struct CorruptRegistry {
    contacts: AtomicUsize,
}
impl InProcRegistry for CorruptRegistry {
    fn ping(&self) -> Result<PingResp, RegistryError> {
        unreachable!()
    }
    fn store_has(&self, _: &[String]) -> Result<BTreeMap<String, bool>, RegistryError> {
        unreachable!()
    }
    fn store_get(&self, _: &str) -> Result<Vec<u8>, RegistryError> {
        self.contacts.fetch_add(1, Ordering::SeqCst);
        Ok(b"corrupt".to_vec())
    }
    fn store_put(&self, _: &str, _: &[u8]) -> Result<(), RegistryError> {
        self.contacts.fetch_add(1, Ordering::SeqCst);
        Ok(())
    }
    fn refs_get(&self, _: &str) -> Result<Option<String>, RegistryError> {
        unreachable!()
    }
    fn refs_list(&self, _: Option<&str>) -> Result<Vec<RefsListEntry>, RegistryError> {
        unreachable!()
    }
    fn refs_set(&self, _: &RefsSetReq<'_>) -> Result<RefsSetResp, RegistryError> {
        unreachable!()
    }
}
struct Registration(&'static str);
impl Drop for Registration {
    fn drop(&mut self) {
        unregister_inproc(self.0).unwrap();
    }
}
fn hash(bytes: &[u8]) -> String {
    blake3::hash(bytes).to_hex().to_string()
}

#[test]
fn inproc_rejects_corrupt_get_and_optional_get() {
    let reg = Arc::new(CorruptRegistry {
        contacts: AtomicUsize::new(0),
    });
    register_inproc("boundary-corrupt", reg).unwrap();
    let _guard = Registration("boundary-corrupt");
    let client = RegistryClient::new("inproc://boundary-corrupt", None).unwrap();
    let expected = hash(b"honest");
    assert_eq!(
        client
            .store_get_bounded(&hash(b"corrupt"), Some(7))
            .unwrap(),
        b"corrupt"
    );
    assert!(
        client
            .store_get_bounded(&hash(b"corrupt"), Some(6))
            .is_err()
    );
    assert!(
        client.store_get_bounded(&expected, Some(100)).is_err(),
        "corrupt get admitted"
    );
    assert!(
        client.store_get_opt_bounded(&expected, Some(100)).is_err(),
        "corrupt optional get admitted"
    );
}

#[test]
fn http_checks_integrity_and_preserves_missing() {
    let good = HttpFixture::new(200, b"honest", None);
    let framing = reqwest::blocking::get(format!("{}/v1/store/get", good.url)).unwrap();
    assert_eq!(
        framing.content_length(),
        None,
        "fixture must exercise streaming without Content-Length"
    );
    let expected = hash(b"honest");
    assert_eq!(
        good.client().store_get_bounded(&expected, Some(6)).unwrap(),
        b"honest"
    );
    assert!(good.client().store_get_bounded(&expected, Some(5)).is_err());
    let empty = HttpFixture::new(200, b"", None);
    assert_eq!(
        empty
            .client()
            .store_get_bounded(&hash(b""), Some(0))
            .unwrap(),
        b""
    );
    let bad = HttpFixture::new(200, b"corrupt", None);
    assert!(
        bad.client()
            .store_get_bounded(&expected, Some(100))
            .is_err(),
        "corrupt HTTP get admitted"
    );
    assert!(
        bad.client()
            .store_get_opt_bounded(&expected, Some(100))
            .is_err(),
        "corrupt HTTP optional get admitted"
    );
    let missing = HttpFixture::new(404, b"", None);
    assert!(
        missing
            .client()
            .store_get_opt_bounded(&expected, Some(0))
            .unwrap()
            .is_none()
    );
}

#[test]
fn redirects_never_contact_another_listener() {
    let forbidden = HttpFixture::new(
        200,
        br#"{"ok":true,"version":"0.1","hash":"blake3-256"}"#,
        None,
    );
    for status in [301, 302, 303, 307, 308] {
        let allowed = HttpFixture::new(status, b"", Some(format!("{}/v1/ping", forbidden.url)));
        let result = allowed.client().ping();
        assert_eq!(
            forbidden.contacts.load(Ordering::SeqCst),
            0,
            "redirect {status} contacted forbidden listener: {result:?}"
        );
        assert!(result.is_err());
        assert_eq!(allowed.contacts.load(Ordering::SeqCst), 1);
    }
}

#[test]
fn malformed_hashes_are_rejected_before_io() {
    let directory = TempDir::new();
    let file = directory.client();
    let http = HttpFixture::new(404, b"", None);
    let reg = Arc::new(CorruptRegistry {
        contacts: AtomicUsize::new(0),
    });
    register_inproc("boundary-invalid", reg.clone()).unwrap();
    let _guard = Registration("boundary-invalid");
    let inproc = RegistryClient::new("inproc://boundary-invalid", None).unwrap();
    for client in [&file, &http.client(), &inproc] {
        for invalid in ["", "../escape", "short", &"A".repeat(64), &"g".repeat(64)] {
            assert!(
                client.store_get_opt_bounded(invalid, Some(10)).is_err(),
                "invalid identity admitted: {invalid}"
            );
            assert!(client.store_put(invalid, b"bad").is_err());
            assert!(client.store_has(&[invalid.to_string()]).is_err());
            assert!(client.store_upload_start(invalid, 1).is_err());
        }
        assert!(client.store_put(&hash(b"honest"), b"corrupt").is_err());
        assert!(
            client
                .store_put_chunked(&hash(b"honest"), b"corrupt", 1)
                .is_err()
        );
    }
    assert!(
        !directory.0.join("v1").exists(),
        "rejection created file transport state"
    );
    assert_eq!(http.contacts.load(Ordering::SeqCst), 0);
    assert_eq!(reg.contacts.load(Ordering::SeqCst), 0);
}

#[test]
fn redirect_chains_loops_and_store_requests_stop_at_the_first_response() {
    let final_endpoint = HttpFixture::new(200, b"honest", None);
    let middle = HttpFixture::new(302, b"", Some(final_endpoint.url.clone()));
    let first = HttpFixture::new(307, b"", Some(middle.url.clone()));
    let client = first.client();
    let expected = hash(b"honest");
    assert!(client.store_get(&expected).is_err());
    assert!(client.store_put(&expected, b"honest").is_err());
    assert_eq!(first.contacts.load(Ordering::SeqCst), 2);
    assert_eq!(middle.contacts.load(Ordering::SeqCst), 0);
    assert_eq!(final_endpoint.contacts.load(Ordering::SeqCst), 0);
    let looping = HttpFixture::new(302, b"", Some("/v1/ping".to_string()));
    assert!(looping.client().ping().is_err());
    assert_eq!(looping.contacts.load(Ordering::SeqCst), 1);
}

#[test]
fn file_get_boundary_and_optional_results() {
    let directory = TempDir::new();
    let client = directory.client();
    let expected = hash(b"honest");
    assert!(
        client
            .store_get_opt_bounded(&expected, Some(6))
            .unwrap()
            .is_none()
    );
    assert!(
        !directory.0.join("v1").exists(),
        "missing read mutated file backend"
    );
    client.store_put(&expected, b"honest").unwrap();
    assert_eq!(
        client.store_get_bounded(&expected, Some(6)).unwrap(),
        b"honest"
    );
    assert!(client.store_get_opt_bounded(&expected, Some(5)).is_err());
    std::fs::write(directory.0.join("v1/store").join(&expected), b"broken").unwrap();
    assert!(client.store_get_opt_bounded(&expected, Some(6)).is_err());
}

// The bounded outer RSS observation invokes this child with 1 KiB and 64 MiB sparse
// files. A post-read check consumes the full latter file; a streaming check does not.
#[test]
#[ignore = "child of the bounded F10 resource observation"]
fn file_get_resource_observation_child() {
    let bytes: u64 = std::env::var("GENESIS_F10_FILE_BYTES")
        .unwrap()
        .parse()
        .unwrap();
    let directory = TempDir::new();
    let client = directory.client();
    let expected = hash(b"expected");
    std::fs::create_dir_all(directory.0.join("v1/store")).unwrap();
    std::fs::File::create(directory.0.join("v1/store").join(&expected))
        .unwrap()
        .set_len(bytes)
        .unwrap();
    assert!(client.store_get_opt_bounded(&expected, Some(1024)).is_err());
}

#[test]
fn wasi_file_bridge_obeys_object_admission_in_an_isolated_process() {
    let directory = TempDir::new();
    let output = std::process::Command::new(std::env::current_exe().unwrap())
        .args(["--ignored", "--exact", "wasi_file_bridge_boundary_child"])
        .env(WASI_HTTP_BRIDGE_ROOT_ENV, directory.0.join("v1"))
        .output()
        .unwrap();
    assert!(
        output.status.success(),
        "{}{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
}

#[test]
#[ignore = "environment-isolated bridge child"]
fn wasi_file_bridge_boundary_child() {
    let client = RegistryClient::new("http://127.0.0.1:9", Some(Duration::from_secs(1))).unwrap();
    let root = PathBuf::from(std::env::var_os(WASI_HTTP_BRIDGE_ROOT_ENV).unwrap());
    let expected = hash(b"honest");
    assert!(
        client
            .store_get_opt_bounded(&expected, Some(6))
            .unwrap()
            .is_none()
    );
    assert!(!root.exists());
    client.store_put(&expected, b"honest").unwrap();
    assert_eq!(
        client.store_get_bounded(&expected, Some(6)).unwrap(),
        b"honest"
    );
    assert!(client.store_get_opt_bounded(&expected, Some(5)).is_err());
    std::fs::write(root.join("store").join(&expected), b"broken").unwrap();
    assert!(matches!(
        client.store_get_opt_bounded(&expected, Some(6)),
        Err(RegistryError::HashMismatch { .. })
    ));
    assert!(client.store_get_opt_bounded("../outside", Some(6)).is_err());
}
