use std::fs;
use std::path::{Path, PathBuf};

use assert_cmd::cargo::cargo_bin_cmd;
use gc_coreform::{
    Term, TermOrdKey, canonicalize_module, hash_module, parse_module, parse_term, print_term,
};
use predicates::prelude::*;

fn cmd() -> assert_cmd::Command {
    let mut c = cargo_bin_cmd!("genesis");
    c.env("GENESIS_ALLOW_RUST_ENGINE", "1");
    c
}

fn write_caps(dir: &Path) -> PathBuf {
    let caps = dir.join("caps.toml");
    fs::write(
        &caps,
        r#"
allow = [
  "core/pkg::init"
]

[op."core/pkg::init"]
base_dir = "."
"#,
    )
    .unwrap();
    caps
}

fn build_selfhost_artifact(dir: &Path) -> PathBuf {
    let artifact = dir.join("selfhost_toolchain.gc");
    cmd()
        .args(["selfhost-artifact", "--out"])
        .arg(&artifact)
        .assert()
        .success();
    artifact
}

fn poison_cli_pkg_init_program(artifact: &Path) {
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

    let poisoned_src = "(def core/cli::pkg-init-program \"shadowed\")\n";
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
    fs::write(artifact, print_term(&term)).unwrap();
}

fn json_value(stdout: &[u8]) -> String {
    let v: serde_json::Value = serde_json::from_slice(stdout).unwrap();
    v.get("data")
        .and_then(|d| d.get("value"))
        .and_then(|x| x.as_str())
        .unwrap()
        .to_string()
}

fn json_frontend_name(stdout: &[u8]) -> String {
    let v: serde_json::Value = serde_json::from_slice(stdout).unwrap();
    v.get("data")
        .and_then(|d| d.get("coreform_frontend"))
        .and_then(|cf| cf.get("name"))
        .and_then(|x| x.as_str())
        .unwrap()
        .to_string()
}

#[test]
fn pkg_init_value_matches_between_frontends() {
    let td = tempfile::tempdir().unwrap();
    let dir = td.path();
    let caps = write_caps(dir);
    let artifact = build_selfhost_artifact(dir);

    let lock = dir.join("genesis.lock");

    let rust_out = cmd()
        .current_dir(dir)
        .arg("--json")
        .args(["--coreform-frontend", "rust"])
        .args(["pkg", "--caps"])
        .arg(&caps)
        .args(["--log"])
        .arg(dir.join("rust.gclog"))
        .args(["init", "--workspace", "w", "--lock"])
        .arg(&lock)
        .args(["--policy", "policy:default-v0.1"])
        .assert()
        .success()
        .get_output()
        .stdout
        .clone();
    assert_eq!(json_frontend_name(&rust_out), "rust");
    let rust_v = json_value(&rust_out);

    let self_out = cmd()
        .current_dir(dir)
        .arg("--json")
        .args(["--coreform-frontend", "selfhost"])
        .args(["--selfhost-artifact", artifact.to_str().unwrap()])
        .args(["pkg", "--caps"])
        .arg(&caps)
        .args(["--log"])
        .arg(dir.join("self.gclog"))
        .args(["init", "--workspace", "w", "--lock"])
        .arg(&lock)
        .args(["--policy", "policy:default-v0.1"])
        .assert()
        .success()
        .get_output()
        .stdout
        .clone();
    assert_eq!(json_frontend_name(&self_out), "selfhost");
    let self_v = json_value(&self_out);

    assert_eq!(rust_v, self_v);
}

#[test]
fn pkg_init_selfhost_frontend_fails_when_contract_is_poisoned() {
    let td = tempfile::tempdir().unwrap();
    let dir = td.path();
    let caps = write_caps(dir);
    let artifact = build_selfhost_artifact(dir);
    poison_cli_pkg_init_program(&artifact);

    let lock = dir.join("genesis.lock");

    cmd()
        .current_dir(dir)
        .args([
            "--coreform-frontend",
            "selfhost",
            "--selfhost-artifact",
            artifact.to_str().unwrap(),
            "pkg",
            "--caps",
        ])
        .arg(&caps)
        .args(["init", "--workspace", "w", "--lock"])
        .arg(&lock)
        .args(["--policy", "policy:default-v0.1"])
        .assert()
        .failure()
        .code(20)
        .stderr(predicate::str::contains("pkg-init-program"));
}

