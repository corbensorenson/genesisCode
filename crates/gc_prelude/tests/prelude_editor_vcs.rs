use gc_coreform::{Term, TermOrdKey, canonicalize_module, parse_module};
use gc_kernel::{EvalCtx, eval_module};
use gc_prelude::build_prelude;

fn eval_to_term(src: &str) -> Term {
    let forms = canonicalize_module(parse_module(src).expect("parse")).expect("canon");
    let mut ctx = EvalCtx::new();
    let prelude = build_prelude(&mut ctx);
    let mut env = prelude.env;
    let v = eval_module(&mut ctx, &mut env, &forms).expect("eval");
    v.to_term_for_log(ctx.protocol.map(|p| p.error))
}

fn map_get<'a>(t: &'a Term, key: &str) -> Option<&'a Term> {
    let Term::Map(m) = t else { return None };
    m.get(&TermOrdKey(Term::symbol(key)))
}

#[test]
fn editor_vcs_diff_panel_projects_patch_and_values() {
    let term = eval_to_term(
        r#"
        (core/editor/vcs::diff-panel-from-response
          (quote
            {
              :ok true
              :patch "patch-h"
              :values ["v1" "v2"]
            }))
        "#,
    );
    assert_eq!(
        map_get(&term, ":kind"),
        Some(&Term::symbol(":editor/vcs-diff-panel"))
    );
    assert_eq!(
        map_get(&term, ":patch"),
        Some(&Term::Str("patch-h".to_string()))
    );
    let Some(Term::Vector(values)) = map_get(&term, ":values") else {
        panic!("panel :values must be vector");
    };
    assert_eq!(values.len(), 2);
}

#[test]
fn editor_vcs_merge_panel_surfaces_conflict_hash() {
    let term = eval_to_term(
        r#"
        (core/editor/vcs::merge-panel-from-response
          (quote
            {
              :ok false
              :conflict "conflict-h"
            }))
        "#,
    );
    assert_eq!(
        map_get(&term, ":kind"),
        Some(&Term::symbol(":editor/vcs-merge-panel"))
    );
    assert_eq!(
        map_get(&term, ":conflict"),
        Some(&Term::Str("conflict-h".to_string()))
    );
}

#[test]
fn editor_vcs_conflict_panel_counts_entries() {
    let term = eval_to_term(
        r#"
        (core/editor/vcs::conflict-panel-from-artifact
          (quote
            {
              :type :vcs/conflict
              :v 1
              :kind :contract-snapshot-merge3
              :base "b"
              :left "l"
              :right "r"
              :conflicts [
                {:op foo/a::x :base "bx" :left "lx" :right "rx"}
                {:op foo/a::y :base nil :left "ly" :right "ry"}
              ]
            }))
        "#,
    );
    assert_eq!(
        map_get(&term, ":kind"),
        Some(&Term::symbol(":editor/vcs-conflict-panel"))
    );
    assert_eq!(map_get(&term, ":count"), Some(&Term::Int(2.into())));
    let Some(Term::Vector(conflicts)) = map_get(&term, ":conflicts") else {
        panic!("panel :conflicts must be vector");
    };
    assert_eq!(conflicts.len(), 2);
}

#[test]
fn editor_vcs_resolve_panel_projects_snapshot_patch_and_values() {
    let term = eval_to_term(
        r#"
        (core/editor/vcs::resolve-panel-from-response
          (quote
            {
              :ok true
              :snapshot "snap-h"
              :patch "patch-h"
              :values ["a" "b" "c"]
            }))
        "#,
    );
    assert_eq!(
        map_get(&term, ":kind"),
        Some(&Term::symbol(":editor/vcs-resolve-panel"))
    );
    assert_eq!(
        map_get(&term, ":snapshot"),
        Some(&Term::Str("snap-h".to_string()))
    );
    assert_eq!(
        map_get(&term, ":patch"),
        Some(&Term::Str("patch-h".to_string()))
    );
    let Some(Term::Vector(values)) = map_get(&term, ":values") else {
        panic!("panel :values must be vector");
    };
    assert_eq!(values.len(), 3);
}

#[test]
fn editor_vcs_commit_panel_projects_core_fields() {
    let term = eval_to_term(
        r#"
        ((core/editor/vcs::commit-panel-from-artifact "commit-h")
          (quote
            {
              :type :vcs/commit
              :v 1
              :parents ["p1" "p2"]
              :base "b"
              :patch "patch-h"
              :result "result-h"
              :obligations [core/obligation::unit-tests]
              :evidence ["ev1"]
              :attestations ["at1"]
              :message "msg"
              :target {:kind :package :name "my-lib"}
            }))
        "#,
    );
    assert_eq!(
        map_get(&term, ":kind"),
        Some(&Term::symbol(":editor/vcs-commit-panel"))
    );
    assert_eq!(
        map_get(&term, ":hash"),
        Some(&Term::Str("commit-h".to_string()))
    );
    assert_eq!(
        map_get(&term, ":patch"),
        Some(&Term::Str("patch-h".to_string()))
    );
    let Some(Term::Vector(evidence)) = map_get(&term, ":evidence") else {
        panic!("panel :evidence must be vector");
    };
    assert_eq!(evidence.len(), 1);
}

#[test]
fn editor_vcs_evidence_panel_projects_counts() {
    let term = eval_to_term(
        r#"
        ((core/editor/vcs::evidence-panel-from-artifact "ev-h")
          (quote
            {
              :type :vcs/evidence
              :v 1
              :kind :effect-log
              :inputs ["a" "b"]
              :outputs ["c"]
              :data "blob-h"
            }))
        "#,
    );
    assert_eq!(
        map_get(&term, ":kind"),
        Some(&Term::symbol(":editor/vcs-evidence-panel"))
    );
    assert_eq!(
        map_get(&term, ":hash"),
        Some(&Term::Str("ev-h".to_string()))
    );
    assert_eq!(map_get(&term, ":input-count"), Some(&Term::Int(2.into())));
    assert_eq!(map_get(&term, ":output-count"), Some(&Term::Int(1.into())));
}

#[test]
fn editor_vcs_evidence_list_panel_from_commit_panel() {
    let term = eval_to_term(
        r#"
        (core/editor/vcs::evidence-list-panel-from-commit-panel
          (quote
            {
              :hash "commit-h"
              :evidence ["ev1" "ev2" "ev3"]
            }))
        "#,
    );
    assert_eq!(
        map_get(&term, ":kind"),
        Some(&Term::symbol(":editor/vcs-evidence-list-panel"))
    );
    assert_eq!(
        map_get(&term, ":commit"),
        Some(&Term::Str("commit-h".to_string()))
    );
    assert_eq!(map_get(&term, ":count"), Some(&Term::Int(3.into())));
}

#[test]
fn editor_vcs_blame_panel_projects_identity_fields() {
    let term = eval_to_term(
        r#"
        (core/editor/vcs::blame-panel-from-response
          (quote
            {
              :commit "c-h"
              :snapshot "s-h"
              :sym pkg/mod::x
              :value "v-h"
            }))
        "#,
    );
    assert_eq!(
        map_get(&term, ":kind"),
        Some(&Term::symbol(":editor/vcs-blame-panel"))
    );
    assert_eq!(
        map_get(&term, ":commit"),
        Some(&Term::Str("c-h".to_string()))
    );
    assert_eq!(map_get(&term, ":sym"), Some(&Term::symbol("pkg/mod::x")));
}

#[test]
fn editor_vcs_why_panel_projects_context_fields() {
    let term = eval_to_term(
        r#"
        (core/editor/vcs::why-panel-from-response
          (quote
            {
              :commit "c-h"
              :snapshot "s-h"
              :sym pkg/mod::x
              :value "v-h"
              :message "updated"
              :why "because"
              :evidence ["e1"]
              :obligations [core/obligation::unit-tests]
            }))
        "#,
    );
    assert_eq!(
        map_get(&term, ":kind"),
        Some(&Term::symbol(":editor/vcs-why-panel"))
    );
    assert_eq!(
        map_get(&term, ":message"),
        Some(&Term::Str("updated".to_string()))
    );
    let Some(Term::Vector(ev)) = map_get(&term, ":evidence") else {
        panic!("panel :evidence must be vector");
    };
    assert_eq!(ev.len(), 1);
}
