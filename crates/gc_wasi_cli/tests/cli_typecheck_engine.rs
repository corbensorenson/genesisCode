use assert_cmd::cargo::cargo_bin_cmd;
use serde_json::Value as JsonValue;
use tempfile::tempdir;

fn cmd() -> assert_cmd::Command {
    let mut c = cargo_bin_cmd!("genesis_wasi");
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
    for name in ["basic.gc", "caps.toml", "package.toml"] {
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

    let rust_v = run_json(&["--json", "typecheck", "--pkg", rust_pkg.to_str().unwrap()]);
    let self_v = run_json(&[
        "--json",
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
