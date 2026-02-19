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

fn write_caps(dir: &Path) -> PathBuf {
    let caps = dir.join("caps.toml");
    fs::write(
        &caps,
        r#"
allow = [
  "core/store::put",
  "core/refs::get",
  "core/refs::list",
  "core/refs::set",
  "core/refs::delete"
]

[store]
dir = "./.genesis/store"

[refs]
path = "./.genesis/refs.gc"
"#,
    )
    .unwrap();
    caps
}

fn store_put(dir: &Path, caps: &Path, term_path: &Path) -> String {
    let out = cargo_bin_cmd!("genesis_wasi")
        .current_dir(dir)
        .args(["store", "--caps"])
        .arg(caps)
        .args(["put", "--input"])
        .arg(term_path)
        .assert()
        .success()
        .get_output()
        .stdout
        .clone();
    String::from_utf8(out).unwrap().trim().to_string()
}

fn keygen_public_key_b64(dir: &Path) -> String {
    let key_path = dir.join("policy_signature_key.toml");
    cargo_bin_cmd!("genesis_wasi")
        .current_dir(dir)
        .args(["keygen", "--out"])
        .arg(&key_path)
        .assert()
        .success();
    let key_s = fs::read_to_string(&key_path).unwrap();
    key_s
        .lines()
        .find_map(|l| {
            let l = l.trim();
            l.strip_prefix("pk_b64 = \"")
                .and_then(|rest| rest.strip_suffix('\"'))
        })
        .expect("pk_b64 in key file")
        .to_string()
}

fn build_selfhost_artifact(dir: &Path) -> PathBuf {
    common::copy_repo_selfhost_toolchain_artifact(dir)
}

fn poison_cli_refs_get_program(artifact: &Path) {
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
    let poisoned_src = format!("{module_src}\n(def core/cli::refs-get-program \"shadowed\")\n");
    let poisoned_forms = canonicalize_module(parse_module(&poisoned_src).unwrap()).unwrap();
    let poisoned_hash = hash_module(&poisoned_forms);
    cli_mod.insert(TermOrdKey(Term::symbol(":source")), Term::Str(poisoned_src));
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
fn refs_get_list_set_delete_match_between_frontends() {
    let td = tempdir().unwrap();
    let dir = td.path();
    let caps = write_caps(dir);
    let artifact = build_selfhost_artifact(dir);

    let policy_term = dir.join("policy.gc");
    fs::write(
        &policy_term,
        r#"
{
  :type :vcs/policy
  :v 1
  :refs {:frozen-prefixes ["refs/frozen/"]}
  :classes {
    :dev  {:patterns ["refs/**/heads/*"] :exclude ["refs/**/heads/main"]
           :required-obligations ["core/obligation::unit-tests"]}
    :main {:patterns ["refs/**/heads/main"]
           :required-obligations ["core/obligation::unit-tests"]}
    :tags {:patterns ["refs/**/tags/*"]
           :required-obligations ["core/obligation::unit-tests"]
           :require-signatures false}
  }
}
"#,
    )
    .unwrap();
    let policy_h = store_put(dir, &caps, &policy_term);

    let evidence_term = dir.join("evidence.gc");
    fs::write(
        &evidence_term,
        r#"{:type :vcs/evidence :v 1 :kind :unit-tests :data nil}"#,
    )
    .unwrap();
    let evidence_h = store_put(dir, &caps, &evidence_term);

    let commit_term = dir.join("commit.gc");
    fs::write(
        &commit_term,
        format!(
            r#"
{{
  :type :vcs/commit
  :v 1
  :parents []
  :base nil
  :patch "{z}"
  :result "{z}"
  :obligations ["core/obligation::unit-tests"]
  :evidence ["{evidence_h}"]
  :attestations []
  :message "test commit"
}}
"#,
            z = "0".repeat(64)
        ),
    )
    .unwrap();
    let commit_h = store_put(dir, &caps, &commit_term);

    let rust_set = cmd()
        .current_dir(dir)
        .args([
            "--coreform-frontend",
            "rust",
            "refs",
            "--caps",
            caps.to_str().unwrap(),
            "set",
            "refs/heads/dev",
            &commit_h,
            "--policy",
            &policy_h,
        ])
        .assert()
        .success()
        .get_output()
        .stdout
        .clone();
    let rust_set = String::from_utf8(rust_set).unwrap();

    let self_set = cmd()
        .current_dir(dir)
        .args([
            "--coreform-frontend",
            "selfhost",
            "--selfhost-artifact",
            artifact.to_str().unwrap(),
            "refs",
            "--caps",
            caps.to_str().unwrap(),
            "set",
            "refs/heads/dev",
            &commit_h,
            "--policy",
            &policy_h,
        ])
        .assert()
        .success()
        .get_output()
        .stdout
        .clone();
    let self_set = String::from_utf8(self_set).unwrap();
    assert_eq!(rust_set, self_set);

    let rust_get = cmd()
        .current_dir(dir)
        .args([
            "--coreform-frontend",
            "rust",
            "refs",
            "--caps",
            caps.to_str().unwrap(),
            "get",
            "refs/heads/dev",
        ])
        .assert()
        .success()
        .get_output()
        .stdout
        .clone();
    let rust_get = String::from_utf8(rust_get).unwrap();

    let self_get = cmd()
        .current_dir(dir)
        .args([
            "--coreform-frontend",
            "selfhost",
            "--selfhost-artifact",
            artifact.to_str().unwrap(),
            "refs",
            "--caps",
            caps.to_str().unwrap(),
            "get",
            "refs/heads/dev",
        ])
        .assert()
        .success()
        .get_output()
        .stdout
        .clone();
    let self_get = String::from_utf8(self_get).unwrap();
    assert_eq!(rust_get, self_get);
    assert_eq!(rust_get.trim(), commit_h);

    let rust_list = cmd()
        .current_dir(dir)
        .args([
            "--coreform-frontend",
            "rust",
            "refs",
            "--caps",
            caps.to_str().unwrap(),
            "list",
            "--prefix",
            "refs/heads/",
        ])
        .assert()
        .success()
        .get_output()
        .stdout
        .clone();
    let rust_list = String::from_utf8(rust_list).unwrap();

    let self_list = cmd()
        .current_dir(dir)
        .args([
            "--coreform-frontend",
            "selfhost",
            "--selfhost-artifact",
            artifact.to_str().unwrap(),
            "refs",
            "--caps",
            caps.to_str().unwrap(),
            "list",
            "--prefix",
            "refs/heads/",
        ])
        .assert()
        .success()
        .get_output()
        .stdout
        .clone();
    let self_list = String::from_utf8(self_list).unwrap();
    assert_eq!(rust_list, self_list);
    assert!(predicate::str::contains(format!("refs/heads/dev {commit_h}\n")).eval(&rust_list));

    let rust_del = cmd()
        .current_dir(dir)
        .args([
            "--coreform-frontend",
            "rust",
            "refs",
            "--caps",
            caps.to_str().unwrap(),
            "delete",
            "refs/heads/dev",
            "--policy",
            &policy_h,
        ])
        .assert()
        .success()
        .get_output()
        .stdout
        .clone();
    let rust_del = String::from_utf8(rust_del).unwrap();

    let self_del = cmd()
        .current_dir(dir)
        .args([
            "--coreform-frontend",
            "selfhost",
            "--selfhost-artifact",
            artifact.to_str().unwrap(),
            "refs",
            "--caps",
            caps.to_str().unwrap(),
            "delete",
            "refs/heads/dev",
            "--policy",
            &policy_h,
        ])
        .assert()
        .success()
        .get_output()
        .stdout
        .clone();
    let self_del = String::from_utf8(self_del).unwrap();
    assert_eq!(rust_del, self_del);
    assert_eq!(rust_del, "ok\n");
}

#[test]
fn wasi_refs_set_tag_signature_policy_rejects_unsigned_commit_in_both_frontends() {
    let td = tempfile::tempdir().unwrap();
    let dir = td.path();
    let caps = write_caps(dir);
    let artifact = build_selfhost_artifact(dir);
    let pk_b64 = keygen_public_key_b64(dir);

    let policy_term = dir.join("policy_signature.gc");
    fs::write(
        &policy_term,
        format!(
            r#"
{{
  :type :vcs/policy
  :v 1
  :refs {{:frozen-prefixes []}}
  :classes {{
    :dev  {{:patterns ["refs/**/heads/*"] :exclude ["refs/**/heads/main"] :required-obligations []}}
    :main {{:patterns ["refs/**/heads/main"] :required-obligations [] :require-signatures false}}
    :tags {{:patterns ["refs/**/tags/*"]
           :required-obligations ["core/obligation::unit-tests"]
           :require-signatures true
           :min-signatures 1
           :allowed-public-keys ["{pk_b64}"]}}
  }}
}}
"#
        ),
    )
    .unwrap();
    let policy_h = store_put(dir, &caps, &policy_term);

    let evidence_term = dir.join("evidence_signature.gc");
    fs::write(
        &evidence_term,
        r#"{:type :vcs/evidence :v 1 :kind :unit-tests :data nil}"#,
    )
    .unwrap();
    let evidence_h = store_put(dir, &caps, &evidence_term);

    let commit_term = dir.join("unsigned_tag_commit.gc");
    fs::write(
        &commit_term,
        format!(
            r#"
{{
  :type :vcs/commit
  :v 1
  :parents []
  :base nil
  :patch "{z}"
  :result "{z}"
  :obligations ["core/obligation::unit-tests"]
  :evidence ["{evidence_h}"]
  :attestations []
  :message "unsigned-tag-commit"
}}
"#,
            z = "0".repeat(64)
        ),
    )
    .unwrap();
    let commit_h = store_put(dir, &caps, &commit_term);

    let rust_out = cmd()
        .current_dir(dir)
        .args([
            "--json",
            "--coreform-frontend",
            "rust",
            "refs",
            "--caps",
            caps.to_str().unwrap(),
            "set",
            "refs/tags/v1.0.0",
            &commit_h,
            "--policy",
            &policy_h,
        ])
        .assert()
        .failure()
        .get_output()
        .stdout
        .clone();
    let rust_json: serde_json::Value = serde_json::from_slice(&rust_out).unwrap();

    let self_out = cmd()
        .current_dir(dir)
        .args([
            "--json",
            "--coreform-frontend",
            "selfhost",
            "--selfhost-artifact",
            artifact.to_str().unwrap(),
            "refs",
            "--caps",
            caps.to_str().unwrap(),
            "set",
            "refs/tags/v1.0.0",
            &commit_h,
            "--policy",
            &policy_h,
        ])
        .assert()
        .failure()
        .get_output()
        .stdout
        .clone();
    let self_json: serde_json::Value = serde_json::from_slice(&self_out).unwrap();

    assert_eq!(rust_json["ok"], serde_json::Value::Bool(false));
    assert_eq!(self_json["ok"], serde_json::Value::Bool(false));
    let rust_code = rust_json["error"]["code"].as_str().unwrap_or_default();
    let self_code = self_json["error"]["code"].as_str().unwrap_or_default();
    assert!(!rust_code.is_empty(), "missing rust error code");
    assert_eq!(rust_code, self_code);
    let rust_msg = rust_json["error"]["message"].as_str().unwrap_or_default();
    let self_msg = self_json["error"]["message"].as_str().unwrap_or_default();
    assert!(!rust_msg.is_empty(), "missing rust error message");
    assert_eq!(rust_msg, self_msg);

    let got = cmd()
        .current_dir(dir)
        .args([
            "refs",
            "--caps",
            caps.to_str().unwrap(),
            "get",
            "refs/tags/v1.0.0",
        ])
        .assert()
        .success()
        .get_output()
        .stdout
        .clone();
    assert_eq!(String::from_utf8(got).unwrap().trim(), "nil");
}

#[test]
fn refs_get_selfhost_frontend_fails_when_contract_is_poisoned() {
    let td = tempdir().unwrap();
    let dir = td.path();
    let caps = write_caps(dir);
    let artifact = build_selfhost_artifact(dir);
    poison_cli_refs_get_program(&artifact);

    cmd()
        .current_dir(dir)
        .args([
            "--coreform-frontend",
            "selfhost",
            "--selfhost-artifact",
            artifact.to_str().unwrap(),
            "refs",
            "--caps",
            caps.to_str().unwrap(),
            "get",
            "refs/heads/main",
        ])
        .assert()
        .failure()
        .code(20)
        .stderr(predicate::str::contains("refs-get-program"));
}
