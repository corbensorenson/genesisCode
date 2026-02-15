use std::collections::BTreeMap;
use std::sync::{Arc, Mutex};

use axum::Router;
use axum::body::Bytes;
use axum::extract::{Path, Query, State};
use axum::http::StatusCode;
use axum::response::IntoResponse;
use axum::routing::{get, post, put};
use axum::Json;
use gc_coreform::{Term, TermOrdKey, parse_term, print_term};
use gc_kernel::{EvalCtx, Value, eval_module, value_hash};
use gc_prelude::build_prelude;
use serde::{Deserialize, Serialize};

use gc_effects::{CapsPolicy, EffectLog, replay, run};

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

#[derive(Debug, Deserialize)]
struct StoreHasReq {
    hashes: Vec<String>,
}

#[derive(Debug, Serialize)]
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

async fn store_get(State(st): State<AppState>, Path(hash): Path<String>) -> impl IntoResponse {
    let g = st.inner.lock().expect("lock");
    match g.store.get(&hash) {
        Some(b) => (StatusCode::OK, b.clone()).into_response(),
        None => StatusCode::NOT_FOUND.into_response(),
    }
}

async fn store_put(
    State(st): State<AppState>,
    Path(hash): Path<String>,
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

#[derive(Debug, Deserialize)]
struct RefsGetQ {
    name: String,
}

async fn refs_get(State(st): State<AppState>, Query(q): Query<RefsGetQ>) -> impl IntoResponse {
    let g = st.inner.lock().expect("lock");
    let h = g.refs.get(&q.name).cloned();
    Json(serde_json::json!({ "name": q.name, "hash": h }))
}

#[derive(Debug, Deserialize)]
struct RefsListQ {
    prefix: Option<String>,
}

async fn refs_list(State(st): State<AppState>, Query(q): Query<RefsListQ>) -> impl IntoResponse {
    let g = st.inner.lock().expect("lock");
    let mut out = Vec::new();
    for (name, hash) in &g.refs {
        if let Some(p) = &q.prefix
            && !name.starts_with(p)
        {
            continue;
        }
        out.push(serde_json::json!({ "name": name, "hash": hash }));
    }
    Json(serde_json::json!({ "refs": out }))
}

#[derive(Debug, Deserialize)]
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
    // Enforce policy gating server-side (minimal but real):
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
                Err(_) => return (StatusCode::FORBIDDEN, "evidence bytes not utf8").into_response(),
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
            .route("/v1/refs/list", get(refs_list))
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

                let srv = axum::serve(listener, app.with_state(st_for_thread)).with_graceful_shutdown(async move {
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
        // Caller can give a remote spec without v1; client/runner normalizes.
        self.base.clone()
    }

    fn normalized_base_v1(&self) -> String {
        gc_registry::normalize_remote_base(&self.base_url())
            .expect("normalize")
            .as_str()
            .to_string()
    }

    fn put_artifact(&self, bytes: &[u8]) -> String {
        let hex = hash_bytes_hex(bytes);
        let mut g = self.state.inner.lock().expect("lock");
        g.store.insert(hex.clone(), bytes.to_vec());
        hex
    }

    fn has(&self, hex: &str) -> bool {
        let g = self.state.inner.lock().expect("lock");
        g.store.contains_key(hex)
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

fn mk_caps_for_sync(store_dir: &std::path::Path, refs_path: &std::path::Path, remote_allow: &str) -> CapsPolicy {
    let s = format!(
        r#"
allow = ["core/sync::push", "core/sync::pull"]

[store]
dir = "{store_dir}"

[refs]
path = "{refs_path}"

[op."core/sync::push"]
remote_allow = ["{remote_allow}"]
allow_http = true

[op."core/sync::pull"]
remote_allow = ["{remote_allow}"]
allow_http = true
"#,
        store_dir = store_dir.display(),
        refs_path = refs_path.display(),
        remote_allow = remote_allow
    );
    CapsPolicy::from_toml_str(&s).expect("caps")
}

fn mk_prog(op: &str, payload: &Term) -> (Vec<Term>, [u8; 32]) {
    // (def prog (core/effect::perform 'op (quote payload) (fn (r) (core/effect::pure r)))) prog
    let op_t = Term::list(vec![Term::symbol("quote"), Term::symbol(op)]);
    let payload_t = Term::list(vec![Term::symbol("quote"), payload.clone()]);
    let k = Term::list(vec![
        Term::symbol("fn"),
        Term::list(vec![Term::symbol("r")]),
        Term::list(vec![Term::symbol("core/effect::pure"), Term::symbol("r")]),
    ]);
    let perform = Term::list(vec![Term::symbol("core/effect::perform"), op_t, payload_t, k]);
    let forms = vec![
        Term::list(vec![Term::symbol("def"), Term::symbol("prog"), perform]),
        Term::symbol("prog"),
    ];
    let h = gc_coreform::hash_module(&forms);
    (forms, h)
}

fn mk_policy_artifact() -> Term {
    // A minimal policy that requires unit-tests for main; no signatures.
    parse_term(
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
    )
    .expect("policy term")
}

fn mk_evidence_with_data(data_hex: &str) -> Term {
    parse_term(&format!(
        r#"{{
          :type :vcs/evidence
          :v 1
          :kind :unit-tests
          :inputs []
          :outputs []
          :data "{data_hex}"
        }}"#
    ))
    .expect("evidence term")
}

fn mk_patch_with_value(value_hex: &str) -> Term {
    parse_term(&format!(
        r#"{{
          :type :vcs/patch
          :v 1
          :ops [
            {{ :op :replace :path [] :value "{value_hex}" }}
          ]
        }}"#
    ))
    .expect("patch term")
}

fn mk_snapshot(module_hex: &str, module_h: [u8; 32]) -> Term {
    Term::Map(
        [
            (TermOrdKey(Term::symbol(":type")), Term::symbol(":vcs/snapshot")),
            (TermOrdKey(Term::symbol(":v")), Term::Int(1.into())),
            (TermOrdKey(Term::symbol(":kind")), Term::symbol(":package")),
            (TermOrdKey(Term::symbol(":pkg/name")), Term::Str("my-lib".to_string())),
            (TermOrdKey(Term::symbol(":pkg/version")), Term::Str("0.1.0".to_string())),
            (
                TermOrdKey(Term::symbol(":modules")),
                Term::Vector(vec![Term::Map(
                    [
                        (TermOrdKey(Term::symbol(":path")), Term::Str("m.gc".to_string())),
                        (TermOrdKey(Term::symbol(":hash")), Term::Str(module_hex.to_string())),
                        (TermOrdKey(Term::symbol(":module-h")), Term::Bytes(module_h.to_vec())),
                    ]
                    .into_iter()
                    .collect(),
                )]),
            ),
            (
                TermOrdKey(Term::symbol(":obligations")),
                Term::Vector(vec![Term::symbol("core/obligation::unit-tests")]),
            ),
        ]
        .into_iter()
        .collect(),
    )
}

fn mk_commit(result_hex: &str, patch_hex: &str, evidence_hex: &str) -> Term {
    parse_term(&format!(
        r#"{{
          :type :vcs/commit
          :v 1
          :parents []
          :target {{ :kind :package :name "my-lib" }}
          :base nil
          :patch "{patch_hex}"
          :result "{result_hex}"
          :obligations [core/obligation::unit-tests]
          :evidence ["{evidence_hex}"]
          :attestations []
          :message "sync test"
        }}"#
    ))
    .expect("commit term")
}

fn is_sealed_error(ctx: &EvalCtx, v: &Value, code: &str) -> bool {
    let Some(proto) = ctx.protocol else { return false };
    let Value::Sealed { token, payload } = v else { return false };
    if *token != proto.error {
        return false;
    }
    let Value::Data(Term::Map(m)) = payload.as_ref() else { return false };
    matches!(
        m.get(&TermOrdKey(Term::symbol(":error/code"))),
        Some(Term::Str(s)) if s == code
    )
}

#[test]
fn sync_push_then_pull_transfers_full_closure_and_updates_refs() {
    let reg = TestRegistry::start();
    let remote = reg.base_url();
    let remote_allow = reg.normalized_base_v1();

    // Policy artifact is server-known (preloaded).
    let policy_t = mk_policy_artifact();
    let policy_hex = reg.put_artifact(print_term(&policy_t).as_bytes());

    // Local workspace dirs.
    let td = tempfile::tempdir().unwrap();
    let store_dir = td.path().join("store");
    let refs_path = td.path().join("refs.gc");
    let caps = mk_caps_for_sync(&store_dir, &refs_path, &remote_allow);

    // Build a local commit closure with patch and evidence pointing at extra artifacts.
    let local_store = gc_effects::ArtifactStore::open(&store_dir).unwrap();

    let module_art = parse_term(r#"{:kind "module" :v 1 :content "ok"}"#).unwrap();
    let module_hex = local_store
        .put_bytes(print_term(&module_art).as_bytes())
        .unwrap();
    let module_h = gc_coreform::hash_term(&module_art);

    let extra_patch_val = parse_term(r#"{:kind "extra" :v 1 :note "patch-ref"}"#).unwrap();
    let extra_patch_hex = local_store
        .put_bytes(print_term(&extra_patch_val).as_bytes())
        .unwrap();

    let extra_data_val = parse_term(r#"{:kind "extra" :v 1 :note "evidence-data"}"#).unwrap();
    let extra_data_hex = local_store
        .put_bytes(print_term(&extra_data_val).as_bytes())
        .unwrap();

    let patch_t = mk_patch_with_value(&extra_patch_hex);
    let patch_hex = local_store.put_bytes(print_term(&patch_t).as_bytes()).unwrap();

    let evidence_t = mk_evidence_with_data(&extra_data_hex);
    let evidence_hex = local_store
        .put_bytes(print_term(&evidence_t).as_bytes())
        .unwrap();

    let snap_t = mk_snapshot(&module_hex, module_h);
    let snap_hex = local_store.put_bytes(print_term(&snap_t).as_bytes()).unwrap();

    let commit_t = mk_commit(&snap_hex, &patch_hex, &evidence_hex);
    let commit_hex = local_store
        .put_bytes(print_term(&commit_t).as_bytes())
        .unwrap();

    // Push and set remote main ref.
    let push_payload = parse_term(&format!(
        r#"{{
          :remote "{remote}"
          :roots ["{commit_hex}"]
          :depth 0
          :set-refs [
            {{ :name "refs/heads/main" :hash "{commit_hex}" :policy "{policy_hex}" :expected-old nil }}
          ]
        }}"#
    ))
    .unwrap();
    let (push_forms, push_h) = mk_prog("core/sync::push", &push_payload);
    let mut ctx = EvalCtx::new();
    let prelude = build_prelude(&mut ctx);
    let mut env = prelude.env;
    let prog = eval_module(&mut ctx, &mut env, &push_forms).unwrap();
    let r = run(&mut ctx, &caps, prog, push_h, "gc_effects-test".to_string()).unwrap();

    assert!(
        !matches!(r.value, Value::Sealed { .. }),
        "push returned error: {}",
        r.value.debug_repr()
    );
    assert_eq!(reg.ref_get("refs/heads/main"), Some(commit_hex.clone()));
    assert!(reg.has(&commit_hex));
    assert!(reg.has(&snap_hex));
    assert!(reg.has(&module_hex));
    assert!(reg.has(&patch_hex));
    assert!(reg.has(&evidence_hex));
    assert!(reg.has(&extra_patch_hex));
    assert!(reg.has(&extra_data_hex));

    // Pull into a fresh local store/refs.
    let td2 = tempfile::tempdir().unwrap();
    let store_dir2 = td2.path().join("store");
    let refs_path2 = td2.path().join("refs.gc");
    let caps2 = mk_caps_for_sync(&store_dir2, &refs_path2, &remote_allow);

    let pull_payload = parse_term(&format!(
        r#"{{
          :remote "{remote}"
          :refs ["refs/heads/main"]
          :depth 0
          :force true
        }}"#
    ))
    .unwrap();
    let (pull_forms, pull_h) = mk_prog("core/sync::pull", &pull_payload);
    let mut ctx2 = EvalCtx::new();
    let prelude2 = build_prelude(&mut ctx2);
    let mut env2 = prelude2.env;
    let prog2 = eval_module(&mut ctx2, &mut env2, &pull_forms).unwrap();
    let r2 = run(
        &mut ctx2,
        &caps2,
        prog2,
        pull_h,
        "gc_effects-test".to_string(),
    )
    .unwrap();
    assert!(
        !matches!(r2.value, Value::Sealed { .. }),
        "pull returned error: {}",
        r2.value.debug_repr()
    );

    let store2 = gc_effects::ArtifactStore::open(&store_dir2).unwrap();
    for h in [
        &commit_hex,
        &snap_hex,
        &module_hex,
        &patch_hex,
        &evidence_hex,
        &extra_patch_hex,
        &extra_data_hex,
    ] {
        assert!(store2.path_for(h).exists(), "missing pulled artifact {h}");
        store2.verify_hex(h).unwrap();
    }
    let refs2 = gc_effects::RefsDb::open(&refs_path2).unwrap();
    assert_eq!(refs2.get("refs/heads/main").unwrap(), Some(commit_hex.clone()));

    // Ensure pull is deterministic with replay (log roundtrip).
    let log_term = r2.log.to_term();
    let log2 = EffectLog::from_term(&log_term).unwrap();
    assert_eq!(log2.entries.len(), r2.log.entries.len());

    let mut ctx3 = EvalCtx::new();
    let prelude3 = build_prelude(&mut ctx3);
    let mut env3 = prelude3.env;
    let prog3 = eval_module(&mut ctx3, &mut env3, &pull_forms).unwrap();
    let v3 = replay(&mut ctx3, prog3, &log2).unwrap();
    assert_eq!(value_hash(&r2.value), value_hash(&v3));
}

#[test]
fn sync_pull_ref_conflict_requires_force() {
    let reg = TestRegistry::start();
    let remote = reg.base_url();
    let remote_allow = reg.normalized_base_v1();

    let policy_t = mk_policy_artifact();
    let policy_hex = reg.put_artifact(print_term(&policy_t).as_bytes());

    // Remote commit, snapshot, patch, evidence (minimal closure).
    let module_art = parse_term(r#"{:kind "module" :v 1 :content "ok"}"#).unwrap();
    let module_hex = reg.put_artifact(print_term(&module_art).as_bytes());
    let module_h = gc_coreform::hash_term(&module_art);
    let snap_t = mk_snapshot(&module_hex, module_h);
    let snap_hex = reg.put_artifact(print_term(&snap_t).as_bytes());
    let patch_t = parse_term(r#"{:type :vcs/patch :v 1 :ops []}"#).unwrap();
    let patch_hex = reg.put_artifact(print_term(&patch_t).as_bytes());
    let ev_t = parse_term(r#"{:type :vcs/evidence :v 1 :kind :unit-tests :data nil}"#).unwrap();
    let ev_hex = reg.put_artifact(print_term(&ev_t).as_bytes());
    let commit_t = mk_commit(&snap_hex, &patch_hex, &ev_hex);
    let commit_hex = reg.put_artifact(print_term(&commit_t).as_bytes());

    // Set remote ref (server-gated).
    {
        let mut g = reg.state.inner.lock().unwrap();
        g.refs.insert("refs/heads/main".to_string(), commit_hex.clone());
    }

    // Local store/refs already have a different main head.
    let td = tempfile::tempdir().unwrap();
    let store_dir = td.path().join("store");
    let refs_path = td.path().join("refs.gc");
    let caps = mk_caps_for_sync(&store_dir, &refs_path, &remote_allow);
    let refs = gc_effects::RefsDb::open(&refs_path).unwrap();
    refs.set("refs/heads/main", Some(&"0".repeat(64)), None).unwrap();

    // Pull without force should return a sealed ERROR with code core/refs/conflict.
    let pull_payload = parse_term(&format!(
        r#"{{
          :remote "{remote}"
          :refs ["refs/heads/main"]
          :depth 0
          :force false
        }}"#
    ))
    .unwrap();
    let (pull_forms, pull_h) = mk_prog("core/sync::pull", &pull_payload);
    let mut ctx = EvalCtx::new();
    let prelude = build_prelude(&mut ctx);
    let mut env = prelude.env;
    let prog = eval_module(&mut ctx, &mut env, &pull_forms).unwrap();
    let r = run(&mut ctx, &caps, prog, pull_h, "gc_effects-test".to_string()).unwrap();
    assert!(is_sealed_error(&ctx, &r.value, "core/refs/conflict"));

    // With force it should succeed and update local ref.
    let pull_payload2 = parse_term(&format!(
        r#"{{
          :remote "{remote}"
          :refs ["refs/heads/main"]
          :depth 0
          :force true
        }}"#
    ))
    .unwrap();
    let (pull_forms2, pull_h2) = mk_prog("core/sync::pull", &pull_payload2);
    let mut ctx2 = EvalCtx::new();
    let prelude2 = build_prelude(&mut ctx2);
    let mut env2 = prelude2.env;
    let prog2 = eval_module(&mut ctx2, &mut env2, &pull_forms2).unwrap();
    let r2 = run(
        &mut ctx2,
        &caps,
        prog2,
        pull_h2,
        "gc_effects-test".to_string(),
    )
    .unwrap();
    assert!(!matches!(r2.value, Value::Sealed { .. }), "force pull failed");
    assert_eq!(refs.get("refs/heads/main").unwrap(), Some(commit_hex));

    // Keep policy_hex referenced (server-side policy must exist; future tests can use it).
    assert!(!policy_hex.is_empty());
}

#[test]
fn sync_remote_allowlist_is_enforced() {
    let reg = TestRegistry::start();
    let remote = reg.base_url();

    let td = tempfile::tempdir().unwrap();
    let store_dir = td.path().join("store");
    let refs_path = td.path().join("refs.gc");

    let caps = mk_caps_for_sync(&store_dir, &refs_path, "https://example.invalid/v1/");

    let payload = parse_term(&format!(
        r#"{{
          :remote "{remote}"
          :refs ["refs/heads/main"]
          :depth 0
          :force true
        }}"#
    ))
    .unwrap();
    let (forms, h) = mk_prog("core/sync::pull", &payload);
    let mut ctx = EvalCtx::new();
    let prelude = build_prelude(&mut ctx);
    let mut env = prelude.env;
    let prog = eval_module(&mut ctx, &mut env, &forms).unwrap();
    let r = run(&mut ctx, &caps, prog, h, "gc_effects-test".to_string()).unwrap();
    assert!(is_sealed_error(&ctx, &r.value, "core/sync/remote-denied"));
}
