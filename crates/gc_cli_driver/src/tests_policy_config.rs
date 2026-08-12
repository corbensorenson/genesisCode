use gc_coreform::{Term, TermOrdKey, canonicalize_module, parse_module};
use gc_kernel::{EvalCtx, eval_module};
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
            let hash = gc_coreform::hash_module(&forms);
            Term::Map(
                [
                    (TermOrdKey(Term::symbol(":path")), Term::Str(path.clone())),
                    (TermOrdKey(Term::symbol(":source")), Term::Str(src.clone())),
                    (TermOrdKey(Term::symbol(":forms")), Term::Vector(forms)),
                    (
                        TermOrdKey(Term::symbol(":module-h")),
                        Term::Bytes(hash.to_vec().into()),
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
    gc_coreform::print_term(&Term::Map(
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
    ))
}

fn eval_tool(source: &str) -> Term {
    let forms = canonicalize_module(parse_module(source).expect("parse tool"))
        .expect("canonicalize tool module");
    let mut ctx = EvalCtx::new();
    let prelude = build_prelude(&mut ctx);
    let mut env = prelude.env;
    load_selfhost_coreform_toolchain_v1_from_artifact_source(
        &mut ctx,
        &mut env,
        &build_selfhost_artifact_source(),
    )
    .expect("load selfhost toolchain");
    eval_module(&mut ctx, &mut env, &forms)
        .expect("evaluate policy authority case")
        .to_term_for_log(ctx.protocol.map(|protocol| protocol.error))
}

fn map_field<'a>(term: &'a Term, key: &str) -> &'a Term {
    let Term::Map(map) = term else {
        panic!("expected map, got {}", gc_coreform::print_term(term));
    };
    map.get(&TermOrdKey(Term::symbol(key)))
        .unwrap_or_else(|| panic!("missing {key}"))
}

#[test]
fn selfhost_policy_list_normalizes_alias_hash_and_resolves_default() {
    let upper = "A".repeat(64);
    let lower = "a".repeat(64);
    let got = eval_tool(&format!(
        r#"
          (core/cli::policy-authority
            {{
              :kind "genesis/policy-authority-request-v0.1"
              :v 1
              :operation (quote :list)
              :config
                {{:version 1
                 :default "stable"
                 :aliases [{{:name "stable" :hash "{upper}"}}]}}
              :selector nil}})
        "#
    ));
    assert_eq!(map_field(&got, ":ok"), &Term::Bool(true));
    assert_eq!(map_field(&got, ":default"), &Term::Str("stable".into()));
    assert_eq!(
        map_field(&got, ":default-resolved"),
        &Term::Str(lower.clone())
    );
    let Term::Vector(aliases) = map_field(&got, ":aliases") else {
        panic!("aliases must be a vector");
    };
    assert_eq!(map_field(&aliases[0], ":hash"), &Term::Str(lower));
}

#[test]
fn selfhost_policy_resolve_canonicalizes_direct_hash() {
    let upper = "B".repeat(64);
    let lower = "b".repeat(64);
    let got = eval_tool(&format!(
        r#"
          (core/cli::policy-authority
            {{
              :kind "genesis/policy-authority-request-v0.1"
              :v 1
              :operation (quote :resolve)
              :config {{:version 1 :default nil :aliases []}}
              :selector "{upper}"}})
        "#
    ));
    assert_eq!(map_field(&got, ":ok"), &Term::Bool(true));
    assert_eq!(map_field(&got, ":hash"), &Term::Str(lower.clone()));
    assert_eq!(map_field(&got, ":resolved"), &Term::Str(lower));
}

#[test]
fn selfhost_policy_set_default_rejects_unknown_and_self_referential_aliases() {
    for (selector, expected_code) in [
        ("missing", "policy/set-default"),
        ("default", "policy/set-default"),
        (" ", "policy/parse"),
    ] {
        let got = eval_tool(&format!(
            r#"
              (core/cli::policy-authority
                {{
                  :kind "genesis/policy-authority-request-v0.1"
                  :v 1
                  :operation (quote :set-default)
                  :config {{:version 1 :default "default" :aliases []}}
                  :selector "{selector}"}})
            "#
        ));
        assert_eq!(
            map_field(&got, ":ok"),
            &Term::Bool(false),
            "selector {selector}"
        );
        assert_eq!(
            map_field(&got, ":error-code"),
            &Term::Str(expected_code.into()),
            "selector {selector}"
        );
    }
}

#[test]
fn selfhost_policy_authority_rejects_duplicate_aliases_and_open_requests() {
    let hash = "c".repeat(64);
    let duplicate = eval_tool(&format!(
        r#"
          (core/cli::policy-authority
            {{
              :kind "genesis/policy-authority-request-v0.1"
              :v 1
              :operation (quote :list)
              :config
                {{:version 1 :default nil
                 :aliases [{{:name "x" :hash "{hash}"}} {{:name "x" :hash "{hash}"}}]}}
              :selector nil}})
        "#
    ));
    assert_eq!(map_field(&duplicate, ":ok"), &Term::Bool(false));
    assert_eq!(
        map_field(&duplicate, ":error-code"),
        &Term::Str("policy/parse".into())
    );

    let open = eval_tool(
        r#"
          (core/error::is?
            (core/cli::policy-authority
              {
                :kind "genesis/policy-authority-request-v0.1"
                :v 1
                :operation (quote :list)
                :config {:version 1 :default nil :aliases []}
                :selector nil
                :extra true}))
        "#,
    );
    assert_eq!(open, Term::Bool(true));

    let unknown_operation = eval_tool(
        r#"
          (core/error::is?
            (core/cli::policy-authority
              {
                :kind "genesis/policy-authority-request-v0.1"
                :v 1
                :operation (quote :unknown)
                :config {:version 1 :default nil :aliases []}
                :selector nil}))
        "#,
    );
    assert_eq!(unknown_operation, Term::Bool(true));
}

#[test]
fn selfhost_policy_authority_preserves_unicode_trim_profile() {
    let upper = "D".repeat(64);
    let lower = "d".repeat(64);
    let got = eval_tool(&format!(
        r#"
          (core/cli::policy-authority
            {{
              :kind "genesis/policy-authority-request-v0.1"
              :v 1
              :operation (quote :resolve)
              :config
                {{:version 1 :default "\u3000stable\u3000"
                 :aliases [{{:name "\u00a0stable\u00a0" :hash "\u2009{upper}\u2009"}}]}}
              :selector "\u2028default\u2029"}})
        "#
    ));
    assert_eq!(map_field(&got, ":ok"), &Term::Bool(true));
    assert_eq!(map_field(&got, ":resolved"), &Term::Str("stable".into()));
    assert_eq!(map_field(&got, ":hash"), &Term::Str(lower));
}

#[test]
fn selfhost_policy_authority_rejects_alias_collisions_after_trim() {
    let hash = "e".repeat(64);
    let got = eval_tool(&format!(
        r#"
          (core/cli::policy-authority
            {{
              :kind "genesis/policy-authority-request-v0.1"
              :v 1
              :operation (quote :list)
              :config
                {{:version 1 :default nil
                 :aliases [{{:name "stable" :hash "{hash}"}}
                           {{:name " stable " :hash "{hash}"}}]}}
              :selector nil}})
        "#
    ));
    assert_eq!(map_field(&got, ":ok"), &Term::Bool(false));
    assert_eq!(
        map_field(&got, ":error-code"),
        &Term::Str("policy/parse".into())
    );
}
