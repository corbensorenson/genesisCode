use gc_coreform::{Term, parse_module};
use num_bigint::BigInt;

use crate::{Env, EvalCtx, KernelErrorKind, MemLimits, Value, eval_module};

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

#[test]
fn memory_limit_on_string_len_is_a_kernel_error() {
    let forms = parse_module(r#""ab""#).unwrap();
    let mut ctx = EvalCtx::new();
    ctx.set_mem_limits(MemLimits {
        max_string_len: Some(1),
        ..MemLimits::default()
    });
    let mut env = Env::empty();
    let e = eval_module(&mut ctx, &mut env, &forms).unwrap_err();
    assert!(matches!(e.kind, KernelErrorKind::MemoryLimit), "{e}");
    assert!(e.msg.contains("string-len"), "msg={}", e.msg);
}

#[test]
fn memory_limit_on_pair_cells_is_a_kernel_error() {
    let forms = parse_module(r#"(prim pair/cons 1 nil)"#).unwrap();
    let mut ctx = EvalCtx::new();
    ctx.set_mem_limits(MemLimits {
        max_pair_cells: Some(0),
        ..MemLimits::default()
    });
    let mut env = Env::empty();
    let e = eval_module(&mut ctx, &mut env, &forms).unwrap_err();
    assert!(matches!(e.kind, KernelErrorKind::MemoryLimit), "{e}");
    assert!(e.msg.contains("pair-cells"), "msg={}", e.msg);
}

#[test]
fn vec_len_and_map_len_work() {
    let forms = parse_module(
        r#"
      {
        :vl (prim vec/len [1 2 3])
        :ml (prim map/len {:a 1 :b 2})
      }
    "#,
    )
    .unwrap();
    let mut ctx = EvalCtx::new();
    let mut env = Env::empty();
    let v = eval_module(&mut ctx, &mut env, &forms).unwrap();
    match v {
        Value::Map(m) => {
            let vl = m
                .get(&gc_coreform::TermOrdKey(Term::symbol(":vl")))
                .unwrap();
            assert!(matches!(vl, Value::Data(Term::Int(i)) if i == &3.into()));
            let ml = m
                .get(&gc_coreform::TermOrdKey(Term::symbol(":ml")))
                .unwrap();
            assert!(matches!(ml, Value::Data(Term::Int(i)) if i == &2.into()));
        }
        Value::Data(Term::Map(m)) => {
            assert!(matches!(
                m.get(&gc_coreform::TermOrdKey(Term::symbol(":vl"))),
                Some(Term::Int(i)) if i == &3.into()
            ));
            assert!(matches!(
                m.get(&gc_coreform::TermOrdKey(Term::symbol(":ml"))),
                Some(Term::Int(i)) if i == &2.into()
            ));
        }
        _ => panic!("expected map, got {}", v.debug_repr()),
    }
}

#[test]
fn bytes_get_slice_utf8_hex_and_blake3_work() {
    let forms = parse_module(
        r#"
      {
        :g (prim bytes/get b"\x01\x02\xff" 2)
        :s (prim bytes/slice b"abcd" 1 2)
        :u (prim bytes/to-str-utf8 b"hi")
        :b (prim str/to-bytes-utf8 "hi")
        :hx (prim bytes/to-hex b"\x00\xff")
        :bh (prim bytes/from-hex "00ff")
        :h (prim crypto/blake3 b"")
      }
    "#,
    )
    .unwrap();
    let mut ctx = EvalCtx::new();
    let mut env = Env::empty();
    let v = eval_module(&mut ctx, &mut env, &forms).unwrap();
    let want = blake3::hash(&[]).as_bytes().to_vec();

    match v {
        Value::Map(m) => {
            let g = m.get(&gc_coreform::TermOrdKey(Term::symbol(":g"))).unwrap();
            assert!(matches!(g, Value::Data(Term::Int(i)) if i == &255.into()));
            let s = m.get(&gc_coreform::TermOrdKey(Term::symbol(":s"))).unwrap();
            assert!(matches!(s, Value::Data(Term::Bytes(bs)) if bs == b"bc"));
            let u = m.get(&gc_coreform::TermOrdKey(Term::symbol(":u"))).unwrap();
            assert!(matches!(u, Value::Data(Term::Str(s)) if s == "hi"));
            let b = m.get(&gc_coreform::TermOrdKey(Term::symbol(":b"))).unwrap();
            assert!(matches!(b, Value::Data(Term::Bytes(bs)) if bs == b"hi"));
            let hx = m
                .get(&gc_coreform::TermOrdKey(Term::symbol(":hx")))
                .unwrap();
            assert!(matches!(hx, Value::Data(Term::Str(s)) if s == "00ff"));
            let bh = m
                .get(&gc_coreform::TermOrdKey(Term::symbol(":bh")))
                .unwrap();
            assert!(matches!(bh, Value::Data(Term::Bytes(bs)) if bs == b"\x00\xff"));
            let h = m.get(&gc_coreform::TermOrdKey(Term::symbol(":h"))).unwrap();
            assert!(matches!(h, Value::Data(Term::Bytes(bs)) if bs == &want));
        }
        Value::Data(Term::Map(m)) => {
            assert!(matches!(
                m.get(&gc_coreform::TermOrdKey(Term::symbol(":g"))),
                Some(Term::Int(i)) if i == &255.into()
            ));
            assert!(matches!(
                m.get(&gc_coreform::TermOrdKey(Term::symbol(":s"))),
                Some(Term::Bytes(bs)) if bs == b"bc"
            ));
            assert!(matches!(
                m.get(&gc_coreform::TermOrdKey(Term::symbol(":u"))),
                Some(Term::Str(s)) if s == "hi"
            ));
            assert!(matches!(
                m.get(&gc_coreform::TermOrdKey(Term::symbol(":b"))),
                Some(Term::Bytes(bs)) if bs == b"hi"
            ));
            assert!(matches!(
                m.get(&gc_coreform::TermOrdKey(Term::symbol(":hx"))),
                Some(Term::Str(s)) if s == "00ff"
            ));
            assert!(matches!(
                m.get(&gc_coreform::TermOrdKey(Term::symbol(":bh"))),
                Some(Term::Bytes(bs)) if bs == b"\x00\xff"
            ));
            assert!(matches!(
                m.get(&gc_coreform::TermOrdKey(Term::symbol(":h"))),
                Some(Term::Bytes(bs)) if bs == &want
            ));
        }
        _ => panic!("expected map, got {}", v.debug_repr()),
    }
}

#[test]
fn bytes_to_str_utf8_invalid_is_sealed_type_error() {
    let forms = parse_module(r#"(prim bytes/to-str-utf8 b"\xff")"#).unwrap();
    let mut ctx = EvalCtx::new();
    let p = ctx.protocol.expect("EvalCtx reserves protocol tokens");
    let mut env = Env::empty();
    let v = eval_module(&mut ctx, &mut env, &forms).unwrap();
    match v {
        Value::Sealed { token, .. } => assert_eq!(token, p.error),
        _ => panic!("expected sealed error, got {}", v.debug_repr()),
    }
}

#[test]
fn bytes_from_hex_invalid_is_sealed_type_error() {
    let forms = parse_module(r#"(prim bytes/from-hex "0g")"#).unwrap();
    let mut ctx = EvalCtx::new();
    let p = ctx.protocol.expect("EvalCtx reserves protocol tokens");
    let mut env = Env::empty();
    let v = eval_module(&mut ctx, &mut env, &forms).unwrap();
    match v {
        Value::Sealed { token, .. } => assert_eq!(token, p.error),
        _ => panic!("expected sealed error, got {}", v.debug_repr()),
    }
}

#[test]
fn term_introspection_and_escape_prims_work() {
    let forms = parse_module(
        r#"
      {
        :t_nil (prim data/tag nil)
        :t_int (prim data/tag 1)
        :pl_ok (prim pair/as-proper-list (quote (1 2)))
        :pl_bad (prim pair/as-proper-list (prim pair/cons 1 2))
        :entries (prim map/entries (quote {:b 2 :a 1}))
        :sym_s (prim sym/to-str 'foo/bar::x)
        :es (prim coreform/escape-str "a\n\"")
        :eb (prim coreform/escape-bytes b"\x00\"\\")
        :rep (prim str/repeat " " 3)
        :join (prim str/join ["a" "b"] ",")
      }
    "#,
    )
    .unwrap();
    let mut ctx = EvalCtx::new();
    let mut env = Env::empty();
    let v = eval_module(&mut ctx, &mut env, &forms).unwrap();

    let m = match v {
        Value::Map(m) => m
            .into_iter()
            .map(|(k, v)| (k, v.as_data().cloned().unwrap_or(Term::Nil)))
            .collect::<std::collections::BTreeMap<_, _>>(),
        Value::Data(Term::Map(m)) => m,
        _ => panic!("expected map, got {}", v.debug_repr()),
    };

    assert!(matches!(
        m.get(&gc_coreform::TermOrdKey(Term::symbol(":t_nil"))),
        Some(Term::Symbol(s)) if s == ":nil"
    ));
    assert!(matches!(
        m.get(&gc_coreform::TermOrdKey(Term::symbol(":t_int"))),
        Some(Term::Symbol(s)) if s == ":int"
    ));
    assert!(matches!(
        m.get(&gc_coreform::TermOrdKey(Term::symbol(":pl_bad"))),
        Some(Term::Nil)
    ));
    let Term::Vector(pl) = m
        .get(&gc_coreform::TermOrdKey(Term::symbol(":pl_ok")))
        .unwrap()
    else {
        panic!("expected vector from pair/as-proper-list");
    };
    assert_eq!(pl.len(), 2);

    let Term::Vector(entries) = m
        .get(&gc_coreform::TermOrdKey(Term::symbol(":entries")))
        .unwrap()
    else {
        panic!("expected entries vector");
    };
    assert_eq!(entries.len(), 2);
    let Term::Vector(e0) = &entries[0] else {
        panic!("expected entry[0] vector");
    };
    assert!(matches!(e0.first(), Some(Term::Symbol(s)) if s == ":a"));
    assert!(matches!(e0.get(1), Some(Term::Int(i)) if i == &1.into()));

    assert!(matches!(
        m.get(&gc_coreform::TermOrdKey(Term::symbol(":sym_s"))),
        Some(Term::Str(s)) if s == "foo/bar::x"
    ));
    assert!(matches!(
        m.get(&gc_coreform::TermOrdKey(Term::symbol(":es"))),
        Some(Term::Str(s)) if s == "a\\n\\\""
    ));
    assert!(matches!(
        m.get(&gc_coreform::TermOrdKey(Term::symbol(":eb"))),
        Some(Term::Str(s)) if s == "\\x00\\\"\\\\"
    ));
    assert!(matches!(
        m.get(&gc_coreform::TermOrdKey(Term::symbol(":rep"))),
        Some(Term::Str(s)) if s == "   "
    ));
    assert!(matches!(
        m.get(&gc_coreform::TermOrdKey(Term::symbol(":join"))),
        Some(Term::Str(s)) if s == "a,b"
    ));
}

#[test]
fn vec_set_replaces_elements() {
    let forms = parse_module(r#"(prim vec/get (prim vec/set [1 2] 1 9) 1)"#).unwrap();
    let mut ctx = EvalCtx::new();
    let mut env = Env::empty();
    let v = eval_module(&mut ctx, &mut env, &forms).unwrap();
    assert_eq!(v.as_data(), Some(&Term::Int(9.into())));
}
