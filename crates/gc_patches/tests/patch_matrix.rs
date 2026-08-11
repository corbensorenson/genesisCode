use std::fs;
use std::path::{Path, PathBuf};

use gc_coreform::{
    Term, TermOrdKey, canonicalize_module, hash_module, parse_module, parse_term, print_term,
};
use gc_kernel::{MemLimits, StepLimit};

#[path = "patch_matrix/patch_matrix_migrate_contract_signature.rs"]
mod patch_matrix_migrate_contract_signature;

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

fn write_refactor_pkg(dir: &Path) -> PathBuf {
    fs::create_dir_all(dir).unwrap();

    let module = dir.join("mod.gc");
    fs::write(
        &module,
        r#"
          (def ::meta
            (quote
              {
                :module "pkg/refactor"
                :exports [pkg/refactor::public pkg/refactor::contract pkg/refactor::tests]
                :imports [pkg/dep::thing]
              }))
          (def pkg/refactor::public 1)
          (def pkg/refactor::internal 2)
          (def pkg/refactor::contract
            (fn (msg)
              (let ((x msg))
                x)))
          (def pkg/refactor::tests
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
name = "patch-refactor"
version = "0.0.0"
modules = [{ path = "mod.gc", hash = "" }]
dependencies = []
obligations = ["core/obligation::unit-tests"]
tests = ["pkg/refactor::tests"]
"#,
    )
    .unwrap();
    pkg
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

fn copy_repo_toolchain_artifact(dir: &Path) -> PathBuf {
    let src = Path::new(concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/../../selfhost/toolchain.gc"
    ));
    let dst = dir.join("toolchain.gc");
    fs::copy(src, &dst).expect("copy selfhost toolchain artifact");
    dst
}

fn poison_patch_schema_validate_patch_unknown_op(artifact: &Path) {
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
                    Some(Term::Str(path)) if path == "selfhost/patch_schema_apply_v1.gc"
                ) =>
            {
                Some(mm)
            }
            _ => None,
        })
        .expect("selfhost/patch_schema_apply_v1.gc entry");

    let module_src = match patch_mod.get(&TermOrdKey(Term::symbol(":source"))) {
        Some(Term::Str(src)) => src.clone(),
        _ => panic!("patch schema apply module missing :source"),
    };
    let poisoned_src = format!(
        "{module_src}\n(def core/cli::validate-patch (fn (t) ((core/error::make2 \"core/patch-schema\") \"unknown :op\")))\n"
    );
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

fn poison_patch_authority_node_index(artifact: &Path) {
    let src = fs::read_to_string(artifact).expect("read toolchain artifact");
    let mut term = parse_term(&src).expect("parse toolchain artifact");
    let Term::Map(root) = &mut term else {
        panic!("artifact root must be map");
    };
    let Term::Vector(entries) = root
        .get_mut(&TermOrdKey(Term::symbol(":modules")))
        .expect("artifact :modules")
    else {
        panic!("artifact :modules must be vector");
    };
    let patch_mod = entries
        .iter_mut()
        .find_map(|entry| match entry {
            Term::Map(module)
                if matches!(
                    module.get(&TermOrdKey(Term::symbol(":path"))),
                    Some(Term::Str(path)) if path == "selfhost/patch_authority_identity_v1.gc"
                ) =>
            {
                Some(module)
            }
            _ => None,
        })
        .expect("patch authority module");
    let module_src = match patch_mod.get(&TermOrdKey(Term::symbol(":source"))) {
        Some(Term::Str(src)) => src.clone(),
        _ => panic!("patch authority module missing :source"),
    };
    let poisoned_src = format!(
        "{module_src}\n(def core/cli::patch-semantic-node-index (fn (request) {{:kind \"genesis/patch-node-index-v0.1\" :module-h \"{}\" :module-path \"mod.gc\" :nodes [] :ok true :profile \"genesis/patch-authority-v0.1\" :v 1}}))\n",
        "0".repeat(64)
    );
    let poisoned_forms = canonicalize_module(parse_module(&poisoned_src).expect("parse poisoned"))
        .expect("canonicalize poisoned");
    patch_mod.insert(TermOrdKey(Term::symbol(":source")), Term::Str(poisoned_src));
    patch_mod.insert(
        TermOrdKey(Term::symbol(":forms")),
        Term::Vector(poisoned_forms.clone()),
    );
    patch_mod.insert(
        TermOrdKey(Term::symbol(":module-h")),
        Term::Bytes(hash_module(&poisoned_forms).to_vec().into()),
    );
    fs::write(artifact, print_term(&term)).expect("write poisoned artifact");
}

fn poison_patch_authority_normalize(artifact: &Path) {
    let src = fs::read_to_string(artifact).expect("read toolchain artifact");
    let mut term = parse_term(&src).expect("parse toolchain artifact");
    let Term::Map(root) = &mut term else {
        panic!("artifact root must be map");
    };
    let Term::Vector(entries) = root
        .get_mut(&TermOrdKey(Term::symbol(":modules")))
        .expect("artifact :modules")
    else {
        panic!("artifact :modules must be vector");
    };
    let patch_mod = entries
        .iter_mut()
        .find_map(|entry| match entry {
            Term::Map(module)
                if matches!(
                    module.get(&TermOrdKey(Term::symbol(":path"))),
                    Some(Term::Str(path))
                        if path == "selfhost/patch_authority_normalize_v1.gc"
                ) =>
            {
                Some(module)
            }
            _ => None,
        })
        .expect("patch normalization authority module");
    let module_src = match patch_mod.get(&TermOrdKey(Term::symbol(":source"))) {
        Some(Term::Str(src)) => src.clone(),
        _ => panic!("patch normalization authority module missing :source"),
    };
    let poisoned_src = format!(
        "{module_src}\n(def core/cli::patch-normalize (fn (request) {{:kind \"genesis/patch-normalize-v0.1\" :normalized-patch {{:intent \"poisoned\" :ops [] :provenance {{}} :version 1}} :ok true :op-identities [] :patch-h \"{zeros}\" :profile \"genesis/patch-authority-v0.1\" :source-patch-h \"{zeros}\" :v 1}}))\n",
        zeros = "0".repeat(64)
    );
    let poisoned_forms = canonicalize_module(parse_module(&poisoned_src).expect("parse poisoned"))
        .expect("canonicalize poisoned");
    patch_mod.insert(TermOrdKey(Term::symbol(":source")), Term::Str(poisoned_src));
    patch_mod.insert(
        TermOrdKey(Term::symbol(":forms")),
        Term::Vector(poisoned_forms.clone()),
    );
    patch_mod.insert(
        TermOrdKey(Term::symbol(":module-h")),
        Term::Bytes(hash_module(&poisoned_forms).to_vec().into()),
    );
    fs::write(artifact, print_term(&term)).expect("write poisoned artifact");
}

fn poison_patch_authority_preflight(artifact: &Path) {
    let src = fs::read_to_string(artifact).expect("read toolchain artifact");
    let mut term = parse_term(&src).expect("parse toolchain artifact");
    let Term::Map(root) = &mut term else {
        panic!("artifact root must be map");
    };
    let Term::Vector(entries) = root
        .get_mut(&TermOrdKey(Term::symbol(":modules")))
        .expect("artifact :modules")
    else {
        panic!("artifact :modules must be vector");
    };
    let patch_mod = entries
        .iter_mut()
        .find_map(|entry| match entry {
            Term::Map(module)
                if matches!(
                    module.get(&TermOrdKey(Term::symbol(":path"))),
                    Some(Term::Str(path))
                        if path == "selfhost/patch_authority_preflight_v1.gc"
                ) =>
            {
                Some(module)
            }
            _ => None,
        })
        .expect("patch preflight authority module");
    let module_src = match patch_mod.get(&TermOrdKey(Term::symbol(":source"))) {
        Some(Term::Str(src)) => src.clone(),
        _ => panic!("patch preflight authority module missing :source"),
    };
    let poisoned_src = format!(
        r#"{module_src}
(def core/cli::patch-preflight
  (fn (request)
    {{:checks []
      :conflict nil
      :final-path-states ((core/map::get request) (quote :path-states))
      :kind "genesis/patch-preflight-v0.1"
      :ok true
      :patch-h (selfhost/hash::hash-term ((core/map::get request) (quote :patch)))
      :path-states-h (selfhost/hash::hash-term ((core/map::get request) (quote :path-states)))
      :profile "genesis/patch-authority-v0.1"
      :v 1}}))
"#
    );
    let poisoned_forms = canonicalize_module(parse_module(&poisoned_src).expect("parse poisoned"))
        .expect("canonicalize poisoned");
    patch_mod.insert(TermOrdKey(Term::symbol(":source")), Term::Str(poisoned_src));
    patch_mod.insert(
        TermOrdKey(Term::symbol(":forms")),
        Term::Vector(poisoned_forms.clone()),
    );
    patch_mod.insert(
        TermOrdKey(Term::symbol(":module-h")),
        Term::Bytes(hash_module(&poisoned_forms).to_vec().into()),
    );
    fs::write(artifact, print_term(&term)).expect("write poisoned artifact");
}

fn poison_patch_authority_preflight_final_state(artifact: &Path) {
    let src = fs::read_to_string(artifact).expect("read toolchain artifact");
    let mut term = parse_term(&src).expect("parse toolchain artifact");
    let Term::Map(root) = &mut term else {
        panic!("artifact root must be map");
    };
    let Term::Vector(entries) = root
        .get_mut(&TermOrdKey(Term::symbol(":modules")))
        .expect("artifact :modules")
    else {
        panic!("artifact :modules must be vector");
    };
    let patch_mod = entries
        .iter_mut()
        .find_map(|entry| match entry {
            Term::Map(module)
                if matches!(
                    module.get(&TermOrdKey(Term::symbol(":path"))),
                    Some(Term::Str(path))
                        if path == "selfhost/patch_authority_preflight_v1.gc"
                ) =>
            {
                Some(module)
            }
            _ => None,
        })
        .expect("patch preflight authority module");
    let module_src = match patch_mod.get(&TermOrdKey(Term::symbol(":source"))) {
        Some(Term::Str(src)) => src.clone(),
        _ => panic!("patch preflight authority module missing :source"),
    };
    let poisoned_src = module_src.replacen(
        "(((core/map::put result) (quote :state))\n            (((core/map::put ((core/map::get result) (quote :state))) path) next-state))",
        "(((core/map::put result) (quote :state))\n            (((core/map::put ((core/map::get result) (quote :state))) path) \"other\"))",
        1,
    );
    assert_ne!(
        poisoned_src, module_src,
        "preflight transition poison applied"
    );
    let poisoned_forms = canonicalize_module(parse_module(&poisoned_src).expect("parse poisoned"))
        .expect("canonicalize poisoned");
    patch_mod.insert(TermOrdKey(Term::symbol(":source")), Term::Str(poisoned_src));
    patch_mod.insert(
        TermOrdKey(Term::symbol(":forms")),
        Term::Vector(poisoned_forms.clone()),
    );
    patch_mod.insert(
        TermOrdKey(Term::symbol(":module-h")),
        Term::Bytes(hash_module(&poisoned_forms).to_vec().into()),
    );
    fs::write(artifact, print_term(&term)).expect("write poisoned artifact");
}

fn poison_patch_refactor_rename_symbol_forms(artifact: &Path) {
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
                    Some(Term::Str(path)) if path == "selfhost/patch_schema_refactor_v1.gc"
                ) =>
            {
                Some(mm)
            }
            _ => None,
        })
        .expect("selfhost/patch_schema_refactor_v1.gc entry");

    let module_src = match patch_mod.get(&TermOrdKey(Term::symbol(":source"))) {
        Some(Term::Str(src)) => src.clone(),
        _ => panic!("patch refactor module missing :source"),
    };
    let poisoned_src = format!(
        "{module_src}\n(def core/cli::rename-symbol-forms (fn (req) ((core/error::make2 \"core/patch-schema\") \"rename-symbol poisoned\")))\n"
    );
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

fn poison_patch_refactor_split_module_forms(artifact: &Path) {
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
                    Some(Term::Str(path)) if path == "selfhost/patch_schema_refactor_v1.gc"
                ) =>
            {
                Some(mm)
            }
            _ => None,
        })
        .expect("selfhost/patch_schema_refactor_v1.gc entry");

    let module_src = match patch_mod.get(&TermOrdKey(Term::symbol(":source"))) {
        Some(Term::Str(src)) => src.clone(),
        _ => panic!("patch refactor module missing :source"),
    };
    let poisoned_src = format!(
        "{module_src}\n(def core/cli::split-module-forms (fn (req) ((core/error::make2 \"core/patch-schema\") \"split-module poisoned\")))\n"
    );
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

fn poison_patch_refactor_rewrite_meta_list_forms(artifact: &Path) {
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
                    Some(Term::Str(path))
                        if path == "selfhost/patch_schema_refactor_meta_migrate_v1.gc"
                ) =>
            {
                Some(mm)
            }
            _ => None,
        })
        .expect("selfhost/patch_schema_refactor_meta_migrate_v1.gc entry");

    let module_src = match patch_mod.get(&TermOrdKey(Term::symbol(":source"))) {
        Some(Term::Str(src)) => src.clone(),
        _ => panic!("patch refactor module missing :source"),
    };
    let poisoned_src = format!(
        "{module_src}\n(def core/cli::rewrite-meta-list-forms (fn (req) ((core/error::make2 \"core/patch-schema\") \"rewrite-meta-list poisoned\")))\n"
    );
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

fn poison_patch_manifest_move_module(artifact: &Path) {
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
                    Some(Term::Str(path)) if path == "selfhost/patch_schema_manifest_v1.gc"
                ) =>
            {
                Some(mm)
            }
            _ => None,
        })
        .expect("selfhost/patch_schema_manifest_v1.gc entry");

    let module_src = match patch_mod.get(&TermOrdKey(Term::symbol(":source"))) {
        Some(Term::Str(src)) => src.clone(),
        _ => panic!("patch manifest module missing :source"),
    };
    let poisoned_src = format!(
        "{module_src}\n(def core/cli::manifest-apply-move-module (fn (manifest) (fn (from) (fn (to) ((core/error::make2 \"core/patch-schema\") \"move-module poisoned\")))))\n"
    );
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
fn apply_patch_selfhost_rejects_unknown_op_validator_error_without_rust_fallback() {
    let td = tempfile::tempdir().unwrap();
    let pkg = write_pkg(td.path());
    let patch = write_patch(
        td.path(),
        &patch_replace_form0(r#"(def my/pkg::tests { "t1" { :body (fn (_) 1) :expect 1 } })"#),
    );

    let artifact = copy_repo_toolchain_artifact(td.path());
    poison_patch_schema_validate_patch_unknown_op(&artifact);

    let frontend =
        gc_obligations::CoreformFrontend::Selfhost(gc_obligations::SelfhostFrontendConfig {
            bootstrap_mode: gc_prelude::SelfhostBootstrapMode::ArtifactOnly,
            artifact: Some(artifact),
        });

    let err = gc_patches::apply_patch_with_step_limit_and_frontend(
        &patch,
        &pkg,
        None,
        StepLimit::Default,
        MemLimits::default(),
        frontend,
    )
    .unwrap_err();
    match err {
        gc_patches::PatchError::Validate(msg) => assert!(
            msg.contains("unknown :op"),
            "expected unknown :op in validation failure, got: {msg}"
        ),
        other => panic!("expected PatchError::Validate, got {other}"),
    }
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
    let module_before = fs::read(td.path().join("mod.gc")).unwrap();

    let r = gc_patches::apply_patch(&patch_path, &pkg, None).unwrap();
    assert!(!r.ok, "expected obligations to fail after patch");
    assert!(r.acceptance_artifact.is_some());
    assert!(r.package_artifact.is_some());
    assert!(!r.report_artifact.is_empty());
    let report = fs::read_to_string(td.path().join(".genesis/store").join(&r.report_artifact))
        .expect("read patch report");
    assert!(report.contains(":patch-h"));
    assert!(report.contains(":source-patch-h"));
    assert!(report.contains(":op-identities"));
    assert_eq!(
        fs::read(td.path().join("mod.gc")).unwrap(),
        module_before,
        "failed obligations must restore package source"
    );
}

#[test]
fn patch_hard_obligation_error_restores_manifest_and_added_module() {
    let td = tempfile::tempdir().unwrap();
    let pkg = write_pkg(td.path());
    let manifest_before = fs::read(&pkg).unwrap();
    let module_before = fs::read(td.path().join("mod.gc")).unwrap();
    let patch = write_patch(
        td.path(),
        r#"{
          :version 1
          :intent "add invalid module"
          :provenance {}
          :ops [{
            :op :add-module
            :module-path "bad.gc"
            :content "(def my/pkg::bad missing/symbol)"
          }]
        }"#,
    );

    let error = gc_patches::apply_patch(&patch, &pkg, None).unwrap_err();
    assert!(error.to_string().contains("obligations error"), "{error}");
    assert_eq!(fs::read(&pkg).unwrap(), manifest_before);
    assert_eq!(fs::read(td.path().join("mod.gc")).unwrap(), module_before);
    assert!(!td.path().join("bad.gc").exists());
}

#[test]
fn patch_normalization_rejects_unknown_fields() {
    let td = tempfile::tempdir().unwrap();
    let pkg = write_pkg(td.path());
    let patch = write_patch(
        td.path(),
        r#"
          {
            :version 1
            :intent "unknown field"
            :provenance {}
            :ops [
              {
                :op :remove-module
                :module-path "mod.gc"
                :surprise true
              }
            ]
          }
        "#,
    );
    let error = gc_patches::apply_patch(&patch, &pkg, None).unwrap_err();
    assert!(
        error.to_string().contains("unknown field"),
        "unexpected normalization failure: {error}"
    );
}

#[test]
fn patch_normalization_rejects_module_paths_outside_package_before_mutation() {
    let td = tempfile::tempdir().unwrap();
    let pkg = write_pkg(td.path());
    let escaped_name = format!(
        "gc-patch-escape-{}.gc",
        td.path().file_name().unwrap().to_string_lossy()
    );
    let escaped = td.path().parent().unwrap().join(&escaped_name);
    assert!(!escaped.exists());
    let patch = write_patch(
        td.path(),
        &format!(
            r#"{{
              :version 1
              :intent "attempt path escape"
              :provenance {{}}
              :ops [{{
                :op :add-module
                :module-path "../{escaped_name}"
                :content "(def escaped::value 1)"
              }}]
            }}"#
        ),
    );

    let error = gc_patches::apply_patch(&patch, &pkg, None).unwrap_err();
    assert!(
        error.to_string().contains("portable package-relative path"),
        "unexpected path validation failure: {error}"
    );
    assert!(!escaped.exists(), "path escape created an external file");
    assert!(td.path().join("mod.gc").exists());
}

#[test]
fn patch_preflight_reports_missing_module_conflict_before_mutation() {
    let td = tempfile::tempdir().unwrap();
    let pkg = write_pkg(td.path());
    let manifest_before = fs::read_to_string(&pkg).unwrap();
    let module_before = fs::read_to_string(td.path().join("mod.gc")).unwrap();
    let patch = write_patch(
        td.path(),
        r#"{
          :version 1
          :intent "remove missing module"
          :provenance {}
          :ops [{:op :remove-module :module-path "missing.gc"}]
        }"#,
    );

    let error = gc_patches::apply_patch(&patch, &pkg, None).unwrap_err();
    let message = error.to_string();
    assert!(message.contains("patch/path-state-conflict"), "{message}");
    assert!(message.contains("ordinal=0"), "{message}");
    assert!(message.contains("path=missing.gc"), "{message}");
    assert!(message.contains("expected=file actual=absent"), "{message}");
    assert_eq!(fs::read_to_string(&pkg).unwrap(), manifest_before);
    assert_eq!(
        fs::read_to_string(td.path().join("mod.gc")).unwrap(),
        module_before
    );
    assert!(!td.path().join(".genesis").exists());
}

#[test]
fn patch_preflight_applies_ordered_remove_then_add_transition() {
    let td = tempfile::tempdir().unwrap();
    let pkg = write_pkg(td.path());
    let patch = write_patch(
        td.path(),
        r#"{
          :version 1
          :intent "replace module through ordered path transition"
          :provenance {}
          :ops [
            {:op :remove-module :module-path "mod.gc"}
            {
              :op :add-module
              :module-path "mod.gc"
              :content "(def my/pkg::tests {\"t1\" {:body (fn (_) 2) :expect 2}})"
            }
          ]
        }"#,
    );

    let result = gc_patches::apply_patch(&patch, &pkg, None).unwrap();
    assert!(result.ok);
    assert!(
        fs::read_to_string(td.path().join("mod.gc"))
            .unwrap()
            .contains(":expect 2")
    );
}

#[test]
fn patch_preflight_rejects_incomplete_authority_report_without_rust_fallback() {
    let td = tempfile::tempdir().unwrap();
    let pkg = write_pkg(td.path());
    let patch = write_patch(
        td.path(),
        &patch_replace_form0(r#"(def my/pkg::tests {"t1" {:body (fn (_) 2) :expect 2}})"#),
    );
    let module_before = fs::read_to_string(td.path().join("mod.gc")).unwrap();
    let artifact = copy_repo_toolchain_artifact(td.path());
    poison_patch_authority_preflight(&artifact);
    let frontend =
        gc_obligations::CoreformFrontend::Selfhost(gc_obligations::SelfhostFrontendConfig {
            bootstrap_mode: gc_prelude::SelfhostBootstrapMode::ArtifactOnly,
            artifact: Some(artifact),
        });

    let error = gc_patches::apply_patch_with_step_limit_and_frontend(
        &patch,
        &pkg,
        None,
        StepLimit::Default,
        MemLimits::default(),
        frontend,
    )
    .unwrap_err();
    assert!(
        error
            .to_string()
            .contains("successful report is incomplete"),
        "unexpected authority failure: {error}"
    );
    assert_eq!(
        fs::read_to_string(td.path().join("mod.gc")).unwrap(),
        module_before
    );
    assert!(!td.path().join(".genesis").exists());
}

#[test]
fn patch_preflight_rejects_tampered_final_state_without_rust_fallback() {
    let td = tempfile::tempdir().unwrap();
    let pkg = write_pkg(td.path());
    let patch = write_patch(
        td.path(),
        r#"{
          :version 1
          :intent "add module with poisoned final state"
          :provenance {}
          :ops [{
            :op :add-module
            :module-path "added.gc"
            :content "(def my/pkg::added 1)"
          }]
        }"#,
    );
    let artifact = copy_repo_toolchain_artifact(td.path());
    poison_patch_authority_preflight_final_state(&artifact);
    let frontend =
        gc_obligations::CoreformFrontend::Selfhost(gc_obligations::SelfhostFrontendConfig {
            bootstrap_mode: gc_prelude::SelfhostBootstrapMode::ArtifactOnly,
            artifact: Some(artifact),
        });

    let error = gc_patches::apply_patch_with_step_limit_and_frontend(
        &patch,
        &pkg,
        None,
        StepLimit::Default,
        MemLimits::default(),
        frontend,
    )
    .unwrap_err();
    assert!(
        error
            .to_string()
            .contains("final-path-states[0] facts mismatch"),
        "unexpected authority failure: {error}"
    );
    assert!(!td.path().join("added.gc").exists());
    assert!(!td.path().join(".genesis").exists());
}

#[cfg(unix)]
#[test]
fn patch_preflight_rejects_symlink_module_before_mutation() {
    use std::os::unix::fs::symlink;

    let td = tempfile::tempdir().unwrap();
    let pkg = write_pkg(td.path());
    let external = td.path().parent().unwrap().join(format!(
        "gc-patch-symlink-target-{}.gc",
        td.path().file_name().unwrap().to_string_lossy()
    ));
    assert!(!external.exists());
    fs::write(&external, "(def external::value 1)\n").unwrap();
    symlink(&external, td.path().join("alias.gc")).unwrap();
    let patch = write_patch(
        td.path(),
        r#"{
          :version 1
          :intent "replace through symlink"
          :provenance {}
          :ops [{
            :op :replace-node
            :module-path "alias.gc"
            :path [[:form 0]]
            :new (def external::value 2)
          }]
        }"#,
    );

    let error = gc_patches::apply_patch(&patch, &pkg, None).unwrap_err();
    assert!(error.to_string().contains("symlink path component denied"));
    assert_eq!(
        fs::read_to_string(&external).unwrap(),
        "(def external::value 1)\n"
    );
    assert!(!td.path().join(".genesis").exists());
    fs::remove_file(external).unwrap();
}

#[test]
fn patch_normalization_rejects_tampered_report_without_rust_fallback() {
    let td = tempfile::tempdir().unwrap();
    let pkg = write_pkg(td.path());
    let patch = write_patch(
        td.path(),
        &patch_replace_form0(r#"(def my/pkg::tests { "t1" { :body (fn (_) 1) :expect 1 } })"#),
    );
    let artifact = copy_repo_toolchain_artifact(td.path());
    poison_patch_authority_normalize(&artifact);
    let frontend =
        gc_obligations::CoreformFrontend::Selfhost(gc_obligations::SelfhostFrontendConfig {
            bootstrap_mode: gc_prelude::SelfhostBootstrapMode::ArtifactOnly,
            artifact: Some(artifact),
        });
    let error = gc_patches::apply_patch_with_step_limit_and_frontend(
        &patch,
        &pkg,
        None,
        StepLimit::Default,
        MemLimits::default(),
        frontend,
    )
    .unwrap_err();
    assert!(
        error.to_string().contains(":source-patch-h mismatch"),
        "unexpected authority failure: {error}"
    );
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
fn semantic_node_index_rejects_tampered_authority_report_without_rust_fallback() {
    let td = tempfile::tempdir().unwrap();
    let artifact = copy_repo_toolchain_artifact(td.path());
    poison_patch_authority_node_index(&artifact);
    let frontend =
        gc_obligations::CoreformFrontend::Selfhost(gc_obligations::SelfhostFrontendConfig {
            bootstrap_mode: gc_prelude::SelfhostBootstrapMode::ArtifactOnly,
            artifact: Some(artifact),
        });
    let error = gc_patches::semantic_node_index_for_module_with_frontend(
        "mod.gc",
        "(def example::value {:nested [1 2 3]})",
        &frontend,
        StepLimit::Default,
        MemLimits::default(),
    )
    .unwrap_err();
    assert!(
        error.to_string().contains(":module-h mismatch"),
        "unexpected authority failure: {error}"
    );
}

#[cfg(feature = "parity-oracle")]
#[test]
fn semantic_node_index_matches_compile_time_rust_oracle() {
    let source = r#"
        (def example::scalar 7)
        (def example::data {:a [nil true 42 "text" b"bytes" example::symbol]})
        (def example::pair (quote (left . right)))
    "#;
    let selfhost = gc_patches::semantic_node_index_for_module_with_frontend(
        "example.gc",
        source,
        &gc_obligations::default_coreform_frontend(),
        StepLimit::Default,
        MemLimits::default(),
    )
    .unwrap();
    let rust = gc_patches::semantic_node_index_for_module_with_frontend(
        "example.gc",
        source,
        &gc_obligations::rust_coreform_frontend(),
        StepLimit::Default,
        MemLimits::default(),
    )
    .unwrap();
    assert_eq!(selfhost, rust);
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

#[test]
fn patch_rename_symbol_updates_module_and_report() {
    let td = tempfile::tempdir().unwrap();
    let pkg = write_refactor_pkg(td.path());
    let patch = write_patch(
        td.path(),
        r#"
          {
            :version 1
            :intent "rename symbol"
            :provenance {}
            :ops [
              {
                :op :rename-symbol
                :module-path "mod.gc"
                :from pkg/refactor::public
                :to pkg/refactor::renamed
              }
            ]
          }
        "#,
    );
    let r = gc_patches::apply_patch(&patch, &pkg, None).unwrap();
    assert!(r.ok, "rename-symbol should keep obligations passing");
    let src = fs::read_to_string(td.path().join("mod.gc")).unwrap();
    assert!(src.contains("pkg/refactor::renamed"));
    assert!(!src.contains("pkg/refactor::public"));
    let report = fs::read_to_string(td.path().join(".genesis/store").join(&r.report_artifact))
        .expect("read report artifact");
    assert!(report.contains(":rename-symbol"));
    assert!(report.contains(":before-module-h"));
    assert!(report.contains(":after-module-h"));
}

#[test]
fn apply_patch_selfhost_rename_uses_refactor_contract() {
    let td = tempfile::tempdir().unwrap();
    let pkg = write_refactor_pkg(td.path());
    let patch = write_patch(
        td.path(),
        r#"
          {
            :version 1
            :intent "rename symbol"
            :provenance {}
            :ops [
              {
                :op :rename-symbol
                :module-path "mod.gc"
                :from pkg/refactor::public
                :to pkg/refactor::renamed
              }
            ]
          }
        "#,
    );
    let artifact = copy_repo_toolchain_artifact(td.path());
    poison_patch_refactor_rename_symbol_forms(&artifact);
    let frontend =
        gc_obligations::CoreformFrontend::Selfhost(gc_obligations::SelfhostFrontendConfig {
            bootstrap_mode: gc_prelude::SelfhostBootstrapMode::ArtifactOnly,
            artifact: Some(artifact),
        });

    let err = gc_patches::apply_patch_with_step_limit_and_frontend(
        &patch,
        &pkg,
        None,
        StepLimit::Default,
        MemLimits::default(),
        frontend,
    )
    .unwrap_err();
    match err {
        gc_patches::PatchError::Validate(msg) => assert!(
            msg.contains("rename-symbol poisoned"),
            "expected poisoned rename error, got: {msg}"
        ),
        other => panic!("expected PatchError::Validate, got {other}"),
    }
}

#[test]
fn patch_move_module_updates_manifest_and_path() {
    let td = tempfile::tempdir().unwrap();
    let pkg = write_refactor_pkg(td.path());
    let patch = write_patch(
        td.path(),
        r#"
          {
            :version 1
            :intent "move module"
            :provenance {}
            :ops [
              {
                :op :move-module
                :from-module-path "mod.gc"
                :to-module-path "moved/mod.gc"
              }
            ]
          }
        "#,
    );
    let r = gc_patches::apply_patch(&patch, &pkg, None).unwrap();
    assert!(r.ok, "move-module should keep obligations passing");
    assert!(!td.path().join("mod.gc").exists());
    assert!(td.path().join("moved/mod.gc").exists());
    let pkg_src = fs::read_to_string(&pkg).unwrap();
    assert!(pkg_src.contains("moved/mod.gc"));
}

#[test]
fn apply_patch_selfhost_move_module_uses_manifest_contract() {
    let td = tempfile::tempdir().unwrap();
    let pkg = write_refactor_pkg(td.path());
    let patch = write_patch(
        td.path(),
        r#"
          {
            :version 1
            :intent "move module"
            :provenance {}
            :ops [
              {
                :op :move-module
                :from-module-path "mod.gc"
                :to-module-path "moved/mod.gc"
              }
            ]
          }
        "#,
    );
    let artifact = copy_repo_toolchain_artifact(td.path());
    poison_patch_manifest_move_module(&artifact);
    let frontend =
        gc_obligations::CoreformFrontend::Selfhost(gc_obligations::SelfhostFrontendConfig {
            bootstrap_mode: gc_prelude::SelfhostBootstrapMode::ArtifactOnly,
            artifact: Some(artifact),
        });

    let err = gc_patches::apply_patch_with_step_limit_and_frontend(
        &patch,
        &pkg,
        None,
        StepLimit::Default,
        MemLimits::default(),
        frontend,
    )
    .unwrap_err();
    match err {
        gc_patches::PatchError::Validate(msg) => assert!(
            msg.contains("move-module poisoned"),
            "expected poisoned move error, got: {msg}"
        ),
        other => panic!("expected PatchError::Validate, got {other}"),
    }
}

#[test]
fn patch_split_module_extracts_requested_defs() {
    let td = tempfile::tempdir().unwrap();
    let pkg = write_refactor_pkg(td.path());
    let patch = write_patch(
        td.path(),
        r#"
          {
            :version 1
            :intent "split module"
            :provenance {}
            :ops [
              {
                :op :split-module
                :from-module-path "mod.gc"
                :to-module-path "split/internal.gc"
                :symbols [pkg/refactor::internal]
              }
            ]
          }
        "#,
    );
    let r = gc_patches::apply_patch(&patch, &pkg, None).unwrap();
    assert!(r.ok, "split-module should keep obligations passing");
    let src = fs::read_to_string(td.path().join("mod.gc")).unwrap();
    let split = fs::read_to_string(td.path().join("split/internal.gc")).unwrap();
    assert!(!src.contains("pkg/refactor::internal"));
    assert!(split.contains("pkg/refactor::internal"));
    let pkg_src = fs::read_to_string(&pkg).unwrap();
    assert!(pkg_src.contains("split/internal.gc"));
}

#[test]
fn apply_patch_selfhost_split_uses_refactor_contract() {
    let td = tempfile::tempdir().unwrap();
    let pkg = write_refactor_pkg(td.path());
    let patch = write_patch(
        td.path(),
        r#"
          {
            :version 1
            :intent "split module"
            :provenance {}
            :ops [
              {
                :op :split-module
                :from-module-path "mod.gc"
                :to-module-path "split/internal.gc"
                :symbols [pkg/refactor::internal]
              }
            ]
          }
        "#,
    );
    let artifact = copy_repo_toolchain_artifact(td.path());
    poison_patch_refactor_split_module_forms(&artifact);
    let frontend =
        gc_obligations::CoreformFrontend::Selfhost(gc_obligations::SelfhostFrontendConfig {
            bootstrap_mode: gc_prelude::SelfhostBootstrapMode::ArtifactOnly,
            artifact: Some(artifact),
        });

    let err = gc_patches::apply_patch_with_step_limit_and_frontend(
        &patch,
        &pkg,
        None,
        StepLimit::Default,
        MemLimits::default(),
        frontend,
    )
    .unwrap_err();
    match err {
        gc_patches::PatchError::Validate(msg) => assert!(
            msg.contains("split-module poisoned"),
            "expected poisoned split error, got: {msg}"
        ),
        other => panic!("expected PatchError::Validate, got {other}"),
    }
}

#[test]
fn apply_patch_selfhost_rewrite_meta_list_uses_refactor_contract() {
    let td = tempfile::tempdir().unwrap();
    let pkg = write_refactor_pkg(td.path());
    let patch = write_patch(
        td.path(),
        r#"
          {
            :version 1
            :intent "rewrite exports"
            :provenance {}
            :ops [
              {
                :op :rewrite-exports
                :module-path "mod.gc"
                :remove [pkg/refactor::public]
                :add [pkg/refactor::internal]
              }
            ]
          }
        "#,
    );
    let artifact = copy_repo_toolchain_artifact(td.path());
    poison_patch_refactor_rewrite_meta_list_forms(&artifact);
    let frontend =
        gc_obligations::CoreformFrontend::Selfhost(gc_obligations::SelfhostFrontendConfig {
            bootstrap_mode: gc_prelude::SelfhostBootstrapMode::ArtifactOnly,
            artifact: Some(artifact),
        });

    let err = gc_patches::apply_patch_with_step_limit_and_frontend(
        &patch,
        &pkg,
        None,
        StepLimit::Default,
        MemLimits::default(),
        frontend,
    )
    .unwrap_err();
    match err {
        gc_patches::PatchError::Validate(msg) => assert!(
            msg.contains("rewrite-meta-list poisoned"),
            "expected poisoned rewrite-meta-list error, got: {msg}"
        ),
        other => panic!("expected PatchError::Validate, got {other}"),
    }
}

#[test]
fn patch_rewrite_exports_updates_meta_lists() {
    let td = tempfile::tempdir().unwrap();
    let pkg = write_refactor_pkg(td.path());
    let patch = write_patch(
        td.path(),
        r#"
          {
            :version 1
            :intent "rewrite exports"
            :provenance {}
            :ops [
              {
                :op :rewrite-exports
                :module-path "mod.gc"
                :remove [pkg/refactor::public]
                :add [pkg/refactor::internal]
              }
            ]
          }
        "#,
    );
    let r = gc_patches::apply_patch(&patch, &pkg, None).unwrap();
    assert!(r.ok, "rewrite-exports should keep obligations passing");
    let src = fs::read_to_string(td.path().join("mod.gc")).unwrap();
    assert!(src.contains("pkg/refactor::internal"));
    assert!(!src.contains(":exports [pkg/refactor::public"));
}

#[test]
fn patch_rewrite_imports_replaces_meta_imports() {
    let td = tempfile::tempdir().unwrap();
    let pkg = write_refactor_pkg(td.path());
    let patch = write_patch(
        td.path(),
        r#"
          {
            :version 1
            :intent "rewrite imports"
            :provenance {}
            :ops [
              {
                :op :rewrite-imports
                :module-path "mod.gc"
                :replace [pkg/dep::new]
              }
            ]
          }
        "#,
    );
    let r = gc_patches::apply_patch(&patch, &pkg, None).unwrap();
    assert!(r.ok, "rewrite-imports should keep obligations passing");
    let src = fs::read_to_string(td.path().join("mod.gc")).unwrap();
    assert!(
        src.contains(":imports [pkg/dep::new]"),
        "module source: {src}"
    );
    assert!(!src.contains("pkg/dep::thing"), "module source: {src}");
}

#[test]
fn patch_migrate_contract_signature_renames_param_and_body_refs() {
    let td = tempfile::tempdir().unwrap();
    let pkg = write_refactor_pkg(td.path());
    let patch = write_patch(
        td.path(),
        r#"
          {
            :version 1
            :intent "migrate contract signature"
            :provenance {}
            :ops [
              {
                :op :migrate-contract-signature
                :module-path "mod.gc"
                :contract-symbol pkg/refactor::contract
                :from-param msg
                :to-param request
              }
            ]
          }
        "#,
    );
    let r = gc_patches::apply_patch(&patch, &pkg, None).unwrap();
    assert!(
        r.ok,
        "migrate-contract-signature should keep obligations passing"
    );
    let src = fs::read_to_string(td.path().join("mod.gc")).unwrap();
    assert!(src.contains("(fn (request)"));
    assert!(src.contains("(let ((x request))"));
    assert!(!src.contains("(fn (msg)"));
}
