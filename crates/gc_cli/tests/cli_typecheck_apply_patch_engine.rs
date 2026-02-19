use assert_cmd::cargo::cargo_bin_cmd;
use gc_coreform::{
    Term, TermOrdKey, canonicalize_module, hash_module, parse_module, parse_term, print_term,
};
use predicates::prelude::*;
use serde_json::Value as JsonValue;
use tempfile::tempdir;

mod support;

fn cmd() -> assert_cmd::Command {
    let c = cargo_bin_cmd!("genesis_parity");
    c
}

fn build_selfhost_artifact(dir: &std::path::Path) -> std::path::PathBuf {
    support::copy_repo_toolchain_artifact(dir)
}

fn copy_pkg_basic_fixture(dst: &std::path::Path) -> std::path::PathBuf {
    std::fs::create_dir_all(dst).unwrap();
    let fixture = std::path::Path::new(concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/../../tests/spec/pkg_basic"
    ));
    for name in ["basic.gc", "caps.toml", "package.toml", "pure.gcpatch"] {
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

    let module_src = match cli_mod.get(&TermOrdKey(Term::symbol(":source"))) {
        Some(Term::Str(src)) => src.clone(),
        _ => panic!("cli module missing :source"),
    };
    let poisoned_src =
        format!("{module_src}\n(def core/cli::module-meta (fn (forms) \"bad-meta\"))\n");
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
    std::fs::write(artifact, print_term(&term)).unwrap();
}

fn poison_cli_hash_module_forms_contract(artifact: &std::path::Path) {
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

    let module_src = match cli_mod.get(&TermOrdKey(Term::symbol(":source"))) {
        Some(Term::Str(src)) => src.clone(),
        _ => panic!("cli module missing :source"),
    };
    let poisoned_src =
        format!("{module_src}\n(def core/cli::hash-module-forms (fn (forms) \"bad-hash\"))\n");
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
    std::fs::write(artifact, print_term(&term)).unwrap();
}

fn poison_patch_schema_apply_replace_node_contract(artifact: &std::path::Path) {
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
    let patch_mod = entries
        .iter_mut()
        .find_map(|entry| match entry {
            Term::Map(mm)
                if matches!(
                    mm.get(&TermOrdKey(Term::symbol(":path"))),
                    Some(Term::Str(path)) if path == "selfhost/patch_schema_v1.gc"
                ) =>
            {
                Some(mm)
            }
            _ => None,
        })
        .expect("selfhost/patch_schema_v1.gc entry");

    let poisoned_src = r#"
      (def core/cli::validate-patch (fn (t) true))
      (def core/cli::apply-replace-node
        (fn (req) ((core/error::make2 "core/poison") "apply-replace-node poisoned")))
      (def core/cli::print-module-forms (fn (forms) ""))
      (def core/cli::print-module-from-content
        (fn (content) ((core/error::make2 "core/poison") "print-module-from-content poisoned")))
      (def core/cli::manifest-apply-add-module
        (fn (manifest)
          (fn (module-path) ((core/error::make2 "core/poison") "manifest-apply-add-module poisoned"))))
      (def core/cli::manifest-apply-remove-module
        (fn (manifest)
          (fn (module-path)
            ((core/error::make2 "core/poison") "manifest-apply-remove-module poisoned"))))
      (def core/cli::manifest-apply-update-manifest-op
        (fn (manifest)
          (fn (op)
            ((core/error::make2 "core/poison") "manifest-apply-update-manifest-op poisoned"))))
    "#;
    let poisoned_forms = canonicalize_module(parse_module(&poisoned_src).unwrap()).unwrap();
    let poisoned_hash = hash_module(&poisoned_forms);
    patch_mod.insert(
        TermOrdKey(Term::symbol(":source")),
        Term::Str(poisoned_src.to_string()),
    );
    patch_mod.insert(
        TermOrdKey(Term::symbol(":forms")),
        Term::Vector(poisoned_forms),
    );
    patch_mod.insert(
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
fn apply_patch_selfhost_frontend_matches_rust_frontend_artifacts() {
    let td = tempdir().unwrap();
    let artifact = build_selfhost_artifact(td.path());
    let rust_dir = td.path().join("pkg_rust");
    let self_dir = td.path().join("pkg_selfhost");
    let rust_pkg = copy_pkg_basic_fixture(&rust_dir);
    let self_pkg = copy_pkg_basic_fixture(&self_dir);
    let rust_patch = rust_dir.join("pure.gcpatch");
    let self_patch = self_dir.join("pure.gcpatch");

    let rust_v = run_json(&[
        "--json",
        "--coreform-frontend",
        "rust",
        "apply-patch",
        rust_patch.to_str().unwrap(),
        "--pkg",
        rust_pkg.to_str().unwrap(),
    ]);
    let self_v = run_json(&[
        "--json",
        "--coreform-frontend",
        "selfhost",
        "--selfhost-artifact",
        artifact.to_str().unwrap(),
        "apply-patch",
        self_patch.to_str().unwrap(),
        "--pkg",
        self_pkg.to_str().unwrap(),
    ]);

    let rust_data = rust_v.get("data").unwrap();
    let self_data = self_v.get("data").unwrap();
    for key in [
        "patch_artifact",
        "report_artifact",
        "acceptance_artifact",
        "package_artifact",
    ] {
        assert_eq!(
            rust_data.get(key).and_then(JsonValue::as_str),
            self_data.get(key).and_then(JsonValue::as_str),
            "engine mismatch for {key}"
        );
    }
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

#[test]
fn apply_patch_selfhost_frontend_fails_when_apply_replace_node_contract_is_poisoned() {
    let td = tempdir().unwrap();
    let artifact = build_selfhost_artifact(td.path());
    poison_patch_schema_apply_replace_node_contract(&artifact);

    let pkg_dir = td.path().join("pkg_selfhost_poison_replace");
    let pkg = copy_pkg_basic_fixture(&pkg_dir);
    let patch = pkg_dir.join("pure.gcpatch");

    cmd()
        .args([
            "--coreform-frontend",
            "selfhost",
            "--selfhost-artifact",
            artifact.to_str().unwrap(),
            "apply-patch",
            patch.to_str().unwrap(),
            "--pkg",
            pkg.to_str().unwrap(),
        ])
        .assert()
        .failure()
        .code(10)
        .stderr(predicate::str::contains("apply-replace-node"));
}

#[test]
fn typecheck_selfhost_frontend_fails_when_hash_module_forms_contract_is_poisoned() {
    let td = tempdir().unwrap();
    let artifact = build_selfhost_artifact(td.path());
    poison_cli_hash_module_forms_contract(&artifact);
    let pkg = copy_pkg_basic_fixture(&td.path().join("pkg_selfhost_bad_hash"));

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
        .stderr(predicate::str::contains("hash-module-forms"));
}
