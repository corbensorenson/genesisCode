use assert_cmd::cargo::cargo_bin_cmd;
use tempfile::tempdir;

fn run_ok(args: &[&str]) {
    cargo_bin_cmd!("genesis_wasi").args(args).assert().success();
}

fn build_selfhost_artifact(dir: &std::path::Path) -> std::path::PathBuf {
    let artifact = dir.join("selfhost_toolchain.gc");
    cargo_bin_cmd!("genesis_wasi")
        .args(["selfhost-artifact", "--out"])
        .arg(&artifact)
        .assert()
        .success();
    artifact
}

#[test]
fn fmt_selfhost_engine_matches_rust_engine_output() {
    let dir = tempdir().unwrap();
    let file = dir.path().join("m.gc");
    let artifact = build_selfhost_artifact(dir.path());

    let src = r#"
      ; intentionally messy spacing
      (def  x   1)
      (def y (prim int/add x 2))
      (  y   )
    "#;
    std::fs::write(&file, src).unwrap();

    run_ok(&["fmt", file.to_str().unwrap(), "--engine", "rust"]);
    let rust_out = std::fs::read_to_string(&file).unwrap();

    std::fs::write(&file, src).unwrap();
    run_ok(&[
        "--no-step-limit",
        "--selfhost-artifact",
        artifact.to_str().unwrap(),
        "fmt",
        file.to_str().unwrap(),
        "--engine",
        "selfhost",
    ]);
    let selfhost_out = std::fs::read_to_string(&file).unwrap();

    assert_eq!(rust_out, selfhost_out);
}

#[test]
fn fmt_selfhost_check_agrees_with_rust_check() {
    let dir = tempdir().unwrap();
    let file = dir.path().join("m.gc");
    let artifact = build_selfhost_artifact(dir.path());

    let noncanon = "(def x 1)   (def y 2)\n";
    std::fs::write(&file, noncanon).unwrap();

    cargo_bin_cmd!("genesis_wasi")
        .args(["fmt", file.to_str().unwrap(), "--check", "--engine", "rust"])
        .assert()
        .failure()
        .code(11);

    cargo_bin_cmd!("genesis_wasi")
        .args([
            "--no-step-limit",
            "--selfhost-artifact",
            artifact.to_str().unwrap(),
            "fmt",
            file.to_str().unwrap(),
            "--check",
            "--engine",
            "selfhost",
        ])
        .assert()
        .failure()
        .code(11);
}
