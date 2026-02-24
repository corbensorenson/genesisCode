use gc_kernel::{Apply, EvalCtx, Value};
use gc_prelude::{build_prelude, load_selfhost_coreform_toolchain_v1_from_artifact};
use gc_coreform::{Term, TermOrdKey, parse_term};
use std::collections::BTreeMap;

#[test]
fn diag_toolchain_decode() {
    let mut ctx = EvalCtx::new();
    let prelude = build_prelude(&mut ctx);
    let mut env = prelude.env;
    let path = std::path::Path::new("../../selfhost/toolchain.gc");
    if let Err(e) = load_selfhost_coreform_toolchain_v1_from_artifact(&mut ctx, &mut env, path) {
        panic!("decode failed: {e:#}");
    }

    let rename = env
        .get("core/cli::rename-symbol-forms")
        .expect("rename binding");

    let form = parse_term("(def foo (fn (x) foo))").expect("form parse");
    let mut req = BTreeMap::new();
    req.insert(
        TermOrdKey(Term::symbol(":forms")),
        Term::Vector(vec![form]),
    );
    req.insert(
        TermOrdKey(Term::symbol(":from")),
        Term::Symbol("foo".to_string()),
    );
    req.insert(
        TermOrdKey(Term::symbol(":to")),
        Term::Symbol("bar".to_string()),
    );
    let out = rename
        .apply(&mut ctx, Value::Data(Term::Map(req)))
        .expect("rename apply");
    let Value::Data(Term::Map(m)) = out else {
        panic!("rename out not map data: {}", out.debug_repr());
    };
    let forms = m
        .get(&TermOrdKey(Term::symbol(":forms")))
        .expect("forms present");
    assert!(
        matches!(forms, Term::Vector(_)),
        "forms not vector: {}",
        gc_coreform::print_term(forms)
    );
}
