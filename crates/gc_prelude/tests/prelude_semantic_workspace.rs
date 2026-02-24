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

fn conflict_codes(term: &Term) -> Vec<String> {
    let Some(Term::Vector(conflicts)) = map_get(term, ":conflicts") else {
        panic!(":conflicts must be vector");
    };
    let mut codes = Vec::new();
    for entry in conflicts {
        let Term::Map(m) = entry else {
            panic!("conflict entry must be map");
        };
        let Some(Term::Str(code)) = m.get(&TermOrdKey(Term::symbol(":code"))) else {
            panic!("conflict entry missing :code");
        };
        codes.push(code.clone());
    }
    codes
}

#[test]
fn semantic_workspace_graph_contract_projects_duplicates_edges_and_unresolved() {
    let term = eval_to_term(
        r#"
        (core/cli::semantic-workspace-graph-analyze
          (quote
            {
              :owners
                [
                  {:symbol "pkg/a::foo" :module-paths ["a.gc"]}
                  {:symbol "pkg/a::dup" :module-paths ["a.gc" "b.gc"]}
                ]
              :occurrences
                [
                  {:module-path "b.gc" :symbol "pkg/a::foo"}
                  {:module-path "a.gc" :symbol "pkg/a::foo"}
                  {:module-path "b.gc" :symbol "pkg/a::dup"}
                  {:module-path "b.gc" :symbol "pkg/a::missing"}
                ]
            }))
        "#,
    );

    let Some(Term::Vector(dups)) = map_get(&term, ":duplicate-symbol-owners") else {
        panic!(":duplicate-symbol-owners must be vector");
    };
    assert_eq!(dups.len(), 1);
    let Some(Term::Map(dup_entry)) = dups.first() else {
        panic!("duplicate entry must be map");
    };
    assert_eq!(
        dup_entry.get(&TermOrdKey(Term::symbol(":symbol"))),
        Some(&Term::Str("pkg/a::dup".to_string()))
    );

    let Some(Term::Vector(edges)) = map_get(&term, ":edge-events") else {
        panic!(":edge-events must be vector");
    };
    assert_eq!(edges.len(), 1);
    let Some(Term::Map(edge)) = edges.first() else {
        panic!("edge entry must be map");
    };
    assert_eq!(
        edge.get(&TermOrdKey(Term::symbol(":from-module"))),
        Some(&Term::Str("b.gc".to_string()))
    );
    assert_eq!(
        edge.get(&TermOrdKey(Term::symbol(":to-module"))),
        Some(&Term::Str("a.gc".to_string()))
    );
    assert_eq!(
        edge.get(&TermOrdKey(Term::symbol(":symbol"))),
        Some(&Term::Str("pkg/a::foo".to_string()))
    );

    let Some(Term::Vector(unresolved)) = map_get(&term, ":unresolved-symbols") else {
        panic!(":unresolved-symbols must be vector");
    };
    assert_eq!(
        unresolved,
        &vec![
            Term::Str("pkg/a::dup".to_string()),
            Term::Str("pkg/a::missing".to_string()),
        ]
    );
}

#[test]
fn semantic_refactor_validate_contract_accepts_valid_payload() {
    let term = eval_to_term(
        r#"
        (core/cli::semantic-refactor-validate
          (quote
            {
              :kind "rename"
              :from-symbol "my/pkg::foo"
              :to-symbol "my/pkg::foo_v2"
            }))
        "#,
    );
    assert_eq!(
        map_get(&term, ":kind"),
        Some(&Term::Str(
            "genesis/semantic-refactor-validation-v0.1".to_string()
        ))
    );
    assert_eq!(map_get(&term, ":ok"), Some(&Term::Bool(true)));
    assert_eq!(
        map_get(&term, ":conflicts"),
        Some(&Term::Vector(Vec::new()))
    );
}

#[test]
fn semantic_refactor_validate_contract_emits_expected_conflicts() {
    let term = eval_to_term(
        r#"
        (core/cli::semantic-refactor-validate
          (quote
            {
              :kind "oops"
              :from-symbol ":kw"
              :to-symbol ":kw"
            }))
        "#,
    );
    assert_eq!(map_get(&term, ":ok"), Some(&Term::Bool(false)));
    let codes = conflict_codes(&term);
    assert_eq!(
        codes,
        vec![
            "refactor/kind-invalid".to_string(),
            "refactor/symbol-keyword-forbidden".to_string(),
            "refactor/no-op".to_string(),
        ]
    );
}

#[test]
fn semantic_refactor_plan_conflicts_contract_accepts_clean_payload() {
    let term = eval_to_term(
        r#"
        (core/cli::semantic-refactor-plan-conflicts
          (quote
            {
              :from-symbol "my/pkg::foo"
              :to-symbol "my/pkg::foo_v2"
              :from-def-modules ["a.gc"]
              :to-def-module ""
              :to-def-path-repr ""
            }))
        "#,
    );
    assert_eq!(
        map_get(&term, ":kind"),
        Some(&Term::Str(
            "genesis/semantic-refactor-plan-conflicts-v0.1".to_string()
        ))
    );
    assert_eq!(map_get(&term, ":ok"), Some(&Term::Bool(true)));
    assert_eq!(
        map_get(&term, ":conflicts"),
        Some(&Term::Vector(Vec::new()))
    );
}

#[test]
fn semantic_refactor_plan_conflicts_contract_emits_expected_codes() {
    let term = eval_to_term(
        r#"
        (core/cli::semantic-refactor-plan-conflicts
          (quote
            {
              :from-symbol "my/pkg::foo"
              :to-symbol "my/pkg::bar"
              :from-def-modules ["a.gc" "b.gc"]
              :to-def-module "dest.gc"
              :to-def-path-repr "(def my/pkg::bar ...)"
            }))
        "#,
    );
    assert_eq!(map_get(&term, ":ok"), Some(&Term::Bool(false)));
    let codes = conflict_codes(&term);
    assert_eq!(
        codes,
        vec![
            "refactor/source-symbol-ambiguous".to_string(),
            "refactor/destination-symbol-exists".to_string(),
        ]
    );
}

#[test]
fn semantic_refactor_target_conflicts_contract_requires_target_for_move_extract() {
    let term = eval_to_term(
        r#"
        (core/cli::semantic-refactor-target-conflicts
          (quote
            {
              :kind "move"
              :target-module-path ""
              :target-module-valid true
              :target-module-exists false
            }))
        "#,
    );
    assert_eq!(map_get(&term, ":ok"), Some(&Term::Bool(false)));
    let codes = conflict_codes(&term);
    assert_eq!(codes, vec!["refactor/target-module-required".to_string()]);
}

#[test]
fn semantic_refactor_target_conflicts_contract_emits_exists_for_existing_target() {
    let term = eval_to_term(
        r#"
        (core/cli::semantic-refactor-target-conflicts
          (quote
            {
              :kind "extract"
              :target-module-path "dest.gc"
              :target-module-valid true
              :target-module-exists true
            }))
        "#,
    );
    assert_eq!(map_get(&term, ":ok"), Some(&Term::Bool(false)));
    let codes = conflict_codes(&term);
    assert_eq!(codes, vec!["refactor/target-module-exists".to_string()]);
}

#[test]
fn semantic_refactor_target_conflicts_contract_emits_invalid_path_conflict() {
    let term = eval_to_term(
        r#"
        (core/cli::semantic-refactor-target-conflicts
          (quote
            {
              :kind "move"
              :target-module-path "../dest.gc"
              :target-module-valid false
              :target-module-exists false
            }))
        "#,
    );
    assert_eq!(map_get(&term, ":ok"), Some(&Term::Bool(false)));
    let codes = conflict_codes(&term);
    assert_eq!(codes, vec!["refactor/target-module-invalid".to_string()]);
}
