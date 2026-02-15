use std::fs;

use assert_cmd::cargo::cargo_bin_cmd;
use gc_coreform::{canonicalize_module, hash_module, hash_term, parse_module, parse_term};

#[test]
fn vcs_hash_hashes_terms_and_modules_deterministically() {
    let td = tempfile::tempdir().unwrap();
    let dir = td.path();

    // Term hashing.
    let term_src =
        r#"{:type :vcs/policy :v 1 :name "policy:test" :refs {:frozen-prefixes []} :classes {}}"#;
    fs::write(dir.join("t.gc"), term_src).unwrap();
    let t = parse_term(term_src).unwrap();
    let expected_term = gc_vcs::bytes32_to_hex(&hash_term(&t));

    let out = cargo_bin_cmd!("genesis")
        .current_dir(dir)
        .args(["vcs", "hash", "--in", "t.gc"])
        .assert()
        .success()
        .get_output()
        .stdout
        .clone();
    let got = String::from_utf8(out).unwrap().trim().to_string();
    assert_eq!(got, expected_term);

    // Module hashing fallback.
    let module_src = r#"
      (def m::x 1)
      m::x
    "#;
    fs::write(dir.join("m.gc"), module_src).unwrap();
    let forms = canonicalize_module(parse_module(module_src).unwrap()).unwrap();
    let expected_mod = gc_vcs::bytes32_to_hex(&hash_module(&forms));

    let out = cargo_bin_cmd!("genesis")
        .current_dir(dir)
        .args(["vcs", "hash", "--in", "m.gc"])
        .assert()
        .success()
        .get_output()
        .stdout
        .clone();
    let got = String::from_utf8(out).unwrap().trim().to_string();
    assert_eq!(got, expected_mod);
}
