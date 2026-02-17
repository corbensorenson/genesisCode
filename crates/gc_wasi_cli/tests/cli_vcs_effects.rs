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
  "core/store::get",
  "core/vcs::diff",
  "core/vcs::apply",
  "core/vcs::log"
]

[store]
dir = "./.genesis/store"

[refs]
path = "./.genesis/refs.gc"

[op."core/vcs::diff"]
base_dir = "."
create_dirs = true

[op."core/vcs::apply"]
base_dir = "."
create_dirs = true
"#,
    )
    .unwrap();
    caps
}

fn store_put(dir: &Path, caps: &Path, term_src: &str, filename: &str) -> String {
    fs::write(dir.join(filename), term_src).unwrap();
    let out = cargo_bin_cmd!("genesis_wasi")
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
fn wasi_vcs_diff_then_apply_roundtrips_snapshot_bytes() {
    let td = tempfile::tempdir().unwrap();
    let dir = td.path();
    let caps = write_caps(dir);

    let h1 = "1".repeat(64);
    let h2 = "2".repeat(64);
    let h3 = "3".repeat(64);

    let base_snap = format!(
        r#"
{{
  :type :vcs/snapshot
  :v 1
  :kind :contract
  :proto nil
  :overrides {{
    my/op::a "{h1}"
  }}
}}
"#
    );
    let to_snap = format!(
        r#"
{{
  :type :vcs/snapshot
  :v 1
  :kind :contract
  :proto nil
  :overrides {{
    my/op::a "{h2}"
    my/op::b "{h3}"
  }}
}}
"#
    );

    let base_h = store_put(dir, &caps, &base_snap, "base.gc");
    let to_h = store_put(dir, &caps, &to_snap, "to.gc");
    assert!(
        predicate::str::is_match("^[0-9a-f]{64}$")
            .unwrap()
            .eval(&base_h)
    );
    assert!(
        predicate::str::is_match("^[0-9a-f]{64}$")
            .unwrap()
            .eval(&to_h)
    );

    let patch_path = dir.join("delta.patch");
    let patch_h = String::from_utf8(
        cargo_bin_cmd!("genesis_wasi")
            .current_dir(dir)
            .args(["vcs", "--caps"])
            .arg(&caps)
            .args([
                "diff",
                "--base",
                &base_h,
                "--to",
                &to_h,
                "--out",
                patch_path.to_str().unwrap(),
            ])
            .assert()
            .success()
            .get_output()
            .stdout
            .clone(),
    )
    .unwrap()
    .trim()
    .to_string();
    assert!(
        predicate::str::is_match("^[0-9a-f]{64}$")
            .unwrap()
            .eval(&patch_h)
    );
    assert!(patch_path.exists());

    cargo_bin_cmd!("genesis_wasi")
        .current_dir(dir)
        .args(["store", "--caps"])
        .arg(&caps)
        .args(["put", "--input"])
        .arg(&patch_path)
        .assert()
        .success()
        .stdout(format!("{patch_h}\n"));

    let out_snap_path = dir.join("out-snap.gc");
    let got_h = String::from_utf8(
        cargo_bin_cmd!("genesis_wasi")
            .current_dir(dir)
            .args(["vcs", "--caps"])
            .arg(&caps)
            .args([
                "apply",
                "--base",
                &base_h,
                "--patch",
                &patch_h,
                "--out",
                out_snap_path.to_str().unwrap(),
            ])
            .assert()
            .success()
            .get_output()
            .stdout
            .clone(),
    )
    .unwrap()
    .trim()
    .to_string();
    assert_eq!(got_h, to_h);
    assert!(out_snap_path.exists());
}

#[test]
fn wasi_vcs_log_walks_commit_history_from_hash() {
    let td = tempfile::tempdir().unwrap();
    let dir = td.path();
    let caps = write_caps(dir);

    let patch_h = store_put(dir, &caps, r#"{:type :vcs/patch :v 1 :ops []}"#, "patch.gc");
    let snap_h = store_put(
        dir,
        &caps,
        r#"{:type :vcs/snapshot :v 1 :kind :package :pkg/name "x" :pkg/version "0" :modules [] :obligations []}"#,
        "snap.gc",
    );
    let ev_h = store_put(
        dir,
        &caps,
        r#"{:type :vcs/evidence :v 1 :kind :unit-tests :inputs [] :outputs [] :data nil}"#,
        "ev.gc",
    );
    let commit_h = store_put(
        dir,
        &caps,
        &format!(
            r#"{{
  :type :vcs/commit
  :v 1
  :parents []
  :target {{:kind :package :name "x"}}
  :base nil
  :patch "{patch_h}"
  :result "{snap_h}"
  :obligations []
  :evidence ["{ev_h}"]
  :attestations []
  :message "c1"
}}"#
        ),
        "c1.gc",
    );

    let out = cargo_bin_cmd!("genesis_wasi")
        .current_dir(dir)
        .arg("--json")
        .args(["vcs", "--caps"])
        .arg(&caps)
        .args(["log", &commit_h, "--max", "8"])
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
        panic!(":commits must be vector");
    };
    assert_eq!(commits.len(), 1);
    let Term::Map(h0) = &commits[0] else {
        panic!("commit entry must be map")
    };
    assert_eq!(
        h0.get(&TermOrdKey(Term::symbol(":hash"))),
        Some(&Term::Str(commit_h))
    );
}
