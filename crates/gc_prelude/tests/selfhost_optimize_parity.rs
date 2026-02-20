use std::collections::BTreeMap;

use gc_coreform::{Term, TermOrdKey, canonicalize_module, parse_module};
use gc_kernel::{Apply, EvalCtx, Value};
use gc_prelude::{
    build_prelude, load_selfhost_coreform_toolchain_v1_from_artifact_source,
    selfhost_coreform_toolchain_v1_sources,
};

fn build_selfhost_artifact_source() -> String {
    let modules = selfhost_coreform_toolchain_v1_sources()
        .expect("load selfhost toolchain sources")
        .iter()
        .map(|(path, src)| {
            let forms = canonicalize_module(parse_module(src).expect("parse module source"))
                .expect("canonicalize module source");
            let h = gc_coreform::hash_module(&forms);
            Term::Map(
                [
                    (TermOrdKey(Term::symbol(":path")), Term::Str(path.clone())),
                    (TermOrdKey(Term::symbol(":source")), Term::Str(src.clone())),
                    (
                        TermOrdKey(Term::symbol(":forms")),
                        Term::Vector(forms.clone()),
                    ),
                    (
                        TermOrdKey(Term::symbol(":module-h")),
                        Term::Bytes(h.to_vec().into()),
                    ),
                    (TermOrdKey(Term::symbol(":stage1-ok")), Term::Bool(true)),
                    (
                        TermOrdKey(Term::symbol(":stage2-supported")),
                        Term::Bool(false),
                    ),
                    (TermOrdKey(Term::symbol(":stage2-ok")), Term::Bool(false)),
                ]
                .into_iter()
                .collect::<BTreeMap<_, _>>(),
            )
        })
        .collect();

    let artifact = Term::Map(
        [
            (
                TermOrdKey(Term::symbol(":kind")),
                Term::Str("genesis/selfhost-toolchain-artifact-v0.2".to_string()),
            ),
            (TermOrdKey(Term::symbol(":v")), Term::Int(1.into())),
            (TermOrdKey(Term::symbol(":ok")), Term::Bool(true)),
            (TermOrdKey(Term::symbol(":modules")), Term::Vector(modules)),
        ]
        .into_iter()
        .collect::<BTreeMap<_, _>>(),
    );
    gc_coreform::print_term(&artifact)
}

fn selfhost_optimize(forms: &[Term]) -> Vec<Term> {
    let mut ctx = EvalCtx::new();
    let prelude = build_prelude(&mut ctx);
    let mut env = prelude.env;
    let artifact = build_selfhost_artifact_source();
    load_selfhost_coreform_toolchain_v1_from_artifact_source(&mut ctx, &mut env, &artifact)
        .expect("load selfhost toolchain");

    let optimize = env
        .get("core/cli::optimize-module")
        .expect("missing core/cli::optimize-module");
    let out = optimize
        .apply(&mut ctx, Value::Data(Term::Vector(forms.to_vec())))
        .expect("selfhost optimize-module apply");
    let Some(Term::Vector(v)) = out.as_data() else {
        panic!("optimize-module returned non-vector: {}", out.debug_repr());
    };
    v.clone()
}

#[test]
fn selfhost_core_cli_optimize_matches_gc_opt_stage1_pipeline() {
    let cases = [
        (
            "const-fold-and-identities",
            r#"
              (def x (prim int/add 40 2))
              (def y (prim int/mul x 1))
              (def z (prim int/add y 0))
              z
            "#,
        ),
        (
            "if-and-begin-simplify",
            r#"
              (def a (if true (begin 1) (begin 2)))
              (def b (if false 10 20))
              (prim int/add a b)
            "#,
        ),
        (
            "let-and-nested-apps",
            r#"
              (let
                ((x (prim int/add 1 2))
                 (y (prim int/mul 3 4)))
                (prim int/sub (prim int/add x y) 0))
            "#,
        ),
        (
            "effectful-left-opaque",
            r#"
              (def p
                (core/effect::perform
                  'sys/time::now
                  {}
                  (fn (t) (core/effect::pure t))))
              p
            "#,
        ),
        (
            "pkg-basic-fixture",
            include_str!("../../../tests/spec/pkg_basic/basic.gc"),
        ),
    ];

    for (name, src) in cases {
        let forms =
            canonicalize_module(parse_module(src).expect("parse case")).expect("canonicalize case");
        let rust_stage1 = gc_opt::stage1_pipeline(&forms)
            .expect("stage1 pipeline")
            .transformed_forms;
        let selfhost_stage1 = selfhost_optimize(&forms);
        assert_eq!(
            selfhost_stage1, rust_stage1,
            "selfhost optimizer parity mismatch for case {name}"
        );
    }
}
