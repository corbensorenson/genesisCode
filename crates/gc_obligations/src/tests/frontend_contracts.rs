use super::*;

#[test]
fn lint_autofix_builds_replace_node_patch_for_missing_types() {
    let src = r#"
          (def ::meta (quote {:exports [pkg/a::x pkg/a::y]}))
          (def pkg/a::x 1)
          (def pkg/a::y 2)
        "#;
    let forms = parse_module(src).unwrap();
    let (patch, reasons) =
        obligation_lint::lint_autofix_patch_for_module("lint.gc", &forms).unwrap();
    assert!(reasons.iter().any(|r| r == "editor/lint/missing-types-map"));
    assert!(reasons.iter().any(|r| r == "editor/lint/missing-type"));

    let Term::Map(m) = patch else {
        panic!("patch must be map")
    };
    let ops = m
        .get(&TermOrdKey(Term::symbol(":ops")))
        .expect("patch must contain :ops");
    let Term::Vector(ops) = ops else {
        panic!(":ops must be vector")
    };
    assert_eq!(ops.len(), 1);
    let Term::Map(opm) = &ops[0] else {
        panic!("op must be map")
    };
    assert!(matches!(
        opm.get(&TermOrdKey(Term::symbol(":op"))),
        Some(Term::Symbol(s)) if s == ":replace-node"
    ));
}

#[test]
fn lint_autofix_returns_none_when_types_are_complete() {
    let src = r#"
          (def ::meta (quote {:exports [pkg/a::x] :types {pkg/a::x Int}}))
          (def pkg/a::x 1)
        "#;
    let forms = parse_module(src).unwrap();
    assert!(obligation_lint::lint_autofix_patch_for_module("lint.gc", &forms).is_none());
}

#[test]
fn env_truthy_accepts_expected_values() {
    let is_truthy = |v: &str| {
        matches!(
            v.trim().to_ascii_lowercase().as_str(),
            "1" | "true" | "yes" | "on"
        )
    };
    for v in ["1", "true", "TRUE", " yes ", "On"] {
        assert!(is_truthy(v), "expected truthy: {v}");
    }
    for v in ["0", "false", "no", "", "off", "wat"] {
        assert!(!is_truthy(v), "expected falsey: {v}");
    }
}

#[test]
fn eval_module_default_executes_with_compiled_fast_path() {
    let forms = parse_module("(def pkg/a::x 41)\n(prim int/add pkg/a::x 1)\n").expect("parse");
    let mut ctx = EvalCtx::with_step_limit(None);
    let prelude = build_prelude(&mut ctx);
    let mut env = prelude.env;
    let value =
        eval_module_default(&mut env, &mut ctx, &forms, "tests/eval_default.gc").expect("eval");
    let Some(Term::Int(n)) = value.to_plain_term() else {
        panic!("expected int result");
    };
    assert_eq!(n, BigInt::from(42));
}

#[test]
fn selfhost_only_rejects_rust_frontend_at_library_boundary() {
    let rust_frontend = rust_coreform_frontend();
    let err =
        crate::frontend::enforce_frontend_allowed_with_flag(&rust_frontend, "test", true, true)
            .expect_err("rust frontend must be blocked in selfhost-only mode");
    assert!(format!("{err}").contains("selfhost-only mode forbids Rust frontend"));
    crate::frontend::enforce_frontend_allowed_with_flag(
        &default_coreform_frontend(),
        "test",
        true,
        true,
    )
    .expect("selfhost frontend must be allowed");
}

#[test]
fn rust_frontend_requires_compat_flag_at_library_boundary() {
    let rust_frontend = rust_coreform_frontend();
    let err =
        crate::frontend::enforce_frontend_allowed_with_flag(&rust_frontend, "test", false, false)
            .expect_err("rust frontend must require explicit compatibility mode");
    assert!(format!("{err}").contains("Rust frontend is disabled in this profile"));
    crate::frontend::enforce_frontend_allowed_with_flag(&rust_frontend, "test", false, true)
        .expect("rust frontend should be permitted when compatibility mode is enabled");
}

#[test]
fn non_artifact_bootstrap_mode_is_dev_only_at_library_boundary() {
    let frontend = CoreformFrontend::Selfhost(SelfhostFrontendConfig {
        bootstrap_mode: SelfhostBootstrapMode::Embedded,
        artifact: None,
    });
    let err = crate::frontend::enforce_frontend_bootstrap_mode_with_flag(&frontend, "test", false)
        .expect_err("embedded bootstrap should be blocked outside development mode");
    assert!(format!("{err}").contains("development-only"));
    crate::frontend::enforce_frontend_bootstrap_mode_with_flag(&frontend, "test", true)
        .expect("embedded bootstrap should be allowed in development mode");
}

fn selfhost_env() -> (EvalCtx, Env) {
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
    (ctx, env)
}

#[test]
fn selfhost_parse_prefers_core_cli_canonicalize_handler_when_present() {
    let (mut ctx, mut env) = selfhost_env();
    env.set_local(
        "core/cli::canonicalize-module-src",
        Value::data(Term::Str("shadowed".to_string())),
    );

    let err = selfhost_parse_canonicalize_module(&mut ctx, &env, "(def x 1)\n x\n").unwrap_err();
    assert!(
        format!("{err}").contains("not callable"),
        "expected core/cli path to be attempted first, got: {err}"
    );
}

#[test]
fn selfhost_meta_prefers_core_cli_module_meta_handler_when_present() {
    let (mut ctx, mut env) = selfhost_env();
    env.set_local(
        "core/cli::module-meta",
        Value::data(Term::Str("shadowed".to_string())),
    );

    let forms = canonicalize_module(parse_module("(def ::meta (quote {:caps []}))\n").unwrap())
        .expect("canonical module");
    let err = selfhost_extract_module_meta(&mut ctx, &env, &forms).unwrap_err();
    assert!(
        format!("{err}").contains("not callable"),
        "expected core/cli module-meta path to be attempted first, got: {err}"
    );
}

#[test]
fn selfhost_hash_prefers_core_cli_hash_module_forms_handler_when_present() {
    let (mut ctx, mut env) = selfhost_env();
    env.set_local(
        "core/cli::hash-module-forms",
        Value::data(Term::Str("shadowed".to_string())),
    );

    let forms = canonicalize_module(parse_module("(def x 1)\n x\n").unwrap()).unwrap();
    let err = selfhost_hash_module_forms(&mut ctx, &env, &forms).unwrap_err();
    assert!(
        format!("{err}").contains("not callable"),
        "expected core/cli hash-module-forms path to be attempted first, got: {err}"
    );
}

#[test]
fn selfhost_hash_requires_a_selfhost_hash_binding_and_does_not_fallback_to_rust() {
    let mut ctx = EvalCtx::with_step_limit(None);
    let prelude = build_prelude(&mut ctx);
    let env = prelude.env;
    let forms = canonicalize_module(parse_module("(def x 1)\n x\n").unwrap()).unwrap();
    let err = selfhost_hash_module_forms(&mut ctx, &env, &forms).unwrap_err();
    assert!(
        format!("{err}").contains("missing binding core/cli::hash-module-forms"),
        "expected missing-binding error, got: {err}"
    );
}

#[test]
fn selfhost_optimize_prefers_core_cli_optimize_module_handler_when_present() {
    let (mut ctx, mut env) = selfhost_env();
    env.set_local(
        "core/cli::optimize-module",
        Value::data(Term::Str("shadowed".to_string())),
    );

    let forms = canonicalize_module(parse_module("(def x (prim int/add 1 2))\n x\n").unwrap())
        .expect("canonical module");
    let err = selfhost_optimize_module_forms(&mut ctx, &env, &forms).unwrap_err();
    assert!(
        format!("{err}").contains("not callable"),
        "expected core/cli optimize-module path to be attempted first, got: {err}"
    );
}

#[test]
fn selfhost_optimize_requires_core_cli_binding_and_does_not_fallback_to_rust() {
    let mut ctx = EvalCtx::with_step_limit(None);
    let prelude = build_prelude(&mut ctx);
    let env = prelude.env;
    let forms = canonicalize_module(parse_module("(def x (prim int/add 1 2))\n x\n").unwrap())
        .expect("canonical module");
    let err = selfhost_optimize_module_forms(&mut ctx, &env, &forms).unwrap_err();
    assert!(
        format!("{err}").contains("missing binding core/cli::optimize-module"),
        "expected missing-binding error, got: {err}"
    );
}

#[test]
fn selfhost_infer_effects_prefers_core_cli_handler_when_present() {
    let (mut ctx, mut env) = selfhost_env();
    env.set_local(
        "core/cli::infer-effects",
        Value::data(Term::Str("shadowed".to_string())),
    );

    let forms = canonicalize_module(
        parse_module("(def p (core/effect::perform 'sys/time::now {} (fn (x) x)))\n").unwrap(),
    )
    .expect("canonical module");
    let err = selfhost_infer_effects_forms(&mut ctx, &env, &forms).unwrap_err();
    assert!(
        format!("{err}").contains("not callable"),
        "expected core/cli infer-effects path to be attempted first, got: {err}"
    );
}

#[test]
fn selfhost_infer_effects_matches_gc_types_for_pkg_basic_fixture() {
    let (mut ctx, env) = selfhost_env();
    let forms = canonicalize_module(
        parse_module(include_str!("../../../../tests/spec/pkg_basic/basic.gc")).unwrap(),
    )
    .expect("canonical module");
    let rust = gc_types::infer_effects(&forms);
    let selfhost = selfhost_infer_effects_forms(&mut ctx, &env, &forms).expect("infer");
    assert_eq!(selfhost.unknown, rust.unknown);
    assert_eq!(selfhost.ops, rust.ops);
}

#[test]
fn selfhost_infer_effects_matches_gc_types_for_pkg_fail_caps_declared_fixture() {
    let (mut ctx, env) = selfhost_env();
    let forms = canonicalize_module(
        parse_module(include_str!(
            "../../../../tests/spec/pkg_fail_caps_declared/fail.gc"
        ))
        .unwrap(),
    )
    .expect("canonical module");
    let rust = gc_types::infer_effects(&forms);
    let selfhost = selfhost_infer_effects_forms(&mut ctx, &env, &forms).expect("infer");
    assert_eq!(selfhost.unknown, rust.unknown);
    assert_eq!(selfhost.ops, rust.ops);
}
