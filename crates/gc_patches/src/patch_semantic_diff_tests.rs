use std::fs;
use std::path::{Path, PathBuf};

use gc_coreform::{canonicalize_module, hash_module, parse_module, parse_term, print_term};

use super::*;

fn module(path: &str, source: &str) -> SemanticWorkspaceModule {
    SemanticWorkspaceModule {
        module_path: path.to_string(),
        forms: canonicalize_module(parse_module(source).expect("parse module"))
            .expect("canonicalize module"),
    }
}

fn repo_artifact() -> PathBuf {
    Path::new(concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/../../selfhost/toolchain.gc"
    ))
    .to_path_buf()
}

fn frontend(artifact: PathBuf) -> CoreformFrontend {
    CoreformFrontend::Selfhost(gc_obligations::SelfhostFrontendConfig {
        bootstrap_mode: gc_prelude::SelfhostBootstrapMode::ArtifactOnly,
        artifact: Some(artifact),
    })
}

fn empty_provenance() -> Term {
    Term::Map(BTreeMap::new())
}

fn poison_patch_diff_report(artifact: &Path) {
    let source = fs::read_to_string(artifact).expect("read toolchain artifact");
    let mut root = parse_term(&source).expect("parse toolchain artifact");
    let Term::Map(root_map) = &mut root else {
        panic!("artifact root must be a map");
    };
    let Some(Term::Vector(modules)) = root_map.get_mut(&TermOrdKey(Term::symbol(":modules")))
    else {
        panic!("artifact :modules must be a vector");
    };
    let module = modules
        .iter_mut()
        .find_map(|entry| match entry {
            Term::Map(map)
                if matches!(
                    map.get(&TermOrdKey(Term::symbol(":path"))),
                    Some(Term::Str(path)) if path == "selfhost/patch_authority_diff_v1.gc"
                ) =>
            {
                Some(map)
            }
            _ => None,
        })
        .expect("patch diff module");
    let source = match module.get(&TermOrdKey(Term::symbol(":source"))) {
        Some(Term::Str(source)) => source.clone(),
        _ => panic!("patch diff module source"),
    };
    let poisoned =
        format!("{source}\n(def core/cli::patch-diff (fn (request) {{:kind \"poisoned\"}}))\n");
    let forms = canonicalize_module(parse_module(&poisoned).expect("parse poisoned module"))
        .expect("canonicalize poisoned module");
    module.insert(TermOrdKey(Term::symbol(":source")), Term::Str(poisoned));
    module.insert(
        TermOrdKey(Term::symbol(":forms")),
        Term::Vector(forms.clone()),
    );
    module.insert(
        TermOrdKey(Term::symbol(":module-h")),
        Term::Bytes(hash_module(&forms).to_vec().into()),
    );
    fs::write(artifact, print_term(&root)).expect("write poisoned artifact");
}

#[test]
fn authority_emits_canonical_minimal_workspace_diff() {
    let base = vec![
        module("a.gc", "(def pkg::a 1)\n(def pkg::stable 2)"),
        module("old.gc", "(def pkg::old 1)"),
        module("resize.gc", "(def pkg::one 1)"),
    ];
    let target = vec![
        module("resize.gc", "(def pkg::one 1)\n(def pkg::two 2)"),
        module("new.gc", "(def pkg::new 1)"),
        module("a.gc", "(def pkg::a 3)\n(def pkg::stable 2)"),
    ];
    let result = diff_semantic_workspaces_with_frontend(
        "synchronize modules",
        &empty_provenance(),
        &base,
        &target,
        &frontend(repo_artifact()),
        StepLimit::Default,
        MemLimits::default(),
    )
    .expect("semantic workspace diff");

    assert_eq!(result.op_count, 5);
    assert_eq!(result.replacements, 1);
    assert_eq!(result.additions, 2);
    assert_eq!(result.removals, 2);
    assert_eq!(result.patch_hash, hash32_hex(hash_term(&result.patch)));
}

#[test]
fn authority_is_independent_of_input_module_order() {
    let base_a = vec![
        module("b.gc", "(def pkg::b 1)"),
        module("a.gc", "(def pkg::a 1)"),
    ];
    let base_b = vec![base_a[1].clone(), base_a[0].clone()];
    let target_a = vec![
        module("b.gc", "(def pkg::b 2)"),
        module("a.gc", "(def pkg::a 3)"),
    ];
    let target_b = vec![target_a[1].clone(), target_a[0].clone()];
    let frontend = frontend(repo_artifact());
    let left = diff_semantic_workspaces_with_frontend(
        "ordered",
        &empty_provenance(),
        &base_a,
        &target_a,
        &frontend,
        StepLimit::Default,
        MemLimits::default(),
    )
    .expect("first diff");
    let right = diff_semantic_workspaces_with_frontend(
        "ordered",
        &empty_provenance(),
        &base_b,
        &target_b,
        &frontend,
        StepLimit::Default,
        MemLimits::default(),
    )
    .expect("reordered diff");
    assert_eq!(left.patch, right.patch);
    assert_eq!(left.patch_hash, right.patch_hash);
}

#[test]
fn authority_emits_empty_patch_for_identical_workspaces() {
    let modules = vec![module("same.gc", "(def pkg::same 1)")];
    let result = diff_semantic_workspaces_with_frontend(
        "no changes",
        &empty_provenance(),
        &modules,
        &modules,
        &frontend(repo_artifact()),
        StepLimit::Default,
        MemLimits::default(),
    )
    .expect("empty diff");
    assert_eq!(result.op_count, 0);
    assert_eq!(result.additions + result.removals + result.replacements, 0);
}

#[test]
fn authority_rejects_duplicate_module_paths() {
    let modules = vec![
        module("duplicate.gc", "(def pkg::a 1)"),
        module("duplicate.gc", "(def pkg::b 2)"),
    ];
    let error = diff_semantic_workspaces_with_frontend(
        "duplicates",
        &empty_provenance(),
        &modules,
        &[],
        &frontend(repo_artifact()),
        StepLimit::Default,
        MemLimits::default(),
    )
    .expect_err("duplicate path must fail");
    assert!(
        error.to_string().contains("duplicate module path"),
        "{error}"
    );
}

#[test]
fn decoder_rejects_poisoned_authority_without_fallback() {
    let temp = tempfile::tempdir().expect("tempdir");
    let artifact = temp.path().join("toolchain.gc");
    fs::copy(repo_artifact(), &artifact).expect("copy toolchain artifact");
    poison_patch_diff_report(&artifact);
    let error = diff_semantic_workspaces_with_frontend(
        "poison",
        &empty_provenance(),
        &[module("a.gc", "(def pkg::a 1)")],
        &[module("a.gc", "(def pkg::a 2)")],
        &frontend(artifact),
        StepLimit::Default,
        MemLimits::default(),
    )
    .expect_err("poisoned report must fail");
    assert!(
        error.to_string().contains("must contain exactly fields"),
        "{error}"
    );
}

#[test]
fn authority_fails_closed_on_resource_exhaustion() {
    let error = diff_semantic_workspaces_with_frontend(
        "bounded",
        &empty_provenance(),
        &[module("a.gc", "(def pkg::a 1)")],
        &[module("a.gc", "(def pkg::a 2)")],
        &frontend(repo_artifact()),
        StepLimit::Limit(1),
        MemLimits::default(),
    )
    .expect_err("step exhaustion must fail");
    assert!(error.to_string().contains("step limit exceeded"), "{error}");
}
