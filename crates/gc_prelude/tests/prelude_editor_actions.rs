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

fn vec_has_sym(t: &Term, want: &str) -> bool {
    let Term::Vector(items) = t else { return false };
    items
        .iter()
        .any(|it| matches!(it, Term::Symbol(s) if s == want))
}

fn vec_has_str(t: &Term, want: &str) -> bool {
    let Term::Vector(items) = t else { return false };
    items
        .iter()
        .any(|it| matches!(it, Term::Str(s) if s == want))
}

#[test]
fn editor_action_format_source_success() {
    let term = eval_to_term(
        r#"
        (core/editor/action::format-source "(def x 1)")
        "#,
    );
    assert_eq!(
        map_get(&term, ":kind"),
        Some(&Term::symbol(":editor/coreform-format-result"))
    );
    assert_eq!(map_get(&term, ":ok"), Some(&Term::Bool(true)));
    let Some(Term::Str(formatted)) = map_get(&term, ":formatted") else {
        panic!("formatted output expected");
    };
    assert!(formatted.contains("(def x 1)"));
    let Some(Term::Str(hash)) = map_get(&term, ":hash") else {
        panic!("hash expected");
    };
    assert_eq!(hash.len(), 64);
}

#[test]
fn editor_action_format_source_parse_error_payload() {
    let term = eval_to_term(
        r#"
        (core/editor/action::format-source "(def x")
        "#,
    );
    assert_eq!(map_get(&term, ":ok"), Some(&Term::Bool(false)));
    assert_eq!(map_get(&term, ":formatted"), Some(&Term::Nil));
    let Some(Term::Map(err)) = map_get(&term, ":error") else {
        panic!("error payload map expected");
    };
    assert!(err.contains_key(&TermOrdKey(Term::symbol(":error/code"))));
}

#[test]
fn editor_pkg_status_panel_projects_missing_count() {
    let term = eval_to_term(
        r#"
        ((core/editor/pkg::status-panel-from-response (quote :editor/pkg-install-panel))
          (quote
            {
              :ok false
              :lock "genesis.lock"
              :missing ["h1" "h2"]
              :checked 3
            }))
        "#,
    );
    assert_eq!(
        map_get(&term, ":kind"),
        Some(&Term::symbol(":editor/pkg-install-panel"))
    );
    assert_eq!(map_get(&term, ":missing-count"), Some(&Term::Int(2.into())));
    assert_eq!(map_get(&term, ":checked"), Some(&Term::Int(3.into())));
}

#[test]
fn editor_action_parse_source_builds_ast_index() {
    let term = eval_to_term(
        r#"
        (core/editor/action::parse-source
          "(def ::meta (quote {:exports [my/mod::a] :types {my/mod::a ?}}))
(def my/mod::a 1)")
        "#,
    );
    assert_eq!(
        map_get(&term, ":kind"),
        Some(&Term::symbol(":editor/ast-module-index"))
    );
    assert_eq!(map_get(&term, ":ok"), Some(&Term::Bool(true)));
    assert_eq!(map_get(&term, ":form-count"), Some(&Term::Int(2.into())));
    let defs = map_get(&term, ":defs").expect("defs expected");
    assert!(vec_has_sym(defs, "::meta"));
    assert!(vec_has_sym(defs, "my/mod::a"));
    let exports = map_get(&term, ":exports").expect("exports expected");
    assert!(vec_has_sym(exports, "my/mod::a"));
}

#[test]
fn editor_action_parse_source_surfaces_error_payload() {
    let term = eval_to_term(
        r#"
        (core/editor/action::parse-source "(def broken")
        "#,
    );
    assert_eq!(
        map_get(&term, ":kind"),
        Some(&Term::symbol(":editor/ast-module-index"))
    );
    assert_eq!(map_get(&term, ":ok"), Some(&Term::Bool(false)));
    let Some(Term::Map(err)) = map_get(&term, ":error") else {
        panic!("error payload map expected");
    };
    assert!(err.contains_key(&TermOrdKey(Term::symbol(":error/code"))));
}

#[test]
fn editor_ast_changed_syms_tracks_defs_and_meta_changes() {
    let term = eval_to_term(
        r#"
        ((core/editor/ast::changed-syms
           (core/editor/ast::parse-module-index
             "(def ::meta (quote {:exports [my/mod::a] :types {my/mod::a ?}}))
(def my/mod::a 1)"))
          (core/editor/ast::parse-module-index
            "(def ::meta (quote {:exports [my/mod::b] :types {my/mod::b ?}}))
(def my/mod::b 2)"))
        "#,
    );
    assert!(vec_has_sym(&term, "my/mod::a"));
    assert!(vec_has_sym(&term, "my/mod::b"));
    assert!(vec_has_sym(&term, "::meta"));
}

#[test]
fn editor_plugin_caps_policy_is_deny_by_default() {
    let term = eval_to_term(
        r#"
        {
          :allow_get ((core/editor/plugin::caps-allowed? {:allow [core/store::get] :deny []}) (quote core/store::get))
          :deny_put ((core/editor/plugin::caps-allowed? {:allow [core/store::get] :deny []}) (quote core/store::put))
          :deny_explicit ((core/editor/plugin::caps-allowed? {:allow [core/store::get] :deny [core/store::get]}) (quote core/store::get))
          :deny_empty ((core/editor/plugin::caps-allowed? {:allow [] :deny []}) (quote core/store::get))
        }
        "#,
    );
    assert_eq!(map_get(&term, ":allow_get"), Some(&Term::Bool(true)));
    assert_eq!(map_get(&term, ":deny_put"), Some(&Term::Bool(false)));
    assert_eq!(map_get(&term, ":deny_explicit"), Some(&Term::Bool(false)));
    assert_eq!(map_get(&term, ":deny_empty"), Some(&Term::Bool(false)));
}

#[test]
fn editor_agent_session_log_artifact_is_stable_shape() {
    let term = eval_to_term(
        r#"
        (core/editor/agent::session-log-artifact
          ((core/editor/agent::session-add-event
             (core/editor/agent::session-empty "agent-1"))
            {:kind (quote :plan) :msg "hello"}))
        "#,
    );
    assert_eq!(
        map_get(&term, ":kind"),
        Some(&Term::symbol("genesis/editor-agent-session-v0.2"))
    );
    assert_eq!(map_get(&term, ":v"), Some(&Term::Int(1.into())));
    let Some(Term::Str(h)) = map_get(&term, ":session-h") else {
        panic!("session hash expected");
    };
    assert_eq!(h.len(), 64);
}

#[test]
fn editor_agent_acceptance_report_tracks_verify_gate() {
    let term = eval_to_term(
        r#"
        {
          :accepted
            ((core/editor/agent::acceptance-report {:ok true :snapshot "s"}) {:ok true})
          :rejected
            ((core/editor/agent::acceptance-report {:ok true :snapshot "s"}) {:ok false})
        }
        "#,
    );
    let Some(Term::Map(accepted)) = map_get(&term, ":accepted") else {
        panic!("accepted report expected");
    };
    assert_eq!(
        accepted.get(&TermOrdKey(Term::symbol(":accepted"))),
        Some(&Term::Bool(true))
    );
    let Some(Term::Map(rejected)) = map_get(&term, ":rejected") else {
        panic!("rejected report expected");
    };
    assert_eq!(
        rejected.get(&TermOrdKey(Term::symbol(":accepted"))),
        Some(&Term::Bool(false))
    );
}

#[test]
fn editor_agent_semantic_patch_plan_reports_changed_symbols() {
    let term = eval_to_term(
        r#"
        ((((core/editor/agent::semantic-patch-plan "base-h") "to-h")
           "(def ::meta (quote {:exports [my/mod::a] :types {my/mod::a ?}}))
(def my/mod::a 1)")
          "(def ::meta (quote {:exports [my/mod::b] :types {my/mod::b ?}}))
(def my/mod::b 2)")
        "#,
    );
    assert_eq!(
        map_get(&term, ":kind"),
        Some(&Term::symbol(":editor/agent-semantic-patch-plan"))
    );
    assert_eq!(map_get(&term, ":ok"), Some(&Term::Bool(true)));
    assert_eq!(map_get(&term, ":meta-changed"), Some(&Term::Bool(true)));
    let Some(Term::Int(changed)) = map_get(&term, ":changed-count") else {
        panic!("changed-count expected");
    };
    assert_eq!(changed.to_string(), "3");
    let changed = map_get(&term, ":changed-syms").expect("changed-syms expected");
    assert!(vec_has_sym(changed, "::meta"));
    assert!(vec_has_sym(changed, "my/mod::a"));
    assert!(vec_has_sym(changed, "my/mod::b"));
}

#[test]
fn editor_agent_conflict_resolution_plan_prefers_requested_side() {
    let term = eval_to_term(
        r#"
        (((core/editor/agent::conflict-resolution-plan "conflict-h")
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
               })))
          (quote :left))
        "#,
    );
    assert_eq!(
        map_get(&term, ":kind"),
        Some(&Term::symbol(":editor/agent-conflict-resolution-plan"))
    );
    assert_eq!(map_get(&term, ":strategy"), Some(&Term::symbol(":left")));
    assert_eq!(map_get(&term, ":count"), Some(&Term::Int(2.into())));
    let ops = map_get(&term, ":ops").expect("ops expected");
    assert!(vec_has_str(ops, "foo/a::x"));
    assert!(vec_has_str(ops, "foo/a::y"));
    let Some(Term::Map(resolutions)) = map_get(&term, ":resolutions") else {
        panic!("resolutions map expected");
    };
    assert_eq!(
        resolutions.get(&TermOrdKey(Term::symbol("foo/a::x"))),
        Some(&Term::symbol(":left"))
    );
    assert_eq!(
        resolutions.get(&TermOrdKey(Term::symbol("foo/a::y"))),
        Some(&Term::symbol(":left"))
    );
}

#[test]
fn editor_agent_repair_plan_prioritizes_autofix_and_verification() {
    let term = eval_to_term(
        r#"
        ((core/editor/agent::repair-plan
           {
             :accepted false
             :verify {:ok false}
           })
          {
            :error-count 2
            :warn-count 1
            :autofixes [{:module "m.gc" :patch "p1"}]
          })
        "#,
    );
    assert_eq!(
        map_get(&term, ":kind"),
        Some(&Term::symbol(":editor/agent-repair-plan"))
    );
    assert_eq!(map_get(&term, ":accepted"), Some(&Term::Bool(false)));
    assert_eq!(map_get(&term, ":verify-ok"), Some(&Term::Bool(false)));
    assert_eq!(map_get(&term, ":autofix-count"), Some(&Term::Int(1.into())));
    assert_eq!(map_get(&term, ":error-count"), Some(&Term::Int(2.into())));
    let Some(Term::Vector(steps)) = map_get(&term, ":steps") else {
        panic!("steps expected");
    };
    assert_eq!(steps.len(), 4);
    let first = steps.first().expect("first step");
    let Some(first_op) = map_get(first, ":op") else {
        panic!("step op expected");
    };
    assert_eq!(first_op, &Term::symbol(":apply-autofixes"));
}

#[test]
fn editor_action_gfx_plan_frame_trace_builds_explainable_plan() {
    let term = eval_to_term(
        r#"
        (def scene (core/gfx/scene::empty "editor-trace"))
        (def style ((((core/gfx/ui::style {:axis "vertical"}) {:w 400000 :h 200000}) {:gap 4000}) {:bg [0 0 0 1000000]}))
        (def label (((core/gfx/ui::text "title") "Editor") style))
        (def ui-root (((core/gfx/ui::container "root") style) ((core/vec::push []) label)))
        ((((((core/editor/action::gfx-plan-frame-2d+ui-trace scene) ui-root) "scene-pass") 101) "ui-pass") 202)
        "#,
    );
    assert_eq!(
        map_get(&term, ":kind"),
        Some(&Term::symbol(":gfx/plan-frame-2d+ui-trace"))
    );
    let Some(Term::Map(trace)) = map_get(&term, ":trace") else {
        panic!("trace map expected");
    };
    assert_eq!(
        trace.get(&TermOrdKey(Term::symbol(":kind"))),
        Some(&Term::symbol(":gfx/frame-trace"))
    );
    let Some(Term::Str(trace_h)) = map_get(&term, ":trace-h") else {
        panic!("trace-h expected");
    };
    assert_eq!(trace_h.len(), 64);
}
