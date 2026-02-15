use std::path::Path;

use gc_coreform::{
    Term, canonicalize_form, canonicalize_module, parse_module, parse_term, print_module,
    print_term,
};
use gc_kernel::{Apply, EvalCtx, Value, eval_module};
use gc_prelude::build_prelude;

#[test]
fn selfhost_printer_matches_rust_canonical_printer_for_terms_and_modules() {
    let printer_path = Path::new("/Users/corbensorenson/Documents/genesisCode/selfhost/printer.gc");
    let src = std::fs::read_to_string(printer_path).expect("read selfhost/printer.gc");

    let raw_forms = parse_module(&src).unwrap();
    for (i, f) in raw_forms.iter().enumerate() {
        if let Err(e) = canonicalize_form(f.clone()) {
            panic!(
                "selfhost/printer.gc canonicalize failed at form {i}: {e}\nform={}",
                print_term(f)
            );
        }
    }
    let forms = canonicalize_module(raw_forms).unwrap();
    let mut ctx = EvalCtx::new();
    let prelude = build_prelude(&mut ctx);
    let mut env = prelude.env;
    let _ = eval_module(&mut ctx, &mut env, &forms).unwrap();

    let print_term_fn = env
        .get("selfhost/printer::print-term")
        .expect("selfhost/printer::print-term bound");
    let print_module_fn = env
        .get("selfhost/printer::print-module")
        .expect("selfhost/printer::print-module bound");

    let term_cases = [
        "nil",
        "true",
        "false",
        "123",
        "\"a\\n\\\"b\\t\"",
        "\"\\u0001\"",
        "b\"\\x00\\xFF\"",
        "foo/bar::x",
        "(a b)",
        "[1 2 3]",
        "[1 [2 3] 4]",
        "{:b 2 :a 1}",
        "{:a {:b 2}}",
    ];
    for t_src in term_cases {
        let t = parse_term(t_src).unwrap();
        let got = print_term_fn
            .clone()
            .apply(&mut ctx, Value::Data(t.clone()))
            .unwrap();
        let Value::Data(Term::Str(got_s)) = got else {
            panic!(
                "print-term must return string datum for case {t_src}, got {}",
                got.debug_repr()
            );
        };
        let want_s = print_term(&t);
        assert_eq!(got_s, want_s, "term case: {t_src}");
    }

    let module_src = r#"
      (def x   1)
      (def y (fn (a b) a))
      (y x 2)
    "#;
    let module_forms = canonicalize_module(parse_module(module_src).unwrap()).unwrap();
    let module_term = Term::Vector(module_forms.clone());
    let got = print_module_fn
        .clone()
        .apply(&mut ctx, Value::Data(module_term))
        .unwrap();
    let Value::Data(Term::Str(got_s)) = got else {
        panic!(
            "print-module must return string datum, got {}",
            got.debug_repr()
        );
    };
    let want_s = print_module(&module_forms);
    assert_eq!(got_s, want_s);
}
