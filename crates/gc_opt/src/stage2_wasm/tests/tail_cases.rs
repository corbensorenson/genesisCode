use super::*;

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
fn stage2_validates_collection_ops_on_begin_let_wrapped_alias_pipelines() {
    let src = r#"
          (let ((v (begin
                     0
                     (let ((base [1 2]))
                       (prim vec/push base 3))))
                (m (begin
                     0
                     (let ((base {:a 1}))
                       (prim map/put base (quote :b) 2)))))
            (if (prim int/eq? (prim vec/get v 2) 3)
              (prim int/eq? (prim map/get m (quote :b)) 2)
              false))
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
fn stage2_validates_effect_program_via_deterministic_projection() {
    let src = r#"
          (core/effect::perform
            'sys/time::now
            nil
            (fn (t) (core/effect::pure t)))
        "#;
    let forms = canonicalize_module(parse_module(src).unwrap()).unwrap();
    let r = stage2_validation_report(&forms);
    assert!(r.supported, "{r:?}");
    assert!(r.ok, "{r:?}");
    assert_eq!(r.value_kind, Some(Stage2ValueKind::Term));
}
