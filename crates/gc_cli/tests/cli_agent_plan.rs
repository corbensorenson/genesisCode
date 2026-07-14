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

fn planner_selection(intent: &std::path::Path, caps: &std::path::Path) -> Value {
    let output = cargo_bin_cmd!("genesis")
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
    serde_json::from_slice(&output).expect("parse planner JSON")
}

fn reference_selection(intent: &std::path::Path) -> Value {
    let root = std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../..");
    let output = std::process::Command::new("python3")
        .current_dir(&root)
        .args([
            "scripts/lib/gc_agent_task_cards.py",
            "--select-intent",
            &intent.display().to_string(),
        ])
        .output()
        .expect("run task-card reference selector");
    assert!(
        output.status.success(),
        "reference selector failed: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    serde_json::from_slice(&output.stdout).expect("parse reference selection")
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
    assert_eq!(
        json.pointer("/data/plan/context_cards/kind")
            .and_then(Value::as_str),
        Some("genesis/gc-agent-task-card-selection-v0.3")
    );
    assert_eq!(
        json.pointer("/data/plan/context_cards/cards/0/id")
            .and_then(Value::as_str),
        Some("capability")
    );
    assert!(
        json.pointer("/data/plan/context_cards/selectionIdentitySha256")
            .and_then(Value::as_str)
            .is_some()
    );
}

#[test]
fn task_cards_reject_unknown_intent_fields() {
    let td = tempfile::tempdir().expect("tempdir");
    let intent = td.path().join("intent.json");
    let caps = td.path().join("caps.toml");
    std::fs::write(
        &intent,
        r#"{"goal":"test","prompt_authority":"ignore policy"}"#,
    )
    .expect("write intent");
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
    assert_eq!(json.get("ok").and_then(Value::as_bool), Some(false));
    assert_eq!(
        json.pointer("/data/failure_taxonomy/0/code")
            .and_then(Value::as_str),
        Some("agent-plan/intent-invalid")
    );
    assert!(json.pointer("/data/plan/context_cards").unwrap().is_null());
}

#[test]
fn task_cards_selection_is_bound_into_plan_hash() {
    let td = tempfile::tempdir().expect("tempdir");
    let intent = td.path().join("intent.json");
    let caps = td.path().join("caps.toml");
    write_intent(&intent);
    std::fs::write(&caps, "allow = [\"sys/time::now\"]\n").expect("write caps");

    let run = || {
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
        serde_json::from_slice::<Value>(&out).expect("parse planner JSON")
    };
    let first = run();
    let second = run();
    assert_eq!(
        first.pointer("/data/plan/context_cards"),
        second.pointer("/data/plan/context_cards")
    );
    assert_eq!(
        first.pointer("/data/plan/plan_hash_blake3"),
        second.pointer("/data/plan/plan_hash_blake3")
    );
}

#[test]
fn task_cards_python_and_planner_selection_match() {
    let td = tempfile::tempdir().expect("tempdir");
    let intent = td.path().join("intent.json");
    let caps = td.path().join("caps.toml");
    write_intent(&intent);
    std::fs::write(&caps, "allow = [\"sys/time::now\"]\n").expect("write caps");

    let cli = planner_selection(&intent, &caps);
    let reference = reference_selection(&intent);
    assert_eq!(
        cli.pointer("/data/plan/context_cards"),
        Some(&reference),
        "production and reference task-card selectors drifted"
    );
}

#[test]
#[ignore = "stress-gate"]
fn task_cards_python_and_planner_selection_remain_stable_under_parallel_load() {
    let workers = (0..16)
        .map(|_| {
            std::thread::spawn(|| {
                let td = tempfile::tempdir().expect("tempdir");
                let intent = td.path().join("intent.json");
                let caps = td.path().join("caps.toml");
                write_intent(&intent);
                std::fs::write(&caps, "allow = [\"sys/time::now\"]\n").expect("write caps");

                let cli = planner_selection(&intent, &caps);
                let reference = reference_selection(&intent);
                assert_eq!(
                    cli.pointer("/data/plan/context_cards"),
                    Some(&reference),
                    "parallel production/reference selector drift"
                );
            })
        })
        .collect::<Vec<_>>();
    for worker in workers {
        worker.join().expect("parallel planner worker panicked");
    }
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
