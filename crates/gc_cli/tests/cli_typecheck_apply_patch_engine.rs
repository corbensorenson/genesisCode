use assert_cmd::cargo::cargo_bin_cmd;
use serde_json::Value as JsonValue;
use tempfile::tempdir;

fn cmd() -> assert_cmd::Command {
    let mut c = cargo_bin_cmd!("genesis");
    c.env("GENESIS_ALLOW_RUST_ENGINE", "1");
    c
}

fn build_selfhost_artifact(dir: &std::path::Path) -> std::path::PathBuf {
    let artifact = dir.join("selfhost_toolchain.gc");
    cmd()
        .args(["selfhost-artifact", "--out"])
        .arg(&artifact)
        .assert()
        .success();
    artifact
}

fn copy_pkg_basic_fixture(dst: &std::path::Path) -> std::path::PathBuf {
    std::fs::create_dir_all(dst).unwrap();
    let fixture = std::path::Path::new(concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/../../tests/spec/pkg_basic"
    ));
    for name in ["basic.gc", "caps.toml", "package.toml", "pure.gcpatch"] {
        std::fs::copy(fixture.join(name), dst.join(name)).unwrap();
    }
    dst.join("package.toml")
}

fn run_json(args: &[&str]) -> JsonValue {
    let out = cmd()
        .args(args)
        .assert()
        .success()
        .get_output()
        .stdout
        .clone();
    serde_json::from_slice(&out).unwrap()
}

#[test]
fn typecheck_selfhost_frontend_matches_rust_frontend_report() {
    let td = tempdir().unwrap();
    let artifact = build_selfhost_artifact(td.path());
    let rust_pkg = copy_pkg_basic_fixture(&td.path().join("pkg_rust"));
    let self_pkg = copy_pkg_basic_fixture(&td.path().join("pkg_selfhost"));

    let rust_v = run_json(&[
        "--json",
        "--coreform-frontend",
        "rust",
        "typecheck",
        "--pkg",
        rust_pkg.to_str().unwrap(),
    ]);
    let self_v = run_json(&[
        "--json",
        "--coreform-frontend",
        "selfhost",
        "--selfhost-artifact",
        artifact.to_str().unwrap(),
        "typecheck",
        "--pkg",
        self_pkg.to_str().unwrap(),
    ]);

    let rust_report = rust_v
        .get("data")
        .and_then(|d| d.get("report_coreform"))
        .and_then(JsonValue::as_str)
        .unwrap();
    let self_report = self_v
        .get("data")
        .and_then(|d| d.get("report_coreform"))
        .and_then(JsonValue::as_str)
        .unwrap();
    assert_eq!(rust_report, self_report);
}

#[test]
fn apply_patch_selfhost_frontend_matches_rust_frontend_artifacts() {
    let td = tempdir().unwrap();
    let artifact = build_selfhost_artifact(td.path());
    let rust_dir = td.path().join("pkg_rust");
    let self_dir = td.path().join("pkg_selfhost");
    let rust_pkg = copy_pkg_basic_fixture(&rust_dir);
    let self_pkg = copy_pkg_basic_fixture(&self_dir);
    let rust_patch = rust_dir.join("pure.gcpatch");
    let self_patch = self_dir.join("pure.gcpatch");

    let rust_v = run_json(&[
        "--json",
        "--coreform-frontend",
        "rust",
        "apply-patch",
        rust_patch.to_str().unwrap(),
        "--pkg",
        rust_pkg.to_str().unwrap(),
    ]);
    let self_v = run_json(&[
        "--json",
        "--coreform-frontend",
        "selfhost",
        "--selfhost-artifact",
        artifact.to_str().unwrap(),
        "apply-patch",
        self_patch.to_str().unwrap(),
        "--pkg",
        self_pkg.to_str().unwrap(),
    ]);

    let rust_data = rust_v.get("data").unwrap();
    let self_data = self_v.get("data").unwrap();
    for key in [
        "patch_artifact",
        "report_artifact",
        "acceptance_artifact",
        "package_artifact",
    ] {
        assert_eq!(
            rust_data.get(key).and_then(JsonValue::as_str),
            self_data.get(key).and_then(JsonValue::as_str),
            "engine mismatch for {key}"
        );
    }
}
