use assert_cmd::cargo::cargo_bin_cmd;
use serde_json::Value as JsonValue;
use std::fs;
use std::path::{Path, PathBuf};

fn repo_root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../..")
        .canonicalize()
        .unwrap_or_else(|_| PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../.."))
}

fn repo_toolchain_artifact() -> PathBuf {
    repo_root().join("selfhost").join("toolchain.gc")
}

fn cmd() -> assert_cmd::Command {
    let mut cmd = cargo_bin_cmd!("genesis");
    cmd.env("GENESIS_SELFHOST_TOOLCHAIN_ARTIFACT", repo_toolchain_artifact());
    cmd
}

#[test]
fn warm_mode_handles_multiple_requests_in_one_process() {
    let td = tempfile::tempdir().unwrap();
    let file = td.path().join("prog.gc");
    fs::write(&file, "(prim int/add 2 3)\n").unwrap();

    let req1 = serde_json::json!({
        "argv": ["--json", "eval", file.to_str().unwrap()]
    });
    let req2 = serde_json::json!({
        "argv": ["--json", "fmt", file.to_str().unwrap(), "--check"]
    });
    let req3 = serde_json::json!({
        "argv": ["exit"]
    });
    let input = format!("{req1}\n{req2}\n{req3}\n");

    let out = cmd()
        .arg("warm")
        .write_stdin(input)
        .assert()
        .success()
        .get_output()
        .stdout
        .clone();
    let stdout = String::from_utf8(out).unwrap();
    let lines: Vec<&str> = stdout.lines().filter(|l| !l.trim().is_empty()).collect();
    assert_eq!(lines.len(), 2, "unexpected warm output lines: {stdout}");

    let r1: JsonValue = serde_json::from_str(lines[0]).unwrap();
    assert!(r1.get("ok").and_then(|v| v.as_bool()).unwrap_or(false));
    assert_eq!(r1["kind"], "genesis/warm-response-v0.1");
    assert_eq!(r1["data"]["result"]["kind"], "genesis/eval-v0.2");

    let r2: JsonValue = serde_json::from_str(lines[1]).unwrap();
    assert!(r2.get("ok").and_then(|v| v.as_bool()).unwrap_or(false));
    assert_eq!(r2["kind"], "genesis/warm-response-v0.1");
    assert_eq!(r2["data"]["result"]["kind"], "genesis/fmt-v0.2");
}

#[test]
fn warm_mode_rejects_nested_warm_request() {
    let req = serde_json::json!({
        "argv": ["warm"]
    });
    let stop = serde_json::json!({
        "argv": ["exit"]
    });
    let input = format!("{req}\n{stop}\n");
    let out = cmd()
        .arg("warm")
        .write_stdin(input)
        .assert()
        .success()
        .get_output()
        .stdout
        .clone();
    let stdout = String::from_utf8(out).unwrap();
    let lines: Vec<&str> = stdout.lines().filter(|l| !l.trim().is_empty()).collect();
    assert_eq!(lines.len(), 1, "unexpected warm output lines: {stdout}");
    let r: JsonValue = serde_json::from_str(lines[0]).unwrap();
    assert!(!r.get("ok").and_then(|v| v.as_bool()).unwrap_or(true));
    assert_eq!(r["error"]["code"], "warm/nested");
}

#[test]
fn warm_mode_matches_cold_json_and_exposes_stable_session_cache_key() {
    let td = tempfile::tempdir().unwrap();
    let file = td.path().join("prog.gc");
    fs::write(&file, "(prim int/add 9 4)\n").unwrap();

    let cold_out = cmd()
        .arg("--json")
        .arg("eval")
        .arg(&file)
        .assert()
        .success()
        .get_output()
        .stdout
        .clone();
    let cold_json: JsonValue = serde_json::from_slice(&cold_out).unwrap();

    let req = serde_json::json!({
        "argv": ["--json", "eval", file.to_str().unwrap()]
    });
    let stop = serde_json::json!({
        "argv": ["exit"]
    });
    let input = format!("{req}\n{stop}\n");
    let out = cmd()
        .arg("warm")
        .write_stdin(input)
        .assert()
        .success()
        .get_output()
        .stdout
        .clone();
    let stdout = String::from_utf8(out).unwrap();
    let lines: Vec<&str> = stdout.lines().filter(|l| !l.trim().is_empty()).collect();
    assert_eq!(lines.len(), 1, "unexpected warm output lines: {stdout}");
    let warm_json: JsonValue = serde_json::from_str(lines[0]).unwrap();

    assert!(warm_json["ok"].as_bool().unwrap_or(false));
    assert_eq!(warm_json["kind"], "genesis/warm-response-v0.1");
    let warm_result = warm_json["data"]["result"].clone();
    assert_eq!(warm_result, cold_json);

    let key = warm_json["data"]["session_cache_key"]
        .as_str()
        .unwrap_or_default()
        .to_string();
    assert_eq!(key.len(), 64);
    assert!(
        key.chars().all(|c| c.is_ascii_hexdigit()),
        "session_cache_key must be hex, got {key}"
    );
}
