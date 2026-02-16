use std::fs;

use assert_cmd::cargo::cargo_bin_cmd;
use gc_coreform::{Term, TermOrdKey, parse_term, print_term};
use tempfile::tempdir;

fn map_get<'a>(m: &'a std::collections::BTreeMap<TermOrdKey, Term>, k: &str) -> Option<&'a Term> {
    m.get(&TermOrdKey(Term::symbol(k)))
}

#[test]
fn selfhost_artifact_can_be_built_and_used_for_selfhost_fmt() {
    let td = tempdir().unwrap();
    let artifact = td.path().join("selfhost_toolchain.gc");
    let file = td.path().join("m.gc");

    cargo_bin_cmd!("genesis")
        .args(["selfhost-artifact", "--out"])
        .arg(&artifact)
        .assert()
        .success();

    let artifact_s = fs::read_to_string(&artifact).unwrap();
    let term = parse_term(&artifact_s).unwrap();
    let Term::Map(root) = term else {
        panic!("artifact must be a map");
    };
    assert!(matches!(
        map_get(&root, ":kind"),
        Some(Term::Str(s)) if s == "genesis/selfhost-toolchain-artifact-v0.2"
    ));
    assert!(matches!(map_get(&root, ":v"), Some(Term::Int(i)) if i == &1.into()));

    let src = r#"
      (def x (prim int/add 1 2))
      x
    "#;
    fs::write(&file, src).unwrap();
    cargo_bin_cmd!("genesis")
        .args(["fmt", "--engine", "rust"])
        .arg(&file)
        .assert()
        .success();
    let expected = fs::read_to_string(&file).unwrap();

    fs::write(&file, src).unwrap();
    cargo_bin_cmd!("genesis")
        .env("GENESIS_SELFHOST_TOOLCHAIN_ARTIFACT", &artifact)
        .args(["fmt", "--engine", "selfhost"])
        .arg(&file)
        .assert()
        .success();
    let actual = fs::read_to_string(&file).unwrap();
    assert_eq!(expected, actual);
}

#[test]
fn invalid_selfhost_artifact_is_rejected_by_loader() {
    let td = tempdir().unwrap();
    let artifact = td.path().join("selfhost_toolchain.gc");
    let file = td.path().join("m.gc");

    cargo_bin_cmd!("genesis")
        .args(["selfhost-artifact", "--out"])
        .arg(&artifact)
        .assert()
        .success();

    let artifact_s = fs::read_to_string(&artifact).unwrap();
    let mut term = parse_term(&artifact_s).unwrap();
    let Term::Map(root) = &mut term else {
        panic!("artifact must be map");
    };
    let modules = root
        .get_mut(&TermOrdKey(Term::symbol(":modules")))
        .expect("modules");
    let Term::Vector(mods) = modules else {
        panic!("modules must be vector");
    };
    let Term::Map(first) = mods.first_mut().expect("first module") else {
        panic!("first module must be map");
    };
    first.insert(
        TermOrdKey(Term::symbol(":module-h")),
        Term::Bytes(vec![0u8; 32].into()),
    );
    fs::write(&artifact, print_term(&term)).unwrap();

    fs::write(&file, "(def x 1)\nx\n").unwrap();
    cargo_bin_cmd!("genesis")
        .env("GENESIS_SELFHOST_TOOLCHAIN_ARTIFACT", &artifact)
        .args(["fmt", "--engine", "selfhost"])
        .arg(&file)
        .assert()
        .failure()
        .code(1);
}
