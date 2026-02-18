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

fn vec_contains_str(vec: &Term, wanted: &str) -> bool {
    let Term::Vector(items) = vec else {
        return false;
    };
    items.iter().any(|t| t == &Term::Str(wanted.to_string()))
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

#[test]
fn selfhost_vcs_artifact_refs_commit_respects_options() {
    let base = hex('a');
    let patch = hex('b');
    let result = hex('c');
    let evidence = hex('d');
    let parent = hex('e');
    let attestation = hex('f');
    let src = format!(
        r#"
          ((core/cli::vcs-artifact-refs
             {{
               :type (quote :vcs/commit)
               :base "{base}"
               :patch "{patch}"
               :result "{result}"
               :evidence ["{evidence}"]
               :attestations ["{attestation}"]
               :parents ["{parent}"]
             }})
           {{:include-evidence true :include-parents true}})
        "#
    );
    let got = eval_tool(&src);
    let Term::Vector(items) = &got else {
        panic!("expected vector, got {}", gc_coreform::print_term(&got));
    };
    assert_eq!(items.len(), 6);
    assert!(vec_contains_str(&got, &base));
    assert!(vec_contains_str(&got, &patch));
    assert!(vec_contains_str(&got, &result));
    assert!(vec_contains_str(&got, &evidence));
    assert!(vec_contains_str(&got, &attestation));
    assert!(vec_contains_str(&got, &parent));
}

#[test]
fn selfhost_vcs_plan_reachability_collects_live_and_missing() {
    let commit = hex('a');
    let patch = hex('b');
    let snapshot = hex('c');
    let evidence = hex('d');
    let missing_value = hex('e');
    let missing_member = hex('f');
    let missing_input = hex('9');
    let missing_data = hex('8');
    let src = format!(
        r#"
          (((core/cli::vcs-plan-reachability
              '{{
                "{commit}"
                  {{
                    :type :vcs/commit
                    :v 1
                    :parents []
                    :base nil
                    :patch "{patch}"
                    :result "{snapshot}"
                    :obligations []
                    :evidence ["{evidence}"]
                    :attestations []
                    :message "m"
                  }}
                "{patch}"
                  {{
                    :type :vcs/patch
                    :v 1
                    :ops [{{:op :replace :path [] :value "{missing_value}"}}]
                  }}
                "{snapshot}"
                  {{
                    :type :vcs/snapshot
                    :v 1
                    :kind :package
                    :members {{pkg/a "{missing_member}"}}
                    :deps []
                  }}
                "{evidence}"
                  {{
                    :type :vcs/evidence
                    :v 1
                    :kind :effect-log
                    :inputs ["{missing_input}"]
                    :outputs []
                    :data "{missing_data}"
                  }}
              }})
            ["{commit}"])
           {{:include-evidence true :include-parents false :include-deps false}})
        "#
    );
    let got = eval_tool(&src);
    let Term::Map(m) = &got else {
        panic!("expected map, got {}", gc_coreform::print_term(&got));
    };
    let live = m
        .get(&TermOrdKey(Term::symbol(":live")))
        .expect("missing :live");
    let missing = m
        .get(&TermOrdKey(Term::symbol(":missing")))
        .expect("missing :missing");
    assert!(vec_contains_str(live, &commit));
    assert!(vec_contains_str(live, &patch));
    assert!(vec_contains_str(live, &snapshot));
    assert!(vec_contains_str(live, &evidence));
    assert!(vec_contains_str(missing, &missing_value));
    assert!(vec_contains_str(missing, &missing_member));
    assert!(vec_contains_str(missing, &missing_input));
    assert!(vec_contains_str(missing, &missing_data));
}

#[test]
fn selfhost_lock_plan_from_requirements_tracks_missing() {
    let commit = hex('1');
    let snapshot = hex('2');
    let src = format!(
        r#"
          ((core/cli::lock-plan-from-requirements
             '{{
               "my-lib" {{:selector "refs/heads/main" :update_policy (quote :auto)}}
               "missing-lib" {{:selector "ref:refs/heads/dev" :update_policy (quote :manual)}}
             }})
           '{{
             "my-lib"
               {{
                 :commit "{commit}"
                 :snapshot "{snapshot}"
                 :registry "default"
                 :resolved_ref "refs/heads/main"
               }}
           }})
        "#
    );
    let got = eval_tool(&src);
    let Term::Map(m) = &got else {
        panic!("expected map, got {}", gc_coreform::print_term(&got));
    };
    assert_eq!(
        m.get(&TermOrdKey(Term::symbol(":count"))),
        Some(&Term::Int(1.into()))
    );
    let missing = m
        .get(&TermOrdKey(Term::symbol(":missing")))
        .expect("missing :missing");
    assert!(vec_contains_str(missing, "missing-lib"));
    let Some(Term::Map(locked)) = m.get(&TermOrdKey(Term::symbol(":locked"))) else {
        panic!("missing :locked");
    };
    let Some(Term::Map(entry)) = locked.get(&TermOrdKey(Term::Str("my-lib".to_string()))) else {
        panic!("missing locked entry for my-lib");
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
