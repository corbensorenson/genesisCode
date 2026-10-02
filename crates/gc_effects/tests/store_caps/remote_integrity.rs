use super::*;

#[test]
fn remote_store_get_classifies_corruption_before_caching_and_replays() {
    let td = tempfile::tempdir().unwrap();
    let remote_dir = td.path().join("remote");
    let remote = format!("file://{}/", remote_dir.display());
    let object = gc_coreform::print_term(&parse_term("{:x 1}").unwrap());
    let expected = blake3::hash(object.as_bytes()).to_hex().to_string();
    std::fs::create_dir_all(remote_dir.join("v1/store")).unwrap();
    for mode in ["missing", "corrupt", "honest"] {
        let object_path = remote_dir.join("v1/store").join(&expected);
        match mode {
            "missing" => {}
            "corrupt" => std::fs::write(&object_path, b"corrupt").unwrap(),
            "honest" => std::fs::write(&object_path, &object).unwrap(),
            _ => unreachable!(),
        }
        let local = td.path().join(mode);
        let caps_path = td.path().join(format!("{mode}.toml"));
        std::fs::write(
            &caps_path,
            format!(
                r#"
allow = ["core/store::get"]
[store]
dir = "{}"
remote = "{remote}"
remote_allow = ["{remote}"]
"#,
                local.display()
            ),
        )
        .unwrap();
        let policy = load_policy(&caps_path);
        let forms = parse_module(&format!(r#"(def prog (core/effect::perform 'core/store::get {{:hash "{expected}"}} (fn (r) (core/effect::pure r)))) prog"#)).unwrap();
        let (mut ctx, prog) = eval_prog(&forms);
        let result = run(
            &mut ctx,
            &policy,
            prog,
            hash_module(&forms),
            "gc_effects-test".to_string(),
        )
        .unwrap();
        if mode == "honest" {
            assert!(
                sealed_error_code(&result.value).is_none(),
                "{}",
                result.value.debug_repr()
            );
            assert_eq!(
                std::fs::read(local.join(&expected)).unwrap(),
                object.as_bytes()
            );
        } else {
            assert_eq!(
                sealed_error_code(&result.value).as_deref(),
                Some(if mode == "missing" {
                    "core/store/not-found"
                } else {
                    "core/store/hash-mismatch"
                })
            );
            assert_eq!(std::fs::read_dir(&local).unwrap().count(), 0);
            if mode == "corrupt" {
                assert_eq!(
                    sealed_error_message(&result.value).as_deref(),
                    Some("remote bytes hash mismatch")
                );
            }
        }
        let log = EffectLog::from_term(&result.log.to_term()).unwrap();
        let (mut replay_ctx, replay_prog) = eval_prog(&forms);
        let replayed = replay(&mut replay_ctx, replay_prog, &log).unwrap();
        assert_eq!(value_hash(&result.value), value_hash(&replayed));
    }
}
