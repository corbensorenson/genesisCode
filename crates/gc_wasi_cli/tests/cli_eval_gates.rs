use assert_cmd::cargo::cargo_bin_cmd;
use predicates::prelude::*;
use serde_json::Value as JsonValue;
use tempfile::tempdir;

#[test]
fn eval_help_exposes_stage_gating_flags() {
    cargo_bin_cmd!("genesis_wasi")
        .args(["eval", "--help"])
        .assert()
        .success()
        .stdout(predicate::str::contains("--stage1-pipeline"))
        .stdout(predicate::str::contains("--stage1-gate"))
        .stdout(predicate::str::contains("--stage2-gate"));
}

#[test]
fn eval_stage2_gate_succeeds_for_scalar_pure_program() {
    let td = tempdir().unwrap();
    let file = td.path().join("pure.gc");
    std::fs::write(
        &file,
        r#"
          (def x (prim int/add 20 22))
          x
        "#,
    )
    .unwrap();

    cargo_bin_cmd!("genesis_wasi")
        .args(["eval", file.to_str().unwrap(), "--stage2-gate"])
        .assert()
        .success()
        .stdout(predicate::str::contains("42"));
}

#[test]
fn eval_stage2_gate_allows_unsupported_non_scalar_module() {
    let td = tempdir().unwrap();
    let file = td.path().join("map.gc");
    std::fs::write(
        &file,
        r#"
          (quote {a 1 b 2})
        "#,
    )
    .unwrap();

    cargo_bin_cmd!("genesis_wasi")
        .args(["eval", file.to_str().unwrap(), "--stage2-gate"])
        .assert()
        .success()
        .stdout(predicate::str::contains("{a 1 b 2}"));
}

#[test]
fn eval_stage2_gate_uses_stage1_transformed_input_for_stage2_report() {
    let td = tempdir().unwrap();
    let file = td.path().join("if_fold.gc");
    std::fs::write(
        &file,
        r#"
          (if true
            (prim int/add 1 2)
            (quote {a 1}))
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
}

#[test]
fn eval_stage2_gate_reports_string_and_bytes_value_kinds_in_json() {
    let td = tempdir().unwrap();
    let file_str = td.path().join("s.gc");
    std::fs::write(
        &file_str,
        r#"
          "hello"
        "#,
    )
    .unwrap();
    let out_str = cargo_bin_cmd!("genesis_wasi")
        .args([
            "--json",
            "eval",
            file_str.to_str().unwrap(),
            "--stage2-gate",
        ])
        .assert()
        .success()
        .get_output()
        .stdout
        .clone();
    let v_str: JsonValue = serde_json::from_slice(&out_str).unwrap();
    let stage2_str = &v_str["data"]["stage2"];
    assert_eq!(stage2_str["supported"].as_bool(), Some(true), "{v_str}");
    assert_eq!(stage2_str["ok"].as_bool(), Some(true), "{v_str}");
    assert_eq!(stage2_str["value_kind"].as_str(), Some("str"), "{v_str}");

    let file_bytes = td.path().join("b.gc");
    std::fs::write(
        &file_bytes,
        r#"
          b"\xAA\xBB"
        "#,
    )
    .unwrap();
    let out_bytes = cargo_bin_cmd!("genesis_wasi")
        .args([
            "--json",
            "eval",
            file_bytes.to_str().unwrap(),
            "--stage2-gate",
        ])
        .assert()
        .success()
        .get_output()
        .stdout
        .clone();
    let v_bytes: JsonValue = serde_json::from_slice(&out_bytes).unwrap();
    let stage2_bytes = &v_bytes["data"]["stage2"];
    assert_eq!(stage2_bytes["supported"].as_bool(), Some(true), "{v_bytes}");
    assert_eq!(stage2_bytes["ok"].as_bool(), Some(true), "{v_bytes}");
    assert_eq!(
        stage2_bytes["value_kind"].as_str(),
        Some("bytes"),
        "{v_bytes}"
    );
}

#[test]
fn eval_stage2_gate_validates_string_bytes_concat_len_module() {
    let td = tempdir().unwrap();
    let file = td.path().join("sb_concat_len.gc");
    std::fs::write(
        &file,
        r#"
          (def s1 "hello, ")
          (def s2 "world")
          (def b1 b"\x01")
          (def b2 b"\x02\x03")
          (def s ((core/str::concat s1) s2))
          (def b ((core/bytes::concat b1) b2))
          (if (prim core/eq? s "hello, world")
            (if (prim core/eq? b b"\x01\x02\x03")
              (if (prim int/eq? (core/str::len s) 12)
                (prim int/eq? (core/bytes::len b) 3)
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
fn eval_stage2_gate_validates_branch_sensitive_string_bytes_len_prims() {
    let td = tempdir().unwrap();
    let file = td.path().join("sb_len_if_variant.gc");
    std::fs::write(
        &file,
        r#"
          (def cond (prim int/lt? 0 1))
          (if (prim int/eq? (prim str/len (if cond "abc" "abcd")) 3)
            (prim int/eq? (prim bytes/len (if cond b"\x10\x20" b"\x10\x20\x30")) 2)
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
fn eval_stage2_gate_validates_branch_sensitive_string_bytes_len_wrappers() {
    let td = tempdir().unwrap();
    let file = td.path().join("sb_len_if_variant_wrappers.gc");
    std::fs::write(
        &file,
        r#"
          (def cond (prim int/lt? 0 1))
          (if (prim int/eq? (core/str::len (if cond "abc" "abcd")) 3)
            (prim int/eq? (core/bytes::len (if cond b"\x10\x20" b"\x10\x20\x30")) 2)
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
fn eval_stage2_gate_validates_nested_let_branch_sensitive_len_wrappers() {
    let td = tempdir().unwrap();
    let file = td.path().join("sb_len_if_variant_wrappers_nested.gc");
    std::fs::write(
        &file,
        r#"
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
fn eval_stage2_gate_validates_int_to_str_wrapper_branch_sensitive_values() {
    let td = tempdir().unwrap();
    let file = td.path().join("int_to_str_if_variant.gc");
    std::fs::write(
        &file,
        r#"
          (def cond (prim int/lt? 0 1))
          (if (prim core/eq?
                (core/int::to-str
                  (let ((x 1))
                    (if cond 42 420)))
                "42")
            (prim core/eq? (core/int::to-str (if cond -7 -70)) "-7")
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
fn eval_stage2_gate_validates_sym_string_wrapper_branch_sensitive_values() {
    let td = tempdir().unwrap();
    let file = td.path().join("sym_string_if_variant.gc");
    std::fs::write(
        &file,
        r#"
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
fn eval_stage2_gate_validates_utf8_wrapper_branch_sensitive_values() {
    let td = tempdir().unwrap();
    let file = td.path().join("utf8_if_variant.gc");
    std::fs::write(
        &file,
        r#"
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
