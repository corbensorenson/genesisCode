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
