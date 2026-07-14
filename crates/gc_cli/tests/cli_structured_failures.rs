use std::fs;
use std::path::{Path, PathBuf};

use assert_cmd::cargo::cargo_bin_cmd;
use gc_coreform::{canonicalize_module, hash_module, parse_module};
use serde_json::Value;

fn run_failure(args: &[String]) -> Value {
    let output = cargo_bin_cmd!("genesis_parity")
        .args(args)
        .assert()
        .failure()
        .get_output()
        .stdout
        .clone();
    serde_json::from_slice(&output).expect("failure must emit JSON")
}

fn assert_failure_context(value: &Value, domain: &str, temp_root: &Path) {
    let context = value
        .pointer("/error/context")
        .unwrap_or_else(|| panic!("missing error context for {domain}: {value}"));
    assert_eq!(context["schema"], "genesis/failure-context-v0.1");
    assert_eq!(context["domain"], domain);
    assert!(context["kind"].as_str().is_some_and(|x| !x.is_empty()));
    assert!(context["operation"].as_str().is_some_and(|x| !x.is_empty()));
    assert!(context["facts"].is_object());
    assert!(context["primary_span"].is_null() || context["primary_span"].is_object());
    assert!(context["related_spans"].is_array());
    assert!(
        !context
            .to_string()
            .contains(&temp_root.display().to_string()),
        "structured deterministic context leaked absolute temp root: {context}"
    );
}

fn strings(args: &[&str]) -> Vec<String> {
    args.iter().map(|arg| (*arg).to_string()).collect()
}

fn write_type_error_package(root: &Path) -> PathBuf {
    let source = r#"
      (def ::meta (quote {:exports [] :types {}}))
      nil
    "#;
    let forms = canonicalize_module(parse_module(source).expect("parse fixture"))
        .expect("canonicalize fixture");
    let hash = hash_module(&forms)
        .iter()
        .map(|byte| format!("{byte:02x}"))
        .collect::<String>();
    fs::write(root.join("bad.gc"), source).expect("write type error module");
    fs::write(
        root.join("package.toml"),
        format!(
            r#"schema = 1
name = "structured_type_error"
version = "0.0.1"
dependencies = []
obligations = []
tests = []

[[modules]]
path = "bad.gc"
hash = "{hash}"
"#,
        ),
    )
    .expect("write type error manifest");
    root.join("package.toml")
}

#[test]
fn failures_expose_closed_structured_contexts_for_all_authority_domains() {
    let td = tempfile::tempdir().expect("tempdir");
    let root = td.path();
    let malformed = root.join("malformed.gc");
    let evaluator = root.join("evaluator.gc");
    let effect = root.join("effect.gc");
    let denied_effect = root.join("denied-effect.gc");
    let bad_caps = root.join("bad-caps.toml");
    let caps = root.join("caps.toml");
    let bad_log = root.join("bad.gclog");
    let missing_pkg = root.join("missing-package.toml");
    let missing_patch = root.join("missing.gcpatch");
    let type_pkg = write_type_error_package(root);

    fs::write(&malformed, "(").expect("write malformed source");
    fs::write(&evaluator, "(missing/function 1)\n").expect("write evaluator source");
    fs::write(&effect, "(core/effect::pure 1)\n").expect("write effect source");
    fs::write(
        &denied_effect,
        "(def prog (((core/effect::perform (quote io/fs::read)) {:path \"input.txt\"}) (fn (bytes) (core/effect::pure bytes))))\nprog\n",
    )
    .expect("write denied effect source");
    fs::write(&bad_caps, "allow = [\n").expect("write bad caps");
    fs::write(&caps, "allow = []\n").expect("write caps");
    fs::write(&bad_log, "nil\n").expect("write bad log");

    let parser = run_failure(&[
        "--json".into(),
        "fmt".into(),
        malformed.display().to_string(),
        "--engine".into(),
        "rust".into(),
    ]);
    assert_failure_context(&parser, "parser", root);
    assert!(parser.pointer("/error/context/primary_span").is_some());

    let evaluator_failure = run_failure(&[
        "--json".into(),
        "eval".into(),
        evaluator.display().to_string(),
        "--engine".into(),
        "rust".into(),
    ]);
    assert_failure_context(&evaluator_failure, "evaluator", root);

    let package = run_failure(&[
        "--json".into(),
        "--coreform-frontend".into(),
        "rust".into(),
        "typecheck".into(),
        "--pkg".into(),
        missing_pkg.display().to_string(),
    ]);
    assert_failure_context(&package, "package", root);

    let policy = run_failure(&[
        "--json".into(),
        "run".into(),
        effect.display().to_string(),
        "--engine".into(),
        "rust".into(),
        "--caps".into(),
        bad_caps.display().to_string(),
    ]);
    assert_failure_context(&policy, "policy", root);

    let denied_log = root.join("denied.gclog");
    let denied = run_failure(&[
        "--json".into(),
        "run".into(),
        denied_effect.display().to_string(),
        "--engine".into(),
        "rust".into(),
        "--caps".into(),
        caps.display().to_string(),
        "--log".into(),
        denied_log.display().to_string(),
    ]);
    assert_failure_context(&denied, "policy", root);
    assert_eq!(denied["error"]["code"], "caps/denied");
    assert_eq!(
        denied["diagnostics"][0]["blocking_capability"],
        "io/fs::read"
    );
    let policy_diff = &denied["diagnostics"][0]["repair_plan"]["policy_diff"];
    assert_eq!(policy_diff["capability"], "io/fs::read");
    assert_eq!(policy_diff["requires_review"], true);
    assert_eq!(policy_diff["auto_apply"], false);
    assert_eq!(
        denied["diagnostics"][0]["repair_plan"]["authorization"]["policy_change_allowed"],
        false
    );

    let replay = run_failure(&[
        "--json".into(),
        "replay".into(),
        effect.display().to_string(),
        "--engine".into(),
        "rust".into(),
        "--log".into(),
        bad_log.display().to_string(),
    ]);
    assert_failure_context(&replay, "replay", root);

    let patch = run_failure(&[
        "--json".into(),
        "--coreform-frontend".into(),
        "rust".into(),
        "apply-patch".into(),
        missing_patch.display().to_string(),
        "--pkg".into(),
        missing_pkg.display().to_string(),
    ]);
    assert_failure_context(&patch, "patch", root);

    let build = run_failure(&[
        "--json".into(),
        "--coreform-frontend".into(),
        "rust".into(),
        "pkg".into(),
        "--caps".into(),
        caps.display().to_string(),
        "build".into(),
        "--pkg".into(),
        missing_pkg.display().to_string(),
        "--target".into(),
        "web".into(),
    ]);
    assert_failure_context(&build, "build", root);

    let zeros = "0".repeat(64);
    let deployment = run_failure(&[
        "--json".into(),
        "--coreform-frontend".into(),
        "rust".into(),
        "pkg".into(),
        "--caps".into(),
        caps.display().to_string(),
        "publish".into(),
        "--remote".into(),
        "gen://example.invalid/registry".into(),
        "--ref".into(),
        "refs/heads/main".into(),
        "--policy".into(),
        zeros.clone(),
        "--commit".into(),
        zeros,
    ]);
    assert_failure_context(&deployment, "deployment", root);

    let typechecker = run_failure(&[
        "--json".into(),
        "--coreform-frontend".into(),
        "rust".into(),
        "typecheck".into(),
        "--pkg".into(),
        type_pkg.display().to_string(),
    ]);
    assert_failure_context(&typechecker, "typechecker", root);
    assert!(
        typechecker
            .pointer("/data/diagnostics")
            .and_then(Value::as_array)
            .is_some_and(|diagnostics| !diagnostics.is_empty())
    );
}

#[test]
fn human_failure_rendering_remains_concise() {
    let td = tempfile::tempdir().expect("tempdir");
    let malformed = td.path().join("malformed.gc");
    fs::write(&malformed, "(").expect("write malformed source");
    let output = cargo_bin_cmd!("genesis_parity")
        .args(strings(&[
            "fmt",
            malformed.to_str().expect("utf8 path"),
            "--engine",
            "rust",
        ]))
        .assert()
        .failure()
        .get_output()
        .stderr
        .clone();
    let rendered = String::from_utf8(output).expect("utf8 stderr");
    assert!(!rendered.trim().is_empty());
    assert!(rendered.lines().count() <= 4, "verbose failure: {rendered}");
    assert!(!rendered.contains("failure-context-v0.1"));
}
