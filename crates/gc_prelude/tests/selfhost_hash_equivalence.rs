use gc_coreform::{canonicalize_module, parse_module};
use gc_kernel::{EvalCtx, Value, eval_module};
use gc_prelude::build_prelude;

const PRINTER_MODULES: [&str; 4] = [
    "selfhost/printer/00_core_single_line.gc",
    "selfhost/printer/01_single_line_list.gc",
    "selfhost/printer/02_fmt_structured.gc",
    "selfhost/printer/03_fmt_list_module.gc",
];

fn repo_root() -> std::path::PathBuf {
    std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../..")
        .canonicalize()
        .expect("canonical repo root")
}

fn selfhost_printer_src(root: &std::path::Path) -> String {
    let mut out = String::new();
    for module in PRINTER_MODULES {
        let src = std::fs::read_to_string(root.join(module))
            .unwrap_or_else(|e| panic!("read {}: {e}", module));
        out.push_str(&src);
        out.push('\n');
    }
    out
}

fn bytes32_hex(h: [u8; 32]) -> String {
    blake3::Hash::from_bytes(h).to_hex().to_string()
}

#[test]
fn selfhost_hash_matches_rust_for_terms_and_modules() {
    let root = repo_root();
    let hash_path = root.join("selfhost/hash.gc");
    let printer_src = selfhost_printer_src(&root);
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
        Some(v) => match v.as_data() {
            Some(gc_coreform::Term::Str(s)) => s.clone(),
            _ => panic!("expected :t string, got {v:?}"),
        },
        other => panic!("expected :t string, got {other:?}"),
    };
    let mod_h = match m.get(&gc_coreform::TermOrdKey(gc_coreform::Term::symbol(":m"))) {
        Some(v) => match v.as_data() {
            Some(gc_coreform::Term::Str(s)) => s.clone(),
            _ => panic!("expected :m string, got {v:?}"),
        },
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
