use assert_cmd::cargo::cargo_bin_cmd;
use predicates::prelude::*;
use serde_json::Value as JsonValue;
use std::path::{Path, PathBuf};
use tempfile::tempdir;

fn write_store_caps(dir: &Path) -> PathBuf {
    let caps = dir.join("caps_store.toml");
    std::fs::write(
        &caps,
        r#"
allow = ["core/store::put"]

[store]
dir = "./.genesis/store"
"#,
    )
    .unwrap();
    caps
}

fn put_policy_artifact(dir: &Path) -> String {
    let policy = dir.join("policy.gc");
    std::fs::write(
        &policy,
        r#"
{:type :vcs/policy
 :v 1
 :name "policy:default-v0.1"
 :refs {:frozen-prefixes ["refs/frozen/"]}
 :classes
 {:dev
  {:patterns ["refs/**/heads/*"]
   :exclude ["refs/**/heads/main"]
   :required-obligations [core/obligation::unit-tests core/obligation::capabilities-declared]}
  :main
  {:patterns ["refs/**/heads/main"]
   :required-obligations [core/obligation::unit-tests core/obligation::replayable-tests]}
  :tags
  {:patterns ["refs/**/tags/*"]
   :required-obligations [core/obligation::unit-tests core/obligation::signed-provenance]
   :require-signatures false}}}
"#,
    )
    .unwrap();
    let caps = write_store_caps(dir);
    let out = cargo_bin_cmd!("genesis")
        .current_dir(dir)
        .args(["store", "--caps"])
        .arg(&caps)
        .args(["put", "--input"])
        .arg(&policy)
        .assert()
        .success()
        .get_output()
        .stdout
        .clone();
    String::from_utf8(out).unwrap().trim().to_string()
}

#[test]
fn policy_set_default_hash_and_list_json() {
    let td = tempdir().unwrap();
    let policies = td.path().join("policies.toml");
    let hash = "a".repeat(64);

    cargo_bin_cmd!("genesis")
        .current_dir(td.path())
        .args(["policy", "set-default", &hash, "--policies"])
        .arg(&policies)
        .assert()
        .success()
        .stdout(predicate::str::contains("default-resolved"));

    let out = cargo_bin_cmd!("genesis")
        .current_dir(td.path())
        .args(["--json", "policy", "list", "--policies"])
        .arg(&policies)
        .assert()
        .success()
        .get_output()
        .stdout
        .clone();
    let v: JsonValue = serde_json::from_slice(&out).unwrap();
    assert_eq!(v["ok"], JsonValue::Bool(true));
    assert_eq!(
        v["kind"],
        JsonValue::String("genesis/policy-list-v0.1".into())
    );
    assert_eq!(v["data"]["default"], JsonValue::String(hash.clone()));
    assert_eq!(v["data"]["default_resolved"], JsonValue::String(hash));
}

#[test]
fn policy_show_resolves_alias_and_hash() {
    let td = tempdir().unwrap();
    let hash = put_policy_artifact(td.path());
    let policies = td.path().join("policies.toml");
    std::fs::write(
        &policies,
        format!(
            "version = 1\ndefault = \"policy:default-v0.1\"\n\n[aliases]\n\"policy:default-v0.1\" = \"{hash}\"\n"
        ),
    )
    .unwrap();

    cargo_bin_cmd!("genesis")
        .current_dir(td.path())
        .args(["policy", "show", "policy:default-v0.1", "--policies"])
        .arg(&policies)
        .args(["--store", ".genesis/store"])
        .assert()
        .success()
        .stdout(predicate::str::contains(":vcs/policy"));

    let out = cargo_bin_cmd!("genesis")
        .current_dir(td.path())
        .args([
            "--json",
            "policy",
            "show",
            &hash,
            "--store",
            ".genesis/store",
        ])
        .assert()
        .success()
        .get_output()
        .stdout
        .clone();
    let v: JsonValue = serde_json::from_slice(&out).unwrap();
    assert_eq!(v["ok"], JsonValue::Bool(true));
    assert_eq!(
        v["kind"],
        JsonValue::String("genesis/policy-show-v0.1".into())
    );
    assert_eq!(v["data"]["hash"], JsonValue::String(hash));
    assert_eq!(v["data"]["classes"]["dev"], JsonValue::Bool(true));
    assert_eq!(v["data"]["classes"]["main"], JsonValue::Bool(true));
    assert_eq!(v["data"]["classes"]["tags"], JsonValue::Bool(true));
}

#[test]
fn policy_set_default_rejects_unknown_alias() {
    let td = tempdir().unwrap();
    let policies = td.path().join("policies.toml");
    std::fs::write(&policies, "version = 1\n").unwrap();
    cargo_bin_cmd!("genesis")
        .current_dir(td.path())
        .args(["policy", "set-default", "policy:missing", "--policies"])
        .arg(&policies)
        .assert()
        .failure()
        .code(50)
        .stderr(predicate::str::contains("unknown policy alias"));
}
