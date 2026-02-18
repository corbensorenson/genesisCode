use assert_cmd::cargo::cargo_bin_cmd;
use serde_json::Value as JsonValue;
use std::fs;

fn cmd() -> assert_cmd::Command {
    cargo_bin_cmd!("genesis")
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
