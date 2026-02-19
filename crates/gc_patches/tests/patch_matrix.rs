use std::fs;
use std::path::{Path, PathBuf};

use gc_coreform::print_term;
use gc_kernel::{MemLimits, StepLimit};

fn write_pkg(dir: &Path) -> PathBuf {
    fs::create_dir_all(dir).unwrap();

    let module = dir.join("mod.gc");
    fs::write(
        &module,
        r#"
          (def my/pkg::tests
            {
              "t1" { :body (fn (_) 1) :expect 1 }
            })
        "#,
    )
    .unwrap();

    let pkg = dir.join("package.toml");
    fs::write(
        &pkg,
        r#"
name = "patch-matrix"
version = "0.0.0"
modules = [{ path = "mod.gc", hash = "" }]
dependencies = []
obligations = ["core/obligation::unit-tests"]
tests = ["my/pkg::tests"]
"#,
    )
    .unwrap();
    pkg
}

fn write_patch(dir: &Path, term_src: &str) -> PathBuf {
    let p = dir.join("p.gcpatch");
    fs::write(&p, term_src).unwrap();
    p
}

fn patch_replace_form0(new_form_src: &str) -> String {
    // Replace the single top-level form ([:form 0]) with the provided CoreForm list.
    format!(
        r#"
          {{
            :version 1
            :intent "replace form"
            :provenance {{}}
            :ops [
              {{
                :op :replace-node
                :module-path "mod.gc"
                :path [[:form 0]]
                :new {new_form_src}
              }}
            ]
          }}
        "#
    )
}

fn semantic_node_index_for_mod(src: &str) -> Vec<gc_patches::SemanticNodeRecord> {
    gc_patches::semantic_node_index_for_module_with_frontend(
        "mod.gc",
        src,
        &gc_obligations::default_coreform_frontend(),
        StepLimit::Default,
        MemLimits::default(),
    )
    .unwrap()
}

#[test]
fn patch_schema_missing_ops_is_rejected() {
    let td = tempfile::tempdir().unwrap();
    let pkg = write_pkg(td.path());
    let patch = write_patch(td.path(), r#"{ :version 1 :intent "bad" :provenance {} }"#);

    let err = gc_patches::apply_patch(&patch, &pkg, None).unwrap_err();
    assert!(
        matches!(err, gc_patches::PatchError::Validate(_)),
        "expected Validate error, got {err}"
    );
}

#[test]
fn patch_replace_node_requires_path_starting_with_form() {
    let td = tempfile::tempdir().unwrap();
    let pkg = write_pkg(td.path());
    let patch = write_patch(
        td.path(),
        r#"
          {
            :version 1
            :intent "bad path"
            :provenance {}
            :ops [
              {
                :op :replace-node
                :module-path "mod.gc"
                :path [[:vec 0]]
                :new 123
              }
            ]
          }
        "#,
    );

    let err = gc_patches::apply_patch(&patch, &pkg, None).unwrap_err();
    assert!(
        matches!(err, gc_patches::PatchError::Validate(_)),
        "expected Validate error, got {err}"
    );
}

#[test]
fn patch_obligation_rerun_failure_is_reported_ok_false() {
    let td = tempfile::tempdir().unwrap();
    let pkg = write_pkg(td.path());

    // Ensure the package can be packed (pins module hashes).
    let _ = gc_obligations::pack(&pkg).unwrap();

    // Patch changes the test to expect 2, but the body still returns 1.
    let new_form = r#"(def my/pkg::tests { "t1" { :body (fn (_) 1) :expect 2 } })"#;
    let patch_src = patch_replace_form0(new_form);
    let patch_term = gc_coreform::parse_term(&patch_src).unwrap();
    let patch_path = td.path().join("break.gcpatch");
    fs::write(&patch_path, print_term(&patch_term)).unwrap();

    let r = gc_patches::apply_patch(&patch_path, &pkg, None).unwrap();
    assert!(!r.ok, "expected obligations to fail after patch");
    assert!(r.acceptance_artifact.is_some());
    assert!(r.package_artifact.is_some());
    assert!(!r.report_artifact.is_empty());
}

#[test]
fn semantic_node_index_is_deterministic_for_same_module_source() {
    let td = tempfile::tempdir().unwrap();
    let _pkg = write_pkg(td.path());
    let src = fs::read_to_string(td.path().join("mod.gc")).unwrap();

    let a = semantic_node_index_for_mod(&src);
    let b = semantic_node_index_for_mod(&src);

    assert_eq!(a, b);
    assert!(!a.is_empty(), "node index should not be empty");
}

#[test]
fn patch_replace_node_id_applies_and_reports_semantic_edit() {
    let td = tempfile::tempdir().unwrap();
    let pkg = write_pkg(td.path());
    let _ = gc_obligations::pack(&pkg).unwrap();
    let src = fs::read_to_string(td.path().join("mod.gc")).unwrap();
    let nodes = semantic_node_index_for_mod(&src);
    let expect_node = nodes
        .iter()
        .find(|n| n.term_tag == "int" && n.path_repr.contains(":expect"))
        .expect("expected :expect int node");
    let patch = write_patch(
        td.path(),
        &format!(
            r#"
          {{
            :version 1
            :intent "replace via node-id"
            :provenance {{}}
            :ops [
              {{
                :op :replace-node-id
                :module-path "mod.gc"
                :node-id "{node_id}"
                :new 2
              }}
            ]
          }}
        "#,
            node_id = expect_node.node_id
        ),
    );

    let r = gc_patches::apply_patch(&patch, &pkg, None).unwrap();
    assert!(!r.ok, "changing :expect should fail obligations");

    let report_src = fs::read_to_string(td.path().join(".genesis/store").join(&r.report_artifact))
        .expect("read report artifact");
    assert!(
        report_src.contains(":semantic-edits"),
        "report should include semantic edit provenance"
    );
}

#[test]
fn patch_replace_node_id_rejects_unknown_node_id() {
    let td = tempfile::tempdir().unwrap();
    let pkg = write_pkg(td.path());
    let patch = write_patch(
        td.path(),
        r#"
          {
            :version 1
            :intent "bad node id"
            :provenance {}
            :ops [
              {
                :op :replace-node-id
                :module-path "mod.gc"
                :node-id "deadbeef"
                :new 123
              }
            ]
          }
        "#,
    );
    let err = gc_patches::apply_patch(&patch, &pkg, None).unwrap_err();
    assert!(matches!(err, gc_patches::PatchError::Validate(_)));
}
