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

fn write_workspace_pkg(dir: &Path) {
    fs::write(
        dir.join("package.toml"),
        r#"
name = "semantic-workspace-test"
version = "0.0.1"
dependencies = []
obligations = ["core/obligation::unit-tests"]
modules = [
  { path = "a.gc", hash = "" },
  { path = "b.gc", hash = "" }
]
tests = ["my/pkg::tests"]
"#,
    )
    .unwrap();
    fs::write(
        dir.join("a.gc"),
        r#"
(def my/pkg::foo 41)

(def my/pkg::tests
  {
    "t1" { :body (fn (_) my/pkg::foo) :expect 41 }
  })
"#,
    )
    .unwrap();
    fs::write(
        dir.join("b.gc"),
        r#"
(def my/pkg::use-foo (fn (_) my/pkg::foo))
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
        cargo_bin_cmd!("genesis_parity")
            .current_dir(dir)
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

#[test]
fn semantic_edit_workspace_graph_is_deterministic_with_dependency_edges() {
    let td = tempfile::tempdir().unwrap();
    let dir = td.path();
    write_workspace_pkg(dir);

    let run_once = || {
        cargo_bin_cmd!("genesis_parity")
            .current_dir(dir)
            .args([
                "--json",
                "--coreform-frontend",
                "rust",
                "semantic-edit",
                "workspace-graph",
                "--pkg",
                "package.toml",
            ])
            .assert()
            .success()
            .get_output()
            .stdout
            .clone()
    };

    let a = run_once();
    let b = run_once();
    assert_eq!(a, b, "workspace graph output must be deterministic");

    let envelope: serde_json::Value = serde_json::from_slice(&a).unwrap();
    assert_eq!(
        envelope.get("kind").and_then(|x| x.as_str()),
        Some("genesis/semantic-edit-workspace-graph-v0.1")
    );
    let edge_count = envelope
        .pointer("/data/edge_count")
        .and_then(|v| v.as_u64())
        .unwrap_or(0);
    assert!(
        edge_count > 0,
        "workspace graph should contain dependency edges"
    );
}

#[test]
fn semantic_edit_refactor_plan_rename_emits_multifile_patch() {
    let td = tempfile::tempdir().unwrap();
    let dir = td.path();
    write_workspace_pkg(dir);

    let out = cargo_bin_cmd!("genesis_parity")
        .current_dir(dir)
        .args([
            "--json",
            "--coreform-frontend",
            "rust",
            "semantic-edit",
            "refactor-plan",
            "--pkg",
            "package.toml",
            "--kind",
            "rename",
            "--from",
            "my/pkg::foo",
            "--to",
            "my/pkg::foo_v2",
        ])
        .assert()
        .success()
        .get_output()
        .stdout
        .clone();

    let envelope: serde_json::Value = serde_json::from_slice(&out).unwrap();
    assert_eq!(
        envelope.get("kind").and_then(|x| x.as_str()),
        Some("genesis/semantic-edit-refactor-plan-v0.1")
    );
    assert_eq!(
        envelope
            .pointer("/data/safe_to_apply")
            .and_then(|v| v.as_bool()),
        Some(true)
    );
    let patch = envelope
        .pointer("/data/patch_coreform")
        .and_then(|v| v.as_str())
        .unwrap_or_default();
    assert!(
        patch.contains("my/pkg::foo_v2"),
        "refactor patch should target destination symbol"
    );
    assert!(
        patch.contains("a.gc") && patch.contains("b.gc"),
        "refactor patch should contain multi-file edits"
    );
}
