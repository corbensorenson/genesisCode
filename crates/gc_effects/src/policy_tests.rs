use super::{
    AuthorizedDatabasePolicy, AuthorizedMaxBytes, AuthorizedProcessPrograms, AuthorizedStringList,
    CapsPolicy,
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
