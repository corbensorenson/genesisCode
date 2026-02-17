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
  "core/refs::set",
  "core/vcs::blame",
  "core/vcs::why"
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
fn wasi_vcs_blame_and_why_attribute_symbol_to_introducing_commit() {
    let td = tempfile::tempdir().unwrap();
    let dir = td.path();
    let caps = write_caps(dir);

    let def1 = store_put(dir, &caps, "1", "def1.gc");
    let def2 = store_put(dir, &caps, "2", "def2.gc");
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

    let snap1 = store_put(
        dir,
        &caps,
        &format!(
            r#"{{
  :type :vcs/snapshot
  :v 1
  :kind :module
  :module/name "pkg/mod"
  :defs {{pkg/mod::x "{def1}"}}
  :exports [pkg/mod::x]
  :obligations []
}}"#
        ),
        "snap1.gc",
    );
    let snap2 = store_put(
        dir,
        &caps,
        &format!(
            r#"{{
  :type :vcs/snapshot
  :v 1
  :kind :module
  :module/name "pkg/mod"
  :defs {{pkg/mod::x "{def1}"}}
  :exports [pkg/mod::x]
  :obligations []
}}"#
        ),
        "snap2.gc",
    );
    let snap3 = store_put(
        dir,
        &caps,
        &format!(
            r#"{{
  :type :vcs/snapshot
  :v 1
  :kind :module
  :module/name "pkg/mod"
  :defs {{pkg/mod::x "{def2}"}}
  :exports [pkg/mod::x]
  :obligations []
}}"#
        ),
        "snap3.gc",
    );

    let c1 = store_put(
        dir,
        &caps,
        &format!(
            r#"{{
  :type :vcs/commit
  :v 1
  :parents []
  :target {{:kind :module :name "pkg/mod"}}
  :base nil
  :patch "{patch1}"
  :result "{snap1}"
  :obligations []
  :evidence ["{ev1}"]
  :attestations []
  :message "c1"
  :why "initial x"
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
  :target {{:kind :module :name "pkg/mod"}}
  :base "{snap1}"
  :patch "{patch2}"
  :result "{snap2}"
  :obligations []
  :evidence ["{ev2}"]
  :attestations []
  :message "c2"
  :why "noop x"
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
  :target {{:kind :module :name "pkg/mod"}}
  :base "{snap2}"
  :patch "{patch3}"
  :result "{snap3}"
  :obligations []
  :evidence ["{ev3}"]
  :attestations []
  :message "c3"
  :why "changed x"
}}"#
        ),
        "c3.gc",
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
        "policy.gc",
    );

    cmd()
        .current_dir(dir)
        .args(["refs", "--caps"])
        .arg(&caps)
        .args(["set"])
        .arg("refs/heads/main")
        .arg(&c3)
        .args(["--policy", &policy_h, "--expected-old", "nil"])
        .assert()
        .success();

    let out = cmd()
        .current_dir(dir)
        .arg("--json")
        .args(["vcs", "--caps"])
        .arg(&caps)
        .args(["blame", "--snapshot", &snap3, "--sym", "pkg/mod::x"])
        .assert()
        .success()
        .get_output()
        .stdout
        .clone();
    let t = parse_term(&json_value(&out)).unwrap();
    let Term::Map(m) = t else {
        panic!("expected map")
    };
    assert_eq!(
        m.get(&TermOrdKey(Term::symbol(":commit"))),
        Some(&Term::Str(c3.clone()))
    );
    assert_eq!(
        m.get(&TermOrdKey(Term::symbol(":value"))),
        Some(&Term::Str(def2.clone()))
    );

    let out = cmd()
        .current_dir(dir)
        .arg("--json")
        .args(["vcs", "--caps"])
        .arg(&caps)
        .args(["blame", "--snapshot", &snap2, "--sym", "pkg/mod::x"])
        .assert()
        .success()
        .get_output()
        .stdout
        .clone();
    let t = parse_term(&json_value(&out)).unwrap();
    let Term::Map(m) = t else {
        panic!("expected map")
    };
    assert_eq!(
        m.get(&TermOrdKey(Term::symbol(":commit"))),
        Some(&Term::Str(c1.clone()))
    );
    assert_eq!(
        m.get(&TermOrdKey(Term::symbol(":value"))),
        Some(&Term::Str(def1.clone()))
    );

    let out = cmd()
        .current_dir(dir)
        .arg("--json")
        .args(["vcs", "--caps"])
        .arg(&caps)
        .args(["why", "--snapshot", &snap3, "--sym", "pkg/mod::x"])
        .assert()
        .success()
        .get_output()
        .stdout
        .clone();
    let t = parse_term(&json_value(&out)).unwrap();
    let Term::Map(m) = t else {
        panic!("expected map")
    };
    assert_eq!(
        m.get(&TermOrdKey(Term::symbol(":commit"))),
        Some(&Term::Str(c3))
    );
    assert_eq!(
        m.get(&TermOrdKey(Term::symbol(":message"))),
        Some(&Term::Str("c3".to_string()))
    );
    assert_eq!(
        m.get(&TermOrdKey(Term::symbol(":why"))),
        Some(&Term::Str("changed x".to_string()))
    );
}
