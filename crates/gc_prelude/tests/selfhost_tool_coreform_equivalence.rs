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

#[test]
fn selfhost_coreform_tool_fmt_and_hash_match_rust_bootstrap_api() {
    let root = repo_root();
    let parse_path = root.join("selfhost/parse.gc");
    let parse_core_path = root.join("selfhost/parse_core_v1.gc");
    let canon_path = root.join("selfhost/canon.gc");
    let hash_path = root.join("selfhost/hash.gc");
    let tool_path = root.join("selfhost/tool_coreform_v1.gc");

    let parse_src = std::fs::read_to_string(&parse_path).expect("read parse");
    let parse_core_src = std::fs::read_to_string(&parse_core_path).expect("read parse_core");
    let printer_src = selfhost_printer_src(&root);
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
{parse}
{parse_core}
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
        parse = parse_src,
        printer = printer_src,
        canon = canon_src,
        hash = hash_src,
        tool = tool_src,
        s = src_escaped,
        parse_core = parse_core_src,
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
        Some(v) => match v.as_data() {
            Some(gc_coreform::Term::Str(s)) => s.clone(),
            _ => panic!("expected :fmt_rust string, got {v:?}"),
        },
        other => panic!("expected :fmt_rust string, got {other:?}"),
    };
    let fmt_sh = match m.get(&gc_coreform::TermOrdKey(gc_coreform::Term::symbol(
        ":fmt_sh",
    ))) {
        Some(v) => match v.as_data() {
            Some(gc_coreform::Term::Str(s)) => s.clone(),
            _ => panic!("expected :fmt_sh string, got {v:?}"),
        },
        other => panic!("expected :fmt_sh string, got {other:?}"),
    };
    let hash_rust = match m.get(&gc_coreform::TermOrdKey(gc_coreform::Term::symbol(
        ":hash_rust",
    ))) {
        Some(v) => match v.as_data() {
            Some(gc_coreform::Term::Str(s)) => s.clone(),
            _ => panic!("expected :hash_rust string, got {v:?}"),
        },
        other => panic!("expected :hash_rust string, got {other:?}"),
    };
    let hash_sh = match m.get(&gc_coreform::TermOrdKey(gc_coreform::Term::symbol(
        ":hash_sh",
    ))) {
        Some(v) => match v.as_data() {
            Some(gc_coreform::Term::Str(s)) => s.clone(),
            _ => panic!("expected :hash_sh string, got {v:?}"),
        },
        other => panic!("expected :hash_sh string, got {other:?}"),
    };

    assert_eq!(fmt_sh, fmt_rust);
    assert_eq!(hash_sh, hash_rust);
}
