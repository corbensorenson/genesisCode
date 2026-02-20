use super::*;

#[test]
fn eval_stage2_gate_validates_generic_let_collection_alias_flow() {
    let td = tempdir().unwrap();
    let file = td.path().join("generic_let_collection_alias_flow.gc");
    std::fs::write(
        &file,
        r#"
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
        "#,
    )
    .unwrap();

    let out = cargo_bin_cmd!("genesis_wasi")
        .args(["--json", "eval", file.to_str().unwrap(), "--stage2-gate"])
        .assert()
        .success()
        .get_output()
        .stdout
        .clone();
    let v: JsonValue = serde_json::from_slice(&out).unwrap();
    let stage2 = &v["data"]["stage2"];
    assert_eq!(stage2["supported"].as_bool(), Some(true), "{v}");
    assert_eq!(stage2["ok"].as_bool(), Some(true), "{v}");
    assert_eq!(stage2["value_kind"].as_str(), Some("bool"), "{v}");
}

#[test]
fn eval_stage2_gate_validates_def_bound_collection_alias_chains() {
    let td = tempdir().unwrap();
    let file = td.path().join("def_bound_collection_alias_chains.gc");
    std::fs::write(
        &file,
        r#"
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
        "#,
    )
    .unwrap();

    let out = cargo_bin_cmd!("genesis_wasi")
        .args(["--json", "eval", file.to_str().unwrap(), "--stage2-gate"])
        .assert()
        .success()
        .get_output()
        .stdout
        .clone();
    let v: JsonValue = serde_json::from_slice(&out).unwrap();
    let stage2 = &v["data"]["stage2"];
    assert_eq!(stage2["supported"].as_bool(), Some(true), "{v}");
    assert_eq!(stage2["ok"].as_bool(), Some(true), "{v}");
    assert_eq!(stage2["value_kind"].as_str(), Some("bool"), "{v}");
}

#[test]
fn eval_stage2_gate_validates_vec_push_constant_composition() {
    let td = tempdir().unwrap();
    let file = td.path().join("vec_push_constant_composition.gc");
    std::fs::write(
        &file,
        r#"
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
              false)
            false)
        "#,
    )
    .unwrap();

    let out = cargo_bin_cmd!("genesis_wasi")
        .args(["--json", "eval", file.to_str().unwrap(), "--stage2-gate"])
        .assert()
        .success()
        .get_output()
        .stdout
        .clone();
    let v: JsonValue = serde_json::from_slice(&out).unwrap();
    let stage2 = &v["data"]["stage2"];
    assert_eq!(stage2["supported"].as_bool(), Some(true), "{v}");
    assert_eq!(stage2["ok"].as_bool(), Some(true), "{v}");
    assert_eq!(stage2["value_kind"].as_str(), Some("bool"), "{v}");
}

#[test]
fn eval_stage2_gate_validates_bytes_get_wrapper_branch_sensitive_values() {
    let td = tempdir().unwrap();
    let file = td.path().join("bytes_get_if_variant.gc");
    std::fs::write(
        &file,
        r#"
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
        "#,
    )
    .unwrap();

    let out = cargo_bin_cmd!("genesis_wasi")
        .args(["--json", "eval", file.to_str().unwrap(), "--stage2-gate"])
        .assert()
        .success()
        .get_output()
        .stdout
        .clone();
    let v: JsonValue = serde_json::from_slice(&out).unwrap();
    let stage2 = &v["data"]["stage2"];
    assert_eq!(stage2["supported"].as_bool(), Some(true), "{v}");
    assert_eq!(stage2["ok"].as_bool(), Some(true), "{v}");
    assert_eq!(stage2["value_kind"].as_str(), Some("bool"), "{v}");
}

#[test]
fn eval_stage2_gate_validates_coreform_escape_wrapper_branch_sensitive_values() {
    let td = tempdir().unwrap();
    let file = td.path().join("coreform_escape_if_variant.gc");
    std::fs::write(
        &file,
        r#"
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
        "#,
    )
    .unwrap();

    let out = cargo_bin_cmd!("genesis_wasi")
        .args(["--json", "eval", file.to_str().unwrap(), "--stage2-gate"])
        .assert()
        .success()
        .get_output()
        .stdout
        .clone();
    let v: JsonValue = serde_json::from_slice(&out).unwrap();
    let stage2 = &v["data"]["stage2"];
    assert_eq!(stage2["supported"].as_bool(), Some(true), "{v}");
    assert_eq!(stage2["ok"].as_bool(), Some(true), "{v}");
    assert_eq!(stage2["value_kind"].as_str(), Some("bool"), "{v}");
}

#[test]
fn eval_stage2_gate_validates_branch_sensitive_string_bytes_concat_prims() {
    let td = tempdir().unwrap();
    let file = td.path().join("sb_concat_if_variant_prims.gc");
    std::fs::write(
        &file,
        r#"
          (def cond (prim int/lt? 0 1))
          (if (prim core/eq? (prim str/concat (if cond "ab" "abc") "!") "ab!")
            (prim core/eq? (prim bytes/concat (if cond b"\x01" b"\x01\x02") b"\xFF") b"\x01\xFF")
            false)
        "#,
    )
    .unwrap();

    let out = cargo_bin_cmd!("genesis_wasi")
        .args(["--json", "eval", file.to_str().unwrap(), "--stage2-gate"])
        .assert()
        .success()
        .get_output()
        .stdout
        .clone();
    let v: JsonValue = serde_json::from_slice(&out).unwrap();
    let stage2 = &v["data"]["stage2"];
    assert_eq!(stage2["supported"].as_bool(), Some(true), "{v}");
    assert_eq!(stage2["ok"].as_bool(), Some(true), "{v}");
    assert_eq!(stage2["value_kind"].as_str(), Some("bool"), "{v}");
}

#[test]
fn eval_stage2_gate_validates_nested_let_branch_sensitive_concat_wrappers() {
    let td = tempdir().unwrap();
    let file = td.path().join("sb_concat_if_variant_wrappers_nested.gc");
    std::fs::write(
        &file,
        r#"
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
        "#,
    )
    .unwrap();

    let out = cargo_bin_cmd!("genesis_wasi")
        .args(["--json", "eval", file.to_str().unwrap(), "--stage2-gate"])
        .assert()
        .success()
        .get_output()
        .stdout
        .clone();
    let v: JsonValue = serde_json::from_slice(&out).unwrap();
    let stage2 = &v["data"]["stage2"];
    assert_eq!(stage2["supported"].as_bool(), Some(true), "{v}");
    assert_eq!(stage2["ok"].as_bool(), Some(true), "{v}");
    assert_eq!(stage2["value_kind"].as_str(), Some("bool"), "{v}");
}

#[test]
fn eval_stage2_gate_validates_branch_sensitive_concat_both_if_sides() {
    let td = tempdir().unwrap();
    let file = td.path().join("sb_concat_if_variant_both_sides.gc");
    std::fs::write(
        &file,
        r#"
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
        "#,
    )
    .unwrap();

    let out = cargo_bin_cmd!("genesis_wasi")
        .args(["--json", "eval", file.to_str().unwrap(), "--stage2-gate"])
        .assert()
        .success()
        .get_output()
        .stdout
        .clone();
    let v: JsonValue = serde_json::from_slice(&out).unwrap();
    let stage2 = &v["data"]["stage2"];
    assert_eq!(stage2["supported"].as_bool(), Some(true), "{v}");
    assert_eq!(stage2["ok"].as_bool(), Some(true), "{v}");
    assert_eq!(stage2["value_kind"].as_str(), Some("bool"), "{v}");
}

#[test]
fn eval_stage2_gate_validates_branch_sensitive_concat_wrappers_both_if_sides() {
    let td = tempdir().unwrap();
    let file = td
        .path()
        .join("sb_concat_if_variant_wrappers_both_sides.gc");
    std::fs::write(
        &file,
        r#"
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
        "#,
    )
    .unwrap();

    let out = cargo_bin_cmd!("genesis_wasi")
        .args(["--json", "eval", file.to_str().unwrap(), "--stage2-gate"])
        .assert()
        .success()
        .get_output()
        .stdout
        .clone();
    let v: JsonValue = serde_json::from_slice(&out).unwrap();
    let stage2 = &v["data"]["stage2"];
    assert_eq!(stage2["supported"].as_bool(), Some(true), "{v}");
    assert_eq!(stage2["ok"].as_bool(), Some(true), "{v}");
    assert_eq!(stage2["value_kind"].as_str(), Some("bool"), "{v}");
}

#[test]
fn eval_stage1_gate_fails_for_effect_program() {
    let td = tempdir().unwrap();
    let file = td.path().join("effect.gc");
    std::fs::write(
        &file,
        r#"
          (core/effect::perform
            'sys/time::now
            nil
            (fn (t) (core/effect::pure t)))
        "#,
    )
    .unwrap();

    cargo_bin_cmd!("genesis_wasi")
        .args(["eval", file.to_str().unwrap(), "--stage1-gate"])
        .assert()
        .failure()
        .code(30);
}
