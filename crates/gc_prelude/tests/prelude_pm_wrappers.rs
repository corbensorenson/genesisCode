use gc_coreform::{Term, TermOrdKey, canonicalize_module, parse_module};
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

fn payload_flag(req: &EffectRequest, key: &str) -> Option<bool> {
    let Term::Map(mm) = &req.payload else {
        return None;
    };
    let Term::Bool(b) = mm.get(&TermOrdKey(Term::symbol(key)))? else {
        return None;
    };
    Some(*b)
}

#[test]
fn pm_wrappers_route_to_pkg_caps_and_enforce_strict_defaults() {
    let src = r#"
      {
        :lock (core/pm::lock "genesis.lock")
        :update (core/pm::update "genesis.lock")
        :install ((core/pm::install "genesis.lock") true)
        :verify (core/pm::verify "genesis.lock")
        :publish ((((((core/pm::publish "origin") "refs/heads/main") "policy-h") nil) 3) nil)
      }
    "#;
    let forms = canonicalize_module(parse_module(src).expect("parse")).expect("canonicalize");

    let mut ctx = EvalCtx::new();
    let prelude = build_prelude(&mut ctx);
    let mut env = prelude.env;
    let v = eval_module(&mut ctx, &mut env, &forms).expect("eval");

    let Value::Map(m) = v else {
        panic!("expected map");
    };

    let lock = get_req(
        m.get(&TermOrdKey(Term::symbol(":lock")))
            .expect("lock")
            .clone(),
    );
    assert_eq!(lock.op, "core/pkg::lock");
    assert_eq!(payload_flag(&lock, ":strict"), Some(true));

    let update = get_req(
        m.get(&TermOrdKey(Term::symbol(":update")))
            .expect("update")
            .clone(),
    );
    assert_eq!(update.op, "core/pkg::update");
    assert_eq!(payload_flag(&update, ":strict"), Some(true));

    let install = get_req(
        m.get(&TermOrdKey(Term::symbol(":install")))
            .expect("install")
            .clone(),
    );
    assert_eq!(install.op, "core/pkg::install");
    assert_eq!(payload_flag(&install, ":strict"), Some(true));
    assert_eq!(payload_flag(&install, ":frozen"), Some(true));

    let verify = get_req(
        m.get(&TermOrdKey(Term::symbol(":verify")))
            .expect("verify")
            .clone(),
    );
    assert_eq!(verify.op, "core/pkg::verify");
    assert_eq!(payload_flag(&verify, ":strict"), Some(true));

    let publish = get_req(
        m.get(&TermOrdKey(Term::symbol(":publish")))
            .expect("publish")
            .clone(),
    );
    assert_eq!(publish.op, "core/pkg::publish");
    let Term::Map(pm) = publish.payload else {
        panic!("expected publish payload map");
    };
    assert_eq!(
        pm.get(&TermOrdKey(Term::symbol(":policy"))),
        Some(&Term::Str("policy-h".to_string()))
    );
}

#[test]
fn pm_publish_requires_policy_before_emitting_effect() {
    let src = r#"
      {
        :publish-missing-policy ((((((core/pm::publish "origin") "refs/heads/main") nil) nil) 0) nil)
      }
    "#;
    let forms = canonicalize_module(parse_module(src).expect("parse")).expect("canonicalize");

    let mut ctx = EvalCtx::new();
    let prelude = build_prelude(&mut ctx);
    let mut env = prelude.env;
    let v = eval_module(&mut ctx, &mut env, &forms).expect("eval");

    let Value::Map(m) = v else {
        panic!("expected map");
    };
    let val = m
        .get(&TermOrdKey(Term::symbol(":publish-missing-policy")))
        .expect("publish-missing-policy")
        .clone();

    let Value::EffectProgram(p) = val else {
        panic!("expected effect program");
    };
    match p.as_ref() {
        EffectProgram::Pure(inner) => {
            assert!(matches!(inner.as_ref(), Value::Sealed { .. }));
        }
        EffectProgram::Perform { .. } => {
            panic!("expected pure error path when policy is nil");
        }
    }
}
