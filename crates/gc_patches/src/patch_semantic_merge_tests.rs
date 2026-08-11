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

fn provenance() -> Term {
    Term::Map(BTreeMap::new())
}

fn merge(
    base: &[SemanticWorkspaceModule],
    left: &[SemanticWorkspaceModule],
    right: &[SemanticWorkspaceModule],
) -> SemanticWorkspaceMerge {
    merge_semantic_workspaces_with_frontend(
        "merge semantic workspaces",
        &provenance(),
        base,
        left,
        right,
        &frontend(repo_artifact()),
        StepLimit::Default,
        MemLimits::default(),
    )
    .expect("semantic workspace merge")
}

fn poison_patch_merge_report(artifact: &Path) {
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
                    Some(Term::Str(path)) if path == "selfhost/patch_authority_merge_v1.gc"
                ) =>
            {
                Some(map)
            }
            _ => None,
        })
        .expect("patch merge module");
    let source = match module.get(&TermOrdKey(Term::symbol(":source"))) {
        Some(Term::Str(source)) => source.clone(),
        _ => panic!("patch merge module source"),
    };
    let poisoned =
        format!("{source}\n(def core/cli::patch-merge (fn (request) {{:kind \"poisoned\"}}))\n");
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
fn authority_merges_disjoint_top_form_edits_and_embeds_diff() {
    let base = vec![module("app.gc", "(def app::left 1)\n(def app::right 2)")];
    let left = vec![module("app.gc", "(def app::left 3)\n(def app::right 2)")];
    let right = vec![module("app.gc", "(def app::left 1)\n(def app::right 4)")];
    let result = merge(&base, &left, &right);

    assert!(result.conflicts.is_empty());
    assert_eq!(
        result.merged_modules,
        Some(vec![module(
            "app.gc",
            "(def app::left 3)\n(def app::right 4)"
        )])
    );
    let diff = result.diff.expect("embedded canonical diff");
    assert_eq!(diff.op_count, 2);
    assert_eq!(diff.replacements, 2);
}

#[test]
fn authority_resolves_identical_and_unchanged_side_edits() {
    let base = vec![module("base.gc", "(def app::value 1)")];
    let changed = vec![module("base.gc", "(def app::value 2)")];
    let identical = merge(&base, &changed, &changed);
    assert_eq!(identical.merged_modules, Some(changed.clone()));

    let right_only = merge(&base, &base, &changed);
    assert_eq!(right_only.merged_modules, Some(changed));
}

#[test]
fn authority_merges_independent_addition_and_deletion() {
    let base = vec![module("old.gc", "(def app::old 1)")];
    let left = Vec::new();
    let right = vec![
        module("old.gc", "(def app::old 1)"),
        module("new.gc", "(def app::new 2)"),
    ];
    let result = merge(&base, &left, &right);
    assert_eq!(result.merged_modules, Some(vec![right[1].clone()]));
    let diff = result.diff.expect("embedded diff");
    assert_eq!(diff.additions, 1);
    assert_eq!(diff.removals, 1);
}

#[test]
fn authority_reports_same_form_divergence_with_stable_identity() {
    let base = vec![module("app.gc", "(def app::value 1)")];
    let left = vec![module("app.gc", "(def app::value 2)")];
    let right = vec![module("app.gc", "(def app::value 3)")];
    let result = merge(&base, &left, &right);

    assert!(result.merged_modules.is_none());
    assert!(result.diff.is_none());
    assert_eq!(result.conflicts.len(), 1);
    assert_eq!(result.conflicts[0].code, "form/divergent-edit");
    assert_eq!(result.conflicts[0].form_index, Some(0));
    assert_eq!(result.conflicts[0].conflict_hash.len(), 64);
}

#[test]
fn authority_distinguishes_delete_modify_divergent_add_and_structure_conflicts() {
    let base = vec![module("app.gc", "(def app::value 1)")];
    let modified = vec![module("app.gc", "(def app::value 2)")];
    let delete_modify = merge(&base, &[], &modified);
    assert_eq!(delete_modify.conflicts[0].code, "module/delete-modify");

    let left_add = vec![module("new.gc", "(def app::value 1)")];
    let right_add = vec![module("new.gc", "(def app::value 2)")];
    let divergent_add = merge(&[], &left_add, &right_add);
    assert_eq!(divergent_add.conflicts[0].code, "module/divergent-add");

    let left_resize = vec![module("app.gc", "(def app::value 1)\n(def app::left 2)")];
    let right_resize = vec![module(
        "app.gc",
        "(def app::value 1)\n(def app::right 3)\n(def app::extra 4)",
    )];
    let structural = merge(&base, &left_resize, &right_resize);
    assert_eq!(structural.conflicts[0].code, "module/structural-divergence");
}

#[test]
fn authority_is_independent_of_workspace_module_order() {
    let base = vec![
        module("b.gc", "(def app::b 1)"),
        module("a.gc", "(def app::a 1)"),
    ];
    let left = vec![
        module("a.gc", "(def app::a 2)"),
        module("b.gc", "(def app::b 1)"),
    ];
    let right = vec![
        module("b.gc", "(def app::b 3)"),
        module("a.gc", "(def app::a 1)"),
    ];
    let first = merge(&base, &left, &right);
    let second = merge(
        &[base[1].clone(), base[0].clone()],
        &[left[1].clone(), left[0].clone()],
        &[right[1].clone(), right[0].clone()],
    );
    assert_eq!(first, second);
}

#[test]
fn decoder_rejects_poisoned_merge_authority_without_fallback() {
    let temp = tempfile::tempdir().expect("tempdir");
    let artifact = temp.path().join("toolchain.gc");
    fs::copy(repo_artifact(), &artifact).expect("copy toolchain artifact");
    poison_patch_merge_report(&artifact);
    let base = vec![module("app.gc", "(def app::value 1)")];
    let changed = vec![module("app.gc", "(def app::value 2)")];
    let error = merge_semantic_workspaces_with_frontend(
        "poison",
        &provenance(),
        &base,
        &changed,
        &base,
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
    let base = vec![module("app.gc", "(def app::value 1)")];
    let changed = vec![module("app.gc", "(def app::value 2)")];
    let error = merge_semantic_workspaces_with_frontend(
        "bounded",
        &provenance(),
        &base,
        &changed,
        &base,
        &frontend(repo_artifact()),
        StepLimit::Limit(1),
        MemLimits::default(),
    )
    .expect_err("step exhaustion must fail");
    assert!(error.to_string().contains("step limit exceeded"), "{error}");
}
