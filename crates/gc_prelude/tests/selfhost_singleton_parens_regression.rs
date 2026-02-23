use gc_coreform::{canonicalize_module, parse_module};
use gc_kernel::{EvalCtx, Value, eval_module};
use gc_prelude::build_prelude;

#[test]
fn selfhost_canon_collapses_singleton_list_forms() {
    let parse_path =
        std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../selfhost/parse.gc");
    let parse_core_path =
        std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../selfhost/parse_core_v1.gc");
    let canon_path =
        std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../selfhost/canon.gc");

    let parse_src = std::fs::read_to_string(&parse_path).expect("read parse");
    let parse_core_src = std::fs::read_to_string(&parse_core_path).expect("read parse core");
    let canon_src = std::fs::read_to_string(&canon_path).expect("read canon");

    // This is the regression that historically diverged between selfhost and Rust fmt: `(y)` must
    // canonicalize to `y` since application is binary and singleton lists are just grouping.
    let program_src = r#"
      (def x 1)
      (def y (prim int/add x 2))
      (  y   )
    "#;
    let program_escaped = program_src
        .replace('\\', "\\\\")
        .replace('"', "\\\"")
        .replace('\n', "\\n");

    let src = format!(
        r#"
{parse}
{parse_core}
{canon}

(let ((forms (selfhost/parse::parse-module "{p}")))
  (let ((canon (selfhost/canon::canonicalize-module forms)))
    (let ((forms2 (core/vec::get forms 2)))
      (let ((canon2 (core/vec::get canon 2)))
        {{
          :forms2 forms2
          :forms2-tag (core/data::tag forms2)
          :canon2 canon2
          :canon2-tag (core/data::tag canon2)
        }}))))
        "#,
        parse = parse_src,
        parse_core = parse_core_src,
        canon = canon_src,
        p = program_escaped,
    );

    let forms = canonicalize_module(parse_module(&src).expect("parse")).expect("canon");
    let mut ctx = EvalCtx::new();
    let prelude = build_prelude(&mut ctx);
    let mut env = prelude.env;
    let v = eval_module(&mut ctx, &mut env, &forms).expect("eval");

    let Value::Map(m) = v else {
        panic!("expected map, got {}", v.debug_repr());
    };

    let forms2_tag = m
        .get(&gc_coreform::TermOrdKey(gc_coreform::Term::symbol(
            ":forms2-tag",
        )))
        .cloned()
        .unwrap_or(Value::Data(gc_coreform::Term::Nil));
    let canon2_tag = m
        .get(&gc_coreform::TermOrdKey(gc_coreform::Term::symbol(
            ":canon2-tag",
        )))
        .cloned()
        .unwrap_or(Value::Data(gc_coreform::Term::Nil));
    let canon2 = m
        .get(&gc_coreform::TermOrdKey(gc_coreform::Term::symbol(
            ":canon2",
        )))
        .cloned()
        .unwrap_or(Value::Data(gc_coreform::Term::Nil));

    let Value::Data(gc_coreform::Term::Symbol(ft)) = forms2_tag else {
        panic!(
            "expected :forms2-tag symbol, got {}",
            forms2_tag.debug_repr()
        );
    };
    assert_eq!(ft, ":pair", "expected forms2 to be a list form");

    let Value::Data(gc_coreform::Term::Symbol(ct)) = canon2_tag else {
        panic!(
            "expected :canon2-tag symbol, got {}",
            canon2_tag.debug_repr()
        );
    };
    assert_eq!(
        ct,
        ":sym",
        "expected canon2 to collapse to a symbol; canon2 = {}",
        canon2.debug_repr()
    );
}
