use assert_cmd::cargo::cargo_bin_cmd;
use tempfile::tempdir;

fn build_selfhost_artifact(dir: &std::path::Path) -> std::path::PathBuf {
    let artifact = dir.join("selfhost_toolchain.gc");
    cargo_bin_cmd!("genesis")
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

fn run_stdout(args: &[&str]) -> String {
    let out = cargo_bin_cmd!("genesis")
        .args(args)
        .assert()
        .success()
        .get_output()
        .stdout
        .clone();
    String::from_utf8(out).unwrap().trim().to_string()
}

#[test]
fn pack_selfhost_frontend_matches_rust_frontend_artifact() {
    let td = tempdir().unwrap();
    let artifact = build_selfhost_artifact(td.path());
    let rust_pkg = copy_pkg_basic_fixture(&td.path().join("pkg_rust"));
    let self_pkg = copy_pkg_basic_fixture(&td.path().join("pkg_selfhost"));

    let rust_h = run_stdout(&["pack", "--pkg", rust_pkg.to_str().unwrap()]);
    let self_h = run_stdout(&[
        "--selfhost-artifact",
        artifact.to_str().unwrap(),
        "pack",
        "--pkg",
        self_pkg.to_str().unwrap(),
    ]);

    assert_eq!(rust_h, self_h);
}

#[test]
fn test_selfhost_frontend_matches_rust_frontend_acceptance_artifact() {
    let td = tempdir().unwrap();
    let artifact = build_selfhost_artifact(td.path());
    let rust_pkg = copy_pkg_basic_fixture(&td.path().join("pkg_rust"));
    let self_pkg = copy_pkg_basic_fixture(&td.path().join("pkg_selfhost"));

    let rust_h = run_stdout(&["test", "--pkg", rust_pkg.to_str().unwrap()]);
    let self_h = run_stdout(&[
        "--selfhost-artifact",
        artifact.to_str().unwrap(),
        "test",
        "--pkg",
        self_pkg.to_str().unwrap(),
    ]);

    assert_eq!(rust_h, self_h);
}
