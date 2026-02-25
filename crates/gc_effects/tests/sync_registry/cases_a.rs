use super::*;

#[test]
fn sync_push_then_pull_transfers_full_closure_and_updates_refs() {
    let reg = Arc::new(MemRegistry::new());
    gc_registry::register_inproc("t1", reg.clone()).expect("register inproc");
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
    let local_policy_hex = local_store
        .put_bytes(print_term(&policy_t).as_bytes())
        .unwrap();
    assert_eq!(local_policy_hex, policy_hex);

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
fn sync_push_uses_chunked_upload_when_remote_advertises_small_chunks() {
    let reg = Arc::new(MemRegistry::new_with_max_chunk_bytes(32));
    gc_registry::register_inproc("t_sync_chunked", reg.clone()).expect("register inproc");
    let (remote, remote_allow) = mk_remote("t_sync_chunked");

    let policy_t = mk_policy_artifact();
    let policy_hex = reg.put_artifact(print_term(&policy_t).as_bytes());

    let td = tempfile::tempdir().unwrap();
    let store_dir = td.path().join("store");
    let refs_path = td.path().join("refs.gc");
    let caps = mk_caps_for_sync(&store_dir, &refs_path, &remote_allow);

    let local_store = gc_effects::ArtifactStore::open(&store_dir).unwrap();
    let local_policy_hex = local_store
        .put_bytes(print_term(&policy_t).as_bytes())
        .unwrap();
    assert_eq!(local_policy_hex, policy_hex);

    let module_art = parse_term(r#"{:kind "module" :v 1 :content "ok"}"#).unwrap();
    let module_hex = local_store
        .put_bytes(print_term(&module_art).as_bytes())
        .unwrap();
    let module_h = gc_coreform::hash_term(&module_art);
    let patch_t = mk_patch_with_value(&module_hex);
    let patch_hex = local_store
        .put_bytes(print_term(&patch_t).as_bytes())
        .unwrap();
    let evidence_t = mk_evidence_with_data(&module_hex);
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
    assert_eq!(reg.ref_get("refs/heads/main"), Some(commit_hex));

    let (starts, chunks, finishes) = reg.upload_counts();
    assert!(starts > 0, "expected chunked upload start calls");
    assert!(chunks >= starts, "expected chunk upload calls");
    assert!(finishes > 0, "expected chunked upload finish calls");
}

#[test]
fn sync_push_rejects_duplicate_set_ref_targets_in_payload() {
    let reg = Arc::new(MemRegistry::new());
    gc_registry::register_inproc("t_sync_dup_set_ref", reg).expect("register inproc");
    let (remote, remote_allow) = mk_remote("t_sync_dup_set_ref");

    let td = tempfile::tempdir().unwrap();
    let store_dir = td.path().join("store");
    let refs_path = td.path().join("refs.gc");
    let caps = mk_caps_for_sync(&store_dir, &refs_path, &remote_allow);

    let commit_hex = "a".repeat(64);
    let policy_hex = "b".repeat(64);
    let payload = parse_term(&format!(
        r#"{{
          :remote "{remote}"
          :roots ["{commit_hex}"]
          :depth 0
          :set-refs [
            {{ :name "refs/heads/main" :hash "{commit_hex}" :policy "{policy_hex}" }}
            {{ :name "refs/heads/main" :hash "{commit_hex}" :policy "{policy_hex}" }}
          ]
        }}"#
    ))
    .unwrap();
    let (forms, h) = mk_prog("core/sync::push", &payload);
    let mut ctx = EvalCtx::new();
    let prelude = build_prelude(&mut ctx);
    let mut env = prelude.env;
    let prog = eval_module(&mut ctx, &mut env, &forms).unwrap();
    let r = run(&mut ctx, &caps, prog, h, "gc_effects-test".to_string()).unwrap();
    assert!(is_sealed_error(&ctx, &r.value, "core/sync/bad-payload"));
}

#[test]
fn sync_push_set_refs_preflight_fails_before_upload() {
    let reg = Arc::new(MemRegistry::new());
    gc_registry::register_inproc("t_sync_preflight", reg.clone()).expect("register inproc");
    let (remote, remote_allow) = mk_remote("t_sync_preflight");

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
    let snap_t = mk_snapshot(&module_hex, module_h);
    let snap_hex = local_store
        .put_bytes(print_term(&snap_t).as_bytes())
        .unwrap();
    let patch_t = parse_term(r#"{:type :vcs/patch :v 1 :ops []}"#).unwrap();
    let patch_hex = local_store
        .put_bytes(print_term(&patch_t).as_bytes())
        .unwrap();
    let commit_bad_t = parse_term(&format!(
        r#"{{
          :type :vcs/commit
          :v 1
          :parents []
          :target {{ :kind :package :name "my-lib" }}
          :base nil
          :patch "{patch_hex}"
          :result "{snap_hex}"
          :obligations []
          :evidence []
          :attestations []
          :message "missing obligation"
        }}"#
    ))
    .unwrap();
    let commit_bad_hex = local_store
        .put_bytes(print_term(&commit_bad_t).as_bytes())
        .unwrap();

    let policy_t = mk_policy_artifact();
    let policy_hex = local_store
        .put_bytes(print_term(&policy_t).as_bytes())
        .unwrap();

    let payload = parse_term(&format!(
        r#"{{
          :remote "{remote}"
          :roots ["{commit_bad_hex}"]
          :depth 0
          :set-refs [
            {{ :name "refs/heads/main" :hash "{commit_bad_hex}" :policy "{policy_hex}" }}
          ]
        }}"#
    ))
    .unwrap();
    let (forms, h) = mk_prog("core/sync::push", &payload);
    let mut ctx = EvalCtx::new();
    let prelude = build_prelude(&mut ctx);
    let mut env = prelude.env;
    let prog = eval_module(&mut ctx, &mut env, &forms).unwrap();
    let r = run(&mut ctx, &caps, prog, h, "gc_effects-test".to_string()).unwrap();
    assert!(is_sealed_error(
        &ctx,
        &r.value,
        "core/refs/missing-obligation"
    ));
    assert!(!reg.has(&commit_bad_hex));
    assert_eq!(reg.ref_get("refs/heads/main"), None);
}

#[test]
fn pkg_publish_validates_policy_and_pushes_commit_closure() {
    let reg = Arc::new(MemRegistry::new());
    gc_registry::register_inproc("t_pkg_publish", reg.clone()).expect("register inproc");
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
    let (forms, h) = mk_prog("core/pkg-low::publish", &payload);
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
    let (forms_bad, h_bad) = mk_prog("core/pkg-low::publish", &payload_bad);
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
fn pkg_publish_enforces_obligation_bound_evidence_kinds() {
    let reg = Arc::new(MemRegistry::new());
    gc_registry::register_inproc("t_pkg_publish_kinds", reg.clone()).expect("register inproc");
    let (remote, remote_allow) = mk_remote("t_pkg_publish_kinds");

    let td = tempfile::tempdir().unwrap();
    let store_dir = td.path().join("store");
    let refs_path = td.path().join("refs.gc");
    let caps = mk_caps_for_pkg_publish(&store_dir, &refs_path, &remote_allow);
    let local_store = gc_effects::ArtifactStore::open(&store_dir).unwrap();

    let policy_t = mk_policy_artifact_with_obligation_evidence_kinds();
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

    let ev_unit = mk_evidence_of_kind(":unit-tests");
    let ev_unit_hex = local_store
        .put_bytes(print_term(&ev_unit).as_bytes())
        .unwrap();
    let ev_effect = mk_evidence_of_kind(":effect-log");
    let ev_effect_hex = local_store
        .put_bytes(print_term(&ev_effect).as_bytes())
        .unwrap();

    let snap_t = mk_snapshot(&module_hex, module_h);
    let snap_hex = local_store
        .put_bytes(print_term(&snap_t).as_bytes())
        .unwrap();

    let commit_missing_kind_t = mk_commit(&snap_hex, &patch_hex, &ev_unit_hex);
    let commit_missing_kind_hex = local_store
        .put_bytes(print_term(&commit_missing_kind_t).as_bytes())
        .unwrap();
    let refs_db = gc_effects::RefsDb::open(&refs_path).unwrap();
    refs_db
        .set("refs/heads/main", Some(&commit_missing_kind_hex), None)
        .unwrap();

    let payload_bad = parse_term(&format!(
        r#"{{
          :remote "{remote}"
          :ref "refs/heads/main"
          :policy "{policy_hex}"
        }}"#
    ))
    .unwrap();
    let (forms_bad, h_bad) = mk_prog("core/pkg-low::publish", &payload_bad);
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
        "core/pkg/missing-evidence-kind"
    ));
    assert_eq!(reg.ref_get("refs/heads/main"), None);

    let commit_ok_t = parse_term(&format!(
        r#"{{
          :type :vcs/commit
          :v 1
          :parents []
          :target {{ :kind :package :name "my-lib" }}
          :base nil
          :patch "{patch_hex}"
          :result "{snap_hex}"
          :obligations [core/obligation::unit-tests]
          :evidence ["{ev_unit_hex}" "{ev_effect_hex}"]
          :attestations []
          :message "has required kinds"
        }}"#
    ))
    .unwrap();
    let commit_ok_hex = local_store
        .put_bytes(print_term(&commit_ok_t).as_bytes())
        .unwrap();
    refs_db
        .set("refs/heads/main", Some(&commit_ok_hex), None)
        .unwrap();

    let payload_ok = parse_term(&format!(
        r#"{{
          :remote "{remote}"
          :ref "refs/heads/main"
          :policy "{policy_hex}"
        }}"#
    ))
    .unwrap();
    let (forms_ok, h_ok) = mk_prog("core/pkg-low::publish", &payload_ok);
    let mut ctx_ok = EvalCtx::new();
    let prelude_ok = build_prelude(&mut ctx_ok);
    let mut env_ok = prelude_ok.env;
    let prog_ok = eval_module(&mut ctx_ok, &mut env_ok, &forms_ok).unwrap();
    let r_ok = run(
        &mut ctx_ok,
        &caps,
        prog_ok,
        h_ok,
        "gc_effects-test".to_string(),
    )
    .unwrap();
    assert!(
        !matches!(r_ok.value, Value::Sealed { .. }),
        "publish returned error: {}",
        r_ok.value.debug_repr()
    );
    assert_eq!(reg.ref_get("refs/heads/main"), Some(commit_ok_hex));
}

#[test]
fn pkg_bridge_creates_signed_commit_and_updates_lock() {
    let td = tempfile::tempdir().unwrap();
    let workspace_dir = td.path().join("workspace");
    std::fs::create_dir_all(&workspace_dir).unwrap();
    let store_dir = td.path().join("store");
    let refs_path = td.path().join("refs.gc");
    let caps = mk_caps_for_pkg_bridge(&workspace_dir, &store_dir, &refs_path);

    let lock_path = workspace_dir.join("genesis.lock");
    let lock = gc_pkg::GenesisLock::empty("workspace");
    std::fs::write(&lock_path, lock.to_toml_canonical()).unwrap();

    let payload = parse_term(
        r#"{
          :ecosystem "crates"
          :name "serde"
          :version "1.0.217"
          :source "serde@1.0.217"
          :source-hash "0123456789abcdef0123456789abcdef0123456789abcdef0123456789abcdef"
          :key-id "mirror-key"
          :public-key "abcdef0123456789abcdef0123456789abcdef0123456789abcdef0123456789"
          :lock "genesis.lock"
          :dep-name "serde"
          :registry "upstream"
        }"#,
    )
    .unwrap();
    let (forms, h) = mk_prog("core/pkg-low::bridge", &payload);
    let mut ctx = EvalCtx::new();
    let prelude = build_prelude(&mut ctx);
    let mut env = prelude.env;
    let prog = eval_module(&mut ctx, &mut env, &forms).unwrap();
    let r = run(&mut ctx, &caps, prog, h, "gc_effects-test".to_string()).unwrap();
    assert!(
        !matches!(r.value, Value::Sealed { .. }),
        "bridge returned error: {}",
        r.value.debug_repr()
    );

    let Term::Map(mm) = r.value.to_term_for_log(None) else {
        panic!("bridge result should be a map");
    };
    let get_str = |key: &str| -> String {
        let Some(Term::Str(value)) = mm.get(&TermOrdKey(Term::symbol(key))) else {
            panic!("bridge result missing {key}");
        };
        value.clone()
    };

    let commit_h = get_str(":commit");
    let snapshot_h = get_str(":snapshot");
    let provenance_root = get_str(":provenance-root");
    let conversion_evidence = get_str(":conversion-evidence");
    let attestation_h = get_str(":attestation");
    let lock_h = get_str(":lock-h");
    assert_eq!(commit_h.len(), 64);
    assert_eq!(snapshot_h.len(), 64);
    assert_eq!(provenance_root.len(), 64);
    assert_eq!(conversion_evidence.len(), 64);
    assert_eq!(attestation_h.len(), 64);
    assert_eq!(lock_h.len(), 64);

    let store = gc_effects::ArtifactStore::open(&store_dir).unwrap();
    let commit_bytes = store.get_bytes(&commit_h).unwrap();
    let commit_t = parse_term(&String::from_utf8(commit_bytes).unwrap()).unwrap();
    let Term::Map(commit_mm) = commit_t else {
        panic!("commit should be a map");
    };
    assert_eq!(
        commit_mm.get(&TermOrdKey(Term::symbol(":result"))),
        Some(&Term::Str(snapshot_h.clone()))
    );
    assert_eq!(
        commit_mm.get(&TermOrdKey(Term::symbol(":evidence"))),
        Some(&Term::Vector(vec![Term::Str(conversion_evidence.clone())]))
    );
    assert_eq!(
        commit_mm.get(&TermOrdKey(Term::symbol(":attestations"))),
        Some(&Term::Vector(vec![Term::Str(attestation_h.clone())]))
    );

    let snapshot_bytes = store.get_bytes(&snapshot_h).unwrap();
    let snapshot_t = parse_term(&String::from_utf8(snapshot_bytes).unwrap()).unwrap();
    let Term::Map(snapshot_mm) = snapshot_t else {
        panic!("snapshot should be a map");
    };
    assert_eq!(
        snapshot_mm.get(&TermOrdKey(Term::symbol(":pkg/name"))),
        Some(&Term::Str("serde".to_string()))
    );
    let Some(Term::Map(meta_mm)) = snapshot_mm.get(&TermOrdKey(Term::symbol(":meta"))) else {
        panic!("snapshot meta should be a map");
    };
    assert_eq!(
        meta_mm.get(&TermOrdKey(Term::symbol(":bridge/provenance-root"))),
        Some(&Term::Str(provenance_root.clone()))
    );

    let lock_after = gc_pkg::GenesisLock::load(&lock_path).unwrap();
    let req = lock_after
        .requirements
        .get("serde")
        .expect("missing serde requirement");
    assert_eq!(req.selector, format!("commit:{commit_h}"));
    assert_eq!(req.update_policy, gc_pkg::UpdatePolicy::Manual);
    assert_eq!(req.strategy, gc_pkg::ResolutionStrategy::Pinned);
    assert_eq!(req.registry.as_deref(), Some("upstream"));
    let locked = lock_after
        .locked
        .get("serde")
        .expect("missing locked serde");
    assert_eq!(locked.commit.as_deref(), Some(commit_h.as_str()));
    assert_eq!(locked.snapshot, snapshot_h);
    assert_eq!(locked.registry.as_deref(), Some("upstream"));
    assert_eq!(locked.source_selector, format!("commit:{commit_h}"));
    assert_eq!(
        lock_after.artifacts.get("bridge.serde.provenance_root"),
        Some(&provenance_root)
    );
    assert_eq!(
        lock_after.artifacts.get("bridge.serde.conversion_evidence"),
        Some(&conversion_evidence)
    );
    assert_eq!(
        lock_after.artifacts.get("bridge.serde.attestation"),
        Some(&attestation_h)
    );
    assert_eq!(
        lock_after.artifacts.get("bridge.serde.commit"),
        Some(&commit_h)
    );
    let expected_lock_h = blake3::hash(lock_after.to_toml_canonical().as_bytes())
        .to_hex()
        .to_string();
    assert_eq!(lock_h, expected_lock_h);
}

#[test]
fn pkg_bridge_rejects_lock_without_dep_name() {
    let td = tempfile::tempdir().unwrap();
    let workspace_dir = td.path().join("workspace");
    std::fs::create_dir_all(&workspace_dir).unwrap();
    let store_dir = td.path().join("store");
    let refs_path = td.path().join("refs.gc");
    let caps = mk_caps_for_pkg_bridge(&workspace_dir, &store_dir, &refs_path);

    let payload = parse_term(
        r#"{
          :ecosystem "crates"
          :name "serde"
          :version "1.0.217"
          :source "serde@1.0.217"
          :source-hash "0123456789abcdef0123456789abcdef0123456789abcdef0123456789abcdef"
          :key-id "mirror-key"
          :public-key "abcdef0123456789abcdef0123456789abcdef0123456789abcdef0123456789"
          :lock "genesis.lock"
        }"#,
    )
    .unwrap();
    let (forms, h) = mk_prog("core/pkg-low::bridge", &payload);
    let mut ctx = EvalCtx::new();
    let prelude = build_prelude(&mut ctx);
    let mut env = prelude.env;
    let prog = eval_module(&mut ctx, &mut env, &forms).unwrap();
    let r = run(&mut ctx, &caps, prog, h, "gc_effects-test".to_string()).unwrap();
    assert!(is_sealed_error(&ctx, &r.value, "core/pkg/bad-payload"));
}
