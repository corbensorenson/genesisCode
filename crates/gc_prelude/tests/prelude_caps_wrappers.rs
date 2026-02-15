use gc_coreform::{canonicalize_module, parse_module};
use gc_kernel::{EffectProgram, EffectRequest, EvalCtx, Value, eval_module};
use gc_prelude::build_prelude;

fn get_req(v: Value) -> EffectRequest {
    let Value::EffectProgram(p) = v else {
        panic!("expected effect program, got {}", v.debug_repr());
    };
    let EffectProgram::Perform { request } = p.as_ref() else {
        panic!("expected perform");
    };
    let Value::Sealed { payload, .. } = request.as_ref() else {
        panic!("expected sealed request");
    };
    let Value::EffectRequest(req) = payload.as_ref() else {
        panic!("expected effect request payload");
    };
    req.clone()
}

#[test]
fn prelude_capability_wrappers_construct_expected_requests() {
    let src = r#"
      {
        :store_get (core/store::get "abc")
        :refs_set (core/refs::set "refs/heads/main" "h" "p")
        :refs_set_cas (core/refs::set-cas "refs/heads/main" "h" "p" nil)
        :vcs_log (core/vcs::log "refs/heads/main" 10)
        :pkg_init (core/pkg::init "genesis.lock" "ws" nil nil)
        :gc_plan (core/gc::plan "genesis.lock" ".genesis/pins.toml" 200 true true)
      }
    "#;
    let forms = canonicalize_module(parse_module(src).unwrap()).unwrap();

    let mut ctx = EvalCtx::new();
    let prelude = build_prelude(&mut ctx);
    let mut env = prelude.env;
    let v = eval_module(&mut ctx, &mut env, &forms).unwrap();

    let Value::Map(m) = v else {
        panic!("expected map, got {}", v.debug_repr());
    };

    let store_get = m
        .get(&gc_coreform::TermOrdKey(gc_coreform::Term::symbol(
            ":store_get",
        )))
        .unwrap()
        .clone();
    let req = get_req(store_get);
    assert_eq!(req.op, "core/store::get");
    assert!(matches!(
        req.payload,
        gc_coreform::Term::Map(ref mm)
            if mm.get(&gc_coreform::TermOrdKey(gc_coreform::Term::symbol(":hash")))
                == Some(&gc_coreform::Term::Str("abc".to_string()))
    ));

    let refs_set = m
        .get(&gc_coreform::TermOrdKey(gc_coreform::Term::symbol(
            ":refs_set",
        )))
        .unwrap()
        .clone();
    let req = get_req(refs_set);
    assert_eq!(req.op, "core/refs::set");
    let gc_coreform::Term::Map(mm) = req.payload else {
        panic!("expected map payload");
    };
    assert!(
        !mm.contains_key(&gc_coreform::TermOrdKey(gc_coreform::Term::symbol(
            ":expected-old"
        )))
    );

    let refs_set_cas = m
        .get(&gc_coreform::TermOrdKey(gc_coreform::Term::symbol(
            ":refs_set_cas",
        )))
        .unwrap()
        .clone();
    let req = get_req(refs_set_cas);
    assert_eq!(req.op, "core/refs::set");
    let gc_coreform::Term::Map(mm) = req.payload else {
        panic!("expected map payload");
    };
    assert!(
        mm.contains_key(&gc_coreform::TermOrdKey(gc_coreform::Term::symbol(
            ":expected-old"
        )))
    );

    let vcs_log = m
        .get(&gc_coreform::TermOrdKey(gc_coreform::Term::symbol(
            ":vcs_log",
        )))
        .unwrap()
        .clone();
    let req = get_req(vcs_log);
    assert_eq!(req.op, "core/vcs::log");

    let pkg_init = m
        .get(&gc_coreform::TermOrdKey(gc_coreform::Term::symbol(
            ":pkg_init",
        )))
        .unwrap()
        .clone();
    let req = get_req(pkg_init);
    assert_eq!(req.op, "core/pkg::init");

    let gc_plan = m
        .get(&gc_coreform::TermOrdKey(gc_coreform::Term::symbol(
            ":gc_plan",
        )))
        .unwrap()
        .clone();
    let req = get_req(gc_plan);
    assert_eq!(req.op, "core/gc::plan");
}
