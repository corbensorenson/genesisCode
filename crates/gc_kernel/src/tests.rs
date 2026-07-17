use gc_coreform::{Term, parse_module, parse_term};
use num_bigint::BigInt;
use std::collections::BTreeSet;
use std::mem::size_of;
use std::path::PathBuf;

use crate::compiled::{
    appn_native_partial_materializations, primitive_forward_executions,
    reset_appn_native_partial_materializations, reset_primitive_forward_executions,
    reset_tail_loop_executions, tail_loop_executions,
};
use crate::eval::PrimOp;
use crate::eval::{evaluator_max_call_depth, reset_evaluator_max_call_depth};
use crate::{
    Apply, Contract, EffectProgram, EffectRequest, Env, EvalCtx, EvalObservedCounters, KernelError,
    KernelErrorKind, MemLimits, NativeFn, Sym, Value, ValueMap, ValueVector, compile_module,
    compile_module_with_site_namespace, compiled_module_coverage_manifest,
    compiled_module_coverage_manifest_from_compiled, decode_compiled_module_blob,
    encode_compiled_module_blob, eval_compiled_module, eval_module, eval_module_compiled,
    eval_term, value_hash,
};

#[derive(Debug, Eq, PartialEq)]
struct TierObservation {
    result: Result<(String, [u8; 32]), (String, String)>,
    counters: EvalObservedCounters,
}

fn observe_tier(
    source: &str,
    compiled: bool,
    step_limit: Option<u64>,
    mem_limits: MemLimits,
) -> TierObservation {
    let forms =
        parse_module(source).unwrap_or_else(|error| panic!("parse differential case: {error}"));
    let mut ctx = EvalCtx::with_step_limit(step_limit);
    ctx.set_mem_limits(mem_limits);
    let mut env = Env::empty();
    let result = if compiled {
        eval_module_compiled(&mut ctx, &mut env, &forms)
    } else {
        eval_module(&mut ctx, &mut env, &forms)
    };
    TierObservation {
        result: result
            .map(|value| (value.debug_repr(), value_hash(&value)))
            .map_err(|error| (error.kind.to_string(), error.msg)),
        counters: ctx.observed_counters(),
    }
}

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
    assert_eq!(size_of::<ValueVector>(), 24);
    assert_eq!(size_of::<ValueMap>(), 24);
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

fn eval_tree_compiled_and_restored(src: &str) -> (Value, Value, Value, u64, u64) {
    let forms = parse_module(src).unwrap();
    let mut tree_ctx = EvalCtx::new();
    let mut tree_env = Env::empty();
    let tree = eval_module(&mut tree_ctx, &mut tree_env, &forms).unwrap();

    reset_primitive_forward_executions();
    let compiled_module = compile_module(&forms).unwrap();
    let blob = encode_compiled_module_blob(&compiled_module).unwrap();
    let mut compiled_ctx = EvalCtx::new();
    let mut compiled_env = Env::empty();
    let compiled =
        eval_compiled_module(&mut compiled_ctx, &mut compiled_env, &compiled_module).unwrap();
    assert!(primitive_forward_executions() > 0, "source: {src}");

    let restored_module = decode_compiled_module_blob(&blob).unwrap();
    assert_eq!(encode_compiled_module_blob(&restored_module).unwrap(), blob);
    let mut restored_ctx = EvalCtx::new();
    let mut restored_env = Env::empty();
    let restored =
        eval_compiled_module(&mut restored_ctx, &mut restored_env, &restored_module).unwrap();
    (
        tree,
        compiled,
        restored,
        tree_ctx.observed_counters().steps,
        compiled_ctx.observed_counters().steps,
    )
}

#[test]
fn compiled_primitive_forward_plan_is_semantic_and_differentially_exact() {
    let cases = [
        (
            "(def length (fn (value) (prim str/len value))) (length \"hi\")",
            "2",
        ),
        (
            "(def subtract-reversed (fn (left right) (prim int/sub right left))) (subtract-reversed 2 44)",
            "42",
        ),
        (
            "(def duplicate-first (fn (first ignored) (prim int/add first first))) (duplicate-first 21 999)",
            "42",
        ),
        (
            "(def outer-pair (fn (first ignored last) (prim int/add first last))) (outer-pair 40 999 2)",
            "42",
        ),
    ];
    for (src, expected) in cases {
        let (tree, compiled, restored, tree_steps, compiled_steps) =
            eval_tree_compiled_and_restored(src);
        assert_eq!(tree.debug_repr(), expected, "source: {src}");
        assert_eq!(compiled.debug_repr(), tree.debug_repr(), "source: {src}");
        assert_eq!(restored.debug_repr(), tree.debug_repr(), "source: {src}");
        assert_eq!(value_hash(&compiled), value_hash(&tree), "source: {src}");
        assert_eq!(value_hash(&restored), value_hash(&tree), "source: {src}");
        assert_eq!(compiled_steps, tree_steps, "source: {src}");
    }
}

#[test]
fn compiled_primitive_forward_plan_preserves_errors_limits_coverage_and_partial_hashes() {
    let error_src = r#"
      (def add (fn (left right) (prim int/add left right)))
      (add 1 "not-an-int")
    "#;
    let (tree, compiled, restored, tree_steps, compiled_steps) =
        eval_tree_compiled_and_restored(error_src);
    assert_eq!(compiled.debug_repr(), tree.debug_repr());
    assert_eq!(restored.debug_repr(), tree.debug_repr());
    assert_eq!(value_hash(&compiled), value_hash(&tree));
    assert_eq!(compiled_steps, tree_steps);

    let over_forms =
        parse_module("(def add (fn (left right) (prim int/add left right))) (add 40 2 missing)")
            .unwrap();
    let mut over_tree_ctx = EvalCtx::new();
    let mut over_tree_env = Env::empty();
    let tree_error = eval_module(&mut over_tree_ctx, &mut over_tree_env, &over_forms).unwrap_err();
    let mut over_compiled_ctx = EvalCtx::new();
    let mut over_compiled_env = Env::empty();
    reset_primitive_forward_executions();
    let compiled_error =
        eval_module_compiled(&mut over_compiled_ctx, &mut over_compiled_env, &over_forms)
            .unwrap_err();
    assert!(primitive_forward_executions() > 0);
    assert_eq!(
        std::mem::discriminant(&compiled_error.kind),
        std::mem::discriminant(&tree_error.kind)
    );
    assert_eq!(compiled_error.msg, tree_error.msg);

    let forms =
        parse_module("(def add (fn (left right) (prim int/add left right))) (add 40 2)").unwrap();
    let mut baseline_ctx = EvalCtx::new();
    let mut baseline_env = Env::empty();
    let _ = eval_module(&mut baseline_ctx, &mut baseline_env, &forms).unwrap();
    let exact_steps = baseline_ctx.observed_counters().steps;
    let mut limited_ctx = EvalCtx::with_step_limit(Some(exact_steps - 1));
    let mut limited_env = Env::empty();
    reset_primitive_forward_executions();
    let err = eval_module_compiled(&mut limited_ctx, &mut limited_env, &forms).unwrap_err();
    assert!(matches!(err.kind, KernelErrorKind::StepLimit));
    assert_eq!(limited_ctx.observed_counters().steps, exact_steps);
    assert!(primitive_forward_executions() > 0);

    let mut coverage_ctx = EvalCtx::new();
    coverage_ctx.enable_coverage(BTreeSet::from(["left".to_string(), "right".to_string()]));
    let mut coverage_env = Env::empty();
    reset_primitive_forward_executions();
    let covered = eval_module_compiled(&mut coverage_ctx, &mut coverage_env, &forms).unwrap();
    assert_eq!(covered.debug_repr(), "42");
    assert!(primitive_forward_executions() > 0);
    assert_eq!(coverage_ctx.coverage_hits().unwrap().get("left"), Some(&1));
    assert_eq!(coverage_ctx.coverage_hits().unwrap().get("right"), Some(&1));
    assert_eq!(
        coverage_ctx
            .coverage_statement_site_hits()
            .unwrap()
            .values()
            .sum::<u64>(),
        3
    );

    let partial_forms =
        parse_module("(def add (fn (left right) (prim int/add left right))) (add 40)").unwrap();
    let partial_module = compile_module(&partial_forms).unwrap();
    let partial_blob = encode_compiled_module_blob(&partial_module).unwrap();
    let mut compiled_ctx = EvalCtx::new();
    let mut compiled_env = Env::empty();
    reset_primitive_forward_executions();
    let compiled_partial =
        eval_compiled_module(&mut compiled_ctx, &mut compiled_env, &partial_module).unwrap();
    assert_eq!(primitive_forward_executions(), 0);
    let restored_partial_module = decode_compiled_module_blob(&partial_blob).unwrap();
    let mut restored_partial_ctx = EvalCtx::new();
    let mut restored_partial_env = Env::empty();
    let restored_partial = eval_compiled_module(
        &mut restored_partial_ctx,
        &mut restored_partial_env,
        &restored_partial_module,
    )
    .unwrap();
    assert_eq!(value_hash(&compiled_partial), value_hash(&restored_partial));

    let nontrivial = parse_module(
        "(def add (fn (left right) (let ((copy left)) (prim int/add copy right)))) (add 40 2)",
    )
    .unwrap();
    let mut ctx = EvalCtx::new();
    let mut env = Env::empty();
    reset_primitive_forward_executions();
    assert_value_int(
        &eval_module_compiled(&mut ctx, &mut env, &nontrivial).unwrap(),
        42,
    );
    assert_eq!(primitive_forward_executions(), 0);
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
fn reference_compiled_differential_matrix_covers_semantic_observables() {
    let cases = [
        (
            "data-and-collections",
            "{:answer (prim int/add 40 2) :items (prim vec/push [1 2] 3)}",
            Some(1_000),
            MemLimits::default(),
        ),
        (
            "closure-and-shadowing",
            "(def mk (fn (x) (fn (x) (prim int/add x 2))))\n((mk 100) 40)",
            Some(1_000),
            MemLimits::default(),
        ),
        (
            "seal-roundtrip",
            "(def token (seal))\n(unseal (seal 42 token) token)",
            Some(1_000),
            MemLimits::default(),
        ),
        (
            "sealed-type-error",
            "(prim int/add 1 \"not-an-int\")",
            Some(1_000),
            MemLimits::default(),
        ),
        (
            "explicit-unbound-error",
            "missing/value",
            Some(1_000),
            MemLimits::default(),
        ),
        (
            "explicit-step-limit",
            "(prim int/add 1 2)",
            Some(2),
            MemLimits::default(),
        ),
        (
            "explicit-memory-limit",
            "[1]",
            Some(1_000),
            MemLimits {
                max_vec_len: Some(0),
                ..MemLimits::default()
            },
        ),
    ];

    for (name, source, step_limit, mem_limits) in cases {
        let reference = observe_tier(source, false, step_limit, mem_limits);
        let optimized = observe_tier(source, true, step_limit, mem_limits);
        assert_eq!(reference, optimized, "tier divergence in {name}");

        if reference.result.is_ok() && reference.counters.steps > 0 {
            let short_limit = reference.counters.steps - 1;
            let reference_short = observe_tier(source, false, Some(short_limit), mem_limits);
            let optimized_short = observe_tier(source, true, Some(short_limit), mem_limits);
            assert_eq!(
                reference_short, optimized_short,
                "limit divergence in {name}"
            );
            assert!(
                matches!(&reference_short.result, Err((kind, _)) if kind == "step limit exceeded"),
                "one-step-short control did not fail closed in {name}: {reference_short:?}"
            );
        }
    }
}

#[test]
fn workload_recognizer_retirement_variants_preserve_semantic_observables() {
    let variants = [
        (
            "direct-counted-vector",
            true,
            r#"
              (def gather (fn (cursor limit out)
                (if (prim int/eq? cursor limit)
                  out
                  (gather (prim int/add cursor 1) limit (prim vec/push out cursor)))))
              (gather 0 32 [])
            "#,
        ),
        (
            "let-rewritten-counted-vector",
            true,
            r#"
              (def assemble (fn (position stop result)
                (if (prim int/lt? position stop)
                  (let ((next (prim int/add position 1))
                        (extended (prim vec/push result position)))
                    (assemble next stop extended))
                  result)))
              (assemble 0 32 [])
            "#,
        ),
        (
            "shared-accumulator-branch",
            true,
            r#"
              (def preserve (fn (position stop current previous)
                (if (prim int/lt? position stop)
                  (preserve
                    (prim int/add position 1)
                    stop
                    (prim vec/push current position)
                    current)
                  previous)))
              (preserve 0 4 [] [])
            "#,
        ),
        (
            "direct-byte-in-bounds",
            false,
            r#"
              (def byte-at (fn (blob offset)
                (if (prim int/lt? offset (prim bytes/len blob))
                  (prim bytes/get blob offset)
                  nil)))
              (byte-at b"abc" 1)
            "#,
        ),
        (
            "branch-rewritten-byte-out-of-bounds",
            false,
            r#"
              (def read-byte (fn (payload index)
                (if (prim int/lt? index (prim bytes/len payload))
                  (prim bytes/get payload index)
                  nil)))
              (read-byte b"abc" 9)
            "#,
        ),
    ];

    for (name, accelerated, source) in variants {
        let reference = observe_tier(source, false, None, MemLimits::default());
        reset_tail_loop_executions();
        let optimized = observe_tier(source, true, None, MemLimits::default());
        assert_eq!(reference, optimized, "tier divergence in {name}");
        assert!(
            reference.result.is_ok(),
            "variant failed in {name}: {reference:?}"
        );
        assert_eq!(
            tail_loop_executions(),
            usize::from(accelerated),
            "semantics-derived loop-plan selection drifted in {name}"
        );

        let short_limit = reference.counters.steps.saturating_sub(1);
        reset_tail_loop_executions();
        let reference_short = observe_tier(source, false, Some(short_limit), MemLimits::default());
        let optimized_short = observe_tier(source, true, Some(short_limit), MemLimits::default());
        assert_eq!(
            reference_short, optimized_short,
            "one-step-short divergence in {name}"
        );
        assert!(
            matches!(&reference_short.result, Err((kind, _)) if kind == "step limit exceeded"),
            "one-step-short control did not fail closed in {name}: {reference_short:?}"
        );
        assert_eq!(
            tail_loop_executions(),
            0,
            "resource-limited execution must retain the ordinary evaluator in {name}"
        );
    }
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
        Value::Vector(values) => values.weak_alive_probe(),
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
    assert!(!dead_weak(), "closure retained an unrelated lexical value");
    assert_value_int(
        &closure
            .apply(&mut ctx, Value::data(Term::Int(BigInt::from(2))))
            .unwrap(),
        42,
    );
}

fn value_owner_probe(value: &Value) -> Box<dyn Fn() -> bool> {
    match value {
        Value::Closure(owner) => owner.weak_alive_probe(),
        Value::CompiledClosure(owner) => owner.weak_alive_probe(),
        Value::Vector(owner) => owner.weak_alive_probe(),
        other => panic!("expected cycle-capable value, got {}", other.debug_repr()),
    }
}

fn trigger_collection_boundary(ctx: &mut EvalCtx) {
    let fresh = Env::empty();
    let nil = parse_term("nil").unwrap();
    assert_eq!(eval_term(ctx, &fresh, &nil).unwrap().debug_repr(), "nil");
}

#[test]
fn recursive_module_cycles_are_reclaimed_after_treewalk_and_compiled_roots_retire() {
    let forms = parse_module("(def recurse (fn (x) (recurse x)))").unwrap();

    for compiled in [false, true] {
        let mut ctx = EvalCtx::new();
        let mut env = Env::empty();
        if compiled {
            eval_module_compiled(&mut ctx, &mut env, &forms).unwrap();
        } else {
            eval_module(&mut ctx, &mut env, &forms).unwrap();
        }
        let closure = env.get("recurse").unwrap();
        let alive = value_owner_probe(&closure);
        let hash_before = value_hash(&closure);

        trigger_collection_boundary(&mut ctx);
        assert!(alive(), "a live recursive closure was collected");
        assert_eq!(value_hash(&closure), hash_before);

        drop(closure);
        drop(env);
        assert!(alive(), "the cycle disappeared without a safe point");
        trigger_collection_boundary(&mut ctx);
        assert!(!alive(), "an unreachable recursive module cycle leaked");
    }
}

#[test]
fn indirect_container_cycles_are_reclaimed_across_repeated_sessions() {
    let forms =
        parse_module("(def boxed [(fn (x) (prim vec/len boxed))])\n(prim vec/len boxed)").unwrap();

    for compiled in [false, true] {
        let mut ctx = EvalCtx::new();
        let mut retired = Vec::new();
        for _ in 0..128 {
            let mut env = Env::empty();
            let result = if compiled {
                eval_module_compiled(&mut ctx, &mut env, &forms).unwrap()
            } else {
                eval_module(&mut ctx, &mut env, &forms).unwrap()
            };
            assert_value_int(&result, 1);
            let boxed = env.get("boxed").unwrap();
            retired.push(value_owner_probe(&boxed));
            drop(boxed);
            drop(env);
        }

        trigger_collection_boundary(&mut ctx);
        assert!(
            retired.iter().all(|alive| !alive()),
            "an indirect container/closure/module cycle leaked"
        );
    }
}

fn cycle_test_native(_ctx: &mut EvalCtx, mut args: Vec<Value>) -> Result<Value, KernelError> {
    Ok(args.pop().unwrap_or_else(|| Value::data(Term::Nil)))
}

fn cycle_test_contract(handler: Value, meta: Value) -> Contract {
    Contract {
        handler,
        proto: None,
        meta,
        overrides: std::collections::BTreeMap::new(),
        shape_id: [0; 32],
        contract_id: [0; 32],
    }
}

#[test]
fn every_cycle_capable_value_edge_participates_in_reclamation() {
    type Wrapper = fn(Value) -> Value;
    let wrappers: &[(&str, Wrapper)] = &[
        ("vector", |closure| {
            Value::vector(ValueVector::from_iter([closure]))
        }),
        ("map", |closure| {
            Value::map(ValueMap::from_iter([(
                gc_coreform::TermOrdKey(Term::Int(BigInt::from(0))),
                closure,
            )]))
        }),
        ("sealed payload", |closure| Value::Sealed {
            token: crate::SealId(0),
            payload: Box::new(closure),
        }),
        ("native partial", |closure| {
            Value::native_fn(NativeFn {
                name: "cycle-test",
                arity: 2,
                collected: vec![closure],
                func: cycle_test_native,
            })
        }),
        ("contract handler", |closure| {
            Value::Contract(crate::Shared::new(cycle_test_contract(
                closure,
                Value::data(Term::Nil),
            )))
        }),
        ("contract metadata", |closure| {
            Value::Contract(crate::Shared::new(cycle_test_contract(
                Value::data(Term::Nil),
                closure,
            )))
        }),
        ("contract override", |closure| {
            let mut contract = cycle_test_contract(Value::data(Term::Nil), Value::data(Term::Nil));
            contract.overrides.insert("call".to_string(), closure);
            Value::Contract(crate::Shared::new(contract))
        }),
        ("contract prototype", |closure| {
            let prototype =
                crate::Shared::new(cycle_test_contract(closure, Value::data(Term::Nil)));
            let mut contract = cycle_test_contract(Value::data(Term::Nil), Value::data(Term::Nil));
            contract.proto = Some(prototype);
            Value::Contract(crate::Shared::new(contract))
        }),
        ("pure effect program", |closure| {
            Value::EffectProgram(Box::new(EffectProgram::Pure(Box::new(closure))))
        }),
        ("perform effect program", |closure| {
            Value::EffectProgram(Box::new(EffectProgram::Perform {
                request: Box::new(closure),
            }))
        }),
        ("effect continuation", |closure| {
            Value::effect_request(EffectRequest {
                op: "cycle/test".to_string(),
                payload: Term::Nil,
                k: Box::new(closure),
            })
        }),
    ];

    let mut ctx = EvalCtx::new();
    for (label, wrap) in wrappers {
        let mut env = Env::empty();
        let closure = Value::closure("x".to_string(), Term::Symbol("x".to_string()), env.clone());
        let alive = value_owner_probe(&closure);
        env.set_local("cycle", wrap(closure.clone()));

        drop(closure);
        drop(env);
        assert!(alive(), "{label} cycle disappeared before a safe point");
        trigger_collection_boundary(&mut ctx);
        assert!(!alive(), "{label} cycle leaked");
    }
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

fn countdown_tail_expected_steps(iterations: u64) -> u64 {
    iterations.saturating_add(1).saturating_mul(9)
}

fn run_countdown_tail_loop_with_limit(
    iterations: u64,
    compiled: bool,
    step_limit: u64,
) -> (Result<Value, KernelError>, u64, u32) {
    let source = format!(
        "(def countdown (fn (n) (if (prim int/eq? n 0) 0 (countdown (prim int/sub n 1)))))\n(countdown {iterations})\n"
    );
    let forms = parse_module(&source).unwrap();
    let mut ctx = EvalCtx::with_step_limit(Some(step_limit));
    let mut env = Env::empty();
    reset_evaluator_max_call_depth();
    let value = if compiled {
        eval_module_compiled(&mut ctx, &mut env, &forms)
    } else {
        eval_module(&mut ctx, &mut env, &forms)
    };
    (
        value,
        ctx.observed_counters().steps,
        evaluator_max_call_depth(),
    )
}

fn run_countdown_tail_loop(iterations: u64, compiled: bool) -> (Value, u64, u32) {
    let expected_steps = countdown_tail_expected_steps(iterations);
    let (value, steps, depth) =
        run_countdown_tail_loop_with_limit(iterations, compiled, expected_steps);
    (value.unwrap(), steps, depth)
}

#[test]
fn tail_loop_step_accounting_is_exact_linear_and_cross_evaluator() {
    let samples = [0_u64, 1, 2, 10];
    let mut observed = Vec::new();
    for iterations in samples {
        let (tree, tree_steps, tree_depth) = run_countdown_tail_loop(iterations, false);
        let (compiled, compiled_steps, compiled_depth) = run_countdown_tail_loop(iterations, true);
        assert_eq!(tree.debug_repr(), compiled.debug_repr());
        assert_eq!(tree_steps, compiled_steps);
        assert_eq!(tree_steps, countdown_tail_expected_steps(iterations));
        assert_eq!(tree_depth, compiled_depth);
        assert_eq!(tree_depth, 3);
        observed.push((iterations, tree_steps, tree_depth));
    }
    assert_eq!(observed, [(0, 9, 3), (1, 18, 3), (2, 27, 3), (10, 99, 3)]);
}

#[test]
fn tail_loop_rejects_a_one_step_short_budget_identically() {
    const ITERATIONS: u64 = 10;
    let expected_steps = countdown_tail_expected_steps(ITERATIONS);
    for compiled in [false, true] {
        let (result, observed_steps, depth) =
            run_countdown_tail_loop_with_limit(ITERATIONS, compiled, expected_steps - 1);
        let error = result.unwrap_err();
        assert!(matches!(error.kind, KernelErrorKind::StepLimit));
        assert_eq!(error.msg, "step limit exceeded");
        assert_eq!(observed_steps, expected_steps);
        assert_eq!(depth, 3);
    }
}

#[test]
#[ignore = "stress-gate"]
fn tail_loop_ten_million_iterations_has_constant_evaluator_depth() {
    const ITERATIONS: u64 = 10_000_000;
    let (tree, tree_steps, tree_depth) = run_countdown_tail_loop(ITERATIONS, false);
    let (compiled, compiled_steps, compiled_depth) = run_countdown_tail_loop(ITERATIONS, true);
    assert_value_int(&tree, 0);
    assert_eq!(tree.debug_repr(), compiled.debug_repr());
    assert_eq!(tree_steps, compiled_steps);
    assert_eq!(tree_steps, countdown_tail_expected_steps(ITERATIONS));
    assert_eq!(tree_depth, compiled_depth);
    assert_eq!(tree_depth, 3);
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
    reset_appn_native_partial_materializations();
    let out = eval_compiled_module(&mut ctx, &mut env, &restored).unwrap();

    assert_eq!(out.as_data(), Some(&Term::Int(BigInt::from(42))));
    assert_eq!(appn_native_partial_materializations(), 1);

    let full_forms = parse_module("(test/add3 1 2 39)").unwrap();
    let mut full_env = Env::empty();
    full_env.set_local(
        "test/add3",
        Value::native_fn(NativeFn::new("test/add3", 3, native_test_add3)),
    );
    let mut full_ctx = EvalCtx::new();
    reset_appn_native_partial_materializations();
    let full = eval_module_compiled(&mut full_ctx, &mut full_env, &full_forms).unwrap();
    assert_eq!(full.as_data(), Some(&Term::Int(BigInt::from(42))));
    assert_eq!(appn_native_partial_materializations(), 0);
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
fn persistent_map_updates_preserve_branches_and_merge_right_bias() {
    let forms = parse_module(
        r#"
      (def base {:z 0 :same 1})
      (def branch-a (prim map/put base (quote :a) 2))
      (def branch-b (prim map/put base (quote :b) 3))
      (def replaced (prim map/put branch-a (quote :same) 9))
      (def merged (prim map/merge branch-a {:same 7 :b 3}))
      {:base base :branch-a branch-a :branch-b branch-b :merged merged :replaced replaced}
    "#,
    )
    .unwrap();

    let mut tree_ctx = EvalCtx::new();
    let mut tree_env = Env::empty();
    let tree = eval_module(&mut tree_ctx, &mut tree_env, &forms).unwrap();
    let mut compiled_ctx = EvalCtx::new();
    let mut compiled_env = Env::empty();
    let compiled = eval_module_compiled(&mut compiled_ctx, &mut compiled_env, &forms).unwrap();

    let expected = concat!(
        "{:base {:same 1 :z 0} ",
        ":branch-a {:a 2 :same 1 :z 0} ",
        ":branch-b {:b 3 :same 1 :z 0} ",
        ":merged {:a 2 :b 3 :same 7 :z 0} ",
        ":replaced {:a 2 :same 9 :z 0}}"
    );
    assert_eq!(tree.debug_repr(), expected);
    assert_eq!(compiled.debug_repr(), expected);
    assert_eq!(value_hash(&tree), value_hash(&compiled));
}

#[test]
fn persistent_map_hash_and_shape_ignore_insertion_permutation() {
    let keys = [
        "nil",
        "false",
        "-1",
        r#""s""#,
        r#"b"\\x01""#,
        "(quote :sym)",
        "(quote (a b))",
        "[1]",
        "{}",
    ];
    let build = |order: &[usize]| {
        order.iter().copied().fold("{}".to_string(), |map, index| {
            format!("(prim map/put {map} {} {index})", keys[index])
        })
    };
    let ascending_order: Vec<_> = (0..keys.len()).collect();
    let descending_order: Vec<_> = (0..keys.len()).rev().collect();
    let ascending = parse_module(&build(&ascending_order)).unwrap();
    let descending = parse_module(&build(&descending_order)).unwrap();

    let evaluate = |forms: &[Term], compiled: bool| {
        let mut ctx = EvalCtx::new();
        let mut env = Env::empty();
        if compiled {
            eval_module_compiled(&mut ctx, &mut env, forms).unwrap()
        } else {
            eval_module(&mut ctx, &mut env, forms).unwrap()
        }
    };
    let asc_tree = evaluate(&ascending, false);
    let desc_tree = evaluate(&descending, false);
    let asc_compiled = evaluate(&ascending, true);
    let desc_compiled = evaluate(&descending, true);

    assert_eq!(asc_tree.debug_repr(), desc_tree.debug_repr());
    let expected_hash = value_hash(&asc_tree);
    assert_eq!(value_hash(&desc_tree), expected_hash);
    assert_eq!(value_hash(&asc_compiled), expected_hash);
    assert_eq!(value_hash(&desc_compiled), expected_hash);
}

#[test]
fn map_replacement_at_length_limit_succeeds_but_growth_fails() {
    let evaluate = |source: &str, compiled: bool| {
        let forms = parse_module(source).unwrap();
        let mut ctx = EvalCtx::new();
        ctx.set_mem_limits(MemLimits {
            max_map_len: Some(1),
            ..MemLimits::default()
        });
        let mut env = Env::empty();
        if compiled {
            eval_module_compiled(&mut ctx, &mut env, &forms)
        } else {
            eval_module(&mut ctx, &mut env, &forms)
        }
    };

    for compiled in [false, true] {
        let replaced = evaluate("(prim map/put {:a 1} (quote :a) 2)", compiled).unwrap();
        assert_eq!(replaced.debug_repr(), "{:a 2}");

        let error = evaluate("(prim map/put {:a 1} (quote :b) 2)", compiled).unwrap_err();
        assert!(
            matches!(error.kind, KernelErrorKind::MemoryLimit),
            "{error}"
        );
        assert!(error.msg.contains("map-len"), "{error}");
    }
}

#[test]
fn map_fast_growth_path_preserves_observed_length_counters() {
    let forms =
        parse_module("(prim map/put (prim map/put (prim map/put {} 1 1) 2 2) 3 3)").unwrap();
    for compiled in [false, true] {
        let mut ctx = EvalCtx::new();
        let mut env = Env::empty();
        let result = if compiled {
            eval_module_compiled(&mut ctx, &mut env, &forms).unwrap()
        } else {
            eval_module(&mut ctx, &mut env, &forms).unwrap()
        };
        assert_eq!(result.debug_repr(), "{1 1 2 2 3 3}");
        assert_eq!(ctx.observed_counters().mem.max_map_len, 3);
    }
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
