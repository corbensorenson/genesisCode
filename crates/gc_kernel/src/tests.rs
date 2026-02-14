use gc_coreform::{Term, parse_module};
use num_bigint::BigInt;

use crate::{Env, EvalCtx, Value, eval_module};

#[test]
fn seal_unseal_roundtrip() {
    let src = r#"
      (def s (seal))
      (unseal (seal 42 s) s)
    "#;
    let forms = parse_module(src).unwrap();
    let mut ctx = EvalCtx::new();
    let mut env = Env::empty();
    let v = eval_module(&mut ctx, &mut env, &forms).unwrap();
    assert_eq!(v.as_data(), Some(&Term::Int(BigInt::from(42))));
}

#[test]
fn unseal_mismatch_is_nil() {
    let src = r#"
      (def s (seal))
      (def t (seal))
      (unseal (seal 42 s) t)
    "#;
    let forms = parse_module(src).unwrap();
    let mut ctx = EvalCtx::new();
    let mut env = Env::empty();
    let v = eval_module(&mut ctx, &mut env, &forms).unwrap();
    assert_eq!(v.as_data(), Some(&Term::Nil));
}

#[test]
fn prim_type_error_is_sealed_error_with_protocol() {
    let src = r#"(prim int/add 1 "x")"#;
    let forms = parse_module(src).unwrap();
    let mut ctx = EvalCtx::new();
    let p = ctx.protocol.expect("EvalCtx reserves protocol tokens");
    let mut env = Env::empty();
    let v = eval_module(&mut ctx, &mut env, &forms).unwrap();
    match v {
        Value::Sealed { token, payload } => {
            assert_eq!(token, p.error);
            assert!(matches!(*payload, Value::Data(Term::Map(_))));
        }
        _ => panic!("expected sealed error"),
    }
}

#[test]
fn unseal_with_non_token_is_sealed_type_error() {
    let src = r#"(unseal nil 1)"#;
    let forms = parse_module(src).unwrap();
    let mut ctx = EvalCtx::new();
    let p = ctx.protocol.expect("EvalCtx reserves protocol tokens");
    let mut env = Env::empty();
    let v = eval_module(&mut ctx, &mut env, &forms).unwrap();
    match v {
        Value::Sealed { token, payload } => {
            assert_eq!(token, p.error);
            let Value::Data(Term::Map(m)) = payload.as_ref() else {
                panic!("expected error payload map datum");
            };
            assert!(matches!(
                m.get(&gc_coreform::TermOrdKey(Term::symbol(":error/code"))),
                Some(Term::Str(s)) if s == "core/type-error"
            ));
        }
        _ => panic!("expected sealed error, got {}", v.debug_repr()),
    }
}

#[test]
fn seal_with_non_token_is_sealed_type_error() {
    let src = r#"(seal 1 2)"#;
    let forms = parse_module(src).unwrap();
    let mut ctx = EvalCtx::new();
    let p = ctx.protocol.expect("EvalCtx reserves protocol tokens");
    let mut env = Env::empty();
    let v = eval_module(&mut ctx, &mut env, &forms).unwrap();
    match v {
        Value::Sealed { token, payload } => {
            assert_eq!(token, p.error);
            let Value::Data(Term::Map(m)) = payload.as_ref() else {
                panic!("expected error payload map datum");
            };
            assert!(matches!(
                m.get(&gc_coreform::TermOrdKey(Term::symbol(":error/code"))),
                Some(Term::Str(s)) if s == "core/type-error"
            ));
        }
        _ => panic!("expected sealed error, got {}", v.debug_repr()),
    }
}

#[test]
fn application_sugar_left_associates() {
    let src = r#"
      (((fn (x y) (prim int/add x y)) 1) 2)
    "#;
    // Also accept the sugar form.
    let forms2 = parse_module(r#"((fn (x y) (prim int/add x y)) 1 2)"#).unwrap();

    for forms in [parse_module(src).unwrap(), forms2] {
        let mut ctx = EvalCtx::new();
        let mut env = Env::empty();
        let v = eval_module(&mut ctx, &mut env, &forms).unwrap();
        assert_eq!(v.as_data(), Some(&Term::Int(BigInt::from(3))));
    }
}
