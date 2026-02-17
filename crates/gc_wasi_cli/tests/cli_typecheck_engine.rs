use assert_cmd::cargo::cargo_bin_cmd;
use gc_coreform::{
    Term, TermOrdKey, canonicalize_module, hash_module, parse_module, parse_term, print_term,
};
use predicates::prelude::*;
use serde_json::Value as JsonValue;
use tempfile::tempdir;

fn cmd() -> assert_cmd::Command {
    let mut c = cargo_bin_cmd!("genesis_wasi");
    c.env("GENESIS_ALLOW_RUST_ENGINE", "1");
    c
}

fn build_selfhost_artifact(dir: &std::path::Path) -> std::path::PathBuf {
    let artifact = dir.join("selfhost_toolchain.gc");
    cmd()
        .args(["selfhost-artifact", "--out"])
        .arg(&artifact)
        .assert()
        .success();
    artifact
}

fn copy_pkg_basic_fixture(dst: &std::path::Path) -> std::path::PathBuf {
    std::fs::create_dir_all(dst).unwrap();
    let fixture = std::path::Path::new(concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/../../tests/spec/pkg_basic"
    ));
    for name in ["basic.gc", "caps.toml", "package.toml"] {
        std::fs::copy(fixture.join(name), dst.join(name)).unwrap();
    }
    dst.join("package.toml")
}

fn run_json(args: &[&str]) -> JsonValue {
    let out = cmd()
        .args(args)
        .assert()
        .success()
        .get_output()
        .stdout
        .clone();
    serde_json::from_slice(&out).unwrap()
}

fn poison_cli_module_meta_contract(artifact: &std::path::Path) {
    let src = std::fs::read_to_string(artifact).unwrap();
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

    let poisoned_src = "(def core/cli::module-meta (fn (forms) \"bad-meta\"))\n";
    let poisoned_forms = canonicalize_module(parse_module(poisoned_src).unwrap()).unwrap();
    let poisoned_hash = hash_module(&poisoned_forms);
    cli_mod.insert(
        TermOrdKey(Term::symbol(":source")),
        Term::Str(poisoned_src.to_string()),
    );
    cli_mod.insert(
        TermOrdKey(Term::symbol(":module-h")),
        Term::Bytes(poisoned_hash.to_vec().into()),
    );
    std::fs::write(artifact, print_term(&term)).unwrap();
}

#[test]
fn typecheck_selfhost_frontend_matches_rust_frontend_report() {
    let td = tempdir().unwrap();
    let artifact = build_selfhost_artifact(td.path());
    let rust_pkg = copy_pkg_basic_fixture(&td.path().join("pkg_rust"));
    let self_pkg = copy_pkg_basic_fixture(&td.path().join("pkg_selfhost"));

    let rust_v = run_json(&[
        "--json",
        "--coreform-frontend",
        "rust",
        "typecheck",
        "--pkg",
        rust_pkg.to_str().unwrap(),
    ]);
    let self_v = run_json(&[
        "--json",
        "--coreform-frontend",
        "selfhost",
        "--selfhost-artifact",
        artifact.to_str().unwrap(),
        "typecheck",
        "--pkg",
        self_pkg.to_str().unwrap(),
    ]);

    let rust_report = rust_v
        .get("data")
        .and_then(|d| d.get("report_coreform"))
        .and_then(JsonValue::as_str)
        .unwrap();
    let self_report = self_v
        .get("data")
        .and_then(|d| d.get("report_coreform"))
        .and_then(JsonValue::as_str)
        .unwrap();
    assert_eq!(rust_report, self_report);
    assert_eq!(
        rust_v["data"]["coreform_frontend"]["name"].as_str(),
        Some("rust")
    );
    assert_eq!(
        self_v["data"]["coreform_frontend"]["name"].as_str(),
        Some("selfhost")
    );
}

#[test]
fn typecheck_selfhost_frontend_fails_when_module_meta_contract_is_poisoned() {
    let td = tempdir().unwrap();
    let artifact = build_selfhost_artifact(td.path());
    poison_cli_module_meta_contract(&artifact);
    let pkg = copy_pkg_basic_fixture(&td.path().join("pkg_selfhost_bad_meta"));

    cmd()
        .args([
            "--coreform-frontend",
            "selfhost",
            "--selfhost-artifact",
            artifact.to_str().unwrap(),
            "typecheck",
            "--pkg",
            pkg.to_str().unwrap(),
        ])
        .assert()
        .failure()
        .code(10)
        .stderr(predicate::str::contains("module-meta"));
}
