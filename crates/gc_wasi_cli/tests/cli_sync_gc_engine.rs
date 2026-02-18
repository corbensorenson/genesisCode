use std::fs;
use std::path::{Path, PathBuf};

use assert_cmd::cargo::cargo_bin_cmd;
use gc_coreform::{
    Term, TermOrdKey, canonicalize_module, hash_module, parse_module, parse_term, print_term,
};
use predicates::prelude::*;
use tempfile::tempdir;

mod common;

fn cmd() -> assert_cmd::Command {
    let mut c = cargo_bin_cmd!("genesis_wasi");
    c.env("GENESIS_ALLOW_RUST_ENGINE", "1");
    c
}

fn write_caps(dir: &Path, allow: &[&str]) -> PathBuf {
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
    s.push_str(
        "]\n\n[store]\ndir = \"./.genesis/store\"\n\n[refs]\npath = \"./.genesis/refs.gc\"\n",
    );
    fs::write(&caps, s).unwrap();
    caps
}

fn build_selfhost_artifact(dir: &Path) -> PathBuf {
    common::copy_repo_selfhost_toolchain_artifact(dir)
}

fn poison_cli_binding(artifact: &Path, binding: &str) {
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

    let poisoned_src = format!("(def {binding} \"shadowed\")\n");
    let poisoned_forms = canonicalize_module(parse_module(&poisoned_src).unwrap()).unwrap();
    let poisoned_hash = hash_module(&poisoned_forms);
    cli_mod.insert(
        TermOrdKey(Term::symbol(":source")),
        Term::Str(poisoned_src.to_string()),
    );
    cli_mod.insert(
        TermOrdKey(Term::symbol(":module-h")),
        Term::Bytes(poisoned_hash.to_vec().into()),
    );
    cli_mod.insert(
        TermOrdKey(Term::symbol(":forms")),
        Term::Vector(poisoned_forms.clone()),
    );
    fs::write(artifact, print_term(&term)).unwrap();
}

#[test]
fn sync_and_gc_selfhost_frontend_fail_when_contracts_are_poisoned() {
    let td = tempdir().unwrap();
    let dir = td.path();

    let caps = write_caps(dir, &["core/sync::pull", "core/gc::plan"]);
    let artifact = build_selfhost_artifact(dir);

    poison_cli_binding(&artifact, "core/cli::sync-pull-program");
    cmd()
        .current_dir(dir)
        .args([
            "--coreform-frontend",
            "selfhost",
            "--selfhost-artifact",
            artifact.to_str().unwrap(),
            "sync",
            "--caps",
        ])
        .arg(&caps)
        .args([
            "pull",
            "--remote",
            "file:///tmp/",
            "--ref",
            "refs/heads/main",
        ])
        .assert()
        .failure()
        .code(20)
        .stderr(predicate::str::contains("sync-pull-program"));

    poison_cli_binding(&artifact, "core/cli::gc-plan-program");
    cmd()
        .current_dir(dir)
        .args([
            "--coreform-frontend",
            "selfhost",
            "--selfhost-artifact",
            artifact.to_str().unwrap(),
            "gc",
            "--caps",
        ])
        .arg(&caps)
        .args([
            "plan",
            "--lock",
            "genesis.lock",
            "--pins",
            ".genesis/pins.toml",
            "--depth",
            "0",
        ])
        .assert()
        .failure()
        .code(20)
        .stderr(predicate::str::contains("gc-plan-program"));
}
