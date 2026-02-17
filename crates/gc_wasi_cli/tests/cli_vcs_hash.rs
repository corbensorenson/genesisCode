use std::fs;

use assert_cmd::cargo::cargo_bin_cmd;
use gc_coreform::{canonicalize_module, hash_module, hash_term, parse_module, parse_term};

fn build_selfhost_artifact(dir: &std::path::Path) -> std::path::PathBuf {
    let artifact = dir.join("selfhost_toolchain.gc");
    cargo_bin_cmd!("genesis_wasi")
        .args(["selfhost-artifact", "--out"])
        .arg(&artifact)
        .assert()
        .success();
    artifact
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

    let rust_out = cargo_bin_cmd!("genesis_wasi")
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

    let rust_out = cargo_bin_cmd!("genesis_wasi")
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
