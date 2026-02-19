use gc_coreform::{Term, parse_module};
use num_bigint::BigInt;

use crate::{
    Env, EvalCtx, KernelErrorKind, MemLimits, Value, compile_module, decode_compiled_module_blob,
    encode_compiled_module_blob, eval_compiled_module, eval_module, eval_module_compiled,
};

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
fn compiled_eval_matches_treewalk_eval_on_pure_programs() {
    let src = r#"
      (def add3 (fn (a b c) (prim int/add a (prim int/add b c))))
      (def pickx (fn (m) (prim map/get m (quote :x))))
      (add3 10 (pickx {:x 20 :v [1 2 3]}) ((fn (z) z) 30))
    "#;
    let forms = parse_module(src).unwrap();

    let mut ctx_tree = EvalCtx::new();
    let mut env_tree = Env::empty();
    let v_tree = eval_module(&mut ctx_tree, &mut env_tree, &forms).unwrap();

    let mut ctx_comp = EvalCtx::new();
    let mut env_comp = Env::empty();
    let v_comp = eval_module_compiled(&mut ctx_comp, &mut env_comp, &forms).unwrap();

    assert_eq!(v_tree.debug_repr(), v_comp.debug_repr());
}

#[test]
fn compiled_eval_matches_treewalk_eval_with_closure_calls() {
    let src = r#"
      (def mkadder (fn (a) (fn (b) (prim int/add a b))))
      (def f (mkadder 7))
      (f 9)
    "#;
    let forms = parse_module(src).unwrap();

    let mut ctx_tree = EvalCtx::new();
    let mut env_tree = Env::empty();
    let v_tree = eval_module(&mut ctx_tree, &mut env_tree, &forms).unwrap();

    let mut ctx_comp = EvalCtx::new();
    let mut env_comp = Env::empty();
    let v_comp = eval_module_compiled(&mut ctx_comp, &mut env_comp, &forms).unwrap();

    assert_eq!(v_tree.debug_repr(), v_comp.debug_repr());
}

#[test]
fn compiled_module_blob_roundtrip_preserves_behavior() {
    let src = r#"
      (def mk (fn (x) (fn (y) (prim int/add x y))))
      (def add9 (mk 9))
      (add9 33)
    "#;
    let forms = parse_module(src).unwrap();
    let compiled = compile_module(&forms).unwrap();
    let blob = encode_compiled_module_blob(&compiled).unwrap();
    let restored = decode_compiled_module_blob(&blob).unwrap();

    let mut ctx_a = EvalCtx::new();
    let mut env_a = Env::empty();
    let out_a = eval_compiled_module(&mut ctx_a, &mut env_a, &compiled).unwrap();

    let mut ctx_b = EvalCtx::new();
    let mut env_b = Env::empty();
    let out_b = eval_compiled_module(&mut ctx_b, &mut env_b, &restored).unwrap();

    assert_eq!(out_a.debug_repr(), out_b.debug_repr());
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
            assert!(matches!(s, Value::Data(Term::Bytes(bs)) if bs.as_ref() == b"bc"));
            let u = m.get(&gc_coreform::TermOrdKey(Term::symbol(":u"))).unwrap();
            assert!(matches!(u, Value::Data(Term::Str(s)) if s == "hi"));
            let b = m.get(&gc_coreform::TermOrdKey(Term::symbol(":b"))).unwrap();
            assert!(matches!(b, Value::Data(Term::Bytes(bs)) if bs.as_ref() == b"hi"));
            let hx = m
                .get(&gc_coreform::TermOrdKey(Term::symbol(":hx")))
                .unwrap();
            assert!(matches!(hx, Value::Data(Term::Str(s)) if s == "00ff"));
            let bh = m
                .get(&gc_coreform::TermOrdKey(Term::symbol(":bh")))
                .unwrap();
            assert!(matches!(bh, Value::Data(Term::Bytes(bs)) if bs.as_ref() == b"\x00\xff"));
            let h = m.get(&gc_coreform::TermOrdKey(Term::symbol(":h"))).unwrap();
            assert!(matches!(h, Value::Data(Term::Bytes(bs)) if bs.as_ref() == want.as_slice()));
        }
        Value::Data(Term::Map(m)) => {
            assert!(matches!(
                m.get(&gc_coreform::TermOrdKey(Term::symbol(":g"))),
                Some(Term::Int(i)) if i == &255.into()
            ));
            assert!(matches!(
                m.get(&gc_coreform::TermOrdKey(Term::symbol(":s"))),
                Some(Term::Bytes(bs)) if bs.as_ref() == b"bc"
            ));
            assert!(matches!(
                m.get(&gc_coreform::TermOrdKey(Term::symbol(":u"))),
                Some(Term::Str(s)) if s == "hi"
            ));
            assert!(matches!(
                m.get(&gc_coreform::TermOrdKey(Term::symbol(":b"))),
                Some(Term::Bytes(bs)) if bs.as_ref() == b"hi"
            ));
            assert!(matches!(
                m.get(&gc_coreform::TermOrdKey(Term::symbol(":hx"))),
                Some(Term::Str(s)) if s == "00ff"
            ));
            assert!(matches!(
                m.get(&gc_coreform::TermOrdKey(Term::symbol(":bh"))),
                Some(Term::Bytes(bs)) if bs.as_ref() == b"\x00\xff"
            ));
            assert!(matches!(
                m.get(&gc_coreform::TermOrdKey(Term::symbol(":h"))),
                Some(Term::Bytes(bs)) if bs.as_ref() == want.as_slice()
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
fn utf8_encode_codepoint_produces_expected_bytes_and_rejects_invalid() {
    let forms = parse_module(
        r#"
        {
          :a (prim utf8/encode-codepoint 36)        ; '$'
          :b (prim utf8/encode-codepoint 128)       ; U+0080 -> C2 80
          :c (prim utf8/encode-codepoint 128512)    ; U+1F600 -> F0 9F 98 80
          :bad (prim utf8/encode-codepoint 1114112) ; 0x110000 (out of range)
        }
        "#,
    )
    .unwrap();
    let mut ctx = EvalCtx::new();
    let p = ctx.protocol.expect("EvalCtx reserves protocol tokens");
    let mut env = Env::empty();
    let v = eval_module(&mut ctx, &mut env, &forms).unwrap();

    let m = match v {
        Value::Map(m) => m,
        _ => panic!("expected map, got {}", v.debug_repr()),
    };
    let a = m.get(&gc_coreform::TermOrdKey(Term::symbol(":a"))).unwrap();
    assert!(matches!(a, Value::Data(Term::Bytes(bs)) if bs.as_ref() == b"$"));
    let b = m.get(&gc_coreform::TermOrdKey(Term::symbol(":b"))).unwrap();
    assert!(matches!(
        b,
        Value::Data(Term::Bytes(bs)) if bs.as_ref() == &[0xC2, 0x80][..]
    ));
    let c = m.get(&gc_coreform::TermOrdKey(Term::symbol(":c"))).unwrap();
    assert!(matches!(
        c,
        Value::Data(Term::Bytes(bs)) if bs.as_ref() == &[0xF0, 0x9F, 0x98, 0x80][..]
    ));
    let bad = m
        .get(&gc_coreform::TermOrdKey(Term::symbol(":bad")))
        .unwrap();
    match bad {
        Value::Sealed { token, .. } => assert_eq!(*token, p.error),
        _ => panic!("expected sealed error, got {}", bad.debug_repr()),
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

#[test]
fn fixed_decimal_primitives_are_deterministic() {
    let forms = parse_module(
        r#"
      {
        :sum (prim dec/to-str (prim dec/add (prim dec/parse "1.20") (prim dec/parse "2.345")))
        :mul (prim dec/to-str (prim dec/mul (prim dec/from-int 3) (prim dec/parse "2.50")))
        :lt (prim dec/lt? (prim dec/parse "-1.00") (prim dec/parse "0.00"))
        :eq (prim dec/eq? (prim dec/parse "1.2300") (prim dec/parse "1.23"))
      }
    "#,
    )
    .unwrap();
    let mut ctx = EvalCtx::new();
    let mut env = Env::empty();
    let v = eval_module(&mut ctx, &mut env, &forms).unwrap();
    let m = match v {
        Value::Map(m) => m,
        Value::Data(Term::Map(m)) => m
            .into_iter()
            .map(|(k, v)| (k, Value::Data(v)))
            .collect::<std::collections::BTreeMap<_, _>>(),
        _ => panic!("expected map, got {}", v.debug_repr()),
    };
    assert!(matches!(
        m.get(&gc_coreform::TermOrdKey(Term::symbol(":sum"))),
        Some(Value::Data(Term::Str(s))) if s == "3.545"
    ));
    assert!(matches!(
        m.get(&gc_coreform::TermOrdKey(Term::symbol(":mul"))),
        Some(Value::Data(Term::Str(s))) if s == "7.5"
    ));
    assert!(matches!(
        m.get(&gc_coreform::TermOrdKey(Term::symbol(":lt"))),
        Some(Value::Data(Term::Bool(true)))
    ));
    assert!(matches!(
        m.get(&gc_coreform::TermOrdKey(Term::symbol(":eq"))),
        Some(Value::Data(Term::Bool(true)))
    ));
}

#[test]
fn fixed_decimal_type_mismatch_is_sealed_error() {
    let forms = parse_module(r#"(prim dec/add 1 2)"#).unwrap();
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
fn fixed_decimal_parse_failure_is_sealed_error() {
    let forms = parse_module(r#"(prim dec/parse "1.")"#).unwrap();
    let mut ctx = EvalCtx::new();
    let p = ctx.protocol.expect("EvalCtx reserves protocol tokens");
    let mut env = Env::empty();
    let v = eval_module(&mut ctx, &mut env, &forms).unwrap();
    match v {
        Value::Sealed { token, .. } => assert_eq!(token, p.error),
        _ => panic!("expected sealed error, got {}", v.debug_repr()),
    }
}
