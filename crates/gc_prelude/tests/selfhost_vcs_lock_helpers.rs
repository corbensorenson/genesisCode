use std::collections::BTreeMap;

use gc_coreform::{Term, TermOrdKey, canonicalize_module, parse_module};
use gc_kernel::{EvalCtx, eval_module};
use gc_prelude::{
    build_prelude, load_selfhost_coreform_toolchain_v1_from_artifact_source,
    selfhost_coreform_toolchain_v1_sources,
};

fn build_selfhost_artifact_source() -> String {
    let modules = selfhost_coreform_toolchain_v1_sources()
        .iter()
        .map(|(path, src)| {
            let forms = canonicalize_module(parse_module(src).expect("parse module source"))
                .expect("canonicalize module source");
            let h = gc_coreform::hash_module(&forms);
            Term::Map(
                [
                    (
                        TermOrdKey(Term::symbol(":path")),
                        Term::Str((*path).to_string()),
                    ),
                    (
                        TermOrdKey(Term::symbol(":source")),
                        Term::Str((*src).to_string()),
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

fn eval_tool(src: &str) -> Term {
    let forms = canonicalize_module(parse_module(src).expect("parse tool"))
        .expect("canonicalize tool module");
    let mut ctx = EvalCtx::new();
    let prelude = build_prelude(&mut ctx);
    let mut env = prelude.env;
    let artifact = build_selfhost_artifact_source();
    load_selfhost_coreform_toolchain_v1_from_artifact_source(&mut ctx, &mut env, &artifact)
        .expect("load selfhost toolchain");
    let v = eval_module(&mut ctx, &mut env, &forms).expect("eval tool");
    v.to_term_for_log(None)
}

fn hex(ch: char) -> String {
    std::iter::repeat_n(ch, 64).collect()
}

#[test]
fn selfhost_vcs_make_and_validate_commit_roundtrip() {
    let patch = hex('a');
    let result = hex('b');
    let parent = hex('c');
    let evidence = hex('d');
    let attestation = hex('e');

    let src = format!(
        r#"
          (def commit
            (core/cli::vcs-make-commit
              {{
                :parents ["{parent}"]
                :base nil
                :patch "{patch}"
                :result "{result}"
                :obligations [core/obligation::unit-tests]
                :evidence ["{evidence}"]
                :attestations ["{attestation}"]
                :message "msg"
              }}))
          (core/cli::vcs-validate-commit commit)
        "#
    );
    let got = eval_tool(&src);
    let Term::Map(m) = got else {
        panic!("expected map, got {}", gc_coreform::print_term(&got));
    };
    assert_eq!(
        m.get(&TermOrdKey(Term::symbol(":type"))),
        Some(&Term::symbol(":vcs/commit"))
    );
    assert_eq!(
        m.get(&TermOrdKey(Term::symbol(":patch"))),
        Some(&Term::Str(patch))
    );
    assert_eq!(
        m.get(&TermOrdKey(Term::symbol(":result"))),
        Some(&Term::Str(result))
    );
}

#[test]
fn selfhost_vcs_validate_commit_rejects_bad_patch_hash() {
    let result = hex('b');
    let src = format!(
        r#"
          (core/error::is?
            (core/cli::vcs-validate-commit
              {{
                :type (quote :vcs/commit)
                :v 1
                :parents []
                :base nil
                :patch "bad"
                :result "{result}"
                :obligations []
                :evidence []
                :attestations []
                :message "msg"
              }}))
        "#
    );
    let got = eval_tool(&src);
    assert_eq!(got, Term::Bool(true));
}

#[test]
fn selfhost_lock_helpers_classify_selector_and_build_entry() {
    let commit = hex('f');
    let snapshot = hex('9');
    let src = format!(
        r#"
          {{
            :kind-commit (core/cli::lock-selector-kind "commit:{commit}")
            :kind-ref (core/cli::lock-selector-kind "refs/heads/main")
            :entry
              ((core/cli::lock-make-entry
                 {{:selector "refs/heads/main"}})
               {{
                 :commit "{commit}"
                 :snapshot "{snapshot}"
                 :registry "default"
                 :resolved_ref "refs/heads/main"
               }})
          }}
        "#
    );
    let got = eval_tool(&src);
    let Term::Map(m) = got else {
        panic!("expected map, got {}", gc_coreform::print_term(&got));
    };
    assert_eq!(
        m.get(&TermOrdKey(Term::symbol(":kind-commit"))),
        Some(&Term::symbol(":commit"))
    );
    assert_eq!(
        m.get(&TermOrdKey(Term::symbol(":kind-ref"))),
        Some(&Term::symbol(":ref"))
    );
    let Some(Term::Map(entry)) = m.get(&TermOrdKey(Term::symbol(":entry"))) else {
        panic!("entry missing or not a map");
    };
    assert_eq!(
        entry.get(&TermOrdKey(Term::symbol(":commit"))),
        Some(&Term::Str(commit))
    );
    assert_eq!(
        entry.get(&TermOrdKey(Term::symbol(":snapshot"))),
        Some(&Term::Str(snapshot))
    );
}
