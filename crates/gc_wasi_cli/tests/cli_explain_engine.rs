use assert_cmd::cargo::cargo_bin_cmd;
use gc_coreform::{Term, TermOrdKey, parse_term};
use predicates::prelude::*;
use tempfile::tempdir;

mod common;

fn cmd() -> assert_cmd::Command {
    let mut c = cargo_bin_cmd!("genesis_wasi");
    c.env("GENESIS_ALLOW_RUST_ENGINE", "1");
    c
}

fn build_selfhost_artifact(dir: &std::path::Path) -> std::path::PathBuf {
    common::copy_repo_selfhost_toolchain_artifact(dir)
}

#[test]
fn explain_selfhost_engine_matches_rust_engine_output() {
    let dir = tempdir().unwrap();
    let file = dir.path().join("m.gc");
    let artifact = build_selfhost_artifact(dir.path());

    std::fs::write(
        &file,
        r#"
          (def c (core/contract::make (fn (msg) nil) nil {}))
          c
        "#,
    )
    .unwrap();

    let rust_out = cmd()
        .args([
            "explain",
            file.to_str().unwrap(),
            "--engine",
            "rust",
            "--contract",
            "c",
            "--msg",
            "(msg foo nil)",
        ])
        .assert()
        .success()
        .get_output()
        .stdout
        .clone();

    let selfhost_out = cmd()
        .args([
            "--no-step-limit",
            "--selfhost-artifact",
            artifact.to_str().unwrap(),
            "explain",
            file.to_str().unwrap(),
            "--engine",
            "selfhost",
            "--contract",
            "c",
            "--msg",
            "(msg foo nil)",
        ])
        .assert()
        .success()
        .get_output()
        .stdout
        .clone();

    let rust_s = String::from_utf8(rust_out).unwrap();
    let selfhost_s = String::from_utf8(selfhost_out).unwrap();

    let rust_t = parse_term(rust_s.trim()).unwrap();
    let selfhost_t = parse_term(selfhost_s.trim()).unwrap();
    let (Term::Map(rm), Term::Map(sm)) = (&rust_t, &selfhost_t) else {
        panic!("explain outputs must be map terms");
    };

    let k_op = TermOrdKey(Term::symbol(":op"));
    let k_result = TermOrdKey(Term::symbol(":result"));
    let k_steps = TermOrdKey(Term::symbol(":steps"));
    assert_eq!(rm.get(&k_op), sm.get(&k_op));
    assert_eq!(rm.get(&k_result), sm.get(&k_result));

    let (Some(Term::Vector(rsteps)), Some(Term::Vector(ssteps))) =
        (rm.get(&k_steps), sm.get(&k_steps))
    else {
        panic!("explain outputs must contain :steps vector");
    };
    assert_eq!(rsteps.len(), ssteps.len());
    assert_eq!(rsteps.len(), 1);

    let (Term::Map(rstep), Term::Map(sstep)) = (&rsteps[0], &ssteps[0]) else {
        panic!("first :steps entries must be map terms");
    };
    for key in [":shape-id", ":has-proto", ":override", ":unhandled"] {
        let k = TermOrdKey(Term::symbol(key));
        assert_eq!(
            rstep.get(&k),
            sstep.get(&k),
            "engine mismatch for explain step field {key}"
        );
    }
    let k_contract_id = TermOrdKey(Term::symbol(":contract-id"));
    let (Some(Term::Str(rid)), Some(Term::Str(sid))) =
        (rstep.get(&k_contract_id), sstep.get(&k_contract_id))
    else {
        panic!(":contract-id must be string");
    };
    assert_eq!(
        rid, sid,
        "engine mismatch for explain step field :contract-id"
    );
    assert_eq!(rid.len(), 64);
    assert!(rid.chars().all(|c| c.is_ascii_hexdigit()));
}

#[test]
fn explain_selfhost_engine_surfaces_parse_errors() {
    let dir = tempdir().unwrap();
    let file = dir.path().join("m.gc");
    let artifact = build_selfhost_artifact(dir.path());

    std::fs::write(
        &file,
        r#"
          (def c (core/contract::make (fn (msg) nil) nil {}))
          c
        "#,
    )
    .unwrap();

    cmd()
        .args([
            "--no-step-limit",
            "--selfhost-artifact",
            artifact.to_str().unwrap(),
            "explain",
            file.to_str().unwrap(),
            "--engine",
            "selfhost",
            "--contract",
            "c",
            "--msg",
            "(msg foo",
        ])
        .assert()
        .failure()
        .code(10)
        .stderr(predicate::str::contains("core/parse/"));
}
