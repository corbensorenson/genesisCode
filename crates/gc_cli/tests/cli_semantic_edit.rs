use std::fs;
use std::path::Path;

use assert_cmd::cargo::cargo_bin_cmd;

fn write_pkg(dir: &Path) {
    fs::write(
        dir.join("package.toml"),
        r#"
name = "semantic-edit-test"
version = "0.0.1"
dependencies = []
obligations = ["core/obligation::unit-tests"]
modules = [{ path = "mod.gc", hash = "" }]
tests = ["my/pkg::tests"]
"#,
    )
    .unwrap();
    fs::write(
        dir.join("mod.gc"),
        r#"
(def my/pkg::tests
  {
    "t1" { :body (fn (_) 1) :expect 1 }
  })
"#,
    )
    .unwrap();
}

#[test]
fn semantic_edit_index_emits_stable_node_inventory() {
    let td = tempfile::tempdir().unwrap();
    let dir = td.path();
    write_pkg(dir);

    let run_once = || {
        cargo_bin_cmd!("genesis")
            .current_dir(dir)
            .env("GENESIS_ALLOW_RUST_ENGINE", "1")
            .args([
                "--json",
                "--coreform-frontend",
                "rust",
                "semantic-edit",
                "index",
                "--pkg",
                "package.toml",
                "--module-path",
                "mod.gc",
            ])
            .assert()
            .success()
            .get_output()
            .stdout
            .clone()
    };

    let a = run_once();
    let b = run_once();
    assert_eq!(a, b, "semantic node index must be deterministic");

    let envelope: serde_json::Value = serde_json::from_slice(&a).unwrap();
    assert_eq!(
        envelope.get("kind").and_then(|x| x.as_str()),
        Some("genesis/semantic-edit-index-v0.1")
    );
    let node_count = envelope
        .pointer("/data/node_count")
        .and_then(|v| v.as_u64())
        .unwrap_or(0);
    assert!(node_count > 0, "semantic index should contain nodes");
    let nodes = envelope
        .pointer("/data/nodes")
        .and_then(|v| v.as_array())
        .cloned()
        .unwrap_or_default();
    assert!(!nodes.is_empty(), "semantic index nodes must be present");
    let first = nodes.first().expect("first node");
    assert!(
        first.get("node_id").and_then(|v| v.as_str()).is_some(),
        "node entry should contain node_id"
    );
    assert!(
        first.get("path_repr").and_then(|v| v.as_str()).is_some(),
        "node entry should contain path_repr"
    );
}
