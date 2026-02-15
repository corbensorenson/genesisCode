use gc_coreform::{canonicalize_module, parse_module};
use gc_kernel::{EvalCtx, Value, eval_module};
use gc_prelude::build_prelude;

fn bytes32_hex(h: [u8; 32]) -> String {
    blake3::Hash::from_bytes(h).to_hex().to_string()
}

#[test]
fn selfhost_hash_matches_rust_for_terms_and_modules() {
    let printer_path =
        std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../selfhost/printer.gc");
    let hash_path = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../selfhost/hash.gc");
    let printer_src = std::fs::read_to_string(&printer_path).expect("read printer");
    let hash_src = std::fs::read_to_string(&hash_path).expect("read hash");

    let src = format!(
        r#"
{printer}
{hash}

{{
  :t (selfhost/hash::hash-term (quote (1 2 3)))
  :m (selfhost/hash::hash-module [(def x 1) (prim int/add x 2)])
}}
        "#,
        printer = printer_src,
        hash = hash_src
    );

    let forms = canonicalize_module(parse_module(&src).expect("parse")).expect("canon");
    let mut ctx = EvalCtx::new();
    let prelude = build_prelude(&mut ctx);
    let mut env = prelude.env;
    let v = eval_module(&mut ctx, &mut env, &forms).expect("eval");

    let Value::Map(m) = v else {
        panic!("expected map, got {}", v.debug_repr());
    };

    let term_h = match m.get(&gc_coreform::TermOrdKey(gc_coreform::Term::symbol(":t"))) {
        Some(Value::Data(gc_coreform::Term::Str(s))) => s.clone(),
        other => panic!("expected :t string, got {other:?}"),
    };
    let mod_h = match m.get(&gc_coreform::TermOrdKey(gc_coreform::Term::symbol(":m"))) {
        Some(Value::Data(gc_coreform::Term::Str(s))) => s.clone(),
        other => panic!("expected :m string, got {other:?}"),
    };

    let want_term = bytes32_hex(gc_coreform::hash_term(&gc_coreform::Term::list(vec![
        gc_coreform::Term::Int(1.into()),
        gc_coreform::Term::Int(2.into()),
        gc_coreform::Term::Int(3.into()),
    ])));

    let want_mod = {
        let ms = r#"
          (def x 1)
          (prim int/add x 2)
        "#;
        let mf = canonicalize_module(parse_module(ms).unwrap()).unwrap();
        bytes32_hex(gc_coreform::hash_module(&mf))
    };

    assert_eq!(term_h, want_term);
    assert_eq!(mod_h, want_mod);
}
