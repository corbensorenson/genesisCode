use std::path::{Path, PathBuf};

use assert_cmd::cargo::cargo_bin_cmd;
use serde_json::Value;

mod support;

fn cmd() -> assert_cmd::Command {
    cargo_bin_cmd!("genesis_parity")
}

fn build_selfhost_artifact(dir: &Path) -> PathBuf {
    support::copy_repo_toolchain_artifact(dir)
}

fn copy_fixture(src: &Path, dst: &Path) -> PathBuf {
    std::fs::create_dir_all(dst).expect("create fixture destination");
    let out = dst.join(src.file_name().expect("fixture name"));
    std::fs::copy(src, &out).expect("copy fixture");
    out
}

fn copy_pkg_basic_fixture(dst: &Path) -> PathBuf {
    std::fs::create_dir_all(dst).expect("create pkg fixture destination");
    let fixture = Path::new(concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/../../tests/spec/pkg_basic"
    ));
    for name in ["basic.gc", "caps.toml", "package.toml", "pure.gcpatch"] {
        std::fs::copy(fixture.join(name), dst.join(name)).expect("copy pkg fixture file");
    }
    dst.join("package.toml")
}

fn run_case(frontend: &str, artifact: Option<&Path>, argv: &[String], cwd: &Path) -> (i32, Value) {
    let mut full_argv = vec![
        "--json".to_string(),
        "--coreform-frontend".to_string(),
        frontend.to_string(),
    ];
    if let Some(artifact) = artifact {
        full_argv.push("--selfhost-artifact".to_string());
        full_argv.push(artifact.display().to_string());
    }
    full_argv.extend_from_slice(argv);

    let output = cmd()
        .current_dir(cwd)
        .args(&full_argv)
        .output()
        .expect("run parity cli");
    let code = output.status.code().unwrap_or(-1);
    let json: Value = serde_json::from_slice(&output.stdout)
        .expect("all differential corpus cases must emit json");
    (code, json)
}

fn ptr_str<'a>(value: &'a Value, pointer: &str) -> Option<&'a str> {
    value.pointer(pointer).and_then(Value::as_str)
}

fn ptr_u64(value: &Value, pointer: &str) -> Option<u64> {
    value.pointer(pointer).and_then(Value::as_u64)
}

fn assert_parity_failure_shape(case_name: &str, rust_json: &Value, self_json: &Value) {
    assert_eq!(
        rust_json.get("ok").and_then(Value::as_bool),
        Some(false),
        "case {case_name}: rust frontend should fail"
    );
    assert_eq!(
        self_json.get("ok").and_then(Value::as_bool),
        Some(false),
        "case {case_name}: selfhost frontend should fail"
    );
    assert_eq!(
        ptr_str(rust_json, "/kind"),
        ptr_str(self_json, "/kind"),
        "case {case_name}: failure kind drift"
    );
    assert_eq!(
        ptr_str(rust_json, "/diagnostics_schema"),
        ptr_str(self_json, "/diagnostics_schema"),
        "case {case_name}: diagnostics schema drift"
    );
    assert_eq!(
        ptr_str(rust_json, "/error/code"),
        ptr_str(self_json, "/error/code"),
        "case {case_name}: envelope error code drift"
    );
    assert_eq!(
        ptr_str(rust_json, "/diagnostics/0/code"),
        ptr_str(self_json, "/diagnostics/0/code"),
        "case {case_name}: first diagnostic code drift"
    );
    assert_eq!(
        ptr_u64(rust_json, "/diagnostics/0/exit_code"),
        ptr_u64(self_json, "/diagnostics/0/exit_code"),
        "case {case_name}: first diagnostic exit_code drift"
    );
}

#[test]
fn malformed_and_adversarial_corpus_matches_between_rust_and_selfhost_frontends() {
    let td = tempfile::tempdir().expect("tempdir");
    let root = td.path();
    let artifact = build_selfhost_artifact(root);
    let corpus_root = Path::new(concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/../../tests/spec/adversarial_coreform"
    ));

    let malformed_unterminated = copy_fixture(
        &corpus_root.join("malformed_unterminated.gc"),
        &root.join("cases"),
    );
    let malformed_map_odd = copy_fixture(
        &corpus_root.join("malformed_map_odd.gc"),
        &root.join("cases"),
    );
    let adversarial_deep = copy_fixture(
        &corpus_root.join("adversarial_deep_unbalanced.gc"),
        &root.join("cases"),
    );
    let malformed_patch = copy_fixture(
        &corpus_root.join("malformed_patch_schema.gcpatch"),
        &root.join("cases"),
    );
    let pkg_toml = copy_pkg_basic_fixture(&root.join("pkg_basic"));

    let cases: Vec<(&str, Vec<String>)> = vec![
        (
            "fmt/malformed-unterminated",
            vec![
                "fmt".to_string(),
                malformed_unterminated.display().to_string(),
            ],
        ),
        (
            "fmt/malformed-map-odd",
            vec!["fmt".to_string(), malformed_map_odd.display().to_string()],
        ),
        (
            "fmt/adversarial-deep-unbalanced",
            vec!["fmt".to_string(), adversarial_deep.display().to_string()],
        ),
        (
            "apply-patch/malformed-schema",
            vec![
                "apply-patch".to_string(),
                malformed_patch.display().to_string(),
                "--pkg".to_string(),
                pkg_toml.display().to_string(),
            ],
        ),
    ];

    for (name, argv) in cases {
        let (rust_code, rust_json) = run_case("rust", None, &argv, root);
        let (self_code, self_json) = run_case("selfhost", Some(&artifact), &argv, root);
        assert_eq!(
            rust_code, self_code,
            "case {name}: process exit code drift between rust and selfhost frontends"
        );
        assert_parity_failure_shape(name, &rust_json, &self_json);
    }
}
