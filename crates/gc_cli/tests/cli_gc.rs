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
    s.push_str(
        "]\n\n[store]\ndir = \"./.genesis/store\"\n\n[refs]\npath = \"./.genesis/refs.gc\"\n",
    );
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
    let Term::Map(m) = t else {
        panic!("expected map")
    };
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
    let value_s = v
        .get("data")
        .and_then(|d| d.get("value"))
        .and_then(|x| x.as_str())
        .unwrap();
    // store put prints the value term; extract :hash
    let term = parse_term(value_s).unwrap();
    let Term::Map(m) = term else {
        panic!("expected map")
    };
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

    let caps = write_caps(dir, &["core/store::put", "core/gc::plan", "core/gc::run"]);

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
        .args([
            "plan",
            "--lock",
            "genesis.lock",
            "--pins",
            ".genesis/pins.toml",
            "--depth",
            "0",
        ])
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
        .args([
            "run",
            "--lock",
            "genesis.lock",
            "--pins",
            ".genesis/pins.toml",
            "--depth",
            "0",
        ])
        .assert()
        .success();

    assert!(dir.join(".genesis").join("store").join(&keep_h).exists());
    assert!(!dir.join(".genesis").join("store").join(&dead_h).exists());
}

#[test]
fn gc_quarantine_and_purge_roundtrip() {
    let td = tempfile::tempdir().unwrap();
    let dir = td.path();

    let caps = write_caps(dir, &["core/store::put", "core/gc::run", "core/gc::purge"]);

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

    let caps = write_caps(dir, &["core/store::put", "core/gc::pin", "core/gc::run"]);

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
        .args([
            "run",
            "--lock",
            "genesis.lock",
            "--pins",
            ".genesis/pins.toml",
            "--depth",
            "0",
        ])
        .assert()
        .success();

    assert!(dir.join(".genesis").join("store").join(&keep_h).exists());
    assert!(dir.join(".genesis").join("store").join(&pinned_h).exists());
}

#[test]
fn gc_unpin_allows_reclaim_after_run() {
    let td = tempfile::tempdir().unwrap();
    let dir = td.path();

    let caps = write_caps(
        dir,
        &[
            "core/store::put",
            "core/gc::pin",
            "core/gc::unpin",
            "core/gc::run",
        ],
    );

    let keep_h = store_put(dir, &caps, "{:keep true}\n", "keep4");
    let pinned_h = store_put(dir, &caps, "{:pin true}\n", "pin4");
    write_min_lock(dir, Path::new("genesis.lock"), &keep_h);

    let pin_log = dir.join("gc-pin4.gclog");
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

    let unpin_log = dir.join("gc-unpin4.gclog");
    cargo_bin_cmd!("genesis")
        .current_dir(dir)
        .arg("--json")
        .args(["gc", "--caps"])
        .arg(&caps)
        .args(["--log"])
        .arg(&unpin_log)
        .args(["unpin", &pinned_h, "--pins", ".genesis/pins.toml"])
        .assert()
        .success();

    let run_log = dir.join("gc-run4.gclog");
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
        ])
        .assert()
        .success();

    assert!(dir.join(".genesis").join("store").join(&keep_h).exists());
    assert!(!dir.join(".genesis").join("store").join(&pinned_h).exists());
}

#[test]
fn gc_pin_ref_keeps_target_even_with_no_refs_root_scan() {
    let td = tempfile::tempdir().unwrap();
    let dir = td.path();

    let caps = write_caps(
        dir,
        &[
            "core/store::put",
            "core/gc::pin",
            "core/gc::run",
            "core/refs::set",
        ],
    );

    let kept_via_ref_h = store_put(dir, &caps, "{:kept-via-ref true}\n", "keep_ref");
    let dead_h = store_put(dir, &caps, "{:dead true}\n", "dead_ref");

    let refs_path = dir.join(".genesis").join("refs.gc");
    let refs_db = gc_effects::RefsDb::open(&refs_path).unwrap();
    refs_db
        .set("refs/heads/main", Some(&kept_via_ref_h), None)
        .unwrap();

    let pin_log = dir.join("gc-pin-ref.gclog");
    cargo_bin_cmd!("genesis")
        .current_dir(dir)
        .arg("--json")
        .args(["gc", "--caps"])
        .arg(&caps)
        .args(["--log"])
        .arg(&pin_log)
        .args(["pin", "refs/heads/main", "--pins", ".genesis/pins.toml"])
        .assert()
        .success();

    let run_log = dir.join("gc-run-ref.gclog");
    cargo_bin_cmd!("genesis")
        .current_dir(dir)
        .arg("--json")
        .args(["gc", "--caps"])
        .arg(&caps)
        .args(["--log"])
        .arg(&run_log)
        .args([
            "run",
            "--pins",
            ".genesis/pins.toml",
            "--depth",
            "0",
            "--no-lock",
            "--no-refs",
        ])
        .assert()
        .success();

    assert!(
        dir.join(".genesis")
            .join("store")
            .join(&kept_via_ref_h)
            .exists()
    );
    assert!(!dir.join(".genesis").join("store").join(&dead_h).exists());
}

#[test]
fn gc_keeps_tag_ref_commit_closure_and_prunes_unreachable() {
    let td = tempfile::tempdir().unwrap();
    let dir = td.path();

    let caps = write_caps(dir, &["core/store::put", "core/gc::run"]);

    let patch_h = store_put(
        dir,
        &caps,
        r#"{:type :vcs/patch :v 1 :ops []}"#,
        "gc_tag_patch",
    );
    let snap_h = store_put(
        dir,
        &caps,
        r#"{:type :vcs/snapshot :v 1 :kind :package :pkg/name "x" :pkg/version "0" :modules [] :obligations []}"#,
        "gc_tag_snap",
    );
    let evidence_h = store_put(
        dir,
        &caps,
        r#"{:type :vcs/evidence :v 1 :kind :unit-tests :data nil}"#,
        "gc_tag_evidence",
    );
    let attestation_h = store_put(
        dir,
        &caps,
        r#"{:type :vcs/attestation :v 1 :alg "ed25519" :signing-h b"" :pk b"" :sig b""}"#,
        "gc_tag_attestation",
    );
    let commit_h = store_put(
        dir,
        &caps,
        &format!(
            r#"{{
  :type :vcs/commit
  :v 1
  :parents []
  :target {{ :kind :package :name "x" }}
  :base nil
  :patch "{patch_h}"
  :result "{snap_h}"
  :obligations [core/obligation::unit-tests]
  :evidence ["{evidence_h}"]
  :attestations ["{attestation_h}"]
  :message "release"
}}"#
        ),
        "gc_tag_commit",
    );
    let dead_h = store_put(dir, &caps, "{:dead true}\n", "gc_tag_dead");

    let refs_path = dir.join(".genesis").join("refs.gc");
    let refs_db = gc_effects::RefsDb::open(&refs_path).unwrap();
    refs_db
        .set("refs/tags/v1.0.0", Some(&commit_h), None)
        .unwrap();

    let run_log = dir.join("gc-tag-run.gclog");
    cargo_bin_cmd!("genesis")
        .current_dir(dir)
        .arg("--json")
        .args(["gc", "--caps"])
        .arg(&caps)
        .args(["--log"])
        .arg(&run_log)
        .args([
            "run",
            "--pins",
            ".genesis/pins.toml",
            "--depth",
            "0",
            "--no-lock",
        ])
        .assert()
        .success();

    let store_dir = dir.join(".genesis").join("store");
    for h in [&patch_h, &snap_h, &evidence_h, &attestation_h, &commit_h] {
        assert!(
            store_dir.join(h).exists(),
            "expected retained artifact: {h}"
        );
    }
    assert!(
        !store_dir.join(&dead_h).exists(),
        "dead artifact should be pruned"
    );
}
