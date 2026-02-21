use super::*;

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
