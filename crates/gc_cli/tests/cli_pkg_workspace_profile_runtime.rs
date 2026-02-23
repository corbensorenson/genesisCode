use std::fs;

use assert_cmd::cargo::cargo_bin_cmd;
use gc_coreform::{Term, TermOrdKey};

#[path = "support/pkg_workspace_test_support.rs"]
mod pkg_workspace_test_support;
use pkg_workspace_test_support::{parse_coreform_value_map, write_caps};

#[test]
fn gcpm_profile_runtime_emits_profile_artifact_and_history() {
    let td = tempfile::tempdir().unwrap();
    let dir = td.path();
    let caps = write_caps(dir);
    let out_file = dir.join("runtime_profile.gc");
    let history_file = dir.join("runtime_profile_history.jsonl");

    let out = cargo_bin_cmd!("genesis")
        .current_dir(dir)
        .args(["--json", "gcpm", "--caps"])
        .arg(&caps)
        .args([
            "profile-runtime",
            "--out",
            "runtime_profile.gc",
            "--history",
            "runtime_profile_history.jsonl",
            "--min-history",
            "999",
            "--task-budget-us",
            "20000000",
            "--io-budget-us",
            "20000000",
            "--memory-budget-us",
            "20000000",
        ])
        .assert()
        .success()
        .get_output()
        .stdout
        .clone();
    let v: serde_json::Value = serde_json::from_slice(&out).unwrap();
    assert_eq!(
        v.get("kind").and_then(|x| x.as_str()),
        Some("genesis/pkg-runtime-profile-v0.1")
    );
    assert_eq!(v.get("ok").and_then(|x| x.as_bool()), Some(true));

    let map = parse_coreform_value_map(&out);
    assert_eq!(
        map.get(&TermOrdKey(Term::symbol(":ok"))),
        Some(&Term::Bool(true))
    );
    assert!(map.contains_key(&TermOrdKey(Term::symbol(":task-elapsed-us"))));
    assert!(map.contains_key(&TermOrdKey(Term::symbol(":io-elapsed-us"))));
    assert!(map.contains_key(&TermOrdKey(Term::symbol(":memory-elapsed-us"))));
    assert!(out_file.is_file());
    assert!(history_file.is_file());

    let profile_src = fs::read_to_string(&out_file).unwrap();
    let profile_t = gc_coreform::parse_term(&profile_src).unwrap();
    let Term::Map(profile_m) = profile_t else {
        panic!("runtime profile artifact must be map");
    };
    assert_eq!(
        profile_m.get(&TermOrdKey(Term::symbol(":kind"))),
        Some(&Term::symbol(":runtime-profile"))
    );
    assert!(profile_m.contains_key(&TermOrdKey(Term::symbol(":task-scheduler"))));
    assert!(profile_m.contains_key(&TermOrdKey(Term::symbol(":io-store-cycle"))));
    assert!(profile_m.contains_key(&TermOrdKey(Term::symbol(":memory-pressure"))));
}

#[test]
fn gcpm_profile_runtime_fails_closed_when_budget_is_exceeded() {
    let td = tempfile::tempdir().unwrap();
    let dir = td.path();
    let caps = write_caps(dir);

    let out = cargo_bin_cmd!("genesis")
        .current_dir(dir)
        .args(["--json", "gcpm", "--caps"])
        .arg(&caps)
        .args([
            "profile-runtime",
            "--out",
            "runtime_profile.gc",
            "--history",
            "runtime_profile_history.jsonl",
            "--no-history-append",
            "--task-budget-us",
            "1",
            "--io-budget-us",
            "1",
            "--memory-budget-us",
            "1",
        ])
        .assert()
        .code(50)
        .get_output()
        .stdout
        .clone();
    let v: serde_json::Value = serde_json::from_slice(&out).unwrap();
    assert_eq!(
        v.get("kind").and_then(|x| x.as_str()),
        Some("genesis/pkg-runtime-profile-v0.1")
    );
    assert_eq!(v.get("ok").and_then(|x| x.as_bool()), Some(false));

    let map = parse_coreform_value_map(&out);
    assert_eq!(
        map.get(&TermOrdKey(Term::symbol(":ok"))),
        Some(&Term::Bool(false))
    );
    assert_eq!(
        map.get(&TermOrdKey(Term::symbol(":task-budget-ok"))),
        Some(&Term::Bool(false))
    );
}
