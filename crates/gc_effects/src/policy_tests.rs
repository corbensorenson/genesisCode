use super::{
    AuthorizedBindPorts, AuthorizedBridgeAllowlist, AuthorizedBridgeDigest,
    AuthorizedBridgeIdentityPolicy, AuthorizedBridgeTransport, AuthorizedDatabasePolicy,
    AuthorizedFfiSignedPolicy, AuthorizedGpuBackend, AuthorizedGpuFallback, AuthorizedGpuPolicy,
    AuthorizedMaxBytes, AuthorizedNetworkPolicy, AuthorizedOptionalBool, AuthorizedOptionalString,
    AuthorizedProcessPrograms, AuthorizedStoreRemotePolicy, AuthorizedStringList, CapsPolicy,
};
use gc_coreform::{Term, TermOrdKey};
use gc_prelude::SelfhostBootstrapMode;
use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

fn selfhost_artifact() -> PathBuf {
    std::env::var_os("GENESIS_TEST_SELFHOST_ARTIFACT")
        .map(PathBuf::from)
        .unwrap_or_else(|| {
            Path::new(env!("CARGO_MANIFEST_DIR"))
                .join("../..")
                .join("selfhost/toolchain.gc")
        })
}

fn expected_cap(op: &str, fields: impl IntoIterator<Item = (&'static str, Term)>) -> Term {
    let mut cap = BTreeMap::from([(TermOrdKey(Term::symbol(":op")), Term::symbol(op))]);
    cap.extend(
        fields
            .into_iter()
            .map(|(key, value)| (TermOrdKey(Term::symbol(key)), value)),
    );
    Term::Map(cap)
}

fn max_bytes_policy(status: &str, limit: Term) -> Term {
    Term::Map(BTreeMap::from([
        (TermOrdKey(Term::symbol(":limit")), limit),
        (TermOrdKey(Term::symbol(":status")), Term::symbol(status)),
    ]))
}

fn process_program_policy(status: &str, programs: Term) -> Term {
    Term::Map(BTreeMap::from([
        (TermOrdKey(Term::symbol(":programs")), programs),
        (TermOrdKey(Term::symbol(":status")), Term::symbol(status)),
    ]))
}

fn string_list_policy(status: &str, values: Term) -> Term {
    Term::Map(BTreeMap::from([
        (TermOrdKey(Term::symbol(":status")), Term::symbol(status)),
        (TermOrdKey(Term::symbol(":values")), values),
    ]))
}

fn database_policy_term(
    target_allow: Term,
    query_classes: Term,
    max_result_bytes: Term,
    max_row_count: Term,
    max_value_bytes: Term,
) -> Term {
    Term::Map(BTreeMap::from([
        (
            TermOrdKey(Term::symbol(":max-result-bytes")),
            max_result_bytes,
        ),
        (TermOrdKey(Term::symbol(":max-row-count")), max_row_count),
        (
            TermOrdKey(Term::symbol(":max-value-bytes")),
            max_value_bytes,
        ),
        (TermOrdKey(Term::symbol(":query-classes")), query_classes),
        (TermOrdKey(Term::symbol(":target-allow")), target_allow),
    ]))
}

fn optional_value_policy(status: &str, value: Term) -> Term {
    Term::Map(BTreeMap::from([
        (TermOrdKey(Term::symbol(":status")), Term::symbol(status)),
        (TermOrdKey(Term::symbol(":value")), value),
    ]))
}

fn bridge_identity_policy_term(digest: Term, pin_required: Term) -> Term {
    Term::Map(BTreeMap::from([
        (TermOrdKey(Term::symbol(":active")), Term::Bool(false)),
        (
            TermOrdKey(Term::symbol(":allowlist")),
            string_list_policy(":absent", Term::Nil),
        ),
        (TermOrdKey(Term::symbol(":args")), Term::Vector(Vec::new())),
        (TermOrdKey(Term::symbol(":command")), Term::Nil),
        (TermOrdKey(Term::symbol(":digest")), digest),
        (TermOrdKey(Term::symbol(":pin-required")), pin_required),
        (
            TermOrdKey(Term::symbol(":transport")),
            optional_value_policy(":spawn-per-op", Term::Nil),
        ),
        (TermOrdKey(Term::symbol(":wasi-profile")), Term::Bool(false)),
    ]))
}

fn gpu_policy_term(backend: &str, fallback: &str) -> Term {
    Term::Map(BTreeMap::from([
        (TermOrdKey(Term::symbol(":backend")), Term::symbol(backend)),
        (
            TermOrdKey(Term::symbol(":fallback")),
            Term::symbol(fallback),
        ),
    ]))
}

fn store_remote_policy_term(remote: Term, remote_allow: Term, allow_http: Term) -> Term {
    Term::Map(BTreeMap::from([
        (TermOrdKey(Term::symbol(":allow-http")), allow_http),
        (TermOrdKey(Term::symbol(":remote")), remote),
        (TermOrdKey(Term::symbol(":remote-allow")), remote_allow),
    ]))
}

fn bind_ports_policy(status: &str, any: Term, ports: Term) -> Term {
    Term::Map(BTreeMap::from([
        (TermOrdKey(Term::symbol(":any")), any),
        (TermOrdKey(Term::symbol(":ports")), ports),
        (TermOrdKey(Term::symbol(":status")), Term::symbol(status)),
    ]))
}

fn network_policy_term(
    url_allow: Term,
    remote_allow: Term,
    allow_http: Term,
    wasi_profile: Term,
    bind_hosts: Term,
    bind_ports: Term,
    max_request_bytes: Term,
) -> Term {
    Term::Map(BTreeMap::from([
        (TermOrdKey(Term::symbol(":allow-http")), allow_http),
        (TermOrdKey(Term::symbol(":bind-hosts")), bind_hosts),
        (TermOrdKey(Term::symbol(":bind-ports")), bind_ports),
        (
            TermOrdKey(Term::symbol(":max-request-bytes")),
            max_request_bytes,
        ),
        (TermOrdKey(Term::symbol(":remote-allow")), remote_allow),
        (TermOrdKey(Term::symbol(":url-allow")), url_allow),
        (
            TermOrdKey(Term::symbol(":wasi-network-profile")),
            wasi_profile,
        ),
    ]))
}

fn crypto_policy_term(
    algorithms: Term,
    key_ids: Term,
    limits: impl IntoIterator<Item = (&'static str, Term)>,
) -> Term {
    let absent = max_bytes_policy(":absent", Term::Nil);
    let mut fields = BTreeMap::from([
        (TermOrdKey(Term::symbol(":algorithms")), algorithms),
        (TermOrdKey(Term::symbol(":key-ids")), key_ids),
    ]);
    for key in [
        ":max-aad-bytes",
        ":max-ciphertext-bytes",
        ":max-context-bytes",
        ":max-info-bytes",
        ":max-input-bytes",
        ":max-message-bytes",
        ":max-nonce-bytes",
        ":max-output-bytes",
        ":max-plaintext-bytes",
        ":max-salt-bytes",
        ":max-signature-bytes",
        ":max-tag-bytes",
    ] {
        fields.insert(TermOrdKey(Term::symbol(key)), absent.clone());
    }
    for (key, value) in limits {
        fields.insert(TermOrdKey(Term::symbol(key)), value);
    }
    Term::Map(fields)
}

fn plugin_policy_term(plugins: Term, commands: Term, schema_ids: Term) -> Term {
    Term::Map(BTreeMap::from([
        (TermOrdKey(Term::symbol(":commands")), commands),
        (TermOrdKey(Term::symbol(":plugins")), plugins),
        (TermOrdKey(Term::symbol(":schema-ids")), schema_ids),
    ]))
}

fn ffi_policy_term(
    abi_ids: Term,
    libraries: Term,
    symbols: Term,
    schema_ids: Term,
    max_buffer_bytes: Term,
    max_call_payload_bytes: Term,
) -> Term {
    Term::Map(BTreeMap::from([
        (TermOrdKey(Term::symbol(":abi-ids")), abi_ids),
        (TermOrdKey(Term::symbol(":libraries")), libraries),
        (
            TermOrdKey(Term::symbol(":max-buffer-bytes")),
            max_buffer_bytes,
        ),
        (
            TermOrdKey(Term::symbol(":max-call-payload-bytes")),
            max_call_payload_bytes,
        ),
        (TermOrdKey(Term::symbol(":schema-ids")), schema_ids),
        (
            TermOrdKey(Term::symbol(":signed-policy")),
            ffi_signed_policy_term(":disabled", Term::Nil, Term::Nil, Term::Nil, Term::Nil),
        ),
        (TermOrdKey(Term::symbol(":symbols")), symbols),
    ]))
}

fn ffi_signed_policy_term(
    status: &str,
    artifact: Term,
    signature: Term,
    key_id: Term,
    evidence_mode: Term,
) -> Term {
    Term::Map(BTreeMap::from([
        (TermOrdKey(Term::symbol(":evidence-mode")), evidence_mode),
        (TermOrdKey(Term::symbol(":policy-artifact-h")), artifact),
        (TermOrdKey(Term::symbol(":policy-key-id")), key_id),
        (TermOrdKey(Term::symbol(":policy-signature-h")), signature),
        (TermOrdKey(Term::symbol(":status")), Term::symbol(status)),
    ]))
}

#[test]
fn selfhost_authority_composes_admission_and_canonical_caps() {
    let td = tempfile::tempdir().unwrap();
    let caps = td.path().join("caps.toml");
    std::fs::write(
        &caps,
        r#"
allow = ["sys/time::now", "io/fs::read"]

[op."io/fs::read"]
allow = false

[op."core/task::await"]
allow = true
create_dirs = true
timeout_ms = 25
log_inline_max_bytes = 64
"#,
    )
    .unwrap();

    let artifact = selfhost_artifact();
    let policy = CapsPolicy::load_with_selfhost_authority(
        &caps,
        SelfhostBootstrapMode::ArtifactOnly,
        Some(&artifact),
    )
    .unwrap();

    assert!(policy.is_allowed("sys/time::now"));
    assert!(!policy.is_allowed("io/fs::read"));
    assert!(policy.is_allowed("core/task::await"));
    assert!(!policy.is_allowed("sys/env::get"));
    assert_eq!(
        policy.authorized_cap("sys/time::now"),
        Some(&expected_cap("sys/time::now", []))
    );
    assert_eq!(
        policy.authorized_cap("core/task::await"),
        Some(&expected_cap(
            "core/task::await",
            [
                (":create-dirs", Term::Bool(true)),
                (":timeout-ms", Term::Int(25.into())),
                (":log-inline-max-bytes", Term::Int(64.into())),
            ]
        ))
    );
    let task_policy = policy.op_policy("core/task::await").unwrap();
    assert!(task_policy.create_dirs);
    assert_eq!(task_policy.timeout_ms, Some(25));
    assert_eq!(task_policy.log_inline_max_bytes, Some(64));
    assert!(policy.authorized_cap("io/fs::read").is_none());
}

#[test]
fn selfhost_authority_installs_normalized_operation_controls() {
    let td = tempfile::tempdir().unwrap();
    let caps = td.path().join("caps.toml");
    std::fs::write(
        &caps,
        r#"
[op."io/fs::write"]
create_dirs = false
timeout_ms = -7
log_inline_max_bytes = 0
"#,
    )
    .unwrap();

    let artifact = selfhost_artifact();
    let policy = CapsPolicy::load_with_selfhost_authority(
        &caps,
        SelfhostBootstrapMode::ArtifactOnly,
        Some(&artifact),
    )
    .unwrap();
    let op_policy = policy.op_policy("io/fs::write").unwrap();

    assert!(!op_policy.create_dirs);
    assert_eq!(op_policy.timeout_ms, Some(0));
    assert_eq!(op_policy.log_inline_max_bytes, None);
    assert_eq!(
        policy.authorized_cap("io/fs::write"),
        Some(&expected_cap(
            "io/fs::write",
            [(":timeout-ms", Term::Int(0.into()))]
        ))
    );
}

#[test]
fn selfhost_authority_installs_valid_and_absent_max_byte_controls() {
    let td = tempfile::tempdir().unwrap();
    let caps = td.path().join("caps.toml");
    std::fs::write(
        &caps,
        r#"
[op."io/fs::read"]
max_bytes = 7

[op."io/fs::write"]
allow = true
"#,
    )
    .unwrap();

    let artifact = selfhost_artifact();
    let policy = CapsPolicy::load_with_selfhost_authority(
        &caps,
        SelfhostBootstrapMode::ArtifactOnly,
        Some(&artifact),
    )
    .unwrap();
    let read = policy.op_policy("io/fs::read").unwrap();
    let write = policy.op_policy("io/fs::write").unwrap();

    assert_eq!(
        read.authorized_max_bytes,
        Some(AuthorizedMaxBytes::Valid(7))
    );
    assert_eq!(write.authorized_max_bytes, Some(AuthorizedMaxBytes::Absent));
    assert_eq!(
        crate::runner::runner_response_budget::op_extra_positive_usize(Some(read), "max_bytes"),
        Ok(Some(7))
    );
    assert_eq!(
        crate::runner::runner_response_budget::op_extra_positive_usize(Some(write), "max_bytes"),
        Ok(None)
    );
    assert_eq!(
        crate::runner_host_bridge::runner_host_bridge_policy::bridge_max_bytes(Some(read), "test")
            .unwrap(),
        Some(7)
    );
    assert_eq!(
        crate::runner_host_bridge::runner_host_bridge_policy::bridge_max_bytes(Some(write), "test")
            .unwrap(),
        None
    );
    assert_eq!(
        policy.authorized_cap("io/fs::read"),
        Some(&expected_cap("io/fs::read", []))
    );
}

#[test]
fn selfhost_authority_preserves_invalid_max_byte_effect_errors() {
    let cases = [
        (
            "max_bytes = \"bad\"",
            AuthorizedMaxBytes::InvalidType,
            "max_bytes must be a positive integer".to_string(),
        ),
        (
            "max_bytes = 0",
            AuthorizedMaxBytes::NonPositive,
            "max_bytes must be > 0".to_string(),
        ),
    ];

    for (setting, expected_state, expected_error) in cases {
        let td = tempfile::tempdir().unwrap();
        let caps = td.path().join("caps.toml");
        std::fs::write(&caps, format!("[op.\"io/fs::read\"]\n{setting}\n")).unwrap();

        let artifact = selfhost_artifact();
        let policy = CapsPolicy::load_with_selfhost_authority(
            &caps,
            SelfhostBootstrapMode::ArtifactOnly,
            Some(&artifact),
        )
        .unwrap();
        let op_policy = policy.op_policy("io/fs::read").unwrap();

        assert_eq!(
            op_policy.authorized_max_bytes,
            Some(expected_state),
            "setting: {setting}"
        );
        assert_eq!(
            crate::runner::runner_response_budget::op_extra_positive_usize(
                Some(op_policy),
                "max_bytes"
            ),
            Err(expected_error.clone()),
            "setting: {setting}"
        );
        let bridge_error = crate::runner_host_bridge::runner_host_bridge_policy::bridge_max_bytes(
            Some(op_policy),
            "test",
        )
        .expect_err("bridge enforcement must consume the authorized invalid state");
        assert_eq!(
            bridge_error.code, "test/bridge-policy",
            "setting: {setting}"
        );
        assert_eq!(bridge_error.message, expected_error, "setting: {setting}");
    }
}

#[test]
fn selfhost_authority_rejects_malformed_max_byte_decisions() {
    use super::policy_authority::decode_max_bytes_policy;

    assert_eq!(
        decode_max_bytes_policy(&max_bytes_policy(":platform-overflow", Term::Nil), true).unwrap(),
        AuthorizedMaxBytes::PlatformOverflow
    );
    let platform_overflow = Term::Int(((usize::MAX as u128) + 1).to_string().parse().unwrap());
    let cases = [
        (max_bytes_policy(":unknown", Term::Nil), true),
        (max_bytes_policy(":valid", Term::Nil), true),
        (max_bytes_policy(":valid", Term::Int(0.into())), true),
        (max_bytes_policy(":valid", platform_overflow), true),
        (max_bytes_policy(":nonpositive", Term::Int(1.into())), true),
        (max_bytes_policy(":valid", Term::Int(1.into())), false),
    ];
    for (decision, allowed) in cases {
        decode_max_bytes_policy(&decision, allowed)
            .expect_err("contradictory max-byte authority decision must fail closed");
    }

    let mut extra = BTreeMap::from([
        (TermOrdKey(Term::symbol(":limit")), Term::Nil),
        (TermOrdKey(Term::symbol(":status")), Term::symbol(":absent")),
    ]);
    extra.insert(TermOrdKey(Term::symbol(":unknown")), Term::Nil);
    decode_max_bytes_policy(&Term::Map(extra), true)
        .expect_err("unknown max-byte authority fields must fail closed");
}

#[test]
fn selfhost_authority_installs_process_program_policy() {
    let td = tempfile::tempdir().unwrap();
    let caps = td.path().join("caps.toml");
    std::fs::write(
        &caps,
        r#"
[op."sys/process::exec"]
allow_programs = ["  gcpm  ", "", "tool-*"]

[op."sys/process::spawn"]
allow = true
"#,
    )
    .unwrap();

    let artifact = selfhost_artifact();
    let policy = CapsPolicy::load_with_selfhost_authority(
        &caps,
        SelfhostBootstrapMode::ArtifactOnly,
        Some(&artifact),
    )
    .unwrap();

    assert_eq!(
        policy
            .op_policy("sys/process::exec")
            .unwrap()
            .authorized_process_programs,
        Some(AuthorizedProcessPrograms::Valid(vec![
            "gcpm".to_string(),
            "tool-*".to_string(),
        ]))
    );
    assert_eq!(
        policy
            .op_policy("sys/process::spawn")
            .unwrap()
            .authorized_process_programs,
        Some(AuthorizedProcessPrograms::Absent)
    );
}

#[test]
fn selfhost_authority_preserves_invalid_process_program_states() {
    let cases = [
        (
            "allow_programs = \"gcpm\"",
            AuthorizedProcessPrograms::InvalidType,
        ),
        (
            "allow_programs = [\"gcpm\", 7]",
            AuthorizedProcessPrograms::InvalidEntry,
        ),
        (
            "allow_programs = [\"\", \"   \"]",
            AuthorizedProcessPrograms::Empty,
        ),
    ];
    for (setting, expected) in cases {
        let td = tempfile::tempdir().unwrap();
        let caps = td.path().join("caps.toml");
        std::fs::write(&caps, format!("[op.\"sys/process::exec\"]\n{setting}\n")).unwrap();
        let artifact = selfhost_artifact();
        let policy = CapsPolicy::load_with_selfhost_authority(
            &caps,
            SelfhostBootstrapMode::ArtifactOnly,
            Some(&artifact),
        )
        .unwrap();
        assert_eq!(
            policy
                .op_policy("sys/process::exec")
                .unwrap()
                .authorized_process_programs,
            Some(expected),
            "setting: {setting}"
        );
    }
}

#[test]
fn selfhost_authority_rejects_malformed_process_program_decisions() {
    use super::policy_authority::decode_process_program_policy;

    let cases = [
        (process_program_policy(":unknown", Term::Nil), true),
        (process_program_policy(":valid", Term::Nil), true),
        (process_program_policy(":valid", Term::Vector(vec![])), true),
        (
            process_program_policy(
                ":valid",
                Term::Vector(vec![Term::Str(" padded ".to_string())]),
            ),
            true,
        ),
        (
            process_program_policy(":valid", Term::Vector(vec![Term::Str("gcpm".to_string())])),
            false,
        ),
        (process_program_policy(":empty", Term::Vector(vec![])), true),
    ];
    for (decision, allowed) in cases {
        decode_process_program_policy(&decision, allowed)
            .expect_err("contradictory process-program authority decision must fail closed");
    }

    let mut extra = BTreeMap::from([
        (TermOrdKey(Term::symbol(":programs")), Term::Nil),
        (TermOrdKey(Term::symbol(":status")), Term::symbol(":absent")),
    ]);
    extra.insert(TermOrdKey(Term::symbol(":unknown")), Term::Nil);
    decode_process_program_policy(&Term::Map(extra), true)
        .expect_err("unknown process-program authority fields must fail closed");
}

#[test]
fn selfhost_authority_installs_database_policy() {
    let td = tempfile::tempdir().unwrap();
    let caps = td.path().join("caps.toml");
    std::fs::write(
        &caps,
        r#"
[op."io/db::query"]
allow_query_classes = ["  read-only  ", "", "analytics"]
max_result_bytes = 8192
max_row_count = 500

[op."io/db::connect"]
db_target_allow = ["  sqlite://data/app.db  "]
"#,
    )
    .unwrap();

    let policy = CapsPolicy::load_with_selfhost_authority(
        &caps,
        SelfhostBootstrapMode::ArtifactOnly,
        Some(&selfhost_artifact()),
    )
    .unwrap();

    assert_eq!(
        policy
            .op_policy("io/db::query")
            .unwrap()
            .authorized_database,
        Some(AuthorizedDatabasePolicy {
            target_allow: AuthorizedStringList::Absent,
            query_classes: AuthorizedStringList::Valid(vec![
                "read-only".to_string(),
                "analytics".to_string(),
            ]),
            max_result_bytes: AuthorizedMaxBytes::Valid(8192),
            max_row_count: AuthorizedMaxBytes::Valid(500),
            max_value_bytes: AuthorizedMaxBytes::Absent,
        })
    );
    assert_eq!(
        policy
            .op_policy("io/db::connect")
            .unwrap()
            .authorized_database
            .as_ref()
            .unwrap()
            .target_allow,
        AuthorizedStringList::Valid(vec!["sqlite://data/app.db".to_string()])
    );
}

#[test]
fn selfhost_authority_preserves_invalid_database_policy_states() {
    let cases = [
        (
            "allow_query_classes = \"read-only\"",
            AuthorizedStringList::InvalidType,
            AuthorizedMaxBytes::Absent,
        ),
        (
            "allow_query_classes = [\"read-only\", 7]",
            AuthorizedStringList::InvalidEntry,
            AuthorizedMaxBytes::Absent,
        ),
        (
            "allow_query_classes = [\"\", \"   \"]",
            AuthorizedStringList::Empty,
            AuthorizedMaxBytes::Absent,
        ),
        (
            "max_result_bytes = \"large\"",
            AuthorizedStringList::Absent,
            AuthorizedMaxBytes::InvalidType,
        ),
        (
            "max_result_bytes = 0",
            AuthorizedStringList::Absent,
            AuthorizedMaxBytes::NonPositive,
        ),
    ];
    for (setting, query_classes, max_result_bytes) in cases {
        let td = tempfile::tempdir().unwrap();
        let caps = td.path().join("caps.toml");
        std::fs::write(&caps, format!("[op.\"io/db::query\"]\n{setting}\n")).unwrap();
        let policy = CapsPolicy::load_with_selfhost_authority(
            &caps,
            SelfhostBootstrapMode::ArtifactOnly,
            Some(&selfhost_artifact()),
        )
        .unwrap();
        let database = policy
            .op_policy("io/db::query")
            .unwrap()
            .authorized_database
            .as_ref()
            .unwrap();
        assert_eq!(database.query_classes, query_classes, "setting: {setting}");
        assert_eq!(
            database.max_result_bytes, max_result_bytes,
            "setting: {setting}"
        );
    }
}

#[test]
fn selfhost_authority_rejects_malformed_database_decisions() {
    use super::policy_authority::decode_database_policy;

    let absent_list = string_list_policy(":absent", Term::Nil);
    let absent_limit = max_bytes_policy(":absent", Term::Nil);
    let valid = || {
        database_policy_term(
            absent_list.clone(),
            absent_list.clone(),
            absent_limit.clone(),
            absent_limit.clone(),
            absent_limit.clone(),
        )
    };
    decode_database_policy(&valid(), true).unwrap();

    let cases = [
        database_policy_term(
            string_list_policy(":valid", Term::Nil),
            absent_list.clone(),
            absent_limit.clone(),
            absent_limit.clone(),
            absent_limit.clone(),
        ),
        database_policy_term(
            absent_list.clone(),
            string_list_policy(":valid", Term::Vector(vec![])),
            absent_limit.clone(),
            absent_limit.clone(),
            absent_limit.clone(),
        ),
        database_policy_term(
            absent_list.clone(),
            absent_list.clone(),
            max_bytes_policy(":valid", Term::Int(0.into())),
            absent_limit.clone(),
            absent_limit.clone(),
        ),
    ];
    for decision in cases {
        decode_database_policy(&decision, true)
            .expect_err("contradictory database authority decision must fail closed");
    }
    decode_database_policy(&valid(), false)
        .expect_err("denied operation must not carry a database decision");

    let Term::Map(mut extra) = valid() else {
        return;
    };
    extra.insert(TermOrdKey(Term::symbol(":unknown")), Term::Nil);
    decode_database_policy(&Term::Map(extra), true)
        .expect_err("unknown database authority fields must fail closed");
}

#[test]
fn selfhost_authority_installs_network_policy() {
    let td = tempfile::tempdir().unwrap();
    let caps = td.path().join("caps.toml");
    std::fs::write(
        &caps,
        r#"
[op."io/net::http-listen"]
url_allow = ["  http://127.0.0.1:8080  ", ""]
remote_allow = ["https://ignored.example"]
allow_http = true
wasi_network_profile = "  preview2  "
allow_bind_hosts = [" 127.0.0.1 "]
allow_bind_ports = [8080, " * "]
max_request_bytes = 4096
"#,
    )
    .unwrap();

    let policy = CapsPolicy::load_with_selfhost_authority(
        &caps,
        SelfhostBootstrapMode::ArtifactOnly,
        Some(&selfhost_artifact()),
    )
    .unwrap();
    assert_eq!(
        policy
            .op_policy("io/net::http-listen")
            .unwrap()
            .authorized_network,
        Some(AuthorizedNetworkPolicy {
            url_allow: AuthorizedStringList::Valid(vec!["http://127.0.0.1:8080".to_string()]),
            remote_allow: AuthorizedStringList::Valid(vec!["https://ignored.example".to_string(),]),
            allow_http: AuthorizedOptionalBool::Valid(true),
            wasi_network_profile: AuthorizedOptionalString::Valid("preview2".to_string()),
            bind_hosts: AuthorizedStringList::Valid(vec!["127.0.0.1".to_string()]),
            bind_ports: AuthorizedBindPorts::Valid {
                any: true,
                ports: vec![8080],
            },
            max_request_bytes: AuthorizedMaxBytes::Valid(4096),
        })
    );
}

#[test]
fn selfhost_authority_preserves_invalid_network_policy_states() {
    let cases = [
        (
            "url_allow = \"bad\"",
            AuthorizedStringList::InvalidType,
            AuthorizedBindPorts::Absent,
            AuthorizedMaxBytes::Absent,
        ),
        (
            "allow_bind_ports = [70000]",
            AuthorizedStringList::Absent,
            AuthorizedBindPorts::OutOfRange,
            AuthorizedMaxBytes::Absent,
        ),
        (
            "allow_bind_ports = [\"nope\"]",
            AuthorizedStringList::Absent,
            AuthorizedBindPorts::InvalidEntry,
            AuthorizedMaxBytes::Absent,
        ),
        (
            "max_request_bytes = 0",
            AuthorizedStringList::Absent,
            AuthorizedBindPorts::Absent,
            AuthorizedMaxBytes::NonPositive,
        ),
    ];
    for (setting, target_allow, bind_ports, max_request_bytes) in cases {
        let td = tempfile::tempdir().unwrap();
        let caps = td.path().join("caps.toml");
        std::fs::write(&caps, format!("[op.\"io/net::http-listen\"]\n{setting}\n")).unwrap();
        let policy = CapsPolicy::load_with_selfhost_authority(
            &caps,
            SelfhostBootstrapMode::ArtifactOnly,
            Some(&selfhost_artifact()),
        )
        .unwrap();
        let network = policy
            .op_policy("io/net::http-listen")
            .unwrap()
            .authorized_network
            .as_ref()
            .unwrap();
        assert_eq!(network.url_allow, target_allow, "setting: {setting}");
        assert_eq!(network.bind_ports, bind_ports, "setting: {setting}");
        assert_eq!(
            network.max_request_bytes, max_request_bytes,
            "setting: {setting}"
        );
    }
}

#[test]
fn selfhost_authority_rejects_malformed_network_decisions() {
    use super::policy_authority::decode_network_policy;

    let absent_list = string_list_policy(":absent", Term::Nil);
    let absent_bool = optional_value_policy(":absent", Term::Nil);
    let absent_string = optional_value_policy(":absent", Term::Nil);
    let absent_ports = bind_ports_policy(":absent", Term::Nil, Term::Nil);
    let absent_limit = max_bytes_policy(":absent", Term::Nil);
    let valid = || {
        network_policy_term(
            absent_list.clone(),
            absent_list.clone(),
            absent_bool.clone(),
            absent_string.clone(),
            absent_list.clone(),
            absent_ports.clone(),
            absent_limit.clone(),
        )
    };
    decode_network_policy(&valid(), true).unwrap();

    let cases = [
        network_policy_term(
            string_list_policy(":valid", Term::Nil),
            absent_list.clone(),
            absent_bool.clone(),
            absent_string.clone(),
            absent_list.clone(),
            absent_ports.clone(),
            absent_limit.clone(),
        ),
        network_policy_term(
            absent_list.clone(),
            absent_list.clone(),
            optional_value_policy(":valid", Term::Nil),
            absent_string.clone(),
            absent_list.clone(),
            absent_ports.clone(),
            absent_limit.clone(),
        ),
        network_policy_term(
            absent_list.clone(),
            absent_list.clone(),
            absent_bool.clone(),
            absent_string.clone(),
            absent_list.clone(),
            bind_ports_policy(":valid", Term::Bool(false), Term::Vector(vec![])),
            absent_limit.clone(),
        ),
    ];
    for decision in cases {
        decode_network_policy(&decision, true)
            .expect_err("contradictory network authority decision must fail closed");
    }
    decode_network_policy(&valid(), false)
        .expect_err("denied operation must not carry a network decision");

    let Term::Map(mut extra) = valid() else {
        return;
    };
    extra.insert(TermOrdKey(Term::symbol(":unknown")), Term::Nil);
    decode_network_policy(&Term::Map(extra), true)
        .expect_err("unknown network authority fields must fail closed");
}

#[test]
fn selfhost_authority_installs_crypto_policy() {
    let td = tempfile::tempdir().unwrap();
    let caps = td.path().join("caps.toml");
    std::fs::write(
        &caps,
        r#"
[op."core/crypto::sign"]
allow_algorithms = ["  Ed25519  ", ""]
allow_key_ids = [" key-main "]
max_message_bytes = 4096
max_context_bytes = 128
"#,
    )
    .unwrap();

    let policy = CapsPolicy::load_with_selfhost_authority(
        &caps,
        SelfhostBootstrapMode::ArtifactOnly,
        Some(&selfhost_artifact()),
    )
    .unwrap();
    let crypto = policy
        .op_policy("core/crypto::sign")
        .unwrap()
        .authorized_crypto
        .as_ref()
        .unwrap();
    assert_eq!(
        crypto.algorithms,
        AuthorizedStringList::Valid(vec!["ed25519".to_string()])
    );
    assert_eq!(
        crypto.key_ids,
        AuthorizedStringList::Valid(vec!["key-main".to_string()])
    );
    assert_eq!(crypto.max_message_bytes, AuthorizedMaxBytes::Valid(4096));
    assert_eq!(crypto.max_context_bytes, AuthorizedMaxBytes::Valid(128));
    assert_eq!(crypto.max_signature_bytes, AuthorizedMaxBytes::Absent);
}

#[test]
fn selfhost_authority_preserves_invalid_crypto_policy_states() {
    let cases = [
        (
            "allow_algorithms = \"ed25519\"",
            AuthorizedStringList::InvalidType,
            AuthorizedMaxBytes::Absent,
        ),
        (
            "allow_algorithms = [\"ed25519\", 7]",
            AuthorizedStringList::InvalidEntry,
            AuthorizedMaxBytes::Absent,
        ),
        (
            "allow_algorithms = [\"\", \"   \"]",
            AuthorizedStringList::Empty,
            AuthorizedMaxBytes::Absent,
        ),
        (
            "max_message_bytes = \"large\"",
            AuthorizedStringList::Absent,
            AuthorizedMaxBytes::InvalidType,
        ),
        (
            "max_message_bytes = 0",
            AuthorizedStringList::Absent,
            AuthorizedMaxBytes::NonPositive,
        ),
    ];
    for (setting, algorithms, max_message_bytes) in cases {
        let td = tempfile::tempdir().unwrap();
        let caps = td.path().join("caps.toml");
        std::fs::write(&caps, format!("[op.\"core/crypto::sign\"]\n{setting}\n")).unwrap();
        let policy = CapsPolicy::load_with_selfhost_authority(
            &caps,
            SelfhostBootstrapMode::ArtifactOnly,
            Some(&selfhost_artifact()),
        )
        .unwrap();
        let crypto = policy
            .op_policy("core/crypto::sign")
            .unwrap()
            .authorized_crypto
            .as_ref()
            .unwrap();
        assert_eq!(crypto.algorithms, algorithms, "setting: {setting}");
        assert_eq!(
            crypto.max_message_bytes, max_message_bytes,
            "setting: {setting}"
        );
    }
}

#[test]
fn selfhost_authority_rejects_malformed_crypto_decisions() {
    use super::policy_authority::decode_crypto_policy;

    let absent_list = string_list_policy(":absent", Term::Nil);
    let valid = || crypto_policy_term(absent_list.clone(), absent_list.clone(), []);
    decode_crypto_policy(&valid(), true).unwrap();

    let cases = [
        crypto_policy_term(
            string_list_policy(
                ":valid",
                Term::Vector(vec![Term::Str("ED25519".to_string())]),
            ),
            absent_list.clone(),
            [],
        ),
        crypto_policy_term(
            absent_list.clone(),
            string_list_policy(":valid", Term::Nil),
            [],
        ),
        crypto_policy_term(
            absent_list.clone(),
            absent_list.clone(),
            [(
                ":max-message-bytes",
                max_bytes_policy(":valid", Term::Int(0.into())),
            )],
        ),
    ];
    for decision in cases {
        decode_crypto_policy(&decision, true)
            .expect_err("contradictory crypto authority decision must fail closed");
    }
    decode_crypto_policy(&valid(), false)
        .expect_err("denied operation must not carry a crypto decision");

    let Term::Map(mut extra) = valid() else {
        return;
    };
    extra.insert(TermOrdKey(Term::symbol(":unknown")), Term::Nil);
    decode_crypto_policy(&Term::Map(extra), true)
        .expect_err("unknown crypto authority fields must fail closed");
}

#[test]
fn selfhost_authority_installs_plugin_policy() {
    let td = tempfile::tempdir().unwrap();
    let caps = td.path().join("caps.toml");
    std::fs::write(
        &caps,
        r#"
[op."host/plugin::command"]
allow_plugins = [" demo ", ""]
allow_commands = [" run "]
allow_schema_ids = [" genesis/plugin.request.exec.v1 "]
"#,
    )
    .unwrap();

    let policy = CapsPolicy::load_with_selfhost_authority(
        &caps,
        SelfhostBootstrapMode::ArtifactOnly,
        Some(&selfhost_artifact()),
    )
    .unwrap();
    let plugin = policy
        .op_policy("host/plugin::command")
        .unwrap()
        .authorized_plugin
        .as_ref()
        .unwrap();
    assert_eq!(
        plugin.plugins,
        AuthorizedStringList::Valid(vec!["demo".to_string()])
    );
    assert_eq!(
        plugin.commands,
        AuthorizedStringList::Valid(vec!["run".to_string()])
    );
    assert_eq!(
        plugin.schema_ids,
        AuthorizedStringList::Valid(vec!["genesis/plugin.request.exec.v1".to_string()])
    );
}

#[test]
fn selfhost_authority_preserves_invalid_plugin_policy_states() {
    let cases = [
        (
            "allow_plugins = \"demo\"",
            AuthorizedStringList::InvalidType,
        ),
        (
            "allow_plugins = [\"demo\", 7]",
            AuthorizedStringList::InvalidEntry,
        ),
        (
            "allow_plugins = [\"\", \"   \"]",
            AuthorizedStringList::Empty,
        ),
    ];
    for (setting, plugins) in cases {
        let td = tempfile::tempdir().unwrap();
        let caps = td.path().join("caps.toml");
        std::fs::write(&caps, format!("[op.\"host/plugin::command\"]\n{setting}\n")).unwrap();
        let policy = CapsPolicy::load_with_selfhost_authority(
            &caps,
            SelfhostBootstrapMode::ArtifactOnly,
            Some(&selfhost_artifact()),
        )
        .unwrap();
        let plugin = policy
            .op_policy("host/plugin::command")
            .unwrap()
            .authorized_plugin
            .as_ref()
            .unwrap();
        assert_eq!(plugin.plugins, plugins, "setting: {setting}");
        assert_eq!(plugin.commands, AuthorizedStringList::Absent);
        assert_eq!(plugin.schema_ids, AuthorizedStringList::Absent);
    }
}

#[test]
fn selfhost_authority_rejects_malformed_plugin_decisions() {
    use super::policy_authority::decode_plugin_policy;

    let absent = string_list_policy(":absent", Term::Nil);
    let valid = || plugin_policy_term(absent.clone(), absent.clone(), absent.clone());
    decode_plugin_policy(&valid(), true).unwrap();

    let cases = [
        plugin_policy_term(
            string_list_policy(":valid", Term::Vector(vec![])),
            absent.clone(),
            absent.clone(),
        ),
        plugin_policy_term(
            absent.clone(),
            string_list_policy(":valid", Term::Nil),
            absent.clone(),
        ),
        plugin_policy_term(
            absent.clone(),
            absent.clone(),
            string_list_policy(
                ":valid",
                Term::Vector(vec![Term::Str(" padded ".to_string())]),
            ),
        ),
    ];
    for decision in cases {
        decode_plugin_policy(&decision, true)
            .expect_err("contradictory plugin authority decision must fail closed");
    }
    decode_plugin_policy(&valid(), false)
        .expect_err("denied operation must not carry a plugin decision");

    let Term::Map(mut extra) = valid() else {
        return;
    };
    extra.insert(TermOrdKey(Term::symbol(":unknown")), Term::Nil);
    decode_plugin_policy(&Term::Map(extra), true)
        .expect_err("unknown plugin authority fields must fail closed");
}

#[test]
fn selfhost_authority_installs_bridge_digest_pin_policy() {
    let td = tempfile::tempdir().unwrap();
    let caps = td.path().join("caps.toml");
    std::fs::write(
        &caps,
        format!(
            r#"
[op."host/plugin::command"]
bridge_cmd = "/opt/genesis/plugin-bridge"
bridge_cmd_sha256 = " SHA256:{} "
"#,
            "AB".repeat(32)
        ),
    )
    .unwrap();

    let policy = CapsPolicy::load_with_selfhost_authority(
        &caps,
        SelfhostBootstrapMode::ArtifactOnly,
        Some(&selfhost_artifact()),
    )
    .unwrap();
    assert_eq!(
        policy
            .op_policy("host/plugin::command")
            .unwrap()
            .authorized_bridge_identity,
        Some(AuthorizedBridgeIdentityPolicy {
            active: true,
            allowlist: AuthorizedBridgeAllowlist::Absent,
            args: Vec::new(),
            command: Some("/opt/genesis/plugin-bridge".to_string()),
            pin_required: true,
            digest: AuthorizedBridgeDigest::Valid("ab".repeat(32)),
            transport: AuthorizedBridgeTransport::SpawnPerOp,
            wasi_profile: false,
        })
    );
}

#[test]
fn selfhost_authority_preserves_bridge_digest_states_and_wasi_precedence() {
    let cases = [
        (
            "bridge_cmd_sha256 = 7",
            AuthorizedBridgeDigest::InvalidType,
            true,
        ),
        (
            "bridge_cmd_sha256 = \"   \"",
            AuthorizedBridgeDigest::Empty,
            true,
        ),
        (
            "bridge_cmd_sha256 = \"not-a-digest\"",
            AuthorizedBridgeDigest::InvalidDigest,
            true,
        ),
        (
            "wasi_bridge_profile = true",
            AuthorizedBridgeDigest::Absent,
            false,
        ),
    ];
    for (setting, digest, pin_required) in cases {
        let td = tempfile::tempdir().unwrap();
        let caps = td.path().join("caps.toml");
        std::fs::write(
            &caps,
            format!(
                "[op.\"host/ffi::call\"]\nbridge_cmd = \"/opt/genesis/ffi-bridge\"\n{setting}\n"
            ),
        )
        .unwrap();
        let policy = CapsPolicy::load_with_selfhost_authority(
            &caps,
            SelfhostBootstrapMode::ArtifactOnly,
            Some(&selfhost_artifact()),
        )
        .unwrap();
        assert_eq!(
            policy
                .op_policy("host/ffi::call")
                .unwrap()
                .authorized_bridge_identity,
            Some(AuthorizedBridgeIdentityPolicy {
                active: true,
                allowlist: AuthorizedBridgeAllowlist::Absent,
                args: Vec::new(),
                command: Some("/opt/genesis/ffi-bridge".to_string()),
                pin_required,
                digest,
                transport: AuthorizedBridgeTransport::SpawnPerOp,
                wasi_profile: setting == "wasi_bridge_profile = true",
            }),
            "setting: {setting}"
        );
    }
}

#[test]
fn selfhost_authority_rejects_malformed_bridge_digest_decisions() {
    use super::policy_authority::decode_bridge_identity_policy;

    let valid = || {
        bridge_identity_policy_term(
            optional_value_policy(":valid", Term::Str("a".repeat(64))),
            Term::Bool(true),
        )
    };
    assert_eq!(
        decode_bridge_identity_policy(&valid(), "host/ffi::call", true).unwrap(),
        AuthorizedBridgeIdentityPolicy {
            active: false,
            allowlist: AuthorizedBridgeAllowlist::Absent,
            args: Vec::new(),
            command: None,
            pin_required: true,
            digest: AuthorizedBridgeDigest::Valid("a".repeat(64)),
            transport: AuthorizedBridgeTransport::SpawnPerOp,
            wasi_profile: false,
        }
    );

    let cases = [
        bridge_identity_policy_term(
            optional_value_policy(":valid", Term::Str("A".repeat(64))),
            Term::Bool(true),
        ),
        bridge_identity_policy_term(
            optional_value_policy(":invalid-digest", Term::Str("a".repeat(64))),
            Term::Bool(true),
        ),
        bridge_identity_policy_term(
            optional_value_policy(":valid", Term::Str("a".repeat(64))),
            Term::Bool(true),
        ),
    ];
    decode_bridge_identity_policy(&cases[0], "host/ffi::call", true)
        .expect_err("uppercase canonical digest must fail closed");
    decode_bridge_identity_policy(&cases[1], "host/ffi::call", true)
        .expect_err("contradictory digest state must fail closed");
    decode_bridge_identity_policy(&cases[2], "io/fs::read", true)
        .expect_err("ineligible operation pin requirement must fail closed");
    decode_bridge_identity_policy(&valid(), "host/ffi::call", false)
        .expect_err("denied operation must not carry a bridge decision");

    let Term::Map(mut open) = valid() else {
        return;
    };
    open.insert(TermOrdKey(Term::symbol(":unknown")), Term::Nil);
    decode_bridge_identity_policy(&Term::Map(open), "host/ffi::call", true)
        .expect_err("unknown bridge decision fields must fail closed");
}

#[test]
fn selfhost_authority_normalizes_bridge_allowlist_without_changing_empty_semantics() {
    let cases = [
        (
            "bridge_cmd_allowlist = [\" tool \" , \"tool\", \"/opt/bridge\"]",
            AuthorizedBridgeAllowlist::Valid(vec![
                "tool".to_string(),
                "tool".to_string(),
                "/opt/bridge".to_string(),
            ]),
        ),
        (
            "bridge_cmd_allowlist = []",
            AuthorizedBridgeAllowlist::Valid(Vec::new()),
        ),
        (
            "bridge_cmd_allowlist = \"tool\"",
            AuthorizedBridgeAllowlist::InvalidType,
        ),
        (
            "bridge_cmd_allowlist = [7]",
            AuthorizedBridgeAllowlist::InvalidEntry,
        ),
        (
            "bridge_cmd_allowlist = [\"   \"]",
            AuthorizedBridgeAllowlist::EmptyEntry,
        ),
    ];
    for (setting, expected) in cases {
        let td = tempfile::tempdir().unwrap();
        let caps = td.path().join("caps.toml");
        std::fs::write(&caps, format!("[op.\"host/plugin::command\"]\n{setting}\n")).unwrap();
        let policy = CapsPolicy::load_with_selfhost_authority(
            &caps,
            SelfhostBootstrapMode::ArtifactOnly,
            Some(&selfhost_artifact()),
        )
        .unwrap();
        assert_eq!(
            policy
                .op_policy("host/plugin::command")
                .unwrap()
                .authorized_bridge_identity
                .as_ref()
                .unwrap()
                .allowlist,
            expected,
            "setting: {setting}"
        );
    }
}

#[test]
fn selfhost_authority_rejects_malformed_bridge_allowlist_decisions() {
    use super::policy_authority::decode_bridge_identity_policy;

    let valid = bridge_identity_policy_term(
        optional_value_policy(":absent", Term::Nil),
        Term::Bool(false),
    );
    let Term::Map(mut padded) = valid.clone() else {
        return;
    };
    padded.insert(
        TermOrdKey(Term::symbol(":allowlist")),
        string_list_policy(
            ":valid",
            Term::Vector(vec![Term::Str(" tool ".to_string())]),
        ),
    );
    decode_bridge_identity_policy(&Term::Map(padded), "host/plugin::command", true)
        .expect_err("noncanonical padded allowlist value must fail closed");

    let Term::Map(mut contradiction) = valid.clone() else {
        return;
    };
    contradiction.insert(
        TermOrdKey(Term::symbol(":allowlist")),
        string_list_policy(
            ":invalid-entry",
            Term::Vector(vec![Term::Str("tool".to_string())]),
        ),
    );
    decode_bridge_identity_policy(&Term::Map(contradiction), "host/plugin::command", true)
        .expect_err("contradictory allowlist state must fail closed");

    let Term::Map(mut open) = valid else {
        return;
    };
    open.insert(
        TermOrdKey(Term::symbol(":allowlist")),
        Term::Map(BTreeMap::from([
            (TermOrdKey(Term::symbol(":status")), Term::symbol(":absent")),
            (TermOrdKey(Term::symbol(":unknown")), Term::Nil),
            (TermOrdKey(Term::symbol(":values")), Term::Nil),
        ])),
    );
    decode_bridge_identity_policy(&Term::Map(open), "host/plugin::command", true)
        .expect_err("unknown allowlist state fields must fail closed");
}

#[test]
fn selfhost_authority_owns_bridge_invocation_configuration() {
    let td = tempfile::tempdir().unwrap();
    let caps = td.path().join("caps.toml");
    std::fs::write(
        &caps,
        r#"
[op."host/plugin::command"]
bridge_cmd = " bridge-bin "
bridge_args = [" --mode ", 7, ""]
bridge_transport = " persistent-stdio "
wasi_bridge_profile = true
"#,
    )
    .unwrap();
    let policy = CapsPolicy::load_with_selfhost_authority(
        &caps,
        SelfhostBootstrapMode::ArtifactOnly,
        Some(&selfhost_artifact()),
    )
    .unwrap();
    assert_eq!(
        policy
            .op_policy("host/plugin::command")
            .unwrap()
            .authorized_bridge_identity,
        Some(AuthorizedBridgeIdentityPolicy {
            active: true,
            allowlist: AuthorizedBridgeAllowlist::Absent,
            args: vec![" --mode ".to_string(), "".to_string()],
            command: Some(" bridge-bin ".to_string()),
            pin_required: false,
            digest: AuthorizedBridgeDigest::Absent,
            transport: AuthorizedBridgeTransport::PersistentStdio,
            wasi_profile: true,
        })
    );
}

#[test]
fn selfhost_authority_preserves_bridge_invocation_compatibility_defaults() {
    let cases = [
        (
            "bridge_cmd = 7\nbridge_args = \"bad\"\nbridge_transport = 7\nwasi_bridge_profile = 7",
            None,
            Vec::<String>::new(),
            AuthorizedBridgeTransport::SpawnPerOp,
            false,
            false,
        ),
        (
            "bridge_transport = \" udp-magic \"",
            None,
            Vec::<String>::new(),
            AuthorizedBridgeTransport::Invalid("udp-magic".to_string()),
            false,
            false,
        ),
        (
            "wasi_bridge_response = \" {:ok true} \"",
            None,
            Vec::<String>::new(),
            AuthorizedBridgeTransport::SpawnPerOp,
            false,
            true,
        ),
        (
            "wasi_bridge_response_file = \" ./response.gc \"",
            None,
            Vec::<String>::new(),
            AuthorizedBridgeTransport::SpawnPerOp,
            false,
            true,
        ),
    ];
    for (settings, command, args, transport, wasi_profile, active) in cases {
        let td = tempfile::tempdir().unwrap();
        let caps = td.path().join("caps.toml");
        std::fs::write(&caps, format!("[op.\"gpu/compute::limits\"]\n{settings}\n")).unwrap();
        let policy = CapsPolicy::load_with_selfhost_authority(
            &caps,
            SelfhostBootstrapMode::ArtifactOnly,
            Some(&selfhost_artifact()),
        )
        .unwrap();
        let authority = policy
            .op_policy("gpu/compute::limits")
            .unwrap()
            .authorized_bridge_identity
            .as_ref()
            .unwrap();
        assert_eq!(authority.command, command, "settings: {settings}");
        assert_eq!(authority.args, args, "settings: {settings}");
        assert_eq!(authority.transport, transport, "settings: {settings}");
        assert_eq!(authority.wasi_profile, wasi_profile, "settings: {settings}");
        assert_eq!(authority.active, active, "settings: {settings}");
    }
}

#[test]
fn selfhost_authority_rejects_malformed_bridge_invocation_decisions() {
    use super::policy_authority::decode_bridge_identity_policy;

    let valid = bridge_identity_policy_term(
        optional_value_policy(":absent", Term::Nil),
        Term::Bool(false),
    );
    let mutations = [
        (":active", Term::Nil),
        (":args", Term::Vector(vec![Term::Int(7.into())])),
        (":command", Term::Bool(false)),
        (
            ":transport",
            optional_value_policy(":invalid", Term::Str(" udp-magic ".to_string())),
        ),
        (":wasi-profile", Term::Nil),
    ];
    for (field, value) in mutations {
        let Term::Map(mut malformed) = valid.clone() else {
            return;
        };
        malformed.insert(TermOrdKey(Term::symbol(field)), value);
        decode_bridge_identity_policy(&Term::Map(malformed), "host/plugin::command", true)
            .expect_err("malformed bridge invocation decision must fail closed");
    }
}

#[test]
fn selfhost_authority_installs_normalized_gpu_policy() {
    let td = tempfile::tempdir().unwrap();
    let caps = td.path().join("caps.toml");
    std::fs::write(
        &caps,
        r#"
[op."gpu/compute::limits"]
gpu_backend = " Device-Runtime "
gpu_backend_policy = " REQUIRE-DEVICE "

[op."gfx/gpu::features"]
gpu_backend = " device-runtime-full "
gpu_backend_policy = " dev-allow-fallback "

[op."gpu/compute::submit"]
gpu_backend = "device-bridge"
gpu_backend_policy = "unknown"
"#,
    )
    .unwrap();
    let policy = CapsPolicy::load_with_selfhost_authority(
        &caps,
        SelfhostBootstrapMode::ArtifactOnly,
        Some(&selfhost_artifact()),
    )
    .unwrap();

    let cases = [
        (
            "gpu/compute::limits",
            AuthorizedGpuPolicy {
                backend: AuthorizedGpuBackend::DeviceRuntimeSubmitIntrospection,
                fallback: AuthorizedGpuFallback::RequireDevice,
            },
        ),
        (
            "gfx/gpu::features",
            AuthorizedGpuPolicy {
                backend: AuthorizedGpuBackend::DeviceRuntimeFullLifecycle,
                fallback: AuthorizedGpuFallback::AllowFallback,
            },
        ),
        (
            "gpu/compute::submit",
            AuthorizedGpuPolicy {
                backend: AuthorizedGpuBackend::FirstParty,
                fallback: AuthorizedGpuFallback::AllowFallback,
            },
        ),
    ];
    for (op, expected) in cases {
        assert_eq!(policy.op_policy(op).unwrap().authorized_gpu, Some(expected));
    }
}

#[test]
fn selfhost_authority_binds_observed_gpu_fallback_default() {
    let td = tempfile::tempdir().unwrap();
    let caps = td.path().join("caps.toml");
    std::fs::write(
        &caps,
        "[op.\"gpu/compute::limits\"]\ngpu_backend = \"device-runtime\"\n",
    )
    .unwrap();
    let policy = CapsPolicy::load_with_selfhost_authority(
        &caps,
        SelfhostBootstrapMode::ArtifactOnly,
        Some(&selfhost_artifact()),
    )
    .unwrap();
    let expected_fallback = match std::env::var("GENESIS_GPU_BACKEND_POLICY_DEFAULT")
        .ok()
        .unwrap_or_default()
        .trim()
        .to_ascii_lowercase()
        .as_str()
    {
        "require-device" => AuthorizedGpuFallback::RequireDevice,
        _ => AuthorizedGpuFallback::AllowFallback,
    };
    assert_eq!(
        policy
            .op_policy("gpu/compute::limits")
            .unwrap()
            .authorized_gpu,
        Some(AuthorizedGpuPolicy {
            backend: AuthorizedGpuBackend::DeviceRuntimeSubmitIntrospection,
            fallback: expected_fallback,
        })
    );
}

#[test]
fn selfhost_authority_rejects_malformed_gpu_decisions() {
    use super::policy_authority::{decode_gpu_policy, legacy_gpu_policy};

    assert_eq!(
        legacy_gpu_policy(None, Some(" REQUIRE-DEVICE ")),
        AuthorizedGpuPolicy {
            backend: AuthorizedGpuBackend::FirstParty,
            fallback: AuthorizedGpuFallback::RequireDevice,
        }
    );
    let valid = gpu_policy_term(":device-runtime", ":require-device");
    assert_eq!(
        decode_gpu_policy(&valid, true).unwrap(),
        AuthorizedGpuPolicy {
            backend: AuthorizedGpuBackend::DeviceRuntimeSubmitIntrospection,
            fallback: AuthorizedGpuFallback::RequireDevice,
        }
    );
    for malformed in [
        gpu_policy_term(":device", ":require-device"),
        gpu_policy_term(":device-runtime", ":fallback"),
        Term::Nil,
    ] {
        decode_gpu_policy(&malformed, true)
            .expect_err("malformed GPU authority decision must fail closed");
    }
    decode_gpu_policy(&valid, false).expect_err("denied GPU authority decision must be nil");
    let Term::Map(mut extra) = valid else {
        return;
    };
    extra.insert(TermOrdKey(Term::symbol(":unknown")), Term::Nil);
    decode_gpu_policy(&Term::Map(extra), true)
        .expect_err("open GPU authority decision must fail closed");
}

#[test]
fn selfhost_authority_installs_ffi_policy() {
    let td = tempfile::tempdir().unwrap();
    let caps = td.path().join("caps.toml");
    std::fs::write(
        &caps,
        r#"
[op."host/ffi::call"]
allow_abi_ids = [" abi.math.v1 ", ""]
allow_libraries = [" libmath.so "]
allow_symbols = [" sum_f64 "]
allow_schema_ids = [" genesis/ffi.request.call.v1 "]
max_buffer_bytes = 64
max_call_payload_bytes = 128
signed_policy_required = true
policy_artifact_h = "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa"
policy_signature_h = "bbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb"
policy_key_id = " root-key "
evidence_mode = " deterministic "
"#,
    )
    .unwrap();

    let policy = CapsPolicy::load_with_selfhost_authority(
        &caps,
        SelfhostBootstrapMode::ArtifactOnly,
        Some(&selfhost_artifact()),
    )
    .unwrap();
    let ffi = policy
        .op_policy("host/ffi::call")
        .unwrap()
        .authorized_ffi
        .as_ref()
        .unwrap();
    assert_eq!(
        ffi.abi_ids,
        AuthorizedStringList::Valid(vec!["abi.math.v1".to_string()])
    );
    assert_eq!(
        ffi.libraries,
        AuthorizedStringList::Valid(vec!["libmath.so".to_string()])
    );
    assert_eq!(
        ffi.symbols,
        AuthorizedStringList::Valid(vec!["sum_f64".to_string()])
    );
    assert_eq!(
        ffi.schema_ids,
        AuthorizedStringList::Valid(vec!["genesis/ffi.request.call.v1".to_string()])
    );
    assert_eq!(ffi.max_buffer_bytes, AuthorizedMaxBytes::Valid(64));
    assert_eq!(ffi.max_call_payload_bytes, AuthorizedMaxBytes::Valid(128));
    assert_eq!(
        ffi.signed_policy,
        AuthorizedFfiSignedPolicy::Valid {
            policy_artifact_h: "a".repeat(64),
            policy_signature_h: "b".repeat(64),
            policy_key_id: "root-key".to_string(),
            evidence_mode: "deterministic".to_string(),
        }
    );
}

#[test]
fn selfhost_authority_preserves_invalid_ffi_policy_states() {
    let td = tempfile::tempdir().unwrap();
    let caps = td.path().join("caps.toml");
    std::fs::write(
        &caps,
        r#"
[op."host/ffi::call"]
allow_abi_ids = "abi.math.v1"
allow_libraries = [7]
allow_symbols = ["", "   "]
max_buffer_bytes = "large"
max_call_payload_bytes = 0
signed_policy_required = "yes"
policy_artifact_h = 7
policy_signature_h = "   "
"#,
    )
    .unwrap();
    let policy = CapsPolicy::load_with_selfhost_authority(
        &caps,
        SelfhostBootstrapMode::ArtifactOnly,
        Some(&selfhost_artifact()),
    )
    .unwrap();
    let ffi = policy
        .op_policy("host/ffi::call")
        .unwrap()
        .authorized_ffi
        .as_ref()
        .unwrap();
    assert_eq!(ffi.abi_ids, AuthorizedStringList::InvalidType);
    assert_eq!(ffi.libraries, AuthorizedStringList::InvalidEntry);
    assert_eq!(ffi.symbols, AuthorizedStringList::Empty);
    assert_eq!(ffi.schema_ids, AuthorizedStringList::Absent);
    assert_eq!(ffi.max_buffer_bytes, AuthorizedMaxBytes::InvalidType);
    assert_eq!(ffi.max_call_payload_bytes, AuthorizedMaxBytes::NonPositive);
    assert_eq!(
        ffi.signed_policy,
        AuthorizedFfiSignedPolicy::InvalidRequiredType
    );
}

#[test]
fn selfhost_authority_decides_ffi_signed_policy_precedence() {
    let cases = [
        (
            "signed_policy_required = true".to_string(),
            AuthorizedFfiSignedPolicy::MissingArtifactHash,
        ),
        (
            format!(
                "signed_policy_required = true\npolicy_artifact_h = \"{}\"",
                "z".repeat(64)
            ),
            AuthorizedFfiSignedPolicy::InvalidArtifactHash,
        ),
        (
            format!(
                "signed_policy_required = true\npolicy_artifact_h = \"{}\"",
                "a".repeat(64)
            ),
            AuthorizedFfiSignedPolicy::MissingSignatureHash,
        ),
        (
            format!(
                "signed_policy_required = true\npolicy_artifact_h = \"{}\"\npolicy_signature_h = \"{}\"",
                "a".repeat(64),
                "b".repeat(64)
            ),
            AuthorizedFfiSignedPolicy::MissingKeyId,
        ),
        (
            format!(
                "signed_policy_required = true\npolicy_artifact_h = \"{}\"\npolicy_signature_h = \"{}\"\npolicy_key_id = \"root\"",
                "a".repeat(64),
                "b".repeat(64)
            ),
            AuthorizedFfiSignedPolicy::MissingEvidenceMode,
        ),
        (
            format!(
                "signed_policy_required = true\npolicy_artifact_h = \"{}\"\npolicy_signature_h = \"{}\"\npolicy_key_id = \"root\"\nevidence_mode = \"random\"",
                "a".repeat(64),
                "b".repeat(64)
            ),
            AuthorizedFfiSignedPolicy::InvalidEvidenceMode,
        ),
    ];

    for (body, expected) in cases {
        let td = tempfile::tempdir().unwrap();
        let caps = td.path().join("caps.toml");
        std::fs::write(
            &caps,
            format!("allow = [\"host/ffi::call\"]\n\n[op.\"host/ffi::call\"]\n{body}\n"),
        )
        .unwrap();
        let policy = CapsPolicy::load_with_selfhost_authority(
            &caps,
            SelfhostBootstrapMode::ArtifactOnly,
            Some(&selfhost_artifact()),
        )
        .unwrap();
        assert_eq!(
            policy
                .op_policy("host/ffi::call")
                .unwrap()
                .authorized_ffi
                .as_ref()
                .unwrap()
                .signed_policy,
            expected
        );
    }
}

#[test]
fn selfhost_authority_rejects_malformed_ffi_decisions() {
    use super::policy_authority::decode_ffi_policy;

    let absent_list = string_list_policy(":absent", Term::Nil);
    let absent_limit = max_bytes_policy(":absent", Term::Nil);
    let valid = || {
        ffi_policy_term(
            absent_list.clone(),
            absent_list.clone(),
            absent_list.clone(),
            absent_list.clone(),
            absent_limit.clone(),
            absent_limit.clone(),
        )
    };
    decode_ffi_policy(&valid(), true).unwrap();

    let Term::Map(mut contradictory_signed_policy) = valid() else {
        return;
    };
    contradictory_signed_policy.insert(
        TermOrdKey(Term::symbol(":signed-policy")),
        ffi_signed_policy_term(
            ":disabled",
            Term::Str("a".repeat(64)),
            Term::Nil,
            Term::Nil,
            Term::Nil,
        ),
    );
    let Term::Map(mut invalid_valid_signed_policy) = valid() else {
        return;
    };
    invalid_valid_signed_policy.insert(
        TermOrdKey(Term::symbol(":signed-policy")),
        ffi_signed_policy_term(
            ":valid",
            Term::Str("not-a-hash".to_string()),
            Term::Str("b".repeat(64)),
            Term::Str("root-key".to_string()),
            Term::Str("deterministic".to_string()),
        ),
    );

    let cases = [
        ffi_policy_term(
            string_list_policy(":valid", Term::Vector(vec![])),
            absent_list.clone(),
            absent_list.clone(),
            absent_list.clone(),
            absent_limit.clone(),
            absent_limit.clone(),
        ),
        ffi_policy_term(
            absent_list.clone(),
            absent_list.clone(),
            absent_list.clone(),
            absent_list.clone(),
            max_bytes_policy(":valid", Term::Int(0.into())),
            absent_limit.clone(),
        ),
        ffi_policy_term(
            absent_list.clone(),
            absent_list.clone(),
            absent_list.clone(),
            string_list_policy(
                ":valid",
                Term::Vector(vec![Term::Str(" padded ".to_string())]),
            ),
            absent_limit.clone(),
            absent_limit.clone(),
        ),
        Term::Map(contradictory_signed_policy),
        Term::Map(invalid_valid_signed_policy),
    ];
    for decision in cases {
        decode_ffi_policy(&decision, true)
            .expect_err("contradictory ffi authority decision must fail closed");
    }
    decode_ffi_policy(&valid(), false)
        .expect_err("denied operation must not carry an ffi decision");

    let Term::Map(mut extra) = valid() else {
        return;
    };
    extra.insert(TermOrdKey(Term::symbol(":unknown")), Term::Nil);
    decode_ffi_policy(&Term::Map(extra), true)
        .expect_err("unknown ffi authority fields must fail closed");
}

#[test]
fn selfhost_authority_rejects_noncanonical_operation_controls() {
    let op = "io/fs::write";
    let cases = [
        expected_cap(op, [(":create-dirs", Term::Bool(false))]),
        expected_cap(op, [(":timeout-ms", Term::Int((-1).into()))]),
        expected_cap(
            op,
            [(
                ":timeout-ms",
                Term::Int("18446744073709551616".parse().unwrap()),
            )],
        ),
        expected_cap(op, [(":log-inline-max-bytes", Term::Int(0.into()))]),
        expected_cap("io/fs::read", []),
        expected_cap(op, [(":unknown", Term::Bool(true))]),
    ];

    for cap in cases {
        super::policy_authority::decode_cap(&cap, op)
            .expect_err("noncanonical operation control must fail closed");
    }
}

#[test]
fn selfhost_authority_owns_per_operation_base_directory() {
    let td = tempfile::tempdir().unwrap();
    let caps = td.path().join("caps.toml");
    std::fs::write(
        &caps,
        r#"
[op."io/fs::read"]
base_dir = "./sandbox"
"#,
    )
    .unwrap();

    let artifact = selfhost_artifact();
    let policy = CapsPolicy::load_with_selfhost_authority(
        &caps,
        SelfhostBootstrapMode::ArtifactOnly,
        Some(&artifact),
    )
    .unwrap();

    assert_eq!(
        policy.op_policy("io/fs::read").unwrap().base_dir,
        Some(td.path().join("./sandbox"))
    );
    assert_eq!(
        policy.authorized_cap("io/fs::read"),
        Some(&expected_cap("io/fs::read", []))
    );
}

#[test]
fn selfhost_authority_discards_denied_operation_base_directory() {
    let td = tempfile::tempdir().unwrap();
    let caps = td.path().join("caps.toml");
    std::fs::write(
        &caps,
        r#"
allow = ["io/fs::read"]

[op."io/fs::read"]
allow = false
base_dir = "./must-not-survive"
max_bytes = 7
"#,
    )
    .unwrap();

    let artifact = selfhost_artifact();
    let policy = CapsPolicy::load_with_selfhost_authority(
        &caps,
        SelfhostBootstrapMode::ArtifactOnly,
        Some(&artifact),
    )
    .unwrap();

    assert!(!policy.is_allowed("io/fs::read"));
    assert!(policy.op_policy("io/fs::read").is_none());
    assert!(policy.authorized_cap("io/fs::read").is_none());
}

#[test]
fn selfhost_authority_owns_sorted_unique_candidate_inventory() {
    let td = tempfile::tempdir().unwrap();
    let caps = td.path().join("caps.toml");
    std::fs::write(
        &caps,
        r#"
allow = ["sys/time::now", "sys/time::now", "io/fs::read"]

[op."core/task::await"]
allow = true

[op."io/fs::read"]
allow = false
"#,
    )
    .unwrap();

    let artifact = selfhost_artifact();
    let policy = CapsPolicy::load_with_selfhost_authority(
        &caps,
        SelfhostBootstrapMode::ArtifactOnly,
        Some(&artifact),
    )
    .unwrap();

    assert!(policy.is_allowed("sys/time::now"));
    assert!(policy.is_allowed("core/task::await"));
    assert!(!policy.is_allowed("io/fs::read"));
}

#[test]
fn selfhost_authority_owns_runtime_and_task_resource_composition() {
    let td = tempfile::tempdir().unwrap();
    let caps = td.path().join("caps.toml");
    std::fs::write(
        &caps,
        r#"
[runtime]
max_effect_ops = 11
max_payload_bytes_per_op = 12
max_payload_bytes_per_run = 13
max_response_bytes_per_op = 14
max_response_bytes_per_run = 15

[task]
default_workers = 2
max_tasks = 21
max_workers = 22
max_queue = 23
max_steps_per_task = 24
max_time_ms_per_task = 25
"#,
    )
    .unwrap();

    let artifact = selfhost_artifact();
    let policy = CapsPolicy::load_with_selfhost_authority(
        &caps,
        SelfhostBootstrapMode::ArtifactOnly,
        Some(&artifact),
    )
    .unwrap();

    assert_eq!(policy.runtime.max_effect_ops, Some(11));
    assert_eq!(policy.runtime.max_payload_bytes_per_op, Some(12));
    assert_eq!(policy.runtime.max_payload_bytes_per_run, Some(13));
    assert_eq!(policy.runtime.max_response_bytes_per_op, Some(14));
    assert_eq!(policy.runtime.max_response_bytes_per_run, Some(15));
    assert_eq!(policy.task.default_workers, 2);
    assert_eq!(policy.task.max_tasks, Some(21));
    assert_eq!(policy.task.max_workers, Some(22));
    assert_eq!(policy.task.max_queue, Some(23));
    assert_eq!(policy.task.max_steps_per_task, Some(24));
    assert_eq!(policy.task.max_time_ms_per_task, Some(25));
}

#[test]
fn selfhost_authority_owns_adaptive_task_worker_default() {
    let td = tempfile::tempdir().unwrap();
    let caps = td.path().join("caps.toml");
    std::fs::write(&caps, "").unwrap();

    let artifact = selfhost_artifact();
    let policy = CapsPolicy::load_with_selfhost_authority(
        &caps,
        SelfhostBootstrapMode::ArtifactOnly,
        Some(&artifact),
    )
    .unwrap();
    let expected = std::thread::available_parallelism()
        .map(|workers| workers.get() as u64)
        .unwrap_or(1)
        .max(1);

    assert_eq!(policy.task.default_workers, expected);
    assert_eq!(policy.log.store_dir, None);
    assert_eq!(
        policy.store.dir,
        Some(td.path().join(".genesis").join("store"))
    );
    assert_eq!(
        policy.refs.path,
        Some(td.path().join(".genesis").join("refs.gc"))
    );
}

#[test]
fn selfhost_authority_owns_global_log_and_store_resource_limits() {
    let td = tempfile::tempdir().unwrap();
    let caps = td.path().join("caps.toml");
    std::fs::write(
        &caps,
        r#"
[log]
inline_max_bytes = 123
max_artifact_bytes_per_run = 456

[store]
max_run_bytes = 2048
"#,
    )
    .unwrap();

    let artifact = selfhost_artifact();
    let policy = CapsPolicy::load_with_selfhost_authority(
        &caps,
        SelfhostBootstrapMode::ArtifactOnly,
        Some(&artifact),
    )
    .unwrap();

    assert_eq!(policy.log.inline_max_bytes, Some(123));
    assert_eq!(policy.log.max_artifact_bytes_per_run, Some(456));
    assert_eq!(policy.store.max_run_bytes, Some(2048));
}

#[test]
fn selfhost_authority_owns_global_store_remote_policy() {
    let td = tempfile::tempdir().unwrap();
    let caps = td.path().join("caps.toml");
    std::fs::write(
        &caps,
        r#"
[store]
remote = "  https://registry.example.com/root  "
remote_allow = ["  https://registry.example.com/root/v1/  ", ""]
allow_http = false
"#,
    )
    .unwrap();

    let policy = CapsPolicy::load_with_selfhost_authority(
        &caps,
        SelfhostBootstrapMode::ArtifactOnly,
        Some(&selfhost_artifact()),
    )
    .unwrap();

    assert_eq!(
        policy.authorized_store_remote(),
        Some(&AuthorizedStoreRemotePolicy {
            remote: AuthorizedOptionalString::Valid(
                "https://registry.example.com/root".to_string(),
            ),
            remote_allow: AuthorizedStringList::Valid(vec![
                "https://registry.example.com/root/v1/".to_string(),
            ]),
            allow_http: AuthorizedOptionalBool::Valid(false),
        })
    );
}

#[test]
fn selfhost_authority_preserves_malformed_global_store_remote_states() {
    let td = tempfile::tempdir().unwrap();
    let caps = td.path().join("caps.toml");
    std::fs::write(
        &caps,
        r#"
[store]
remote = 7
remote_allow = "https://registry.example.com/v1/"
allow_http = "yes"
"#,
    )
    .unwrap();

    let policy = CapsPolicy::load_with_selfhost_authority(
        &caps,
        SelfhostBootstrapMode::ArtifactOnly,
        Some(&selfhost_artifact()),
    )
    .unwrap();
    assert_eq!(
        policy.authorized_store_remote(),
        Some(&AuthorizedStoreRemotePolicy {
            remote: AuthorizedOptionalString::InvalidType,
            remote_allow: AuthorizedStringList::InvalidType,
            allow_http: AuthorizedOptionalBool::InvalidType,
        })
    );
}

#[test]
fn selfhost_authority_rejects_contradictory_store_remote_decisions() {
    use super::policy_authority::decode_store_remote_policy;

    let valid = store_remote_policy_term(
        optional_value_policy(":valid", Term::Str("https://safe.example/v1/".to_string())),
        string_list_policy(
            ":valid",
            Term::Vector(vec![Term::Str("https://safe.example/v1/".to_string())]),
        ),
        optional_value_policy(":valid", Term::Bool(false)),
    );
    decode_store_remote_policy(&valid).unwrap();

    let cases = [
        store_remote_policy_term(
            optional_value_policy(":absent", Term::Str("https://unsafe.example/".to_string())),
            string_list_policy(":absent", Term::Nil),
            optional_value_policy(":absent", Term::Nil),
        ),
        store_remote_policy_term(
            optional_value_policy(":valid", Term::Str(" padded ".to_string())),
            string_list_policy(":absent", Term::Nil),
            optional_value_policy(":absent", Term::Nil),
        ),
        store_remote_policy_term(
            optional_value_policy(":absent", Term::Nil),
            string_list_policy(":valid", Term::Vector(Vec::new())),
            optional_value_policy(":absent", Term::Nil),
        ),
        store_remote_policy_term(
            optional_value_policy(":absent", Term::Nil),
            string_list_policy(":absent", Term::Nil),
            optional_value_policy(":valid", Term::Nil),
        ),
    ];
    for decision in cases {
        decode_store_remote_policy(&decision)
            .expect_err("contradictory store remote decision must fail closed");
    }

    let Term::Map(mut open) = valid else {
        return;
    };
    open.insert(TermOrdKey(Term::symbol(":extra")), Term::Nil);
    decode_store_remote_policy(&Term::Map(open))
        .expect_err("open store remote decision must fail closed");
}

#[test]
fn selfhost_authority_normalizes_nonpositive_global_resource_limits() {
    let td = tempfile::tempdir().unwrap();
    let caps = td.path().join("caps.toml");
    std::fs::write(
        &caps,
        r#"
[log]
inline_max_bytes = 0
max_artifact_bytes_per_run = -1

[store]
max_run_bytes = 0
"#,
    )
    .unwrap();

    let artifact = selfhost_artifact();
    let policy = CapsPolicy::load_with_selfhost_authority(
        &caps,
        SelfhostBootstrapMode::ArtifactOnly,
        Some(&artifact),
    )
    .unwrap();

    assert_eq!(policy.log.inline_max_bytes, None);
    assert_eq!(policy.log.max_artifact_bytes_per_run, None);
    assert_eq!(policy.store.max_run_bytes, None);
}

#[test]
fn selfhost_authority_owns_default_global_storage_locations() {
    let td = tempfile::tempdir().unwrap();
    let caps = td.path().join("caps.toml");
    std::fs::write(
        &caps,
        r#"
[log]
inline_max_bytes = 1
"#,
    )
    .unwrap();

    let artifact = selfhost_artifact();
    let policy = CapsPolicy::load_with_selfhost_authority(
        &caps,
        SelfhostBootstrapMode::ArtifactOnly,
        Some(&artifact),
    )
    .unwrap();

    assert_eq!(
        policy.log.store_dir,
        Some(td.path().join(".genesis").join("store"))
    );
    assert_eq!(
        policy.store.dir,
        Some(td.path().join(".genesis").join("store"))
    );
    assert_eq!(
        policy.refs.path,
        Some(td.path().join(".genesis").join("refs.gc"))
    );
}

#[test]
fn selfhost_authority_preserves_explicit_global_storage_locations() {
    let td = tempfile::tempdir().unwrap();
    let caps = td.path().join("caps.toml");
    std::fs::write(
        &caps,
        r#"
[log]
store_dir = "./log-store"

[refs]
path = "./state/refs.gc"

[store]
dir = "./content-store"
"#,
    )
    .unwrap();

    let artifact = selfhost_artifact();
    let policy = CapsPolicy::load_with_selfhost_authority(
        &caps,
        SelfhostBootstrapMode::ArtifactOnly,
        Some(&artifact),
    )
    .unwrap();

    assert_eq!(policy.log.store_dir, Some(td.path().join("./log-store")));
    assert_eq!(policy.refs.path, Some(td.path().join("./state/refs.gc")));
    assert_eq!(policy.store.dir, Some(td.path().join("./content-store")));
}

#[test]
fn selfhost_authority_rejects_unbounded_operation_inventories_before_evaluation() {
    let td = tempfile::tempdir().unwrap();
    let caps = td.path().join("caps.toml");
    let allow = (0..=4096)
        .map(|index| format!("\"test/op::{index}\""))
        .collect::<Vec<_>>()
        .join(", ");
    std::fs::write(&caps, format!("allow = [{allow}]\n")).unwrap();

    let missing_artifact = td.path().join("must-not-be-loaded.gc");
    let err = CapsPolicy::load_with_selfhost_authority(
        &caps,
        SelfhostBootstrapMode::ArtifactOnly,
        Some(&missing_artifact),
    )
    .expect_err("oversized policy must fail before loading an artifact");

    assert!(
        err.to_string()
            .contains("operation inventory exceeds fixed limit 4096")
    );
}

#[test]
fn rejects_legacy_top_level_op_tables() {
    let err = CapsPolicy::from_toml_str(
        r#"
allow = ["io/fs::read"]

["io/fs::read"]
base_dir = "./x"
"#,
    )
    .expect_err("legacy top-level op tables must be rejected");
    assert!(format!("{err}").contains("top-level table"));
    assert!(format!("{err}").contains("[op.\"io/fs::read\"]"));
}

#[test]
fn supports_canonical_op_table() {
    let p = CapsPolicy::from_toml_str(
        r#"
allow = ["io/fs::read"]

[op."io/fs::read"]
base_dir = "./x"
"#,
    )
    .unwrap();
    assert!(p.is_allowed("io/fs::read"));
    assert!(p.op_policy("io/fs::read").unwrap().base_dir.is_some());
}

#[test]
fn rejects_unknown_top_level_keys() {
    let err = CapsPolicy::from_toml_str(
        r#"
allow = ["io/fs::read"]
custom = "x"
"#,
    )
    .expect_err("unknown top-level keys must be rejected");
    assert!(format!("{err}").contains("unknown top-level key"));
    assert!(format!("{err}").contains("custom"));
}

#[test]
fn parses_log_policy_and_resolves_defaults() {
    let p = CapsPolicy::from_toml_str(
        r#"
allow = ["sys/time::now"]

[log]
inline_max_bytes = 123
store_dir = "./s"
max_artifact_bytes_per_run = 456
"#,
    )
    .unwrap();
    assert_eq!(p.log.inline_max_bytes, Some(123));
    assert_eq!(p.log.max_artifact_bytes_per_run, Some(456));
    assert!(p.log.store_dir.is_some());
}

#[test]
fn parses_store_run_budget() {
    let p = CapsPolicy::from_toml_str(
        r#"
allow = ["core/store::put"]

[store]
max_run_bytes = 2048
"#,
    )
    .unwrap();
    assert_eq!(p.store.max_run_bytes, Some(2048));
}

#[test]
fn parses_store_auth_policy_fields() {
    let p = CapsPolicy::from_toml_str(
        r#"
allow = ["core/store::get"]

[store]
auth_token = "token-value"
auth_token_env = "GENESIS_TEST_TOKEN"
basic_username = "robot"
basic_password = "s3cr3t"
basic_password_env = "GENESIS_BASIC_PASS"
mtls_ca_pem = "./ca.pem"
mtls_identity_pem = "./id.pem"
"#,
    )
    .unwrap();
    assert_eq!(p.store.auth_token.as_deref(), Some("token-value"));
    assert_eq!(
        p.store.auth_token_env.as_deref(),
        Some("GENESIS_TEST_TOKEN")
    );
    assert_eq!(p.store.basic_username.as_deref(), Some("robot"));
    assert_eq!(p.store.basic_password.as_deref(), Some("s3cr3t"));
    assert_eq!(
        p.store.basic_password_env.as_deref(),
        Some("GENESIS_BASIC_PASS")
    );
    assert_eq!(p.store.mtls_ca_pem.as_deref(), Some(Path::new("./ca.pem")));
    assert_eq!(
        p.store.mtls_identity_pem.as_deref(),
        Some(Path::new("./id.pem"))
    );
}

#[test]
fn load_resolves_relative_mtls_paths() {
    let td = tempfile::tempdir().unwrap();
    let caps = td.path().join("caps.toml");
    std::fs::write(
        &caps,
        r#"
allow = ["core/sync::pull"]

[store]
mtls_ca_pem = "./certs/ca.pem"
mtls_identity_pem = "./certs/id.pem"

[op."core/sync::pull"]
mtls_ca_pem = "./certs/op-ca.pem"
"#,
    )
    .unwrap();
    let p = CapsPolicy::load(&caps).unwrap();
    assert!(p.store.mtls_ca_pem.as_ref().unwrap().is_absolute());
    assert!(p.store.mtls_identity_pem.as_ref().unwrap().is_absolute());
    let op = p.op_policy("core/sync::pull").unwrap();
    assert!(
        op.extra
            .get("mtls_ca_pem")
            .and_then(|v| v.as_str())
            .is_some_and(|s| Path::new(s).is_absolute())
    );
}

#[test]
fn per_op_inline_max_overrides_global() {
    let p = CapsPolicy::from_toml_str(
        r#"
allow = ["sys/time::now"]

[log]
inline_max_bytes = 10

[op."sys/time::now"]
log_inline_max_bytes = 5
"#,
    )
    .unwrap();
    assert_eq!(p.inline_max_bytes_for("sys/time::now"), Some(5));
}

#[test]
fn parses_task_policy_limits() {
    let p = CapsPolicy::from_toml_str(
        r#"
allow = ["core/task::await"]

[task]
default_workers = 3
max_tasks = 12
max_workers = 4
max_queue = 16
max_steps_per_task = 20
max_time_ms_per_task = 50
"#,
    )
    .unwrap();
    assert_eq!(p.task.default_workers, 3);
    assert_eq!(p.task.max_tasks, Some(12));
    assert_eq!(p.task.max_workers, Some(4));
    assert_eq!(p.task.max_queue, Some(16));
    assert_eq!(p.task.max_steps_per_task, Some(20));
    assert_eq!(p.task.max_time_ms_per_task, Some(50));
}

#[test]
fn parses_runtime_policy_limits() {
    let p = CapsPolicy::from_toml_str(
        r#"
allow = ["sys/time::now"]

[runtime]
max_effect_ops = 12
max_payload_bytes_per_op = 4096
max_payload_bytes_per_run = 8192
max_response_bytes_per_op = 2048
max_response_bytes_per_run = 4096
"#,
    )
    .unwrap();
    assert_eq!(p.runtime.max_effect_ops, Some(12));
    assert_eq!(p.runtime.max_payload_bytes_per_op, Some(4096));
    assert_eq!(p.runtime.max_payload_bytes_per_run, Some(8192));
    assert_eq!(p.runtime.max_response_bytes_per_op, Some(2048));
    assert_eq!(p.runtime.max_response_bytes_per_run, Some(4096));
}

#[test]
fn runtime_policy_allows_zero_limits_for_fail_closed_mode() {
    let p = CapsPolicy::from_toml_str(
        r#"
allow = ["sys/time::now"]

[runtime]
max_effect_ops = 0
max_payload_bytes_per_op = 0
max_payload_bytes_per_run = 0
max_response_bytes_per_op = 0
max_response_bytes_per_run = 0
"#,
    )
    .unwrap();
    assert_eq!(p.runtime.max_effect_ops, Some(0));
    assert_eq!(p.runtime.max_payload_bytes_per_op, Some(0));
    assert_eq!(p.runtime.max_payload_bytes_per_run, Some(0));
    assert_eq!(p.runtime.max_response_bytes_per_op, Some(0));
    assert_eq!(p.runtime.max_response_bytes_per_run, Some(0));
}

#[test]
fn rejects_negative_runtime_policy_limits() {
    let err = CapsPolicy::from_toml_str(
        r#"
allow = ["sys/time::now"]

[runtime]
max_effect_ops = -1
"#,
    )
    .expect_err("must reject negative runtime policy limits");
    assert!(format!("{err}").contains("runtime.max_effect_ops"));
}

#[test]
fn defaults_task_worker_budget_to_adaptive_host_parallelism_when_unspecified() {
    let p = CapsPolicy::from_toml_str(
        r#"
allow = ["core/task::await"]
"#,
    )
    .unwrap();
    let expected = std::thread::available_parallelism()
        .map(|n| n.get() as u64)
        .unwrap_or(1)
        .max(1);
    assert_eq!(p.task.default_workers, expected);
}

#[test]
fn rejects_zero_default_workers() {
    let err = CapsPolicy::from_toml_str(
        r#"
allow = ["core/task::await"]

[task]
default_workers = 0
"#,
    )
    .expect_err("must reject zero default workers");
    assert!(format!("{err}").contains("task.default_workers"));
}

#[test]
fn pkg_snapshot_allow_does_not_implicitly_allow_load_package() {
    let p = CapsPolicy::from_toml_str(
        r#"
allow = ["core/pkg-low::snapshot"]

[op."core/pkg-low::snapshot"]
base_dir = "."
"#,
    )
    .unwrap();
    assert!(!p.is_allowed("core/pkg-low::load-package"));
    assert!(p.op_policy("core/pkg-low::load-package").is_none());
}

#[test]
fn low_level_allow_does_not_authorize_high_level_alias() {
    let p = CapsPolicy::from_toml_str(
        r#"
allow = ["core/gc-low::pin"]

[op."core/gc-low::pin"]
timeout_ms = 10
"#,
    )
    .unwrap();
    assert!(!p.is_allowed("core/gc::pin"));
    assert!(p.op_policy("core/gc::pin").is_none());
}

#[test]
fn rejects_legacy_high_level_ops_in_allow_and_op_tables() {
    let err = CapsPolicy::from_toml_str(
        r#"
allow = ["core/pkg::lock"]
"#,
    )
    .expect_err("must reject retired high-level op in allow list");
    assert!(format!("{err}").contains("legacy high-level op `core/pkg::lock`"));

    let err = CapsPolicy::from_toml_str(
        r#"
allow = ["core/pkg-low::lock"]

[op."core/pkg::lock"]
base_dir = "."
"#,
    )
    .expect_err("must reject retired high-level op in op table");
    assert!(format!("{err}").contains("legacy high-level op `core/pkg::lock`"));
}

#[cfg(unix)]
#[test]
fn rejects_non_utf8_resolved_tls_paths_without_lossy_replacement() {
    use std::ffi::OsString;
    use std::os::unix::ffi::OsStringExt;

    let mut policy = CapsPolicy::from_toml_str(
        r#"
allow = ["net/http::request"]

[op."net/http::request"]
mtls_ca_pem = "ca.pem"
"#,
    )
    .unwrap();
    let base = std::path::PathBuf::from(OsString::from_vec(vec![
        b'n', b'o', b'n', 0xff, b'u', b't', b'f',
    ]));

    let err = policy
        .resolve_relative_paths(&base)
        .expect_err("non-UTF-8 resolved path must fail closed");
    let message = err.to_string();
    assert!(message.contains("resolved TLS path is not valid UTF-8"));
    assert!(!message.contains('\u{fffd}'));
}
