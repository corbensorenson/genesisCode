use assert_cmd::cargo::cargo_bin_cmd;
use serde_json::Value;

#[test]
fn agent_index_emits_expected_schema_and_sources() {
    let out = cargo_bin_cmd!("genesis")
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
        json.pointer("/data/cli_schema/schema")
            .and_then(Value::as_str),
        Some("genesis/cli-schema-v0.1")
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
        json.pointer("/data/capability_indices/prelude_capabilities/path")
            .and_then(Value::as_str),
        Some("docs/spec/PRELUDE_CAPABILITY_INDEX_v0.1.json")
    );
    assert_eq!(
        json.pointer("/data/selfhost_symbol_index/schema")
            .and_then(Value::as_str),
        Some("genesis/selfhost-symbol-ownership-index-v0.1")
    );
    assert_eq!(
        json.pointer("/data/selfhost_symbol_index/path")
            .and_then(Value::as_str),
        Some("selfhost/toolchain_manifest.gc")
    );
    assert_eq!(
        json.pointer("/data/selfhost_symbol_index/loaded")
            .and_then(Value::as_bool),
        Some(true)
    );
    assert!(
        json.pointer("/data/selfhost_symbol_index/symbol_count")
            .and_then(Value::as_u64)
            .is_some_and(|n| n > 0),
        "selfhost symbol index must expose owned symbols"
    );
    let symbols = json
        .pointer("/data/selfhost_symbol_index/symbols")
        .and_then(Value::as_array)
        .expect("selfhost_symbol_index.symbols");
    assert!(
        symbols.iter().any(|entry| {
            entry.get("symbol").and_then(Value::as_str) == Some("core/cli::fmt-module")
                && entry.get("module_path").and_then(Value::as_str).is_some()
        }),
        "selfhost symbol ownership index must include core/cli::fmt-module"
    );

    let workflows = json
        .pointer("/data/reference_workflows")
        .and_then(Value::as_array)
        .expect("reference_workflows");
    assert!(
        workflows
            .iter()
            .any(|w| w.get("name").and_then(Value::as_str) == Some("agent_compute_workflow")),
        "agent_compute_workflow must be discoverable in agent index"
    );
}

#[test]
fn agent_index_parity_profile_reports_runtime_profile() {
    let out = cargo_bin_cmd!("genesis_parity")
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
