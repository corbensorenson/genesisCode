use super::{ArtifactBudgetState, backend_unavailable_message, call_capability};
use crate::CapsPolicy;
use gc_coreform::{Term, TermOrdKey};
use gc_kernel::{SealId, Value};
use tempfile::tempdir;

fn code_from_error(v: Value) -> String {
    let Value::Sealed { payload, .. } = v else {
        panic!("expected sealed error value");
    };
    let Value::Data(Term::Map(mm)) = *payload else {
        panic!("expected sealed payload map");
    };
    let Some(Term::Str(code)) = mm.get(&TermOrdKey(Term::symbol(":error/code"))) else {
        panic!("expected :error/code in payload map");
    };
    code.clone()
}

fn msg_from_error(v: Value) -> String {
    let Value::Sealed { payload, .. } = v else {
        panic!("expected sealed error value");
    };
    let Value::Data(Term::Map(mm)) = *payload else {
        panic!("expected sealed payload map");
    };
    let Some(Term::Str(msg)) = mm.get(&TermOrdKey(Term::symbol(":error/message"))) else {
        panic!("expected :error/message in payload map");
    };
    msg.clone()
}

fn term_map(rows: impl IntoIterator<Item = (Term, Term)>) -> Term {
    Term::Map(rows.into_iter().map(|(k, v)| (TermOrdKey(k), v)).collect())
}

#[test]
fn stable_host_integrated_ops_report_backend_unavailable_actionably() {
    let policy =
        CapsPolicy::from_toml_str(r#"allow = ["editor/task::typecheck-pkg"]"#).expect("caps");
    let mut budget = ArtifactBudgetState::default();
    let out = call_capability(
        "editor/task::typecheck-pkg",
        &Term::Nil,
        policy.op_policy("editor/task::typecheck-pkg"),
        &policy,
        None,
        None,
        &mut budget,
        SealId(1),
    )
    .expect("call capability");
    assert_eq!(
        code_from_error(out.clone()),
        "core/caps/backend-unavailable"
    );
    let msg = msg_from_error(out);
    assert!(
        msg.contains("editor host integration is bridge-backed"),
        "message must be actionable for editor ops: {msg}"
    );
}

#[test]
fn backend_unavailable_message_guides_gpu_compute_configuration() {
    let msg = backend_unavailable_message("gpu/compute::submit");
    assert!(msg.contains("bridge_cmd/bridge_args/bridge_cmd_sha256"));
    assert!(msg.contains("first-party runtime backend"));
}

#[test]
fn io_net_http_request_policy_gate_enforces_remote_allowlist() {
    let policy = CapsPolicy::from_toml_str(
        r#"
allow = ["io/net::http-request"]

[op."io/net::http-request"]
url_allow = ["https://registry.example.com/api/"]
wasi_network_profile = "preview2"
wasi_bridge_profile = true
wasi_bridge_response = "{:ok true :status 200 :body \"ok\"}"
"#,
    )
    .expect("caps");
    let mut budget = ArtifactBudgetState::default();
    let payload = Term::Map(
        [(
            TermOrdKey(Term::symbol(":url")),
            Term::Str("https://evil.example.com/api/ping".to_string()),
        )]
        .into_iter()
        .collect(),
    );
    let out = call_capability(
        "io/net::http-request",
        &payload,
        policy.op_policy("io/net::http-request"),
        &policy,
        None,
        None,
        &mut budget,
        SealId(7),
    )
    .expect("call capability");
    assert_eq!(code_from_error(out), "core/caps/policy-error");
}

#[test]
fn io_net_http_request_wasi_bridge_profile_returns_data() {
    let policy = CapsPolicy::from_toml_str(
        r#"
allow = ["io/net::http-request"]

[op."io/net::http-request"]
url_allow = ["https://registry.example.com/api/"]
wasi_network_profile = "preview2"
wasi_bridge_profile = true
wasi_bridge_response = "{:ok true :status 200 :body \"ok\"}"
"#,
    )
    .expect("caps");
    let mut budget = ArtifactBudgetState::default();
    let payload = Term::Map(
        [(
            TermOrdKey(Term::symbol(":url")),
            Term::Str("https://registry.example.com/api/ping".to_string()),
        )]
        .into_iter()
        .collect(),
    );
    let out = call_capability(
        "io/net::http-request",
        &payload,
        policy.op_policy("io/net::http-request"),
        &policy,
        None,
        None,
        &mut budget,
        SealId(9),
    )
    .expect("call capability");
    let Value::Data(Term::Map(mm)) = out else {
        panic!("expected data map");
    };
    assert_eq!(
        mm.get(&TermOrdKey(Term::symbol(":status"))),
        Some(&Term::Int(200_i64.into()))
    );
}

#[test]
fn io_net_ws_open_policy_gate_enforces_remote_allowlist() {
    let policy = CapsPolicy::from_toml_str(
        r#"
allow = ["io/net::ws-open"]

[op."io/net::ws-open"]
url_allow = ["wss://realtime.example.com/ws/"]
wasi_network_profile = "preview2"
wasi_bridge_profile = true
wasi_bridge_response = "{:ok true :stream-id \"ws-1\"}"
"#,
    )
    .expect("caps");
    let mut budget = ArtifactBudgetState::default();
    let payload = Term::Map(
        [(
            TermOrdKey(Term::symbol(":url")),
            Term::Str("wss://evil.example.com/ws/room".to_string()),
        )]
        .into_iter()
        .collect(),
    );
    let out = call_capability(
        "io/net::ws-open",
        &payload,
        policy.op_policy("io/net::ws-open"),
        &policy,
        None,
        None,
        &mut budget,
        SealId(10),
    )
    .expect("call capability");
    assert_eq!(code_from_error(out), "core/caps/policy-error");
}

#[test]
fn io_net_ws_family_wasi_bridge_profile_returns_data() {
    let policy = CapsPolicy::from_toml_str(
        r#"
allow = ["io/net::ws-open", "io/net::ws-send", "io/net::ws-recv", "io/net::ws-close"]

[op."io/net::ws-open"]
url_allow = ["wss://realtime.example.com/ws/"]
wasi_network_profile = "preview2"
wasi_bridge_profile = true
wasi_bridge_response = "{:ok true :stream-id \"ws-1\"}"

[op."io/net::ws-send"]
wasi_bridge_profile = true
wasi_bridge_response = "{:ok true :sent-bytes 5}"

[op."io/net::ws-recv"]
wasi_bridge_profile = true
wasi_bridge_response = "{:ok true :data b\"hello\" :eof false}"

[op."io/net::ws-close"]
wasi_bridge_profile = true
wasi_bridge_response = "{:ok true :closed true}"
"#,
    )
    .expect("caps");
    let mut budget = ArtifactBudgetState::default();
    let open_payload = Term::Map(
        [(
            TermOrdKey(Term::symbol(":url")),
            Term::Str("wss://realtime.example.com/ws/room".to_string()),
        )]
        .into_iter()
        .collect(),
    );
    let open_out = call_capability(
        "io/net::ws-open",
        &open_payload,
        policy.op_policy("io/net::ws-open"),
        &policy,
        None,
        None,
        &mut budget,
        SealId(12),
    )
    .expect("call capability");
    let Value::Data(Term::Map(open_map)) = open_out else {
        panic!("expected ws-open data map");
    };
    assert_eq!(
        open_map.get(&TermOrdKey(Term::symbol(":stream-id"))),
        Some(&Term::Str("ws-1".to_string()))
    );

    let send_payload = Term::Map(
        [
            (
                TermOrdKey(Term::symbol(":stream-id")),
                Term::Str("ws-1".to_string()),
            ),
            (
                TermOrdKey(Term::symbol(":data")),
                Term::Bytes(b"hello".to_vec().into()),
            ),
        ]
        .into_iter()
        .collect(),
    );
    let send_out = call_capability(
        "io/net::ws-send",
        &send_payload,
        policy.op_policy("io/net::ws-send"),
        &policy,
        None,
        None,
        &mut budget,
        SealId(14),
    )
    .expect("call capability");
    let Value::Data(Term::Map(send_map)) = send_out else {
        panic!("expected ws-send data map");
    };
    assert_eq!(
        send_map.get(&TermOrdKey(Term::symbol(":sent-bytes"))),
        Some(&Term::Int(5_i64.into()))
    );

    let recv_payload = Term::Map(
        [(
            TermOrdKey(Term::symbol(":stream-id")),
            Term::Str("ws-1".to_string()),
        )]
        .into_iter()
        .collect(),
    );
    let recv_out = call_capability(
        "io/net::ws-recv",
        &recv_payload,
        policy.op_policy("io/net::ws-recv"),
        &policy,
        None,
        None,
        &mut budget,
        SealId(16),
    )
    .expect("call capability");
    let Value::Data(Term::Map(recv_map)) = recv_out else {
        panic!("expected ws-recv data map");
    };
    assert_eq!(
        recv_map.get(&TermOrdKey(Term::symbol(":eof"))),
        Some(&Term::Bool(false))
    );

    let close_payload = Term::Map(
        [(
            TermOrdKey(Term::symbol(":stream-id")),
            Term::Str("ws-1".to_string()),
        )]
        .into_iter()
        .collect(),
    );
    let close_out = call_capability(
        "io/net::ws-close",
        &close_payload,
        policy.op_policy("io/net::ws-close"),
        &policy,
        None,
        None,
        &mut budget,
        SealId(18),
    )
    .expect("call capability");
    let Value::Data(Term::Map(close_map)) = close_out else {
        panic!("expected ws-close data map");
    };
    assert_eq!(
        close_map.get(&TermOrdKey(Term::symbol(":closed"))),
        Some(&Term::Bool(true))
    );
}

#[test]
fn io_net_tcp_open_policy_gate_enforces_remote_allowlist() {
    let policy = CapsPolicy::from_toml_str(
        r#"
allow = ["io/net::tcp-open"]

[op."io/net::tcp-open"]
url_allow = ["tcp://allowed.example.com:443"]
wasi_network_profile = "preview2"
wasi_bridge_profile = true
wasi_bridge_response = "{:ok true :stream-id \"tcp-1\"}"
"#,
    )
    .expect("caps");
    let mut budget = ArtifactBudgetState::default();
    let payload = Term::Map(
        [(
            TermOrdKey(Term::symbol(":remote")),
            Term::Str("tcp://evil.example.com:443".to_string()),
        )]
        .into_iter()
        .collect(),
    );
    let out = call_capability(
        "io/net::tcp-open",
        &payload,
        policy.op_policy("io/net::tcp-open"),
        &policy,
        None,
        None,
        &mut budget,
        SealId(32),
    )
    .expect("call capability");
    assert_eq!(code_from_error(out), "core/caps/policy-error");
}

#[test]
fn io_net_tcp_family_wasi_bridge_profile_returns_data() {
    let policy = CapsPolicy::from_toml_str(
        r#"
allow = ["io/net::tcp-open", "io/net::tcp-send", "io/net::tcp-recv", "io/net::tcp-close"]

[op."io/net::tcp-open"]
url_allow = ["tcp://allowed.example.com:443"]
wasi_network_profile = "preview2"
wasi_bridge_profile = true
wasi_bridge_response = "{:ok true :stream-id \"tcp-1\"}"

[op."io/net::tcp-send"]
wasi_bridge_profile = true
wasi_bridge_response = "{:ok true :sent-bytes 4}"

[op."io/net::tcp-recv"]
wasi_bridge_profile = true
wasi_bridge_response = "{:ok true :data b\"pong\" :eof false}"

[op."io/net::tcp-close"]
wasi_bridge_profile = true
wasi_bridge_response = "{:ok true :closed true}"
"#,
    )
    .expect("caps");
    let mut budget = ArtifactBudgetState::default();
    let open_payload = Term::Map(
        [(
            TermOrdKey(Term::symbol(":remote")),
            Term::Str("tcp://allowed.example.com:443".to_string()),
        )]
        .into_iter()
        .collect(),
    );
    let open_out = call_capability(
        "io/net::tcp-open",
        &open_payload,
        policy.op_policy("io/net::tcp-open"),
        &policy,
        None,
        None,
        &mut budget,
        SealId(34),
    )
    .expect("call capability");
    let Value::Data(Term::Map(open_map)) = open_out else {
        panic!("expected tcp-open data map");
    };
    assert_eq!(
        open_map.get(&TermOrdKey(Term::symbol(":stream-id"))),
        Some(&Term::Str("tcp-1".to_string()))
    );
    let send_payload = Term::Map(
        [
            (
                TermOrdKey(Term::symbol(":stream-id")),
                Term::Str("tcp-1".to_string()),
            ),
            (
                TermOrdKey(Term::symbol(":data")),
                Term::Bytes(b"ping".to_vec().into()),
            ),
        ]
        .into_iter()
        .collect(),
    );
    let send_out = call_capability(
        "io/net::tcp-send",
        &send_payload,
        policy.op_policy("io/net::tcp-send"),
        &policy,
        None,
        None,
        &mut budget,
        SealId(36),
    )
    .expect("call capability");
    let Value::Data(Term::Map(send_map)) = send_out else {
        panic!("expected tcp-send data map");
    };
    assert_eq!(
        send_map.get(&TermOrdKey(Term::symbol(":sent-bytes"))),
        Some(&Term::Int(4_i64.into()))
    );
    let recv_payload = Term::Map(
        [(
            TermOrdKey(Term::symbol(":stream-id")),
            Term::Str("tcp-1".to_string()),
        )]
        .into_iter()
        .collect(),
    );
    let recv_out = call_capability(
        "io/net::tcp-recv",
        &recv_payload,
        policy.op_policy("io/net::tcp-recv"),
        &policy,
        None,
        None,
        &mut budget,
        SealId(38),
    )
    .expect("call capability");
    let Value::Data(Term::Map(recv_map)) = recv_out else {
        panic!("expected tcp-recv data map");
    };
    assert_eq!(
        recv_map.get(&TermOrdKey(Term::symbol(":eof"))),
        Some(&Term::Bool(false))
    );
    let close_payload = Term::Map(
        [(
            TermOrdKey(Term::symbol(":stream-id")),
            Term::Str("tcp-1".to_string()),
        )]
        .into_iter()
        .collect(),
    );
    let close_out = call_capability(
        "io/net::tcp-close",
        &close_payload,
        policy.op_policy("io/net::tcp-close"),
        &policy,
        None,
        None,
        &mut budget,
        SealId(40),
    )
    .expect("call capability");
    let Value::Data(Term::Map(close_map)) = close_out else {
        panic!("expected tcp-close data map");
    };
    assert_eq!(
        close_map.get(&TermOrdKey(Term::symbol(":closed"))),
        Some(&Term::Bool(true))
    );
}

#[test]
fn io_net_udp_bind_policy_gate_enforces_allowlist() {
    let policy = CapsPolicy::from_toml_str(
        r#"
allow = ["io/net::udp-bind"]

[op."io/net::udp-bind"]
url_allow = ["udp://127.0.0.1:5353"]
wasi_network_profile = "preview2"
wasi_bridge_profile = true
wasi_bridge_response = "{:ok true :socket-id \"udp-1\"}"
"#,
    )
    .expect("caps");
    let mut budget = ArtifactBudgetState::default();
    let payload = Term::Map(
        [(
            TermOrdKey(Term::symbol(":local")),
            Term::Str("udp://0.0.0.0:5353".to_string()),
        )]
        .into_iter()
        .collect(),
    );
    let out = call_capability(
        "io/net::udp-bind",
        &payload,
        policy.op_policy("io/net::udp-bind"),
        &policy,
        None,
        None,
        &mut budget,
        SealId(42),
    )
    .expect("call capability");
    assert_eq!(code_from_error(out), "core/caps/policy-error");
}

#[test]
fn io_net_udp_family_wasi_bridge_profile_returns_data() {
    let policy = CapsPolicy::from_toml_str(
        r#"
allow = ["io/net::udp-bind", "io/net::udp-send", "io/net::udp-recv", "io/net::udp-close"]

[op."io/net::udp-bind"]
url_allow = ["udp://127.0.0.1:5353"]
wasi_network_profile = "preview2"
wasi_bridge_profile = true
wasi_bridge_response = "{:ok true :socket-id \"udp-1\"}"

[op."io/net::udp-send"]
url_allow = ["udp://127.0.0.1:5354"]
wasi_network_profile = "preview2"
wasi_bridge_profile = true
wasi_bridge_response = "{:ok true :sent-bytes 3}"

[op."io/net::udp-recv"]
wasi_bridge_profile = true
wasi_bridge_response = "{:ok true :remote \"udp://127.0.0.1:5354\" :data b\"ack\"}"

[op."io/net::udp-close"]
wasi_bridge_profile = true
wasi_bridge_response = "{:ok true :closed true}"
"#,
    )
    .expect("caps");
    let mut budget = ArtifactBudgetState::default();
    let bind_payload = Term::Map(
        [(
            TermOrdKey(Term::symbol(":local")),
            Term::Str("udp://127.0.0.1:5353".to_string()),
        )]
        .into_iter()
        .collect(),
    );
    let bind_out = call_capability(
        "io/net::udp-bind",
        &bind_payload,
        policy.op_policy("io/net::udp-bind"),
        &policy,
        None,
        None,
        &mut budget,
        SealId(44),
    )
    .expect("call capability");
    let Value::Data(Term::Map(bind_map)) = bind_out else {
        panic!("expected udp-bind data map");
    };
    assert_eq!(
        bind_map.get(&TermOrdKey(Term::symbol(":socket-id"))),
        Some(&Term::Str("udp-1".to_string()))
    );
    let send_payload = Term::Map(
        [
            (
                TermOrdKey(Term::symbol(":socket-id")),
                Term::Str("udp-1".to_string()),
            ),
            (
                TermOrdKey(Term::symbol(":remote")),
                Term::Str("udp://127.0.0.1:5354".to_string()),
            ),
            (
                TermOrdKey(Term::symbol(":data")),
                Term::Bytes(b"msg".to_vec().into()),
            ),
        ]
        .into_iter()
        .collect(),
    );
    let send_out = call_capability(
        "io/net::udp-send",
        &send_payload,
        policy.op_policy("io/net::udp-send"),
        &policy,
        None,
        None,
        &mut budget,
        SealId(46),
    )
    .expect("call capability");
    let Value::Data(Term::Map(send_map)) = send_out else {
        panic!("expected udp-send data map");
    };
    assert_eq!(
        send_map.get(&TermOrdKey(Term::symbol(":sent-bytes"))),
        Some(&Term::Int(3_i64.into()))
    );
    let recv_payload = Term::Map(
        [(
            TermOrdKey(Term::symbol(":socket-id")),
            Term::Str("udp-1".to_string()),
        )]
        .into_iter()
        .collect(),
    );
    let recv_out = call_capability(
        "io/net::udp-recv",
        &recv_payload,
        policy.op_policy("io/net::udp-recv"),
        &policy,
        None,
        None,
        &mut budget,
        SealId(48),
    )
    .expect("call capability");
    let Value::Data(Term::Map(recv_map)) = recv_out else {
        panic!("expected udp-recv data map");
    };
    assert_eq!(
        recv_map.get(&TermOrdKey(Term::symbol(":remote"))),
        Some(&Term::Str("udp://127.0.0.1:5354".to_string()))
    );
    let close_payload = Term::Map(
        [(
            TermOrdKey(Term::symbol(":socket-id")),
            Term::Str("udp-1".to_string()),
        )]
        .into_iter()
        .collect(),
    );
    let close_out = call_capability(
        "io/net::udp-close",
        &close_payload,
        policy.op_policy("io/net::udp-close"),
        &policy,
        None,
        None,
        &mut budget,
        SealId(50),
    )
    .expect("call capability");
    let Value::Data(Term::Map(close_map)) = close_out else {
        panic!("expected udp-close data map");
    };
    assert_eq!(
        close_map.get(&TermOrdKey(Term::symbol(":closed"))),
        Some(&Term::Bool(true))
    );
}

#[test]
fn io_net_dns_resolve_policy_gate_enforces_allowlist() {
    let policy = CapsPolicy::from_toml_str(
        r#"
allow = ["io/net::dns-resolve"]

[op."io/net::dns-resolve"]
url_allow = ["dns://allowed.example.com"]
wasi_network_profile = "preview2"
wasi_bridge_profile = true
wasi_bridge_response = "{:ok true :records [{:type \"A\" :value \"127.0.0.1\"}]}"
"#,
    )
    .expect("caps");
    let mut budget = ArtifactBudgetState::default();
    let payload = Term::Map(
        [(
            TermOrdKey(Term::symbol(":name")),
            Term::Str("evil.example.com".to_string()),
        )]
        .into_iter()
        .collect(),
    );
    let out = call_capability(
        "io/net::dns-resolve",
        &payload,
        policy.op_policy("io/net::dns-resolve"),
        &policy,
        None,
        None,
        &mut budget,
        SealId(52),
    )
    .expect("call capability");
    assert_eq!(code_from_error(out), "core/caps/policy-error");
}

#[test]
fn io_net_dns_resolve_wasi_bridge_profile_returns_data() {
    let policy = CapsPolicy::from_toml_str(
        r#"
allow = ["io/net::dns-resolve"]

[op."io/net::dns-resolve"]
url_allow = ["dns://allowed.example.com"]
wasi_network_profile = "preview2"
wasi_bridge_profile = true
wasi_bridge_response = "{:ok true :records [{:type \"A\" :value \"127.0.0.1\"}]}"
"#,
    )
    .expect("caps");
    let mut budget = ArtifactBudgetState::default();
    let payload = Term::Map(
        [(
            TermOrdKey(Term::symbol(":name")),
            Term::Str("allowed.example.com".to_string()),
        )]
        .into_iter()
        .collect(),
    );
    let out = call_capability(
        "io/net::dns-resolve",
        &payload,
        policy.op_policy("io/net::dns-resolve"),
        &policy,
        None,
        None,
        &mut budget,
        SealId(54),
    )
    .expect("call capability");
    let Value::Data(Term::Map(mm)) = out else {
        panic!("expected dns-resolve data map");
    };
    assert!(mm.contains_key(&TermOrdKey(Term::symbol(":records"))));
}

#[test]
fn sys_process_exec_policy_gate_requires_allowlisted_program() {
    let policy = CapsPolicy::from_toml_str(
        r#"
allow = ["sys/process::exec"]

[op."sys/process::exec"]
allow_programs = ["gcpm"]
wasi_bridge_profile = true
wasi_bridge_response = "{:ok true :status 0}"
"#,
    )
    .expect("caps");
    let mut budget = ArtifactBudgetState::default();
    let payload = Term::Map(
        [(
            TermOrdKey(Term::symbol(":program")),
            Term::Str("bash".to_string()),
        )]
        .into_iter()
        .collect(),
    );
    let out = call_capability(
        "sys/process::exec",
        &payload,
        policy.op_policy("sys/process::exec"),
        &policy,
        None,
        None,
        &mut budget,
        SealId(11),
    )
    .expect("call capability");
    assert_eq!(code_from_error(out), "core/caps/policy-error");
}

#[test]
fn sys_process_exec_wasi_bridge_profile_returns_data() {
    let policy = CapsPolicy::from_toml_str(
        r#"
allow = ["sys/process::exec"]

[op."sys/process::exec"]
allow_programs = ["gcpm"]
wasi_bridge_profile = true
wasi_bridge_response = "{:ok true :status 0 :stdout \"ready\"}"
"#,
    )
    .expect("caps");
    let mut budget = ArtifactBudgetState::default();
    let payload = Term::Map(
        [(
            TermOrdKey(Term::symbol(":program")),
            Term::Str("gcpm".to_string()),
        )]
        .into_iter()
        .collect(),
    );
    let out = call_capability(
        "sys/process::exec",
        &payload,
        policy.op_policy("sys/process::exec"),
        &policy,
        None,
        None,
        &mut budget,
        SealId(13),
    )
    .expect("call capability");
    let Value::Data(Term::Map(mm)) = out else {
        panic!("expected data map");
    };
    assert_eq!(
        mm.get(&TermOrdKey(Term::symbol(":status"))),
        Some(&Term::Int(0_i64.into()))
    );
}

#[test]
fn io_fs_extended_ops_execute_with_deterministic_payload_contracts() {
    let temp = tempdir().expect("tempdir");
    let base_dir = temp.path().display().to_string().replace('\\', "/");
    let policy = CapsPolicy::from_toml_str(&format!(
        r#"
allow = ["io/fs::mkdir", "io/fs::write", "io/fs::stat", "io/fs::list", "io/fs::rename", "io/fs::remove"]

[op."io/fs::mkdir"]
base_dir = "{base_dir}"

[op."io/fs::write"]
base_dir = "{base_dir}"
create_dirs = true

[op."io/fs::stat"]
base_dir = "{base_dir}"

[op."io/fs::list"]
base_dir = "{base_dir}"

[op."io/fs::rename"]
base_dir = "{base_dir}"
create_dirs = true

[op."io/fs::remove"]
base_dir = "{base_dir}"
"#
    ))
    .expect("caps");

    let mut budget = ArtifactBudgetState::default();
    let mkdir_payload = term_map([
        (Term::symbol(":path"), Term::Str("sandbox/work".to_string())),
        (Term::symbol(":parents"), Term::Bool(true)),
    ]);
    let mkdir_out = call_capability(
        "io/fs::mkdir",
        &mkdir_payload,
        policy.op_policy("io/fs::mkdir"),
        &policy,
        None,
        None,
        &mut budget,
        SealId(70),
    )
    .expect("io/fs::mkdir");
    assert!(matches!(mkdir_out, Value::Data(Term::Nil)));

    let write_payload = term_map([
        (
            Term::symbol(":path"),
            Term::Str("sandbox/work/input.txt".to_string()),
        ),
        (Term::symbol(":data"), Term::Bytes(b"hello".to_vec().into())),
    ]);
    let write_out = call_capability(
        "io/fs::write",
        &write_payload,
        policy.op_policy("io/fs::write"),
        &policy,
        None,
        None,
        &mut budget,
        SealId(71),
    )
    .expect("io/fs::write");
    assert!(matches!(write_out, Value::Data(Term::Nil)));

    let stat_payload = term_map([(
        Term::symbol(":path"),
        Term::Str("sandbox/work/input.txt".to_string()),
    )]);
    let stat_out = call_capability(
        "io/fs::stat",
        &stat_payload,
        policy.op_policy("io/fs::stat"),
        &policy,
        None,
        None,
        &mut budget,
        SealId(72),
    )
    .expect("io/fs::stat");
    let Value::Data(Term::Map(stat_map)) = stat_out else {
        panic!("expected stat data map");
    };
    assert_eq!(
        stat_map.get(&TermOrdKey(Term::symbol(":exists"))),
        Some(&Term::Bool(true))
    );
    assert_eq!(
        stat_map.get(&TermOrdKey(Term::symbol(":kind"))),
        Some(&Term::Symbol("file".to_string()))
    );

    let list_payload = term_map([(Term::symbol(":path"), Term::Str("sandbox/work".to_string()))]);
    let list_out = call_capability(
        "io/fs::list",
        &list_payload,
        policy.op_policy("io/fs::list"),
        &policy,
        None,
        None,
        &mut budget,
        SealId(73),
    )
    .expect("io/fs::list");
    let Value::Data(Term::Vector(entries)) = list_out else {
        panic!("expected list vector");
    };
    assert!(entries.iter().any(|entry| {
        let Term::Map(mm) = entry else {
            return false;
        };
        mm.get(&TermOrdKey(Term::symbol(":name"))) == Some(&Term::Str("input.txt".to_string()))
    }));

    let rename_payload = term_map([
        (
            Term::symbol(":from"),
            Term::Str("sandbox/work/input.txt".to_string()),
        ),
        (
            Term::symbol(":to"),
            Term::Str("sandbox/work/output.txt".to_string()),
        ),
    ]);
    let rename_out = call_capability(
        "io/fs::rename",
        &rename_payload,
        policy.op_policy("io/fs::rename"),
        &policy,
        None,
        None,
        &mut budget,
        SealId(74),
    )
    .expect("io/fs::rename");
    assert!(matches!(rename_out, Value::Data(Term::Nil)));

    let remove_payload = term_map([(
        Term::symbol(":path"),
        Term::Str("sandbox/work/output.txt".to_string()),
    )]);
    let remove_out = call_capability(
        "io/fs::remove",
        &remove_payload,
        policy.op_policy("io/fs::remove"),
        &policy,
        None,
        None,
        &mut budget,
        SealId(75),
    )
    .expect("io/fs::remove");
    assert!(matches!(remove_out, Value::Data(Term::Nil)));

    let stat_missing_out = call_capability(
        "io/fs::stat",
        &term_map([(
            Term::symbol(":path"),
            Term::Str("sandbox/work/output.txt".to_string()),
        )]),
        policy.op_policy("io/fs::stat"),
        &policy,
        None,
        None,
        &mut budget,
        SealId(76),
    )
    .expect("io/fs::stat missing");
    let Value::Data(Term::Map(missing_map)) = stat_missing_out else {
        panic!("expected stat data map");
    };
    assert_eq!(
        missing_map.get(&TermOrdKey(Term::symbol(":exists"))),
        Some(&Term::Bool(false))
    );
}

#[test]
fn io_fs_mutating_ops_reject_timeout_policy() {
    let temp = tempdir().expect("tempdir");
    let base_dir = temp.path().display().to_string().replace('\\', "/");
    let policy = CapsPolicy::from_toml_str(&format!(
        r#"
allow = ["io/fs::mkdir"]

[op."io/fs::mkdir"]
base_dir = "{base_dir}"
timeout_ms = 5
"#
    ))
    .expect("caps");
    let mut budget = ArtifactBudgetState::default();
    let out = call_capability(
        "io/fs::mkdir",
        &term_map([(Term::symbol(":path"), Term::Str("sandbox/work".to_string()))]),
        policy.op_policy("io/fs::mkdir"),
        &policy,
        None,
        None,
        &mut budget,
        SealId(77),
    )
    .expect("call capability");
    assert_eq!(code_from_error(out), "core/caps/policy-error");
}

#[test]
fn sys_process_spawn_and_stream_ops_use_bridge_contracts() {
    let policy = CapsPolicy::from_toml_str(
        r#"
allow = [
  "sys/process::spawn",
  "sys/process::wait",
  "sys/process::kill",
  "sys/process::stdin-write",
  "sys/process::stdout-read",
  "sys/process::stderr-read"
]

[op."sys/process::spawn"]
allow_programs = ["gcpm"]
wasi_bridge_profile = true
wasi_bridge_response = "{:ok true :process-id \"proc-1\"}"

[op."sys/process::wait"]
wasi_bridge_profile = true
wasi_bridge_response = "{:ok true :status 0}"

[op."sys/process::kill"]
wasi_bridge_profile = true
wasi_bridge_response = "{:ok true :killed true}"

[op."sys/process::stdin-write"]
wasi_bridge_profile = true
wasi_bridge_response = "{:ok true :written-bytes 4}"

[op."sys/process::stdout-read"]
wasi_bridge_profile = true
wasi_bridge_response = "{:ok true :data b\"done\" :eof true}"

[op."sys/process::stderr-read"]
wasi_bridge_profile = true
wasi_bridge_response = "{:ok true :data b\"\" :eof true}"
"#,
    )
    .expect("caps");
    let mut budget = ArtifactBudgetState::default();

    let spawn_out = call_capability(
        "sys/process::spawn",
        &term_map([(Term::symbol(":program"), Term::Str("gcpm".to_string()))]),
        policy.op_policy("sys/process::spawn"),
        &policy,
        None,
        None,
        &mut budget,
        SealId(78),
    )
    .expect("spawn");
    let Value::Data(Term::Map(spawn_map)) = spawn_out else {
        panic!("expected spawn map");
    };
    let Some(Term::Str(process_id)) = spawn_map.get(&TermOrdKey(Term::symbol(":process-id")))
    else {
        panic!("missing process-id");
    };
    assert_eq!(process_id, "proc-1");

    let wait_payload = term_map([(Term::symbol(":process-id"), Term::Str(process_id.clone()))]);
    let wait_out = call_capability(
        "sys/process::wait",
        &wait_payload,
        policy.op_policy("sys/process::wait"),
        &policy,
        None,
        None,
        &mut budget,
        SealId(79),
    )
    .expect("wait");
    let Value::Data(Term::Map(wait_map)) = wait_out else {
        panic!("expected wait map");
    };
    assert_eq!(
        wait_map.get(&TermOrdKey(Term::symbol(":status"))),
        Some(&Term::Int(0_i64.into()))
    );

    let write_out = call_capability(
        "sys/process::stdin-write",
        &term_map([
            (Term::symbol(":process-id"), Term::Str(process_id.clone())),
            (Term::symbol(":data"), Term::Bytes(b"ping".to_vec().into())),
        ]),
        policy.op_policy("sys/process::stdin-write"),
        &policy,
        None,
        None,
        &mut budget,
        SealId(80),
    )
    .expect("stdin-write");
    let Value::Data(Term::Map(write_map)) = write_out else {
        panic!("expected stdin-write map");
    };
    assert_eq!(
        write_map.get(&TermOrdKey(Term::symbol(":written-bytes"))),
        Some(&Term::Int(4_i64.into()))
    );

    let stdout_out = call_capability(
        "sys/process::stdout-read",
        &wait_payload,
        policy.op_policy("sys/process::stdout-read"),
        &policy,
        None,
        None,
        &mut budget,
        SealId(81),
    )
    .expect("stdout-read");
    let Value::Data(Term::Map(stdout_map)) = stdout_out else {
        panic!("expected stdout-read map");
    };
    assert_eq!(
        stdout_map.get(&TermOrdKey(Term::symbol(":eof"))),
        Some(&Term::Bool(true))
    );

    let stderr_out = call_capability(
        "sys/process::stderr-read",
        &wait_payload,
        policy.op_policy("sys/process::stderr-read"),
        &policy,
        None,
        None,
        &mut budget,
        SealId(82),
    )
    .expect("stderr-read");
    let Value::Data(Term::Map(stderr_map)) = stderr_out else {
        panic!("expected stderr-read map");
    };
    assert_eq!(
        stderr_map.get(&TermOrdKey(Term::symbol(":eof"))),
        Some(&Term::Bool(true))
    );

    let kill_out = call_capability(
        "sys/process::kill",
        &wait_payload,
        policy.op_policy("sys/process::kill"),
        &policy,
        None,
        None,
        &mut budget,
        SealId(83),
    )
    .expect("kill");
    let Value::Data(Term::Map(kill_map)) = kill_out else {
        panic!("expected kill map");
    };
    assert_eq!(
        kill_map.get(&TermOrdKey(Term::symbol(":killed"))),
        Some(&Term::Bool(true))
    );
}

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
