use std::fs;
use std::path::{Path, PathBuf};

use gc_coreform::print_term;

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
