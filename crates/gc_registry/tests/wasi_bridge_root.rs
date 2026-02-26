fn make_temp_dir(prefix: &str) -> std::path::PathBuf {
    let mut dir = std::env::temp_dir();
    let nanos = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .expect("clock drift")
        .as_nanos();
    dir.push(format!(
        "genesis-gc-registry-{prefix}-{}-{nanos}",
        std::process::id()
    ));
    std::fs::create_dir_all(&dir).expect("mkdir temp test dir");
    dir
}

#[test]
fn wasi_bridge_root_resolution_scopes_http_remote_by_host_and_port() {
    let test_root = make_temp_dir("wasi-bridge-scope");
    let bridge_root = test_root.join("wasi-http-bridge");
    std::fs::create_dir_all(&bridge_root).expect("mkdir");

    let resolved = gc_registry::wasi_http_bridge_resolve_remote_root(
        &bridge_root,
        "https://registry.example.com/",
    )
    .expect("resolve bridge root");
    let expected = bridge_root
        .join("https")
        .join("registry.example.com_443")
        .join("v1");
    assert_eq!(resolved, expected);
    std::fs::remove_dir_all(&test_root).expect("cleanup");
}

#[test]
fn wasi_bridge_root_resolution_preserves_v1_root() {
    let test_root = make_temp_dir("wasi-bridge-v1");
    let bridge_root = test_root.join("v1");
    std::fs::create_dir_all(&bridge_root).expect("mkdir");

    let resolved =
        gc_registry::wasi_http_bridge_resolve_remote_root(&bridge_root, "http://127.0.0.1:18181/")
            .expect("resolve bridge root");
    assert_eq!(resolved, bridge_root);
    std::fs::remove_dir_all(&test_root).expect("cleanup");
}
