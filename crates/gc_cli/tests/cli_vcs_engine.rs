use std::fs;
use std::path::{Path, PathBuf};

use assert_cmd::cargo::cargo_bin_cmd;
use gc_coreform::{
    Term, TermOrdKey, canonicalize_module, hash_module, parse_module, parse_term, print_term,
};
use predicates::prelude::*;

mod support;

fn cmd() -> assert_cmd::Command {
    let mut c = cargo_bin_cmd!("genesis");
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
    s.push_str("]\n\n[store]\ndir = \"./.genesis/store\"\n");
    fs::write(&caps, s).unwrap();
    caps
}

fn build_selfhost_artifact(dir: &Path) -> PathBuf {
    support::copy_repo_toolchain_artifact(dir)
}

fn poison_cli_vcs_log_program(artifact: &Path) {
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

    let poisoned_src = "(def core/cli::vcs-log-program \"shadowed\")\n";
    let poisoned_forms = canonicalize_module(parse_module(poisoned_src).unwrap()).unwrap();
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

fn store_put(dir: &Path, caps: &Path, src: &str, filename: &str) -> String {
    let p = dir.join(filename);
    fs::write(&p, src).unwrap();
    let out = cmd()
        .current_dir(dir)
        .args(["store", "--caps"])
        .arg(caps)
        .args(["put", "--input"])
        .arg(&p)
        .assert()
        .success()
        .get_output()
        .stdout
        .clone();
    String::from_utf8(out).unwrap().trim().to_string()
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
fn vcs_log_value_matches_between_frontends() {
    let td = tempfile::tempdir().unwrap();
    let dir = td.path();

    let caps = write_caps(dir, &["core/store::put", "core/vcs::log"]);
    let artifact = build_selfhost_artifact(dir);

    let patch = store_put(dir, &caps, "{:type :vcs/patch :v 1 :ops []}\n", "patch.gc");
    let snap = store_put(
        dir,
        &caps,
        r#"{:type :vcs/snapshot :v 1 :kind :package :pkg/name "x" :pkg/version "0" :modules [] :obligations []}"#,
        "snap.gc",
    );
    let ev = store_put(
        dir,
        &caps,
        r#"{:type :vcs/evidence :v 1 :kind :unit-tests :inputs [] :outputs [] :data nil}"#,
        "ev.gc",
    );

    let c1 = store_put(
        dir,
        &caps,
        &format!(
            r#"{{
  :type :vcs/commit
  :v 1
  :parents []
  :target {{:kind :package :name "x"}}
  :base nil
  :patch "{patch}"
  :result "{snap}"
  :obligations []
  :evidence ["{ev}"]
  :attestations []
  :message "c1"
}}"#
        ),
        "c1.gc",
    );

    let rust_out = cmd()
        .current_dir(dir)
        .arg("--json")
        .args(["--coreform-frontend", "rust"])
        .args(["vcs", "--caps"])
        .arg(&caps)
        .args(["--log"])
        .arg(dir.join("rust.gclog"))
        .args(["log", &c1, "--max", "10"])
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
        .args(["vcs", "--caps"])
        .arg(&caps)
        .args(["--log"])
        .arg(dir.join("self.gclog"))
        .args(["log", &c1, "--max", "10"])
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
fn vcs_log_selfhost_frontend_fails_when_contract_is_poisoned() {
    let td = tempfile::tempdir().unwrap();
    let dir = td.path();

    let caps = write_caps(dir, &["core/store::put", "core/vcs::log"]);
    let artifact = build_selfhost_artifact(dir);
    poison_cli_vcs_log_program(&artifact);

    let patch = store_put(dir, &caps, "{:type :vcs/patch :v 1 :ops []}\n", "patch.gc");
    let snap = store_put(
        dir,
        &caps,
        r#"{:type :vcs/snapshot :v 1 :kind :package :pkg/name "x" :pkg/version "0" :modules [] :obligations []}"#,
        "snap.gc",
    );
    let ev = store_put(
        dir,
        &caps,
        r#"{:type :vcs/evidence :v 1 :kind :unit-tests :inputs [] :outputs [] :data nil}"#,
        "ev.gc",
    );
    let c1 = store_put(
        dir,
        &caps,
        &format!(
            r#"{{
  :type :vcs/commit
  :v 1
  :parents []
  :target {{:kind :package :name "x"}}
  :base nil
  :patch "{patch}"
  :result "{snap}"
  :obligations []
  :evidence ["{ev}"]
  :attestations []
  :message "c1"
}}"#
        ),
        "c1.gc",
    );

    cmd()
        .current_dir(dir)
        .args([
            "--coreform-frontend",
            "selfhost",
            "--selfhost-artifact",
            artifact.to_str().unwrap(),
            "vcs",
            "--caps",
        ])
        .arg(&caps)
        .args(["log", &c1, "--max", "10"])
        .assert()
        .failure()
        .code(20)
        .stderr(predicate::str::contains("vcs-log-program"));
}
