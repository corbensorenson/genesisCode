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

fn diag_has_code(diags: &Term, code: &str) -> bool {
    let Term::Vector(vs) = diags else {
        return false;
    };
    vs.iter().any(|d| {
        let Term::Map(m) = d else { return false };
        matches!(
            m.get(&TermOrdKey(Term::symbol(":code"))),
            Some(Term::Str(s)) if s == code
        )
    })
}

fn count_level(diags: &Term, level: &str) -> usize {
    let Term::Vector(vs) = diags else { return 0 };
    vs.iter()
        .filter(|d| {
            let Term::Map(m) = d else { return false };
            matches!(
                m.get(&TermOrdKey(Term::symbol(":level"))),
                Some(Term::Symbol(s)) if s == level
            )
        })
        .count()
}

fn diag_len(diags: &Term) -> usize {
    match diags {
        Term::Vector(vs) => vs.len(),
        _ => 0,
    }
}

#[test]
fn editor_lint_valid_module_has_no_errors() {
    let term = eval_to_term(
        r#"
        (core/editor/lint::lint-module
          "ok.gc"
          [
            (def ::meta (quote {:exports [pkg/ok::x] :types {pkg/ok::x Int}}))
            (def pkg/ok::x 1)
          ])
        "#,
    );
    assert_eq!(count_level(&term, ":error"), 0);
}

#[test]
fn editor_lint_reports_missing_meta() {
    let term = eval_to_term(
        r#"
        (core/editor/lint::lint-module
          "missing.gc"
          [
            (def pkg/missing::x 1)
          ])
        "#,
    );
    assert!(diag_has_code(&term, "editor/lint/missing-meta"));
    assert!(count_level(&term, ":error") >= 1);
}

#[test]
fn editor_lint_reports_export_missing_def() {
    let term = eval_to_term(
        r#"
        (core/editor/lint::lint-module
          "exp.gc"
          [
            (def ::meta (quote {:exports [pkg/exp::x] :types {pkg/exp::x Int}}))
            (def pkg/exp::y 1)
          ])
        "#,
    );
    assert!(diag_has_code(&term, "editor/lint/export-missing-def"));
    assert!(count_level(&term, ":error") >= 1);
}

#[test]
fn editor_lint_delta_filters_to_changed_symbols() {
    let term = eval_to_term(
        r#"
        (core/editor/lint::lint-module-delta
          "delta.gc"
          [
            (def ::meta (quote {:exports [pkg/delta::a pkg/delta::b] :types {pkg/delta::a Int pkg/delta::b Int}}))
            (def pkg/delta::a 1)
          ]
          [pkg/delta::a])
        "#,
    );
    assert_eq!(diag_len(&term), 0);
    assert!(!diag_has_code(&term, "editor/lint/export-missing-def"));
}

#[test]
fn editor_lint_delta_meta_change_returns_full_diagnostics() {
    let term = eval_to_term(
        r#"
        (core/editor/lint::lint-module-delta
          "delta.gc"
          [
            (def ::meta (quote {:exports [pkg/delta::a pkg/delta::b] :types {pkg/delta::a Int pkg/delta::b Int}}))
            (def pkg/delta::a 1)
          ]
          [::meta])
        "#,
    );
    assert!(diag_has_code(&term, "editor/lint/export-missing-def"));
}

#[test]
fn editor_lint_delta_preserves_global_missing_meta() {
    let term = eval_to_term(
        r#"
        (core/editor/lint::lint-module-delta
          "missing.gc"
          [
            (def pkg/missing::x 1)
          ]
          [pkg/missing::x])
        "#,
    );
    assert!(diag_has_code(&term, "editor/lint/missing-meta"));
}
