use std::fs;
use std::path::PathBuf;

use assert_cmd::cargo::cargo_bin_cmd;
use predicates::prelude::*;

fn write_caps(dir: &std::path::Path, allow: &[&str]) -> PathBuf {
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

#[test]
fn store_put_has_get_roundtrip() {
    let td = tempfile::tempdir().unwrap();
    let dir = td.path();

    let caps = write_caps(
        dir,
        &["core/store::put", "core/store::has", "core/store::get"],
    );

    let inp = dir.join("artifact.gc");
    fs::write(&inp, "{:x 1 :y \"hi\"}\n").unwrap();

    let log_put = dir.join("put.gclog");
    let put_out = cargo_bin_cmd!("genesis")
        .current_dir(dir)
        .args(["store", "--caps"])
        .arg(&caps)
        .args(["--log"])
        .arg(&log_put)
        .args(["put", "--input"])
        .arg(&inp)
        .assert()
        .success()
        .get_output()
        .stdout
        .clone();
    let h = String::from_utf8(put_out).unwrap();
    let h = h.trim().to_string();
    assert!(predicate::str::is_match("^[0-9a-f]{64}$").unwrap().eval(&h));

    let stored = dir.join(".genesis").join("store").join(&h);
    assert!(stored.exists());
    assert!(log_put.exists());

    // has => true
    let log_has = dir.join("has.gclog");
    cargo_bin_cmd!("genesis")
        .current_dir(dir)
        .args(["store", "--caps"])
        .arg(&caps)
        .args(["--log"])
        .arg(&log_has)
        .args(["has"])
        .arg(&h)
        .assert()
        .success()
        .stdout("true\n");

    // get => artifact term printed
    let log_get = dir.join("get.gclog");
    cargo_bin_cmd!("genesis")
        .current_dir(dir)
        .args(["store", "--caps"])
        .arg(&caps)
        .args(["--log"])
        .arg(&log_get)
        .args(["get"])
        .arg(&h)
        .assert()
        .success()
        .stdout("{:x 1 :y \"hi\"}\n");

    // get --out writes canonical term, stdout empty
    let out = dir.join("out.gc");
    let log_get2 = dir.join("get2.gclog");
    cargo_bin_cmd!("genesis")
        .current_dir(dir)
        .args(["store", "--caps"])
        .arg(&caps)
        .args(["--log"])
        .arg(&log_get2)
        .args(["get"])
        .arg(&h)
        .args(["--out"])
        .arg(&out)
        .assert()
        .success()
        .stdout("");
    let out_s = fs::read_to_string(&out).unwrap();
    assert_eq!(out_s, "{:x 1 :y \"hi\"}\n");

    // has missing => false
    let log_has2 = dir.join("has2.gclog");
    cargo_bin_cmd!("genesis")
        .current_dir(dir)
        .args(["store", "--caps"])
        .arg(&caps)
        .args(["--log"])
        .arg(&log_has2)
        .args(["has"])
        .arg("0000000000000000000000000000000000000000000000000000000000000000")
        .assert()
        .success()
        .stdout("false\n");
}

#[test]
fn store_deny_by_default_is_caps_denied_exit_41() {
    let td = tempfile::tempdir().unwrap();
    let dir = td.path();

    let caps = write_caps(dir, &[]);
    let inp = dir.join("artifact.gc");
    fs::write(&inp, "{:x 1 :y \"hi\"}\n").unwrap();

    cargo_bin_cmd!("genesis")
        .current_dir(dir)
        .args(["store", "--caps"])
        .arg(&caps)
        .args(["put", "--input"])
        .arg(&inp)
        .assert()
        .code(41);
}

#[test]
fn store_get_missing_is_exit_20_and_does_not_write_out() {
    let td = tempfile::tempdir().unwrap();
    let dir = td.path();

    let caps = write_caps(dir, &["core/store::get"]);
    let out = dir.join("missing-out.gc");

    cargo_bin_cmd!("genesis")
        .current_dir(dir)
        .args(["store", "--caps"])
        .arg(&caps)
        .args(["get"])
        .arg("0000000000000000000000000000000000000000000000000000000000000000")
        .args(["--out"])
        .arg(&out)
        .assert()
        .code(20)
        .stdout(predicate::str::contains("core/store/not-found"));

    assert!(!out.exists());
}
