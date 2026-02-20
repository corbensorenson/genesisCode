use assert_cmd::cargo::cargo_bin_cmd;
use serde_json::Value as JsonValue;
use tempfile::tempdir;

mod support;

fn build_selfhost_artifact(dir: &std::path::Path) -> std::path::PathBuf {
    support::copy_repo_toolchain_artifact(dir)
}

fn genesis_cmd() -> assert_cmd::Command {
    let mut cmd = cargo_bin_cmd!("genesis");
    cmd.env(
        "GENESIS_SELFHOST_TOOLCHAIN_ARTIFACT",
        support::repo_toolchain_artifact(),
    );
    cmd
}

#[test]
fn eval_stage1_pipeline_and_gate_match_baseline_for_pure_module() {
    let dir = tempdir().unwrap();
    let file = dir.path().join("m.gc");
    std::fs::write(
        &file,
        r#"
          (def x (prim int/add 1 (prim int/add 2 0)))
          x
        "#,
    )
    .unwrap();

    let base = genesis_cmd()
        .args(["eval", file.to_str().unwrap()])
        .assert()
        .success()
        .get_output()
        .stdout
        .clone();
    let stage1 = genesis_cmd()
        .args([
            "eval",
            file.to_str().unwrap(),
            "--stage1-pipeline",
            "--stage1-gate",
        ])
        .assert()
        .success()
        .get_output()
        .stdout
        .clone();

    let base_s = String::from_utf8(base).unwrap();
    let stage1_s = String::from_utf8(stage1).unwrap();
    assert_eq!(base_s.trim(), stage1_s.trim());
}

#[test]
fn eval_stage1_gate_fails_for_effect_program() {
    let dir = tempdir().unwrap();
    let file = dir.path().join("effect.gc");
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

    genesis_cmd()
        .args(["eval", file.to_str().unwrap(), "--stage1-gate"])
        .assert()
        .failure()
        .code(30);
}

#[test]
fn optimize_stage1_gate_fails_for_effect_program() {
    let dir = tempdir().unwrap();
    let file = dir.path().join("effect.gc");
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

    genesis_cmd()
        .args(["optimize", file.to_str().unwrap(), "--stage1-gate"])
        .assert()
        .failure()
        .code(30);
}

#[test]
fn optimize_selfhost_engine_matches_rust_stage1_output() {
    let dir = tempdir().unwrap();
    let file = dir.path().join("m.gc");
    let artifact = build_selfhost_artifact(dir.path());
    std::fs::write(
        &file,
        r#"
          (def x (prim int/add 0 (prim int/add 1 2)))
          x
        "#,
    )
    .unwrap();

    let rust = cargo_bin_cmd!("genesis_parity")
        .args(["optimize", file.to_str().unwrap(), "--engine", "rust"])
        .assert()
        .success()
        .get_output()
        .stdout
        .clone();
    let selfhost = genesis_cmd()
        .args([
            "--selfhost-artifact",
            artifact.to_str().unwrap(),
            "optimize",
            file.to_str().unwrap(),
            "--engine",
            "selfhost",
        ])
        .assert()
        .success()
        .get_output()
        .stdout
        .clone();

    let rust_s = String::from_utf8(rust).unwrap();
    let selfhost_s = String::from_utf8(selfhost).unwrap();
    assert_eq!(rust_s, selfhost_s);
}

#[test]
fn eval_selfhost_engine_stage1_gate_matches_rust() {
    let dir = tempdir().unwrap();
    let file = dir.path().join("m.gc");
    let artifact = build_selfhost_artifact(dir.path());
    std::fs::write(
        &file,
        r#"
          (def x (prim int/add 4 (prim int/add 5 0)))
          x
        "#,
    )
    .unwrap();

    let rust = cargo_bin_cmd!("genesis_parity")
        .args([
            "eval",
            file.to_str().unwrap(),
            "--engine",
            "rust",
            "--stage1-gate",
        ])
        .assert()
        .success()
        .get_output()
        .stdout
        .clone();
    let selfhost = genesis_cmd()
        .args([
            "--selfhost-artifact",
            artifact.to_str().unwrap(),
            "eval",
            file.to_str().unwrap(),
            "--engine",
            "selfhost",
            "--stage1-gate",
        ])
        .assert()
        .success()
        .get_output()
        .stdout
        .clone();

    let rust_s = String::from_utf8(rust).unwrap();
    let selfhost_s = String::from_utf8(selfhost).unwrap();
    assert_eq!(rust_s.trim(), selfhost_s.trim());
}

#[test]
fn optimize_stage2_gate_and_emit_wasm_succeeds_for_scalar_pure_module() {
    let dir = tempdir().unwrap();
    let file = dir.path().join("m.gc");
    let wasm = dir.path().join("m.wasm");
    std::fs::write(
        &file,
        r#"
          (def x (prim int/add 20 22))
          x
        "#,
    )
    .unwrap();

    genesis_cmd()
        .args([
            "optimize",
            file.to_str().unwrap(),
            "--stage2-gate",
            "--emit-wasm",
            wasm.to_str().unwrap(),
        ])
        .assert()
        .success();

    let bytes = std::fs::read(&wasm).unwrap();
    assert!(!bytes.is_empty());
}

#[test]
fn eval_stage2_gate_matches_baseline_for_scalar_pure_module() {
    let dir = tempdir().unwrap();
    let file = dir.path().join("m.gc");
    std::fs::write(
        &file,
        r#"
          (def x (prim int/add 20 22))
          x
        "#,
    )
    .unwrap();

    let base = genesis_cmd()
        .args(["eval", file.to_str().unwrap()])
        .assert()
        .success()
        .get_output()
        .stdout
        .clone();
    let gated = genesis_cmd()
        .args(["eval", file.to_str().unwrap(), "--stage2-gate"])
        .assert()
        .success()
        .get_output()
        .stdout
        .clone();

    let base_s = String::from_utf8(base).unwrap();
    let gated_s = String::from_utf8(gated).unwrap();
    assert_eq!(base_s.trim(), gated_s.trim());
}

#[test]
fn eval_stage2_gate_rejects_unsupported_module() {
    let dir = tempdir().unwrap();
    let file = dir.path().join("effect.gc");
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

    genesis_cmd()
        .args(["eval", file.to_str().unwrap(), "--stage2-gate"])
        .assert()
        .failure()
        .code(30)
        .stderr(predicates::str::contains(
            "core/obligation::translation-validation",
        ));
}

#[test]
fn eval_stage2_gate_uses_stage1_transformed_input_for_stage2_report() {
    let dir = tempdir().unwrap();
    let file = dir.path().join("if_fold.gc");
    std::fs::write(
        &file,
        r#"
          (if true
            (prim int/add 1 2)
            (quote {a 1}))
        "#,
    )
    .unwrap();

    let out = genesis_cmd()
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
fn eval_stage2_gate_reports_string_value_kind_in_json() {
    let dir = tempdir().unwrap();
    let file = dir.path().join("s.gc");
    std::fs::write(
        &file,
        r#"
          "hello"
        "#,
    )
    .unwrap();

    let out = genesis_cmd()
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
    assert_eq!(stage2["value_kind"].as_str(), Some("str"), "{v}");
}

#[test]
fn eval_stage2_gate_reports_bytes_value_kind_in_json() {
    let dir = tempdir().unwrap();
    let file = dir.path().join("b.gc");
    std::fs::write(
        &file,
        r#"
          b"\xAA\xBB"
        "#,
    )
    .unwrap();

    let out = genesis_cmd()
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
    assert_eq!(stage2["value_kind"].as_str(), Some("bytes"), "{v}");
}

#[test]
fn eval_stage2_gate_validates_string_bytes_concat_len_module() {
    let dir = tempdir().unwrap();
    let file = dir.path().join("sb_concat_len.gc");
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

    let out = genesis_cmd()
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
    let dir = tempdir().unwrap();
    let file = dir.path().join("sb_len_if_variant.gc");
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

    let out = genesis_cmd()
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
    let dir = tempdir().unwrap();
    let file = dir.path().join("sb_len_if_variant_wrappers.gc");
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

    let out = genesis_cmd()
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
    let dir = tempdir().unwrap();
    let file = dir.path().join("sb_len_if_variant_wrappers_nested.gc");
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

    let out = genesis_cmd()
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
    let dir = tempdir().unwrap();
    let file = dir.path().join("int_to_str_if_variant.gc");
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

    let out = genesis_cmd()
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
    let dir = tempdir().unwrap();
    let file = dir.path().join("sym_string_if_variant.gc");
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

    let out = genesis_cmd()
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
    let dir = tempdir().unwrap();
    let file = dir.path().join("utf8_if_variant.gc");
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

    let out = genesis_cmd()
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
fn eval_stage2_gate_validates_hex_wrapper_branch_sensitive_values() {
    let dir = tempdir().unwrap();
    let file = dir.path().join("hex_if_variant.gc");
    std::fs::write(
        &file,
        r#"
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
        "#,
    )
    .unwrap();

    let out = genesis_cmd()
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
fn eval_stage2_gate_validates_str_repeat_wrapper_branch_sensitive_values() {
    let dir = tempdir().unwrap();
    let file = dir.path().join("str_repeat_if_variant.gc");
    std::fs::write(
        &file,
        r#"
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
        "#,
    )
    .unwrap();

    let out = genesis_cmd()
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
fn eval_stage2_gate_validates_join_wrappers_branch_sensitive_values() {
    let dir = tempdir().unwrap();
    let file = dir.path().join("join_wrappers_if_variant.gc");
    std::fs::write(
        &file,
        r#"
          (def cond (prim int/lt? 0 1))
          (if (prim core/eq?
                ((core/str::join
                   (let ((x 1))
                     (if cond ["ab" "cd"] ["x" "y"])))
                 (if cond "-" ":"))
                "ab-cd")
            (prim core/eq?
              (core/bytes::join
                (let ((x 1))
                  (if cond [b"\xAA" b"\xBB"] [b"\xCC"])))
              b"\xAA\xBB")
            false)
        "#,
    )
    .unwrap();

    let out = genesis_cmd()
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
fn eval_stage2_gate_validates_join_let_bound_vector_aliases() {
    let dir = tempdir().unwrap();
    let file = dir.path().join("join_let_aliases.gc");
    std::fs::write(
        &file,
        r#"
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
        "#,
    )
    .unwrap();

    let out = genesis_cmd()
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
fn eval_stage2_gate_validates_vec_get_wrapper_branch_sensitive_values() {
    let dir = tempdir().unwrap();
    let file = dir.path().join("vec_get_if_variant.gc");
    std::fs::write(
        &file,
        r#"
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
        "#,
    )
    .unwrap();

    let out = genesis_cmd()
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
fn eval_stage2_gate_validates_vec_len_wrapper_branch_sensitive_values() {
    let dir = tempdir().unwrap();
    let file = dir.path().join("vec_len_if_variant.gc");
    std::fs::write(
        &file,
        r#"
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
        "#,
    )
    .unwrap();

    let out = genesis_cmd()
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
fn eval_stage2_gate_validates_let_bound_vector_aliases() {
    let dir = tempdir().unwrap();
    let file = dir.path().join("vec_let_aliases.gc");
    std::fs::write(
        &file,
        r#"
          (if (prim int/eq?
                (prim vec/get
                  (let ((v1 [5 6 7])
                        (v2 v1)
                        (v3 v2))
                    v3)
                  1)
                6)
            (if (prim int/eq?
                  (core/vec::len
                    (let ((v1 (prim vec/push [8] 9))
                          (v2 v1)
                          (v3 v2))
                      v3))
                  2)
              (prim list/is-nil?
                (prim vec/get
                  (let ((v1 (prim vec/push [] 9))
                        (v2 v1)
                        (v3 v2))
                    v3)
                  5))
              false)
            false)
        "#,
    )
    .unwrap();

    let out = genesis_cmd()
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
fn eval_stage2_gate_validates_map_wrappers_branch_sensitive_values() {
    let dir = tempdir().unwrap();
    let file = dir.path().join("map_wrappers_if_variant.gc");
    std::fs::write(
        &file,
        r#"
          (def cond (prim int/lt? 0 1))
          (if (prim int/eq?
                (core/map::len
                  (if cond {:a 1 :b 2} {:z 9}))
                2)
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
            false)
        "#,
    )
    .unwrap();

    let out = genesis_cmd()
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
fn eval_stage2_gate_validates_map_put_merge_constant_composition() {
    let dir = tempdir().unwrap();
    let file = dir.path().join("map_put_merge_constant_composition.gc");
    std::fs::write(
        &file,
        r#"
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
        "#,
    )
    .unwrap();

    let out = genesis_cmd()
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
fn eval_stage2_gate_validates_collection_constant_composition_on_alias_sources() {
    let dir = tempdir().unwrap();
    let file = dir
        .path()
        .join("collection_constant_composition_alias_sources.gc");
    std::fs::write(
        &file,
        r#"
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
        "#,
    )
    .unwrap();

    let out = genesis_cmd()
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
fn eval_stage2_gate_validates_defs_only_collection_composition_module() {
    let dir = tempdir().unwrap();
    let file = dir.path().join("defs_only_collection_composition.gc");
    std::fs::write(
        &file,
        r#"
          (def base {:a 1})
          (def merged (prim map/merge base {:b 2}))
          (def updated (prim map/put merged (quote :c) 3))
          (def v0 [1 2])
          (def v1 (prim vec/push v0 3))
          (def v2 ((core/vec::push v1) 4))
        "#,
    )
    .unwrap();

    let out = genesis_cmd()
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
    assert_eq!(stage2["value_kind"].as_str(), Some("nil"), "{v}");
}

#[test]
fn eval_stage2_gate_validates_defs_only_if_selected_collection_module() {
    let dir = tempdir().unwrap();
    let file = dir.path().join("defs_only_if_selected_collection.gc");
    std::fs::write(
        &file,
        r#"
          (def selected-map (if true {:a 1} {:b 2}))
          (def selected-vec (if false [1 2] [3 4]))
          (def merged (prim map/put selected-map (quote :c) 3))
          (def pushed (prim vec/push selected-vec 5))
        "#,
    )
    .unwrap();

    let out = genesis_cmd()
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
    assert_eq!(stage2["value_kind"].as_str(), Some("nil"), "{v}");
}

#[test]
fn eval_stage2_gate_validates_defs_only_if_selected_collection_module_via_prim_condition() {
    let dir = tempdir().unwrap();
    let file = dir
        .path()
        .join("defs_only_if_selected_collection_prim_cond.gc");
    std::fs::write(
        &file,
        r#"
          (def selected-map (if (prim int/lt? 0 1) {:a 1} {:b 2}))
          (def selected-vec (if ((core/int::eq? 1) 2) [1 2] [3 4]))
          (def merged (prim map/put selected-map (quote :c) 3))
          (def pushed (prim vec/push selected-vec 5))
        "#,
    )
    .unwrap();

    let out = genesis_cmd()
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
    assert_eq!(stage2["value_kind"].as_str(), Some("nil"), "{v}");
}

#[test]
fn eval_stage2_gate_validates_defs_only_if_selected_collection_module_via_def_condition_aliases() {
    let dir = tempdir().unwrap();
    let file = dir
        .path()
        .join("defs_only_if_selected_collection_def_cond_alias.gc");
    std::fs::write(
        &file,
        r#"
          (def cond0 (prim int/lt? 0 1))
          (def cond1 cond0)
          (def selected-map (if cond1 {:a 1} {:b 2}))
          (def selected-vec (if cond1 [1 2] [3 4]))
          (def merged (prim map/put selected-map (quote :c) 3))
          (def pushed (prim vec/push selected-vec 5))
        "#,
    )
    .unwrap();

    let out = genesis_cmd()
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
    assert_eq!(stage2["value_kind"].as_str(), Some("nil"), "{v}");
}

#[test]
fn eval_stage2_gate_validates_map_let_bound_aliases() {
    let dir = tempdir().unwrap();
    let file = dir.path().join("map_let_aliases.gc");
    std::fs::write(
        &file,
        r#"
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
        "#,
    )
    .unwrap();

    let out = genesis_cmd()
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
fn eval_stage2_gate_validates_let_bound_collection_alias_chains() {
    let dir = tempdir().unwrap();
    let file = dir.path().join("let_bound_collection_alias_chains.gc");
    std::fs::write(
        &file,
        r#"
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
        "#,
    )
    .unwrap();

    let out = genesis_cmd()
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
fn eval_stage2_gate_validates_generic_let_collection_alias_flow() {
    let dir = tempdir().unwrap();
    let file = dir.path().join("generic_let_collection_alias_flow.gc");
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

    let out = genesis_cmd()
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
    let dir = tempdir().unwrap();
    let file = dir.path().join("def_bound_collection_alias_chains.gc");
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

    let out = genesis_cmd()
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
    let dir = tempdir().unwrap();
    let file = dir.path().join("vec_push_constant_composition.gc");
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

    let out = genesis_cmd()
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
    let dir = tempdir().unwrap();
    let file = dir.path().join("bytes_get_if_variant.gc");
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

    let out = genesis_cmd()
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
    let dir = tempdir().unwrap();
    let file = dir.path().join("coreform_escape_if_variant.gc");
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

    let out = genesis_cmd()
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
    let dir = tempdir().unwrap();
    let file = dir.path().join("sb_concat_if_variant_prims.gc");
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

    let out = genesis_cmd()
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
    let dir = tempdir().unwrap();
    let file = dir.path().join("sb_concat_if_variant_wrappers_nested.gc");
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

    let out = genesis_cmd()
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
    let dir = tempdir().unwrap();
    let file = dir.path().join("sb_concat_if_variant_both_sides.gc");
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

    let out = genesis_cmd()
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
    let dir = tempdir().unwrap();
    let file = dir
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

    let out = genesis_cmd()
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
fn optimize_stage2_gate_rejects_unsupported_module() {
    let dir = tempdir().unwrap();
    let file = dir.path().join("effect.gc");
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

    genesis_cmd()
        .args(["optimize", file.to_str().unwrap(), "--stage2-gate"])
        .assert()
        .failure()
        .code(30)
        .stderr(predicates::str::contains(
            "core/obligation::translation-validation",
        ));
}

#[test]
fn optimize_emit_wasm_fails_for_unsupported_module() {
    let dir = tempdir().unwrap();
    let file = dir.path().join("effect.gc");
    let wasm = dir.path().join("effect.wasm");
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

    genesis_cmd()
        .args([
            "optimize",
            file.to_str().unwrap(),
            "--emit-wasm",
            wasm.to_str().unwrap(),
        ])
        .assert()
        .failure()
        .code(30);
}
