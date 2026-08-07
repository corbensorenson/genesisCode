use std::fs;
use std::path::{Path, PathBuf};

fn repo_root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../..")
        .canonicalize()
        .expect("canonicalize repo root")
}

#[test]
fn host_bridge_fault_renderer_does_not_require_ripgrep() {
    let renderer = fs::read_to_string(
        repo_root().join("scripts/render_host_bridge_fault_injection_report.sh"),
    )
    .expect("read host-bridge fault renderer");

    for required in [
        "grep -En 'static SESSIONS|persistent_bridge_session_map'",
        "grep -Eq 'struct HostBridgeRuntime'",
        "grep -Eq 'bridge_runtime: &mut HostBridgeRuntime'",
    ] {
        assert!(
            renderer.contains(required),
            "missing portable check: {required}"
        );
    }
    assert!(
        !renderer.lines().any(|line| {
            let command = line.trim_start();
            command.starts_with("rg ") || command.starts_with("if rg ")
        }),
        "host-bridge renderer must not require optional ripgrep"
    );
}
