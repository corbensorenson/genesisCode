use super::{
    EX_PARSE, EX_VERIFY, SelfhostBootstrapMode, enforce_bootstrap_mode_allowed_with_flag,
    json_canonical_string, parse_sync_set_refs,
};
use crate::vcs_helpers::parse_set_ref_spec;
use crate::warm_request::validate_workspace_argv;

#[test]
fn parse_set_ref_spec_supports_contract_refs_with_colons() {
    let commit = "a".repeat(64);
    let policy = "b".repeat(64);
    let expected_old = "c".repeat(64);
    let spec = format!(
        "refs/contracts/my-lib/counter::Counter/heads/dev:{commit}:{policy}@{expected_old}"
    );
    let parsed = parse_set_ref_spec(&spec).expect("parse");
    assert_eq!(
        parsed.name,
        "refs/contracts/my-lib/counter::Counter/heads/dev"
    );
    assert_eq!(parsed.hash, commit);
    assert_eq!(parsed.policy, policy);
    assert_eq!(parsed.expected_old.as_deref(), Some(expected_old.as_str()));
}

#[test]
fn parse_set_ref_spec_rejects_invalid_hashes() {
    let err = parse_set_ref_spec("refs/heads/main:nothex:alsonothex").expect_err("must fail");
    assert_eq!(err.exit_code, EX_PARSE);
}

#[test]
fn parse_set_ref_spec_accepts_expected_old_nil() {
    let commit = "a".repeat(64);
    let policy = "b".repeat(64);
    let spec = format!("refs/heads/main:{commit}:{policy}@nil");
    let parsed = parse_set_ref_spec(&spec).expect("parse");
    assert_eq!(parsed.expected_old.as_deref(), Some("nil"));
}

#[test]
fn parse_set_ref_spec_supports_contract_refs_without_expected_old() {
    let commit = "a".repeat(64);
    let policy = "b".repeat(64);
    let spec = format!("refs/contracts/p::q/heads/dev:{commit}:{policy}");
    let parsed = parse_set_ref_spec(&spec).expect("parse");
    assert_eq!(parsed.name, "refs/contracts/p::q/heads/dev");
    assert_eq!(parsed.hash, commit);
    assert_eq!(parsed.policy, policy);
    assert_eq!(parsed.expected_old, None);
}

#[test]
fn json_canonical_string_sorts_object_keys_recursively() {
    let value = serde_json::json!({
        "z": 1,
        "a": {
            "y": 2,
            "x": [{"b": 1, "a": 2}]
        }
    });
    let s = json_canonical_string(&value);
    assert_eq!(s, r#"{"a":{"x":[{"a":2,"b":1}],"y":2},"z":1}"#);
}

#[test]
fn parse_sync_set_refs_rejects_duplicate_targets() {
    let commit = "a".repeat(64);
    let policy = "b".repeat(64);
    let specs = vec![
        format!("refs/heads/main:{commit}:{policy}"),
        format!("refs/heads/main:{commit}:{policy}@nil"),
    ];
    let err = parse_sync_set_refs(&specs).expect_err("must fail");
    assert_eq!(err.exit_code, EX_PARSE);
}

#[test]
fn non_artifact_bootstrap_mode_is_dev_only() {
    let err =
        enforce_bootstrap_mode_allowed_with_flag(SelfhostBootstrapMode::Embedded, "test", false)
            .expect_err("embedded bootstrap should be rejected outside development mode");
    assert_eq!(err.exit_code, EX_VERIFY);
    assert!(err.json.message.contains("development-only"));
    enforce_bootstrap_mode_allowed_with_flag(SelfhostBootstrapMode::Embedded, "test", true)
        .expect("embedded bootstrap should be allowed in development mode");
}

#[test]
fn logical_memory_flags_parse_and_are_session_owned() {
    use clap::Parser;

    let cli = super::Cli::parse_from([
        "genesis",
        "--max-alloc-units",
        "123",
        "--max-live-units",
        "45",
        "cli-schema",
    ]);
    assert_eq!(cli.max_alloc_units, Some(123));
    assert_eq!(cli.max_live_units, Some(45));

    for flag in ["--max-alloc-units=1", "--max-live-units", "1"] {
        let args = if flag == "1" {
            continue;
        } else {
            vec![flag.to_string()]
        };
        let error = validate_workspace_argv(&args, std::path::Path::new("."))
            .expect_err("request must not override a session-owned logical limit");
        assert_eq!(error.code, "warm/resource-override");
    }
}
