use gc_coreform::{canonicalize_module, parse_module, print_module};
use gc_effects::{CapsPolicy, Decision, run};
use gc_kernel::{EvalCtx, Value, eval_module};
use gc_prelude::{build_prelude, load_selfhost_coreform_toolchain_v1};

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

    // Today, the self-hosted CoreForm toolchain is correct but can exceed the v0.2 default step
    // limit for non-trivial workloads. We run this end-to-end correctness test without a step limit
    // and track step-limit practicality as an explicit upgrade-plan item.
    let mut ctx = EvalCtx::with_step_limit(None);
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

    // Today, the self-hosted CoreForm toolchain is correct but can exceed the v0.2 default step
    // limit for non-trivial workloads. We run this end-to-end correctness test without a step limit
    // and track step-limit practicality as an explicit upgrade-plan item.
    let mut ctx = EvalCtx::with_step_limit(None);
    let prelude = build_prelude(&mut ctx);
    let mut env = prelude.env;

    load_selfhost_coreform_toolchain_v1(&mut ctx, &mut env).expect("load selfhost toolchain");

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
