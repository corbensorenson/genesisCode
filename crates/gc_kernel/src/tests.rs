use gc_coreform::{Term, parse_module, parse_term};
use num_bigint::BigInt;
use std::collections::BTreeSet;
use std::mem::size_of;
use std::path::PathBuf;
use std::rc::Rc;

use crate::eval::PrimOp;
use crate::{
    Apply, Contract, EffectProgram, EffectRequest, Env, EvalCtx, KernelError, KernelErrorKind,
    MemLimits, NativeFn, Sym, Value, ValueMap, ValueVector, compile_module,
    compile_module_with_site_namespace, compiled_module_coverage_manifest,
    compiled_module_coverage_manifest_from_compiled, decode_compiled_module_blob,
    encode_compiled_module_blob, eval_compiled_module, eval_module, eval_module_compiled,
    eval_term, value_hash,
};

fn assert_value_int(value: &Value, expected: i64) {
    match value.to_plain_term() {
        Some(Term::Int(n)) if n == BigInt::from(expected) => {}
        other => panic!(
            "expected int {expected}, got {other:?} from {}",
            value.debug_repr()
        ),
    }
}

fn assert_value_int_decimal(value: &Value, expected: &str) {
    let expected = BigInt::parse_bytes(expected.as_bytes(), 10)
        .unwrap_or_else(|| panic!("bad decimal test expectation: {expected}"));
    match value.to_plain_term() {
        Some(Term::Int(n)) if n == expected => {}
        other => panic!(
            "expected int {expected}, got {other:?} from {}",
            value.debug_repr()
        ),
    }
}

fn assert_value_bool(value: &Value, expected: bool) {
    match value.to_plain_term() {
        Some(Term::Bool(actual)) if actual == expected => {}
        other => panic!(
            "expected bool {expected}, got {other:?} from {}",
            value.debug_repr()
        ),
    }
}

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
            assert!(matches!(payload.as_ref().as_data(), Some(Term::Map(_))));
        }
        _ => panic!("expected sealed error"),
    }
}

#[test]
fn prim_op_table_roundtrips_all_current_ops() {
    let mut names = BTreeSet::new();
    assert_eq!(PrimOp::ALL.len(), 51);
    for op in PrimOp::ALL {
        let name = op.as_str();
        assert!(names.insert(name), "duplicate prim op name: {name}");
        assert_eq!(PrimOp::from_str(name), Some(*op), "op={name}");
    }
    assert_eq!(PrimOp::from_str("does/not-exist"), None);
}

#[test]
fn value_layout_snapshot_documents_r1_5_baseline() {
    // R1.5 pre-slim baseline on this target was Value=104 bytes. Keep exact assertions so
    // representation drift is intentional and reviewed.
    assert_eq!(size_of::<Value>(), 24);
    assert_eq!(size_of::<Term>(), 40);
    assert_eq!(size_of::<ValueVector>(), 32);
    assert_eq!(size_of::<ValueMap>(), 16);
    assert_eq!(size_of::<Sym>(), 16);
    assert_eq!(size_of::<NativeFn>(), 56);
    assert_eq!(size_of::<Contract>(), 144);
    assert_eq!(size_of::<EffectProgram>(), 16);
    assert_eq!(size_of::<EffectRequest>(), 72);
    assert_eq!(size_of::<Env>(), 8);
}

#[test]
fn int_prims_keep_small_fast_path_and_bignum_overflow_semantics() {
    let int_cases = [
        ("(prim int/add 40 2)", "42"),
        ("(prim int/sub 45 3)", "42"),
        ("(prim int/mul 6 7)", "42"),
        (
            "(prim int/add 9223372036854775807 1)",
            "9223372036854775808",
        ),
        (
            "(prim int/mul 3037000500 3037000500)",
            "9223372037000250000",
        ),
    ];
    for (src, expected) in int_cases {
        let forms = parse_module(src).unwrap();

        let mut tree_ctx = EvalCtx::new();
        let mut tree_env = Env::empty();
        let tree = eval_module(&mut tree_ctx, &mut tree_env, &forms).unwrap();
        assert_value_int_decimal(&tree, expected);

        let mut compiled_ctx = EvalCtx::new();
        let mut compiled_env = Env::empty();
        let compiled = eval_module_compiled(&mut compiled_ctx, &mut compiled_env, &forms).unwrap();
        assert_value_int_decimal(&compiled, expected);
    }

    let bool_cases = [
        ("(prim int/lt? 1 2)", true),
        (
            "(prim int/eq? 9223372036854775808 9223372036854775808)",
            true,
        ),
        ("(prim int/lt? 9223372036854775808 1)", false),
    ];
    for (src, expected) in bool_cases {
        let forms = parse_module(src).unwrap();

        let mut tree_ctx = EvalCtx::new();
        let mut tree_env = Env::empty();
        let tree = eval_module(&mut tree_ctx, &mut tree_env, &forms).unwrap();
        assert_value_bool(&tree, expected);

        let mut compiled_ctx = EvalCtx::new();
        let mut compiled_env = Env::empty();
        let compiled = eval_module_compiled(&mut compiled_ctx, &mut compiled_env, &forms).unwrap();
        assert_value_bool(&compiled, expected);
    }
}

#[test]
fn compiled_binary_prim_wrapper_fast_path_preserves_values_and_errors_without_step_limit() {
    let forms = parse_module(
        r#"
      (def add (fn (a) (fn (b) (prim int/add a b))))
      (add 40 2)
    "#,
    )
    .unwrap();
    let mut ctx = EvalCtx::with_step_limit(None);
    let mut env = Env::empty();
    let value = eval_module_compiled(&mut ctx, &mut env, &forms).unwrap();
    assert_value_int(&value, 42);

    let forms = parse_module(
        r#"
      (def add (fn (a) (fn (b) (prim int/add a b))))
      (add 1 "x")
    "#,
    )
    .unwrap();
    let mut ctx = EvalCtx::with_step_limit(None);
    let p = ctx.protocol.expect("EvalCtx reserves protocol tokens");
    let mut env = Env::empty();
    let value = eval_module_compiled(&mut ctx, &mut env, &forms).unwrap();
    match value {
        Value::Sealed { token, payload } => {
            assert_eq!(token, p.error);
            assert!(matches!(payload.as_ref().as_data(), Some(Term::Map(_))));
        }
        _ => panic!("expected sealed type error, got {}", value.debug_repr()),
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
            let Some(Term::Map(m)) = payload.as_ref().as_data() else {
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
            let Some(Term::Map(m)) = payload.as_ref().as_data() else {
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
        assert_value_int(&v, 3);
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
fn treewalk_closure_capture_releases_unreferenced_lexical_values() {
    let root = Env::empty();
    let dead_vector = Value::vector(ValueVector::from_iter([
        Value::data(Term::Int(BigInt::from(1))),
        Value::data(Term::Int(BigInt::from(2))),
    ]));
    let dead_weak = match &dead_vector {
        Value::Vector(values) => Rc::downgrade(values),
        other => panic!("expected vector, got {}", other.debug_repr()),
    };
    let mut lexical = Env::with_binding(&root, "dead/target", dead_vector.clone());
    for index in 0..128 {
        lexical = Env::with_binding(
            &lexical,
            format!("dead/{index}"),
            Value::data(Term::Int(BigInt::from(index))),
        );
    }
    lexical = Env::with_binding(&lexical, "keep", Value::data(Term::Int(BigInt::from(40))));

    let term = parse_term("(fn (x) (prim int/add keep x))").unwrap();
    let mut ctx = EvalCtx::new();
    let closure = eval_term(&mut ctx, &lexical, &term).unwrap();
    assert_eq!(closure.closure_captured_value_count(), Some(1));

    drop(lexical);
    drop(dead_vector);
    assert!(
        dead_weak.upgrade().is_none(),
        "closure retained an unrelated lexical value"
    );
    assert_value_int(
        &closure
            .apply(&mut ctx, Value::data(Term::Int(BigInt::from(2))))
            .unwrap(),
        42,
    );
}

#[test]
fn compiled_closure_capture_is_sparse_and_survives_blob_roundtrip() {
    let mut bindings = vec!["(keep 40)".to_string()];
    bindings.extend((0..128).map(|index| format!("(dead{index} {index})")));
    let source = format!(
        "(let ({}) (fn (x) (prim int/add keep x)))",
        bindings.join(" ")
    );
    let forms = parse_module(&source).unwrap();

    let compiled = compile_module(&forms).unwrap();
    let blob = encode_compiled_module_blob(&compiled).unwrap();
    assert!(blob.starts_with(b"GCKM5\0"));
    let restored = decode_compiled_module_blob(&blob).unwrap();
    let mut compiled_ctx = EvalCtx::new();
    let mut compiled_env = Env::empty();
    let compiled_closure =
        eval_compiled_module(&mut compiled_ctx, &mut compiled_env, &restored).unwrap();
    assert_eq!(compiled_closure.closure_captured_value_count(), Some(1));
    assert_eq!(
        compiled_closure.compiled_closure_capture_slot_span(),
        Some(129)
    );
    assert_value_int(
        &compiled_closure
            .apply(&mut compiled_ctx, Value::data(Term::Int(BigInt::from(2))))
            .unwrap(),
        42,
    );

    let mut tree_ctx = EvalCtx::new();
    let mut tree_env = Env::empty();
    let tree_closure = eval_module(&mut tree_ctx, &mut tree_env, &forms).unwrap();
    assert_eq!(tree_closure.closure_captured_value_count(), Some(1));
    assert_value_int(
        &tree_closure
            .apply(&mut tree_ctx, Value::data(Term::Int(BigInt::from(2))))
            .unwrap(),
        42,
    );
}

#[test]
fn minimal_capture_preserves_shadowing_mutual_recursion_and_nested_binders() {
    let programs = [
        r#"
          (def x 1000)
          (def mk (fn (x) (let ((saved x)) (fn (x) (fn (z) (prim int/add saved (prim int/add x z)))))))
          (((mk 10) 20) 12)
        "#,
        r#"
          (def even? (fn (n) (if (prim int/eq? n 0) true (odd? (prim int/sub n 1)))))
          (def odd? (fn (n) (if (prim int/eq? n 0) false (even? (prim int/sub n 1)))))
          (even? 100)
        "#,
    ];

    for source in programs {
        let forms = parse_module(source).unwrap();
        let mut tree_ctx = EvalCtx::new();
        let mut tree_env = Env::empty();
        let tree = eval_module(&mut tree_ctx, &mut tree_env, &forms).unwrap();
        let mut compiled_ctx = EvalCtx::new();
        let mut compiled_env = Env::empty();
        let compiled = eval_module_compiled(&mut compiled_ctx, &mut compiled_env, &forms).unwrap();
        assert_eq!(tree.debug_repr(), compiled.debug_repr(), "{source}");
    }

    let invalid_nested_pattern = parse_module("(((fn (x) (fn ((not-a-symbol)) x)) 1) 2)").unwrap();
    let mut tree_ctx = EvalCtx::new();
    let mut tree_env = Env::empty();
    let tree_error =
        eval_module(&mut tree_ctx, &mut tree_env, &invalid_nested_pattern).unwrap_err();
    let mut compiled_ctx = EvalCtx::new();
    let mut compiled_env = Env::empty();
    let compiled_error = eval_module_compiled(
        &mut compiled_ctx,
        &mut compiled_env,
        &invalid_nested_pattern,
    )
    .unwrap_err();
    assert_eq!(tree_error.kind.to_string(), compiled_error.kind.to_string());
    assert_eq!(tree_error.msg, compiled_error.msg);
}

#[test]
fn minimal_capture_tracks_module_scope_above_a_parent_environment() {
    let forms = parse_module(
        r#"
          (def call-later (fn (x) (later (prim int/add host/base x))))
          (def later (fn (x) x))
          (call-later 2)
        "#,
    )
    .unwrap();
    let root = Env::empty();
    let parent = Env::with_binding(&root, "host/base", Value::data(Term::Int(BigInt::from(40))));

    let mut tree_env = Env::with_binding(&parent, "module/sentinel", Value::data(Term::Nil));
    let mut tree_ctx = EvalCtx::new();
    let tree = eval_module(&mut tree_ctx, &mut tree_env, &forms).unwrap();

    let mut compiled_env = Env::with_binding(&parent, "module/sentinel", Value::data(Term::Nil));
    let mut compiled_ctx = EvalCtx::new();
    let compiled = eval_module_compiled(&mut compiled_ctx, &mut compiled_env, &forms).unwrap();

    assert_value_int(&tree, 42);
    assert_eq!(tree.debug_repr(), compiled.debug_repr());
}

#[test]
fn compiled_slot_resolution_preserves_lexical_shadowing_and_module_forward_refs() {
    let src = r#"
      (def x 100)
      (def mk (fn (x) (fn (y) (prim int/add x y))))
      (def add40 (mk 40))
      (def call-forward (fn (n) (later n)))
      (def later (fn (z) (let ((x 1) (y (prim int/add x z))) (prim int/add y (add40 1)))))
      (call-forward 0)
    "#;
    let forms = parse_module(src).unwrap();

    let mut ctx_tree = EvalCtx::new();
    let mut env_tree = Env::empty();
    let v_tree = eval_module(&mut ctx_tree, &mut env_tree, &forms).unwrap();

    let mut ctx_comp = EvalCtx::new();
    let mut env_comp = Env::empty();
    let v_comp = eval_module_compiled(&mut ctx_comp, &mut env_comp, &forms).unwrap();

    assert_value_int(&v_tree, 42);
    assert_eq!(v_tree.debug_repr(), v_comp.debug_repr());
}

#[test]
fn compiled_flat_slots_preserve_deep_sequential_let_and_closure_resolution() {
    let bindings = (0..512)
        .map(|index| format!("(v{index} {index})"))
        .collect::<Vec<_>>()
        .join(" ");
    let source = format!("(let ({bindings}) (fn (x) (prim int/add (prim int/add v0 v511) x)))");
    let forms = parse_module(&source).unwrap();

    let mut tree_ctx = EvalCtx::new();
    let mut tree_env = Env::empty();
    let tree_closure = eval_module(&mut tree_ctx, &mut tree_env, &forms).unwrap();

    let mut compiled_ctx = EvalCtx::new();
    let mut compiled_env = Env::empty();
    let compiled_closure =
        eval_module_compiled(&mut compiled_ctx, &mut compiled_env, &forms).unwrap();

    assert_eq!(compiled_closure.closure_captured_value_count(), Some(2));
    assert_eq!(
        compiled_closure.compiled_closure_capture_slot_span(),
        Some(512)
    );
    let argument = Value::data(Term::Int(BigInt::from(1)));
    let tree = tree_closure.apply(&mut tree_ctx, argument.clone()).unwrap();
    let compiled = compiled_closure.apply(&mut compiled_ctx, argument).unwrap();
    assert_value_int(&tree, 512);
    assert_eq!(tree.debug_repr(), compiled.debug_repr());
}

#[test]
fn compiled_forward_reference_before_def_matches_treewalk_unbound_error() {
    let src = r#"
      (def call-later (fn (n) (later n)))
      (call-later 1)
      (def later (fn (z) z))
    "#;
    let forms = parse_module(src).unwrap();

    let mut ctx_tree = EvalCtx::new();
    let mut env_tree = Env::empty();
    let tree_err = eval_module(&mut ctx_tree, &mut env_tree, &forms).unwrap_err();

    let mut ctx_comp = EvalCtx::new();
    let mut env_comp = Env::empty();
    let comp_err = eval_module_compiled(&mut ctx_comp, &mut env_comp, &forms).unwrap_err();

    assert!(matches!(tree_err.kind, KernelErrorKind::Unbound));
    assert_eq!(tree_err.kind.to_string(), comp_err.kind.to_string());
    assert_eq!(tree_err.msg, comp_err.msg);
}

#[test]
fn compiled_external_name_fallback_preserves_mixed_env_semantics() {
    let forms = parse_module("(prim int/add external/x 1)").unwrap();
    let mut ctx = EvalCtx::new();
    let mut env = Env::empty();
    env.set_local("external/x", Value::data(Term::Int(BigInt::from(41))));

    let out = eval_module_compiled(&mut ctx, &mut env, &forms).unwrap();
    assert_value_int(&out, 42);
}

#[test]
fn compiled_eval_matches_treewalk_on_coreform_fixtures() {
    let root = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .and_then(|p| p.parent())
        .expect("crate is under <repo>/crates/gc_kernel")
        .to_path_buf();
    let fixture_dir = root.join("tests/spec/coreform");
    let mut fixtures = std::fs::read_dir(&fixture_dir)
        .unwrap_or_else(|e| panic!("read {}: {e}", fixture_dir.display()))
        .map(|entry| entry.expect("read fixture entry").path())
        .filter(|path| path.extension().is_some_and(|ext| ext == "gc"))
        .filter(|path| {
            path.file_name()
                .is_some_and(|name| name.to_string_lossy().ends_with(".in.gc"))
        })
        .collect::<Vec<_>>();
    fixtures.sort();
    assert!(!fixtures.is_empty(), "expected coreform fixtures");

    for path in fixtures {
        let src = std::fs::read_to_string(&path)
            .unwrap_or_else(|e| panic!("read {}: {e}", path.display()));
        let forms = parse_module(&src).unwrap_or_else(|e| panic!("parse {}: {e}", path.display()));

        let mut ctx_tree = EvalCtx::new();
        let mut env_tree = Env::empty();
        let tree = eval_module(&mut ctx_tree, &mut env_tree, &forms)
            .map(|v| v.debug_repr())
            .map_err(|e| format!("{:?}:{}", e.kind, e.msg));

        let mut ctx_comp = EvalCtx::new();
        let mut env_comp = Env::empty();
        let comp = eval_module_compiled(&mut ctx_comp, &mut env_comp, &forms)
            .map(|v| v.debug_repr())
            .map_err(|e| format!("{:?}:{}", e.kind, e.msg));

        assert_eq!(tree, comp, "fixture {}", path.display());
    }
}

#[test]
fn compiled_eval_can_call_legacy_treewalk_closure_from_env() {
    let legacy_src = r#"
      (def legacy/mk (fn (a) (fn (b) (prim int/add a b))))
      nil
    "#;
    let call_src = r#"
      (def plus9 (legacy/mk 9))
      (plus9 33)
    "#;

    let legacy_forms = parse_module(legacy_src).unwrap();
    let call_forms = parse_module(call_src).unwrap();

    let mut ctx = EvalCtx::new();
    let mut env = Env::empty();

    let _ = eval_module(&mut ctx, &mut env, &legacy_forms).unwrap();
    let out = eval_module_compiled(&mut ctx, &mut env, &call_forms).unwrap();

    assert_value_int(&out, 42);
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
    assert!(blob.starts_with(b"GCKM5\0"));
    let mut obsolete = blob.clone();
    obsolete[..6].copy_from_slice(b"GCKM4\0");
    assert!(decode_compiled_module_blob(&obsolete).is_err());
    let restored = decode_compiled_module_blob(&blob).unwrap();
    assert_eq!(
        compiled_module_coverage_manifest_from_compiled(&compiled),
        compiled_module_coverage_manifest_from_compiled(&restored)
    );

    let mut ctx_a = EvalCtx::new();
    let mut env_a = Env::empty();
    let out_a = eval_compiled_module(&mut ctx_a, &mut env_a, &compiled).unwrap();

    let mut ctx_b = EvalCtx::new();
    let mut env_b = Env::empty();
    let out_b = eval_compiled_module(&mut ctx_b, &mut env_b, &restored).unwrap();

    assert_eq!(out_a.debug_repr(), out_b.debug_repr());
}

#[test]
fn compiled_unknown_prim_preserves_argument_eval_order_and_blob_roundtrip() {
    let forms = parse_module("(prim does/not-exist missing)").unwrap();

    let mut tree_ctx = EvalCtx::new();
    let mut tree_env = Env::empty();
    let tree_err = eval_module(&mut tree_ctx, &mut tree_env, &forms).unwrap_err();

    let mut compiled_ctx = EvalCtx::new();
    let mut compiled_env = Env::empty();
    let compiled_err =
        eval_module_compiled(&mut compiled_ctx, &mut compiled_env, &forms).unwrap_err();

    assert!(matches!(tree_err.kind, KernelErrorKind::Unbound));
    assert!(matches!(compiled_err.kind, KernelErrorKind::Unbound));
    assert_eq!(tree_err.msg, compiled_err.msg);

    let bad_forms = parse_module("(prim does/not-exist 1)").unwrap();
    let compiled = compile_module(&bad_forms).unwrap();
    let blob = encode_compiled_module_blob(&compiled).unwrap();
    let restored = decode_compiled_module_blob(&blob).unwrap();
    let mut ctx = EvalCtx::new();
    let mut env = Env::empty();
    let err = eval_compiled_module(&mut ctx, &mut env, &restored).unwrap_err();
    assert!(matches!(err.kind, KernelErrorKind::BadForm));
    assert_eq!(err.msg, "unknown prim op: does/not-exist");
}

#[test]
fn compiled_appn_matches_treewalk_for_multi_arg_partial_and_blob_roundtrip() {
    let forms = parse_module(
        r#"
      (def add3 (fn (a b c) (prim int/add a (prim int/add b c))))
      (def add12 (add3 1 2))
      (add12 39)
    "#,
    )
    .unwrap();

    let mut tree_ctx = EvalCtx::new();
    let mut tree_env = Env::empty();
    let tree = eval_module(&mut tree_ctx, &mut tree_env, &forms).unwrap();

    let compiled = compile_module(&forms).unwrap();
    let mut compiled_ctx = EvalCtx::new();
    let mut compiled_env = Env::empty();
    let compiled_out =
        eval_compiled_module(&mut compiled_ctx, &mut compiled_env, &compiled).unwrap();

    let blob = encode_compiled_module_blob(&compiled).unwrap();
    let restored = decode_compiled_module_blob(&blob).unwrap();
    let mut restored_ctx = EvalCtx::new();
    let mut restored_env = Env::empty();
    let restored_out =
        eval_compiled_module(&mut restored_ctx, &mut restored_env, &restored).unwrap();

    assert_value_int(&tree, 42);
    assert_eq!(tree.debug_repr(), compiled_out.debug_repr());
    assert_eq!(compiled_out.debug_repr(), restored_out.debug_repr());
}

#[test]
fn compiled_appn_preserves_left_associated_error_order() {
    let forms = parse_module("(((fn (x) 0) 1) missing)").unwrap();

    let mut tree_ctx = EvalCtx::new();
    let mut tree_env = Env::empty();
    let tree_err = eval_module(&mut tree_ctx, &mut tree_env, &forms).unwrap_err();

    let mut compiled_ctx = EvalCtx::new();
    let mut compiled_env = Env::empty();
    let compiled_err =
        eval_module_compiled(&mut compiled_ctx, &mut compiled_env, &forms).unwrap_err();

    assert!(matches!(tree_err.kind, KernelErrorKind::Unbound));
    assert!(matches!(compiled_err.kind, KernelErrorKind::Unbound));
    assert_eq!(tree_err.msg, compiled_err.msg);
}

#[test]
fn compiled_appn_preserves_source_shape_step_accounting_across_sugar_and_nested_forms() {
    let sugar_forms = parse_module(
        r#"
      (def add3 (fn (a b c) (prim int/add a (prim int/add b c))))
      (add3 1 2 3)
    "#,
    )
    .unwrap();
    let nested_forms = parse_module(
        r#"
      (def add3 (fn (a b c) (prim int/add a (prim int/add b c))))
      (((add3 1) 2) 3)
    "#,
    )
    .unwrap();

    let mut sugar_ctx = EvalCtx::new();
    let mut sugar_env = Env::empty();
    let sugar = eval_module_compiled(&mut sugar_ctx, &mut sugar_env, &sugar_forms).unwrap();

    let mut nested_ctx = EvalCtx::new();
    let mut nested_env = Env::empty();
    let nested = eval_module_compiled(&mut nested_ctx, &mut nested_env, &nested_forms).unwrap();

    assert_value_int(&sugar, 6);
    assert_eq!(sugar.debug_repr(), nested.debug_repr());
    assert_eq!(
        sugar_ctx.observed_counters().steps + 2,
        nested_ctx.observed_counters().steps
    );
}

#[test]
fn compiled_step_accounting_matches_treewalk_for_appn_and_core_paths() {
    let cases = [
        ("atom", "1"),
        ("primitive", "(prim int/add 1 2)"),
        ("single_app", "((fn (x) x) 42)"),
        (
            "module_def",
            r#"
          (def id (fn (x) x))
          (id 42)
        "#,
        ),
        (
            "appn_sugar",
            r#"
          (def add3 (fn (a b c) (prim int/add a (prim int/add b c))))
          (add3 1 2 3)
        "#,
        ),
        (
            "appn_left_associated",
            r#"
          (def add3 (fn (a b c) (prim int/add a (prim int/add b c))))
          (((add3 1) 2) 3)
        "#,
        ),
    ];

    for (name, src) in cases {
        let forms = parse_module(src).unwrap_or_else(|e| panic!("parse {name}: {e}"));

        let mut tree_ctx = EvalCtx::new();
        let mut tree_env = Env::empty();
        let tree = eval_module(&mut tree_ctx, &mut tree_env, &forms)
            .unwrap_or_else(|e| panic!("treewalk {name}: {e}"));

        let mut compiled_ctx = EvalCtx::new();
        let mut compiled_env = Env::empty();
        let compiled = eval_module_compiled(&mut compiled_ctx, &mut compiled_env, &forms)
            .unwrap_or_else(|e| panic!("compiled {name}: {e}"));

        assert_eq!(tree.debug_repr(), compiled.debug_repr(), "{name}");
        assert_eq!(
            tree_ctx.observed_counters().steps,
            compiled_ctx.observed_counters().steps,
            "{name}"
        );
    }
}

fn native_test_add3(_ctx: &mut EvalCtx, args: Vec<Value>) -> Result<Value, KernelError> {
    let mut total = BigInt::from(0);
    for arg in args {
        let Some(Term::Int(n)) = arg.as_data() else {
            return Err(KernelError::new(
                KernelErrorKind::Type,
                "test/add3 expects integer arguments",
            ));
        };
        total += n;
    }
    Ok(Value::data(Term::Int(total)))
}

fn native_test_panic(_ctx: &mut EvalCtx, _args: Vec<Value>) -> Result<Value, KernelError> {
    panic!("intentional test native panic");
}

fn assert_internal_panic_boundary(err: KernelError) {
    assert!(matches!(err.kind, KernelErrorKind::Internal), "{err}");
    assert!(err.msg.contains("panicked"), "{err}");
}

#[test]
fn compiled_appn_collects_native_args_without_intermediate_value_roundtrip() {
    let forms = parse_module(
        r#"
      (def add12 (test/add3 1 2))
      (add12 39)
    "#,
    )
    .unwrap();

    let compiled = compile_module(&forms).unwrap();
    let blob = encode_compiled_module_blob(&compiled).unwrap();
    let restored = decode_compiled_module_blob(&blob).unwrap();

    let mut env = Env::empty();
    env.set_local(
        "test/add3",
        Value::native_fn(NativeFn::new("test/add3", 3, native_test_add3)),
    );
    let mut ctx = EvalCtx::new();
    let out = eval_compiled_module(&mut ctx, &mut env, &restored).unwrap();

    assert_eq!(out.as_data(), Some(&Term::Int(BigInt::from(42))));
}

#[test]
fn panic_guard_catches_panicking_native_in_treewalk_eval() {
    let forms = parse_module("(test/panic nil)").unwrap();
    let mut env = Env::empty();
    env.set_local(
        "test/panic",
        Value::native_fn(NativeFn::new("test/panic", 1, native_test_panic)),
    );
    let mut ctx = EvalCtx::new();

    let err = eval_module(&mut ctx, &mut env, &forms).unwrap_err();
    assert_internal_panic_boundary(err);
}

#[test]
fn panic_guard_catches_panicking_native_in_compiled_eval() {
    let forms = parse_module("(test/panic nil)").unwrap();
    let mut env = Env::empty();
    env.set_local(
        "test/panic",
        Value::native_fn(NativeFn::new("test/panic", 1, native_test_panic)),
    );
    let mut ctx = EvalCtx::new();

    let err = eval_module_compiled(&mut ctx, &mut env, &forms).unwrap_err();
    assert_internal_panic_boundary(err);
}

#[test]
fn panic_guard_catches_panicking_native_direct_apply_boundaries() {
    let mut ctx = EvalCtx::new();
    let native = NativeFn::new("test/panic", 1, native_test_panic);

    let err = native
        .apply(&mut ctx, Value::data(Term::Nil))
        .expect_err("native panic must become a kernel error");
    assert_internal_panic_boundary(err);

    let err = Value::native_fn(native)
        .apply(&mut ctx, Value::data(Term::Nil))
        .expect_err("value apply panic must become a kernel error");
    assert_internal_panic_boundary(err);
}

#[test]
fn coverage_decision_counts_track_treewalk_if_branches() {
    let src = r#"
      (def choose (fn (x) (if x 1 2)))
      (choose true)
      (choose false)
    "#;
    let forms = parse_module(src).unwrap();
    let mut ctx = EvalCtx::new();
    ctx.enable_coverage(BTreeSet::new());
    let mut env = Env::empty();
    let _ = eval_module(&mut ctx, &mut env, &forms).unwrap();
    let decision = ctx.coverage_decision_counts().expect("coverage enabled");
    assert_eq!(decision.total, 2);
    assert_eq!(decision.taken_true, 1);
    assert_eq!(decision.taken_false, 1);
}

#[test]
fn coverage_decision_counts_track_compiled_if_branches() {
    let src = r#"
      (def choose (fn (x) (if x 1 2)))
      (choose true)
      (choose false)
    "#;
    let forms = parse_module(src).unwrap();
    let mut ctx = EvalCtx::new();
    ctx.enable_coverage(BTreeSet::new());
    let mut env = Env::empty();
    let _ = eval_module_compiled(&mut ctx, &mut env, &forms).unwrap();
    let decision = ctx.coverage_decision_counts().expect("coverage enabled");
    assert_eq!(decision.total, 2);
    assert_eq!(decision.taken_true, 1);
    assert_eq!(decision.taken_false, 1);
}

#[test]
fn compiled_module_site_namespace_emits_distinct_coverage_sites() {
    let forms = parse_module("(def x 1)\n(def y (if true x x))\n").unwrap();
    let a = compiled_module_coverage_manifest(&forms, "pkg/a.gc").expect("coverage manifest a");
    let b = compiled_module_coverage_manifest(&forms, "pkg/b.gc").expect("coverage manifest b");
    assert!(!a.statement_sites.is_empty());
    assert!(!a.decision_sites.is_empty());
    assert_ne!(a.statement_sites, b.statement_sites);
    assert_ne!(a.decision_sites, b.decision_sites);
}

#[test]
fn compiled_eval_records_per_site_statement_and_decision_hits() {
    let forms = parse_module(
        r#"
      (def choose (fn (x) (if x 1 2)))
      (choose true)
      (choose false)
    "#,
    )
    .unwrap();
    let compiled = compile_module_with_site_namespace(&forms, "pkg/coverage.gc").unwrap();
    let mut ctx = EvalCtx::new();
    ctx.enable_coverage(BTreeSet::new());
    let mut env = Env::empty();
    let _ = eval_compiled_module(&mut ctx, &mut env, &compiled).unwrap();
    let statement_sites = ctx.coverage_statement_site_hits().expect("statement sites");
    assert!(!statement_sites.is_empty());
    let decision_sites = ctx.coverage_decision_site_hits().expect("decision sites");
    assert_eq!(decision_sites.len(), 1);
    let counts = decision_sites.values().next().unwrap();
    assert_eq!(counts.total, 2);
    assert_eq!(counts.taken_true, 1);
    assert_eq!(counts.taken_false, 1);
    let samples = ctx.coverage_decision_samples().expect("decision samples");
    assert_eq!(samples.len(), 1);
    assert_eq!(samples.values().next().unwrap().len(), 2);
}

#[test]
fn compiled_returned_closure_keeps_dense_coverage_table_after_module_flush() {
    let forms = parse_module(
        r#"
      (def mk (fn (x) (fn (y) (if x y 0))))
      (mk true)
    "#,
    )
    .unwrap();
    let compiled = compile_module_with_site_namespace(&forms, "pkg/returned-closure.gc").unwrap();
    let mut ctx = EvalCtx::new();
    ctx.enable_coverage(BTreeSet::new());
    let mut env = Env::empty();
    let closure = eval_compiled_module(&mut ctx, &mut env, &compiled).unwrap();
    let out = closure
        .apply(&mut ctx, Value::data(Term::Int(BigInt::from(42))))
        .unwrap();
    assert_eq!(out.as_data(), Some(&Term::Int(BigInt::from(42))));

    let decision_sites = ctx.coverage_decision_site_hits().expect("decision sites");
    assert_eq!(decision_sites.len(), 1);
    let (site_id, counts) = decision_sites.iter().next().unwrap();
    assert!(site_id.starts_with("pkg/returned-closure.gc::decision:"));
    assert_eq!(counts.total, 1);
    assert_eq!(counts.taken_true, 1);
    assert_eq!(counts.taken_false, 0);
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
            assert_value_int(vl, 3);
            let ml = m
                .get(&gc_coreform::TermOrdKey(Term::symbol(":ml")))
                .unwrap();
            assert_value_int(ml, 2);
        }
        Value::Data(t) if matches!(t.as_ref(), Term::Map(_)) => {
            let Term::Map(m) = t.as_ref() else {
                panic!("expected map datum");
            };
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
            assert_value_int(g, 255);
            let s = m.get(&gc_coreform::TermOrdKey(Term::symbol(":s"))).unwrap();
            assert!(matches!(s.as_data(), Some(Term::Bytes(bs)) if bs.as_ref() == b"bc"));
            let u = m.get(&gc_coreform::TermOrdKey(Term::symbol(":u"))).unwrap();
            assert!(matches!(u.as_data(), Some(Term::Str(s)) if s == "hi"));
            let b = m.get(&gc_coreform::TermOrdKey(Term::symbol(":b"))).unwrap();
            assert!(matches!(b.as_data(), Some(Term::Bytes(bs)) if bs.as_ref() == b"hi"));
            let hx = m
                .get(&gc_coreform::TermOrdKey(Term::symbol(":hx")))
                .unwrap();
            assert!(matches!(hx.as_data(), Some(Term::Str(s)) if s == "00ff"));
            let bh = m
                .get(&gc_coreform::TermOrdKey(Term::symbol(":bh")))
                .unwrap();
            assert!(matches!(bh.as_data(), Some(Term::Bytes(bs)) if bs.as_ref() == b"\x00\xff"));
            let h = m.get(&gc_coreform::TermOrdKey(Term::symbol(":h"))).unwrap();
            assert!(matches!(h.as_data(), Some(Term::Bytes(bs)) if bs.as_ref() == want.as_slice()));
        }
        Value::Data(t) if matches!(t.as_ref(), Term::Map(_)) => {
            let Term::Map(m) = t.as_ref() else {
                panic!("expected map datum");
            };
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
    assert!(matches!(a.as_data(), Some(Term::Bytes(bs)) if bs.as_ref() == b"$"));
    let b = m.get(&gc_coreform::TermOrdKey(Term::symbol(":b"))).unwrap();
    assert!(matches!(
        b.as_data(),
        Some(Term::Bytes(bs)) if bs.as_ref() == &[0xC2, 0x80][..]
    ));
    let c = m.get(&gc_coreform::TermOrdKey(Term::symbol(":c"))).unwrap();
    assert!(matches!(
        c.as_data(),
        Some(Term::Bytes(bs)) if bs.as_ref() == &[0xF0, 0x9F, 0x98, 0x80][..]
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
            .iter()
            .map(|(k, v)| (k.clone(), v.as_data().cloned().unwrap_or(Term::Nil)))
            .collect::<std::collections::BTreeMap<_, _>>(),
        Value::Data(t) if matches!(t.as_ref(), Term::Map(_)) => {
            let Term::Map(m) = t.as_ref() else {
                panic!("expected map datum");
            };
            m.clone()
        }
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
    assert_value_int(&v, 9);
}

#[test]
fn persistent_vector_ops_do_not_mutate_shared_original() {
    let forms = parse_module(
        r#"
      (def v [1 2])
      {:v v :push (prim vec/push v 3) :set (prim vec/set v 0 9)}
    "#,
    )
    .unwrap();
    let mut ctx = EvalCtx::new();
    let mut env = Env::empty();
    let v = eval_module(&mut ctx, &mut env, &forms).unwrap();
    let Value::Map(m) = v else {
        panic!("expected map");
    };
    assert_eq!(
        m.get(&gc_coreform::TermOrdKey(Term::symbol(":v")))
            .map(Value::debug_repr),
        Some("[1 2]".to_string())
    );
    assert_eq!(
        m.get(&gc_coreform::TermOrdKey(Term::symbol(":push")))
            .map(Value::debug_repr),
        Some("[1 2 3]".to_string())
    );
    assert_eq!(
        m.get(&gc_coreform::TermOrdKey(Term::symbol(":set")))
            .map(Value::debug_repr),
        Some("[9 2]".to_string())
    );
}

#[test]
fn persistent_map_ops_preserve_term_ordered_observation() {
    let forms =
        parse_module(r#"(prim map/put (prim map/put {} (quote :b) 2) (quote :a) 1)"#).unwrap();
    let mut ctx = EvalCtx::new();
    let mut env = Env::empty();
    let v = eval_module(&mut ctx, &mut env, &forms).unwrap();
    assert_eq!(v.debug_repr(), "{:a 1 :b 2}");
}

#[test]
fn persistent_collection_hashes_match_treewalk_and_compiled() {
    let forms = parse_module(
        r#"
      (def v [1 2])
      (def m (prim map/put {:b 2} (quote :a) (prim vec/push v 3)))
      {:m m :v2 (prim vec/set v 1 9)}
    "#,
    )
    .unwrap();

    let mut tree_ctx = EvalCtx::new();
    let mut tree_env = Env::empty();
    let tree = eval_module(&mut tree_ctx, &mut tree_env, &forms).unwrap();

    let mut compiled_ctx = EvalCtx::new();
    let mut compiled_env = Env::empty();
    let compiled = eval_module_compiled(&mut compiled_ctx, &mut compiled_env, &forms).unwrap();

    assert_eq!(tree.debug_repr(), compiled.debug_repr());
    assert_eq!(value_hash(&tree), value_hash(&compiled));
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
        Value::Map(m) => m
            .iter()
            .map(|(k, v)| (k.clone(), v.clone()))
            .collect::<std::collections::BTreeMap<_, _>>(),
        Value::Data(t) if matches!(t.as_ref(), Term::Map(_)) => {
            let Term::Map(m) = t.as_ref() else {
                panic!("expected map datum");
            };
            m.iter()
                .map(|(k, v)| (k.clone(), Value::data(v.clone())))
                .collect::<std::collections::BTreeMap<_, _>>()
        }
        _ => panic!("expected map, got {}", v.debug_repr()),
    };
    assert!(matches!(
        m.get(&gc_coreform::TermOrdKey(Term::symbol(":sum"))),
        Some(v) if matches!(v.as_data(), Some(Term::Str(s)) if s == "3.545")
    ));
    assert!(matches!(
        m.get(&gc_coreform::TermOrdKey(Term::symbol(":mul"))),
        Some(v) if matches!(v.as_data(), Some(Term::Str(s)) if s == "7.5")
    ));
    assert!(matches!(
        m.get(&gc_coreform::TermOrdKey(Term::symbol(":lt"))),
        Some(v) if matches!(v.as_data(), Some(Term::Bool(true)))
    ));
    assert!(matches!(
        m.get(&gc_coreform::TermOrdKey(Term::symbol(":eq"))),
        Some(v) if matches!(v.as_data(), Some(Term::Bool(true)))
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
