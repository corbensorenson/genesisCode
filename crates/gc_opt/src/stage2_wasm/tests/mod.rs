use gc_coreform::{canonicalize_module, parse_module};

mod string_bytes_collections;
mod tail_cases;
use super::{Stage2ValueKind, stage2_validation_report};

#[test]
fn stage2_validates_simple_int_module() {
    let src = r#"
          (def x (prim int/add 40 2))
          x
        "#;
    let forms = canonicalize_module(parse_module(src).unwrap()).unwrap();
    let r = stage2_validation_report(&forms);
    assert!(r.supported, "{r:?}");
    assert!(r.ok, "{r:?}");
    assert_eq!(r.value_kind, Some(Stage2ValueKind::Int));
    assert!(r.wasm_bytes_len.unwrap_or(0) > 0);
}

#[test]
fn stage2_validates_bool_comparison_module() {
    let src = r#"
          (prim int/lt? 1 2)
        "#;
    let forms = canonicalize_module(parse_module(src).unwrap()).unwrap();
    let r = stage2_validation_report(&forms);
    assert!(r.supported, "{r:?}");
    assert!(r.ok, "{r:?}");
    assert_eq!(r.value_kind, Some(Stage2ValueKind::Bool));
}

#[test]
fn stage2_validates_begin_expression() {
    let src = r#"
          (begin
            (prim int/add 1 2)
            (prim int/mul 7 6))
        "#;
    let forms = canonicalize_module(parse_module(src).unwrap()).unwrap();
    let r = stage2_validation_report(&forms);
    assert!(r.supported, "{r:?}");
    assert!(r.ok, "{r:?}");
    assert_eq!(r.value_kind, Some(Stage2ValueKind::Int));
}

#[test]
fn stage2_validates_let_expression() {
    let src = r#"
          (let ((x 10) (y (prim int/add x 5)))
            (if (prim int/lt? y 20)
              (prim int/mul y 2)
              (prim int/sub y 1)))
        "#;
    let forms = canonicalize_module(parse_module(src).unwrap()).unwrap();
    let r = stage2_validation_report(&forms);
    assert!(r.supported, "{r:?}");
    assert!(r.ok, "{r:?}");
    assert_eq!(r.value_kind, Some(Stage2ValueKind::Int));
}

#[test]
fn stage2_validates_if_truthiness_for_int_condition() {
    let src = r#"
          (if (prim int/sub 3 3)
            7
            9)
        "#;
    let forms = canonicalize_module(parse_module(src).unwrap()).unwrap();
    let r = stage2_validation_report(&forms);
    assert!(r.supported, "{r:?}");
    assert!(r.ok, "{r:?}");
    assert_eq!(r.value_kind, Some(Stage2ValueKind::Int));
}

#[test]
fn stage2_validates_if_truthiness_for_nil_condition() {
    let src = r#"
          (if nil
            7
            9)
        "#;
    let forms = canonicalize_module(parse_module(src).unwrap()).unwrap();
    let r = stage2_validation_report(&forms);
    assert!(r.supported, "{r:?}");
    assert!(r.ok, "{r:?}");
    assert_eq!(r.value_kind, Some(Stage2ValueKind::Int));
}

#[test]
fn stage2_validates_if_truthiness_for_symbol_condition() {
    let src = r#"
          (if (quote :feature/on)
            7
            9)
        "#;
    let forms = canonicalize_module(parse_module(src).unwrap()).unwrap();
    let r = stage2_validation_report(&forms);
    assert!(r.supported, "{r:?}");
    assert!(r.ok, "{r:?}");
    assert_eq!(r.value_kind, Some(Stage2ValueKind::Int));
}

#[test]
fn stage2_validates_if_truthiness_for_string_and_bytes_condition() {
    let src = r#"
          (if "x"
            (if b"\x01"
              7
              8)
            9)
        "#;
    let forms = canonicalize_module(parse_module(src).unwrap()).unwrap();
    let r = stage2_validation_report(&forms);
    assert!(r.supported, "{r:?}");
    assert!(r.ok, "{r:?}");
    assert_eq!(r.value_kind, Some(Stage2ValueKind::Int));
}

#[test]
fn stage2_validates_quote_scalar_literals() {
    let src = r#"
          (if (quote false)
            (quote 10)
            (quote 11))
        "#;
    let forms = canonicalize_module(parse_module(src).unwrap()).unwrap();
    let r = stage2_validation_report(&forms);
    assert!(r.supported, "{r:?}");
    assert!(r.ok, "{r:?}");
    assert_eq!(r.value_kind, Some(Stage2ValueKind::Int));
}

#[test]
fn stage2_validates_immediate_lambda_application() {
    let src = r#"
          ((fn (x) (prim int/add x 1)) 41)
        "#;
    let forms = canonicalize_module(parse_module(src).unwrap()).unwrap();
    let r = stage2_validation_report(&forms);
    assert!(r.supported, "{r:?}");
    assert!(r.ok, "{r:?}");
    assert_eq!(r.value_kind, Some(Stage2ValueKind::Int));
}

#[test]
fn stage2_validates_immediate_lambda_application_with_capture() {
    let src = r#"
          (def base 40)
          ((fn (x)
             (prim int/add base x))
           2)
        "#;
    let forms = canonicalize_module(parse_module(src).unwrap()).unwrap();
    let r = stage2_validation_report(&forms);
    assert!(r.supported, "{r:?}");
    assert!(r.ok, "{r:?}");
    assert_eq!(r.value_kind, Some(Stage2ValueKind::Int));
}

#[test]
fn stage2_validates_immediate_lambda_application_with_multi_body() {
    let src = r#"
          ((fn (x)
             (prim int/add x 1)
             (prim int/mul x 2))
           5)
        "#;
    let forms = canonicalize_module(parse_module(src).unwrap()).unwrap();
    let r = stage2_validation_report(&forms);
    assert!(r.supported, "{r:?}");
    assert!(r.ok, "{r:?}");
    assert_eq!(r.value_kind, Some(Stage2ValueKind::Int));
}

#[test]
fn stage2_validates_def_bound_function_call() {
    let src = r#"
          (def add1 (fn (x) (prim int/add x 1)))
          (add1 41)
        "#;
    let forms = canonicalize_module(parse_module(src).unwrap()).unwrap();
    let r = stage2_validation_report(&forms);
    assert!(r.supported, "{r:?}");
    assert!(r.ok, "{r:?}");
    assert_eq!(r.value_kind, Some(Stage2ValueKind::Int));
}

#[test]
fn stage2_validates_def_bound_function_call_with_lexical_capture() {
    let src = r#"
          (def base 1)
          (def f (fn (x) (prim int/add x base)))
          (def base 10)
          (f base)
        "#;
    let forms = canonicalize_module(parse_module(src).unwrap()).unwrap();
    let r = stage2_validation_report(&forms);
    assert!(r.supported, "{r:?}");
    assert!(r.ok, "{r:?}");
    assert_eq!(r.value_kind, Some(Stage2ValueKind::Int));
}

#[test]
fn stage2_validates_def_bound_function_call_ignores_let_shadow_for_global_free_var() {
    let src = r#"
          (def base 1)
          (def f (fn (x) (prim int/add x base)))
          (let ((base 100))
            (f 1))
        "#;
    let forms = canonicalize_module(parse_module(src).unwrap()).unwrap();
    let r = stage2_validation_report(&forms);
    assert!(r.supported, "{r:?}");
    assert!(r.ok, "{r:?}");
    assert_eq!(r.value_kind, Some(Stage2ValueKind::Int));
}

#[test]
fn stage2_validates_def_bound_curried_call_chain() {
    let src = r#"
          (def add (fn (a b) (prim int/add a b)))
          ((add 1) 2)
        "#;
    let forms = canonicalize_module(parse_module(src).unwrap()).unwrap();
    let r = stage2_validation_report(&forms);
    assert!(r.supported, "{r:?}");
    assert!(r.ok, "{r:?}");
    assert_eq!(r.value_kind, Some(Stage2ValueKind::Int));
}

#[test]
fn stage2_validates_immediate_lambda_curried_call_chain() {
    let src = r#"
          (((fn (a b) (prim int/add a b)) 1) 2)
        "#;
    let forms = canonicalize_module(parse_module(src).unwrap()).unwrap();
    let r = stage2_validation_report(&forms);
    assert!(r.supported, "{r:?}");
    assert!(r.ok, "{r:?}");
    assert_eq!(r.value_kind, Some(Stage2ValueKind::Int));
}

#[test]
fn stage2_validates_def_alias_to_builtin_function_chain() {
    let src = r#"
          (def add core/int::add)
          ((add 1) 2)
        "#;
    let forms = canonicalize_module(parse_module(src).unwrap()).unwrap();
    let r = stage2_validation_report(&forms);
    assert!(r.supported, "{r:?}");
    assert!(r.ok, "{r:?}");
    assert_eq!(r.value_kind, Some(Stage2ValueKind::Int));
}

#[test]
fn stage2_validates_def_alias_to_user_defined_function() {
    let src = r#"
          (def inc (fn (x) (prim int/add x 1)))
          (def f inc)
          (f 41)
        "#;
    let forms = canonicalize_module(parse_module(src).unwrap()).unwrap();
    let r = stage2_validation_report(&forms);
    assert!(r.supported, "{r:?}");
    assert!(r.ok, "{r:?}");
    assert_eq!(r.value_kind, Some(Stage2ValueKind::Int));
}

#[test]
fn stage2_validates_let_bound_function_call() {
    let src = r#"
          (let ((f (fn (x) (prim int/add x 1))))
            (f 41))
        "#;
    let forms = canonicalize_module(parse_module(src).unwrap()).unwrap();
    let r = stage2_validation_report(&forms);
    assert!(r.supported, "{r:?}");
    assert!(r.ok, "{r:?}");
    assert_eq!(r.value_kind, Some(Stage2ValueKind::Int));
}

#[test]
fn stage2_validates_let_bound_function_lexical_capture_before_shadow() {
    let src = r#"
          (let ((a 1)
                (f (fn (x) (prim int/add x a)))
                (a 10))
            (f 1))
        "#;
    let forms = canonicalize_module(parse_module(src).unwrap()).unwrap();
    let r = stage2_validation_report(&forms);
    assert!(r.supported, "{r:?}");
    assert!(r.ok, "{r:?}");
    assert_eq!(r.value_kind, Some(Stage2ValueKind::Int));
}

#[test]
fn stage2_validates_let_bound_function_alias_chain() {
    let src = r#"
          (let ((f (fn (x) (prim int/add x 1)))
                (g f))
            (g 41))
        "#;
    let forms = canonicalize_module(parse_module(src).unwrap()).unwrap();
    let r = stage2_validation_report(&forms);
    assert!(r.supported, "{r:?}");
    assert!(r.ok, "{r:?}");
    assert_eq!(r.value_kind, Some(Stage2ValueKind::Int));
}

#[test]
fn stage2_rejects_recursive_def_bound_function_call() {
    let src = r#"
          (def f (fn (x) (f x)))
          (f 1)
        "#;
    let forms = canonicalize_module(parse_module(src).unwrap()).unwrap();
    let r = stage2_validation_report(&forms);
    assert!(!r.supported, "{r:?}");
    assert!(!r.ok, "{r:?}");
    assert!(
        r.errors
            .iter()
            .any(|e| e.contains("recursive function call is unsupported in stage2")),
        "{r:?}"
    );
}

#[test]
fn stage2_validates_curried_core_int_wrapper_calls() {
    let src = r#"
          (def x ((core/int::add 40) 2))
          (def y ((core/int::mul x) 3))
          ((core/int::sub y) 6)
        "#;
    let forms = canonicalize_module(parse_module(src).unwrap()).unwrap();
    let r = stage2_validation_report(&forms);
    assert!(r.supported, "{r:?}");
    assert!(r.ok, "{r:?}");
    assert_eq!(r.value_kind, Some(Stage2ValueKind::Int));
}

#[test]
fn stage2_validates_curried_core_int_predicate_calls() {
    let src = r#"
          (def x ((core/int::add 1) 2))
          ((core/int::lt? x) 10)
        "#;
    let forms = canonicalize_module(parse_module(src).unwrap()).unwrap();
    let r = stage2_validation_report(&forms);
    assert!(r.supported, "{r:?}");
    assert!(r.ok, "{r:?}");
    assert_eq!(r.value_kind, Some(Stage2ValueKind::Bool));
}

#[test]
fn stage2_validates_core_eq_prim_for_ints_and_bools() {
    let src = r#"
          (def a (prim core/eq? (prim int/add 1 2) 3))
          (def b (prim core/eq? a true))
          b
        "#;
    let forms = canonicalize_module(parse_module(src).unwrap()).unwrap();
    let r = stage2_validation_report(&forms);
    assert!(r.supported, "{r:?}");
    assert!(r.ok, "{r:?}");
    assert_eq!(r.value_kind, Some(Stage2ValueKind::Bool));
}

#[test]
fn stage2_validates_curried_core_eq_wrapper_calls() {
    let src = r#"
          (def x ((core/int::add 1) 1))
          ((core/eq? x) 2)
        "#;
    let forms = canonicalize_module(parse_module(src).unwrap()).unwrap();
    let r = stage2_validation_report(&forms);
    assert!(r.supported, "{r:?}");
    assert!(r.ok, "{r:?}");
    assert_eq!(r.value_kind, Some(Stage2ValueKind::Bool));
}

#[test]
fn stage2_validates_curried_core_eq_wrapper_calls_for_bool_and_nil() {
    let src = r#"
          (def a ((core/eq? true) true))
          (if a
            ((core/eq? nil) nil)
            false)
        "#;
    let forms = canonicalize_module(parse_module(src).unwrap()).unwrap();
    let r = stage2_validation_report(&forms);
    assert!(r.supported, "{r:?}");
    assert!(r.ok, "{r:?}");
    assert_eq!(r.value_kind, Some(Stage2ValueKind::Bool));
}

#[test]
fn stage2_validates_core_eq_mixed_scalar_types_as_false() {
    let src = r#"
          (def a (prim core/eq? 1 true))
          (def b (prim core/eq? nil false))
          (if a
            1
            (if b 2 3))
        "#;
    let forms = canonicalize_module(parse_module(src).unwrap()).unwrap();
    let r = stage2_validation_report(&forms);
    assert!(r.supported, "{r:?}");
    assert!(r.ok, "{r:?}");
    assert_eq!(r.value_kind, Some(Stage2ValueKind::Int));
}

#[test]
fn stage2_validates_curried_core_eq_wrapper_call_for_mixed_scalar_types() {
    let src = r#"
          ((core/eq? (prim int/add 1 1)) true)
        "#;
    let forms = canonicalize_module(parse_module(src).unwrap()).unwrap();
    let r = stage2_validation_report(&forms);
    assert!(r.supported, "{r:?}");
    assert!(r.ok, "{r:?}");
    assert_eq!(r.value_kind, Some(Stage2ValueKind::Bool));
}

#[test]
fn stage2_validates_list_is_nil_prim_for_nil_and_non_nil_scalars() {
    let src = r#"
          (def a (prim list/is-nil? nil))
          (def b (prim list/is-nil? false))
          (if a
            (if b 0 1)
            2)
        "#;
    let forms = canonicalize_module(parse_module(src).unwrap()).unwrap();
    let r = stage2_validation_report(&forms);
    assert!(r.supported, "{r:?}");
    assert!(r.ok, "{r:?}");
    assert_eq!(r.value_kind, Some(Stage2ValueKind::Int));
}

#[test]
fn stage2_validates_core_list_is_nil_wrapper_call() {
    let src = r#"
          (core/list::is-nil? nil)
        "#;
    let forms = canonicalize_module(parse_module(src).unwrap()).unwrap();
    let r = stage2_validation_report(&forms);
    assert!(r.supported, "{r:?}");
    assert!(r.ok, "{r:?}");
    assert_eq!(r.value_kind, Some(Stage2ValueKind::Bool));
}

#[test]
fn stage2_validates_quote_symbol_via_core_eq() {
    let src = r#"
          (prim core/eq? (quote :k) (quote :k))
        "#;
    let forms = canonicalize_module(parse_module(src).unwrap()).unwrap();
    let r = stage2_validation_report(&forms);
    assert!(r.supported, "{r:?}");
    assert!(r.ok, "{r:?}");
    assert_eq!(r.value_kind, Some(Stage2ValueKind::Bool));
}

#[test]
fn stage2_validates_quote_string_and_bytes_literals() {
    let src = r#"
          (if (prim core/eq? (quote "alpha") "alpha")
            (prim core/eq? (quote b"\xAA\xBB") b"\xAA\xBB")
            false)
        "#;
    let forms = canonicalize_module(parse_module(src).unwrap()).unwrap();
    let r = stage2_validation_report(&forms);
    assert!(r.supported, "{r:?}");
    assert!(r.ok, "{r:?}");
    assert_eq!(r.value_kind, Some(Stage2ValueKind::Bool));
}

#[test]
fn stage2_validates_str_concat_and_len_prims_on_literals() {
    let src = r#"
          (def s (prim str/concat "hello, " "world"))
          (if (prim core/eq? s "hello, world")
            (prim int/eq? (prim str/len "hello, world") 12)
            false)
        "#;
    let forms = canonicalize_module(parse_module(src).unwrap()).unwrap();
    let r = stage2_validation_report(&forms);
    assert!(r.supported, "{r:?}");
    assert!(r.ok, "{r:?}");
    assert_eq!(r.value_kind, Some(Stage2ValueKind::Bool));
}

#[test]
fn stage2_validates_bytes_concat_and_len_prims_on_literals() {
    let src = r#"
          (def b (prim bytes/concat b"\x01\x02" b"\x03"))
          (if (prim core/eq? b b"\x01\x02\x03")
            (prim int/eq? (prim bytes/len b"\x01\x02\x03") 3)
            false)
        "#;
    let forms = canonicalize_module(parse_module(src).unwrap()).unwrap();
    let r = stage2_validation_report(&forms);
    assert!(r.supported, "{r:?}");
    assert!(r.ok, "{r:?}");
    assert_eq!(r.value_kind, Some(Stage2ValueKind::Bool));
}

#[test]
fn stage2_validates_str_and_bytes_wrapper_calls_on_literals() {
    let src = r#"
          (def s ((core/str::concat "a") "b"))
          (def b ((core/bytes::concat b"\xAA") b"\xBB"))
          (if (prim core/eq? s "ab")
            (if (prim core/eq? b b"\xAA\xBB")
              (if (prim int/eq? (core/str::len "abc") 3)
                (prim int/eq? (core/bytes::len b"\x10\x20\x30") 3)
                false)
              false)
            false)
        "#;
    let forms = canonicalize_module(parse_module(src).unwrap()).unwrap();
    let r = stage2_validation_report(&forms);
    assert!(r.supported, "{r:?}");
    assert!(r.ok, "{r:?}");
    assert_eq!(r.value_kind, Some(Stage2ValueKind::Bool));
}
