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
