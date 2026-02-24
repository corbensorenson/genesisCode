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
