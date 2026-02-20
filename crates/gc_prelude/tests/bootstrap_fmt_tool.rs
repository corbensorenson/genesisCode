use gc_coreform::{
    Term, TermOrdKey, canonicalize_module, hash_module, parse_module, print_module, print_term,
};
use gc_effects::{CapsPolicy, Decision, run};
use gc_kernel::{EvalCtx, Value, eval_module};
use gc_prelude::{
    build_prelude, load_selfhost_coreform_toolchain_v1_from_artifact_source,
    selfhost_coreform_toolchain_v1_sources,
};

fn build_selfhost_artifact_source() -> String {
    let modules = selfhost_coreform_toolchain_v1_sources()
        .expect("load selfhost toolchain sources")
        .iter()
        .map(|(path, src)| {
            let forms = canonicalize_module(parse_module(src).unwrap()).unwrap();
            let h = hash_module(&forms);
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
    print_term(&artifact)
}

#[test]
fn selfhost_tool_can_format_a_file_via_coreform_bootstrap_api() {
    let td = tempfile::tempdir().expect("tempdir");

    let messy = r#"
        (def  x   1)
        (def y   (fn (a b) a))
        (y x 2)
    "#;
    std::fs::write(td.path().join("input.gc"), messy).expect("write input");

    let base_dir = td.path().to_string_lossy().to_string();
    let caps = CapsPolicy::from_toml_str(&format!(
        r#"
allow = ["io/fs::read", "io/fs::write"]

[op."io/fs::read"]
base_dir = "{base_dir}"

[op."io/fs::write"]
base_dir = "{base_dir}"
create_dirs = true
"#,
    ))
    .expect("caps parse");

    // The tool is GenesisCode, but uses the pure prelude bootstrap API:
    // - core/coreform::fmt-module
    let tool_src = r#"
        (def prog
          (core/effect::perform
            'io/fs::read
            {:path "input.gc"}
            (fn (src)
              (core/effect::perform
                'io/fs::write
                {:path "output.gc" :data (core/coreform::fmt-module src)}
                (fn (_)
                  (core/effect::pure true))))))
        prog
    "#;

    let tool_forms = canonicalize_module(parse_module(tool_src).expect("parse tool"))
        .expect("canonicalize tool");
    let program_h = gc_coreform::hash_module(&tool_forms);

    let mut ctx = EvalCtx::new();
    let prelude = build_prelude(&mut ctx);
    let mut env = prelude.env;

    let prog = eval_module(&mut ctx, &mut env, &tool_forms).expect("eval tool");
    assert!(matches!(prog, Value::EffectProgram(_)));

    let toolchain = "gc_prelude-test".to_string();
    let r = run(&mut ctx, &caps, prog, program_h, toolchain).expect("run");

    assert!(
        r.log.entries.iter().all(|e| e.decision == Decision::Allow),
        "expected allowed decisions"
    );

    let out = std::fs::read_to_string(td.path().join("output.gc")).expect("read output");
    let want = {
        let p = parse_module(messy).unwrap();
        let c = canonicalize_module(p).unwrap();
        print_module(&c)
    };
    assert_eq!(out, want);
}

#[test]
fn selfhost_tool_can_format_a_file_via_selfhost_toolchain() {
    let td = tempfile::tempdir().expect("tempdir");

    let messy = r#"
        (def  x   1)
        (def y   (fn (a b) a))
        (y x 2)
    "#;
    std::fs::write(td.path().join("input.gc"), messy).expect("write input");

    let base_dir = td.path().to_string_lossy().to_string();
    let caps = CapsPolicy::from_toml_str(&format!(
        r#"
allow = ["io/fs::read", "io/fs::write"]

[op."io/fs::read"]
base_dir = "{base_dir}"

[op."io/fs::write"]
base_dir = "{base_dir}"
create_dirs = true
"#,
    ))
    .expect("caps parse");

    // The tool is GenesisCode and uses the self-hosted CoreForm toolchain:
    // - selfhost/tool::fmt-module
    let tool_src = r#"
        (def prog
          (core/effect::perform
            'io/fs::read
            {:path "input.gc"}
            (fn (src)
              (core/effect::perform
                'io/fs::write
                {:path "output.gc" :data (selfhost/tool::fmt-module src)}
                (fn (_)
                  (core/effect::pure true))))))
        prog
    "#;

    let tool_forms = canonicalize_module(parse_module(tool_src).expect("parse tool"))
        .expect("canonicalize tool");
    let program_h = gc_coreform::hash_module(&tool_forms);

    let mut ctx = EvalCtx::new();
    let prelude = build_prelude(&mut ctx);
    let mut env = prelude.env;

    let artifact = build_selfhost_artifact_source();
    load_selfhost_coreform_toolchain_v1_from_artifact_source(&mut ctx, &mut env, &artifact)
        .expect("load selfhost toolchain");

    let prog = eval_module(&mut ctx, &mut env, &tool_forms).expect("eval tool");
    assert!(matches!(prog, Value::EffectProgram(_)));

    let toolchain = "gc_prelude-test".to_string();
    let r = run(&mut ctx, &caps, prog, program_h, toolchain).expect("run");

    assert!(
        r.log.entries.iter().all(|e| e.decision == Decision::Allow),
        "expected allowed decisions"
    );

    let out = std::fs::read_to_string(td.path().join("output.gc")).expect("read output");
    let want = {
        let p = parse_module(messy).unwrap();
        let c = canonicalize_module(p).unwrap();
        print_module(&c)
    };
    assert_eq!(out, want);
}
