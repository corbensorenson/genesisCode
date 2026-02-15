use gc_coreform::{canonicalize_module, parse_module};
use gc_kernel::{EvalCtx, Value, eval_module};
use gc_prelude::build_prelude;

#[test]
fn selfhost_coreform_tool_fmt_and_hash_match_rust_bootstrap_api() {
    let printer_path =
        std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../selfhost/printer.gc");
    let canon_path =
        std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../selfhost/canon.gc");
    let hash_path = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../selfhost/hash.gc");
    let tool_path =
        std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../selfhost/tool_coreform_v0.gc");

    let printer_src = std::fs::read_to_string(&printer_path).expect("read printer");
    let canon_src = std::fs::read_to_string(&canon_path).expect("read canon");
    let hash_src = std::fs::read_to_string(&hash_path).expect("read hash");
    let tool_src = std::fs::read_to_string(&tool_path).expect("read tool");

    let messy = r#"
      (def   x   1)
      (def y (fn (a b) a))
      (y x 2)
    "#;
    let src_escaped = messy
        .replace('\\', "\\\\")
        .replace('"', "\\\"")
        .replace('\n', "\\n");

    let src = format!(
        r#"
{printer}
{canon}
{hash}
{tool}

{{
  :fmt_rust (core/coreform::fmt-module "{s}")
  :fmt_sh (selfhost/tool::fmt-module "{s}")
  :hash_rust (core/coreform::hash-module-src "{s}")
  :hash_sh (selfhost/tool::hash-module-src "{s}")
}}
        "#,
        printer = printer_src,
        canon = canon_src,
        hash = hash_src,
        tool = tool_src,
        s = src_escaped,
    );

    let forms = canonicalize_module(parse_module(&src).expect("parse")).expect("canon");
    let mut ctx = EvalCtx::new();
    let prelude = build_prelude(&mut ctx);
    let mut env = prelude.env;
    let v = eval_module(&mut ctx, &mut env, &forms).expect("eval");

    let Value::Map(m) = v else {
        panic!("expected map, got {}", v.debug_repr());
    };
    let fmt_rust = match m.get(&gc_coreform::TermOrdKey(gc_coreform::Term::symbol(
        ":fmt_rust",
    ))) {
        Some(Value::Data(gc_coreform::Term::Str(s))) => s.clone(),
        other => panic!("expected :fmt_rust string, got {other:?}"),
    };
    let fmt_sh = match m.get(&gc_coreform::TermOrdKey(gc_coreform::Term::symbol(
        ":fmt_sh",
    ))) {
        Some(Value::Data(gc_coreform::Term::Str(s))) => s.clone(),
        other => panic!("expected :fmt_sh string, got {other:?}"),
    };
    let hash_rust = match m.get(&gc_coreform::TermOrdKey(gc_coreform::Term::symbol(
        ":hash_rust",
    ))) {
        Some(Value::Data(gc_coreform::Term::Str(s))) => s.clone(),
        other => panic!("expected :hash_rust string, got {other:?}"),
    };
    let hash_sh = match m.get(&gc_coreform::TermOrdKey(gc_coreform::Term::symbol(
        ":hash_sh",
    ))) {
        Some(Value::Data(gc_coreform::Term::Str(s))) => s.clone(),
        other => panic!("expected :hash_sh string, got {other:?}"),
    };

    assert_eq!(fmt_sh, fmt_rust);
    assert_eq!(hash_sh, hash_rust);
}
