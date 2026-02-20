use assert_cmd::cargo::cargo_bin_cmd;

#[test]
fn registry_serve_supports_zero_request_smoke_run() {
    let td = tempfile::tempdir().unwrap();
    cargo_bin_cmd!("genesis")
        .current_dir(td.path())
        .args([
            "registry",
            "serve",
            "--root",
            td.path().to_str().unwrap(),
            "--addr",
            "127.0.0.1:0",
            "--max-requests",
            "0",
        ])
        .assert()
        .success();
}
