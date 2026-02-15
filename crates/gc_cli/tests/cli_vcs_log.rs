use std::fs;
use std::path::{Path, PathBuf};

use assert_cmd::cargo::cargo_bin_cmd;
use gc_coreform::{Term, TermOrdKey, parse_term};
use predicates::prelude::*;

fn write_caps(dir: &Path) -> PathBuf {
    let caps = dir.join("caps.toml");
    fs::write(
        &caps,
        r#"
allow = [
  "core/store::put",
  "core/refs::set",
  "core/gpk::export",
  "core/gpk::import",
  "core/vcs::log"
]

[store]
dir = "./.genesis/store"

[refs]
path = "./.genesis/refs.gc"

[op."core/gpk::export"]
base_dir = "."

[op."core/gpk::import"]
base_dir = "."
"#,
    )
    .unwrap();
    caps
}

fn store_put(dir: &Path, caps: &Path, term_src: &str, filename: &str) -> String {
    let p = dir.join(filename);
    fs::write(&p, term_src).unwrap();
    let out = cargo_bin_cmd!("genesis")
        .current_dir(dir)
        .args(["store", "--caps"])
        .arg(caps)
        .args(["put", "--input"])
        .arg(filename)
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

#[test]
fn full_history_import_supports_vcs_log_over_embedded_ref_head() {
    let td = tempfile::tempdir().unwrap();
    let dir = td.path();
    let caps = write_caps(dir);

    // Minimal patch/snapshot/evidence artifacts.
    let patch1 = store_put(
        dir,
        &caps,
        r#"{:type :vcs/patch :v 1 :ops []}"#,
        "patch1.gc",
    );
    let patch2 = store_put(
        dir,
        &caps,
        r#"{:type :vcs/patch :v 1 :ops []}"#,
        "patch2.gc",
    );
    let patch3 = store_put(
        dir,
        &caps,
        r#"{:type :vcs/patch :v 1 :ops []}"#,
        "patch3.gc",
    );

    let snap1 = store_put(
        dir,
        &caps,
        r#"{:type :vcs/snapshot :v 1 :kind :package :pkg/name "x" :pkg/version "0" :modules [] :obligations []}"#,
        "snap1.gc",
    );
    let snap2 = store_put(
        dir,
        &caps,
        r#"{:type :vcs/snapshot :v 1 :kind :package :pkg/name "x" :pkg/version "1" :modules [] :obligations []}"#,
        "snap2.gc",
    );
    let snap3 = store_put(
        dir,
        &caps,
        r#"{:type :vcs/snapshot :v 1 :kind :package :pkg/name "x" :pkg/version "2" :modules [] :obligations []}"#,
        "snap3.gc",
    );

    let ev1 = store_put(
        dir,
        &caps,
        r#"{:type :vcs/evidence :v 1 :kind :unit-tests :inputs [] :outputs [] :data nil}"#,
        "ev1.gc",
    );
    let ev2 = store_put(
        dir,
        &caps,
        r#"{:type :vcs/evidence :v 1 :kind :unit-tests :inputs [] :outputs [] :data nil}"#,
        "ev2.gc",
    );
    let ev3 = store_put(
        dir,
        &caps,
        r#"{:type :vcs/evidence :v 1 :kind :unit-tests :inputs [] :outputs [] :data nil}"#,
        "ev3.gc",
    );

    // 3-commit linear history.
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
  :patch "{patch1}"
  :result "{snap1}"
  :obligations []
  :evidence ["{ev1}"]
  :attestations []
  :message "c1"
}}"#
        ),
        "c1.gc",
    );
    let c2 = store_put(
        dir,
        &caps,
        &format!(
            r#"{{
  :type :vcs/commit
  :v 1
  :parents ["{c1}"]
  :target {{:kind :package :name "x"}}
  :base "{snap1}"
  :patch "{patch2}"
  :result "{snap2}"
  :obligations []
  :evidence ["{ev2}"]
  :attestations []
  :message "c2"
}}"#
        ),
        "c2.gc",
    );
    let c3 = store_put(
        dir,
        &caps,
        &format!(
            r#"{{
  :type :vcs/commit
  :v 1
  :parents ["{c2}"]
  :target {{:kind :package :name "x"}}
  :base "{snap2}"
  :patch "{patch3}"
  :result "{snap3}"
  :obligations []
  :evidence ["{ev3}"]
  :attestations []
  :message "c3"
}}"#
        ),
        "c3.gc",
    );

    // Policy allowing refs/heads/main updates without obligations for test simplicity.
    let policy_h = store_put(
        dir,
        &caps,
        r#"
{
  :type :vcs/policy
  :v 1
  :name "policy:test"
  :refs { :frozen-prefixes [] }
  :classes {
    :dev  { :patterns ["refs/**/heads/*"] :exclude ["refs/**/heads/main"] :required-obligations [] }
    :main { :patterns ["refs/**/heads/main"] :required-obligations [] :require-signatures false }
    :tags { :patterns ["refs/**/tags/*"] :required-obligations [] :require-signatures false }
  }
}
"#,
        "policy.gc",
    );

    // Set ref -> c3.
    cargo_bin_cmd!("genesis")
        .current_dir(dir)
        .args(["refs", "--caps"])
        .arg(&caps)
        .args(["set"])
        .arg("refs/heads/main")
        .arg(&c3)
        .args(["--policy", &policy_h, "--expected-old", "nil"])
        .assert()
        .success();

    // Export full-history bundle rooted at c3, include parent commits and embed the ref.
    let bundle = dir.join("x-full.gpk");
    cargo_bin_cmd!("genesis")
        .current_dir(dir)
        .args(["pkg", "--caps"])
        .arg(&caps)
        .args(["export", "--snapshot", &c3, "--out"])
        .arg(&bundle)
        .args(["--full", "--depth", "2"])
        .args(["--include-ref", "refs/heads/main"])
        .assert()
        .success()
        .stdout(predicate::str::is_match("^[0-9a-f]{64}\n$").unwrap());
    assert!(bundle.exists());

    // Simulate empty store, then import.
    let store_dir = dir.join(".genesis").join("store");
    fs::remove_dir_all(&store_dir).unwrap();

    cargo_bin_cmd!("genesis")
        .current_dir(dir)
        .args(["pkg", "--caps"])
        .arg(&caps)
        .args(["import", "--input"])
        .arg(&bundle)
        .assert()
        .success();

    // Now `vcs log` must be able to walk the imported history.
    let out = cargo_bin_cmd!("genesis")
        .current_dir(dir)
        .arg("--json")
        .args(["vcs", "--caps"])
        .arg(&caps)
        .args(["log", "refs/heads/main", "--max", "10"])
        .assert()
        .success()
        .get_output()
        .stdout
        .clone();

    let t = parse_term(&json_value(&out)).unwrap();
    let Term::Map(m) = t else {
        panic!("expected map")
    };
    let Term::Vector(commits) = m
        .get(&TermOrdKey(Term::symbol(":commits")))
        .expect("missing :commits")
        .clone()
    else {
        panic!(":commits must be a vector");
    };
    assert_eq!(commits.len(), 3);
    // First entry is the head.
    let Term::Map(h0) = &commits[0] else {
        panic!("commit entry must be map")
    };
    assert_eq!(
        h0.get(&TermOrdKey(Term::symbol(":hash"))),
        Some(&Term::Str(c3))
    );
}
