use std::fs;
use std::path::PathBuf;

use assert_cmd::cargo::cargo_bin_cmd;
use gc_coreform::{
    Term, TermOrdKey, canonicalize_module, hash_module, parse_module, parse_term, print_term,
};
use predicates::prelude::*;

mod support;

fn write_caps(dir: &std::path::Path, allow: &[&str]) -> PathBuf {
    let caps = dir.join("caps.toml");
    let mut s = String::new();
    s.push_str("allow = [");
    for (i, op) in allow.iter().enumerate() {
        if i != 0 {
            s.push_str(", ");
        }
        s.push('"');
        s.push_str(op);
        s.push('"');
    }
    s.push_str("]\n\n[store]\ndir = \"./.genesis/store\"\n");
    fs::write(&caps, s).unwrap();
    caps
}

fn cmd() -> assert_cmd::Command {
    let mut c = cargo_bin_cmd!("genesis");
    c.env("GENESIS_ALLOW_RUST_ENGINE", "1");
    c
}

fn build_selfhost_artifact(dir: &std::path::Path) -> std::path::PathBuf {
    support::copy_repo_toolchain_artifact(dir)
}

fn poison_cli_store_put_program(artifact: &std::path::Path) {
    let src = fs::read_to_string(artifact).unwrap();
    let mut term = parse_term(&src).unwrap();
    let Term::Map(root) = &mut term else {
        panic!("artifact root must be map");
    };
    let modules = root
        .get_mut(&TermOrdKey(Term::symbol(":modules")))
        .expect("artifact :modules");
    let Term::Vector(entries) = modules else {
        panic!("artifact :modules must be vector");
    };
    let cli_mod = entries
        .iter_mut()
        .find_map(|entry| match entry {
            Term::Map(mm)
                if matches!(
                    mm.get(&TermOrdKey(Term::symbol(":path"))),
                    Some(Term::Str(path)) if path == "selfhost/cli_coreform_v1.gc"
                ) =>
            {
                Some(mm)
            }
            _ => None,
        })
        .expect("selfhost/cli_coreform_v1.gc entry");

    let module_src = match cli_mod.get(&TermOrdKey(Term::symbol(":source"))) {
        Some(Term::Str(src)) => src.clone(),
        _ => panic!("cli module missing :source"),
    };
    let poisoned_src = format!("{module_src}\n(def core/cli::store-put-program \"shadowed\")\n");
    let poisoned_forms = canonicalize_module(parse_module(&poisoned_src).unwrap()).unwrap();
    let poisoned_hash = hash_module(&poisoned_forms);
    cli_mod.insert(
        TermOrdKey(Term::symbol(":source")),
        Term::Str(poisoned_src.to_string()),
    );
    cli_mod.insert(
        TermOrdKey(Term::symbol(":forms")),
        Term::Vector(poisoned_forms),
    );
    cli_mod.insert(
        TermOrdKey(Term::symbol(":module-h")),
        Term::Bytes(poisoned_hash.to_vec().into()),
    );
    fs::write(artifact, print_term(&term)).unwrap();
}

#[test]
fn store_put_has_get_roundtrip() {
    let td = tempfile::tempdir().unwrap();
    let dir = td.path();

    let caps = write_caps(
        dir,
        &["core/store::put", "core/store::has", "core/store::get"],
    );

    let inp = dir.join("artifact.gc");
    fs::write(&inp, "{:x 1 :y \"hi\"}\n").unwrap();

    let log_put = dir.join("put.gclog");
    let put_out = cargo_bin_cmd!("genesis")
        .current_dir(dir)
        .args(["store", "--caps"])
        .arg(&caps)
        .args(["--log"])
        .arg(&log_put)
        .args(["put", "--input"])
        .arg(&inp)
        .assert()
        .success()
        .get_output()
        .stdout
        .clone();
    let h = String::from_utf8(put_out).unwrap();
    let h = h.trim().to_string();
    assert!(predicate::str::is_match("^[0-9a-f]{64}$").unwrap().eval(&h));

    let stored = dir.join(".genesis").join("store").join(&h);
    assert!(stored.exists());
    assert!(log_put.exists());

    // has => true
    let log_has = dir.join("has.gclog");
    cargo_bin_cmd!("genesis")
        .current_dir(dir)
        .args(["store", "--caps"])
        .arg(&caps)
        .args(["--log"])
        .arg(&log_has)
        .args(["has"])
        .arg(&h)
        .assert()
        .success()
        .stdout("true\n");

    // get => artifact term printed
    let log_get = dir.join("get.gclog");
    cargo_bin_cmd!("genesis")
        .current_dir(dir)
        .args(["store", "--caps"])
        .arg(&caps)
        .args(["--log"])
        .arg(&log_get)
        .args(["get"])
        .arg(&h)
        .assert()
        .success()
        .stdout("{:x 1 :y \"hi\"}\n");

    // get --out writes canonical term, stdout empty
    let out = dir.join("out.gc");
    let log_get2 = dir.join("get2.gclog");
    cargo_bin_cmd!("genesis")
        .current_dir(dir)
        .args(["store", "--caps"])
        .arg(&caps)
        .args(["--log"])
        .arg(&log_get2)
        .args(["get"])
        .arg(&h)
        .args(["--out"])
        .arg(&out)
        .assert()
        .success()
        .stdout("");
    let out_s = fs::read_to_string(&out).unwrap();
    assert_eq!(out_s, "{:x 1 :y \"hi\"}\n");

    // has missing => false
    let log_has2 = dir.join("has2.gclog");
    cmd()
        .current_dir(dir)
        .args(["store", "--caps"])
        .arg(&caps)
        .args(["--log"])
        .arg(&log_has2)
        .args(["has"])
        .arg("0000000000000000000000000000000000000000000000000000000000000000")
        .assert()
        .success()
        .stdout("false\n");
}

#[test]
fn store_deny_by_default_is_caps_denied_exit_41() {
    let td = tempfile::tempdir().unwrap();
    let dir = td.path();

    let caps = write_caps(dir, &[]);
    let inp = dir.join("artifact.gc");
    fs::write(&inp, "{:x 1 :y \"hi\"}\n").unwrap();

    cmd()
        .current_dir(dir)
        .args(["store", "--caps"])
        .arg(&caps)
        .args(["put", "--input"])
        .arg(&inp)
        .assert()
        .code(41);
}

#[test]
fn store_get_missing_is_exit_20_and_does_not_write_out() {
    let td = tempfile::tempdir().unwrap();
    let dir = td.path();

    let caps = write_caps(dir, &["core/store::get"]);
    let out = dir.join("missing-out.gc");

    cmd()
        .current_dir(dir)
        .args(["store", "--caps"])
        .arg(&caps)
        .args(["get"])
        .arg("0000000000000000000000000000000000000000000000000000000000000000")
        .args(["--out"])
        .arg(&out)
        .assert()
        .code(20)
        .stdout(predicate::str::contains("core/store/not-found"));

    assert!(!out.exists());
}

#[test]
fn store_put_hash_matches_between_frontends() {
    let td = tempfile::tempdir().unwrap();
    let dir = td.path();

    let caps = write_caps(
        dir,
        &["core/store::put", "core/store::has", "core/store::get"],
    );
    let artifact = build_selfhost_artifact(dir);

    let inp = dir.join("artifact.gc");
    fs::write(&inp, "{:x 1 :y \"hi\"}\n").unwrap();

    let rust_out = cmd()
        .current_dir(dir)
        .args(["--coreform-frontend", "rust", "store", "--caps"])
        .arg(&caps)
        .args(["put", "--input"])
        .arg(&inp)
        .assert()
        .success()
        .get_output()
        .stdout
        .clone();
    let rust_h = String::from_utf8(rust_out).unwrap().trim().to_string();
    assert!(
        predicate::str::is_match("^[0-9a-f]{64}$")
            .unwrap()
            .eval(&rust_h)
    );

    let self_out = cmd()
        .current_dir(dir)
        .args([
            "--coreform-frontend",
            "selfhost",
            "--selfhost-artifact",
            artifact.to_str().unwrap(),
            "store",
            "--caps",
        ])
        .arg(&caps)
        .args(["put", "--input"])
        .arg(&inp)
        .assert()
        .success()
        .get_output()
        .stdout
        .clone();
    let self_h = String::from_utf8(self_out).unwrap().trim().to_string();
    assert_eq!(rust_h, self_h);
}

#[test]
fn store_put_selfhost_frontend_fails_when_contract_is_poisoned() {
    let td = tempfile::tempdir().unwrap();
    let dir = td.path();

    let caps = write_caps(dir, &["core/store::put"]);
    let artifact = build_selfhost_artifact(dir);
    poison_cli_store_put_program(&artifact);

    let inp = dir.join("artifact.gc");
    fs::write(&inp, "{:x 1}\n").unwrap();

    cmd()
        .current_dir(dir)
        .args([
            "--coreform-frontend",
            "selfhost",
            "--selfhost-artifact",
            artifact.to_str().unwrap(),
            "store",
            "--caps",
        ])
        .arg(&caps)
        .args(["put", "--input"])
        .arg(&inp)
        .assert()
        .failure()
        .code(20)
        .stderr(predicate::str::contains("store-put-program"));
}
