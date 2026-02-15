use gc_coreform::{Term, TermOrdKey, hash_module, parse_module, parse_term};
use gc_effects::{CapsPolicy, EffectLog, replay, run};
use gc_kernel::{EvalCtx, Value, eval_module, value_hash};
use gc_prelude::build_prelude;

fn eval_prog(forms: &[Term]) -> (EvalCtx, Value) {
    let mut ctx = EvalCtx::new();
    let prelude = build_prelude(&mut ctx);
    let mut env = prelude.env;
    let prog = eval_module(&mut ctx, &mut env, forms).expect("eval module");
    (ctx, prog)
}

fn extract_str_field(map: &std::collections::BTreeMap<TermOrdKey, Term>, k: &str) -> String {
    let kk = TermOrdKey(Term::symbol(k));
    match map.get(&kk) {
        Some(Term::Str(s)) => s.clone(),
        other => panic!("expected string field {k}, got {other:?}"),
    }
}

#[test]
fn store_put_has_get_roundtrip_and_replay_does_not_need_store() {
    let td = tempfile::tempdir().unwrap();

    let caps_path = td.path().join("caps.toml");
    std::fs::write(
        &caps_path,
        r#"
allow = ["core/store::put", "core/store::has", "core/store::get"]

[store]
dir = "./.genesis/store"
"#,
    )
    .unwrap();
    let pol = CapsPolicy::load(&caps_path).unwrap();

    let artifact: Term = parse_term(r#"{:x 1 :y "hi"}"#).unwrap();

    // ---- put
    let put_src = r#"
      (def prog
        (core/effect::perform
          'core/store::put
          {:artifact (quote {:x 1 :y "hi"})}
          (fn (r) (core/effect::pure r))))
      prog
    "#;
    let put_forms = parse_module(put_src).unwrap();
    let put_hash = hash_module(&put_forms);
    let (mut ctx_put, prog_put) = eval_prog(&put_forms);
    let r_put = run(
        &mut ctx_put,
        &pol,
        prog_put,
        put_hash,
        "gc_effects-test".to_string(),
    )
    .unwrap();

    let Value::Data(Term::Map(put_m)) = r_put.value else {
        panic!("expected put result map, got {}", r_put.value.debug_repr());
    };
    let h = extract_str_field(&put_m, ":hash");

    let stored_path = td.path().join(".genesis").join("store").join(&h);
    assert!(stored_path.exists(), "stored artifact path missing");

    // ---- has (present true)
    let has_src = format!(
        r#"
        (def prog
          (core/effect::perform
            'core/store::has
            {{:hash "{h}"}}
            (fn (r) (core/effect::pure r))))
        prog
      "#
    );
    let has_forms = parse_module(&has_src).unwrap();
    let has_hash = hash_module(&has_forms);
    let (mut ctx_has, prog_has) = eval_prog(&has_forms);
    let r_has = run(
        &mut ctx_has,
        &pol,
        prog_has,
        has_hash,
        "gc_effects-test".to_string(),
    )
    .unwrap();

    let Value::Data(Term::Map(has_m)) = r_has.value else {
        panic!("expected has result map, got {}", r_has.value.debug_repr());
    };
    match has_m.get(&TermOrdKey(Term::symbol(":present"))) {
        Some(Term::Bool(true)) => {}
        other => panic!("expected :present true, got {other:?}"),
    }

    // ---- has (present false for missing hash)
    let missing = "0".repeat(64);
    let has_missing_src = format!(
        r#"
        (def prog
          (core/effect::perform
            'core/store::has
            {{:hash "{missing}"}}
            (fn (r) (core/effect::pure r))))
        prog
      "#
    );
    let has_missing_forms = parse_module(&has_missing_src).unwrap();
    let has_missing_hash = hash_module(&has_missing_forms);
    let (mut ctx_has2, prog_has2) = eval_prog(&has_missing_forms);
    let r_has2 = run(
        &mut ctx_has2,
        &pol,
        prog_has2,
        has_missing_hash,
        "gc_effects-test".to_string(),
    )
    .unwrap();
    let Value::Data(Term::Map(has2_m)) = r_has2.value else {
        panic!(
            "expected has(missing) result map, got {}",
            r_has2.value.debug_repr()
        );
    };
    match has2_m.get(&TermOrdKey(Term::symbol(":present"))) {
        Some(Term::Bool(false)) => {}
        other => panic!("expected :present false, got {other:?}"),
    }

    // ---- get (and replay without store present)
    let get_src = format!(
        r#"
        (def prog
          (core/effect::perform
            'core/store::get
            {{:hash "{h}"}}
            (fn (r) (core/effect::pure r))))
        prog
      "#
    );
    let get_forms = parse_module(&get_src).unwrap();
    let get_hash = hash_module(&get_forms);
    let (mut ctx_get, prog_get) = eval_prog(&get_forms);
    let r_get = run(
        &mut ctx_get,
        &pol,
        prog_get,
        get_hash,
        "gc_effects-test".to_string(),
    )
    .unwrap();

    let Value::Data(Term::Map(get_m)) = &r_get.value else {
        panic!("expected get result map, got {}", r_get.value.debug_repr());
    };
    match get_m.get(&TermOrdKey(Term::symbol(":artifact"))) {
        Some(t) => assert_eq!(t, &artifact),
        None => panic!("expected :artifact field"),
    }

    // Replay should be fully determined by the log (responses are inlined here), so deleting the
    // store directory should not affect replay.
    std::fs::remove_dir_all(td.path().join(".genesis").join("store")).unwrap();

    let v1_h = value_hash(&r_get.value);
    let log_term = r_get.log.to_term();
    let log2 = EffectLog::from_term(&log_term).expect("parse log");

    let (mut ctx_rep, prog_rep) = eval_prog(&get_forms);
    let v2 = replay(&mut ctx_rep, prog_rep, &log2).expect("replay");
    let v2_h = value_hash(&v2);
    assert_eq!(v1_h, v2_h);
}

#[test]
fn store_get_missing_is_sealed_error() {
    let td = tempfile::tempdir().unwrap();

    let caps_path = td.path().join("caps.toml");
    std::fs::write(
        &caps_path,
        r#"
allow = ["core/store::get"]

[store]
dir = "./.genesis/store"
"#,
    )
    .unwrap();
    let pol = CapsPolicy::load(&caps_path).unwrap();

    let h = "0".repeat(64);
    let get_src = format!(
        r#"
        (def prog
          (core/effect::perform
            'core/store::get
            {{:hash "{h}"}}
            (fn (r) (core/effect::pure r))))
        prog
      "#
    );
    let forms = parse_module(&get_src).unwrap();
    let mh = hash_module(&forms);
    let (mut ctx, prog) = eval_prog(&forms);
    let r = run(&mut ctx, &pol, prog, mh, "gc_effects-test".to_string()).unwrap();

    let proto = ctx.protocol.unwrap();
    match r.value {
        Value::Sealed { token, .. } => assert_eq!(token, proto.error),
        other => panic!("expected sealed error, got {}", other.debug_repr()),
    }
}
