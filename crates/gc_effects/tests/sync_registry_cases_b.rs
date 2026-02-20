use super::*;

#[test]
fn sync_pull_ref_conflict_requires_force() {
    let reg = Arc::new(MemRegistry::new());
    gc_registry::register_inproc("t2", reg.clone()).expect("register inproc");
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
    gc_registry::register_inproc("t3", reg).expect("register inproc");
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

#[test]
fn sync_remote_auth_is_enforced_when_registry_requires_bearer() {
    let reg = Arc::new(MemRegistry::new_with_required_bearer("secret-token"));
    gc_registry::register_inproc("t_sync_auth_required", reg.clone()).expect("register inproc");
    let (remote, remote_allow) = mk_remote("t_sync_auth_required");

    let td = tempfile::tempdir().unwrap();
    let store_dir = td.path().join("store");
    let refs_path = td.path().join("refs.gc");
    let caps = mk_caps_for_sync(&store_dir, &refs_path, &remote_allow);

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
    assert!(is_sealed_error(&ctx, &r.value, "core/sync/remote-auth"));
    assert_eq!(r.log.entries.len(), 1);
    assert_eq!(r.log.entries[0].op, "core/sync::pull");
    assert_eq!(r.log.entries[0].decision, Decision::Allow);
    let audit = reg.auth_audit();
    assert!(!audit.is_empty());
    assert!(!audit.last().expect("audit entry").bearer_present);
}

#[test]
fn sync_remote_auth_accepts_valid_bearer_token() {
    let reg = Arc::new(MemRegistry::new_with_required_bearer("secret-token"));
    gc_registry::register_inproc("t_sync_auth_ok", reg.clone()).expect("register inproc");
    let (remote, remote_allow) = mk_remote("t_sync_auth_ok");

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

    let td = tempfile::tempdir().unwrap();
    let store_dir = td.path().join("store");
    let refs_path = td.path().join("refs.gc");
    let caps =
        mk_caps_for_sync_with_auth_token(&store_dir, &refs_path, &remote_allow, "secret-token");

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
    assert!(
        !matches!(r.value, Value::Sealed { .. }),
        "pull returned error: {}",
        r.value.debug_repr()
    );
    assert_eq!(r.log.entries.len(), 1);
    assert_eq!(r.log.entries[0].op, "core/sync::pull");
    assert_eq!(r.log.entries[0].decision, Decision::Allow);
    let refs = gc_effects::RefsDb::open(&refs_path).unwrap();
    assert_eq!(refs.get("refs/heads/main").unwrap(), Some(commit_hex));
    let audit = reg.auth_audit();
    assert!(!audit.is_empty());
    assert!(audit.last().expect("audit entry").bearer_present);
}

#[test]
fn sync_remote_auth_is_enforced_when_registry_requires_basic_auth() {
    let reg = Arc::new(MemRegistry::new_with_required_basic("robot", "hunter2"));
    gc_registry::register_inproc("t_sync_auth_basic_required", reg.clone())
        .expect("register inproc");
    let (remote, remote_allow) = mk_remote("t_sync_auth_basic_required");

    let td = tempfile::tempdir().unwrap();
    let store_dir = td.path().join("store");
    let refs_path = td.path().join("refs.gc");
    let caps = mk_caps_for_sync(&store_dir, &refs_path, &remote_allow);

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
    assert!(is_sealed_error(&ctx, &r.value, "core/sync/remote-auth"));
    assert_eq!(r.log.entries.len(), 1);
    assert_eq!(r.log.entries[0].op, "core/sync::pull");
    assert_eq!(r.log.entries[0].decision, Decision::Allow);
    let audit = reg.auth_audit();
    assert!(!audit.is_empty());
    let last = audit.last().expect("audit entry");
    assert_eq!(last.basic_username.as_deref(), None);
    assert!(!last.basic_password_present);
}

#[test]
fn sync_remote_auth_accepts_valid_basic_credentials() {
    let reg = Arc::new(MemRegistry::new_with_required_basic("robot", "hunter2"));
    gc_registry::register_inproc("t_sync_auth_basic_ok", reg.clone()).expect("register inproc");
    let (remote, remote_allow) = mk_remote("t_sync_auth_basic_ok");

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

    let td = tempfile::tempdir().unwrap();
    let store_dir = td.path().join("store");
    let refs_path = td.path().join("refs.gc");
    let caps =
        mk_caps_for_sync_with_basic_auth(&store_dir, &refs_path, &remote_allow, "robot", "hunter2");

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
    assert!(
        !matches!(r.value, Value::Sealed { .. }),
        "pull returned error: {}",
        r.value.debug_repr()
    );
    assert_eq!(r.log.entries.len(), 1);
    assert_eq!(r.log.entries[0].op, "core/sync::pull");
    assert_eq!(r.log.entries[0].decision, Decision::Allow);
    let refs = gc_effects::RefsDb::open(&refs_path).unwrap();
    assert_eq!(refs.get("refs/heads/main").unwrap(), Some(commit_hex));
    let audit = reg.auth_audit();
    assert!(!audit.is_empty());
    let last = audit.last().expect("audit entry");
    assert_eq!(last.basic_username.as_deref(), Some("robot"));
    assert!(last.basic_password_present);
}

#[test]
fn sync_remote_auth_accepts_mtls_materials_from_policy_files() {
    let reg = Arc::new(MemRegistry::new_with_required_mtls());
    gc_registry::register_inproc("t_sync_auth_mtls_ok", reg.clone()).expect("register inproc");
    let (remote, remote_allow) = mk_remote("t_sync_auth_mtls_ok");

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

    let td = tempfile::tempdir().unwrap();
    let store_dir = td.path().join("store");
    let refs_path = td.path().join("refs.gc");
    let ca_pem = td.path().join("ca.pem");
    let id_pem = td.path().join("id.pem");
    std::fs::write(
        &ca_pem,
        "-----BEGIN CERTIFICATE-----\nZmFrZQ==\n-----END CERTIFICATE-----\n",
    )
    .unwrap();
    std::fs::write(
        &id_pem,
        "-----BEGIN PRIVATE KEY-----\nZmFrZQ==\n-----END PRIVATE KEY-----\n",
    )
    .unwrap();
    let caps =
        mk_caps_for_sync_with_mtls_files(&store_dir, &refs_path, &remote_allow, &ca_pem, &id_pem);

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
    assert!(
        !matches!(r.value, Value::Sealed { .. }),
        "pull returned error: {}",
        r.value.debug_repr()
    );
    let audit = reg.auth_audit();
    assert!(!audit.is_empty());
    let last = audit.last().expect("audit entry");
    assert!(last.mtls_ca_present);
    assert!(last.mtls_identity_present);
}

#[test]
fn sync_remote_auth_rejects_missing_mtls_pem_files() {
    let reg = Arc::new(MemRegistry::new_with_required_mtls());
    gc_registry::register_inproc("t_sync_auth_mtls_missing", reg.clone()).expect("register inproc");
    let (remote, remote_allow) = mk_remote("t_sync_auth_mtls_missing");

    let td = tempfile::tempdir().unwrap();
    let store_dir = td.path().join("store");
    let refs_path = td.path().join("refs.gc");
    let caps = mk_caps_for_sync_with_mtls_files(
        &store_dir,
        &refs_path,
        &remote_allow,
        &td.path().join("missing-ca.pem"),
        &td.path().join("missing-id.pem"),
    );

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
    assert!(is_sealed_error(&ctx, &r.value, "core/caps/policy-error"));
    assert_eq!(reg.auth_audit().len(), 0);
}

#[test]
fn sync_pull_enforces_max_artifact_bytes_budget() {
    let reg = Arc::new(MemRegistry::new());
    gc_registry::register_inproc("t_sync_limit", reg.clone()).expect("register inproc");
    let (remote, remote_allow) = mk_remote("t_sync_limit");

    let module_art = parse_term(&format!(
        r#"{{:kind "module" :v 1 :blob "{}"}}"#,
        "z".repeat(4096)
    ))
    .unwrap();
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

    let td = tempfile::tempdir().unwrap();
    let store_dir = td.path().join("store");
    let refs_path = td.path().join("refs.gc");
    let caps =
        mk_caps_for_sync_with_limits(&store_dir, &refs_path, &remote_allow, Some(256), Some(1024));

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
    assert!(is_sealed_error(&ctx, &r.value, "core/caps/resource-limit"));
}
