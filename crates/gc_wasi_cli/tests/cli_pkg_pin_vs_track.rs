use std::fs;
use std::path::{Path, PathBuf};

use assert_cmd::cargo::cargo_bin_cmd;
use gc_coreform::{Term, TermOrdKey, parse_term};
use predicates::prelude::*;

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
  "core/pkg-low::save-lock",
  "core/pkg-low::load-lock",
  "core/pkg::init",
  "core/pkg::add",
  "core/pkg::lock",
  "core/pkg::update",
  "core/pkg::info"
]

[store]
dir = "./.genesis/store"

[refs]
path = "./.genesis/refs.gc"

[op."core/pkg::init"]
base_dir = "."
create_dirs = true

[op."core/pkg::add"]
base_dir = "."
create_dirs = true

[op."core/pkg::lock"]
base_dir = "."

[op."core/pkg::update"]
base_dir = "."

[op."core/pkg-low::save-lock"]
base_dir = "."

[op."core/pkg-low::load-lock"]
base_dir = "."

[op."core/pkg::info"]
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

fn pkg_info(dir: &Path, caps: &Path, name: &str) -> Term {
    let out = cmd()
        .current_dir(dir)
        .args(["pkg", "--caps"])
        .arg(caps)
        .args(["info", name, "--lock", "genesis.lock"])
        .assert()
        .success()
        .get_output()
        .stdout
        .clone();
    let s = String::from_utf8(out).unwrap();
    parse_term(&s).unwrap()
}

fn locked_commit_snapshot(info: &Term) -> (String, String) {
    let Term::Map(m) = info else {
        panic!("info must be map")
    };
    let Term::Map(lk) = m
        .get(&TermOrdKey(Term::symbol(":locked")))
        .expect("missing :locked")
        .clone()
    else {
        panic!(":locked must be map");
    };
    let commit = match lk.get(&TermOrdKey(Term::symbol(":commit"))) {
        Some(Term::Str(s)) => s.clone(),
        Some(Term::Nil) => "nil".to_string(),
        other => panic!(":locked/:commit unexpected: {other:?}"),
    };
    let snapshot = match lk.get(&TermOrdKey(Term::symbol(":snapshot"))) {
        Some(Term::Str(s)) => s.clone(),
        other => panic!(":locked/:snapshot unexpected: {other:?}"),
    };
    (commit, snapshot)
}

#[test]
fn wasi_pin_vs_track_lock_and_update_are_deterministic() {
    let td = tempfile::tempdir().unwrap();
    let dir = td.path();
    let caps = write_caps(dir);

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
  :evidence []
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
  :evidence []
  :attestations []
  :message "c2"
}}"#
        ),
        "c2.gc",
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
        .args(["set", "refs/heads/main", &c1])
        .args(["--policy", &policy_h, "--expected-old", "nil"])
        .assert()
        .success();

    cmd()
        .current_dir(dir)
        .args(["pkg", "--caps"])
        .arg(&caps)
        .args(["init", "--workspace", "w", "--lock", "genesis.lock"])
        .assert()
        .success();

    cmd()
        .current_dir(dir)
        .args(["pkg", "--caps"])
        .arg(&caps)
        .args(["add", &format!("pinned@commit:{c1}")])
        .args(["--lock", "genesis.lock", "--update-policy", "manual"])
        .assert()
        .success();

    cmd()
        .current_dir(dir)
        .args(["pkg", "--caps"])
        .arg(&caps)
        .args(["add", "tracked@refs/heads/main"])
        .args(["--lock", "genesis.lock", "--update-policy", "auto"])
        .assert()
        .success();

    cmd()
        .current_dir(dir)
        .args(["pkg", "--caps"])
        .arg(&caps)
        .args(["lock", "--lock", "genesis.lock"])
        .assert()
        .success()
        .stdout(predicate::str::is_match("^[0-9a-f]{64}\n$").unwrap());

    let pinned_info = pkg_info(dir, &caps, "pinned");
    let tracked_info = pkg_info(dir, &caps, "tracked");
    let (p_commit0, p_snap0) = locked_commit_snapshot(&pinned_info);
    let (t_commit0, t_snap0) = locked_commit_snapshot(&tracked_info);
    assert_eq!(p_commit0, c1);
    assert_eq!(p_snap0, snap1);
    assert_eq!(t_commit0, c1);
    assert_eq!(t_snap0, snap1);

    cmd()
        .current_dir(dir)
        .args(["refs", "--caps"])
        .arg(&caps)
        .args(["set", "refs/heads/main", &c2])
        .args(["--policy", &policy_h])
        .assert()
        .success();

    cmd()
        .current_dir(dir)
        .args(["pkg", "--caps"])
        .arg(&caps)
        .args(["update", "--lock", "genesis.lock"])
        .assert()
        .success()
        .stdout(predicate::str::is_match("^[0-9a-f]{64}\n$").unwrap());

    let pinned_info2 = pkg_info(dir, &caps, "pinned");
    let tracked_info2 = pkg_info(dir, &caps, "tracked");
    let (p_commit1, p_snap1) = locked_commit_snapshot(&pinned_info2);
    let (t_commit1, t_snap1) = locked_commit_snapshot(&tracked_info2);

    assert_eq!(p_commit1, c1);
    assert_eq!(p_snap1, snap1);
    assert_eq!(t_commit1, c2);
    assert_eq!(t_snap1, snap2);
}
