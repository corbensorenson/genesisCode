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
