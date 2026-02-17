use std::fs;
use std::path::{Path, PathBuf};

use assert_cmd::cargo::cargo_bin_cmd;
use gc_coreform::{Term, TermOrdKey, parse_term};

fn cmd() -> assert_cmd::Command {
    cargo_bin_cmd!("genesis_wasi")
}

fn write_caps(dir: &Path) -> PathBuf {
    let caps = dir.join("caps.toml");
    fs::write(
        &caps,
        r#"
allow = [
  "core/store::put",
  "core/refs::get",
  "core/gpk::export",
  "core/gpk::import"
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
    let out = cmd()
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
fn pkg_import_set_ref_enforces_policy_gate_and_rejects_invalid_commit() {
    let td = tempfile::tempdir().unwrap();
    let dir = td.path();
    let caps = write_caps(dir);

    let patch = store_put(
        dir,
        &caps,
        r#"{:type :vcs/patch :v 1 :ops []}"#,
        "patch_bad.gc",
    );
    let snap = store_put(
        dir,
        &caps,
        r#"{:type :vcs/snapshot :v 1 :kind :package :pkg/name "x" :pkg/version "0" :modules [] :obligations []}"#,
        "snap_bad.gc",
    );
    let commit = store_put(
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
  :evidence []
  :attestations []
  :message "bad"
}}"#
        ),
        "c_bad.gc",
    );

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
    :main { :patterns ["refs/**/heads/main"] :required-obligations [core/obligation::unit-tests] :require-signatures false }
    :tags { :patterns ["refs/**/tags/*"] :required-obligations [core/obligation::unit-tests] :require-signatures false }
  }
}
"#,
        "policy_bad.gc",
    );

    let bundle = dir.join("x-full-bad.gpk");
    cmd()
        .current_dir(dir)
        .args(["pkg", "--caps"])
        .arg(&caps)
        .args(["export", "--snapshot", &commit, "--out"])
        .arg(&bundle)
        .args(["--full", "--depth", "0"])
        .assert()
        .success();

    cmd()
        .current_dir(dir)
        .args(["pkg", "--caps"])
        .arg(&caps)
        .args(["import", "--input"])
        .arg(&bundle)
        .args(["--set-ref", &format!("refs/heads/main={commit}")])
        .args(["--policy", &policy_h])
        .assert()
        .code(20);

    let out = cmd()
        .current_dir(dir)
        .arg("--json")
        .args(["refs", "--caps"])
        .arg(&caps)
        .args(["get", "refs/heads/main"])
        .assert()
        .success()
        .get_output()
        .stdout
        .clone();
    let t = parse_term(&json_value(&out)).unwrap();
    let Term::Map(m) = t else {
        panic!("expected map");
    };
    assert_eq!(m.get(&TermOrdKey(Term::symbol(":hash"))), Some(&Term::Nil));
}

#[test]
fn pkg_import_set_ref_is_atomic_across_multiple_targets() {
    let td = tempfile::tempdir().unwrap();
    let dir = td.path();
    let caps = write_caps(dir);

    let patch = store_put(
        dir,
        &caps,
        r#"{:type :vcs/patch :v 1 :ops []}"#,
        "patch_atomic.gc",
    );
    let snap = store_put(
        dir,
        &caps,
        r#"{:type :vcs/snapshot :v 1 :kind :package :pkg/name "x" :pkg/version "0" :modules [] :obligations []}"#,
        "snap_atomic.gc",
    );
    let commit = store_put(
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
  :evidence []
  :attestations []
  :message "atomic-bad"
}}"#
        ),
        "c_atomic_bad.gc",
    );

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
    :main { :patterns ["refs/**/heads/main"] :required-obligations [core/obligation::unit-tests] :require-signatures false }
    :tags { :patterns ["refs/**/tags/*"] :required-obligations [core/obligation::unit-tests] :require-signatures false }
  }
}
"#,
        "policy_atomic.gc",
    );

    let bundle = dir.join("x-full-atomic-bad.gpk");
    cmd()
        .current_dir(dir)
        .args(["pkg", "--caps"])
        .arg(&caps)
        .args(["export", "--snapshot", &commit, "--out"])
        .arg(&bundle)
        .args(["--full", "--depth", "0"])
        .assert()
        .success();

    cmd()
        .current_dir(dir)
        .args(["pkg", "--caps"])
        .arg(&caps)
        .args(["import", "--input"])
        .arg(&bundle)
        .args([
            "--set-ref",
            &format!("refs/heads/dev={commit}"),
            "--set-ref",
            &format!("refs/heads/main={commit}"),
        ])
        .args(["--policy", &policy_h])
        .assert()
        .code(20);

    for name in ["refs/heads/dev", "refs/heads/main"] {
        let out = cmd()
            .current_dir(dir)
            .arg("--json")
            .args(["refs", "--caps"])
            .arg(&caps)
            .args(["get", name])
            .assert()
            .success()
            .get_output()
            .stdout
            .clone();
        let t = parse_term(&json_value(&out)).unwrap();
        let Term::Map(m) = t else {
            panic!("expected map");
        };
        assert_eq!(m.get(&TermOrdKey(Term::symbol(":hash"))), Some(&Term::Nil));
    }
}

#[test]
fn pkg_import_set_ref_supports_expected_old_compare_and_set() {
    let td = tempfile::tempdir().unwrap();
    let dir = td.path();
    let caps = write_caps(dir);

    let patch = store_put(
        dir,
        &caps,
        r#"{:type :vcs/patch :v 1 :ops []}"#,
        "patch_cas.gc",
    );
    let snap = store_put(
        dir,
        &caps,
        r#"{:type :vcs/snapshot :v 1 :kind :package :pkg/name "x" :pkg/version "0" :modules [] :obligations []}"#,
        "snap_cas.gc",
    );
    let commit = store_put(
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
  :evidence []
  :attestations []
  :message "cas"
}}"#
        ),
        "c_cas.gc",
    );
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
        "policy_cas.gc",
    );
    let bundle = dir.join("x-full-cas.gpk");
    cmd()
        .current_dir(dir)
        .args(["pkg", "--caps"])
        .arg(&caps)
        .args(["export", "--snapshot", &commit, "--out"])
        .arg(&bundle)
        .args(["--full", "--depth", "0"])
        .assert()
        .success();

    let old = "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa";
    let refs_path = dir.join(".genesis").join("refs.gc");
    let rdb = gc_effects::RefsDb::open(&refs_path).unwrap();
    let _ = rdb.set("refs/heads/main", Some(old), None).unwrap();

    cmd()
        .current_dir(dir)
        .args(["pkg", "--caps"])
        .arg(&caps)
        .args(["import", "--input"])
        .arg(&bundle)
        .args(["--set-ref", &format!("refs/heads/main={commit}@{old}")])
        .args(["--policy", &policy_h])
        .assert()
        .success();

    let out = cmd()
        .current_dir(dir)
        .arg("--json")
        .args(["refs", "--caps"])
        .arg(&caps)
        .args(["get", "refs/heads/main"])
        .assert()
        .success()
        .get_output()
        .stdout
        .clone();
    let t = parse_term(&json_value(&out)).unwrap();
    let Term::Map(m) = t else {
        panic!("expected map");
    };
    assert_eq!(
        m.get(&TermOrdKey(Term::symbol(":hash"))),
        Some(&Term::Str(commit.clone()))
    );

    cmd()
        .current_dir(dir)
        .args(["pkg", "--caps"])
        .arg(&caps)
        .args(["import", "--input"])
        .arg(&bundle)
        .args([
            "--set-ref",
            &format!("refs/heads/main={commit}@bbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb"),
        ])
        .args(["--policy", &policy_h])
        .assert()
        .code(20);
}
