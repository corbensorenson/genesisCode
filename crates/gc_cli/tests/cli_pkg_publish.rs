use std::collections::BTreeMap;
use std::fs;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};

use assert_cmd::cargo::cargo_bin_cmd;
use axum::Json;
use axum::Router;
use axum::body::Bytes;
use axum::extract::{Path as AxPath, Query, State};
use axum::http::StatusCode;
use axum::response::IntoResponse;
use axum::routing::{get, post, put};
use gc_coreform::parse_term;

#[derive(Debug, Clone, Default)]
struct RegistryState {
    store: BTreeMap<String, Vec<u8>>,
    refs: BTreeMap<String, String>,
}

#[derive(Debug, Clone)]
struct AppState {
    inner: Arc<Mutex<RegistryState>>,
}

fn hash_bytes_hex(bytes: &[u8]) -> String {
    blake3::hash(bytes).to_hex().to_string()
}

async fn ping() -> impl IntoResponse {
    Json(serde_json::json!({
        "ok": true,
        "version": "0.1",
        "hash": "blake3-256",
        "max_chunk_bytes": 4_194_304u64,
    }))
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

async fn store_put(
    State(st): State<AppState>,
    AxPath(hash): AxPath<String>,
    body: Bytes,
) -> impl IntoResponse {
    let bytes = body.to_vec();
    let got = hash_bytes_hex(&bytes);
    if got != hash {
        return (StatusCode::BAD_REQUEST, "hash mismatch").into_response();
    }
    let mut g = st.inner.lock().expect("lock");
    g.store.entry(hash).or_insert(bytes);
    StatusCode::OK.into_response()
}

#[derive(Debug, serde::Deserialize)]
struct RefsGetQ {
    name: String,
}

async fn refs_get(State(st): State<AppState>, Query(q): Query<RefsGetQ>) -> impl IntoResponse {
    let g = st.inner.lock().expect("lock");
    let h = g.refs.get(&q.name).cloned();
    Json(serde_json::json!({ "name": q.name, "hash": h }))
}

#[derive(Debug, serde::Deserialize)]
struct RefsSetReq {
    name: String,
    hash: String,
    policy: String,
    expected_old: Option<String>,
}

async fn refs_set(
    State(st): State<AppState>,
    Json(req): Json<RefsSetReq>,
) -> axum::response::Response {
    // Minimal but real server-side policy enforcement:
    // - policy artifact must exist and parse
    // - ref must match a policy class and not be frozen
    // - commit must exist and satisfy required obligations
    // - evidence artifacts referenced by commit must exist and parse as evidence
    {
        let g = st.inner.lock().expect("lock");

        let pol_bytes = match g.store.get(&req.policy) {
            Some(b) => b.clone(),
            None => return (StatusCode::FORBIDDEN, "policy not found").into_response(),
        };
        let pol_s = match String::from_utf8(pol_bytes) {
            Ok(s) => s,
            Err(_) => return (StatusCode::FORBIDDEN, "policy bytes not utf8").into_response(),
        };
        let pol_term = match parse_term(&pol_s) {
            Ok(t) => t,
            Err(_) => return (StatusCode::FORBIDDEN, "bad policy term").into_response(),
        };
        let pol = match gc_vcs::Policy::from_term(&pol_term) {
            Ok(p) => p,
            Err(_) => return (StatusCode::FORBIDDEN, "bad policy schema").into_response(),
        };
        if pol.is_frozen_ref(&req.name) {
            return (StatusCode::FORBIDDEN, "ref frozen").into_response();
        }
        let class = match pol.class_for_ref(&req.name) {
            Some(c) => c,
            None => return (StatusCode::FORBIDDEN, "no matching policy class").into_response(),
        };

        let commit_bytes = match g.store.get(&req.hash) {
            Some(b) => b.clone(),
            None => return (StatusCode::FORBIDDEN, "commit not found").into_response(),
        };
        let commit_s = match String::from_utf8(commit_bytes) {
            Ok(s) => s,
            Err(_) => return (StatusCode::FORBIDDEN, "commit bytes not utf8").into_response(),
        };
        let commit_term = match parse_term(&commit_s) {
            Ok(t) => t,
            Err(_) => return (StatusCode::FORBIDDEN, "bad commit term").into_response(),
        };
        let commit = match gc_vcs::Commit::from_term(&commit_term) {
            Ok(c) => c,
            Err(_) => return (StatusCode::FORBIDDEN, "bad commit schema").into_response(),
        };

        for req_ob in &class.required_obligations {
            if !commit.obligations.iter().any(|o| o == req_ob) {
                return (StatusCode::FORBIDDEN, "missing obligation").into_response();
            }
        }
        if !class.required_obligations.is_empty() && commit.evidence.is_empty() {
            return (StatusCode::FORBIDDEN, "missing evidence").into_response();
        }
        for ev_h in &commit.evidence {
            let ev_bytes = match g.store.get(ev_h) {
                Some(b) => b.clone(),
                None => return (StatusCode::FORBIDDEN, "evidence not found").into_response(),
            };
            let ev_s = match String::from_utf8(ev_bytes) {
                Ok(s) => s,
                Err(_) => {
                    return (StatusCode::FORBIDDEN, "evidence bytes not utf8").into_response();
                }
            };
            let ev_t = match parse_term(&ev_s) {
                Ok(t) => t,
                Err(_) => return (StatusCode::FORBIDDEN, "bad evidence term").into_response(),
            };
            if gc_vcs::Evidence::from_term(&ev_t).is_err() {
                return (StatusCode::FORBIDDEN, "bad evidence schema").into_response();
            }
        }
    }

    // CAS update.
    let mut g = st.inner.lock().expect("lock");
    let cur = g.refs.get(&req.name).cloned();
    if let Some(exp) = &req.expected_old
        && cur.as_deref() != Some(exp.as_str())
    {
        return StatusCode::CONFLICT.into_response();
    }
    g.refs.insert(req.name.clone(), req.hash.clone());
    Json(serde_json::json!({ "ok": true, "name": req.name, "hash": req.hash })).into_response()
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
            .route("/v1/ping", get(ping))
            .route("/v1/store/has", post(store_has))
            .route("/v1/store/get/{hash}", get(store_get))
            .route("/v1/store/put/{hash}", put(store_put))
            .route("/v1/refs/get", get(refs_get))
            .route("/v1/refs/set", post(refs_set));

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
        // Registry client normalizes remote base to .../v1/ before comparing to remote_allow.
        format!("{}v1/", self.base_url())
    }

    fn ref_get(&self, name: &str) -> Option<String> {
        let g = self.state.inner.lock().expect("lock");
        g.refs.get(name).cloned()
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

fn write_caps(dir: &Path, remote_allow: &str) -> PathBuf {
    let caps = dir.join("caps.toml");
    fs::write(
        &caps,
        format!(
            r#"
allow = [
  "core/store::put",
  "core/sync::push"
]

[store]
dir = "./.genesis/store"

[refs]
path = "./.genesis/refs.gc"

[op."core/sync::push"]
remote_allow = ["{remote_allow}"]
allow_http = true
"#
        ),
    )
    .unwrap();
    caps
}

fn cli_store_put(dir: &Path, caps: &Path, term_src: &str, filename: &str) -> String {
    let p = dir.join(filename);
    fs::write(&p, term_src).unwrap();
    let out = cargo_bin_cmd!("genesis")
        .current_dir(dir)
        .args(["store", "--caps"])
        .arg(caps)
        .args(["put", "--input"])
        .arg(filename)
        .assert()
        .success()
        .get_output()
        .stdout
        .clone();
    String::from_utf8(out).unwrap().trim().to_string()
}

fn set_local_ref(dir: &Path, commit_hex: &str) {
    let refs_path = dir.join(".genesis").join("refs.gc");
    let rdb = gc_effects::RefsDb::open(&refs_path).unwrap();
    let _ = rdb.set("refs/heads/main", Some(commit_hex), None).unwrap();
}

#[test]
fn pkg_publish_is_policy_gated_and_advances_remote_ref_on_success() {
    let reg = TestRegistry::start();
    let remote = reg.base_url();
    let remote_allow = reg.normalized_base_v1();

    let td = tempfile::tempdir().unwrap();
    let dir = td.path();
    let caps = write_caps(dir, &remote_allow);

    // Policy: main requires unit-tests.
    let policy_hex = cli_store_put(
        dir,
        &caps,
        r#"
{
  :type :vcs/policy
  :v 1
  :name "policy:test"
  :refs { :frozen-prefixes [] }
  :classes {
    :dev  { :patterns ["refs/**/heads/*"] :exclude ["refs/**/heads/main"] :required-obligations [] }
    :main { :patterns ["refs/**/heads/main"] :required-obligations [core/obligation::unit-tests] :require-signatures false }
    :tags { :patterns ["refs/**/tags/*"] :required-obligations [core/obligation::unit-tests] :require-signatures false }
  }
}
"#,
        "policy.gc",
    );

    let patch_hex = cli_store_put(dir, &caps, r#"{:type :vcs/patch :v 1 :ops []}"#, "patch.gc");
    let snap_hex = cli_store_put(
        dir,
        &caps,
        r#"{:type :vcs/snapshot :v 1 :kind :package :pkg/name "x" :pkg/version "0" :modules [] :obligations []}"#,
        "snap.gc",
    );

    // Commit missing evidence -> publish must refuse locally.
    let commit_bad = cli_store_put(
        dir,
        &caps,
        &format!(
            r#"{{
  :type :vcs/commit
  :v 1
  :parents []
  :target {{ :kind :package :name "x" }}
  :base nil
  :patch "{patch_hex}"
  :result "{snap_hex}"
  :obligations [core/obligation::unit-tests]
  :evidence []
  :attestations []
  :message "bad"
}}"#
        ),
        "commit_bad.gc",
    );
    set_local_ref(dir, &commit_bad);

    cargo_bin_cmd!("genesis")
        .current_dir(dir)
        .args(["pkg", "--caps"])
        .arg(&caps)
        .args([
            "publish",
            "--remote",
            &remote,
            "--ref",
            "refs/heads/main",
            "--policy",
            &policy_hex,
        ])
        .assert()
        .code(30);
    assert_eq!(reg.ref_get("refs/heads/main"), None);

    // Commit with evidence -> publish succeeds and advances remote.
    let evidence_hex = cli_store_put(
        dir,
        &caps,
        r#"{:type :vcs/evidence :v 1 :kind :unit-tests :inputs [] :outputs [] :data nil}"#,
        "evidence.gc",
    );
    let commit_ok = cli_store_put(
        dir,
        &caps,
        &format!(
            r#"{{
  :type :vcs/commit
  :v 1
  :parents []
  :target {{ :kind :package :name "x" }}
  :base nil
  :patch "{patch_hex}"
  :result "{snap_hex}"
  :obligations [core/obligation::unit-tests]
  :evidence ["{evidence_hex}"]
  :attestations []
  :message "ok"
}}"#
        ),
        "commit_ok.gc",
    );
    set_local_ref(dir, &commit_ok);

    cargo_bin_cmd!("genesis")
        .current_dir(dir)
        .args(["pkg", "--caps"])
        .arg(&caps)
        .args([
            "publish",
            "--remote",
            &remote,
            "--ref",
            "refs/heads/main",
            "--policy",
            &policy_hex,
        ])
        .assert()
        .success()
        .stdout(predicates::str::is_match("^[0-9a-f]{64}\n$").unwrap());

    assert_eq!(reg.ref_get("refs/heads/main"), Some(commit_ok));
}
