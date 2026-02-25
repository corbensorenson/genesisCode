use assert_cmd::cargo::cargo_bin_cmd;
use serde_json::Value;

fn write_intent(path: &std::path::Path) {
    std::fs::write(
        path,
        r#"{
  "schema": "genesis/agent-intent-v0.1",
  "goal": "validate time-control workflow",
  "required_workflows": ["agent_time_control_workflow"],
  "domains": ["time"]
}
"#,
    )
    .expect("write intent");
}

#[test]
fn agent_plan_emits_deterministic_plan_contract() {
    let td = tempfile::tempdir().expect("tempdir");
    let intent = td.path().join("intent.json");
    let caps = td.path().join("caps.toml");
    write_intent(&intent);
    std::fs::write(&caps, "allow = [\"sys/time::now\"]\n").expect("write caps");

    let out = cargo_bin_cmd!("genesis")
        .args([
            "--json",
            "agent-plan",
            "--intent",
            &intent.display().to_string(),
            "--caps",
            &caps.display().to_string(),
        ])
        .assert()
        .success()
        .get_output()
        .stdout
        .clone();
    let json: Value = serde_json::from_slice(&out).expect("parse planner json");

    assert_eq!(
        json.get("kind").and_then(Value::as_str),
        Some("genesis/agent-plan-v0.1")
    );
    assert_eq!(json.get("ok").and_then(Value::as_bool), Some(true));
    assert_eq!(
        json.pointer("/data/schema").and_then(Value::as_str),
        Some("genesis/agent-plan-v0.1")
    );
    assert_eq!(
        json.pointer("/data/plan/policy/ok")
            .and_then(Value::as_bool),
        Some(true)
    );
    assert_eq!(
        json.pointer("/data/plan/selected_workflows/0")
            .and_then(Value::as_str),
        Some("agent_time_control_workflow")
    );
    assert!(
        json.pointer("/data/lineage/plan_hash_blake3")
            .and_then(Value::as_str)
            .is_some(),
        "planner must emit deterministic lineage hash"
    );
}

#[test]
fn agent_plan_reports_policy_failure_taxonomy_with_repair_hints() {
    let td = tempfile::tempdir().expect("tempdir");
    let intent = td.path().join("intent.json");
    let caps = td.path().join("caps.toml");
    write_intent(&intent);
    std::fs::write(&caps, "allow = []\n").expect("write caps");

    let out = cargo_bin_cmd!("genesis")
        .args([
            "--json",
            "agent-plan",
            "--intent",
            &intent.display().to_string(),
            "--caps",
            &caps.display().to_string(),
        ])
        .assert()
        .failure()
        .get_output()
        .stdout
        .clone();
    let json: Value = serde_json::from_slice(&out).expect("parse planner json");

    assert_eq!(
        json.get("kind").and_then(Value::as_str),
        Some("genesis/agent-plan-v0.1")
    );
    assert_eq!(json.get("ok").and_then(Value::as_bool), Some(false));
    let codes: Vec<&str> = json
        .pointer("/data/failure_taxonomy")
        .and_then(Value::as_array)
        .expect("failure taxonomy")
        .iter()
        .filter_map(|f| f.get("code").and_then(Value::as_str))
        .collect();
    assert!(
        codes.contains(&"agent-plan/policy-precheck-failed"),
        "expected policy-precheck failure code, got {codes:?}"
    );
    assert!(
        json.pointer("/data/repair_hints")
            .and_then(Value::as_array)
            .is_some_and(|h| !h.is_empty()),
        "planner failures must include deterministic repair hints"
    );
}
