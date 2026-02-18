use std::fs;
use std::path::{Path, PathBuf};

use assert_cmd::cargo::cargo_bin_cmd;
use gc_coreform::{
    Term, TermOrdKey, canonicalize_module, hash_module, parse_module, parse_term, print_term,
};
use predicates::prelude::*;

mod common;

fn cmd() -> assert_cmd::Command {
    let mut c = cargo_bin_cmd!("genesis_wasi");
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
    s.push_str(
        "]\n\n[store]\ndir = \"./.genesis/store\"\n\n[refs]\npath = \"./.genesis/refs.gc\"\n",
    );
    fs::write(&caps, s).unwrap();
    caps
}

fn build_selfhost_artifact(dir: &Path) -> PathBuf {
    common::copy_repo_selfhost_toolchain_artifact(dir)
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

    let module_src = match cli_mod.get(&TermOrdKey(Term::symbol(":source"))) {
        Some(Term::Str(src)) => src.clone(),
        _ => panic!("cli module missing :source"),
    };
    let poisoned_src = format!("{module_src}\n(def core/cli::vcs-log-program \"shadowed\")\n");
    let poisoned_forms = canonicalize_module(parse_module(&poisoned_src).unwrap()).unwrap();
    let poisoned_hash = hash_module(&poisoned_forms);
    cli_mod.insert(
        TermOrdKey(Term::symbol(":source")),
        Term::Str(poisoned_src.to_string()),
    );
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

    let caps = write_caps(
        dir,
        &["core/store::put", "core/store::get", "core/vcs-low::log"],
    );
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

    let caps = write_caps(
        dir,
        &["core/store::put", "core/store::get", "core/vcs-low::log"],
    );
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

#[test]
fn vcs_blame_and_why_values_match_between_frontends() {
    let td = tempfile::tempdir().unwrap();
    let dir = td.path();

    let caps = write_caps(
        dir,
        &[
            "core/store::put",
            "core/store::get",
            "core/refs::set",
            "core/refs::list",
            "core/vcs-low::blame",
            "core/vcs-low::why",
        ],
    );
    let artifact = build_selfhost_artifact(dir);

    let def1 = store_put(dir, &caps, "1", "def1.gc");
    let patch1 = store_put(dir, &caps, "{:type :vcs/patch :v 1 :ops []}\n", "patch1.gc");
    let patch2 = store_put(dir, &caps, "{:type :vcs/patch :v 1 :ops []}\n", "patch2.gc");
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
        .arg(&c2)
        .args(["--policy", &policy_h, "--expected-old", "nil"])
        .assert()
        .success();

    let rust_blame = cmd()
        .current_dir(dir)
        .arg("--json")
        .args(["--coreform-frontend", "rust"])
        .args(["vcs", "--caps"])
        .arg(&caps)
        .args(["blame", "--snapshot", &snap2, "--sym", "pkg/mod::x"])
        .assert()
        .success()
        .get_output()
        .stdout
        .clone();
    let self_blame = cmd()
        .current_dir(dir)
        .arg("--json")
        .args(["--coreform-frontend", "selfhost"])
        .args(["--selfhost-artifact", artifact.to_str().unwrap()])
        .args(["vcs", "--caps"])
        .arg(&caps)
        .args(["blame", "--snapshot", &snap2, "--sym", "pkg/mod::x"])
        .assert()
        .success()
        .get_output()
        .stdout
        .clone();
    assert_eq!(json_frontend_name(&rust_blame), "rust");
    assert_eq!(json_frontend_name(&self_blame), "selfhost");
    assert_eq!(json_value(&rust_blame), json_value(&self_blame));

    let rust_why = cmd()
        .current_dir(dir)
        .arg("--json")
        .args(["--coreform-frontend", "rust"])
        .args(["vcs", "--caps"])
        .arg(&caps)
        .args(["why", "--snapshot", &snap2, "--sym", "pkg/mod::x"])
        .assert()
        .success()
        .get_output()
        .stdout
        .clone();
    let self_why = cmd()
        .current_dir(dir)
        .arg("--json")
        .args(["--coreform-frontend", "selfhost"])
        .args(["--selfhost-artifact", artifact.to_str().unwrap()])
        .args(["vcs", "--caps"])
        .arg(&caps)
        .args(["why", "--snapshot", &snap2, "--sym", "pkg/mod::x"])
        .assert()
        .success()
        .get_output()
        .stdout
        .clone();
    assert_eq!(json_frontend_name(&rust_why), "rust");
    assert_eq!(json_frontend_name(&self_why), "selfhost");
    assert_eq!(json_value(&rust_why), json_value(&self_why));
}

#[test]
fn vcs_diff_and_apply_values_match_between_frontends() {
    let td = tempfile::tempdir().unwrap();
    let dir = td.path();

    let caps = write_caps(
        dir,
        &[
            "core/store::put",
            "core/store::get",
            "core/vcs-low::diff",
            "core/vcs-low::apply",
            "core/vcs-low::diff-terms",
            "core/vcs-low::apply-patch",
        ],
    );
    let artifact = build_selfhost_artifact(dir);

    let h1 = store_put(dir, &caps, "1", "v1.gc");
    let h2 = store_put(dir, &caps, "2", "v2.gc");
    let h3 = store_put(dir, &caps, "3", "v3.gc");

    let base_snap = format!(
        r#"{{
  :type :vcs/snapshot
  :v 1
  :kind :contract
  :proto nil
  :overrides {{ my/op::a "{h1}" }}
}}"#
    );
    let to_snap = format!(
        r#"{{
  :type :vcs/snapshot
  :v 1
  :kind :contract
  :proto nil
  :overrides {{ my/op::a "{h2}" my/op::b "{h3}" }}
}}"#
    );

    let base_h = store_put(dir, &caps, &base_snap, "base.gc");
    let to_h = store_put(dir, &caps, &to_snap, "to.gc");

    let rust_diff = cmd()
        .current_dir(dir)
        .arg("--json")
        .args(["--coreform-frontend", "rust"])
        .args(["vcs", "--caps"])
        .arg(&caps)
        .args(["diff", "--base", &base_h, "--to", &to_h])
        .assert()
        .success()
        .get_output()
        .stdout
        .clone();
    let self_diff = cmd()
        .current_dir(dir)
        .arg("--json")
        .args(["--coreform-frontend", "selfhost"])
        .args(["--selfhost-artifact", artifact.to_str().unwrap()])
        .args(["vcs", "--caps"])
        .arg(&caps)
        .args(["diff", "--base", &base_h, "--to", &to_h])
        .assert()
        .success()
        .get_output()
        .stdout
        .clone();

    assert_eq!(json_frontend_name(&rust_diff), "rust");
    assert_eq!(json_frontend_name(&self_diff), "selfhost");
    assert_eq!(json_value(&rust_diff), json_value(&self_diff));

    let rust_diff_term = parse_term(&json_value(&rust_diff)).unwrap();
    let Term::Map(rust_diff_map) = rust_diff_term else {
        panic!("diff value must be map");
    };
    let patch_h = match rust_diff_map.get(&TermOrdKey(Term::symbol(":patch"))) {
        Some(Term::Str(s)) => s.clone(),
        _ => panic!("diff value missing :patch string"),
    };

    let rust_apply = cmd()
        .current_dir(dir)
        .arg("--json")
        .args(["--coreform-frontend", "rust"])
        .args(["vcs", "--caps"])
        .arg(&caps)
        .args(["apply", "--base", &base_h, "--patch", &patch_h])
        .assert()
        .success()
        .get_output()
        .stdout
        .clone();
    let self_apply = cmd()
        .current_dir(dir)
        .arg("--json")
        .args(["--coreform-frontend", "selfhost"])
        .args(["--selfhost-artifact", artifact.to_str().unwrap()])
        .args(["vcs", "--caps"])
        .arg(&caps)
        .args(["apply", "--base", &base_h, "--patch", &patch_h])
        .assert()
        .success()
        .get_output()
        .stdout
        .clone();

    assert_eq!(json_frontend_name(&rust_apply), "rust");
    assert_eq!(json_frontend_name(&self_apply), "selfhost");
    assert_eq!(json_value(&rust_apply), json_value(&self_apply));

    let rust_apply_term = parse_term(&json_value(&rust_apply)).unwrap();
    let Term::Map(rust_apply_map) = rust_apply_term else {
        panic!("apply value must be map");
    };
    match rust_apply_map.get(&TermOrdKey(Term::symbol(":snapshot"))) {
        Some(Term::Str(s)) => assert_eq!(s, &to_h),
        _ => panic!("apply value missing :snapshot string"),
    }
}

#[test]
fn wasi_vcs_diff_and_apply_selfhost_work_with_low_level_caps_only() {
    let td = tempfile::tempdir().unwrap();
    let dir = td.path();

    let caps = write_caps(
        dir,
        &[
            "core/store::put",
            "core/store::get",
            "core/vcs-low::diff-terms",
            "core/vcs-low::apply-patch",
        ],
    );
    let artifact = build_selfhost_artifact(dir);

    let h1 = store_put(dir, &caps, "1", "v1.gc");
    let h2 = store_put(dir, &caps, "2", "v2.gc");
    let h3 = store_put(dir, &caps, "3", "v3.gc");

    let base_snap = format!(
        r#"{{
  :type :vcs/snapshot
  :v 1
  :kind :contract
  :proto nil
  :overrides {{ my/op::a "{h1}" }}
}}"#
    );
    let to_snap = format!(
        r#"{{
  :type :vcs/snapshot
  :v 1
  :kind :contract
  :proto nil
  :overrides {{ my/op::a "{h2}" my/op::b "{h3}" }}
}}"#
    );
    let base_h = store_put(dir, &caps, &base_snap, "base.gc");
    let to_h = store_put(dir, &caps, &to_snap, "to.gc");

    let self_diff = cmd()
        .current_dir(dir)
        .arg("--json")
        .args(["--coreform-frontend", "selfhost"])
        .args(["--selfhost-artifact", artifact.to_str().unwrap()])
        .args(["vcs", "--caps"])
        .arg(&caps)
        .args(["diff", "--base", &base_h, "--to", &to_h])
        .assert()
        .success()
        .get_output()
        .stdout
        .clone();
    assert_eq!(json_frontend_name(&self_diff), "selfhost");

    let diff_term = parse_term(&json_value(&self_diff)).unwrap();
    let Term::Map(diff_map) = diff_term else {
        panic!("diff value must be map");
    };
    let patch_h = match diff_map.get(&TermOrdKey(Term::symbol(":patch"))) {
        Some(Term::Str(s)) => s.clone(),
        _ => panic!("diff value missing :patch string"),
    };

    let self_apply = cmd()
        .current_dir(dir)
        .arg("--json")
        .args(["--coreform-frontend", "selfhost"])
        .args(["--selfhost-artifact", artifact.to_str().unwrap()])
        .args(["vcs", "--caps"])
        .arg(&caps)
        .args(["apply", "--base", &base_h, "--patch", &patch_h])
        .assert()
        .success()
        .get_output()
        .stdout
        .clone();
    assert_eq!(json_frontend_name(&self_apply), "selfhost");

    let apply_term = parse_term(&json_value(&self_apply)).unwrap();
    let Term::Map(apply_map) = apply_term else {
        panic!("apply value must be map");
    };
    match apply_map.get(&TermOrdKey(Term::symbol(":snapshot"))) {
        Some(Term::Str(s)) => assert_eq!(s, &to_h),
        _ => panic!("apply value missing :snapshot string"),
    }
}

#[test]
fn vcs_merge3_and_resolve_conflict_values_match_between_frontends() {
    let td = tempfile::tempdir().unwrap();
    let dir = td.path();

    let caps = write_caps(
        dir,
        &[
            "core/store::put",
            "core/store::get",
            "core/vcs-low::merge3",
            "core/vcs-low::resolve-conflict",
            "core/vcs-low::resolve-conflict-legacy",
            "core/vcs-low::merge3-contract-snapshots",
            "core/vcs-low::diff-terms",
        ],
    );
    let artifact = build_selfhost_artifact(dir);

    let h1 = store_put(dir, &caps, "1", "v1.gc");
    let h2 = store_put(dir, &caps, "2", "v2.gc");
    let h3 = store_put(dir, &caps, "3", "v3.gc");

    let base_snap = format!(
        r#"{{
  :type :vcs/snapshot
  :v 1
  :kind :contract
  :proto nil
  :overrides {{ my/op::a "{h1}" }}
}}"#
    );
    let left_snap = format!(
        r#"{{
  :type :vcs/snapshot
  :v 1
  :kind :contract
  :proto nil
  :overrides {{ my/op::a "{h2}" }}
}}"#
    );
    let right_snap = format!(
        r#"{{
  :type :vcs/snapshot
  :v 1
  :kind :contract
  :proto nil
  :overrides {{ my/op::a "{h3}" }}
}}"#
    );

    let base_h = store_put(dir, &caps, &base_snap, "base.gc");
    let left_h = store_put(dir, &caps, &left_snap, "left.gc");
    let right_h = store_put(dir, &caps, &right_snap, "right.gc");

    let rust_merge3 = cmd()
        .current_dir(dir)
        .arg("--json")
        .args(["--coreform-frontend", "rust"])
        .args(["vcs", "--caps"])
        .arg(&caps)
        .args([
            "merge3", "--base", &base_h, "--left", &left_h, "--right", &right_h,
        ])
        .assert()
        .code(3)
        .get_output()
        .stdout
        .clone();
    let self_merge3 = cmd()
        .current_dir(dir)
        .arg("--json")
        .args(["--coreform-frontend", "selfhost"])
        .args(["--selfhost-artifact", artifact.to_str().unwrap()])
        .args(["vcs", "--caps"])
        .arg(&caps)
        .args([
            "merge3", "--base", &base_h, "--left", &left_h, "--right", &right_h,
        ])
        .assert()
        .code(3)
        .get_output()
        .stdout
        .clone();

    assert_eq!(json_frontend_name(&rust_merge3), "rust");
    assert_eq!(json_frontend_name(&self_merge3), "selfhost");
    assert_eq!(json_value(&rust_merge3), json_value(&self_merge3));

    let rust_merge3_term = parse_term(&json_value(&rust_merge3)).unwrap();
    let Term::Map(rust_merge3_map) = rust_merge3_term else {
        panic!("merge3 value must be map");
    };
    let conflict_h = match rust_merge3_map.get(&TermOrdKey(Term::symbol(":conflict"))) {
        Some(Term::Str(s)) => s.clone(),
        _ => panic!("merge3 value missing :conflict string"),
    };

    let rust_resolve = cmd()
        .current_dir(dir)
        .arg("--json")
        .args(["--coreform-frontend", "rust"])
        .args(["vcs", "--caps"])
        .arg(&caps)
        .args([
            "resolve-conflict",
            "--conflict",
            &conflict_h,
            "--strategy",
            "left",
        ])
        .assert()
        .success()
        .get_output()
        .stdout
        .clone();
    let self_resolve = cmd()
        .current_dir(dir)
        .arg("--json")
        .args(["--coreform-frontend", "selfhost"])
        .args(["--selfhost-artifact", artifact.to_str().unwrap()])
        .args(["vcs", "--caps"])
        .arg(&caps)
        .args([
            "resolve-conflict",
            "--conflict",
            &conflict_h,
            "--strategy",
            "left",
        ])
        .assert()
        .success()
        .get_output()
        .stdout
        .clone();

    assert_eq!(json_frontend_name(&rust_resolve), "rust");
    assert_eq!(json_frontend_name(&self_resolve), "selfhost");
    assert_eq!(json_value(&rust_resolve), json_value(&self_resolve));

    let rust_resolve_term = parse_term(&json_value(&rust_resolve)).unwrap();
    let Term::Map(rust_resolve_map) = rust_resolve_term else {
        panic!("resolve-conflict value must be map");
    };
    match rust_resolve_map.get(&TermOrdKey(Term::symbol(":snapshot"))) {
        Some(Term::Str(s)) => assert_eq!(s, &left_h),
        _ => panic!("resolve-conflict value missing :snapshot string"),
    }
}
