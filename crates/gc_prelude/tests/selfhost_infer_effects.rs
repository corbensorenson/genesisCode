use std::collections::BTreeMap;

use gc_coreform::{Term, TermOrdKey, canonicalize_module, parse_module};
use gc_kernel::{EvalCtx, eval_module};
use gc_prelude::{
    build_prelude, load_selfhost_coreform_toolchain_v1_from_artifact_source,
    selfhost_coreform_toolchain_v1_sources,
};

fn build_selfhost_artifact_source() -> String {
    // Keep artifact building deterministic and in-tree; tests should not depend on workspace files.
    let modules = selfhost_coreform_toolchain_v1_sources()
        .iter()
        .map(|(path, src)| {
            let forms = canonicalize_module(parse_module(src).unwrap()).unwrap();
            let h = gc_coreform::hash_module(&forms);
            Term::Map(
                [
                    (
                        TermOrdKey(Term::symbol(":path")),
                        Term::Str(path.clone()),
                    ),
                    (
                        TermOrdKey(Term::symbol(":source")),
                        Term::Str(src.clone()),
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
                .collect(),
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
        .collect(),
    );
    gc_coreform::print_term(&artifact)
}

fn expected_effects(module_src: &str) -> Term {
    let forms = canonicalize_module(parse_module(module_src).expect("parse module"))
        .expect("canonicalize module");
    let eff = gc_types::infer_effects(&forms);

    let ops: Vec<Term> = eff.ops.iter().map(|s| Term::Symbol(s.clone())).collect();
    Term::Map(
        [
            (TermOrdKey(Term::symbol(":ops")), Term::Vector(ops)),
            (
                TermOrdKey(Term::symbol(":unknown")),
                Term::Bool(eff.unknown),
            ),
        ]
        .into_iter()
        .collect::<BTreeMap<_, _>>(),
    )
}

#[test]
fn selfhost_core_cli_infer_effects_matches_rust_infer_effects() {
    let module_src = r#"
      (def x
        (core/effect::perform
          'io/fs::read
          {:path "x"}
          (fn (_)
            (core/effect::pure nil))))
      (def y
        (core/effect::perform
          (if true 'io/fs::read 'io/fs::write)
          {:path "y"}
          (fn (_)
            (core/effect::pure nil))))
    "#;

    let tool_src = format!(
        r#"
          (def src "{src}")
          (def forms (selfhost/parse::parse-module src))
          (core/cli::infer-effects forms)
        "#,
        src = module_src
            .replace('\\', "\\\\")
            .replace('\"', "\\\"")
            .replace('\n', "\\n")
    );
    let tool_forms = canonicalize_module(parse_module(&tool_src).expect("parse tool"))
        .expect("canonicalize tool");

    let mut ctx = EvalCtx::new();
    let prelude = build_prelude(&mut ctx);
    let mut env = prelude.env;
    let artifact = build_selfhost_artifact_source();
    load_selfhost_coreform_toolchain_v1_from_artifact_source(&mut ctx, &mut env, &artifact)
        .expect("load selfhost toolchain");

    let v = eval_module(&mut ctx, &mut env, &tool_forms).expect("eval tool");
    let got = v.to_term_for_log(None);

    let want = expected_effects(module_src);
    assert_eq!(got, want, "selfhost infer-effects must match rust");
}
