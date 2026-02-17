use gc_coreform::{Term, TermOrdKey, hash_module, parse_module, parse_term, print_term};
use gc_effects::{ArtifactStore, CapsPolicy, run};
use gc_kernel::{EvalCtx, Value, eval_module};
use gc_prelude::build_prelude;

fn eval_prog(forms: &[Term]) -> (EvalCtx, Value) {
    let mut ctx = EvalCtx::new();
    let prelude = build_prelude(&mut ctx);
    let mut env = prelude.env;
    let prog = eval_module(&mut ctx, &mut env, forms).expect("eval module");
    (ctx, prog)
}

fn is_sealed_error_code(ctx: &EvalCtx, value: &Value, code: &str) -> bool {
    let Some(proto) = ctx.protocol else {
        return false;
    };
    let Value::Sealed { token, payload } = value else {
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

#[test]
fn refs_set_then_get_roundtrips_through_refs_db_and_policy_gate() {
    let td = tempfile::tempdir().unwrap();
    let caps_path = td.path().join("caps.toml");
    std::fs::write(
        &caps_path,
        r#"
allow = ["core/refs::set", "core/refs::get"]

[store]
dir = "./.genesis/store"

[refs]
path = "./.genesis/refs.gc"
"#,
    )
    .unwrap();
    let pol = CapsPolicy::load(&caps_path).unwrap();

    // Seed the artifact store with a policy artifact, evidence, and commit.
    let store_dir = td.path().join(".genesis").join("store");
    let store = ArtifactStore::open(&store_dir).unwrap();

    let policy_term = parse_term(
        r#"
        {
          :type :vcs/policy
          :v 1
          :refs {:frozen-prefixes ["refs/frozen/"]}
          :classes {
            :dev {:patterns ["refs/**/heads/*"] :exclude ["refs/**/heads/main"]
                  :required-obligations ["core/obligation::unit-tests"]}
          }
        }
    "#,
    )
    .unwrap();
    let policy_h = store
        .put_bytes(print_term(&policy_term).as_bytes())
        .unwrap();

    let evidence_term =
        parse_term(r#"{:type :vcs/evidence :v 1 :kind :unit-tests :data nil}"#).unwrap();
    let evidence_h = store
        .put_bytes(print_term(&evidence_term).as_bytes())
        .unwrap();

    let commit_term = parse_term(&format!(
        r#"
        {{
          :type :vcs/commit
          :v 1
          :parents []
          :base nil
          :patch "{z}"
          :result "{z}"
          :obligations ["core/obligation::unit-tests"]
          :evidence ["{evidence_h}"]
          :attestations []
          :message "t"
        }}
        "#,
        z = "0".repeat(64)
    ))
    .unwrap();
    let commit_h = store
        .put_bytes(print_term(&commit_term).as_bytes())
        .unwrap();

    // set
    let set_src = format!(
        r#"
        (def prog
          (core/effect::perform
            'core/refs::set
            {{:name "refs/heads/dev" :hash "{commit_h}" :policy "{policy_h}"}}
            (fn (r) (core/effect::pure r))))
        prog
        "#
    );
    let set_forms = parse_module(&set_src).unwrap();
    let set_hash = hash_module(&set_forms);
    let (mut ctx1, prog1) = eval_prog(&set_forms);
    let r1 = run(
        &mut ctx1,
        &pol,
        prog1,
        set_hash,
        "gc_effects-test".to_string(),
    )
    .unwrap();
    let Value::Data(Term::Map(m)) = r1.value else {
        panic!("expected map result");
    };
    assert!(matches!(
        m.get(&TermOrdKey(Term::symbol(":ok"))),
        Some(Term::Bool(true))
    ));
    assert!(matches!(
        m.get(&TermOrdKey(Term::symbol(":hash"))),
        Some(Term::Str(s)) if s == &commit_h
    ));

    // get should observe the updated ref.
    let get_src = r#"
        (def prog
          (core/effect::perform
            'core/refs::get
            {:name "refs/heads/dev"}
            (fn (r) (core/effect::pure r))))
        prog
    "#;
    let get_forms = parse_module(get_src).unwrap();
    let get_hash = hash_module(&get_forms);
    let (mut ctx2, prog2) = eval_prog(&get_forms);
    let r2 = run(
        &mut ctx2,
        &pol,
        prog2,
        get_hash,
        "gc_effects-test".to_string(),
    )
    .unwrap();
    let Value::Data(Term::Map(m2)) = r2.value else {
        panic!("expected map result");
    };
    assert!(matches!(
        m2.get(&TermOrdKey(Term::symbol(":hash"))),
        Some(Term::Str(s)) if s == &commit_h
    ));
}

#[test]
fn refs_delete_uses_policy_gate_and_cas() {
    let td = tempfile::tempdir().unwrap();
    let caps_path = td.path().join("caps.toml");
    std::fs::write(
        &caps_path,
        r#"
allow = ["core/refs::delete"]

[store]
dir = "./.genesis/store"

[refs]
path = "./.genesis/refs.gc"
"#,
    )
    .unwrap();
    let pol = CapsPolicy::load(&caps_path).unwrap();

    let store_dir = td.path().join(".genesis").join("store");
    let store = ArtifactStore::open(&store_dir).unwrap();
    let refs_path = td.path().join(".genesis").join("refs.gc");
    let refs = gc_effects::RefsDb::open(&refs_path).unwrap();

    let policy_term = parse_term(
        r#"
        {
          :type :vcs/policy
          :v 1
          :refs {:frozen-prefixes ["refs/frozen/"]}
          :classes {
            :dev {:patterns ["refs/**/heads/*"] :exclude ["refs/**/heads/main"] :required-obligations []}
          }
        }
    "#,
    )
    .unwrap();
    let policy_h = store
        .put_bytes(print_term(&policy_term).as_bytes())
        .unwrap();

    // Seed local ref directly; delete operation should be policy/CAS gated.
    let existing = "a".repeat(64);
    refs.set("refs/heads/dev", Some(&existing), None).unwrap();

    // CAS mismatch must fail with refs/conflict.
    let src_cas_miss = format!(
        r#"
        (def prog
          (core/effect::perform
            'core/refs::delete
            {{:name "refs/heads/dev" :policy "{policy_h}" :expected-old "bbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb"}}
            (fn (r) (core/effect::pure r))))
        prog
        "#
    );
    let forms_cas_miss = parse_module(&src_cas_miss).unwrap();
    let hash_cas_miss = hash_module(&forms_cas_miss);
    let (mut ctx_cas_miss, prog_cas_miss) = eval_prog(&forms_cas_miss);
    let r_cas_miss = run(
        &mut ctx_cas_miss,
        &pol,
        prog_cas_miss,
        hash_cas_miss,
        "gc_effects-test".to_string(),
    )
    .unwrap();
    assert!(is_sealed_error_code(
        &ctx_cas_miss,
        &r_cas_miss.value,
        "core/refs/conflict"
    ));
    assert_eq!(refs.get("refs/heads/dev").unwrap(), Some(existing.clone()));

    // Frozen ref must fail at policy gate.
    let src_frozen = format!(
        r#"
        (def prog
          (core/effect::perform
            'core/refs::delete
            {{:name "refs/frozen/main" :policy "{policy_h}"}}
            (fn (r) (core/effect::pure r))))
        prog
        "#
    );
    let forms_frozen = parse_module(&src_frozen).unwrap();
    let hash_frozen = hash_module(&forms_frozen);
    let (mut ctx_frozen, prog_frozen) = eval_prog(&forms_frozen);
    let r_frozen = run(
        &mut ctx_frozen,
        &pol,
        prog_frozen,
        hash_frozen,
        "gc_effects-test".to_string(),
    )
    .unwrap();
    assert!(is_sealed_error_code(
        &ctx_frozen,
        &r_frozen.value,
        "core/refs/frozen"
    ));

    // No class match must fail.
    let src_no_class = format!(
        r#"
        (def prog
          (core/effect::perform
            'core/refs::delete
            {{:name "refs/other/x" :policy "{policy_h}"}}
            (fn (r) (core/effect::pure r))))
        prog
        "#
    );
    let forms_no_class = parse_module(&src_no_class).unwrap();
    let hash_no_class = hash_module(&forms_no_class);
    let (mut ctx_no_class, prog_no_class) = eval_prog(&forms_no_class);
    let r_no_class = run(
        &mut ctx_no_class,
        &pol,
        prog_no_class,
        hash_no_class,
        "gc_effects-test".to_string(),
    )
    .unwrap();
    assert!(is_sealed_error_code(
        &ctx_no_class,
        &r_no_class.value,
        "core/refs/no-class"
    ));

    // Successful delete with matching expected-old.
    let src_ok = format!(
        r#"
        (def prog
          (core/effect::perform
            'core/refs::delete
            {{:name "refs/heads/dev" :policy "{policy_h}" :expected-old "{existing}"}}
            (fn (r) (core/effect::pure r))))
        prog
        "#
    );
    let forms_ok = parse_module(&src_ok).unwrap();
    let hash_ok = hash_module(&forms_ok);
    let (mut ctx_ok, prog_ok) = eval_prog(&forms_ok);
    let r_ok = run(
        &mut ctx_ok,
        &pol,
        prog_ok,
        hash_ok,
        "gc_effects-test".to_string(),
    )
    .unwrap();
    let Value::Data(Term::Map(m_ok)) = r_ok.value else {
        panic!("expected map result");
    };
    assert!(matches!(
        m_ok.get(&TermOrdKey(Term::symbol(":ok"))),
        Some(Term::Bool(true))
    ));
    assert_eq!(refs.get("refs/heads/dev").unwrap(), None);
}
