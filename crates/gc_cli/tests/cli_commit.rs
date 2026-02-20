use std::fs;
use std::path::{Path, PathBuf};

use assert_cmd::cargo::cargo_bin_cmd;
use gc_coreform::{Term, TermOrdKey, parse_term};

fn write_caps(dir: &Path) -> PathBuf {
    let caps = dir.join("caps.toml");
    fs::write(
        &caps,
        r#"
allow = [
  "core/store::put",
  "core/store::get",
  "core/refs::get",
  "core/refs::set",
  "core/vcs-low::apply"
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

fn json_value(stdout: &[u8]) -> Term {
    let v: serde_json::Value = serde_json::from_slice(stdout).unwrap();
    let value_str = v
        .get("data")
        .and_then(|d| d.get("value"))
        .and_then(|x| x.as_str())
        .expect("json value string");
    parse_term(value_str).expect("parse data.value term")
}

#[test]
fn commit_new_and_show_roundtrip_with_ref_base_and_patch_file() {
    let td = tempfile::tempdir().unwrap();
    let dir = td.path();
    let caps = write_caps(dir);

    let base_snapshot_h = store_put(
        dir,
        &caps,
        r#"{:type :vcs/snapshot :v 1 :kind :module :module/name "pkg/mod" :defs {} :exports [] :obligations []}"#,
        "base_snapshot.gc",
    );

    let seed_commit_h = store_put(
        dir,
        &caps,
        &format!(
            r#"{{
  :type :vcs/commit
  :v 1
  :parents []
  :target {{:kind :module :name "pkg/mod"}}
  :base "{base_snapshot_h}"
  :patch "{z}"
  :result "{base_snapshot_h}"
  :obligations []
  :evidence []
  :attestations []
  :message "seed"
}}"#,
            z = "0".repeat(64)
        ),
        "seed_commit.gc",
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

    cargo_bin_cmd!("genesis")
        .current_dir(dir)
        .args(["refs", "--caps"])
        .arg(&caps)
        .args([
            "set",
            "refs/heads/main",
            &seed_commit_h,
            "--policy",
            &policy_h,
        ])
        .assert()
        .success();

    let patch_file = dir.join("patch.gc");
    fs::write(&patch_file, r#"{:type :vcs/patch :v 1 :ops []}"#).unwrap();

    let out = cargo_bin_cmd!("genesis")
        .current_dir(dir)
        .arg("--json")
        .args(["commit", "--caps"])
        .arg(&caps)
        .args([
            "new",
            "--target-kind",
            "module",
            "--target-id",
            "pkg/mod",
            "--base",
            "refs/heads/main",
            "--patch",
        ])
        .arg(&patch_file)
        .args(["--message", "new commit", "--store"])
        .assert()
        .success()
        .get_output()
        .stdout
        .clone();

    let env: serde_json::Value = serde_json::from_slice(&out).unwrap();
    assert_eq!(
        env.get("kind").and_then(|x| x.as_str()),
        Some("genesis/commit-new-v0.1")
    );

    let t = json_value(&out);
    let Term::Map(m) = t else {
        panic!("expected map")
    };
    let commit_h = match m.get(&TermOrdKey(Term::symbol(":commit"))) {
        Some(Term::Str(s)) => s.clone(),
        _ => panic!("missing :commit"),
    };
    let Term::Map(artifact) = m
        .get(&TermOrdKey(Term::symbol(":artifact")))
        .expect("artifact map")
    else {
        panic!("artifact must be map");
    };
    assert_eq!(
        artifact.get(&TermOrdKey(Term::symbol(":base"))),
        Some(&Term::Str(base_snapshot_h.clone()))
    );
    let Term::Vector(parents) = artifact
        .get(&TermOrdKey(Term::symbol(":parents")))
        .expect("parents vector")
    else {
        panic!("parents must be vector");
    };
    assert_eq!(parents, &vec![Term::Str(seed_commit_h.clone())]);

    let show_out = cargo_bin_cmd!("genesis")
        .current_dir(dir)
        .arg("--json")
        .args(["commit", "--caps"])
        .arg(&caps)
        .args(["show", &commit_h])
        .assert()
        .success()
        .get_output()
        .stdout
        .clone();
    let show_env: serde_json::Value = serde_json::from_slice(&show_out).unwrap();
    assert_eq!(
        show_env.get("kind").and_then(|x| x.as_str()),
        Some("genesis/commit-show-v0.1")
    );
    let show_t = json_value(&show_out);
    let Term::Map(show_m) = show_t else {
        panic!("expected map");
    };
    assert_eq!(
        show_m.get(&TermOrdKey(Term::symbol(":hash"))),
        Some(&Term::Str(commit_h))
    );
}

#[test]
fn commit_new_rejects_unset_base_ref() {
    let td = tempfile::tempdir().unwrap();
    let dir = td.path();
    let caps = write_caps(dir);

    let patch_file = dir.join("patch.gc");
    fs::write(&patch_file, r#"{:type :vcs/patch :v 1 :ops []}"#).unwrap();

    cargo_bin_cmd!("genesis")
        .current_dir(dir)
        .args(["commit", "--caps"])
        .arg(&caps)
        .args([
            "new",
            "--target-kind",
            "module",
            "--target-id",
            "pkg/mod",
            "--base",
            "refs/heads/main",
            "--patch",
        ])
        .arg(&patch_file)
        .args(["--message", "new commit", "--store"])
        .assert()
        .code(10)
        .stdout(predicates::str::contains(
            "base ref `refs/heads/main` is unset",
        ));
}
