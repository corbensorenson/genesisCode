use super::*;
use gc_coreform::{TermOrdKey, canonicalize_module, parse_module};
use gc_kernel::eval_module;

fn eval_gc_term(src: &str) -> Term {
    let forms = canonicalize_module(parse_module(src).expect("parse")).expect("canon");
    let mut ctx = EvalCtx::new();
    let prelude = build_prelude(&mut ctx);
    let mut env = prelude.env;
    let v = eval_module(&mut ctx, &mut env, &forms).expect("eval");
    v.to_term_for_log(ctx.protocol.map(|p| p.error))
}

fn map_get<'a>(t: &'a Term, key: &str) -> Option<&'a Term> {
    let Term::Map(m) = t else { return None };
    m.get(&TermOrdKey(Term::symbol(key)))
}

#[test]
fn store_is_content_addressed() {
    let dir = tempfile::tempdir().unwrap();
    let store = EvidenceStore::open(dir.path()).unwrap();
    let t = Term::Str("hello".to_string());
    let h1 = store.put_term(&t).unwrap();
    let h2 = store.put_term(&t).unwrap();
    assert_eq!(h1, h2);
    assert!(store.path_for(&h1).exists());
}

#[test]
fn obligation_cache_term_roundtrip_preserves_result_shape() {
    let key = "abc123";
    let result = PackageTestResult {
        ok: false,
        acceptance_artifact: "acc-hash".to_string(),
        obligation_results: vec![
            ObligationResult {
                name: "core/obligation::unit-tests".to_string(),
                ok: true,
                artifact: Some("art-1".to_string()),
                errors: Vec::new(),
            },
            ObligationResult {
                name: "core/obligation::caps".to_string(),
                ok: false,
                artifact: None,
                errors: vec!["missing cap".to_string()],
            },
        ],
    };
    let t = cached_result_to_term(key, &result);
    let parsed = parse_cached_result_term(key, &t).expect("parse cached result");
    assert_eq!(parsed.ok, result.ok);
    assert_eq!(parsed.acceptance_artifact, result.acceptance_artifact);
    assert_eq!(parsed.obligation_results.len(), 2);
    assert_eq!(
        parsed.obligation_results[0].name,
        "core/obligation::unit-tests"
    );
    assert_eq!(
        parsed.obligation_results[0].artifact.as_deref(),
        Some("art-1")
    );
    assert_eq!(parsed.obligation_results[1].name, "core/obligation::caps");
    assert_eq!(
        parsed.obligation_results[1].errors,
        vec!["missing cap".to_string()]
    );
}

#[test]
fn cache_artifact_presence_check_respects_missing_artifacts() {
    let dir = tempfile::tempdir().unwrap();
    let store = EvidenceStore::open(dir.path()).unwrap();
    let acceptance = store
        .put_term(&Term::Str("acceptance".to_string()))
        .unwrap();
    let ob_artifact = store.put_term(&Term::Str("ob-art".to_string())).unwrap();
    let ok_result = PackageTestResult {
        ok: true,
        acceptance_artifact: acceptance,
        obligation_results: vec![ObligationResult {
            name: "core/obligation::unit-tests".to_string(),
            ok: true,
            artifact: Some(ob_artifact),
            errors: Vec::new(),
        }],
    };
    assert!(cache_artifacts_present_and_valid(&store, &ok_result).unwrap());

    let miss_result = PackageTestResult {
        ok: true,
        acceptance_artifact: "missing".to_string(),
        obligation_results: Vec::new(),
    };
    assert!(!cache_artifacts_present_and_valid(&store, &miss_result).unwrap());
}

#[test]
fn gfx_obligation_report_builders_match_rust_report_shapes() {
    let surface_h = "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa";
    let src = format!(
        r#"
            {{
              :golden
                ((((core/gfx/obligation::golden-report "pkg/demo") true)
                  [{{:suite pkg/tests::gfx :name "case-a" :ok true :kind :frame-graph}}])
                  ["warn"])
              :frame
                (((((core/gfx/obligation::frame-budget-report "pkg/demo") false)
                   {{:max-render-passes-per-frame 2}})
                   [{{:suite pkg/tests::gfx :name "case-b" :ok false :render-passes 3}}])
                   ["budget failed"])
              :api
                ((((((core/gfx/obligation::api-stability-report "pkg/demo") true)
                   "{surface_h}") "{surface_h}")
                   {{:kind "genesis/gfx-api-surface-v0.2" :exports [core/gfx/runtime::plan-frame-2d] :defs []}})
                   [])
            }}
            "#
    );
    let term = eval_gc_term(&src);
    let Some(golden) = map_get(&term, ":golden") else {
        panic!("golden report missing");
    };
    let Some(frame) = map_get(&term, ":frame") else {
        panic!("frame report missing");
    };
    let Some(api) = map_get(&term, ":api") else {
        panic!("api report missing");
    };

    let expected_golden_case = Term::Map(
        [
            (
                TermOrdKey(Term::symbol(":suite")),
                Term::symbol("pkg/tests::gfx"),
            ),
            (
                TermOrdKey(Term::symbol(":name")),
                Term::Str("case-a".to_string()),
            ),
            (TermOrdKey(Term::symbol(":ok")), Term::Bool(true)),
            (
                TermOrdKey(Term::symbol(":kind")),
                Term::symbol(":frame-graph"),
            ),
        ]
        .into_iter()
        .collect(),
    );
    let expected_golden = Term::Map(
        [
            (
                TermOrdKey(Term::symbol(":kind")),
                Term::Str("genesis/gfx-golden-images-v0.2".to_string()),
            ),
            (
                TermOrdKey(Term::symbol(":package")),
                Term::Str("pkg/demo".to_string()),
            ),
            (TermOrdKey(Term::symbol(":ok")), Term::Bool(true)),
            (
                TermOrdKey(Term::symbol(":cases")),
                Term::Vector(vec![expected_golden_case]),
            ),
            (
                TermOrdKey(Term::symbol(":errors")),
                Term::Vector(vec![Term::Str("warn".to_string())]),
            ),
        ]
        .into_iter()
        .collect(),
    );
    assert_eq!(golden, &expected_golden);

    let expected_frame_limits = Term::Map(
        [(
            TermOrdKey(Term::symbol(":max-render-passes-per-frame")),
            Term::Int(2.into()),
        )]
        .into_iter()
        .collect(),
    );
    let expected_frame_case = Term::Map(
        [
            (
                TermOrdKey(Term::symbol(":suite")),
                Term::symbol("pkg/tests::gfx"),
            ),
            (
                TermOrdKey(Term::symbol(":name")),
                Term::Str("case-b".to_string()),
            ),
            (TermOrdKey(Term::symbol(":ok")), Term::Bool(false)),
            (
                TermOrdKey(Term::symbol(":render-passes")),
                Term::Int(3.into()),
            ),
        ]
        .into_iter()
        .collect(),
    );
    let expected_frame = Term::Map(
        [
            (
                TermOrdKey(Term::symbol(":kind")),
                Term::Str("genesis/gfx-frame-budgets-v0.2".to_string()),
            ),
            (
                TermOrdKey(Term::symbol(":package")),
                Term::Str("pkg/demo".to_string()),
            ),
            (TermOrdKey(Term::symbol(":ok")), Term::Bool(false)),
            (TermOrdKey(Term::symbol(":limits")), expected_frame_limits),
            (
                TermOrdKey(Term::symbol(":cases")),
                Term::Vector(vec![expected_frame_case]),
            ),
            (
                TermOrdKey(Term::symbol(":errors")),
                Term::Vector(vec![Term::Str("budget failed".to_string())]),
            ),
        ]
        .into_iter()
        .collect(),
    );
    assert_eq!(frame, &expected_frame);

    let expected_surface = Term::Map(
        [
            (
                TermOrdKey(Term::symbol(":kind")),
                Term::Str("genesis/gfx-api-surface-v0.2".to_string()),
            ),
            (
                TermOrdKey(Term::symbol(":exports")),
                Term::Vector(vec![Term::symbol("core/gfx/runtime::plan-frame-2d")]),
            ),
            (
                TermOrdKey(Term::symbol(":defs")),
                Term::Vector(Vec::<Term>::new()),
            ),
        ]
        .into_iter()
        .collect(),
    );
    let expected_api = Term::Map(
        [
            (
                TermOrdKey(Term::symbol(":kind")),
                Term::Str("genesis/gfx-api-stability-v0.2".to_string()),
            ),
            (
                TermOrdKey(Term::symbol(":package")),
                Term::Str("pkg/demo".to_string()),
            ),
            (TermOrdKey(Term::symbol(":ok")), Term::Bool(true)),
            (
                TermOrdKey(Term::symbol(":surface-h")),
                Term::Str(surface_h.to_string()),
            ),
            (
                TermOrdKey(Term::symbol(":expected-surface-h")),
                Term::Str(surface_h.to_string()),
            ),
            (TermOrdKey(Term::symbol(":surface")), expected_surface),
            (
                TermOrdKey(Term::symbol(":errors")),
                Term::Vector(Vec::<Term>::new()),
            ),
        ]
        .into_iter()
        .collect(),
    );
    assert_eq!(api, &expected_api);
}

#[test]
fn gfx_obligation_report_builders_are_hash_stable() {
    let surface_h = "bbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb";
    let src = format!(
        r#"
            {{
              :golden-a
                (core/coreform::hash-term
                  ((((core/gfx/obligation::golden-report "pkg/demo") true) []) []))
              :golden-b
                (core/coreform::hash-term
                  ((((core/gfx/obligation::golden-report "pkg/demo") true) []) []))
              :frame-a
                (core/coreform::hash-term
                  (((((core/gfx/obligation::frame-budget-report "pkg/demo") true) {{}}) []) []))
              :frame-b
                (core/coreform::hash-term
                  (((((core/gfx/obligation::frame-budget-report "pkg/demo") true) {{}}) []) []))
              :api-a
                (core/coreform::hash-term
                  ((((((core/gfx/obligation::api-stability-report "pkg/demo") true)
                     "{surface_h}") "{surface_h}") {{:kind "genesis/gfx-api-surface-v0.2" :exports [] :defs []}}) []))
              :api-b
                (core/coreform::hash-term
                  ((((((core/gfx/obligation::api-stability-report "pkg/demo") true)
                     "{surface_h}") "{surface_h}") {{:kind "genesis/gfx-api-surface-v0.2" :exports [] :defs []}}) []))
            }}
            "#
    );
    let term = eval_gc_term(&src);
    assert_eq!(map_get(&term, ":golden-a"), map_get(&term, ":golden-b"));
    assert_eq!(map_get(&term, ":frame-a"), map_get(&term, ":frame-b"));
    assert_eq!(map_get(&term, ":api-a"), map_get(&term, ":api-b"));
}

#[test]
fn core_obligation_report_builders_match_exec_report_shapes() {
    let src = r#"
            {
              :unit
                ((core/obligation::unit-tests-report "pkg/demo")
                  [{:suite pkg/tests::unit :name "case-a" :ok true}
                   {:suite pkg/tests::unit :name "case-b" :ok false}])
              :det
                (((core/obligation::determinism-report "pkg/demo") false)
                  ["determinism mismatch"])
              :caps
                (((core/obligation::capabilities-declared-report "pkg/demo") true) [])
            }
            "#;
    let term = eval_gc_term(src);
    let Some(unit) = map_get(&term, ":unit") else {
        panic!("unit report missing");
    };
    let Some(det) = map_get(&term, ":det") else {
        panic!("determinism report missing");
    };
    let Some(caps) = map_get(&term, ":caps") else {
        panic!("caps report missing");
    };

    let expected_unit = Term::Map(
        [
            (
                TermOrdKey(Term::symbol(":kind")),
                Term::Str("genesis/unit-tests-v0.2".to_string()),
            ),
            (
                TermOrdKey(Term::symbol(":package")),
                Term::Str("pkg/demo".to_string()),
            ),
            (TermOrdKey(Term::symbol(":ok")), Term::Bool(false)),
            (
                TermOrdKey(Term::symbol(":tests")),
                Term::Vector(vec![
                    Term::Map(
                        [
                            (
                                TermOrdKey(Term::symbol(":suite")),
                                Term::symbol("pkg/tests::unit"),
                            ),
                            (
                                TermOrdKey(Term::symbol(":name")),
                                Term::Str("case-a".to_string()),
                            ),
                            (TermOrdKey(Term::symbol(":ok")), Term::Bool(true)),
                        ]
                        .into_iter()
                        .collect(),
                    ),
                    Term::Map(
                        [
                            (
                                TermOrdKey(Term::symbol(":suite")),
                                Term::symbol("pkg/tests::unit"),
                            ),
                            (
                                TermOrdKey(Term::symbol(":name")),
                                Term::Str("case-b".to_string()),
                            ),
                            (TermOrdKey(Term::symbol(":ok")), Term::Bool(false)),
                        ]
                        .into_iter()
                        .collect(),
                    ),
                ]),
            ),
        ]
        .into_iter()
        .collect(),
    );
    assert_eq!(unit, &expected_unit);

    let expected_det = Term::Map(
        [
            (
                TermOrdKey(Term::symbol(":kind")),
                Term::Str("genesis/determinism-v0.2".to_string()),
            ),
            (
                TermOrdKey(Term::symbol(":package")),
                Term::Str("pkg/demo".to_string()),
            ),
            (TermOrdKey(Term::symbol(":ok")), Term::Bool(false)),
            (
                TermOrdKey(Term::symbol(":errors")),
                Term::Vector(vec![Term::Str("determinism mismatch".to_string())]),
            ),
        ]
        .into_iter()
        .collect(),
    );
    assert_eq!(det, &expected_det);

    let expected_caps = Term::Map(
        [
            (
                TermOrdKey(Term::symbol(":kind")),
                Term::Str("genesis/caps-declared-v0.2".to_string()),
            ),
            (
                TermOrdKey(Term::symbol(":package")),
                Term::Str("pkg/demo".to_string()),
            ),
            (TermOrdKey(Term::symbol(":ok")), Term::Bool(true)),
            (
                TermOrdKey(Term::symbol(":errors")),
                Term::Vector(Vec::<Term>::new()),
            ),
        ]
        .into_iter()
        .collect(),
    );
    assert_eq!(caps, &expected_caps);
}

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
    let Value::Data(Term::Int(n)) = value else {
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

#[test]
fn selfhost_parse_prefers_core_cli_canonicalize_handler_when_present() {
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

    // Shadow the core/cli binding with an invalid value. If the function prefers
    // core/cli when present, this must fail before falling back to low-level bindings.
    env.set_local(
        "core/cli::canonicalize-module-src",
        Value::Data(Term::Str("shadowed".to_string())),
    );

    let err = selfhost_parse_canonicalize_module(&mut ctx, &env, "(def x 1)\n x\n").unwrap_err();
    assert!(
        format!("{err}").contains("not callable"),
        "expected core/cli path to be attempted first, got: {err}"
    );
}

#[test]
fn selfhost_meta_prefers_core_cli_module_meta_handler_when_present() {
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

    // Shadow the core/cli meta extractor with invalid data. If module-meta prefers
    // core/cli when present, this must fail before any static fallback path.
    env.set_local(
        "core/cli::module-meta",
        Value::Data(Term::Str("shadowed".to_string())),
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

    env.set_local(
        "core/cli::hash-module-forms",
        Value::Data(Term::Str("shadowed".to_string())),
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

    env.set_local(
        "core/cli::optimize-module",
        Value::Data(Term::Str("shadowed".to_string())),
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

    env.set_local(
        "core/cli::infer-effects",
        Value::Data(Term::Str("shadowed".to_string())),
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

#[test]
fn selfhost_literal_op_and_flatten_app_detect_quoted_effect_op() {
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

    let forms = canonicalize_module(parse_module("(((core/effect::perform (quote io/fs::write)) {:data \"x\" :path \"out.txt\"}) (fn (_) (core/effect::pure nil)))\n").unwrap())
            .expect("canonical module");
    let app = forms.first().expect("one form").clone();
    let app_items = app.as_proper_list().expect("app proper list");
    let inner = app_items[0].clone();
    let inner_debug = format!("{inner:?}");

    let flatten = env
        .get("core/cli::flatten-app")
        .expect("flatten-app binding");
    let flat_v = flatten
        .clone()
        .apply(&mut ctx, Value::Data(app.clone()))
        .expect("flatten apply");
    let flat_t = flat_v.to_term_for_log(ctx.protocol.map(|p| p.error));
    let flat_map = match flat_t {
        Term::Map(m) => m,
        other => panic!("flatten-app returned non-map: {}", print_term(&other)),
    };
    let args = match flat_map.get(&TermOrdKey(Term::symbol(":args"))) {
        Some(Term::Vector(v)) => v.clone(),
        other => panic!("flatten-app args missing/non-vector: {:?}", other),
    };
    let args_debug = format!("{args:?}");
    assert_eq!(args.len(), 3, "flatten-app args length mismatch");

    let lit = env
        .get("core/cli::literal-op-sym-or-nil")
        .expect("literal-op binding");
    let mut found = false;
    let mut debug_rows: Vec<String> = Vec::new();
    let app_render = print_term(&app);
    let inner_render = print_term(&inner);
    let flat_render = print_term(&Term::Map(flat_map.clone()));
    let flat_inner_v = flatten
        .clone()
        .apply(&mut ctx, Value::Data(inner))
        .expect("flatten inner apply");
    let flat_inner_t = flat_inner_v.to_term_for_log(ctx.protocol.map(|p| p.error));
    let flat_inner_render = print_term(&flat_inner_t);
    for arg in args {
        let arg_render = print_term(&arg);
        let op_v = lit
            .clone()
            .apply(&mut ctx, Value::Data(arg))
            .expect("literal-op apply");
        let op_t = op_v.to_term_for_log(ctx.protocol.map(|p| p.error));
        debug_rows.push(format!("{arg_render} => {}", print_term(&op_t)));
        if let Term::Symbol(s) = op_t
            && s == "io/fs::write"
        {
            found = true;
        }
    }
    assert!(
        found,
        "literal-op-sym-or-nil failed to detect io/fs::write; app={} inner={} inner_debug={} flat={} flat_inner={} args_debug={} rows={}",
        app_render,
        inner_render,
        inner_debug,
        flat_render,
        flat_inner_render,
        args_debug,
        debug_rows.join(" | ")
    );
}
