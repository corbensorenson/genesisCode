use assert_cmd::cargo::cargo_bin_cmd;
use serde_json::Value;

fn find_option_by_long<'a>(command: &'a Value, long_name: &str) -> Option<&'a Value> {
    command
        .get("options")
        .and_then(Value::as_array)
        .and_then(|opts| {
            opts.iter()
                .find(|opt| opt.get("long").and_then(Value::as_str) == Some(long_name))
        })
}

fn find_subcommand_by_name<'a>(command: &'a Value, name: &str) -> Option<&'a Value> {
    command
        .get("subcommands")
        .and_then(Value::as_array)
        .and_then(|subs| {
            subs.iter()
                .find(|sub| sub.get("name").and_then(Value::as_str) == Some(name))
        })
}

fn option_allowed_values(option: &Value) -> Vec<&str> {
    option
        .get("allowed_values")
        .and_then(Value::as_array)
        .map(|vals| vals.iter().filter_map(Value::as_str).collect::<Vec<_>>())
        .unwrap_or_default()
}

#[test]
fn cli_schema_production_profile_emits_selfhost_only_values() {
    let out = cargo_bin_cmd!("genesis")
        .args(["--json", "cli-schema"])
        .assert()
        .success()
        .get_output()
        .stdout
        .clone();
    let json: Value = serde_json::from_slice(&out).expect("parse json");

    assert_eq!(
        json.get("kind").and_then(Value::as_str),
        Some("genesis/cli-schema-v0.1")
    );
    assert_eq!(
        json.pointer("/data/schema").and_then(Value::as_str),
        Some("genesis/cli-schema-v0.1")
    );
    assert_eq!(
        json.pointer("/data/runtime_profile")
            .and_then(Value::as_str),
        Some("production")
    );

    let command = json.pointer("/data/command").expect("command schema");
    let frontend_opt =
        find_option_by_long(command, "coreform-frontend").expect("coreform-frontend option");
    assert_eq!(option_allowed_values(frontend_opt), vec!["selfhost"]);

    let fmt = find_subcommand_by_name(command, "fmt").expect("fmt subcommand");
    let engine_opt = find_option_by_long(fmt, "engine").expect("fmt engine option");
    assert_eq!(option_allowed_values(engine_opt), vec!["selfhost"]);
    assert_eq!(engine_opt["action"], "set");
    assert_eq!(engine_opt["value_type"], "string");
    assert_eq!(engine_opt["multiple"], false);
    let bench = find_subcommand_by_name(command, "bench").expect("bench subcommand");
    let bench_commands = bench["subcommands"]
        .as_array()
        .expect("bench command schema")
        .iter()
        .filter_map(|row| row["name"].as_str())
        .collect::<Vec<_>>();
    assert_eq!(
        bench_commands,
        vec![
            "bundle",
            "inspect",
            "registry-admit",
            "registry-build",
            "registry-init",
            "registry-verify",
            "replay",
            "run",
            "score",
            "submit",
            "validate-run"
        ]
    );
    assert_eq!(
        json.pointer("/data/mcp_interface/protocolVersion")
            .and_then(Value::as_str),
        Some("2025-11-25")
    );
    assert_eq!(
        json.pointer("/data/mcp_interface/tools")
            .and_then(Value::as_array)
            .map(Vec::len),
        Some(20)
    );
    assert_eq!(
        json.pointer("/data/mcp_interface/identitySha256")
            .and_then(Value::as_str)
            .map(str::len),
        Some(64)
    );
}

#[test]
fn cli_schema_parity_profile_emits_rust_compat_values() {
    let out = cargo_bin_cmd!("genesis_parity")
        .args(["--json", "cli-schema"])
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

    let command = json.pointer("/data/command").expect("command schema");
    let frontend_opt =
        find_option_by_long(command, "coreform-frontend").expect("coreform-frontend option");
    assert_eq!(
        option_allowed_values(frontend_opt),
        vec!["selfhost", "rust"]
    );

    let fmt = find_subcommand_by_name(command, "fmt").expect("fmt subcommand");
    let engine_opt = find_option_by_long(fmt, "engine").expect("fmt engine option");
    assert_eq!(option_allowed_values(engine_opt), vec!["selfhost", "rust"]);
}
