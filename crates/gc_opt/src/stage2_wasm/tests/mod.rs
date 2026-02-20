use gc_coreform::{canonicalize_module, parse_module};

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

#[test]
fn stage2_validates_len_wrappers_on_def_bound_constant_values() {
    let src = r#"
          (def s ((core/str::concat "ab") "c"))
          (def b ((core/bytes::concat b"\x01") b"\x02\x03"))
          (if (prim int/eq? (core/str::len s) 3)
            (prim int/eq? (core/bytes::len b) 3)
            false)
        "#;
    let forms = canonicalize_module(parse_module(src).unwrap()).unwrap();
    let r = stage2_validation_report(&forms);
    assert!(r.supported, "{r:?}");
    assert!(r.ok, "{r:?}");
    assert_eq!(r.value_kind, Some(Stage2ValueKind::Bool));
}

#[test]
fn stage2_validates_len_wrappers_on_let_bound_constant_values() {
    let src = r#"
          (let ((s ((core/str::concat "hel") "lo"))
                (b ((core/bytes::concat b"\xAA") b"\xBB")))
            (if (prim int/eq? (core/str::len s) 5)
              (prim int/eq? (core/bytes::len b) 2)
              false))
        "#;
    let forms = canonicalize_module(parse_module(src).unwrap()).unwrap();
    let r = stage2_validation_report(&forms);
    assert!(r.supported, "{r:?}");
    assert!(r.ok, "{r:?}");
    assert_eq!(r.value_kind, Some(Stage2ValueKind::Bool));
}

#[test]
fn stage2_validates_concat_wrappers_on_bound_constant_values() {
    let src = r#"
          (def a "hello")
          (def b ", world")
          (def x b"\x01")
          (def y b"\x02\x03")
          (def s ((core/str::concat a) b))
          (def bs ((core/bytes::concat x) y))
          (if (prim core/eq? s "hello, world")
            (prim core/eq? bs b"\x01\x02\x03")
            false)
        "#;
    let forms = canonicalize_module(parse_module(src).unwrap()).unwrap();
    let r = stage2_validation_report(&forms);
    assert!(r.supported, "{r:?}");
    assert!(r.ok, "{r:?}");
    assert_eq!(r.value_kind, Some(Stage2ValueKind::Bool));
}

#[test]
fn stage2_validates_len_wrappers_on_if_stable_constant_values() {
    let src = r#"
          (def s (if true "abc" "abc"))
          (def b (if true b"\x10\x20" b"\x10\x20"))
          (if (prim int/eq? (core/str::len s) 3)
            (prim int/eq? (core/bytes::len b) 2)
            false)
        "#;
    let forms = canonicalize_module(parse_module(src).unwrap()).unwrap();
    let r = stage2_validation_report(&forms);
    assert!(r.supported, "{r:?}");
    assert!(r.ok, "{r:?}");
    assert_eq!(r.value_kind, Some(Stage2ValueKind::Bool));
}

#[test]
fn stage2_validates_len_prims_on_if_variant_constant_values() {
    let src = r#"
          (def cond (prim int/lt? 0 1))
          (if (prim int/eq? (prim str/len (if cond "abc" "abcd")) 3)
            (prim int/eq? (prim bytes/len (if cond b"\x10\x20" b"\x10\x20\x30")) 2)
            false)
        "#;
    let forms = canonicalize_module(parse_module(src).unwrap()).unwrap();
    let r = stage2_validation_report(&forms);
    assert!(r.supported, "{r:?}");
    assert!(r.ok, "{r:?}");
    assert_eq!(r.value_kind, Some(Stage2ValueKind::Bool));
}

#[test]
fn stage2_validates_len_wrappers_on_if_variant_constant_values() {
    let src = r#"
          (def cond (prim int/lt? 0 1))
          (if (prim int/eq? (core/str::len (if cond "abc" "abcd")) 3)
            (prim int/eq? (core/bytes::len (if cond b"\x10\x20" b"\x10\x20\x30")) 2)
            false)
        "#;
    let forms = canonicalize_module(parse_module(src).unwrap()).unwrap();
    let r = stage2_validation_report(&forms);
    assert!(r.supported, "{r:?}");
    assert!(r.ok, "{r:?}");
    assert_eq!(r.value_kind, Some(Stage2ValueKind::Bool));
}

#[test]
fn stage2_validates_len_wrappers_on_nested_let_if_variant_values() {
    let src = r#"
          (def cond (prim int/lt? 0 1))
          (if (prim int/eq?
                (core/str::len
                  (let ((x 1))
                    (if cond "abc" "abcd")))
                3)
            (prim int/eq?
              (core/bytes::len
                (let ((x 1))
                  (if cond b"\x10\x20" b"\x10\x20\x30")))
              2)
            false)
        "#;
    let forms = canonicalize_module(parse_module(src).unwrap()).unwrap();
    let r = stage2_validation_report(&forms);
    assert!(r.supported, "{r:?}");
    assert!(r.ok, "{r:?}");
    assert_eq!(r.value_kind, Some(Stage2ValueKind::Bool));
}

#[test]
fn stage2_validates_int_to_str_prim_on_literals() {
    let src = r#"
          (if (prim core/eq? (prim int/to-str 42) "42")
            (prim core/eq? (prim int/to-str -7) "-7")
            false)
        "#;
    let forms = canonicalize_module(parse_module(src).unwrap()).unwrap();
    let r = stage2_validation_report(&forms);
    assert!(r.supported, "{r:?}");
    assert!(r.ok, "{r:?}");
    assert_eq!(r.value_kind, Some(Stage2ValueKind::Bool));
}

#[test]
fn stage2_validates_int_to_str_wrapper_on_bound_constant_values() {
    let src = r#"
          (def n (prim int/add 40 2))
          (let ((m (prim int/sub n 10)))
            (if (prim core/eq? (core/int::to-str n) "42")
              (prim core/eq? (core/int::to-str m) "32")
              false))
        "#;
    let forms = canonicalize_module(parse_module(src).unwrap()).unwrap();
    let r = stage2_validation_report(&forms);
    assert!(r.supported, "{r:?}");
    assert!(r.ok, "{r:?}");
    assert_eq!(r.value_kind, Some(Stage2ValueKind::Bool));
}

#[test]
fn stage2_validates_int_to_str_wrapper_on_if_variant_values() {
    let src = r#"
          (def cond (prim int/lt? 0 1))
          (if (prim core/eq?
                (core/int::to-str
                  (let ((x 1))
                    (if cond 42 420)))
                "42")
            (prim core/eq? (core/int::to-str (if cond -7 -70)) "-7")
            false)
        "#;
    let forms = canonicalize_module(parse_module(src).unwrap()).unwrap();
    let r = stage2_validation_report(&forms);
    assert!(r.supported, "{r:?}");
    assert!(r.ok, "{r:?}");
    assert_eq!(r.value_kind, Some(Stage2ValueKind::Bool));
}

#[test]
fn stage2_validates_str_repeat_prim_on_literals() {
    let src = r#"
          (if (prim core/eq? (prim str/repeat "ab" 3) "ababab")
            (prim core/eq? (prim str/repeat "z" 0) "")
            false)
        "#;
    let forms = canonicalize_module(parse_module(src).unwrap()).unwrap();
    let r = stage2_validation_report(&forms);
    assert!(r.supported, "{r:?}");
    assert!(r.ok, "{r:?}");
    assert_eq!(r.value_kind, Some(Stage2ValueKind::Bool));
}

#[test]
fn stage2_validates_str_repeat_wrapper_on_bound_constant_values() {
    let src = r#"
          (def s ((core/str::repeat "ab") 3))
          (def n (prim int/add 1 1))
          (if (prim core/eq? s "ababab")
            (prim core/eq? ((core/str::repeat "z") n) "zz")
            false)
        "#;
    let forms = canonicalize_module(parse_module(src).unwrap()).unwrap();
    let r = stage2_validation_report(&forms);
    assert!(r.supported, "{r:?}");
    assert!(r.ok, "{r:?}");
    assert_eq!(r.value_kind, Some(Stage2ValueKind::Bool));
}

#[test]
fn stage2_validates_str_repeat_wrapper_on_if_variant_values() {
    let src = r#"
          (def cond (prim int/lt? 0 1))
          (if (prim core/eq?
                ((core/str::repeat
                   (let ((x 1))
                     (if cond "ab" "abc")))
                 (if cond 2 3))
                "abab")
            (prim core/eq?
              ((core/str::repeat "z")
               (if cond 0 1))
              "")
            false)
        "#;
    let forms = canonicalize_module(parse_module(src).unwrap()).unwrap();
    let r = stage2_validation_report(&forms);
    assert!(r.supported, "{r:?}");
    assert!(r.ok, "{r:?}");
    assert_eq!(r.value_kind, Some(Stage2ValueKind::Bool));
}

#[test]
fn stage2_validates_str_join_prim_on_literal_vectors() {
    let src = r#"
          (if (prim core/eq? (prim str/join ["a" "b" "c"] ",") "a,b,c")
            (prim core/eq? (prim str/join [] ",") "")
            false)
        "#;
    let forms = canonicalize_module(parse_module(src).unwrap()).unwrap();
    let r = stage2_validation_report(&forms);
    assert!(r.supported, "{r:?}");
    assert!(r.ok, "{r:?}");
    assert_eq!(r.value_kind, Some(Stage2ValueKind::Bool));
}

#[test]
fn stage2_validates_str_join_wrapper_on_if_variant_vectors() {
    let src = r#"
          (def cond (prim int/lt? 0 1))
          (if (prim core/eq?
                ((core/str::join
                   (let ((x 1))
                     (if cond ["ab" "cd"] ["x" "y"])))
                 (if cond "-" ":"))
                "ab-cd")
            (prim core/eq?
              ((core/str::join
                 (if cond [] ["q"]))
               ",")
              "")
            false)
        "#;
    let forms = canonicalize_module(parse_module(src).unwrap()).unwrap();
    let r = stage2_validation_report(&forms);
    assert!(r.supported, "{r:?}");
    assert!(r.ok, "{r:?}");
    assert_eq!(r.value_kind, Some(Stage2ValueKind::Bool));
}

#[test]
fn stage2_validates_bytes_join_prim_on_literal_vectors() {
    let src = r#"
          (if (prim core/eq? (prim bytes/join [b"\x01\x02" b"\xFF"]) b"\x01\x02\xFF")
            (prim core/eq? (prim bytes/join []) b"")
            false)
        "#;
    let forms = canonicalize_module(parse_module(src).unwrap()).unwrap();
    let r = stage2_validation_report(&forms);
    assert!(r.supported, "{r:?}");
    assert!(r.ok, "{r:?}");
    assert_eq!(r.value_kind, Some(Stage2ValueKind::Bool));
}

#[test]
fn stage2_validates_bytes_join_wrapper_on_if_variant_vectors() {
    let src = r#"
          (def cond (prim int/lt? 0 1))
          (if (prim core/eq?
                (core/bytes::join
                  (let ((x 1))
                    (if cond [b"\xAA" b"\xBB"] [b"\xCC"])))
                b"\xAA\xBB")
            (prim core/eq?
              (core/bytes::join
                (if cond [] [b"\x01"]))
              b"")
            false)
        "#;
    let forms = canonicalize_module(parse_module(src).unwrap()).unwrap();
    let r = stage2_validation_report(&forms);
    assert!(r.supported, "{r:?}");
    assert!(r.ok, "{r:?}");
    assert_eq!(r.value_kind, Some(Stage2ValueKind::Bool));
}

#[test]
fn stage2_validates_vec_len_prim_on_literal_vectors() {
    let src = r#"
          (if (prim int/eq? (prim vec/len [10 20 30]) 3)
            (prim int/eq? (prim vec/len []) 0)
            false)
        "#;
    let forms = canonicalize_module(parse_module(src).unwrap()).unwrap();
    let r = stage2_validation_report(&forms);
    assert!(r.supported, "{r:?}");
    assert!(r.ok, "{r:?}");
    assert_eq!(r.value_kind, Some(Stage2ValueKind::Bool));
}

#[test]
fn stage2_validates_vec_len_wrapper_on_if_variant_vectors() {
    let src = r#"
          (def cond (prim int/lt? 0 1))
          (if (prim int/eq?
                (core/vec::len
                  (if cond [1 2 3] [4]))
                3)
            (prim int/eq?
              (core/vec::len
                (let ((x 1))
                  (if cond [] [0])))
              0)
            false)
        "#;
    let forms = canonicalize_module(parse_module(src).unwrap()).unwrap();
    let r = stage2_validation_report(&forms);
    assert!(r.supported, "{r:?}");
    assert!(r.ok, "{r:?}");
    assert_eq!(r.value_kind, Some(Stage2ValueKind::Bool));
}

#[test]
fn stage2_validates_vec_len_on_let_bound_vector_alias() {
    let src = r#"
          (if (prim int/eq?
                (core/vec::len
                  (let ((v [1 2 3 4]))
                    v))
                4)
            (prim int/eq?
              (prim vec/len
                (let ((v (prim vec/push [8] 9)))
                  v))
              2)
            false)
        "#;
    let forms = canonicalize_module(parse_module(src).unwrap()).unwrap();
    let r = stage2_validation_report(&forms);
    assert!(r.supported, "{r:?}");
    assert!(r.ok, "{r:?}");
    assert_eq!(r.value_kind, Some(Stage2ValueKind::Bool));
}

#[test]
fn stage2_validates_map_len_prim_on_literal_maps() {
    let src = r#"
          (if (prim int/eq? (prim map/len {:a 1 :b 2}) 2)
            (prim int/eq? (prim map/len {}) 0)
            false)
        "#;
    let forms = canonicalize_module(parse_module(src).unwrap()).unwrap();
    let r = stage2_validation_report(&forms);
    assert!(r.supported, "{r:?}");
    assert!(r.ok, "{r:?}");
    assert_eq!(r.value_kind, Some(Stage2ValueKind::Bool));
}

#[test]
fn stage2_validates_map_len_wrapper_on_if_variant_maps() {
    let src = r#"
          (def cond (prim int/lt? 0 1))
          (if (prim int/eq?
                (core/map::len
                  (if cond {:a 1 :b 2} {:z 9}))
                2)
            (prim int/eq?
              (core/map::len
                (let ((x 1))
                  (if cond {} {:k 1})))
              0)
            false)
        "#;
    let forms = canonicalize_module(parse_module(src).unwrap()).unwrap();
    let r = stage2_validation_report(&forms);
    assert!(r.supported, "{r:?}");
    assert!(r.ok, "{r:?}");
    assert_eq!(r.value_kind, Some(Stage2ValueKind::Bool));
}

#[test]
fn stage2_validates_map_get_prim_on_literal_maps() {
    let src = r#"
          (if (prim int/eq? (prim map/get {:a 1 :b 2} (quote :a)) 1)
            (prim list/is-nil? (prim map/get {:a 1} (quote :z)))
            false)
        "#;
    let forms = canonicalize_module(parse_module(src).unwrap()).unwrap();
    let r = stage2_validation_report(&forms);
    assert!(r.supported, "{r:?}");
    assert!(r.ok, "{r:?}");
    assert_eq!(r.value_kind, Some(Stage2ValueKind::Bool));
}

#[test]
fn stage2_validates_map_get_wrapper_on_if_variant_maps_and_keys() {
    let src = r#"
          (def cond (prim int/lt? 0 1))
          (if (prim int/eq?
                ((core/map::get
                   (if cond {:a 7 :b 8} {:a 1 :b 2}))
                 (if cond (quote :a) (quote :b)))
                7)
            (prim list/is-nil?
              ((core/map::get
                 (let ((x 1))
                   (if cond {:k 1} {:m 2})))
               (if cond (quote :z) (quote :y))))
            false)
        "#;
    let forms = canonicalize_module(parse_module(src).unwrap()).unwrap();
    let r = stage2_validation_report(&forms);
    assert!(r.supported, "{r:?}");
    assert!(r.ok, "{r:?}");
    assert_eq!(r.value_kind, Some(Stage2ValueKind::Bool));
}

#[test]
fn stage2_validates_map_get_len_on_put_merge_constant_forms() {
    let src = r#"
          (if (prim int/eq?
                (prim map/get
                  (prim map/put {:a 1} (quote :b) 2)
                  (quote :b))
                2)
            (if (prim int/eq?
                  (prim map/len
                    (prim map/merge {:a 1} {:b 2 :c 3}))
                  3)
              (prim int/eq?
                (prim map/get
                  (((core/map::put {:x 1}) (quote :y)) 9)
                  (quote :y))
                9)
              false)
            false)
        "#;
    let forms = canonicalize_module(parse_module(src).unwrap()).unwrap();
    let r = stage2_validation_report(&forms);
    assert!(r.supported, "{r:?}");
    assert!(r.ok, "{r:?}");
    assert_eq!(r.value_kind, Some(Stage2ValueKind::Bool));
}

#[test]
fn stage2_validates_collection_constant_composition_on_alias_sources() {
    let src = r#"
          (def v0 [1 2])
          (def v1 (prim vec/push v0 3))
          (def m0 {:a 1})
          (def m1 (prim map/put m0 (quote :b) 2))
          (def m2 (prim map/merge m1 {:c 3}))
          (if (prim int/eq? (prim vec/get v1 2) 3)
            (if (prim int/eq? (prim map/get m2 (quote :b)) 2)
              (prim int/eq? (core/map::len m2) 3)
              false)
            false)
        "#;
    let forms = canonicalize_module(parse_module(src).unwrap()).unwrap();
    let r = stage2_validation_report(&forms);
    assert!(r.supported, "{r:?}");
    assert!(r.ok, "{r:?}");
    assert_eq!(r.value_kind, Some(Stage2ValueKind::Bool));
}

#[test]
fn stage2_validates_map_get_len_on_let_bound_map_aliases() {
    let src = r#"
          (def cond (prim int/lt? 0 1))
          (if (prim int/eq?
                (prim map/get
                  (let ((m1 {:a 1 :b 2})
                        (m2 {:a 10 :b 20}))
                    (if cond m1 m2))
                  (quote :b))
                2)
            (if (prim int/eq?
                  (core/map::len
                    (let ((m1 (prim map/put {} (quote :x) 9))
                          (m2 (prim map/merge {:a 1} {:b 2})))
                      (if cond m1 m2)))
                  1)
              (prim list/is-nil?
                (prim map/get
                  (let ((m1 (prim map/merge {:a 1} {:b 2}))
                        (m2 {:y 0}))
                    (if cond m1 m2))
                  (quote :z)))
              false)
            false)
        "#;
    let forms = canonicalize_module(parse_module(src).unwrap()).unwrap();
    let r = stage2_validation_report(&forms);
    assert!(r.supported, "{r:?}");
    assert!(r.ok, "{r:?}");
    assert_eq!(r.value_kind, Some(Stage2ValueKind::Bool));
}

#[test]
fn stage2_validates_collection_ops_on_def_bound_aliases() {
    let src = r#"
          (def v [1 2 3])
          (def m {:a 7 :b 8})
          (def parts ["a" "b"])
          (def bytes-parts [b"\x01" b"\x02"])
          (if (prim int/eq? (prim vec/get v 1) 2)
            (if (prim int/eq? (core/vec::len v) 3)
              (if (prim int/eq? ((core/map::get m) (quote :a)) 7)
                (if (prim int/eq? (core/map::len m) 2)
                  (if (prim core/eq? (core/str::join parts "-") "a-b")
                    (prim core/eq? (core/bytes::join bytes-parts) b"\x01\x02")
                    false)
                  false)
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

#[test]
fn stage2_validates_collection_ops_on_def_bound_alias_chains() {
    let src = r#"
          (def v1 [1 2 3])
          (def v2 v1)
          (def v3 v2)
          (def m1 {:a 7 :b 8})
          (def m2 m1)
          (def m3 m2)
          (def parts1 ["a" "b"])
          (def parts2 parts1)
          (def parts3 parts2)
          (def bytes1 [b"\x01" b"\x02"])
          (def bytes2 bytes1)
          (def bytes3 bytes2)
          (if (prim int/eq? (prim vec/get v3 1) 2)
            (if (prim int/eq? (core/vec::len v3) 3)
              (if (prim int/eq? ((core/map::get m3) (quote :a)) 7)
                (if (prim int/eq? (core/map::len m3) 2)
                  (if (prim core/eq? (core/str::join parts3 "-") "a-b")
                    (prim core/eq? (core/bytes::join bytes3) b"\x01\x02")
                    false)
                  false)
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

#[test]
fn stage2_validates_collection_ops_on_let_bound_alias_chains() {
    let src = r#"
          (if (prim int/eq?
                (prim vec/get
                  (let ((v1 [1 2 3])
                        (v2 v1)
                        (v3 v2))
                    v3)
                  2)
                3)
            (if (prim int/eq?
                  (prim map/get
                    (let ((m1 {:a 7 :b 8})
                          (m2 m1)
                          (m3 m2))
                      m3)
                    (quote :b))
                  8)
              (if (prim core/eq?
                    (prim str/join
                      (let ((s1 ["a" "b"])
                            (s2 s1)
                            (s3 s2))
                        s3)
                      "-")
                    "a-b")
                (prim core/eq?
                  (core/bytes::join
                    (let ((b1 [b"\x01" b"\x02"])
                          (b2 b1)
                          (b3 b2))
                      b3))
                  b"\x01\x02")
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

#[test]
fn stage2_validates_generic_let_collection_alias_flow() {
    let src = r#"
          (let ((v [1 2 3])
                (m {:a 7 :b 8})
                (parts ["a" "b"])
                (bparts [b"\x01" b"\x02"]))
            (if (prim int/eq? (prim vec/get v 1) 2)
              (if (prim int/eq? (prim map/get m (quote :b)) 8)
                (if (prim core/eq? (prim str/join parts "-") "a-b")
                  (prim core/eq? (core/bytes::join bparts) b"\x01\x02")
                  false)
                false)
              false))
        "#;
    let forms = canonicalize_module(parse_module(src).unwrap()).unwrap();
    let r = stage2_validation_report(&forms);
    assert!(r.supported, "{r:?}");
    assert!(r.ok, "{r:?}");
    assert_eq!(r.value_kind, Some(Stage2ValueKind::Bool));
}

#[test]
fn stage2_validates_defs_only_module_with_data_literal_rhs() {
    let src = r#"
          (def v [1 2 3])
          (def m {:a 1 :b 2})
          (def p '(x y))
        "#;
    let forms = canonicalize_module(parse_module(src).unwrap()).unwrap();
    let r = stage2_validation_report(&forms);
    assert!(r.supported, "{r:?}");
    assert!(r.ok, "{r:?}");
    assert_eq!(r.value_kind, Some(Stage2ValueKind::Nil));
}

#[test]
fn stage2_validates_vec_get_len_on_push_constant_forms() {
    let src = r#"
          (def cond (prim int/lt? 0 1))
          (if (prim int/eq?
                (prim vec/get
                  (if cond
                    (prim vec/push [1 2] 3)
                    (prim vec/push [1 2] 4))
                  (if cond 2 1))
                3)
            (if (prim int/eq?
                  (core/vec::len
                    (if cond
                      ((core/vec::push [7]) 10)
                      ((core/vec::push [8 9]) 10)))
                  2)
              (prim list/is-nil?
                (prim vec/get
                  ((core/vec::push []) 5)
                  9))
              false)
            false)
        "#;
    let forms = canonicalize_module(parse_module(src).unwrap()).unwrap();
    let r = stage2_validation_report(&forms);
    assert!(r.supported, "{r:?}");
    assert!(r.ok, "{r:?}");
    assert_eq!(r.value_kind, Some(Stage2ValueKind::Bool));
}

#[test]
fn stage2_validates_join_on_vec_push_constant_forms() {
    let src = r#"
          (def cond (prim int/lt? 0 1))
          (if (prim core/eq?
                (prim str/join
                  (if cond
                    (prim vec/push ["a"] "b")
                    (prim vec/push ["x"] "b"))
                  (if cond "-" ":"))
                "a-b")
            (prim core/eq?
              (core/bytes::join
                (if cond
                  ((core/vec::push [b"\x01"]) b"\x02")
                  ((core/vec::push [b"\xAA"]) b"\x02")))
              b"\x01\x02")
            false)
        "#;
    let forms = canonicalize_module(parse_module(src).unwrap()).unwrap();
    let r = stage2_validation_report(&forms);
    assert!(r.supported, "{r:?}");
    assert!(r.ok, "{r:?}");
    assert_eq!(r.value_kind, Some(Stage2ValueKind::Bool));
}

#[test]
fn stage2_validates_join_on_let_bound_vector_aliases() {
    let src = r#"
          (if (prim core/eq?
                (prim str/join
                  (let ((parts ["a" "b"]))
                    parts)
                  "-")
                "a-b")
            (prim core/eq?
              (core/bytes::join
                (let ((parts (prim vec/push [b"\x01"] b"\x02")))
                  parts))
              b"\x01\x02")
            false)
        "#;
    let forms = canonicalize_module(parse_module(src).unwrap()).unwrap();
    let r = stage2_validation_report(&forms);
    assert!(r.supported, "{r:?}");
    assert!(r.ok, "{r:?}");
    assert_eq!(r.value_kind, Some(Stage2ValueKind::Bool));
}

#[test]
fn stage2_validates_vec_get_prim_on_literal_vectors() {
    let src = r#"
          (if (prim int/eq? (prim vec/get [10 20 30] 1) 20)
            (prim list/is-nil? (prim vec/get [10] 5))
            false)
        "#;
    let forms = canonicalize_module(parse_module(src).unwrap()).unwrap();
    let r = stage2_validation_report(&forms);
    assert!(r.supported, "{r:?}");
    assert!(r.ok, "{r:?}");
    assert_eq!(r.value_kind, Some(Stage2ValueKind::Bool));
}

#[test]
fn stage2_validates_vec_get_wrapper_on_if_variant_vectors_and_indices() {
    let src = r#"
          (def cond (prim int/lt? 0 1))
          (if (prim int/eq?
                ((core/vec::get
                   (if cond [7 8] [9 10]))
                 (if cond 0 1))
                7)
            (prim list/is-nil?
              ((core/vec::get
                 (let ((x 1))
                   (if cond [1] [2])))
               (if cond 5 7)))
            false)
        "#;
    let forms = canonicalize_module(parse_module(src).unwrap()).unwrap();
    let r = stage2_validation_report(&forms);
    assert!(r.supported, "{r:?}");
    assert!(r.ok, "{r:?}");
    assert_eq!(r.value_kind, Some(Stage2ValueKind::Bool));
}

#[test]
fn stage2_validates_vec_get_on_let_bound_vector_alias() {
    let src = r#"
          (if (prim int/eq?
                (prim vec/get
                  (let ((v [5 6 7]))
                    v)
                  1)
                6)
            (prim list/is-nil?
              (prim vec/get
                (let ((v (prim vec/push [] 9)))
                  v)
                5))
            false)
        "#;
    let forms = canonicalize_module(parse_module(src).unwrap()).unwrap();
    let r = stage2_validation_report(&forms);
    assert!(r.supported, "{r:?}");
    assert!(r.ok, "{r:?}");
    assert_eq!(r.value_kind, Some(Stage2ValueKind::Bool));
}

#[test]
fn stage2_validates_bytes_get_prim_on_literals() {
    let src = r#"
          (if (prim int/eq? (prim bytes/get b"\x00\x7f\xff" 2) 255)
            (prim int/eq? (prim bytes/get b"AZ" 0) 65)
            false)
        "#;
    let forms = canonicalize_module(parse_module(src).unwrap()).unwrap();
    let r = stage2_validation_report(&forms);
    assert!(r.supported, "{r:?}");
    assert!(r.ok, "{r:?}");
    assert_eq!(r.value_kind, Some(Stage2ValueKind::Bool));
}

#[test]
fn stage2_validates_bytes_get_wrapper_on_bound_constant_values() {
    let src = r#"
          (def bs b"\x10\x20\x30")
          (def i (prim int/add 1 1))
          (if (prim int/eq? ((core/bytes::get bs) i) 48)
            (prim int/eq? ((core/bytes::get bs) 0) 16)
            false)
        "#;
    let forms = canonicalize_module(parse_module(src).unwrap()).unwrap();
    let r = stage2_validation_report(&forms);
    assert!(r.supported, "{r:?}");
    assert!(r.ok, "{r:?}");
    assert_eq!(r.value_kind, Some(Stage2ValueKind::Bool));
}

#[test]
fn stage2_validates_bytes_get_wrapper_on_if_variant_values() {
    let src = r#"
          (def cond (prim int/lt? 0 1))
          (if (prim int/eq?
                ((core/bytes::get
                   (let ((x 1))
                     (if cond b"\x01\x02" b"\x03\x04")))
                 (if cond 1 0))
                2)
            (prim int/eq?
              ((core/bytes::get b"\x09\x08")
               (if cond 0 1))
              9)
            false)
        "#;
    let forms = canonicalize_module(parse_module(src).unwrap()).unwrap();
    let r = stage2_validation_report(&forms);
    assert!(r.supported, "{r:?}");
    assert!(r.ok, "{r:?}");
    assert_eq!(r.value_kind, Some(Stage2ValueKind::Bool));
}

#[test]
fn stage2_validates_coreform_escape_prims_on_literals() {
    let src = r#"
          (if (prim core/eq? (prim coreform/escape-str "a\n\t\"\\") "a\\n\\t\\\"\\\\")
            (prim core/eq? (prim coreform/escape-bytes b"\x00\xFF") "\\x00\\xFF")
            false)
        "#;
    let forms = canonicalize_module(parse_module(src).unwrap()).unwrap();
    let r = stage2_validation_report(&forms);
    assert!(r.supported, "{r:?}");
    assert!(r.ok, "{r:?}");
    assert_eq!(r.value_kind, Some(Stage2ValueKind::Bool));
}

#[test]
fn stage2_validates_coreform_escape_wrappers_on_bound_constant_values() {
    let src = r#"
          (def s (core/coreform::escape-str "x\n"))
          (def b (core/coreform::escape-bytes b"\n"))
          (if (prim core/eq? s "x\\n")
            (prim core/eq? b "\\n")
            false)
        "#;
    let forms = canonicalize_module(parse_module(src).unwrap()).unwrap();
    let r = stage2_validation_report(&forms);
    assert!(r.supported, "{r:?}");
    assert!(r.ok, "{r:?}");
    assert_eq!(r.value_kind, Some(Stage2ValueKind::Bool));
}

#[test]
fn stage2_validates_coreform_escape_wrappers_on_if_variant_values() {
    let src = r#"
          (def cond (prim int/lt? 0 1))
          (if (prim core/eq?
                (core/coreform::escape-str
                  (if cond "a\n" "b\t"))
                "a\\n")
            (prim core/eq?
              (core/coreform::escape-bytes
                (let ((x 1))
                  (if cond b"\x00" b"\xFF")))
              "\\x00")
            false)
        "#;
    let forms = canonicalize_module(parse_module(src).unwrap()).unwrap();
    let r = stage2_validation_report(&forms);
    assert!(r.supported, "{r:?}");
    assert!(r.ok, "{r:?}");
    assert_eq!(r.value_kind, Some(Stage2ValueKind::Bool));
}

#[test]
fn stage2_validates_sym_string_conversion_prims_on_literals() {
    let src = r#"
          (if (prim core/eq? (prim sym/to-str (quote :alpha/ns::k)) ":alpha/ns::k")
            (prim sym/eq? (prim sym/from-str ":alpha/ns::k") (quote :alpha/ns::k))
            false)
        "#;
    let forms = canonicalize_module(parse_module(src).unwrap()).unwrap();
    let r = stage2_validation_report(&forms);
    assert!(r.supported, "{r:?}");
    assert!(r.ok, "{r:?}");
    assert_eq!(r.value_kind, Some(Stage2ValueKind::Bool));
}

#[test]
fn stage2_validates_sym_string_wrapper_conversion_on_bound_constant_values() {
    let src = r#"
          (def s (core/sym::to-str (quote :alpha/ns::k)))
          (def k (core/sym::from-str s))
          (if ((core/sym::eq? k) (quote :alpha/ns::k))
            (prim core/eq? s ":alpha/ns::k")
            false)
        "#;
    let forms = canonicalize_module(parse_module(src).unwrap()).unwrap();
    let r = stage2_validation_report(&forms);
    assert!(r.supported, "{r:?}");
    assert!(r.ok, "{r:?}");
    assert_eq!(r.value_kind, Some(Stage2ValueKind::Bool));
}

#[test]
fn stage2_validates_sym_string_wrapper_conversion_on_if_variant_values() {
    let src = r#"
          (def cond (prim int/lt? 0 1))
          (if (prim core/eq?
                (core/sym::to-str
                  (let ((x 1))
                    (if cond (quote :alpha) (quote :beta))))
                ":alpha")
            ((core/sym::eq?
               (core/sym::from-str
                 (if cond ":alpha" ":beta")))
             (quote :alpha))
            false)
        "#;
    let forms = canonicalize_module(parse_module(src).unwrap()).unwrap();
    let r = stage2_validation_report(&forms);
    assert!(r.supported, "{r:?}");
    assert!(r.ok, "{r:?}");
    assert_eq!(r.value_kind, Some(Stage2ValueKind::Bool));
}

#[test]
fn stage2_validates_utf8_conversion_prims_on_literals() {
    let src = r#"
          (if (prim core/eq? (prim bytes/to-str-utf8 (prim str/to-bytes-utf8 "alpha")) "alpha")
            (prim core/eq? (prim str/to-bytes-utf8 (prim bytes/to-str-utf8 b"\xCE\xB1")) b"\xCE\xB1")
            false)
        "#;
    let forms = canonicalize_module(parse_module(src).unwrap()).unwrap();
    let r = stage2_validation_report(&forms);
    assert!(r.supported, "{r:?}");
    assert!(r.ok, "{r:?}");
    assert_eq!(r.value_kind, Some(Stage2ValueKind::Bool));
}

#[test]
fn stage2_validates_utf8_wrapper_conversion_on_bound_constant_values() {
    let src = r#"
          (def b (core/str::to-utf8 "hello"))
          (def s (core/str::from-utf8 b))
          (if (prim core/eq? s "hello")
            (prim core/eq? b b"hello")
            false)
        "#;
    let forms = canonicalize_module(parse_module(src).unwrap()).unwrap();
    let r = stage2_validation_report(&forms);
    assert!(r.supported, "{r:?}");
    assert!(r.ok, "{r:?}");
    assert_eq!(r.value_kind, Some(Stage2ValueKind::Bool));
}

#[test]
fn stage2_validates_utf8_wrapper_conversion_on_if_variant_values() {
    let src = r#"
          (def cond (prim int/lt? 0 1))
          (if (prim core/eq?
                (core/str::from-utf8
                  (let ((x 1))
                    (if cond b"alpha" b"beta")))
                "alpha")
            (prim core/eq?
              (core/str::to-utf8
                (if cond "alpha" "beta"))
              b"alpha")
            false)
        "#;
    let forms = canonicalize_module(parse_module(src).unwrap()).unwrap();
    let r = stage2_validation_report(&forms);
    assert!(r.supported, "{r:?}");
    assert!(r.ok, "{r:?}");
    assert_eq!(r.value_kind, Some(Stage2ValueKind::Bool));
}

#[test]
fn stage2_validates_hex_conversion_prims_on_literals() {
    let src = r#"
          (if (prim core/eq? (prim bytes/to-hex b"\x00\xff") "00ff")
            (prim core/eq? (prim bytes/from-hex "00ff") b"\x00\xff")
            false)
        "#;
    let forms = canonicalize_module(parse_module(src).unwrap()).unwrap();
    let r = stage2_validation_report(&forms);
    assert!(r.supported, "{r:?}");
    assert!(r.ok, "{r:?}");
    assert_eq!(r.value_kind, Some(Stage2ValueKind::Bool));
}

#[test]
fn stage2_validates_hex_wrapper_conversion_on_bound_constant_values() {
    let src = r#"
          (def hx (core/bytes::to-hex b"\xAA\xBB"))
          (def bs (core/bytes::from-hex hx))
          (if (prim core/eq? hx "aabb")
            (prim core/eq? bs b"\xAA\xBB")
            false)
        "#;
    let forms = canonicalize_module(parse_module(src).unwrap()).unwrap();
    let r = stage2_validation_report(&forms);
    assert!(r.supported, "{r:?}");
    assert!(r.ok, "{r:?}");
    assert_eq!(r.value_kind, Some(Stage2ValueKind::Bool));
}

#[test]
fn stage2_validates_hex_wrapper_conversion_on_if_variant_values() {
    let src = r#"
          (def cond (prim int/lt? 0 1))
          (if (prim core/eq?
                (core/bytes::to-hex
                  (let ((x 1))
                    (if cond b"\xAA\xBB" b"\xCC\xDD")))
                "aabb")
            (prim core/eq?
              (core/bytes::from-hex
                (if cond "aabb" "ccdd"))
              b"\xAA\xBB")
            false)
        "#;
    let forms = canonicalize_module(parse_module(src).unwrap()).unwrap();
    let r = stage2_validation_report(&forms);
    assert!(r.supported, "{r:?}");
    assert!(r.ok, "{r:?}");
    assert_eq!(r.value_kind, Some(Stage2ValueKind::Bool));
}

#[test]
fn stage2_validates_concat_prims_on_if_variant_constant_values() {
    let src = r#"
          (def cond (prim int/lt? 0 1))
          (if (prim core/eq? (prim str/concat (if cond "ab" "abc") "!") "ab!")
            (prim core/eq? (prim bytes/concat (if cond b"\x01" b"\x01\x02") b"\xFF") b"\x01\xFF")
            false)
        "#;
    let forms = canonicalize_module(parse_module(src).unwrap()).unwrap();
    let r = stage2_validation_report(&forms);
    assert!(r.supported, "{r:?}");
    assert!(r.ok, "{r:?}");
    assert_eq!(r.value_kind, Some(Stage2ValueKind::Bool));
}

#[test]
fn stage2_validates_concat_wrappers_on_if_variant_constant_values() {
    let src = r#"
          (def cond (prim int/lt? 0 1))
          (if (prim core/eq? ((core/str::concat (if cond "ab" "abc")) "!") "ab!")
            (prim core/eq? ((core/bytes::concat (if cond b"\x01" b"\x01\x02")) b"\xFF") b"\x01\xFF")
            false)
        "#;
    let forms = canonicalize_module(parse_module(src).unwrap()).unwrap();
    let r = stage2_validation_report(&forms);
    assert!(r.supported, "{r:?}");
    assert!(r.ok, "{r:?}");
    assert_eq!(r.value_kind, Some(Stage2ValueKind::Bool));
}

#[test]
fn stage2_validates_concat_wrappers_on_nested_let_if_variant_values() {
    let src = r#"
          (def cond (prim int/lt? 0 1))
          (if (prim core/eq?
                ((core/str::concat
                   (let ((x 1))
                     (if cond "ab" "abc")))
                 (begin 0 "!"))
                "ab!")
            (prim core/eq?
              ((core/bytes::concat
                 (let ((x 1))
                   (if cond b"\x01" b"\x01\x02")))
               (begin 0 b"\xFF"))
              b"\x01\xFF")
            false)
        "#;
    let forms = canonicalize_module(parse_module(src).unwrap()).unwrap();
    let r = stage2_validation_report(&forms);
    assert!(r.supported, "{r:?}");
    assert!(r.ok, "{r:?}");
    assert_eq!(r.value_kind, Some(Stage2ValueKind::Bool));
}

#[test]
fn stage2_validates_concat_prims_on_both_sides_if_variant_constants() {
    let src = r#"
          (def lhs-cond (prim int/lt? 0 1))
          (def rhs-cond (prim int/lt? 1 2))
          (if (prim core/eq?
                (prim str/concat
                  (if lhs-cond "ab" "abc")
                  (if rhs-cond "!" "!!"))
                "ab!")
            (prim core/eq?
              (prim bytes/concat
                (if lhs-cond b"\x01" b"\x01\x02")
                (if rhs-cond b"\xFF" b"\xFE"))
              b"\x01\xFF")
            false)
        "#;
    let forms = canonicalize_module(parse_module(src).unwrap()).unwrap();
    let r = stage2_validation_report(&forms);
    assert!(r.supported, "{r:?}");
    assert!(r.ok, "{r:?}");
    assert_eq!(r.value_kind, Some(Stage2ValueKind::Bool));
}

#[test]
fn stage2_validates_concat_wrappers_on_both_sides_if_variant_constants() {
    let src = r#"
          (def lhs-cond (prim int/lt? 0 1))
          (def rhs-cond (prim int/lt? 1 2))
          (if (prim core/eq?
                ((core/str::concat (if lhs-cond "ab" "abc"))
                 (if rhs-cond "!" "!!"))
                "ab!")
            (prim core/eq?
              ((core/bytes::concat (if lhs-cond b"\x01" b"\x01\x02"))
               (if rhs-cond b"\xFF" b"\xFE"))
              b"\x01\xFF")
            false)
        "#;
    let forms = canonicalize_module(parse_module(src).unwrap()).unwrap();
    let r = stage2_validation_report(&forms);
    assert!(r.supported, "{r:?}");
    assert!(r.ok, "{r:?}");
    assert_eq!(r.value_kind, Some(Stage2ValueKind::Bool));
}

#[test]
fn stage2_validates_symbol_top_level_result() {
    let src = r#"
          (quote :hello/world::flag)
        "#;
    let forms = canonicalize_module(parse_module(src).unwrap()).unwrap();
    let r = stage2_validation_report(&forms);
    assert!(r.supported, "{r:?}");
    assert!(r.ok, "{r:?}");
    assert_eq!(r.value_kind, Some(Stage2ValueKind::Sym));
}

#[test]
fn stage2_validates_sym_eq_prim_and_wrapper_with_data_tag() {
    let src = r#"
          (def t (prim data/tag 7))
          (def a (prim sym/eq? t (quote :int)))
          ((core/sym::eq? (core/data::tag nil)) (quote :nil))
        "#;
    let forms = canonicalize_module(parse_module(src).unwrap()).unwrap();
    let r = stage2_validation_report(&forms);
    assert!(r.supported, "{r:?}");
    assert!(r.ok, "{r:?}");
    assert_eq!(r.value_kind, Some(Stage2ValueKind::Bool));
}

#[test]
fn stage2_validates_data_tag_for_string_and_bytes() {
    let src = r#"
          (def a ((core/sym::eq? (core/data::tag "s")) (quote :str)))
          (if a
            ((core/sym::eq? (core/data::tag b"\x00")) (quote :bytes))
            false)
        "#;
    let forms = canonicalize_module(parse_module(src).unwrap()).unwrap();
    let r = stage2_validation_report(&forms);
    assert!(r.supported, "{r:?}");
    assert!(r.ok, "{r:?}");
    assert_eq!(r.value_kind, Some(Stage2ValueKind::Bool));
}

#[test]
fn stage2_validates_string_and_bytes_top_level_results() {
    let src_str = r#"
          "hello/world"
        "#;
    let forms_str = canonicalize_module(parse_module(src_str).unwrap()).unwrap();
    let r_str = stage2_validation_report(&forms_str);
    assert!(r_str.supported, "{r_str:?}");
    assert!(r_str.ok, "{r_str:?}");
    assert_eq!(r_str.value_kind, Some(Stage2ValueKind::Str));

    let src_bytes = r#"
          b"\x10\x20"
        "#;
    let forms_bytes = canonicalize_module(parse_module(src_bytes).unwrap()).unwrap();
    let r_bytes = stage2_validation_report(&forms_bytes);
    assert!(r_bytes.supported, "{r_bytes:?}");
    assert!(r_bytes.ok, "{r_bytes:?}");
    assert_eq!(r_bytes.value_kind, Some(Stage2ValueKind::Bytes));
}

#[test]
fn stage2_validates_defs_only_module_with_safe_rhs_and_nil_result() {
    let src = r#"
          (def add core/int::add)
          (def id (fn (x) x))
          (def marker (quote hello/world::marker))
        "#;
    let forms = canonicalize_module(parse_module(src).unwrap()).unwrap();
    let r = stage2_validation_report(&forms);
    assert!(r.supported, "{r:?}");
    assert!(r.ok, "{r:?}");
    assert_eq!(r.value_kind, Some(Stage2ValueKind::Nil));
}

#[test]
fn stage2_validates_defs_only_module_with_scalar_rhs_via_lowering() {
    let src = r#"
          (def x (prim int/add 1 2))
          (def y (prim int/mul x 10))
        "#;
    let forms = canonicalize_module(parse_module(src).unwrap()).unwrap();
    let r = stage2_validation_report(&forms);
    assert!(r.supported, "{r:?}");
    assert!(r.ok, "{r:?}");
    assert_eq!(r.value_kind, Some(Stage2ValueKind::Nil));
}

#[test]
fn stage2_validates_defs_only_module_with_quoted_scalar_rhs_via_lowering() {
    let src = r#"
          (def x (quote 42))
          (def y (quote true))
        "#;
    let forms = canonicalize_module(parse_module(src).unwrap()).unwrap();
    let r = stage2_validation_report(&forms);
    assert!(r.supported, "{r:?}");
    assert!(r.ok, "{r:?}");
    assert_eq!(r.value_kind, Some(Stage2ValueKind::Nil));
}

#[test]
fn stage2_validates_defs_only_module_with_collection_composition_rhs() {
    let src = r#"
          (def base {:a 1})
          (def merged (prim map/merge base {:b 2}))
          (def updated (prim map/put merged (quote :c) 3))
          (def v0 [1 2])
          (def v1 (prim vec/push v0 3))
          (def v2 ((core/vec::push v1) 4))
        "#;
    let forms = canonicalize_module(parse_module(src).unwrap()).unwrap();
    let r = stage2_validation_report(&forms);
    assert!(r.supported, "{r:?}");
    assert!(r.ok, "{r:?}");
    assert_eq!(r.value_kind, Some(Stage2ValueKind::Nil));
}

#[test]
fn stage2_validates_defs_only_module_with_if_selected_collection_rhs() {
    let src = r#"
          (def selected-map (if true {:a 1} {:b 2}))
          (def selected-vec (if false [1 2] [3 4]))
          (def merged (prim map/put selected-map (quote :c) 3))
          (def pushed (prim vec/push selected-vec 5))
        "#;
    let forms = canonicalize_module(parse_module(src).unwrap()).unwrap();
    let r = stage2_validation_report(&forms);
    assert!(r.supported, "{r:?}");
    assert!(r.ok, "{r:?}");
    assert_eq!(r.value_kind, Some(Stage2ValueKind::Nil));
}

#[test]
fn stage2_validates_defs_only_module_with_if_selected_collection_rhs_via_prim_condition() {
    let src = r#"
          (def selected-map (if (prim int/lt? 0 1) {:a 1} {:b 2}))
          (def selected-vec (if ((core/int::eq? 1) 2) [1 2] [3 4]))
          (def merged (prim map/put selected-map (quote :c) 3))
          (def pushed (prim vec/push selected-vec 5))
        "#;
    let forms = canonicalize_module(parse_module(src).unwrap()).unwrap();
    let r = stage2_validation_report(&forms);
    assert!(r.supported, "{r:?}");
    assert!(r.ok, "{r:?}");
    assert_eq!(r.value_kind, Some(Stage2ValueKind::Nil));
}

#[test]
fn stage2_validates_defs_only_module_with_if_selected_collection_rhs_via_def_condition_aliases() {
    let src = r#"
          (def cond0 (prim int/lt? 0 1))
          (def cond1 cond0)
          (def selected-map (if cond1 {:a 1} {:b 2}))
          (def selected-vec (if cond1 [1 2] [3 4]))
          (def merged (prim map/put selected-map (quote :c) 3))
          (def pushed (prim vec/push selected-vec 5))
        "#;
    let forms = canonicalize_module(parse_module(src).unwrap()).unwrap();
    let r = stage2_validation_report(&forms);
    assert!(r.supported, "{r:?}");
    assert!(r.ok, "{r:?}");
    assert_eq!(r.value_kind, Some(Stage2ValueKind::Nil));
}

#[test]
fn stage2_rejects_defs_only_module_with_non_trivial_rhs() {
    let src = r#"
          (def x (if cond {:a 1} {:b 2}))
        "#;
    let forms = canonicalize_module(parse_module(src).unwrap()).unwrap();
    let r = stage2_validation_report(&forms);
    assert!(!r.supported, "{r:?}");
    assert!(!r.ok, "{r:?}");
}

#[test]
fn stage2_reports_unsupported_for_effect_program() {
    let src = r#"
          (core/effect::perform
            'sys/time::now
            nil
            (fn (t) (core/effect::pure t)))
        "#;
    let forms = canonicalize_module(parse_module(src).unwrap()).unwrap();
    let r = stage2_validation_report(&forms);
    assert!(!r.supported, "{r:?}");
    assert!(!r.ok, "{r:?}");
}
