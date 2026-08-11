use std::fs;
use std::path::Path;

use assert_cmd::cargo::cargo_bin_cmd;
use gc_coreform::{
    Term, TermOrdKey, canonicalize_module, hash_module, parse_module, parse_term, print_term,
};
use predicates::prelude::*;

mod support;

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

fn write_workspace_pkg_with_duplicate_symbol(dir: &Path) {
    fs::write(
        dir.join("package.toml"),
        r#"
name = "semantic-workspace-conflict-test"
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
(def my/pkg::foo (fn (_) 7))
(def my/pkg::use-foo (fn (_) my/pkg::foo))
"#,
    )
    .unwrap();
}

fn write_workspace_pkg_with_late_dependency(dir: &Path) {
    write_workspace_pkg(dir);
    fs::write(
        dir.join("a.gc"),
        r#"
(def my/pkg::helper 41)
(def my/pkg::foo my/pkg::helper)
(def my/pkg::tests
  {
    "t1" { :body (fn (_) my/pkg::foo) :expect 41 }
  })
"#,
    )
    .unwrap();
}

fn poison_refactor_plan_report(artifact: &Path) {
    let src = fs::read_to_string(artifact).expect("read toolchain artifact");
    let mut term = parse_term(&src).expect("parse toolchain artifact");
    let Term::Map(root) = &mut term else {
        panic!("artifact root must be map");
    };
    let modules = root
        .get_mut(&TermOrdKey(Term::symbol(":modules")))
        .expect("artifact :modules");
    let Term::Vector(entries) = modules else {
        panic!("artifact :modules must be vector");
    };
    let patch_mod = entries
        .iter_mut()
        .find_map(|entry| match entry {
            Term::Map(mm)
                if matches!(
                    mm.get(&TermOrdKey(Term::symbol(":path"))),
                    Some(Term::Str(path)) if path == "selfhost/patch_authority_refactor_plan_v1.gc"
                ) =>
            {
                Some(mm)
            }
            _ => None,
        })
        .expect("selfhost/patch_authority_refactor_plan_v1.gc entry");

    let module_src = match patch_mod.get(&TermOrdKey(Term::symbol(":source"))) {
        Some(Term::Str(src)) => src.clone(),
        _ => panic!("refactor plan module missing :source"),
    };
    let poisoned_src =
        format!("{module_src}\n(def core/cli::refactor-plan (fn (request) {{:ok true}}))\n");
    let poisoned_forms = canonicalize_module(parse_module(&poisoned_src).expect("parse poisoned"))
        .expect("canonicalize poisoned");
    let poisoned_hash = hash_module(&poisoned_forms);
    patch_mod.insert(TermOrdKey(Term::symbol(":source")), Term::Str(poisoned_src));
    patch_mod.insert(
        TermOrdKey(Term::symbol(":forms")),
        Term::Vector(poisoned_forms),
    );
    patch_mod.insert(
        TermOrdKey(Term::symbol(":module-h")),
        Term::Bytes(poisoned_hash.to_vec().into()),
    );
    fs::write(artifact, print_term(&term)).expect("write poisoned artifact");
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
    assert_eq!(
        envelope
            .pointer("/data/refactor_authority/name")
            .and_then(|v| v.as_str()),
        Some("selfhost")
    );
    assert_eq!(
        envelope
            .pointer("/data/refactor_authority/bootstrap_mode")
            .and_then(|v| v.as_str()),
        Some("artifact-only")
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

#[test]
fn semantic_edit_apply_plan_move_is_selfhosted_minimized_and_ordered() {
    let td = tempfile::tempdir().unwrap();
    let dir = td.path();
    write_workspace_pkg(dir);
    let artifact = support::copy_repo_toolchain_artifact(dir);

    let out = cargo_bin_cmd!("genesis")
        .current_dir(dir)
        .args([
            "--json",
            "--selfhost-artifact",
            artifact.to_str().unwrap(),
            "semantic-edit",
            "apply-plan",
            "--pkg",
            "package.toml",
            "--kind",
            "move",
            "--from",
            "my/pkg::foo",
            "--to",
            "my/pkg::moved",
            "--target-module-path",
            "moved.gc",
        ])
        .assert()
        .success()
        .get_output()
        .stdout
        .clone();

    let envelope: serde_json::Value = serde_json::from_slice(&out).unwrap();
    assert_eq!(
        envelope.get("ok").and_then(|value| value.as_bool()),
        Some(true)
    );
    assert_eq!(
        envelope
            .pointer("/data/plan/op_count")
            .and_then(|value| value.as_u64()),
        Some(4)
    );
    let patch = envelope
        .pointer("/data/plan/patch_coreform")
        .and_then(|value| value.as_str())
        .unwrap_or_default();
    assert_eq!(patch.matches(":rename-symbol").count(), 2);
    assert_eq!(patch.matches(":split-module").count(), 1);
    assert!(!patch.contains(":replace-node"));

    let (manifest, _) = gc_pkg::PackageManifest::load(&dir.join("package.toml")).unwrap();
    assert_eq!(manifest.modules[0].path, "moved.gc");
    assert_eq!(manifest.modules[1].path, "a.gc");
    assert!(
        fs::read_to_string(dir.join("moved.gc"))
            .unwrap()
            .contains("(def my/pkg::moved 41)")
    );
    let source = fs::read_to_string(dir.join("a.gc")).unwrap();
    assert!(!source.contains("(def my/pkg::foo"));
    assert!(source.contains("my/pkg::moved"));
}

#[test]
fn semantic_edit_refactor_plan_rejects_move_with_late_dependency() {
    let td = tempfile::tempdir().unwrap();
    let dir = td.path();
    write_workspace_pkg_with_late_dependency(dir);
    let artifact = support::copy_repo_toolchain_artifact(dir);

    let out = cargo_bin_cmd!("genesis_parity")
        .current_dir(dir)
        .args([
            "--json",
            "--coreform-frontend",
            "rust",
            "--selfhost-artifact",
            artifact.to_str().unwrap(),
            "semantic-edit",
            "refactor-plan",
            "--pkg",
            "package.toml",
            "--kind",
            "move",
            "--from",
            "my/pkg::foo",
            "--to",
            "my/pkg::moved",
            "--target-module-path",
            "moved.gc",
        ])
        .assert()
        .failure()
        .get_output()
        .stdout
        .clone();

    let envelope: serde_json::Value = serde_json::from_slice(&out).unwrap();
    let conflicts = envelope
        .pointer("/data/conflicts")
        .and_then(|value| value.as_array())
        .unwrap();
    assert!(conflicts.iter().any(|conflict| {
        conflict.get("code").and_then(|value| value.as_str())
            == Some("refactor/target-order-dependency")
    }));
    assert!(!dir.join("moved.gc").exists());
}

#[test]
fn semantic_edit_refactor_plan_reports_noop_conflict() {
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
            "my/pkg::foo",
        ])
        .assert()
        .failure()
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
        Some(false)
    );
    let conflicts = envelope
        .pointer("/data/conflicts")
        .and_then(|v| v.as_array())
        .cloned()
        .unwrap_or_default();
    assert!(
        conflicts
            .iter()
            .any(|c| c.get("code").and_then(|v| v.as_str()) == Some("refactor/no-op")),
        "expected no-op conflict from semantic-refactor-validate"
    );
}

#[test]
fn semantic_edit_refactor_plan_reports_destination_exists_conflict() {
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
            "my/pkg::use-foo",
        ])
        .assert()
        .failure()
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
        Some(false)
    );
    let conflicts = envelope
        .pointer("/data/conflicts")
        .and_then(|v| v.as_array())
        .cloned()
        .unwrap_or_default();
    assert!(
        conflicts.iter().any(|c| {
            c.get("code").and_then(|v| v.as_str()) == Some("refactor/destination-symbol-exists")
        }),
        "expected destination-exists conflict from semantic-refactor-plan-conflicts"
    );
}

#[test]
fn semantic_edit_refactor_plan_move_requires_target_module_path_conflict() {
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
            "move",
            "--from",
            "my/pkg::foo",
            "--to",
            "my/pkg::foo_v2",
        ])
        .assert()
        .failure()
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
        Some(false)
    );
    let conflicts = envelope
        .pointer("/data/conflicts")
        .and_then(|v| v.as_array())
        .cloned()
        .unwrap_or_default();
    assert!(
        conflicts
            .iter()
            .any(|c| c.get("code").and_then(|v| v.as_str())
                == Some("refactor/target-module-required")),
        "expected target-module-required conflict from semantic-refactor-target-conflicts"
    );
}

#[test]
fn semantic_edit_refactor_plan_move_reports_target_module_exists_conflict() {
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
            "move",
            "--from",
            "my/pkg::foo",
            "--to",
            "my/pkg::foo_v2",
            "--target-module-path",
            "b.gc",
        ])
        .assert()
        .failure()
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
        Some(false)
    );
    let conflicts = envelope
        .pointer("/data/conflicts")
        .and_then(|v| v.as_array())
        .cloned()
        .unwrap_or_default();
    assert!(
        conflicts.iter().any(|c| {
            c.get("code").and_then(|v| v.as_str()) == Some("refactor/target-module-exists")
        }),
        "expected target-module-exists conflict from semantic-refactor-target-conflicts"
    );
}

#[test]
fn semantic_edit_refactor_plan_move_reports_target_module_invalid_conflict() {
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
            "move",
            "--from",
            "my/pkg::foo",
            "--to",
            "my/pkg::foo_v2",
            "--target-module-path",
            "../b.gc",
        ])
        .assert()
        .failure()
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
        Some(false)
    );
    let conflicts = envelope
        .pointer("/data/conflicts")
        .and_then(|v| v.as_array())
        .cloned()
        .unwrap_or_default();
    assert!(
        conflicts.iter().any(|c| {
            c.get("code").and_then(|v| v.as_str()) == Some("refactor/target-module-invalid")
        }),
        "expected target-module-invalid conflict from semantic-refactor-target-conflicts"
    );
}

#[test]
fn semantic_edit_refactor_plan_rejects_incomplete_selfhost_report() {
    let td = tempfile::tempdir().unwrap();
    let dir = td.path();
    write_workspace_pkg(dir);
    let artifact = support::copy_repo_toolchain_artifact(dir);
    poison_refactor_plan_report(&artifact);

    cargo_bin_cmd!("genesis_parity")
        .current_dir(dir)
        .args([
            "--coreform-frontend",
            "selfhost",
            "--selfhost-artifact",
            artifact.to_str().unwrap(),
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
        .failure()
        .code(10)
        .stderr(predicate::str::contains("must contain exactly fields"));
}

#[test]
fn semantic_edit_apply_plan_rename_is_deterministic_on_reapply_conflict() {
    let td = tempfile::tempdir().unwrap();
    let dir = td.path();
    write_workspace_pkg(dir);

    let apply_once = || {
        cargo_bin_cmd!("genesis_parity")
            .current_dir(dir)
            .args([
                "--json",
                "--coreform-frontend",
                "rust",
                "semantic-edit",
                "apply-plan",
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
            .clone()
    };

    let first = apply_once();
    let first_env: serde_json::Value = serde_json::from_slice(&first).unwrap();
    assert_eq!(
        first_env.get("kind").and_then(|x| x.as_str()),
        Some("genesis/semantic-edit-apply-plan-v0.1")
    );
    assert_eq!(first_env.get("ok").and_then(|x| x.as_bool()), Some(true));
    assert_eq!(
        first_env
            .pointer("/data/apply_status")
            .and_then(|v| v.as_str()),
        Some("applied")
    );

    let src_a = fs::read_to_string(dir.join("a.gc")).unwrap();
    let src_b = fs::read_to_string(dir.join("b.gc")).unwrap();
    assert!(src_a.contains("my/pkg::foo_v2"));
    assert!(src_b.contains("my/pkg::foo_v2"));
    assert!(!src_a.contains("my/pkg::foo "));
    assert!(!src_b.contains("my/pkg::foo "));

    let reapply = || {
        cargo_bin_cmd!("genesis_parity")
            .current_dir(dir)
            .args([
                "--json",
                "--coreform-frontend",
                "rust",
                "semantic-edit",
                "apply-plan",
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
            .failure()
            .get_output()
            .stdout
            .clone()
    };

    let reapply_a = reapply();
    let reapply_b = reapply();
    assert_eq!(
        reapply_a, reapply_b,
        "reapply conflict diagnostics must be deterministic"
    );

    let reapply_env: serde_json::Value = serde_json::from_slice(&reapply_a).unwrap();
    assert_eq!(
        reapply_env.get("kind").and_then(|x| x.as_str()),
        Some("genesis/semantic-edit-apply-plan-v0.1")
    );
    assert_eq!(reapply_env.get("ok").and_then(|x| x.as_bool()), Some(false));
    assert_eq!(
        reapply_env
            .pointer("/data/apply_status")
            .and_then(|v| v.as_str()),
        Some("plan-conflicts")
    );
    let conflicts = reapply_env
        .pointer("/data/conflicts")
        .and_then(|v| v.as_array())
        .cloned()
        .unwrap_or_default();
    assert!(
        conflicts.iter().any(|c| {
            c.get("code").and_then(|v| v.as_str()) == Some("refactor/source-symbol-missing")
        }),
        "reapply should deterministically report source missing conflict"
    );
}

#[test]
fn semantic_edit_apply_plan_reports_workspace_ambiguous_definition_conflict() {
    let td = tempfile::tempdir().unwrap();
    let dir = td.path();
    write_workspace_pkg_with_duplicate_symbol(dir);

    let out = cargo_bin_cmd!("genesis_parity")
        .current_dir(dir)
        .args([
            "--json",
            "--coreform-frontend",
            "rust",
            "semantic-edit",
            "apply-plan",
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
        .failure()
        .get_output()
        .stdout
        .clone();

    let envelope: serde_json::Value = serde_json::from_slice(&out).unwrap();
    assert_eq!(
        envelope.get("kind").and_then(|x| x.as_str()),
        Some("genesis/semantic-edit-apply-plan-v0.1")
    );
    assert_eq!(envelope.get("ok").and_then(|x| x.as_bool()), Some(false));
    let conflicts = envelope
        .pointer("/data/conflicts")
        .and_then(|v| v.as_array())
        .cloned()
        .unwrap_or_default();
    assert!(
        conflicts.iter().any(|c| {
            c.get("code").and_then(|v| v.as_str()) == Some("refactor/source-symbol-ambiguous")
        }),
        "workspace conflict set must include source ambiguity diagnostics"
    );
}
