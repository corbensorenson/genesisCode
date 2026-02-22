use super::*;

#[test]
fn host_plugin_policy_gate_requires_allowlisted_plugin() {
    let policy = CapsPolicy::from_toml_str(
        r#"
allow = ["host/plugin::command"]

[op."host/plugin::command"]
allow_plugins = ["demo"]
allow_commands = ["run"]
wasi_bridge_profile = true
wasi_bridge_response = "{:ok true}"
"#,
    )
    .expect("caps");
    let mut budget = ArtifactBudgetState::default();
    let payload = Term::Map(
        [
            (
                TermOrdKey(Term::symbol(":plugin")),
                Term::Str("other".to_string()),
            ),
            (
                TermOrdKey(Term::symbol(":command")),
                Term::Str("run".to_string()),
            ),
        ]
        .into_iter()
        .collect(),
    );
    let out = call_capability(
        "host/plugin::command",
        &payload,
        policy.op_policy("host/plugin::command"),
        &policy,
        None,
        None,
        &mut budget,
        SealId(17),
    )
    .expect("call capability");
    assert_eq!(code_from_error(out), "core/caps/policy-error");
}

#[test]
fn host_plugin_policy_gate_requires_command_allowlist() {
    let policy = CapsPolicy::from_toml_str(
        r#"
allow = ["host/plugin::command"]

[op."host/plugin::command"]
allow_plugins = ["demo"]
wasi_bridge_profile = true
wasi_bridge_response = "{:ok true}"
"#,
    )
    .expect("caps");
    let mut budget = ArtifactBudgetState::default();
    let payload = Term::Map(
        [
            (
                TermOrdKey(Term::symbol(":plugin")),
                Term::Str("demo".to_string()),
            ),
            (
                TermOrdKey(Term::symbol(":command")),
                Term::Str("run".to_string()),
            ),
        ]
        .into_iter()
        .collect(),
    );
    let out = call_capability(
        "host/plugin::command",
        &payload,
        policy.op_policy("host/plugin::command"),
        &policy,
        None,
        None,
        &mut budget,
        SealId(18),
    )
    .expect("call capability");
    assert_eq!(code_from_error(out), "core/caps/policy-error");
}

#[test]
fn host_plugin_bridge_transport_requires_digest_pin() {
    let policy = CapsPolicy::from_toml_str(
        r#"
allow = ["host/plugin::command"]

[op."host/plugin::command"]
allow_plugins = ["demo"]
allow_commands = ["run"]
base_dir = "."
bridge_cmd = "bridge.sh"
"#,
    )
    .expect("caps");
    let mut budget = ArtifactBudgetState::default();
    let payload = Term::Map(
        [
            (
                TermOrdKey(Term::symbol(":plugin")),
                Term::Str("demo".to_string()),
            ),
            (
                TermOrdKey(Term::symbol(":command")),
                Term::Str("run".to_string()),
            ),
        ]
        .into_iter()
        .collect(),
    );
    let out = call_capability(
        "host/plugin::command",
        &payload,
        policy.op_policy("host/plugin::command"),
        &policy,
        None,
        None,
        &mut budget,
        SealId(19),
    )
    .expect("call capability");
    assert_eq!(code_from_error(out), "core/caps/policy-error");
}

#[test]
fn host_plugin_wasi_bridge_profile_returns_data() {
    let policy = CapsPolicy::from_toml_str(
        r#"
allow = ["host/plugin::command"]

[op."host/plugin::command"]
allow_plugins = ["demo"]
allow_commands = ["run"]
wasi_bridge_profile = true
wasi_bridge_response = "{:ok true :status \"ok\"}"
"#,
    )
    .expect("caps");
    let mut budget = ArtifactBudgetState::default();
    let payload = Term::Map(
        [
            (
                TermOrdKey(Term::symbol(":plugin")),
                Term::Str("demo".to_string()),
            ),
            (
                TermOrdKey(Term::symbol(":command")),
                Term::Symbol("run".to_string()),
            ),
        ]
        .into_iter()
        .collect(),
    );
    let out = call_capability(
        "host/plugin::command",
        &payload,
        policy.op_policy("host/plugin::command"),
        &policy,
        None,
        None,
        &mut budget,
        SealId(19),
    )
    .expect("call capability");
    let Value::Data(Term::Map(mm)) = out else {
        panic!("expected data map");
    };
    assert_eq!(
        mm.get(&TermOrdKey(Term::symbol(":status"))),
        Some(&Term::Str("ok".to_string()))
    );
}

#[test]
fn host_plugin_typed_schema_requires_schema_allowlist() {
    let policy = CapsPolicy::from_toml_str(
        r#"
allow = ["host/plugin::command"]

[op."host/plugin::command"]
allow_plugins = ["demo"]
allow_commands = ["run"]
wasi_bridge_profile = true
wasi_bridge_response = "{:ok true :result {:exit-code 0}}"
"#,
    )
    .expect("caps");
    let mut budget = ArtifactBudgetState::default();
    let payload = term_map([
        (Term::symbol(":plugin"), Term::Str("demo".to_string())),
        (Term::symbol(":command"), Term::Str("run".to_string())),
        (
            Term::symbol(":request-schema-id"),
            Term::Str("genesis/plugin.request.exec.v1".to_string()),
        ),
        (
            Term::symbol(":response-schema-id"),
            Term::Str("genesis/plugin.response.result.v1".to_string()),
        ),
        (
            Term::symbol(":payload"),
            term_map([(Term::symbol(":args"), Term::Vector(vec![]))]),
        ),
    ]);
    let out = call_capability(
        "host/plugin::command",
        &payload,
        policy.op_policy("host/plugin::command"),
        &policy,
        None,
        None,
        &mut budget,
        SealId(20),
    )
    .expect("call capability");
    assert_eq!(code_from_error(out), "core/caps/policy-error");
}

#[test]
fn host_plugin_typed_schema_rejects_bad_request_payload() {
    let policy = CapsPolicy::from_toml_str(
        r#"
allow = ["host/plugin::command"]

[op."host/plugin::command"]
allow_plugins = ["demo"]
allow_commands = ["run"]
allow_schema_ids = ["genesis/plugin.request.exec.v1", "genesis/plugin.response.result.v1"]
wasi_bridge_profile = true
wasi_bridge_response = "{:ok true :result {:exit-code 0}}"
"#,
    )
    .expect("caps");
    let mut budget = ArtifactBudgetState::default();
    let payload = term_map([
        (Term::symbol(":plugin"), Term::Str("demo".to_string())),
        (Term::symbol(":command"), Term::Str("run".to_string())),
        (
            Term::symbol(":request-schema"),
            Term::Str("genesis/plugin.request.exec.v1".to_string()),
        ),
        (
            Term::symbol(":response-schema"),
            Term::Str("genesis/plugin.response.result.v1".to_string()),
        ),
        (
            Term::symbol(":payload"),
            term_map([(Term::symbol(":method"), Term::Str("run".to_string()))]),
        ),
    ]);
    let out = call_capability(
        "host/plugin::command",
        &payload,
        policy.op_policy("host/plugin::command"),
        &policy,
        None,
        None,
        &mut budget,
        SealId(21),
    )
    .expect("call capability");
    assert_eq!(code_from_error(out), "core/caps/schema-error");
}

#[test]
fn host_plugin_typed_schema_rejects_bad_response_payload() {
    let policy = CapsPolicy::from_toml_str(
        r#"
allow = ["host/plugin::command"]

[op."host/plugin::command"]
allow_plugins = ["demo"]
allow_commands = ["run"]
allow_schema_ids = ["genesis/plugin.request.exec.v1", "genesis/plugin.response.result.v1"]
wasi_bridge_profile = true
wasi_bridge_response = "{:status \"ok\"}"
"#,
    )
    .expect("caps");
    let mut budget = ArtifactBudgetState::default();
    let payload = term_map([
        (Term::symbol(":plugin"), Term::Str("demo".to_string())),
        (Term::symbol(":command"), Term::Str("run".to_string())),
        (
            Term::symbol(":request-schema-id"),
            Term::Str("genesis/plugin.request.exec.v1".to_string()),
        ),
        (
            Term::symbol(":response-schema-id"),
            Term::Str("genesis/plugin.response.result.v1".to_string()),
        ),
        (
            Term::symbol(":payload"),
            term_map([(Term::symbol(":args"), Term::Vector(vec![]))]),
        ),
    ]);
    let out = call_capability(
        "host/plugin::command",
        &payload,
        policy.op_policy("host/plugin::command"),
        &policy,
        None,
        None,
        &mut budget,
        SealId(22),
    )
    .expect("call capability");
    assert_eq!(code_from_error(out), "core/caps/schema-error");
}

#[test]
fn host_plugin_typed_schema_accepts_valid_request_and_response() {
    let policy = CapsPolicy::from_toml_str(
        r#"
allow = ["host/plugin::command"]

[op."host/plugin::command"]
allow_plugins = ["demo"]
allow_commands = ["run"]
allow_schema_ids = ["genesis/plugin.request.exec.v1", "genesis/plugin.response.result.v1"]
wasi_bridge_profile = true
wasi_bridge_response = "{:ok true :result {:exit-code 0}}"
"#,
    )
    .expect("caps");
    let mut budget = ArtifactBudgetState::default();
    let payload = term_map([
        (Term::symbol(":plugin"), Term::Str("demo".to_string())),
        (Term::symbol(":command"), Term::Str("run".to_string())),
        (
            Term::symbol(":request-schema-id"),
            Term::Str("genesis/plugin.request.exec.v1".to_string()),
        ),
        (
            Term::symbol(":response-schema-id"),
            Term::Str("genesis/plugin.response.result.v1".to_string()),
        ),
        (
            Term::symbol(":payload"),
            term_map([
                (
                    Term::symbol(":args"),
                    Term::Vector(vec![Term::Str("--help".to_string())]),
                ),
                (Term::symbol(":cwd"), Term::Str("/tmp".to_string())),
            ]),
        ),
    ]);
    let out = call_capability(
        "host/plugin::command",
        &payload,
        policy.op_policy("host/plugin::command"),
        &policy,
        None,
        None,
        &mut budget,
        SealId(23),
    )
    .expect("call capability");
    let Value::Data(Term::Map(mm)) = out else {
        panic!("expected data map");
    };
    assert_eq!(
        mm.get(&TermOrdKey(Term::symbol(":ok"))),
        Some(&Term::Bool(true))
    );
}
