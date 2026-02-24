use assert_cmd::cargo::cargo_bin_cmd;

fn cmd() -> assert_cmd::Command {
    cargo_bin_cmd!("genesis_wasi_parity")
}

#[test]
fn registry_serve_wasi_bootstraps_file_contract_remote() {
    let td = tempfile::tempdir().unwrap();
    let root = td.path().join("registry-root");

    let out = cmd()
        .current_dir(td.path())
        .arg("--json")
        .args([
            "registry",
            "serve",
            "--root",
            root.to_str().unwrap(),
            "--addr",
            "127.0.0.1:0",
            "--max-chunk-bytes",
            "4096",
            "--max-requests",
            "0",
        ])
        .assert()
        .success()
        .get_output()
        .stdout
        .clone();

    let env: serde_json::Value = serde_json::from_slice(&out).unwrap();
    assert_eq!(
        env.get("kind").and_then(|x| x.as_str()),
        Some("genesis/registry-serve-v0.1")
    );
    let data = env.get("data").expect("data envelope");
    assert_eq!(
        data.get("mode").and_then(|x| x.as_str()),
        Some("wasi-file-contract")
    );
    assert_eq!(data.get("status").and_then(|x| x.as_str()), Some("ready"));
    assert_eq!(
        data.get("max_chunk_bytes").and_then(|x| x.as_u64()),
        Some(4096)
    );
    assert_eq!(data.get("max_requests").and_then(|x| x.as_u64()), Some(0));
    assert!(root.join("v1").join("store").is_dir());

    let remote = data.get("remote").and_then(|x| x.as_str()).expect("remote");
    assert!(
        remote.starts_with("file://"),
        "unexpected remote scheme: {remote}"
    );

    let client = gc_registry::RegistryClient::new(remote, None).expect("registry client");
    let ping = client.ping().expect("registry ping");
    assert!(ping.ok);
    assert_eq!(ping.version, "0.1");
    assert_eq!(ping.hash, "blake3-256");
}
