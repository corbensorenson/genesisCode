use std::fs;
use std::path::{Path, PathBuf};

use assert_cmd::cargo::cargo_bin_cmd;
use gc_coreform::{Term, TermOrdKey, parse_term};

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
    s.push_str("]\n\n[store]\ndir = \"./.genesis/store\"\n");
    fs::write(&caps, s).unwrap();
    caps
}

fn json_value(stdout: &[u8]) -> String {
    let v: serde_json::Value = serde_json::from_slice(stdout).unwrap();
    v.get("data")
        .and_then(|d| d.get("value"))
        .and_then(|x| x.as_str())
        .unwrap()
        .to_string()
}

fn term_map_get_i64(t: &Term, key: &str) -> i64 {
    let Term::Map(m) = t else { panic!("expected map") };
    let k = TermOrdKey(Term::symbol(key));
    match m.get(&k) {
        Some(Term::Int(i)) => i.to_string().parse::<i64>().unwrap(),
        other => panic!("missing/int key {key}: got {other:?}"),
    }
}

fn store_put(dir: &Path, caps: &Path, content: &str, log_name: &str) -> String {
    let inp = dir.join(format!("{log_name}.gc"));
    fs::write(&inp, content).unwrap();
    let log = dir.join(format!("{log_name}.gclog"));
    let out = cargo_bin_cmd!("genesis")
        .current_dir(dir)
        .arg("--json")
        .args(["store", "--caps"])
        .arg(caps)
        .args(["--log"])
        .arg(&log)
        .args(["put", "--input"])
        .arg(&inp)
        .assert()
        .success()
        .get_output()
        .stdout
        .clone();
    let v: serde_json::Value = serde_json::from_slice(&out).unwrap();
    let value_s = v.get("data").and_then(|d| d.get("value")).and_then(|x| x.as_str()).unwrap();
    // store put prints the value term; extract :hash
    let term = parse_term(value_s).unwrap();
    let Term::Map(m) = term else { panic!("expected map") };
    let h = match m.get(&TermOrdKey(Term::symbol(":hash"))) {
        Some(Term::Str(s)) => s.clone(),
        other => panic!("missing :hash: {other:?}"),
    };
    assert!(dir.join(".genesis").join("store").join(&h).exists());
    h
}

fn write_min_lock(dir: &Path, lock: &Path, rooted_snapshot: &str) {
    let s = format!(
        "version = 1\nworkspace = \"w\"\npolicy = \"policy:default-v0.1\"\n\n[locked]\n\"root\" = {{ snapshot = \"{rooted_snapshot}\" }}\n"
    );
    fs::write(dir.join(lock), s).unwrap();
}

#[test]
fn gc_plan_then_run_deletes_unreachable_artifacts() {
    let td = tempfile::tempdir().unwrap();
    let dir = td.path();

    let caps = write_caps(
        dir,
        &["core/store::put", "core/gc::plan", "core/gc::run"],
    );

    let keep_h = store_put(dir, &caps, "{:keep true}\n", "keep");
    let dead_h = store_put(dir, &caps, "{:dead true}\n", "dead");

    write_min_lock(dir, Path::new("genesis.lock"), &keep_h);

    let plan_log = dir.join("gc-plan.gclog");
    let plan_out = cargo_bin_cmd!("genesis")
        .current_dir(dir)
        .arg("--json")
        .args(["gc", "--caps"])
        .arg(&caps)
        .args(["--log"])
        .arg(&plan_log)
        .args(["plan", "--lock", "genesis.lock", "--pins", ".genesis/pins.toml", "--depth", "0"])
        .assert()
        .success()
        .get_output()
        .stdout
        .clone();
    let plan_term = parse_term(&json_value(&plan_out)).unwrap();
    assert_eq!(term_map_get_i64(&plan_term, ":dead"), 1);

    let run_log = dir.join("gc-run.gclog");
    cargo_bin_cmd!("genesis")
        .current_dir(dir)
        .arg("--json")
        .args(["gc", "--caps"])
        .arg(&caps)
        .args(["--log"])
        .arg(&run_log)
        .args(["run", "--lock", "genesis.lock", "--pins", ".genesis/pins.toml", "--depth", "0"])
        .assert()
        .success();

    assert!(dir.join(".genesis").join("store").join(&keep_h).exists());
    assert!(!dir.join(".genesis").join("store").join(&dead_h).exists());
}

#[test]
fn gc_quarantine_and_purge_roundtrip() {
    let td = tempfile::tempdir().unwrap();
    let dir = td.path();

    let caps = write_caps(
        dir,
        &[
            "core/store::put",
            "core/gc::run",
            "core/gc::purge",
        ],
    );

    let keep_h = store_put(dir, &caps, "{:keep true}\n", "keep2");
    let dead_h = store_put(dir, &caps, "{:dead true}\n", "dead2");
    write_min_lock(dir, Path::new("genesis.lock"), &keep_h);

    let run_log = dir.join("gc-run2.gclog");
    cargo_bin_cmd!("genesis")
        .current_dir(dir)
        .arg("--json")
        .args(["gc", "--caps"])
        .arg(&caps)
        .args(["--log"])
        .arg(&run_log)
        .args([
            "run",
            "--lock",
            "genesis.lock",
            "--pins",
            ".genesis/pins.toml",
            "--depth",
            "0",
            "--quarantine",
        ])
        .assert()
        .success();

    let store_dead = dir.join(".genesis").join("store").join(&dead_h);
    let q_dead = dir.join(".genesis").join("quarantine").join(&dead_h);
    assert!(!store_dead.exists());
    assert!(q_dead.exists());

    let purge_log = dir.join("gc-purge.gclog");
    cargo_bin_cmd!("genesis")
        .current_dir(dir)
        .arg("--json")
        .args(["gc", "--caps"])
        .arg(&caps)
        .args(["--log"])
        .arg(&purge_log)
        .args([
            "purge",
            "--ttl-days",
            "0",
            "--quarantine-dir",
            ".genesis/quarantine",
        ])
        .assert()
        .success();

    assert!(!q_dead.exists());
    assert!(dir.join(".genesis").join("store").join(&keep_h).exists());
}

#[test]
fn gc_pin_prevents_deletion() {
    let td = tempfile::tempdir().unwrap();
    let dir = td.path();

    let caps = write_caps(
        dir,
        &[
            "core/store::put",
            "core/gc::pin",
            "core/gc::run",
        ],
    );

    let keep_h = store_put(dir, &caps, "{:keep true}\n", "keep3");
    let pinned_h = store_put(dir, &caps, "{:pin true}\n", "pin3");
    write_min_lock(dir, Path::new("genesis.lock"), &keep_h);

    let pin_log = dir.join("gc-pin.gclog");
    cargo_bin_cmd!("genesis")
        .current_dir(dir)
        .arg("--json")
        .args(["gc", "--caps"])
        .arg(&caps)
        .args(["--log"])
        .arg(&pin_log)
        .args(["pin", &pinned_h, "--pins", ".genesis/pins.toml"])
        .assert()
        .success();

    let run_log = dir.join("gc-run3.gclog");
    cargo_bin_cmd!("genesis")
        .current_dir(dir)
        .arg("--json")
        .args(["gc", "--caps"])
        .arg(&caps)
        .args(["--log"])
        .arg(&run_log)
        .args(["run", "--lock", "genesis.lock", "--pins", ".genesis/pins.toml", "--depth", "0"])
        .assert()
        .success();

    assert!(dir.join(".genesis").join("store").join(&keep_h).exists());
    assert!(dir.join(".genesis").join("store").join(&pinned_h).exists());
}
