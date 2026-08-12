use super::*;
use gc_kernel::{KernelError, NativeFn};

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
fn selfhost_parse_requires_closed_core_cli_frontend_handler() {
    let (mut ctx, mut env) = selfhost_env();
    env.set_local(
        "core/cli::frontend-module",
        Value::data(Term::Str("shadowed".to_string())),
    );

    let err = selfhost_parse_canonicalize_module(&mut ctx, &env, "(def x 1)\n x\n").unwrap_err();
    assert!(
        format!("{err}").contains("not callable"),
        "expected the closed core/cli frontend path to be authoritative, got: {err}"
    );
}

fn frontend_result_term(
    profile: &str,
    module_hash: &str,
    source_end_byte: u64,
    extra_field: bool,
) -> Term {
    let mut result = BTreeMap::from([
        (
            TermOrdKey(Term::symbol(":canonical-source")),
            Term::Str("1\n".to_string()),
        ),
        (
            TermOrdKey(Term::symbol(":forms")),
            Term::Vector(vec![Term::Int(1.into())]),
        ),
        (
            TermOrdKey(Term::symbol(":kind")),
            Term::Str("genesis/frontend-module-v0.1".to_string()),
        ),
        (
            TermOrdKey(Term::symbol(":module-h")),
            Term::Str(module_hash.to_string()),
        ),
        (
            TermOrdKey(Term::symbol(":profile")),
            Term::Str(profile.to_string()),
        ),
        (
            TermOrdKey(Term::symbol(":source-span")),
            Term::Map(BTreeMap::from([
                (
                    TermOrdKey(Term::symbol(":end-byte")),
                    Term::Int(source_end_byte.into()),
                ),
                (TermOrdKey(Term::symbol(":start-byte")), Term::Int(0.into())),
            ])),
        ),
        (
            TermOrdKey(Term::symbol(":span-unit")),
            Term::symbol(":utf8-byte"),
        ),
        (TermOrdKey(Term::symbol(":v")), Term::Int(1.into())),
    ]);
    if extra_field {
        result.insert(TermOrdKey(Term::symbol(":unknown")), Term::Bool(true));
    }
    Term::Map(result)
}

fn frontend_result_extra(_ctx: &mut EvalCtx, _args: Vec<Value>) -> Result<Value, KernelError> {
    Ok(Value::data(frontend_result_term(
        "genesis/coreform-canon-hash-v0.2",
        &"0".repeat(64),
        2,
        true,
    )))
}

fn frontend_result_wrong_profile(
    _ctx: &mut EvalCtx,
    _args: Vec<Value>,
) -> Result<Value, KernelError> {
    Ok(Value::data(frontend_result_term(
        "genesis/unknown",
        &"0".repeat(64),
        2,
        false,
    )))
}

fn frontend_result_wrong_span(_ctx: &mut EvalCtx, _args: Vec<Value>) -> Result<Value, KernelError> {
    Ok(Value::data(frontend_result_term(
        "genesis/coreform-canon-hash-v0.2",
        &"0".repeat(64),
        1,
        false,
    )))
}

fn frontend_result_noncanonical_hash(
    _ctx: &mut EvalCtx,
    _args: Vec<Value>,
) -> Result<Value, KernelError> {
    Ok(Value::data(frontend_result_term(
        "genesis/coreform-canon-hash-v0.2",
        &"A".repeat(64),
        2,
        false,
    )))
}

#[test]
fn selfhost_frontend_result_is_closed_and_span_checked() {
    type FrontendStub = fn(&mut EvalCtx, Vec<Value>) -> Result<Value, KernelError>;
    let cases: &[(&str, FrontendStub, &str)] = &[
        (
            "test/frontend-extra",
            frontend_result_extra,
            "exactly 8 fields",
        ),
        (
            "test/frontend-profile",
            frontend_result_wrong_profile,
            "identity or profile mismatch",
        ),
        (
            "test/frontend-span",
            frontend_result_wrong_span,
            ":source-span is invalid",
        ),
        (
            "test/frontend-hash-shape",
            frontend_result_noncanonical_hash,
            "non-canonical lowercase 64-hex hash",
        ),
    ];
    let (mut ctx, mut env) = selfhost_env();
    for (name, stub, expected) in cases {
        env.set_local(
            "core/cli::frontend-module",
            Value::native_fn(NativeFn::new(name, 1, *stub)),
        );
        let error = selfhost_frontend_module(&mut ctx, &env, "1\n")
            .expect_err("malformed frontend result must fail closed");
        assert!(
            format!("{error}").contains(expected),
            "{name} returned the wrong transport error: {error}"
        );
    }
}

#[test]
fn selfhost_frontend_atomically_matches_canonical_forms_and_identity() {
    let source = "; UTF-8 byte custody\n(def café \"e\\u0301\")\n[{:z 2 :a 1}]\n";
    let (mut ctx, env) = selfhost_env();
    let frontend = selfhost_frontend_module(&mut ctx, &env, source).expect("frontend authority");
    let expected_forms =
        canonicalize_module(parse_module(source).expect("parse oracle")).expect("canonical oracle");
    assert_eq!(frontend.forms, expected_forms);
    assert_eq!(frontend.module_hash, hash_module(&expected_forms));
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
        format!("{err}")
            .contains("missing required production binding core/cli::hash-module-forms"),
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
