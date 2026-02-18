use gc_coreform::{Term, TermOrdKey, canonicalize_module, hash_module, parse_module, print_term};
use gc_prelude::selfhost_coreform_toolchain_v1_sources;

fn main() {
    let modules = selfhost_coreform_toolchain_v1_sources()
        .iter()
        .map(|(path, src)| {
            let forms = canonicalize_module(parse_module(src).expect("parse selfhost module"))
                .expect("canonicalize selfhost module");
            let h = hash_module(&forms);
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

    print!("{}", print_term(&artifact));
}
