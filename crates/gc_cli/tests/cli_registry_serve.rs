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

#[cfg(not(target_os = "wasi"))]
struct OwnedChild(Option<std::process::Child>);
#[cfg(not(target_os = "wasi"))]
impl Drop for OwnedChild {
    fn drop(&mut self) {
        if let Some(child) = &mut self.0 {
            let _ = child.kill();
            let _ = child.wait();
        }
    }
}
#[cfg(not(target_os = "wasi"))]
fn spawn_listener(root: &std::path::Path, max: Option<u64>) -> (OwnedChild, String) {
    let reserved = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
    let address = reserved.local_addr().unwrap().to_string();
    drop(reserved);
    let mut command = std::process::Command::new(env!("CARGO_BIN_EXE_genesis"));
    command.current_dir(root).args([
        "--json",
        "registry",
        "serve",
        "--root",
        root.to_str().unwrap(),
        "--addr",
        &address,
    ]);
    if let Some(max) = max {
        command.args(["--max-requests", &max.to_string()]);
    }
    command
        .stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::piped());
    (
        OwnedChild(Some(command.spawn().unwrap())),
        format!("http://{address}"),
    )
}
#[cfg(not(target_os = "wasi"))]
fn ready(owner: &mut OwnedChild, url: &str) -> gc_registry::RegistryClient {
    let client =
        gc_registry::RegistryClient::new(url, Some(std::time::Duration::from_secs(1))).unwrap();
    let deadline = std::time::Instant::now() + std::time::Duration::from_secs(10);
    loop {
        assert!(
            owner.0.as_mut().unwrap().try_wait().unwrap().is_none(),
            "serve exited before a real request"
        );
        if client.ping().is_ok() {
            return client;
        }
        assert!(
            std::time::Instant::now() < deadline,
            "listener readiness watchdog expired"
        );
        std::thread::sleep(std::time::Duration::from_millis(10));
    }
}
#[cfg(not(target_os = "wasi"))]
fn reaped(owner: &mut OwnedChild) -> std::process::ExitStatus {
    let deadline = std::time::Instant::now() + std::time::Duration::from_secs(5);
    loop {
        if let Some(status) = owner.0.as_mut().unwrap().try_wait().unwrap() {
            return status;
        }
        assert!(
            std::time::Instant::now() < deadline,
            "server completion watchdog expired"
        );
        std::thread::sleep(std::time::Duration::from_millis(10));
    }
}
#[cfg(not(target_os = "wasi"))]
#[test]
fn unlimited_cli_stays_alive_serves_and_is_reaped_by_owner_stop() {
    let root = tempfile::tempdir().unwrap();
    let (mut child, url) = spawn_listener(root.path(), None);
    let client = ready(&mut child, &url);
    let bytes = b"cli-service-liveness";
    let hash = blake3::hash(bytes).to_hex().to_string();
    client.store_put(&hash, bytes).unwrap();
    assert_eq!(
        std::fs::read(root.path().join("v1/store").join(&hash)).unwrap(),
        bytes
    );
    assert!(child.0.as_mut().unwrap().try_wait().unwrap().is_none());
    child.0.as_mut().unwrap().kill().unwrap();
    assert!(!reaped(&mut child).success());
    assert!(client.ping().is_err());
}
#[cfg(not(target_os = "wasi"))]
#[test]
fn cli_request_policy_completes_after_ping_and_store_commit() {
    let root = tempfile::tempdir().unwrap();
    let (mut child, url) = spawn_listener(root.path(), Some(2));
    let client = ready(&mut child, &url);
    let bytes = b"cli-policy-completion";
    let hash = blake3::hash(bytes).to_hex().to_string();
    client.store_put(&hash, bytes).unwrap();
    assert_eq!(
        std::fs::read(root.path().join("v1/store").join(&hash)).unwrap(),
        bytes
    );
    assert!(reaped(&mut child).success());
    let output = child.0.take().unwrap().wait_with_output().unwrap();
    let envelope: serde_json::Value = serde_json::from_slice(&output.stdout).unwrap();
    assert_eq!(envelope["data"]["status"], "stopped");
    assert_eq!(envelope["data"]["max_requests"], 2);
    assert!(client.ping().is_err());
}
