use assert_cmd::cargo::cargo_bin_cmd;
use serde_json::Value;

#[test]
fn wasi_agent_index_emits_expected_schema_and_sources() {
    let out = cargo_bin_cmd!("genesis_wasi")
        .args(["--json", "agent-index"])
        .assert()
        .success()
        .get_output()
        .stdout
        .clone();
    let json: Value = serde_json::from_slice(&out).expect("parse json");

    assert_eq!(
        json.get("kind").and_then(Value::as_str),
        Some("genesis/agent-index-v0.1")
    );
    assert_eq!(
        json.pointer("/data/schema").and_then(Value::as_str),
        Some("genesis/agent-index-v0.1")
    );
    assert_eq!(
        json.pointer("/data/runtime_profile")
            .and_then(Value::as_str),
        Some("production")
    );
    assert_eq!(
        json.pointer("/data/capability_indices/host_abi/path")
            .and_then(Value::as_str),
        Some("docs/spec/HOST_ABI_INDEX_v0.1.json")
    );
    assert_eq!(
        json.pointer("/data/capability_indices/host_abi_schema/path")
            .and_then(Value::as_str),
        Some("docs/spec/HOST_ABI_SCHEMA_INDEX_v0.1.json")
    );
    assert_eq!(
        json.pointer("/data/selfhost_symbol_index/schema")
            .and_then(Value::as_str),
        Some("genesis/selfhost-symbol-ownership-index-v0.1")
    );
    assert_eq!(
        json.pointer("/data/selfhost_symbol_index/loaded")
            .and_then(Value::as_bool),
        Some(true)
    );
}

#[test]
fn wasi_agent_index_parity_profile_reports_runtime_profile() {
    let out = cargo_bin_cmd!("genesis_wasi_parity")
        .args(["--json", "agent-index"])
        .assert()
        .success()
        .get_output()
        .stdout
        .clone();
    let json: Value = serde_json::from_slice(&out).expect("parse json");

    assert_eq!(
        json.pointer("/data/runtime_profile")
            .and_then(Value::as_str),
        Some("parity-harness")
    );
}
