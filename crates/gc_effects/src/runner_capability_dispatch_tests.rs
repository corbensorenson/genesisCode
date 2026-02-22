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
fn io_net_tcp_listen_policy_gate_enforces_bind_host_port_allowlists() {
    let policy = CapsPolicy::from_toml_str(
        r#"
allow = ["io/net::tcp-listen"]

[op."io/net::tcp-listen"]
url_allow = ["tcp://127.0.0.1:9000"]
allow_bind_hosts = ["127.0.0.1"]
allow_bind_ports = [9000]
wasi_network_profile = "preview2"
wasi_bridge_profile = true
wasi_bridge_response = "{:ok true :listener-id \"tcp-listener-1\"}"
"#,
    )
    .expect("caps");
    let mut budget = ArtifactBudgetState::default();
    let payload = term_map([(
        Term::symbol(":local"),
        Term::Str("tcp://0.0.0.0:9000".to_string()),
    )]);
    let out = call_capability(
        "io/net::tcp-listen",
        &payload,
        policy.op_policy("io/net::tcp-listen"),
        &policy,
        None,
        None,
        &mut budget,
        SealId(56),
    )
    .expect("call capability");
    assert_eq!(code_from_error(out), "core/caps/policy-error");
}

#[test]
fn io_net_tcp_accept_policy_requires_max_request_bytes() {
    let policy = CapsPolicy::from_toml_str(
        r#"
allow = ["io/net::tcp-accept"]

[op."io/net::tcp-accept"]
wasi_bridge_profile = true
wasi_bridge_response = "{:ok true :request-id \"req-1\" :data b\"ping\"}"
"#,
    )
    .expect("caps");
    let mut budget = ArtifactBudgetState::default();
    let payload = term_map([(
        Term::symbol(":listener-id"),
        Term::Str("tcp-listener-1".to_string()),
    )]);
    let out = call_capability(
        "io/net::tcp-accept",
        &payload,
        policy.op_policy("io/net::tcp-accept"),
        &policy,
        None,
        None,
        &mut budget,
        SealId(57),
    )
    .expect("call capability");
    assert_eq!(code_from_error(out), "core/caps/policy-error");
}

#[test]
fn io_net_http_listen_and_ws_accept_wasi_bridge_profile_returns_data() {
    let policy = CapsPolicy::from_toml_str(
        r#"
allow = ["io/net::http-listen", "io/net::ws-accept"]

[op."io/net::http-listen"]
url_allow = ["http://127.0.0.1:8080"]
allow_http = true
allow_bind_hosts = ["127.0.0.1"]
allow_bind_ports = [8080]
max_request_bytes = 8192
wasi_network_profile = "preview2"
wasi_bridge_profile = true
wasi_bridge_response = "{:ok true :listener-id \"http-listener-1\"}"

[op."io/net::ws-accept"]
max_request_bytes = 4096
wasi_bridge_profile = true
wasi_bridge_response = "{:ok true :stream-id \"ws-accepted-1\"}"
"#,
    )
    .expect("caps");
    let mut budget = ArtifactBudgetState::default();
    let listen_out = call_capability(
        "io/net::http-listen",
        &term_map([(
            Term::symbol(":local"),
            Term::Str("http://127.0.0.1:8080".to_string()),
        )]),
        policy.op_policy("io/net::http-listen"),
        &policy,
        None,
        None,
        &mut budget,
        SealId(58),
    )
    .expect("http-listen");
    let Value::Data(Term::Map(listen_map)) = listen_out else {
        panic!("expected http-listen data map");
    };
    assert_eq!(
        listen_map.get(&TermOrdKey(Term::symbol(":listener-id"))),
        Some(&Term::Str("http-listener-1".to_string()))
    );

    let ws_accept_out = call_capability(
        "io/net::ws-accept",
        &term_map([
            (
                Term::symbol(":listener-id"),
                Term::Str("http-listener-1".to_string()),
            ),
            (Term::symbol(":request-id"), Term::Str("req-1".to_string())),
        ]),
        policy.op_policy("io/net::ws-accept"),
        &policy,
        None,
        None,
        &mut budget,
        SealId(59),
    )
    .expect("ws-accept");
    let Value::Data(Term::Map(ws_accept_map)) = ws_accept_out else {
        panic!("expected ws-accept data map");
    };
    assert_eq!(
        ws_accept_map.get(&TermOrdKey(Term::symbol(":stream-id"))),
        Some(&Term::Str("ws-accepted-1".to_string()))
    );
}

#[test]
fn io_net_http_respond_requires_integer_status() {
    let policy = CapsPolicy::from_toml_str(
        r#"
allow = ["io/net::http-respond"]

[op."io/net::http-respond"]
wasi_bridge_profile = true
wasi_bridge_response = "{:ok true :sent true}"
"#,
    )
    .expect("caps");
    let mut budget = ArtifactBudgetState::default();
    let bad_payload = term_map([
        (
            Term::symbol(":listener-id"),
            Term::Str("http-listener-1".to_string()),
        ),
        (Term::symbol(":request-id"), Term::Str("req-1".to_string())),
        (Term::symbol(":status"), Term::Str("200".to_string())),
    ]);
    let err = call_capability(
        "io/net::http-respond",
        &bad_payload,
        policy.op_policy("io/net::http-respond"),
        &policy,
        None,
        None,
        &mut budget,
        SealId(60),
    )
    .expect_err("bad payload should fail");
    assert!(
        err.to_string().contains(":status"),
        "expected :status payload error, got {err}"
    );

    let good_out = call_capability(
        "io/net::http-respond",
        &term_map([
            (
                Term::symbol(":listener-id"),
                Term::Str("http-listener-1".to_string()),
            ),
            (Term::symbol(":request-id"), Term::Str("req-1".to_string())),
            (Term::symbol(":status"), Term::Int(200_i64.into())),
            (Term::symbol(":body"), Term::Bytes(b"ok".to_vec().into())),
        ]),
        policy.op_policy("io/net::http-respond"),
        &policy,
        None,
        None,
        &mut budget,
        SealId(61),
    )
    .expect("http-respond");
    let Value::Data(Term::Map(mm)) = good_out else {
        panic!("expected http-respond data map");
    };
    assert_eq!(
        mm.get(&TermOrdKey(Term::symbol(":sent"))),
        Some(&Term::Bool(true))
    );
}

#[path = "runner_capability_dispatch_tests/extended.rs"]
mod extended;
