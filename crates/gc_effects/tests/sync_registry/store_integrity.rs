use super::*;

#[test]
fn corrupt_later_batch_member_prevents_all_batch_installations() {
    for workers in [1, 4] {
        let reg = Arc::new(MemRegistry::new());
        let good = parse_term(r#"{:kind "module" :v 1 :content "good"}"#).unwrap();
        let bad = parse_term(r#"{:kind "module" :v 1 :content "bad"}"#).unwrap();
        let good_hash = reg.put_artifact(print_term(&good).as_bytes());
        let bad_hash = hash_bytes_hex(print_term(&bad).as_bytes());
        reg.st
            .lock()
            .unwrap()
            .store
            .insert(bad_hash.clone(), b"corrupt".to_vec());
        let mut root = mk_snapshot(&good_hash, gc_coreform::hash_term(&good));
        let Term::Map(ref mut fields) = root else {
            unreachable!()
        };
        let mut other = mk_snapshot(&bad_hash, gc_coreform::hash_term(&bad));
        let Term::Map(ref mut other_fields) = other else {
            unreachable!()
        };
        let Term::Vector(mut modules) = other_fields
            .remove(&TermOrdKey(Term::symbol(":modules")))
            .unwrap()
        else {
            unreachable!()
        };
        let Term::Map(ref mut module) = modules[0] else {
            unreachable!()
        };
        module.insert(
            TermOrdKey(Term::symbol(":path")),
            Term::Str("bad.gc".to_string()),
        );
        let Term::Vector(roots) = fields
            .get_mut(&TermOrdKey(Term::symbol(":modules")))
            .unwrap()
        else {
            unreachable!()
        };
        roots.extend(modules);
        assert!(
            gc_vcs::Snapshot::from_term(&root).is_ok(),
            "fixture must traverse its module references"
        );
        let id = format!("sync-corrupt-batch-{workers}");
        gc_registry::register_inproc(&id, reg).unwrap();
        struct Registration(String);
        impl Drop for Registration {
            fn drop(&mut self) {
                gc_registry::unregister_inproc(&self.0).unwrap();
            }
        }
        let _registration = Registration(id.clone());
        let (remote, allowed) = mk_remote(&id);
        let td = tempfile::tempdir().unwrap();
        let store_dir = td.path().join("store");
        let refs_path = td.path().join("refs.gc");
        let caps = selfhost_sync_caps(&format!(
            r#"
allow = ["core/sync::pull"]
[store]
dir = "{}"
[refs]
path = "{}"
[op."core/sync::pull"]
remote_allow = ["{allowed}"]
transfer_workers = {workers}
"#,
            store_dir.display(),
            refs_path.display()
        ));
        let store = gc_effects::ArtifactStore::open(&store_dir).unwrap();
        let root_hash = store.put_bytes(print_term(&root).as_bytes()).unwrap();
        let payload = parse_term(&format!(
            r#"{{:remote "{remote}" :roots ["{root_hash}"] :depth 0}}"#
        ))
        .unwrap();
        let (forms, hash) = mk_prog("core/sync::pull", &payload);
        let mut ctx = EvalCtx::new();
        let mut env = build_prelude(&mut ctx).env;
        let prog = eval_module(&mut ctx, &mut env, &forms).unwrap();
        let result = run(&mut ctx, &caps, prog, hash, "gc_effects-test".to_string()).unwrap();
        assert!(
            is_sealed_error(&ctx, &result.value, "core/sync/hash-mismatch"),
            "{}",
            result.value.debug_repr()
        );
        assert!(
            !store.path_for(&good_hash).exists(),
            "honest member installed before the batch was admitted"
        );
        assert!(!store.path_for(&hash_bytes_hex(b"corrupt")).exists());
        assert_eq!(std::fs::read_dir(&store_dir).unwrap().count(), 1);
    }
}

#[test]
fn corrupt_download_leaves_destination_inventory_unchanged_and_replays() {
    let reg = Arc::new(MemRegistry::new());
    let expected = hash_bytes_hex(b"honest");
    reg.st
        .lock()
        .unwrap()
        .store
        .insert(expected.clone(), b"corrupt".to_vec());
    gc_registry::register_inproc("sync-corrupt-boundary", reg).unwrap();
    struct Registration;
    impl Drop for Registration {
        fn drop(&mut self) {
            gc_registry::unregister_inproc("sync-corrupt-boundary").unwrap();
        }
    }
    let _registration = Registration;
    let (remote, allowed) = mk_remote("sync-corrupt-boundary");
    let td = tempfile::tempdir().unwrap();
    let store_dir = td.path().join("store");
    let refs_path = td.path().join("refs.gc");
    let caps = mk_caps_for_sync(&store_dir, &refs_path, &allowed);
    let store = gc_effects::ArtifactStore::open(&store_dir).unwrap();
    let retained = store.put_bytes(b"retained").unwrap();
    let inventory = || {
        let mut files: Vec<_> = std::fs::read_dir(&store_dir)
            .unwrap()
            .map(|entry| entry.unwrap().file_name())
            .collect();
        files.sort();
        files
    };
    let before = inventory();
    let payload = parse_term(&format!(
        r#"{{:remote "{remote}" :roots ["{expected}"] :depth 0}}"#
    ))
    .unwrap();
    let (forms, hash) = mk_prog("core/sync::pull", &payload);
    let mut ctx = EvalCtx::new();
    let mut env = build_prelude(&mut ctx).env;
    let prog = eval_module(&mut ctx, &mut env, &forms).unwrap();
    let result = run(&mut ctx, &caps, prog, hash, "gc_effects-test".to_string()).unwrap();
    assert!(
        is_sealed_error(&ctx, &result.value, "core/sync/hash-mismatch"),
        "{}",
        result.value.debug_repr()
    );
    assert_eq!(
        inventory(),
        before,
        "corrupt bytes installed before rejection"
    );
    assert_eq!(store.get_bytes(&retained).unwrap(), b"retained");
    let log = EffectLog::from_term(&result.log.to_term()).unwrap();
    let mut replay_ctx = EvalCtx::new();
    let mut replay_env = build_prelude(&mut replay_ctx).env;
    let replay_prog = eval_module(&mut replay_ctx, &mut replay_env, &forms).unwrap();
    let replay_value = replay(&mut replay_ctx, replay_prog, &log).unwrap();
    assert_eq!(value_hash(&result.value), value_hash(&replay_value));
}
