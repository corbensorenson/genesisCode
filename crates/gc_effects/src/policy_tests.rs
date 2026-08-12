use super::CapsPolicy;
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
