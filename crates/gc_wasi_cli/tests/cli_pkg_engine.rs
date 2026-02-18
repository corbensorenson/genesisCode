use std::fs;
use std::path::{Path, PathBuf};

use assert_cmd::cargo::cargo_bin_cmd;
use gc_coreform::{
    Term, TermOrdKey, canonicalize_module, hash_module, parse_module, parse_term, print_term,
};
use predicates::prelude::*;

mod common;

fn cmd() -> assert_cmd::Command {
    let mut c = cargo_bin_cmd!("genesis_wasi");
    c.env("GENESIS_ALLOW_RUST_ENGINE", "1");
    c
}

fn write_caps(dir: &Path) -> PathBuf {
    let caps = dir.join("caps.toml");
    fs::write(
        &caps,
        r#"
allow = [
  "core/store::put",
  "core/pkg::init",
  "core/pkg::add",
  "core/pkg::list",
  "core/pkg::info",
  "core/pkg-low::save-lock",
  "core/pkg-low::load-lock",
  "core/pkg::lock",
  "core/pkg::update",
  "core/pkg::install",
  "core/pkg::verify",
  "core/pkg::snapshot",
  "core/store::has",
  "core/store::get"
]

[store]
dir = "./.genesis/store"

[refs]
path = "./.genesis/refs.gc"

[op."core/pkg::init"]
base_dir = "."
[op."core/pkg::add"]
base_dir = "."
[op."core/pkg::list"]
base_dir = "."
[op."core/pkg::info"]
base_dir = "."
[op."core/pkg-low::save-lock"]
base_dir = "."
[op."core/pkg-low::load-lock"]
base_dir = "."
[op."core/pkg::lock"]
base_dir = "."
[op."core/pkg::update"]
base_dir = "."
[op."core/pkg::install"]
base_dir = "."
[op."core/pkg::verify"]
base_dir = "."
[op."core/pkg::snapshot"]
base_dir = "."
"#,
    )
    .unwrap();
    caps
}

fn build_selfhost_artifact(dir: &Path) -> PathBuf {
    common::copy_repo_selfhost_toolchain_artifact(dir)
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

    let module_src = match cli_mod.get(&TermOrdKey(Term::symbol(":source"))) {
        Some(Term::Str(src)) => src.clone(),
        _ => panic!("cli module missing :source"),
    };
    let poisoned_src = format!("{module_src}\n(def core/cli::pkg-init-program \"shadowed\")\n");
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

fn normalize_pkg_value(s: &str) -> Term {
    fn walk(t: &Term) -> Term {
        match t {
            Term::Map(m) => {
                let mut out = std::collections::BTreeMap::new();
                for (k, v) in m {
                    if k.0 == Term::symbol(":lock") {
                        continue;
                    }
                    out.insert(k.clone(), walk(v));
                }
                Term::Map(out)
            }
            Term::Vector(xs) => Term::Vector(xs.iter().map(walk).collect()),
            Term::Pair(a, d) => Term::Pair(Box::new(walk(a)), Box::new(walk(d))),
            other => other.clone(),
        }
    }
    walk(&parse_term(s).unwrap())
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

#[test]
fn pkg_add_list_info_values_match_between_frontends() {
    let td = tempfile::tempdir().unwrap();
    let rust_dir = td.path().join("rust");
    let self_dir = td.path().join("self");
    fs::create_dir_all(&rust_dir).unwrap();
    fs::create_dir_all(&self_dir).unwrap();

    let rust_caps = write_caps(&rust_dir);
    let self_caps = write_caps(&self_dir);
    let artifact = build_selfhost_artifact(&self_dir);

    let rust_lock = rust_dir.join("genesis.lock");
    let self_lock = self_dir.join("genesis.lock");

    let rust_init = cmd()
        .current_dir(&rust_dir)
        .arg("--json")
        .args(["--coreform-frontend", "rust"])
        .args(["pkg", "--caps"])
        .arg(&rust_caps)
        .args(["init", "--workspace", "w", "--lock"])
        .arg(&rust_lock)
        .args(["--policy", "policy:default-v0.1"])
        .assert()
        .success()
        .get_output()
        .stdout
        .clone();
    let self_init = cmd()
        .current_dir(&self_dir)
        .arg("--json")
        .args(["--coreform-frontend", "selfhost"])
        .args(["--selfhost-artifact", artifact.to_str().unwrap()])
        .args(["pkg", "--caps"])
        .arg(&self_caps)
        .args(["init", "--workspace", "w", "--lock"])
        .arg(&self_lock)
        .args(["--policy", "policy:default-v0.1"])
        .assert()
        .success()
        .get_output()
        .stdout
        .clone();
    assert_eq!(json_frontend_name(&rust_init), "rust");
    assert_eq!(json_frontend_name(&self_init), "selfhost");
    assert_eq!(
        normalize_pkg_value(&json_value(&rust_init)),
        normalize_pkg_value(&json_value(&self_init))
    );

    let rust_add = cmd()
        .current_dir(&rust_dir)
        .arg("--json")
        .args(["--coreform-frontend", "rust"])
        .args(["pkg", "--caps"])
        .arg(&rust_caps)
        .args(["add", "dep@refs/heads/main", "--lock"])
        .arg(&rust_lock)
        .args(["--update-policy", "auto", "--registry", "default"])
        .assert()
        .success()
        .get_output()
        .stdout
        .clone();
    let self_add = cmd()
        .current_dir(&self_dir)
        .arg("--json")
        .args(["--coreform-frontend", "selfhost"])
        .args(["--selfhost-artifact", artifact.to_str().unwrap()])
        .args(["pkg", "--caps"])
        .arg(&self_caps)
        .args(["add", "dep@refs/heads/main", "--lock"])
        .arg(&self_lock)
        .args(["--update-policy", "auto", "--registry", "default"])
        .assert()
        .success()
        .get_output()
        .stdout
        .clone();
    assert_eq!(json_frontend_name(&rust_add), "rust");
    assert_eq!(json_frontend_name(&self_add), "selfhost");
    assert_eq!(
        normalize_pkg_value(&json_value(&rust_add)),
        normalize_pkg_value(&json_value(&self_add))
    );

    let rust_list = cmd()
        .current_dir(&rust_dir)
        .arg("--json")
        .args(["--coreform-frontend", "rust"])
        .args(["pkg", "--caps"])
        .arg(&rust_caps)
        .args(["list", "--lock"])
        .arg(&rust_lock)
        .assert()
        .success()
        .get_output()
        .stdout
        .clone();
    let self_list = cmd()
        .current_dir(&self_dir)
        .arg("--json")
        .args(["--coreform-frontend", "selfhost"])
        .args(["--selfhost-artifact", artifact.to_str().unwrap()])
        .args(["pkg", "--caps"])
        .arg(&self_caps)
        .args(["list", "--lock"])
        .arg(&self_lock)
        .assert()
        .success()
        .get_output()
        .stdout
        .clone();
    assert_eq!(json_frontend_name(&rust_list), "rust");
    assert_eq!(json_frontend_name(&self_list), "selfhost");
    assert_eq!(
        normalize_pkg_value(&json_value(&rust_list)),
        normalize_pkg_value(&json_value(&self_list))
    );

    let rust_info = cmd()
        .current_dir(&rust_dir)
        .arg("--json")
        .args(["--coreform-frontend", "rust"])
        .args(["pkg", "--caps"])
        .arg(&rust_caps)
        .args(["info", "dep", "--lock"])
        .arg(&rust_lock)
        .assert()
        .success()
        .get_output()
        .stdout
        .clone();
    let self_info = cmd()
        .current_dir(&self_dir)
        .arg("--json")
        .args(["--coreform-frontend", "selfhost"])
        .args(["--selfhost-artifact", artifact.to_str().unwrap()])
        .args(["pkg", "--caps"])
        .arg(&self_caps)
        .args(["info", "dep", "--lock"])
        .arg(&self_lock)
        .assert()
        .success()
        .get_output()
        .stdout
        .clone();
    assert_eq!(json_frontend_name(&rust_info), "rust");
    assert_eq!(json_frontend_name(&self_info), "selfhost");
    assert_eq!(
        normalize_pkg_value(&json_value(&rust_info)),
        normalize_pkg_value(&json_value(&self_info))
    );
}

#[test]
fn pkg_lock_value_matches_between_frontends() {
    let td = tempfile::tempdir().unwrap();
    let rust_dir = td.path().join("rust");
    let self_dir = td.path().join("self");
    fs::create_dir_all(&rust_dir).unwrap();
    fs::create_dir_all(&self_dir).unwrap();

    let rust_caps = write_caps(&rust_dir);
    let self_caps = write_caps(&self_dir);
    let artifact = build_selfhost_artifact(&self_dir);

    let rust_lock = rust_dir.join("genesis.lock");
    let self_lock = self_dir.join("genesis.lock");
    let snap_h = "0123456789abcdef0123456789abcdef0123456789abcdef0123456789abcdef";

    let rust_init = cmd()
        .current_dir(&rust_dir)
        .arg("--json")
        .args(["--coreform-frontend", "rust"])
        .args(["pkg", "--caps"])
        .arg(&rust_caps)
        .args(["init", "--workspace", "w", "--lock"])
        .arg(&rust_lock)
        .args(["--policy", "policy:default-v0.1"])
        .assert()
        .success()
        .get_output()
        .stdout
        .clone();
    let self_init = cmd()
        .current_dir(&self_dir)
        .arg("--json")
        .args(["--coreform-frontend", "selfhost"])
        .args(["--selfhost-artifact", artifact.to_str().unwrap()])
        .args(["pkg", "--caps"])
        .arg(&self_caps)
        .args(["init", "--workspace", "w", "--lock"])
        .arg(&self_lock)
        .args(["--policy", "policy:default-v0.1"])
        .assert()
        .success()
        .get_output()
        .stdout
        .clone();
    assert_eq!(json_frontend_name(&rust_init), "rust");
    assert_eq!(json_frontend_name(&self_init), "selfhost");

    let rust_add = cmd()
        .current_dir(&rust_dir)
        .arg("--json")
        .args(["--coreform-frontend", "rust"])
        .args(["pkg", "--caps"])
        .arg(&rust_caps)
        .args(["add"])
        .arg(format!("dep@snapshot:{snap_h}"))
        .args(["--lock"])
        .arg(&rust_lock)
        .assert()
        .success()
        .get_output()
        .stdout
        .clone();
    let self_add = cmd()
        .current_dir(&self_dir)
        .arg("--json")
        .args(["--coreform-frontend", "selfhost"])
        .args(["--selfhost-artifact", artifact.to_str().unwrap()])
        .args(["pkg", "--caps"])
        .arg(&self_caps)
        .args(["add"])
        .arg(format!("dep@snapshot:{snap_h}"))
        .args(["--lock"])
        .arg(&self_lock)
        .assert()
        .success()
        .get_output()
        .stdout
        .clone();
    assert_eq!(json_frontend_name(&rust_add), "rust");
    assert_eq!(json_frontend_name(&self_add), "selfhost");

    let rust_lock_out = cmd()
        .current_dir(&rust_dir)
        .arg("--json")
        .args(["--coreform-frontend", "rust"])
        .args(["pkg", "--caps"])
        .arg(&rust_caps)
        .args(["lock", "--lock"])
        .arg(&rust_lock)
        .assert()
        .success()
        .get_output()
        .stdout
        .clone();
    let self_lock_out = cmd()
        .current_dir(&self_dir)
        .arg("--json")
        .args(["--coreform-frontend", "selfhost"])
        .args(["--selfhost-artifact", artifact.to_str().unwrap()])
        .args(["pkg", "--caps"])
        .arg(&self_caps)
        .args(["lock", "--lock"])
        .arg(&self_lock)
        .assert()
        .success()
        .get_output()
        .stdout
        .clone();

    assert_eq!(json_frontend_name(&rust_lock_out), "rust");
    assert_eq!(json_frontend_name(&self_lock_out), "selfhost");
    assert_eq!(
        normalize_pkg_value(&json_value(&rust_lock_out)),
        normalize_pkg_value(&json_value(&self_lock_out))
    );
}

#[test]
fn pkg_update_value_matches_between_frontends() {
    let td = tempfile::tempdir().unwrap();
    let rust_dir = td.path().join("rust");
    let self_dir = td.path().join("self");
    fs::create_dir_all(&rust_dir).unwrap();
    fs::create_dir_all(&self_dir).unwrap();

    let rust_caps = write_caps(&rust_dir);
    let self_caps = write_caps(&self_dir);
    let artifact = build_selfhost_artifact(&self_dir);

    let rust_lock = rust_dir.join("genesis.lock");
    let self_lock = self_dir.join("genesis.lock");

    let rust_init = cmd()
        .current_dir(&rust_dir)
        .arg("--json")
        .args(["--coreform-frontend", "rust"])
        .args(["pkg", "--caps"])
        .arg(&rust_caps)
        .args(["init", "--workspace", "w", "--lock"])
        .arg(&rust_lock)
        .args(["--policy", "policy:default-v0.1"])
        .assert()
        .success()
        .get_output()
        .stdout
        .clone();
    let self_init = cmd()
        .current_dir(&self_dir)
        .arg("--json")
        .args(["--coreform-frontend", "selfhost"])
        .args(["--selfhost-artifact", artifact.to_str().unwrap()])
        .args(["pkg", "--caps"])
        .arg(&self_caps)
        .args(["init", "--workspace", "w", "--lock"])
        .arg(&self_lock)
        .args(["--policy", "policy:default-v0.1"])
        .assert()
        .success()
        .get_output()
        .stdout
        .clone();
    assert_eq!(json_frontend_name(&rust_init), "rust");
    assert_eq!(json_frontend_name(&self_init), "selfhost");

    let rust_update = cmd()
        .current_dir(&rust_dir)
        .arg("--json")
        .args(["--coreform-frontend", "rust"])
        .args(["pkg", "--caps"])
        .arg(&rust_caps)
        .args(["update", "--lock"])
        .arg(&rust_lock)
        .assert()
        .success()
        .get_output()
        .stdout
        .clone();
    let self_update = cmd()
        .current_dir(&self_dir)
        .arg("--json")
        .args(["--coreform-frontend", "selfhost"])
        .args(["--selfhost-artifact", artifact.to_str().unwrap()])
        .args(["pkg", "--caps"])
        .arg(&self_caps)
        .args(["update", "--lock"])
        .arg(&self_lock)
        .assert()
        .success()
        .get_output()
        .stdout
        .clone();
    assert_eq!(json_frontend_name(&rust_update), "rust");
    assert_eq!(json_frontend_name(&self_update), "selfhost");
    assert_eq!(
        normalize_pkg_value(&json_value(&rust_update)),
        normalize_pkg_value(&json_value(&self_update))
    );
}

#[test]
fn pkg_install_verify_values_match_between_frontends() {
    let td = tempfile::tempdir().unwrap();
    let rust_dir = td.path().join("rust");
    let self_dir = td.path().join("self");
    fs::create_dir_all(&rust_dir).unwrap();
    fs::create_dir_all(&self_dir).unwrap();

    let rust_caps = write_caps(&rust_dir);
    let self_caps = write_caps(&self_dir);
    let artifact = build_selfhost_artifact(&self_dir);

    let rust_lock = rust_dir.join("genesis.lock");
    let self_lock = self_dir.join("genesis.lock");

    let rust_init = cmd()
        .current_dir(&rust_dir)
        .arg("--json")
        .args(["--coreform-frontend", "rust"])
        .args(["pkg", "--caps"])
        .arg(&rust_caps)
        .args(["init", "--workspace", "w", "--lock"])
        .arg(&rust_lock)
        .args(["--policy", "policy:default-v0.1"])
        .assert()
        .success()
        .get_output()
        .stdout
        .clone();
    let self_init = cmd()
        .current_dir(&self_dir)
        .arg("--json")
        .args(["--coreform-frontend", "selfhost"])
        .args(["--selfhost-artifact", artifact.to_str().unwrap()])
        .args(["pkg", "--caps"])
        .arg(&self_caps)
        .args(["init", "--workspace", "w", "--lock"])
        .arg(&self_lock)
        .args(["--policy", "policy:default-v0.1"])
        .assert()
        .success()
        .get_output()
        .stdout
        .clone();
    assert_eq!(json_frontend_name(&rust_init), "rust");
    assert_eq!(json_frontend_name(&self_init), "selfhost");

    let rust_install = cmd()
        .current_dir(&rust_dir)
        .arg("--json")
        .args(["--coreform-frontend", "rust"])
        .args(["pkg", "--caps"])
        .arg(&rust_caps)
        .args(["install", "--lock"])
        .arg(&rust_lock)
        .assert()
        .success()
        .get_output()
        .stdout
        .clone();
    let self_install = cmd()
        .current_dir(&self_dir)
        .arg("--json")
        .args(["--coreform-frontend", "selfhost"])
        .args(["--selfhost-artifact", artifact.to_str().unwrap()])
        .args(["pkg", "--caps"])
        .arg(&self_caps)
        .args(["install", "--lock"])
        .arg(&self_lock)
        .assert()
        .success()
        .get_output()
        .stdout
        .clone();
    assert_eq!(json_frontend_name(&rust_install), "rust");
    assert_eq!(json_frontend_name(&self_install), "selfhost");
    assert_eq!(
        normalize_pkg_value(&json_value(&rust_install)),
        normalize_pkg_value(&json_value(&self_install))
    );

    let rust_verify = cmd()
        .current_dir(&rust_dir)
        .arg("--json")
        .args(["--coreform-frontend", "rust"])
        .args(["pkg", "--caps"])
        .arg(&rust_caps)
        .args(["verify", "--lock"])
        .arg(&rust_lock)
        .assert()
        .success()
        .get_output()
        .stdout
        .clone();
    let self_verify = cmd()
        .current_dir(&self_dir)
        .arg("--json")
        .args(["--coreform-frontend", "selfhost"])
        .args(["--selfhost-artifact", artifact.to_str().unwrap()])
        .args(["pkg", "--caps"])
        .arg(&self_caps)
        .args(["verify", "--lock"])
        .arg(&self_lock)
        .assert()
        .success()
        .get_output()
        .stdout
        .clone();
    assert_eq!(json_frontend_name(&rust_verify), "rust");
    assert_eq!(json_frontend_name(&self_verify), "selfhost");
    assert_eq!(
        normalize_pkg_value(&json_value(&rust_verify)),
        normalize_pkg_value(&json_value(&self_verify))
    );
}

#[test]
fn pkg_snapshot_value_matches_between_frontends() {
    let td = tempfile::tempdir().unwrap();
    let rust_dir = td.path().join("rust");
    let self_dir = td.path().join("self");
    fs::create_dir_all(&rust_dir).unwrap();
    fs::create_dir_all(&self_dir).unwrap();

    let rust_caps = write_caps(&rust_dir);
    let self_caps = write_caps(&self_dir);
    let artifact = build_selfhost_artifact(&self_dir);

    let module_src = "(def demo::x 1)\n";
    for d in [&rust_dir, &self_dir] {
        fs::write(
            d.join("package.toml"),
            r#"
name = "demo"
version = "0.1.0"
dependencies = []
obligations = []

[[modules]]
path = "demo.gc"
"#,
        )
        .unwrap();
        fs::write(d.join("demo.gc"), module_src).unwrap();
    }

    let rust_snapshot = cmd()
        .current_dir(&rust_dir)
        .arg("--json")
        .args(["--coreform-frontend", "rust"])
        .args(["pkg", "--caps"])
        .arg(&rust_caps)
        .args(["snapshot", "--pkg", "package.toml"])
        .assert()
        .success()
        .get_output()
        .stdout
        .clone();
    let self_snapshot = cmd()
        .current_dir(&self_dir)
        .arg("--json")
        .args(["--coreform-frontend", "selfhost"])
        .args(["--selfhost-artifact", artifact.to_str().unwrap()])
        .args(["pkg", "--caps"])
        .arg(&self_caps)
        .args(["snapshot", "--pkg", "package.toml"])
        .assert()
        .success()
        .get_output()
        .stdout
        .clone();

    assert_eq!(json_frontend_name(&rust_snapshot), "rust");
    assert_eq!(json_frontend_name(&self_snapshot), "selfhost");
    assert_eq!(json_value(&rust_snapshot), json_value(&self_snapshot));
}
