use std::collections::BTreeMap;
use std::sync::{Arc, Mutex};

use gc_coreform::{Term, TermOrdKey, parse_term, print_term};
use gc_kernel::{EvalCtx, Value, eval_module, value_hash};
use gc_prelude::build_prelude;

use gc_effects::{CapsPolicy, EffectLog, replay, run};

#[derive(Debug, Default)]
struct RegistryState {
    store: BTreeMap<String, Vec<u8>>,
    refs: BTreeMap<String, String>,
}

#[derive(Debug)]
struct MemRegistry {
    st: Mutex<RegistryState>,
}

fn hash_bytes_hex(bytes: &[u8]) -> String {
    blake3::hash(bytes).to_hex().to_string()
}

impl MemRegistry {
    fn new() -> Self {
        Self {
            st: Mutex::new(RegistryState::default()),
        }
    }

    fn put_artifact(&self, bytes: &[u8]) -> String {
        let hex = hash_bytes_hex(bytes);
        let mut g = self.st.lock().expect("lock");
        g.store.insert(hex.clone(), bytes.to_vec());
        hex
    }

    fn has(&self, hex: &str) -> bool {
        let g = self.st.lock().expect("lock");
        g.store.contains_key(hex)
    }

    fn ref_get(&self, name: &str) -> Option<String> {
        let g = self.st.lock().expect("lock");
        g.refs.get(name).cloned()
    }
}

impl gc_registry::InProcRegistry for MemRegistry {
    fn ping(&self) -> Result<gc_registry::PingResp, gc_registry::RegistryError> {
        Ok(gc_registry::PingResp {
            ok: true,
            version: "0.1".to_string(),
            hash: "blake3-256".to_string(),
            max_chunk_bytes: Some(4_194_304),
        })
    }

    fn store_has(
        &self,
        hashes: &[String],
    ) -> Result<BTreeMap<String, bool>, gc_registry::RegistryError> {
        let g = self.st.lock().expect("lock");
        let mut out = BTreeMap::new();
        for h in hashes {
            out.insert(h.clone(), g.store.contains_key(h));
        }
        Ok(out)
    }

    fn store_get(&self, hash: &str) -> Result<Vec<u8>, gc_registry::RegistryError> {
        let g = self.st.lock().expect("lock");
        g.store
            .get(hash)
            .cloned()
            .ok_or_else(|| gc_registry::RegistryError::Http("store/get: status 404".to_string()))
    }

    fn store_put(&self, hash: &str, bytes: &[u8]) -> Result<(), gc_registry::RegistryError> {
        let got = hash_bytes_hex(bytes);
        if got != hash {
            return Err(gc_registry::RegistryError::Protocol(
                "store/put: hash mismatch".to_string(),
            ));
        }
        let mut g = self.st.lock().expect("lock");
        g.store.entry(hash.to_string()).or_insert(bytes.to_vec());
        Ok(())
    }

    fn refs_get(&self, name: &str) -> Result<Option<String>, gc_registry::RegistryError> {
        let g = self.st.lock().expect("lock");
        Ok(g.refs.get(name).cloned())
    }

    fn refs_list(
        &self,
        prefix: Option<&str>,
    ) -> Result<Vec<gc_registry::RefsListEntry>, gc_registry::RegistryError> {
        let g = self.st.lock().expect("lock");
        let mut out = Vec::new();
        for (name, hash) in &g.refs {
            if let Some(p) = prefix
                && !name.starts_with(p)
            {
                continue;
            }
            out.push(gc_registry::RefsListEntry {
                name: name.clone(),
                hash: Some(hash.clone()),
            });
        }
        out.sort_by(|a, b| a.name.cmp(&b.name));
        Ok(out)
    }

    fn refs_set(
        &self,
        req: &gc_registry::RefsSetReq<'_>,
    ) -> Result<gc_registry::RefsSetResp, gc_registry::RegistryError> {
        // Server-side policy gating (minimal but real):
        // - policy artifact must exist and parse
        // - ref must match a policy class and not be frozen
        // - commit must exist and satisfy required obligations
        // - evidence artifacts referenced by commit must exist and parse as evidence
        // - commit pointers (base/patch/result) must exist
        let g = self.st.lock().expect("lock");

        let pol_bytes = g.store.get(req.policy).ok_or_else(|| {
            gc_registry::RegistryError::Protocol("refs/set: policy not found".to_string())
        })?;
        let pol_s = String::from_utf8(pol_bytes.clone()).map_err(|_| {
            gc_registry::RegistryError::Protocol("refs/set: bad policy utf8".to_string())
        })?;
        let pol_term = parse_term(&pol_s).map_err(|e| {
            gc_registry::RegistryError::Protocol(format!("refs/set: bad policy term: {e}"))
        })?;
        let pol = gc_vcs::Policy::from_term(&pol_term).map_err(|e| {
            gc_registry::RegistryError::Protocol(format!("refs/set: bad policy schema: {e}"))
        })?;

        if pol.is_frozen_ref(req.name) {
            return Err(gc_registry::RegistryError::Protocol(
                "refs/set: ref frozen".to_string(),
            ));
        }
        let class = pol.class_for_ref(req.name).ok_or_else(|| {
            gc_registry::RegistryError::Protocol("refs/set: no matching policy class".to_string())
        })?;

        let commit_bytes = g.store.get(req.hash).ok_or_else(|| {
            gc_registry::RegistryError::Protocol("refs/set: commit not found".to_string())
        })?;
        let commit_s = String::from_utf8(commit_bytes.clone()).map_err(|_| {
            gc_registry::RegistryError::Protocol("refs/set: bad commit utf8".to_string())
        })?;
        let commit_term = parse_term(&commit_s).map_err(|e| {
            gc_registry::RegistryError::Protocol(format!("refs/set: bad commit term: {e}"))
        })?;
        let commit = gc_vcs::Commit::from_term(&commit_term).map_err(|e| {
            gc_registry::RegistryError::Protocol(format!("refs/set: bad commit schema: {e}"))
        })?;

        if let Some(b) = commit.base.as_ref()
            && !g.store.contains_key(b)
        {
            return Err(gc_registry::RegistryError::Protocol(
                "refs/set: commit base missing".to_string(),
            ));
        }
        if !g.store.contains_key(&commit.patch) {
            return Err(gc_registry::RegistryError::Protocol(
                "refs/set: commit patch missing".to_string(),
            ));
        }
        if !g.store.contains_key(&commit.result) {
            return Err(gc_registry::RegistryError::Protocol(
                "refs/set: commit result snapshot missing".to_string(),
            ));
        }

        for req_ob in &class.required_obligations {
            if !commit.obligations.iter().any(|o| o == req_ob) {
                return Err(gc_registry::RegistryError::Protocol(
                    "refs/set: missing obligation".to_string(),
                ));
            }
        }
        if !class.required_obligations.is_empty() && commit.evidence.is_empty() {
            return Err(gc_registry::RegistryError::Protocol(
                "refs/set: missing evidence".to_string(),
            ));
        }
        for ev_h in &commit.evidence {
            let ev_bytes = g.store.get(ev_h).ok_or_else(|| {
                gc_registry::RegistryError::Protocol("refs/set: evidence not found".to_string())
            })?;
            let ev_s = String::from_utf8(ev_bytes.clone()).map_err(|_| {
                gc_registry::RegistryError::Protocol("refs/set: evidence utf8".to_string())
            })?;
            let ev_t = parse_term(&ev_s).map_err(|e| {
                gc_registry::RegistryError::Protocol(format!("refs/set: bad evidence term: {e}"))
            })?;
            gc_vcs::Evidence::from_term(&ev_t).map_err(|e| {
                gc_registry::RegistryError::Protocol(format!("refs/set: bad evidence schema: {e}"))
            })?;
        }

        drop(g);

        let mut g = self.st.lock().expect("lock");
        let cur = g.refs.get(req.name).cloned();
        if let Some(exp) = req.expected_old
            && cur.as_deref() != Some(exp)
        {
            return Err(gc_registry::RegistryError::Http(
                "refs/set: status 409".to_string(),
            ));
        }
        g.refs.insert(req.name.to_string(), req.hash.to_string());

        Ok(gc_registry::RefsSetResp {
            ok: true,
            name: req.name.to_string(),
            hash: req.hash.to_string(),
        })
    }
}

fn mk_caps_for_sync(
    store_dir: &std::path::Path,
    refs_path: &std::path::Path,
    remote_allow: &str,
) -> CapsPolicy {
    let s = format!(
        r#"
allow = ["core/sync::push", "core/sync::pull"]

[store]
dir = "{store_dir}"

[refs]
path = "{refs_path}"

[op."core/sync::push"]
remote_allow = ["{remote_allow}"]

[op."core/sync::pull"]
remote_allow = ["{remote_allow}"]
"#,
        store_dir = store_dir.display(),
        refs_path = refs_path.display(),
        remote_allow = remote_allow
    );
    CapsPolicy::from_toml_str(&s).expect("caps")
}

fn mk_caps_for_pkg_publish(
    store_dir: &std::path::Path,
    refs_path: &std::path::Path,
    remote_allow: &str,
) -> CapsPolicy {
    let s = format!(
        r#"
allow = ["core/pkg::publish"]

[store]
dir = "{store_dir}"

[refs]
path = "{refs_path}"

[op."core/pkg::publish"]
remote_allow = ["{remote_allow}"]
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
    let perform = Term::list(vec![
        Term::symbol("core/effect::perform"),
        op_t,
        payload_t,
        k,
    ]);
    let forms = vec![
        Term::list(vec![Term::symbol("def"), Term::symbol("prog"), perform]),
        Term::symbol("prog"),
    ];
    let h = gc_coreform::hash_module(&forms);
    (forms, h)
}

fn mk_policy_artifact() -> Term {
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
            (
                TermOrdKey(Term::symbol(":type")),
                Term::symbol(":vcs/snapshot"),
            ),
            (TermOrdKey(Term::symbol(":v")), Term::Int(1.into())),
            (TermOrdKey(Term::symbol(":kind")), Term::symbol(":package")),
            (
                TermOrdKey(Term::symbol(":pkg/name")),
                Term::Str("my-lib".to_string()),
            ),
            (
                TermOrdKey(Term::symbol(":pkg/version")),
                Term::Str("0.1.0".to_string()),
            ),
            (
                TermOrdKey(Term::symbol(":modules")),
                Term::Vector(vec![Term::Map(
                    [
                        (
                            TermOrdKey(Term::symbol(":path")),
                            Term::Str("m.gc".to_string()),
                        ),
                        (
                            TermOrdKey(Term::symbol(":hash")),
                            Term::Str(module_hex.to_string()),
                        ),
                        (
                            TermOrdKey(Term::symbol(":module-h")),
                            Term::Bytes(module_h.to_vec().into()),
                        ),
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
    let Some(proto) = ctx.protocol else {
        return false;
    };
    let Value::Sealed { token, payload } = v else {
        return false;
    };
    if *token != proto.error {
        return false;
    }
    let Value::Data(Term::Map(m)) = payload.as_ref() else {
        return false;
    };
    matches!(
        m.get(&TermOrdKey(Term::symbol(":error/code"))),
        Some(Term::Str(s)) if s == code
    )
}

fn mk_remote(id: &str) -> (String, String) {
    let remote = format!("inproc://{id}/");
    let allow = gc_registry::normalize_remote_base(&remote)
        .expect("normalize")
        .as_str()
        .to_string();
    (remote, allow)
}

#[test]
fn sync_push_then_pull_transfers_full_closure_and_updates_refs() {
    let reg = Arc::new(MemRegistry::new());
    gc_registry::register_inproc("t1", reg.clone());
    let (remote, remote_allow) = mk_remote("t1");

    // Policy artifact is remote-known (preloaded).
    let policy_t = mk_policy_artifact();
    let policy_hex = reg.put_artifact(print_term(&policy_t).as_bytes());

    // Local workspace dirs.
    let td = tempfile::tempdir().unwrap();
    let store_dir = td.path().join("store");
    let refs_path = td.path().join("refs.gc");
    let caps = mk_caps_for_sync(&store_dir, &refs_path, &remote_allow);

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
    let patch_hex = local_store
        .put_bytes(print_term(&patch_t).as_bytes())
        .unwrap();

    let evidence_t = mk_evidence_with_data(&extra_data_hex);
    let evidence_hex = local_store
        .put_bytes(print_term(&evidence_t).as_bytes())
        .unwrap();

    let snap_t = mk_snapshot(&module_hex, module_h);
    let snap_hex = local_store
        .put_bytes(print_term(&snap_t).as_bytes())
        .unwrap();

    let commit_t = mk_commit(&snap_hex, &patch_hex, &evidence_hex);
    let commit_hex = local_store
        .put_bytes(print_term(&commit_t).as_bytes())
        .unwrap();

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
    for h in [
        &commit_hex,
        &snap_hex,
        &module_hex,
        &patch_hex,
        &evidence_hex,
        &extra_patch_hex,
        &extra_data_hex,
    ] {
        assert!(reg.has(h), "missing remote artifact {h}");
    }

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
    assert_eq!(
        refs2.get("refs/heads/main").unwrap(),
        Some(commit_hex.clone())
    );

    // Replay the pull run for determinism.
    let log_term = r2.log.to_term();
    let log2 = EffectLog::from_term(&log_term).unwrap();
    let mut ctx3 = EvalCtx::new();
    let prelude3 = build_prelude(&mut ctx3);
    let mut env3 = prelude3.env;
    let prog3 = eval_module(&mut ctx3, &mut env3, &pull_forms).unwrap();
    let v3 = replay(&mut ctx3, prog3, &log2).unwrap();
    assert_eq!(value_hash(&r2.value), value_hash(&v3));
}

#[test]
fn pkg_publish_validates_policy_and_pushes_commit_closure() {
    let reg = Arc::new(MemRegistry::new());
    gc_registry::register_inproc("t_pkg_publish", reg.clone());
    let (remote, remote_allow) = mk_remote("t_pkg_publish");

    let td = tempfile::tempdir().unwrap();
    let store_dir = td.path().join("store");
    let refs_path = td.path().join("refs.gc");
    let caps = mk_caps_for_pkg_publish(&store_dir, &refs_path, &remote_allow);
    let local_store = gc_effects::ArtifactStore::open(&store_dir).unwrap();

    let policy_t = mk_policy_artifact();
    let policy_hex = local_store
        .put_bytes(print_term(&policy_t).as_bytes())
        .unwrap();

    let module_art = parse_term(r#"{:kind "module" :v 1 :content "ok"}"#).unwrap();
    let module_hex = local_store
        .put_bytes(print_term(&module_art).as_bytes())
        .unwrap();
    let module_h = gc_coreform::hash_term(&module_art);

    let patch_t = parse_term(r#"{:type :vcs/patch :v 1 :ops []}"#).unwrap();
    let patch_hex = local_store
        .put_bytes(print_term(&patch_t).as_bytes())
        .unwrap();

    let evidence_t =
        parse_term(r#"{:type :vcs/evidence :v 1 :kind :unit-tests :data nil}"#).unwrap();
    let evidence_hex = local_store
        .put_bytes(print_term(&evidence_t).as_bytes())
        .unwrap();

    let snap_t = mk_snapshot(&module_hex, module_h);
    let snap_hex = local_store
        .put_bytes(print_term(&snap_t).as_bytes())
        .unwrap();

    let commit_t = mk_commit(&snap_hex, &patch_hex, &evidence_hex);
    let commit_hex = local_store
        .put_bytes(print_term(&commit_t).as_bytes())
        .unwrap();
    let refs_db = gc_effects::RefsDb::open(&refs_path).unwrap();
    refs_db
        .set("refs/heads/main", Some(&commit_hex), None)
        .unwrap();

    let payload = parse_term(&format!(
        r#"{{
          :remote "{remote}"
          :ref "refs/heads/main"
          :policy "{policy_hex}"
          :depth 0
        }}"#
    ))
    .unwrap();
    let (forms, h) = mk_prog("core/pkg::publish", &payload);
    let mut ctx = EvalCtx::new();
    let prelude = build_prelude(&mut ctx);
    let mut env = prelude.env;
    let prog = eval_module(&mut ctx, &mut env, &forms).unwrap();
    let r = run(&mut ctx, &caps, prog, h, "gc_effects-test".to_string()).unwrap();
    assert!(
        !matches!(r.value, Value::Sealed { .. }),
        "publish returned error: {}",
        r.value.debug_repr()
    );
    assert_eq!(reg.ref_get("refs/heads/main"), Some(commit_hex.clone()));
    assert!(reg.has(&commit_hex));
    assert!(reg.has(&policy_hex));
    let Term::Map(publish_map) = r.value.to_term_for_log(None) else {
        panic!("publish result should be a map");
    };
    assert_eq!(
        publish_map.get(&TermOrdKey(Term::symbol(":ok"))),
        Some(&Term::Bool(true))
    );
    assert_eq!(
        publish_map.get(&TermOrdKey(Term::symbol(":commit"))),
        Some(&Term::Str(commit_hex.clone()))
    );
    assert_eq!(
        publish_map.get(&TermOrdKey(Term::symbol(":ref"))),
        Some(&Term::Str("refs/heads/main".to_string()))
    );

    // Swap local head to a commit missing evidence; publish must fail before remote mutation.
    let bad_commit_t = parse_term(&format!(
        r#"{{
          :type :vcs/commit
          :v 1
          :parents []
          :target {{ :kind :package :name "my-lib" }}
          :base nil
          :patch "{patch_hex}"
          :result "{snap_hex}"
          :obligations [core/obligation::unit-tests]
          :evidence []
          :attestations []
          :message "missing evidence"
        }}"#
    ))
    .unwrap();
    let bad_commit_hex = local_store
        .put_bytes(print_term(&bad_commit_t).as_bytes())
        .unwrap();
    refs_db
        .set("refs/heads/main", Some(&bad_commit_hex), None)
        .unwrap();

    let payload_bad = parse_term(&format!(
        r#"{{
          :remote "{remote}"
          :ref "refs/heads/main"
          :policy "{policy_hex}"
        }}"#
    ))
    .unwrap();
    let (forms_bad, h_bad) = mk_prog("core/pkg::publish", &payload_bad);
    let mut ctx_bad = EvalCtx::new();
    let prelude_bad = build_prelude(&mut ctx_bad);
    let mut env_bad = prelude_bad.env;
    let prog_bad = eval_module(&mut ctx_bad, &mut env_bad, &forms_bad).unwrap();
    let r_bad = run(
        &mut ctx_bad,
        &caps,
        prog_bad,
        h_bad,
        "gc_effects-test".to_string(),
    )
    .unwrap();
    assert!(is_sealed_error(
        &ctx_bad,
        &r_bad.value,
        "core/pkg/missing-evidence"
    ));
    assert_eq!(reg.ref_get("refs/heads/main"), Some(commit_hex));
}

#[test]
fn sync_pull_ref_conflict_requires_force() {
    let reg = Arc::new(MemRegistry::new());
    gc_registry::register_inproc("t2", reg.clone());
    let (remote, remote_allow) = mk_remote("t2");

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
    {
        let mut g = reg.st.lock().unwrap();
        g.refs
            .insert("refs/heads/main".to_string(), commit_hex.clone());
    }

    // Local store/refs already have a different main head.
    let td = tempfile::tempdir().unwrap();
    let store_dir = td.path().join("store");
    let refs_path = td.path().join("refs.gc");
    let caps = mk_caps_for_sync(&store_dir, &refs_path, &remote_allow);
    let refs = gc_effects::RefsDb::open(&refs_path).unwrap();
    refs.set("refs/heads/main", Some(&"0".repeat(64)), None)
        .unwrap();

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
    assert!(
        !matches!(r2.value, Value::Sealed { .. }),
        "force pull failed"
    );
    assert_eq!(refs.get("refs/heads/main").unwrap(), Some(commit_hex));
}

#[test]
fn sync_remote_allowlist_is_enforced() {
    let reg = Arc::new(MemRegistry::new());
    gc_registry::register_inproc("t3", reg);
    let (remote, _remote_allow) = mk_remote("t3");

    let td = tempfile::tempdir().unwrap();
    let store_dir = td.path().join("store");
    let refs_path = td.path().join("refs.gc");
    let caps = mk_caps_for_sync(&store_dir, &refs_path, "inproc://not-allowed/v1/");

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
