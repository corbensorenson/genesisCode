use std::collections::BTreeMap;

use gc_coreform::{Term, TermOrdKey, canonicalize_module, parse_module, parse_term, print_term};
use gc_kernel::EvalCtx;
use gc_prelude::{
    build_prelude, load_selfhost_coreform_toolchain_v1_from_artifact_source,
    selfhost_coreform_toolchain_v1_sources,
};

const TOOLCHAIN_MANIFEST_SRC: &str = include_str!("../../../selfhost/toolchain_manifest.gc");

fn map_get<'a>(m: &'a BTreeMap<TermOrdKey, Term>, k: &str) -> Option<&'a Term> {
    m.get(&TermOrdKey(Term::symbol(k)))
}

fn manifest_module_paths() -> Vec<String> {
    let term = parse_term(TOOLCHAIN_MANIFEST_SRC).expect("parse toolchain manifest");
    let Term::Map(root) = term else {
        panic!("toolchain manifest root must be map");
    };
    let Some(Term::Vector(paths)) = map_get(&root, ":module-paths") else {
        panic!("toolchain manifest missing :module-paths");
    };
    paths
        .iter()
        .map(|t| match t {
            Term::Str(s) => s.clone(),
            _ => panic!("toolchain manifest :module-paths entry must be string"),
        })
        .collect()
}

fn build_selfhost_artifact_source(modules: &[(String, String)]) -> String {
    let module_terms: Vec<Term> = modules
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
            (
                TermOrdKey(Term::symbol(":modules")),
                Term::Vector(module_terms),
            ),
        ]
        .into_iter()
        .collect::<BTreeMap<_, _>>(),
    );
    print_term(&artifact)
}

fn build_selfhost_artifact_source_without_forms(modules: &[(String, String)]) -> String {
    let module_terms: Vec<Term> = modules
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
            (
                TermOrdKey(Term::symbol(":modules")),
                Term::Vector(module_terms),
            ),
        ]
        .into_iter()
        .collect::<BTreeMap<_, _>>(),
    );
    print_term(&artifact)
}

#[test]
fn toolchain_sources_follow_manifest_paths_and_order() {
    let expected_paths = manifest_module_paths();
    let actual_paths: Vec<String> = selfhost_coreform_toolchain_v1_sources()
        .expect("load selfhost toolchain sources")
        .into_iter()
        .map(|(path, _)| path)
        .collect();
    assert_eq!(actual_paths, expected_paths);
}

#[test]
fn artifact_loader_enforces_manifest_required_symbols() {
    let modules =
        selfhost_coreform_toolchain_v1_sources().expect("load selfhost toolchain sources");
    let mutated: Vec<(String, String)> = modules
        .into_iter()
        .map(|(path, src)| {
            if path == "selfhost/cli_coreform_v1.gc" {
                let needle = "(def core/cli::canonicalize-module-src";
                let replacement = "(def core/cli::canonicalize-module-src-missing";
                assert!(
                    src.contains(needle),
                    "expected symbol definition not found in {path}"
                );
                (path, src.replacen(needle, replacement, 1))
            } else {
                (path, src)
            }
        })
        .collect();

    let artifact = build_selfhost_artifact_source(&mutated);
    let mut ctx = EvalCtx::new();
    let mut env = build_prelude(&mut ctx).env;
    let err =
        load_selfhost_coreform_toolchain_v1_from_artifact_source(&mut ctx, &mut env, &artifact)
            .expect_err("artifact load should fail when manifest required symbol is missing");
    let msg = format!("{err:#}");
    assert!(
        msg.contains(
            "artifact missing required manifest symbol: core/cli::canonicalize-module-src"
        ),
        "unexpected error: {msg}"
    );
}

#[test]
fn artifact_loader_rejects_source_only_modules_in_production_profile() {
    let modules =
        selfhost_coreform_toolchain_v1_sources().expect("load selfhost toolchain sources");
    let artifact = build_selfhost_artifact_source_without_forms(&modules);
    let mut ctx = EvalCtx::new();
    let mut env = build_prelude(&mut ctx).env;
    let err =
        load_selfhost_coreform_toolchain_v1_from_artifact_source(&mut ctx, &mut env, &artifact)
            .expect_err("production profile should reject source-only artifact modules");
    let msg = format!("{err:#}");
    assert!(
        msg.contains("production bootstrap forbids Rust source parse fallback"),
        "unexpected error: {msg}"
    );
}
