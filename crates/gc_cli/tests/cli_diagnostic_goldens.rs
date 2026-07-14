use std::collections::BTreeSet;
use std::fs;
use std::path::{Path, PathBuf};

use assert_cmd::cargo::cargo_bin_cmd;
use gc_coreform::{canonicalize_module, hash_module, parse_module, parse_term};
use gc_effects::{Decision, EffectLog};
use serde_json::{Value, json};

const GOLDEN_SCHEMA: &str = "genesis/diagnostic-goldens-v0.1";
const GOLDEN_PATH: &str = "../../tests/diagnostics/goldens/v0.1/diagnostics.json";
const REQUIRED_CLASSES: [&str; 10] = [
    "exhausted-budgets",
    "incompatible-profiles",
    "invalid-packages",
    "malformed-syntax",
    "path-normalization",
    "replay-tampering",
    "seal-misuse",
    "stale-patches",
    "type-effect-mismatch",
    "unhandled-effects",
];

fn hex32(hash: [u8; 32]) -> String {
    hash.iter().map(|byte| format!("{byte:02x}")).collect()
}

fn write_module_package(root: &Path, name: &str, module_name: &str, source: &str) -> PathBuf {
    fs::create_dir_all(root).expect("create package root");
    let forms = canonicalize_module(parse_module(source).expect("parse fixture module"))
        .expect("canonicalize fixture module");
    fs::write(root.join(module_name), source).expect("write fixture module");
    fs::write(
        root.join("package.toml"),
        format!(
            "schema = 1\nname = \"{name}\"\nversion = \"0.0.1\"\ndependencies = []\nobligations = []\ntests = []\n\n[[modules]]\npath = \"{module_name}\"\nhash = \"{}\"\n",
            hex32(hash_module(&forms))
        ),
    )
    .expect("write fixture manifest");
    root.join("package.toml")
}

fn run_json(root: &Path, args: &[String], expect_success: bool) -> Value {
    let mut command = cargo_bin_cmd!("genesis_parity");
    command.current_dir(root).args(args);
    let output = if expect_success {
        command.assert().success().get_output().stdout.clone()
    } else {
        command.assert().failure().get_output().stdout.clone()
    };
    serde_json::from_slice(&output).expect("command must emit a JSON envelope")
}

fn strings(args: &[&str]) -> Vec<String> {
    args.iter().map(|arg| (*arg).to_string()).collect()
}

fn failure_case(id: &str, class: &str, root: &Path, args: Vec<String>) -> Value {
    let envelope = run_json(root, &args, false);
    let diagnostic = envelope
        .pointer("/diagnostics/0")
        .expect("failure must expose a diagnostic");
    assert_eq!(envelope.get("ok").and_then(Value::as_bool), Some(false));
    assert_eq!(
        envelope.get("diagnostics_schema").and_then(Value::as_str),
        Some("genesis/diagnostics-schema-v1")
    );
    assert_eq!(
        envelope
            .pointer("/error/context/schema")
            .and_then(Value::as_str),
        Some("genesis/failure-context-v0.1")
    );
    assert_eq!(
        diagnostic
            .pointer("/repair_plan/authorization/policy_change_allowed")
            .and_then(Value::as_bool),
        Some(false)
    );
    assert_eq!(
        diagnostic
            .pointer("/repair_plan/authorization/obligation_suppression_allowed")
            .and_then(Value::as_bool),
        Some(false)
    );
    let projection = json!({
        "ok": envelope.get("ok").cloned().unwrap_or(Value::Null),
        "kind": envelope.get("kind").cloned().unwrap_or(Value::Null),
        "diagnostics_schema": envelope
            .get("diagnostics_schema")
            .cloned()
            .unwrap_or(Value::Null),
        "diagnostic_catalog": envelope
            .get("diagnostic_catalog")
            .cloned()
            .unwrap_or(Value::Null),
        "error": envelope.get("error").cloned().unwrap_or(Value::Null),
        "diagnostics": envelope
            .get("diagnostics")
            .cloned()
            .unwrap_or(Value::Null),
    });
    let serialized = serde_json::to_string(&projection).expect("serialize diagnostic projection");
    assert!(
        !serialized.contains(&root.display().to_string()),
        "{id} leaked its absolute fixture root"
    );
    json!({"id": id, "class": class, "envelope": projection})
}

fn build_cases(root: &Path) -> Vec<Value> {
    let malformed = root.join("malformed.gc");
    fs::write(&malformed, "(").expect("write malformed syntax fixture");

    let nested = root.join("private-host-root");
    fs::create_dir_all(&nested).expect("create nested root");
    let path_manifest = nested.join("path-package.toml");
    fs::write(
        &path_manifest,
        "schema = 1\nname = \"path-case\"\nversion = \"0.0.1\"\n",
    )
    .expect("write path fixture");

    let invalid_manifest = root.join("invalid-package.toml");
    fs::write(
        &invalid_manifest,
        "schema = 99\nname = \"invalid\"\nversion = \"0.0.1\"\ndependencies = []\nobligations = []\ntests = []\nmodules = []\n",
    )
    .expect("write invalid package fixture");

    let type_root = root.join("type-effect");
    let type_source = r#"
(def ::meta
  '{:exports [golden/type::program]
    :caps [core/task::await]
    :strict-effects true
    :types {golden/type::program (Prog ? (Eff [core/task::await] ?))}})
(def golden/type::program
  (core/effect::perform
    'core/task::await
    {:task-id "task-1"}
    (fn (response) (core/effect::pure response))))
golden/type::program
"#;
    let type_manifest = write_module_package(
        &type_root,
        "diagnostic_type_effect",
        "type-effect.gc",
        type_source,
    );

    let unknown_effect = root.join("unknown-effect.gc");
    fs::write(
        &unknown_effect,
        "(core/effect::perform 'agent/unknown::op nil (fn (x) (core/effect::pure x)))\n",
    )
    .expect("write unhandled effect fixture");
    let unknown_caps = root.join("unknown-caps.toml");
    fs::write(&unknown_caps, "allow = [\"agent/unknown::op\"]\n")
        .expect("write unhandled effect policy");

    let seal_misuse = root.join("seal-misuse.gc");
    fs::write(&seal_misuse, "(seal 1 0)\n").expect("write seal misuse fixture");

    let budget = root.join("budget.gc");
    fs::write(&budget, "((fn (x) x) 1)\n").expect("write budget fixture");

    let replay_program = root.join("replay.gc");
    let replay_log = root.join("replay.gclog");
    let replay_caps = root.join("replay-caps.toml");
    fs::write(
        &replay_program,
        "(core/effect::perform 'sys/time::now nil (fn (x) (core/effect::pure x)))\n",
    )
    .expect("write replay fixture");
    fs::write(&replay_caps, "allow = [\"sys/time::now\"]\n").expect("write replay policy");
    run_json(
        root,
        &[
            "--json".into(),
            "run".into(),
            replay_program.display().to_string(),
            "--engine".into(),
            "rust".into(),
            "--caps".into(),
            replay_caps.display().to_string(),
            "--log".into(),
            replay_log.display().to_string(),
        ],
        true,
    );
    let log_source = fs::read_to_string(&replay_log).expect("read replay log");
    let mut log = EffectLog::from_term(&parse_term(&log_source).expect("parse replay log"))
        .expect("decode replay log");
    log.entries[0].decision = Decision::Deny;
    fs::write(&replay_log, log.to_string_canonical() + "\n").expect("write tampered replay log");

    let patch_root = root.join("stale-patch");
    let patch_source = "(def ::meta '{:exports [] :types {}})\nnil\n";
    let patch_manifest = write_module_package(
        &patch_root,
        "diagnostic_stale_patch",
        "module.gc",
        patch_source,
    );
    let stale_patch = patch_root.join("stale.gcpatch");
    fs::write(
        &stale_patch,
        r#"{
  :version 1
  :intent "exercise a stale semantic node identity"
  :provenance {}
  :ops [{:op :replace-node-id
         :module-path "module.gc"
         :node-id "ffffffffffffffffffffffffffffffffffffffffffffffffffffffffffffffff"
         :new nil}]}
"#,
    )
    .expect("write stale patch fixture");

    let profile_root = root.join("profile");
    fs::create_dir_all(&profile_root).expect("create profile fixture");
    let profile_caps = profile_root.join("caps.toml");
    fs::write(&profile_caps, "allow = []\n").expect("write profile caps");
    fs::write(profile_root.join("module.gc"), "1\n").expect("write profile module");
    fs::write(
        profile_root.join("genesis.workspace.toml"),
        r#"version = 1
workspace = "diagnostic-profile"

[[members]]
name = "diagnostic-profile"
path = "."
role = "root"

[defaults]
policy = "policy:default-v0.1"
runtime_backend = "backend"

[tasks."eval-local"]
cmd = "eval"
file = "module.gc"
"#,
    )
    .expect("write incompatible profile fixture");

    let mut cases = vec![
        failure_case(
            "malformed-syntax",
            "malformed-syntax",
            root,
            vec![
                "--json".into(),
                "fmt".into(),
                malformed.display().to_string(),
                "--engine".into(),
                "rust".into(),
            ],
        ),
        failure_case(
            "type-effect-row-mismatch",
            "type-effect-mismatch",
            root,
            vec![
                "--json".into(),
                "--coreform-frontend".into(),
                "rust".into(),
                "typecheck".into(),
                "--pkg".into(),
                type_manifest.display().to_string(),
            ],
        ),
        failure_case(
            "unhandled-effect-operation",
            "unhandled-effects",
            root,
            vec![
                "--json".into(),
                "run".into(),
                unknown_effect.display().to_string(),
                "--engine".into(),
                "rust".into(),
                "--caps".into(),
                unknown_caps.display().to_string(),
                "--log".into(),
                root.join("unknown.gclog").display().to_string(),
            ],
        ),
        failure_case(
            "non-token-seal-argument",
            "seal-misuse",
            root,
            vec![
                "--json".into(),
                "eval".into(),
                seal_misuse.display().to_string(),
                "--engine".into(),
                "rust".into(),
            ],
        ),
        failure_case(
            "tampered-replay-decision",
            "replay-tampering",
            root,
            vec![
                "--json".into(),
                "replay".into(),
                replay_program.display().to_string(),
                "--engine".into(),
                "rust".into(),
                "--log".into(),
                replay_log.display().to_string(),
            ],
        ),
        failure_case(
            "absolute-manifest-path-normalized",
            "path-normalization",
            root,
            vec![
                "--json".into(),
                "--coreform-frontend".into(),
                "rust".into(),
                "typecheck".into(),
                "--pkg".into(),
                path_manifest.display().to_string(),
            ],
        ),
        failure_case(
            "kernel-step-budget-exhausted",
            "exhausted-budgets",
            root,
            vec![
                "--json".into(),
                "--step-limit".into(),
                "1".into(),
                "eval".into(),
                budget.display().to_string(),
                "--engine".into(),
                "rust".into(),
            ],
        ),
        failure_case(
            "unsupported-package-schema",
            "invalid-packages",
            root,
            vec![
                "--json".into(),
                "--coreform-frontend".into(),
                "rust".into(),
                "typecheck".into(),
                "--pkg".into(),
                invalid_manifest.display().to_string(),
            ],
        ),
        failure_case(
            "stale-semantic-node-id",
            "stale-patches",
            root,
            vec![
                "--json".into(),
                "--coreform-frontend".into(),
                "rust".into(),
                "apply-patch".into(),
                stale_patch.display().to_string(),
                "--pkg".into(),
                patch_manifest.display().to_string(),
            ],
        ),
        failure_case(
            "incompatible-runtime-backend",
            "incompatible-profiles",
            &profile_root,
            vec![
                "--json".into(),
                "gcpm".into(),
                "--caps".into(),
                profile_caps.display().to_string(),
                "run".into(),
                "eval-local".into(),
            ],
        ),
    ];
    cases.sort_by(|left, right| left["id"].as_str().cmp(&right["id"].as_str()));
    cases
}

#[test]
fn diagnostic_failure_envelopes_match_versioned_goldens() {
    let temp = tempfile::tempdir().expect("create diagnostic golden fixture root");
    let cases = build_cases(temp.path());
    let classes = cases
        .iter()
        .map(|case| case["class"].as_str().expect("case class"))
        .collect::<BTreeSet<_>>();
    assert_eq!(classes.len(), cases.len(), "golden classes must be unique");
    assert_eq!(
        classes,
        REQUIRED_CLASSES.into_iter().collect(),
        "roadmap diagnostic classes changed without updating the golden authority"
    );
    let actual = json!({
        "schema": GOLDEN_SCHEMA,
        "case_count": cases.len(),
        "cases": cases,
    });
    let rendered = serde_json::to_string_pretty(&actual).expect("render golden corpus") + "\n";
    let golden_path = Path::new(env!("CARGO_MANIFEST_DIR")).join(GOLDEN_PATH);
    if std::env::var("GENESIS_UPDATE_DIAGNOSTIC_GOLDENS").as_deref() == Ok("1") {
        fs::create_dir_all(golden_path.parent().expect("golden parent"))
            .expect("create golden parent");
        fs::write(&golden_path, rendered).expect("update diagnostic goldens");
        return;
    }
    let expected = fs::read_to_string(&golden_path).unwrap_or_else(|error| {
        panic!(
            "read {}: {error}; run scripts/update_cli_diagnostic_goldens.sh",
            golden_path.display()
        )
    });
    assert_eq!(
        rendered, expected,
        "diagnostic golden drift; inspect the semantic change and run scripts/update_cli_diagnostic_goldens.sh only when intentional"
    );
}

#[test]
fn diagnostic_golden_update_requires_exact_opt_in() {
    assert_ne!(
        std::env::var("GENESIS_UPDATE_DIAGNOSTIC_GOLDENS").as_deref(),
        Ok("true"),
        "the updater accepts only GENESIS_UPDATE_DIAGNOSTIC_GOLDENS=1"
    );
    assert_eq!(strings(&["--json", "eval"]), vec!["--json", "eval"]);
}
