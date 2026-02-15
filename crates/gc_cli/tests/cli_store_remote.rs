use std::collections::BTreeMap;
use std::fs;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};

use assert_cmd::cargo::cargo_bin_cmd;
use axum::Json;
use axum::Router;
use axum::extract::{Path as AxPath, State};
use axum::http::StatusCode;
use axum::response::IntoResponse;
use axum::routing::{get, post};

#[derive(Debug, Clone, Default)]
struct RegistryState {
    store: BTreeMap<String, Vec<u8>>,
}

#[derive(Debug, Clone)]
struct AppState {
    inner: Arc<Mutex<RegistryState>>,
}

fn hash_bytes_hex(bytes: &[u8]) -> String {
    blake3::hash(bytes).to_hex().to_string()
}

#[derive(Debug, serde::Deserialize)]
struct StoreHasReq {
    hashes: Vec<String>,
}

#[derive(Debug, serde::Serialize)]
struct StoreHasResp {
    present: BTreeMap<String, bool>,
}

async fn store_has(State(st): State<AppState>, Json(req): Json<StoreHasReq>) -> impl IntoResponse {
    let g = st.inner.lock().expect("lock");
    let mut present = BTreeMap::new();
    for h in req.hashes {
        present.insert(h.clone(), g.store.contains_key(&h));
    }
    Json(StoreHasResp { present })
}

async fn store_get(State(st): State<AppState>, AxPath(hash): AxPath<String>) -> impl IntoResponse {
    let g = st.inner.lock().expect("lock");
    match g.store.get(&hash) {
        Some(b) => (StatusCode::OK, b.clone()).into_response(),
        None => StatusCode::NOT_FOUND.into_response(),
    }
}

struct TestRegistry {
    base: String,
    shutdown: Option<tokio::sync::oneshot::Sender<()>>,
    state: AppState,
    thread: Option<std::thread::JoinHandle<()>>,
}

impl TestRegistry {
    fn start() -> Self {
        let st = AppState {
            inner: Arc::new(Mutex::new(RegistryState::default())),
        };
        let app = Router::new()
            .route("/v1/store/has", post(store_has))
            .route("/v1/store/get/{hash}", get(store_get));

        let (base_tx, base_rx) = std::sync::mpsc::sync_channel::<String>(1);
        let (shutdown_tx, shutdown_rx) = tokio::sync::oneshot::channel::<()>();
        let st_for_thread = st.clone();
        let thread = std::thread::spawn(move || {
            let rt = tokio::runtime::Builder::new_multi_thread()
                .enable_all()
                .worker_threads(2)
                .build()
                .expect("runtime");
            rt.block_on(async move {
                let listener = tokio::net::TcpListener::bind(("127.0.0.1", 0))
                    .await
                    .expect("bind");
                let addr = listener.local_addr().expect("addr");
                let base = format!("http://{addr}/");
                base_tx.send(base).expect("send base");
                let srv = axum::serve(listener, app.with_state(st_for_thread))
                    .with_graceful_shutdown(async move {
                        let _ = shutdown_rx.await;
                    });
                let _ = srv.await;
            });
        });

        let base = base_rx.recv().expect("recv base");
        Self {
            base,
            shutdown: Some(shutdown_tx),
            state: st,
            thread: Some(thread),
        }
    }

    fn base_url(&self) -> String {
        self.base.clone()
    }

    fn normalized_base_v1(&self) -> String {
        format!("{}v1/", self.base_url())
    }

    fn insert(&self, hex: &str, bytes: Vec<u8>) {
        let mut g = self.state.inner.lock().expect("lock");
        g.store.insert(hex.to_string(), bytes);
    }
}

impl Drop for TestRegistry {
    fn drop(&mut self) {
        if let Some(tx) = self.shutdown.take() {
            let _ = tx.send(());
        }
        if let Some(t) = self.thread.take() {
            let _ = t.join();
        }
    }
}

fn write_caps(dir: &Path, remote: &str, remote_allow: &str) -> PathBuf {
    let caps = dir.join("caps.toml");
    fs::write(
        &caps,
        format!(
            r#"
allow = [
  "core/store::has",
  "core/store::get"
]

[store]
dir = "./.genesis/store"
remote = "{remote}"
remote_allow = ["{remote_allow}"]
allow_http = true
"#
        ),
    )
    .unwrap();
    caps
}

#[test]
fn store_get_and_has_can_read_through_to_remote_registry() {
    let reg = TestRegistry::start();

    let td = tempfile::tempdir().unwrap();
    let dir = td.path();

    let art = gc_coreform::parse_term("{:x 1 :y \"hi\"}").unwrap();
    let bytes = gc_coreform::print_term(&art).into_bytes();
    let hex = hash_bytes_hex(&bytes);
    reg.insert(&hex, bytes);

    let caps = write_caps(dir, &reg.base_url(), &reg.normalized_base_v1());

    cargo_bin_cmd!("genesis")
        .current_dir(dir)
        .args(["store", "--caps"])
        .arg(&caps)
        .args(["has"])
        .arg(&hex)
        .assert()
        .success()
        .stdout("true\n");

    cargo_bin_cmd!("genesis")
        .current_dir(dir)
        .args(["store", "--caps"])
        .arg(&caps)
        .args(["get"])
        .arg(&hex)
        .assert()
        .success()
        .stdout("{:x 1 :y \"hi\"}\n");

    let local = dir.join(".genesis").join("store").join(&hex);
    assert!(local.exists());
    let local_bytes = fs::read(local).unwrap();
    assert_eq!(hash_bytes_hex(&local_bytes), hex);
}
