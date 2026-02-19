use std::fs;

use assert_cmd::cargo::cargo_bin_cmd;
use gc_coreform::{canonicalize_module, hash_module, hash_term, parse_module, parse_term};
use serde_json::Value as JsonValue;

mod common;

fn build_selfhost_artifact(dir: &std::path::Path) -> std::path::PathBuf {
    common::copy_repo_selfhost_toolchain_artifact(dir)
}

#[test]
fn vcs_hash_hashes_terms_and_modules_deterministically_for_rust_and_selfhost_engines() {
    let td = tempfile::tempdir().unwrap();
    let dir = td.path();
    let artifact = build_selfhost_artifact(dir);

    // Term hashing.
    let term_src =
        r#"{:type :vcs/policy :v 1 :name "policy:test" :refs {:frozen-prefixes []} :classes {}}"#;
    fs::write(dir.join("t.gc"), term_src).unwrap();
    let t = parse_term(term_src).unwrap();
    let expected_term = blake3::Hash::from_bytes(hash_term(&t)).to_hex().to_string();

    let rust_out = cargo_bin_cmd!("genesis_wasi_parity")
        .current_dir(dir)
        .args(["vcs", "hash", "--in", "t.gc", "--engine", "rust"])
        .assert()
        .success()
        .get_output()
        .stdout
        .clone();
    let selfhost_out = cargo_bin_cmd!("genesis_wasi")
        .current_dir(dir)
        .args([
            "--selfhost-artifact",
            artifact.to_str().unwrap(),
            "vcs",
            "hash",
            "--in",
            "t.gc",
            "--engine",
            "selfhost",
        ])
        .assert()
        .success()
        .get_output()
        .stdout
        .clone();
    let rust_got = String::from_utf8(rust_out).unwrap().trim().to_string();
    let selfhost_got = String::from_utf8(selfhost_out).unwrap().trim().to_string();
    assert_eq!(rust_got, expected_term);
    assert_eq!(selfhost_got, expected_term);

    // Module hashing fallback.
    let module_src = r#"
      (def m::x 1)
      m::x
    "#;
    fs::write(dir.join("m.gc"), module_src).unwrap();
    let forms = canonicalize_module(parse_module(module_src).unwrap()).unwrap();
    let expected_mod = blake3::Hash::from_bytes(hash_module(&forms))
        .to_hex()
        .to_string();

    let rust_out = cargo_bin_cmd!("genesis_wasi_parity")
        .current_dir(dir)
        .args(["vcs", "hash", "--in", "m.gc", "--engine", "rust"])
        .assert()
        .success()
        .get_output()
        .stdout
        .clone();
    let selfhost_out = cargo_bin_cmd!("genesis_wasi")
        .current_dir(dir)
        .args([
            "--selfhost-artifact",
            artifact.to_str().unwrap(),
            "vcs",
            "hash",
            "--in",
            "m.gc",
            "--engine",
            "selfhost",
        ])
        .assert()
        .success()
        .get_output()
        .stdout
        .clone();
    let rust_got = String::from_utf8(rust_out).unwrap().trim().to_string();
    let selfhost_got = String::from_utf8(selfhost_out).unwrap().trim().to_string();
    assert_eq!(rust_got, expected_mod);
    assert_eq!(selfhost_got, expected_mod);
}

#[test]
fn vcs_hash_json_schema_v02_matches_between_rust_and_selfhost_engines() {
    let td = tempfile::tempdir().unwrap();
    let dir = td.path();
    let artifact = build_selfhost_artifact(dir);
    fs::write(dir.join("t.gc"), "{:k 1}").unwrap();

    let rust_out = cargo_bin_cmd!("genesis_wasi_parity")
        .current_dir(dir)
        .args(["--json", "vcs", "hash", "--in", "t.gc", "--engine", "rust"])
        .assert()
        .success()
        .get_output()
        .stdout
        .clone();
    let selfhost_out = cargo_bin_cmd!("genesis_wasi")
        .current_dir(dir)
        .args([
            "--json",
            "--selfhost-artifact",
            artifact.to_str().unwrap(),
            "vcs",
            "hash",
            "--in",
            "t.gc",
            "--engine",
            "selfhost",
        ])
        .assert()
        .success()
        .get_output()
        .stdout
        .clone();

    let rust_v: JsonValue = serde_json::from_slice(&rust_out).unwrap();
    let self_v: JsonValue = serde_json::from_slice(&selfhost_out).unwrap();

    assert_eq!(
        rust_v.get("kind").and_then(JsonValue::as_str),
        Some("genesis/vcs-hash-v0.2")
    );
    assert_eq!(rust_v.get("kind"), self_v.get("kind"));

    let rust_d = rust_v.get("data").unwrap();
    let self_d = self_v.get("data").unwrap();
    for key in ["hash", "hash_kind", "hash_format", "in"] {
        assert_eq!(
            rust_d.get(key),
            self_d.get(key),
            "engine mismatch for JSON field {key}"
        );
    }
    assert_eq!(
        rust_d.get("hash_format").and_then(JsonValue::as_str),
        Some("hex")
    );
    assert_eq!(
        rust_d.get("hash_kind").and_then(JsonValue::as_str),
        Some("term")
    );
    assert!(rust_d.get("selfhost_artifact").is_some());
    assert_eq!(
        rust_d.get("selfhost_artifact").and_then(JsonValue::as_null),
        Some(())
    );
    assert_eq!(
        self_d
            .get("selfhost_artifact")
            .and_then(|v| v.get("source"))
            .and_then(JsonValue::as_str),
        Some("explicit")
    );
}
