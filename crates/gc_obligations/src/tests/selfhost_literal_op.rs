use super::*;

#[test]
fn selfhost_literal_op_and_flatten_app_detect_quoted_effect_op() {
    let mut ctx = EvalCtx::with_step_limit(None);
    let prelude = build_prelude(&mut ctx);
    let mut env = prelude.env;
    let artifact = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("../..")
        .join("selfhost/toolchain.gc");
    load_selfhost_coreform_toolchain_v1_with_mode(
        &mut ctx,
        &mut env,
        SelfhostBootstrapMode::ArtifactOnly,
        Some(&artifact),
    )
    .expect("load selfhost toolchain");

    let forms = canonicalize_module(parse_module("(((core/effect::perform (quote io/fs::write)) {:data \"x\" :path \"out.txt\"}) (fn (_) (core/effect::pure nil)))\n").unwrap())
            .expect("canonical module");
    let app = forms.first().expect("one form").clone();
    let app_items = app.as_proper_list().expect("app proper list");
    let inner = app_items[0].clone();
    let inner_debug = format!("{inner:?}");

    let flatten = env
        .get("core/cli::flatten-app")
        .expect("flatten-app binding");
    let flat_v = flatten
        .clone()
        .apply(&mut ctx, Value::data(app.clone()))
        .expect("flatten apply");
    let flat_t = flat_v.to_term_for_log(ctx.protocol.map(|p| p.error));
    let flat_map = match flat_t {
        Term::Map(m) => m,
        other => panic!("flatten-app returned non-map: {}", print_term(&other)),
    };
    let args = match flat_map.get(&TermOrdKey(Term::symbol(":args"))) {
        Some(Term::Vector(v)) => v.clone(),
        other => panic!("flatten-app args missing/non-vector: {:?}", other),
    };
    let args_debug = format!("{args:?}");
    assert_eq!(args.len(), 3, "flatten-app args length mismatch");

    let lit = env
        .get("core/cli::literal-op-sym-or-nil")
        .expect("literal-op binding");
    let mut found = false;
    let mut debug_rows: Vec<String> = Vec::new();
    let app_render = print_term(&app);
    let inner_render = print_term(&inner);
    let flat_render = print_term(&Term::Map(flat_map.clone()));
    let flat_inner_v = flatten
        .clone()
        .apply(&mut ctx, Value::data(inner))
        .expect("flatten inner apply");
    let flat_inner_t = flat_inner_v.to_term_for_log(ctx.protocol.map(|p| p.error));
    let flat_inner_render = print_term(&flat_inner_t);
    for arg in args {
        let arg_render = print_term(&arg);
        let op_v = lit
            .clone()
            .apply(&mut ctx, Value::data(arg))
            .expect("literal-op apply");
        let op_t = op_v.to_term_for_log(ctx.protocol.map(|p| p.error));
        debug_rows.push(format!("{arg_render} => {}", print_term(&op_t)));
        if let Term::Symbol(s) = op_t
            && s == "io/fs::write"
        {
            found = true;
        }
    }
    assert!(
        found,
        "literal-op-sym-or-nil failed to detect io/fs::write; app={} inner={} inner_debug={} flat={} flat_inner={} args_debug={} rows={}",
        app_render,
        inner_render,
        inner_debug,
        flat_render,
        flat_inner_render,
        args_debug,
        debug_rows.join(" | ")
    );
}
