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
  "core/vcs::merge3",
  "core/vcs::resolve-conflict",
  "core/vcs::apply",
  "core/vcs-low::diff-terms",
  "core/vcs-low::apply-patch"
]

[store]
dir = "./.genesis/store"

[op."core/vcs::merge3"]
base_dir = "."

[op."core/vcs::resolve-conflict"]
base_dir = "."

[op."core/vcs::apply"]
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

fn get_artifact_term(dir: &Path, caps: &Path, hash: &str) -> Term {
    let out = cargo_bin_cmd!("genesis")
        .current_dir(dir)
        .args(["store", "--caps"])
        .arg(caps)
        .args(["get", hash])
        .assert()
        .success()
        .get_output()
        .stdout
        .clone();
    let s = String::from_utf8(out).unwrap();
    parse_term(&s).unwrap()
}

#[test]
fn merge3_contract_snapshots_merges_disjoint_ops_and_conflicts_on_divergence() {
    let td = tempfile::tempdir().unwrap();
    let dir = td.path();
    let caps = write_caps(dir);

    // Handler artifacts (opaque values referenced by hash).
    let ha = store_put(dir, &caps, r#"{:handler "a"}"#, "ha.gc");
    let hb = store_put(dir, &caps, r#"{:handler "b"}"#, "hb.gc");
    let hc = store_put(dir, &caps, r#"{:handler "c"}"#, "hc.gc");
    let hd = store_put(dir, &caps, r#"{:handler "d"}"#, "hd.gc");

    // Base snapshot has op1 -> ha.
    let base = store_put(
        dir,
        &caps,
        &format!(
            r#"{{
  :type :vcs/snapshot
  :v 1
  :kind :contract
  :proto nil
  :overrides {{ op1 "{ha}" }}
}}"#
        ),
        "base.gc",
    );

    // Left changes op1 -> hb, adds op2 -> hc.
    let left = store_put(
        dir,
        &caps,
        &format!(
            r#"{{
  :type :vcs/snapshot
  :v 1
  :kind :contract
  :proto nil
  :overrides {{ op1 "{hb}" op2 "{hc}" }}
}}"#
        ),
        "left.gc",
    );

    // Right keeps op1 == base, adds op3 -> hd (disjoint from left op2).
    let right = store_put(
        dir,
        &caps,
        &format!(
            r#"{{
  :type :vcs/snapshot
  :v 1
  :kind :contract
  :proto nil
  :overrides {{ op1 "{ha}" op3 "{hd}" }}
}}"#
        ),
        "right.gc",
    );

    // Clean merge should pick left's op1+op2 and right's op3.
    let out_file = dir.join("merged-out.gc");
    let out = cargo_bin_cmd!("genesis")
        .current_dir(dir)
        .arg("--json")
        .args(["vcs", "--caps"])
        .arg(&caps)
        .args([
            "merge3", "--base", &base, "--left", &left, "--right", &right,
        ])
        .args(["--out", "merged-out.gc"])
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
        m.get(&TermOrdKey(Term::symbol(":ok"))),
        Some(&Term::Bool(true))
    );
    let Term::Str(merged_h) = m
        .get(&TermOrdKey(Term::symbol(":snapshot")))
        .expect("missing :snapshot")
        .clone()
    else {
        panic!(":snapshot must be string");
    };
    assert!(
        predicate::str::is_match("^[0-9a-f]{64}$")
            .unwrap()
            .eval(&merged_h)
    );
    let merged_term = get_artifact_term(dir, &caps, &merged_h);
    let Term::Map(mm) = merged_term else {
        panic!("merged snapshot must be map")
    };
    let Term::Map(ov) = mm
        .get(&TermOrdKey(Term::symbol(":overrides")))
        .expect("missing :overrides")
        .clone()
    else {
        panic!(":overrides must be map");
    };
    assert_eq!(
        ov.get(&TermOrdKey(Term::symbol("op1"))),
        Some(&Term::Str(hb.clone()))
    );
    assert_eq!(
        ov.get(&TermOrdKey(Term::symbol("op2"))),
        Some(&Term::Str(hc.clone()))
    );
    assert_eq!(
        ov.get(&TermOrdKey(Term::symbol("op3"))),
        Some(&Term::Str(hd.clone()))
    );

    // `--out` should contain the merged snapshot term (not the result map).
    assert!(out_file.exists());
    let out_s = fs::read_to_string(&out_file).unwrap();
    let out_t = parse_term(&out_s).unwrap();
    let Term::Map(out_m) = out_t else {
        panic!("out must be a map");
    };
    assert_eq!(
        out_m.get(&TermOrdKey(Term::symbol(":kind"))),
        Some(&Term::symbol(":contract"))
    );

    // Divergent change on same op: right changes op1 too -> conflict.
    let right2 = store_put(
        dir,
        &caps,
        &format!(
            r#"{{
  :type :vcs/snapshot
  :v 1
  :kind :contract
  :proto nil
  :overrides {{ op1 "{hc}" }}
}}"#
        ),
        "right2.gc",
    );

    let conf_out = dir.join("conflict-out.gc");
    let out2 = cargo_bin_cmd!("genesis")
        .current_dir(dir)
        .arg("--json")
        .args(["vcs", "--caps"])
        .arg(&caps)
        .args([
            "merge3", "--base", &base, "--left", &left, "--right", &right2,
        ])
        .args(["--out", "conflict-out.gc"])
        .assert()
        .code(3)
        .get_output()
        .stdout
        .clone();

    let t2 = parse_term(&json_value(&out2)).unwrap();
    let Term::Map(m2) = t2 else {
        panic!("expected map")
    };
    assert_eq!(
        m2.get(&TermOrdKey(Term::symbol(":ok"))),
        Some(&Term::Bool(false))
    );
    let Term::Str(conflict_h) = m2
        .get(&TermOrdKey(Term::symbol(":conflict")))
        .expect("missing :conflict")
        .clone()
    else {
        panic!(":conflict must be string");
    };
    let conf_term = get_artifact_term(dir, &caps, &conflict_h);
    let Term::Map(cm) = conf_term else {
        panic!("conflict must be map")
    };
    assert_eq!(
        cm.get(&TermOrdKey(Term::symbol(":type"))),
        Some(&Term::symbol(":vcs/conflict"))
    );

    // `--out` should contain the conflict artifact term.
    assert!(conf_out.exists());
    let conf_s = fs::read_to_string(&conf_out).unwrap();
    let conf_out_t = parse_term(&conf_s).unwrap();
    let Term::Map(conf_out_m) = conf_out_t else {
        panic!("conflict out must be map");
    };
    assert_eq!(
        conf_out_m.get(&TermOrdKey(Term::symbol(":type"))),
        Some(&Term::symbol(":vcs/conflict"))
    );
    let Term::Vector(xs) = cm
        .get(&TermOrdKey(Term::symbol(":conflicts")))
        .expect("missing :conflicts")
        .clone()
    else {
        panic!(":conflicts must be vector");
    };
    assert!(!xs.is_empty());

    // Resolve by taking left for all conflicts.
    let out3 = cargo_bin_cmd!("genesis")
        .current_dir(dir)
        .arg("--json")
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
    let t3 = parse_term(&json_value(&out3)).unwrap();
    let Term::Map(m3) = t3 else {
        panic!("expected map")
    };
    assert_eq!(
        m3.get(&TermOrdKey(Term::symbol(":ok"))),
        Some(&Term::Bool(true))
    );
    let Term::Str(resolved_h) = m3
        .get(&TermOrdKey(Term::symbol(":snapshot")))
        .expect("missing :snapshot")
        .clone()
    else {
        panic!(":snapshot must be string");
    };
    let Term::Str(patch_h) = m3
        .get(&TermOrdKey(Term::symbol(":patch")))
        .expect("missing :patch")
        .clone()
    else {
        panic!(":patch must be string");
    };

    // Applying the emitted patch to base must yield the resolved snapshot.
    let out4 = cargo_bin_cmd!("genesis")
        .current_dir(dir)
        .arg("--json")
        .args(["vcs", "--caps"])
        .arg(&caps)
        .args(["apply", "--base", &base, "--patch", &patch_h])
        .assert()
        .success()
        .get_output()
        .stdout
        .clone();
    let t4 = parse_term(&json_value(&out4)).unwrap();
    let Term::Map(m4) = t4 else {
        panic!("expected map")
    };
    assert_eq!(
        m4.get(&TermOrdKey(Term::symbol(":ok"))),
        Some(&Term::Bool(true))
    );
    let Term::Str(applied_h) = m4
        .get(&TermOrdKey(Term::symbol(":snapshot")))
        .expect("missing :snapshot")
        .clone()
    else {
        panic!(":snapshot must be string");
    };
    assert_eq!(applied_h, resolved_h);

    // Resolved snapshot must pick left's op1 and retain left's op2.
    let resolved_term = get_artifact_term(dir, &caps, &resolved_h);
    let Term::Map(rm) = resolved_term else {
        panic!("resolved snapshot must be map")
    };
    let Term::Map(rov) = rm
        .get(&TermOrdKey(Term::symbol(":overrides")))
        .expect("missing :overrides")
        .clone()
    else {
        panic!(":overrides must be map");
    };
    assert_eq!(
        rov.get(&TermOrdKey(Term::symbol("op1"))),
        Some(&Term::Str(hb.clone()))
    );
    assert_eq!(
        rov.get(&TermOrdKey(Term::symbol("op2"))),
        Some(&Term::Str(hc.clone()))
    );
}
