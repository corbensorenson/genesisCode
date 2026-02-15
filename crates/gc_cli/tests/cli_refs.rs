use std::fs;
use std::path::{Path, PathBuf};

use assert_cmd::cargo::cargo_bin_cmd;
use predicates::prelude::*;

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
    let out = cargo_bin_cmd!("genesis")
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

#[test]
fn refs_set_get_list_delete_roundtrip_and_policy_gates() {
    let td = tempfile::tempdir().unwrap();
    let dir = td.path();
    let caps = write_caps(dir);

    // Policy artifact: require unit-tests obligation for dev/main/tags.
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
    assert!(
        predicate::str::is_match("^[0-9a-f]{64}$")
            .unwrap()
            .eval(&policy_h)
    );

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

    // set
    cargo_bin_cmd!("genesis")
        .current_dir(dir)
        .args(["refs", "--caps"])
        .arg(&caps)
        .args(["set", "refs/heads/dev", &commit_h, "--policy", &policy_h])
        .assert()
        .success()
        .stdout(format!("{commit_h}\n"));

    // get
    cargo_bin_cmd!("genesis")
        .current_dir(dir)
        .args(["refs", "--caps"])
        .arg(&caps)
        .args(["get", "refs/heads/dev"])
        .assert()
        .success()
        .stdout(format!("{commit_h}\n"));

    // list
    cargo_bin_cmd!("genesis")
        .current_dir(dir)
        .args(["refs", "--caps"])
        .arg(&caps)
        .args(["list", "--prefix", "refs/heads/"])
        .assert()
        .success()
        .stdout(predicate::str::contains(format!(
            "refs/heads/dev {commit_h}\n"
        )));

    // CAS conflict
    cargo_bin_cmd!("genesis")
        .current_dir(dir)
        .args(["refs", "--caps"])
        .arg(&caps)
        .args([
            "set",
            "refs/heads/dev",
            &commit_h,
            "--policy",
            &policy_h,
            "--expected-old",
            "nil",
        ])
        .assert()
        .code(20)
        .stdout(predicate::str::contains("core/refs/conflict"));

    // delete
    cargo_bin_cmd!("genesis")
        .current_dir(dir)
        .args(["refs", "--caps"])
        .arg(&caps)
        .args(["delete", "refs/heads/dev", "--policy", &policy_h])
        .assert()
        .success()
        .stdout("ok\n");

    cargo_bin_cmd!("genesis")
        .current_dir(dir)
        .args(["refs", "--caps"])
        .arg(&caps)
        .args(["get", "refs/heads/dev"])
        .assert()
        .success()
        .stdout("nil\n");

    // Missing obligation fails policy gate.
    let commit_bad_term = dir.join("commit_bad.gc");
    fs::write(
        &commit_bad_term,
        format!(
            r#"
{{
  :type :vcs/commit
  :v 1
  :parents []
  :base nil
  :patch "{z}"
  :result "{z}"
  :obligations []
  :evidence ["{evidence_h}"]
  :attestations []
  :message "bad commit"
}}
"#,
            z = "0".repeat(64)
        ),
    )
    .unwrap();
    let commit_bad_h = store_put(dir, &caps, &commit_bad_term);

    cargo_bin_cmd!("genesis")
        .current_dir(dir)
        .args(["refs", "--caps"])
        .arg(&caps)
        .args([
            "set",
            "refs/heads/dev",
            &commit_bad_h,
            "--policy",
            &policy_h,
        ])
        .assert()
        .code(20)
        .stdout(predicate::str::contains("core/refs/missing-obligation"));
}
