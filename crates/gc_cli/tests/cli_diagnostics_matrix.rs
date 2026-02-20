use std::collections::BTreeSet;
use std::fs;

use assert_cmd::cargo::cargo_bin_cmd;
use serde_json::Value;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum Expect {
    Success,
    Failure,
}

#[derive(Clone, Debug)]
struct Case {
    family: &'static str,
    expect: Expect,
    argv: Vec<String>,
}

fn hash64() -> String {
    "0".repeat(64)
}

fn as_strings(xs: &[&str]) -> Vec<String> {
    xs.iter().map(|x| (*x).to_string()).collect()
}

fn run_case(case: &Case) {
    let output = match case.expect {
        Expect::Success => cargo_bin_cmd!("genesis")
            .args(&case.argv)
            .assert()
            .success()
            .get_output()
            .stdout
            .clone(),
        Expect::Failure => cargo_bin_cmd!("genesis")
            .args(&case.argv)
            .assert()
            .failure()
            .get_output()
            .stdout
            .clone(),
    };
    let value: Value =
        serde_json::from_slice(&output).expect("all matrix cases must emit valid --json envelopes");

    assert_eq!(
        value.get("diagnostics_schema").and_then(Value::as_str),
        Some("genesis/diagnostics-schema-v1"),
        "family {} must carry diagnostics schema",
        case.family
    );
    let diagnostics = value
        .get("diagnostics")
        .and_then(Value::as_array)
        .expect("diagnostics array");

    match case.expect {
        Expect::Success => {
            assert_eq!(
                value.get("ok").and_then(Value::as_bool),
                Some(true),
                "family {} should succeed",
                case.family
            );
            assert!(
                diagnostics.is_empty(),
                "family {} success envelopes must have empty diagnostics",
                case.family
            );
        }
        Expect::Failure => {
            assert_eq!(
                value.get("ok").and_then(Value::as_bool),
                Some(false),
                "family {} should fail",
                case.family
            );
            assert_eq!(
                value.get("kind").and_then(Value::as_str),
                Some("genesis/error-v0.2"),
                "family {} failure kind drift",
                case.family
            );
            assert!(
                !diagnostics.is_empty(),
                "family {} failures must emit diagnostics",
                case.family
            );
            let diag = diagnostics.first().expect("first diagnostic");
            let code = diag.get("code").and_then(Value::as_str).unwrap_or_default();
            assert!(
                !code.is_empty() && code != "error/unknown",
                "family {} must emit machine-actionable diagnostic code",
                case.family
            );
            assert_eq!(
                diag.get("version").and_then(Value::as_str),
                Some("v1"),
                "family {} diagnostic version drift",
                case.family
            );
            assert_eq!(
                diag.get("severity").and_then(Value::as_str),
                Some("error"),
                "family {} severity drift",
                case.family
            );
            assert!(
                diag.get("message")
                    .and_then(Value::as_str)
                    .is_some_and(|m| !m.trim().is_empty()),
                "family {} diagnostics must include non-empty messages",
                case.family
            );
            assert!(
                diag.get("exit_code")
                    .and_then(Value::as_u64)
                    .is_some_and(|ec| ec > 0),
                "family {} diagnostics must include positive exit_code",
                case.family
            );
            assert_eq!(
                value
                    .get("error")
                    .and_then(Value::as_object)
                    .and_then(|e| e.get("code"))
                    .and_then(Value::as_str),
                Some(code),
                "family {} envelope/error code drift",
                case.family
            );
            if code.starts_with("parse/")
                || code.starts_with("io/")
                || code.starts_with("caps/")
                || code.starts_with("replay/")
                || code.starts_with("obligation/")
                || code.starts_with("test/")
                || code.starts_with("typecheck/")
            {
                assert!(
                    diag.get("suggested_fix")
                        .and_then(Value::as_str)
                        .is_some_and(|s| !s.trim().is_empty()),
                    "family {} code {} should include suggested_fix",
                    case.family,
                    code
                );
            }
        }
    }
}

#[test]
fn diagnostics_contract_covers_all_cli_command_families() {
    let td = tempfile::tempdir().expect("tempdir");
    let missing = td.path().join("missing.gc");
    let missing_log = td.path().join("missing.gclog");
    let missing_pkg = td.path().join("missing-package.toml");
    let missing_patch = td.path().join("missing.patch");
    let missing_caps = td.path().join("missing-caps.toml");
    let missing_policy_cfg = td.path().join("missing-policies.toml");
    let missing_store = td.path().join("missing-store");
    let key_out_dir = td.path().join("new-key-dir");
    let key_out = key_out_dir.join("agent-key.toml");
    let registry_root = td.path().join("registry-root");
    fs::create_dir_all(&registry_root).expect("create registry root");

    let h = hash64();

    let cases = vec![
        Case {
            family: "fmt",
            expect: Expect::Failure,
            argv: vec!["--json".into(), "fmt".into(), missing.display().to_string()],
        },
        Case {
            family: "eval",
            expect: Expect::Failure,
            argv: vec![
                "--json".into(),
                "eval".into(),
                missing.display().to_string(),
            ],
        },
        Case {
            family: "explain",
            expect: Expect::Failure,
            argv: vec![
                "--json".into(),
                "explain".into(),
                missing.display().to_string(),
                "--contract".into(),
                "c".into(),
                "--msg".into(),
                "(core/msg::make 'pkg/op::x nil)".into(),
            ],
        },
        Case {
            family: "run",
            expect: Expect::Failure,
            argv: vec![
                "--json".into(),
                "run".into(),
                missing.display().to_string(),
                "--caps".into(),
                missing_caps.display().to_string(),
            ],
        },
        Case {
            family: "replay",
            expect: Expect::Failure,
            argv: vec![
                "--json".into(),
                "replay".into(),
                missing.display().to_string(),
                "--log".into(),
                missing_log.display().to_string(),
            ],
        },
        Case {
            family: "test",
            expect: Expect::Failure,
            argv: vec![
                "--json".into(),
                "test".into(),
                "--pkg".into(),
                missing_pkg.display().to_string(),
            ],
        },
        Case {
            family: "pack",
            expect: Expect::Failure,
            argv: vec![
                "--json".into(),
                "pack".into(),
                "--pkg".into(),
                missing_pkg.display().to_string(),
            ],
        },
        Case {
            family: "typecheck",
            expect: Expect::Failure,
            argv: vec![
                "--json".into(),
                "typecheck".into(),
                "--pkg".into(),
                missing_pkg.display().to_string(),
            ],
        },
        Case {
            family: "optimize",
            expect: Expect::Failure,
            argv: vec![
                "--json".into(),
                "optimize".into(),
                missing.display().to_string(),
            ],
        },
        Case {
            family: "apply-patch",
            expect: Expect::Failure,
            argv: vec![
                "--json".into(),
                "apply-patch".into(),
                missing_patch.display().to_string(),
                "--pkg".into(),
                missing_pkg.display().to_string(),
            ],
        },
        Case {
            family: "semantic-edit",
            expect: Expect::Failure,
            argv: vec![
                "--json".into(),
                "semantic-edit".into(),
                "index".into(),
                "--pkg".into(),
                missing_pkg.display().to_string(),
                "--module-path".into(),
                "src/main.gc".into(),
            ],
        },
        Case {
            family: "verify",
            expect: Expect::Failure,
            argv: vec![
                "--json".into(),
                "verify".into(),
                "--pkg".into(),
                missing_pkg.display().to_string(),
            ],
        },
        Case {
            family: "store",
            expect: Expect::Failure,
            argv: vec![
                "--json".into(),
                "store".into(),
                "--caps".into(),
                missing_caps.display().to_string(),
                "has".into(),
                h.clone(),
            ],
        },
        Case {
            family: "refs",
            expect: Expect::Failure,
            argv: vec![
                "--json".into(),
                "refs".into(),
                "--caps".into(),
                missing_caps.display().to_string(),
                "get".into(),
                "refs/heads/main".into(),
            ],
        },
        Case {
            family: "commit",
            expect: Expect::Failure,
            argv: vec![
                "--json".into(),
                "commit".into(),
                "--caps".into(),
                missing_caps.display().to_string(),
                "show".into(),
                h.clone(),
            ],
        },
        Case {
            family: "pkg",
            expect: Expect::Failure,
            argv: vec![
                "--json".into(),
                "pkg".into(),
                "--caps".into(),
                missing_caps.display().to_string(),
                "lock".into(),
            ],
        },
        Case {
            family: "policy",
            expect: Expect::Failure,
            argv: vec![
                "--json".into(),
                "policy".into(),
                "show".into(),
                "default".into(),
                "--policies".into(),
                missing_policy_cfg.display().to_string(),
                "--store".into(),
                missing_store.display().to_string(),
            ],
        },
        Case {
            family: "sync",
            expect: Expect::Failure,
            argv: vec![
                "--json".into(),
                "sync".into(),
                "--caps".into(),
                missing_caps.display().to_string(),
                "pull".into(),
                "--remote".into(),
                "gen://example.invalid/registry".into(),
                "--root".into(),
                h.clone(),
            ],
        },
        Case {
            family: "registry",
            expect: Expect::Failure,
            argv: vec![
                "--json".into(),
                "registry".into(),
                "serve".into(),
                "--addr".into(),
                "not_an_addr".into(),
                "--root".into(),
                registry_root.display().to_string(),
                "--max-requests".into(),
                "1".into(),
            ],
        },
        Case {
            family: "gc",
            expect: Expect::Failure,
            argv: vec![
                "--json".into(),
                "gc".into(),
                "--caps".into(),
                missing_caps.display().to_string(),
                "plan".into(),
            ],
        },
        Case {
            family: "vcs",
            expect: Expect::Failure,
            argv: vec![
                "--json".into(),
                "vcs".into(),
                "--caps".into(),
                missing_caps.display().to_string(),
                "log".into(),
                "refs/heads/main".into(),
            ],
        },
        Case {
            family: "keygen",
            expect: Expect::Success,
            argv: vec![
                "--json".into(),
                "keygen".into(),
                "--out".into(),
                key_out.display().to_string(),
            ],
        },
        Case {
            family: "sign",
            expect: Expect::Failure,
            argv: vec![
                "--json".into(),
                "sign".into(),
                "--pkg".into(),
                missing_pkg.display().to_string(),
                "--key".into(),
                key_out.display().to_string(),
            ],
        },
        Case {
            family: "transparency-verify",
            expect: Expect::Failure,
            argv: vec![
                "--json".into(),
                "transparency-verify".into(),
                "--pkg".into(),
                missing_pkg.display().to_string(),
            ],
        },
        Case {
            family: "cli-schema",
            expect: Expect::Success,
            argv: as_strings(&["--json", "cli-schema"]),
        },
        Case {
            family: "agent-index",
            expect: Expect::Success,
            argv: as_strings(&["--json", "agent-index"]),
        },
        Case {
            family: "selfhost-dashboard",
            expect: Expect::Success,
            argv: vec![
                "--json".into(),
                "selfhost-dashboard".into(),
                "--markdown".into(),
                td.path().join("selfhost-cutover.md").display().to_string(),
                "--store".into(),
                td.path()
                    .join("selfhost-dashboard-store")
                    .display()
                    .to_string(),
            ],
        },
    ];

    for case in &cases {
        run_case(case);
    }

    let covered: BTreeSet<&str> = cases.iter().map(|c| c.family).collect();
    let excluded: BTreeSet<&str> = ["selfhost-artifact", "warm"].into_iter().collect();

    let cli_schema = cargo_bin_cmd!("genesis")
        .args(["--json", "cli-schema"])
        .assert()
        .success()
        .get_output()
        .stdout
        .clone();
    let schema: Value = serde_json::from_slice(&cli_schema).expect("parse cli-schema");
    let top_level = schema
        .get("data")
        .and_then(|d| d.get("command"))
        .and_then(|c| c.get("subcommands"))
        .and_then(Value::as_array)
        .expect("cli-schema subcommands");

    for cmd in top_level {
        let name = cmd
            .get("name")
            .and_then(Value::as_str)
            .expect("subcommand name");
        assert!(
            covered.contains(name) || excluded.contains(name),
            "top-level command family `{name}` missing from diagnostics contract matrix"
        );
    }
}
