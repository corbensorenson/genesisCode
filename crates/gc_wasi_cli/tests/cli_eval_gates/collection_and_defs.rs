use super::*;

#[test]
fn eval_stage2_gate_validates_join_let_bound_vector_aliases() {
    let td = tempdir().unwrap();
    let file = td.path().join("join_let_aliases.gc");
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
fn eval_stage2_gate_validates_vec_get_wrapper_branch_sensitive_values() {
    let td = tempdir().unwrap();
    let file = td.path().join("vec_get_if_variant.gc");
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
fn eval_stage2_gate_validates_vec_len_wrapper_branch_sensitive_values() {
    let td = tempdir().unwrap();
    let file = td.path().join("vec_len_if_variant.gc");
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
fn eval_stage2_gate_validates_let_bound_vector_aliases() {
    let td = tempdir().unwrap();
    let file = td.path().join("vec_let_aliases.gc");
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
fn eval_stage2_gate_validates_map_wrappers_branch_sensitive_values() {
    let td = tempdir().unwrap();
    let file = td.path().join("map_wrappers_if_variant.gc");
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
fn eval_stage2_gate_validates_map_put_merge_constant_composition() {
    let td = tempdir().unwrap();
    let file = td.path().join("map_put_merge_constant_composition.gc");
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
fn eval_stage2_gate_validates_collection_constant_composition_on_alias_sources() {
    let td = tempdir().unwrap();
    let file = td
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
fn eval_stage2_gate_validates_defs_only_collection_composition_module() {
    let td = tempdir().unwrap();
    let file = td.path().join("defs_only_collection_composition.gc");
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
    assert_eq!(stage2["value_kind"].as_str(), Some("nil"), "{v}");
}

#[test]
fn eval_stage2_gate_validates_defs_only_if_selected_collection_module() {
    let td = tempdir().unwrap();
    let file = td.path().join("defs_only_if_selected_collection.gc");
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
    assert_eq!(stage2["value_kind"].as_str(), Some("nil"), "{v}");
}

#[test]
fn eval_stage2_gate_validates_defs_only_if_selected_collection_module_via_prim_condition() {
    let td = tempdir().unwrap();
    let file = td
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
    assert_eq!(stage2["value_kind"].as_str(), Some("nil"), "{v}");
}

#[test]
fn eval_stage2_gate_validates_defs_only_if_selected_collection_module_via_def_condition_aliases() {
    let td = tempdir().unwrap();
    let file = td
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
    assert_eq!(stage2["value_kind"].as_str(), Some("nil"), "{v}");
}

#[test]
fn eval_stage2_gate_validates_map_let_bound_aliases() {
    let td = tempdir().unwrap();
    let file = td.path().join("map_let_aliases.gc");
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
fn eval_stage2_gate_validates_let_bound_collection_alias_chains() {
    let td = tempdir().unwrap();
    let file = td.path().join("let_bound_collection_alias_chains.gc");
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
