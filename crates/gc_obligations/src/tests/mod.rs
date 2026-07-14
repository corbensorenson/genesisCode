use super::*;
use gc_coreform::{TermOrdKey, canonicalize_module, parse_module};
use gc_kernel::eval_module;

mod frontend_contracts;
mod selfhost_literal_op;

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

fn cache_key_fixture(dir: &Path) -> (PathBuf, PackageManifest, Vec<LoadedModule>) {
    let pkg_toml = dir.join("package.toml");
    std::fs::write(&pkg_toml, "name = \"pkg_cache\"\nversion = \"0.0.1\"\n").unwrap();
    let entry = ModuleEntry {
        path: "mod.gc".to_string(),
        hash: None,
    };
    let forms = vec![Term::Nil];
    let manifest = PackageManifest {
        schema: gc_pkg::PACKAGE_MANIFEST_SCHEMA_VERSION,
        name: "pkg_cache".to_string(),
        version: "0.0.1".to_string(),
        modules: vec![entry.clone()],
        dependencies: Vec::new(),
        obligations: vec!["core/obligation::unit-tests".to_string()],
        tests: vec!["pkg/cache::tests".to_string()],
        property_tests: Vec::new(),
        caps_policy: None,
        limits: Default::default(),
        budgets: Default::default(),
        property: Default::default(),
        gfx: Default::default(),
    };
    let modules = vec![LoadedModule {
        entry,
        abs_path: dir.join("mod.gc"),
        forms: forms.clone(),
        meta: None,
        hash: hash_module(&forms),
    }];
    (pkg_toml, manifest, modules)
}

fn legacy_v01_frontend_term(frontend: &CoreformFrontend) -> Term {
    if let CoreformFrontend::Selfhost(cfg) = frontend {
        let mode = match cfg.bootstrap_mode {
            SelfhostBootstrapMode::ArtifactOnly => ":artifact-only",
            SelfhostBootstrapMode::ArtifactPreferred => ":artifact-preferred",
            SelfhostBootstrapMode::Embedded => ":embedded",
        };
        Term::Map(
            [
                (
                    TermOrdKey(Term::symbol(":kind")),
                    Term::symbol(":frontend/selfhost"),
                ),
                (TermOrdKey(Term::symbol(":mode")), Term::symbol(mode)),
                (
                    TermOrdKey(Term::symbol(":artifact")),
                    cfg.artifact
                        .as_ref()
                        .map(|p| Term::Str(p.display().to_string()))
                        .unwrap_or(Term::Nil),
                ),
            ]
            .into_iter()
            .collect(),
        )
    } else {
        Term::Map(
            [(
                TermOrdKey(Term::symbol(":kind")),
                Term::symbol(":frontend/rust"),
            )]
            .into_iter()
            .collect(),
        )
    }
}

fn legacy_v01_obligation_cache_key(
    pkg_toml: &Path,
    manifest: &PackageManifest,
    modules: &[LoadedModule],
    caps_policy_hash: Option<&str>,
    limits: KernelLimits,
    frontend: &CoreformFrontend,
) -> String {
    let pkg_toml_hash = hash_optional_file(Some(pkg_toml))
        .unwrap()
        .unwrap_or_default();
    let module_hashes = Term::Vector(
        modules
            .iter()
            .map(|m| {
                Term::Map(
                    [
                        (
                            TermOrdKey(Term::symbol(":path")),
                            Term::Str(m.entry.path.clone()),
                        ),
                        (TermOrdKey(Term::symbol(":hash")), Term::Str(hex32(m.hash))),
                    ]
                    .into_iter()
                    .collect(),
                )
            })
            .collect(),
    );
    let key_term = Term::Map(
        [
            (
                TermOrdKey(Term::symbol(":kind")),
                Term::Str("genesis/obligation-cache-key-v0.1".to_string()),
            ),
            (
                TermOrdKey(Term::symbol(":pkg-name")),
                Term::Str(manifest.name.clone()),
            ),
            (
                TermOrdKey(Term::symbol(":pkg-version")),
                Term::Str(manifest.version.clone()),
            ),
            (
                TermOrdKey(Term::symbol(":pkg-toml-h")),
                Term::Str(pkg_toml_hash),
            ),
            (TermOrdKey(Term::symbol(":module-hashes")), module_hashes),
            (
                TermOrdKey(Term::symbol(":caps-policy-h")),
                caps_policy_hash
                    .map(|s| Term::Str(s.to_string()))
                    .unwrap_or(Term::Nil),
            ),
            (
                TermOrdKey(Term::symbol(":obligations")),
                Term::Vector(
                    manifest
                        .obligations
                        .iter()
                        .cloned()
                        .map(Term::Symbol)
                        .collect(),
                ),
            ),
            (
                TermOrdKey(Term::symbol(":tests")),
                Term::Vector(manifest.tests.iter().cloned().map(Term::Symbol).collect()),
            ),
            (
                TermOrdKey(Term::symbol(":property-tests")),
                Term::Vector(
                    manifest
                        .property_tests
                        .iter()
                        .cloned()
                        .map(Term::Symbol)
                        .collect(),
                ),
            ),
            (
                TermOrdKey(Term::symbol(":step-limit")),
                step_limit_term(limits.step_limit),
            ),
            (
                TermOrdKey(Term::symbol(":mem-limits")),
                mem_limits_term(limits.mem_limits),
            ),
            (
                TermOrdKey(Term::symbol(":frontend")),
                legacy_v01_frontend_term(frontend),
            ),
        ]
        .into_iter()
        .collect(),
    );
    hex32(hash_term(&key_term))
}

#[test]
fn obligation_cache_key_is_separated_from_legacy_toolchain_identity() {
    let dir = tempfile::tempdir().unwrap();
    let (pkg_toml, manifest, modules) = cache_key_fixture(dir.path());
    let limits = KernelLimits {
        step_limit: StepLimit::Default,
        mem_limits: MemLimits::default(),
    };
    let frontend = CoreformFrontend::Rust;

    let current =
        obligation_cache_key(&pkg_toml, &manifest, &modules, None, limits, &frontend).unwrap();
    let legacy =
        legacy_v01_obligation_cache_key(&pkg_toml, &manifest, &modules, None, limits, &frontend);

    assert_ne!(
        current, legacy,
        "v0.2 cache keys must not accept stale v0.1 entries that omitted toolchain identity"
    );
}

#[test]
fn obligation_cache_key_changes_when_selfhost_artifact_bytes_change() {
    let dir = tempfile::tempdir().unwrap();
    let (pkg_toml, manifest, modules) = cache_key_fixture(dir.path());
    let artifact = dir.path().join("toolchain.gc");
    let limits = KernelLimits {
        step_limit: StepLimit::Default,
        mem_limits: MemLimits::default(),
    };

    std::fs::write(&artifact, "artifact version a").unwrap();
    let frontend = CoreformFrontend::Selfhost(SelfhostFrontendConfig {
        bootstrap_mode: SelfhostBootstrapMode::ArtifactOnly,
        artifact: Some(artifact.clone()),
    });
    let first =
        obligation_cache_key(&pkg_toml, &manifest, &modules, None, limits, &frontend).unwrap();

    std::fs::write(&artifact, "artifact version b").unwrap();
    let second =
        obligation_cache_key(&pkg_toml, &manifest, &modules, None, limits, &frontend).unwrap();

    assert_ne!(
        first, second,
        "selfhost cache keys must include artifact content, not only artifact path"
    );
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
fn core_obligation_plan_contract_dedupes_and_preserves_order() {
    let src = r#"
            (core/obligation::plan
              ["core/obligation::unit-tests"
               "core/obligation::determinism"
               "core/obligation::unit-tests"
               "core/obligation::capabilities-declared"])
            "#;
    let term = eval_gc_term(src);
    let expected = Term::Map(
        [
            (
                TermOrdKey(Term::symbol(":kind")),
                Term::Str("genesis/obligation-plan-v0.2".to_string()),
            ),
            (
                TermOrdKey(Term::symbol(":rejected")),
                Term::Vector(Vec::<Term>::new()),
            ),
            (
                TermOrdKey(Term::symbol(":run")),
                Term::Vector(vec![
                    Term::Str("core/obligation::unit-tests".to_string()),
                    Term::Str("core/obligation::determinism".to_string()),
                    Term::Str("core/obligation::capabilities-declared".to_string()),
                ]),
            ),
        ]
        .into_iter()
        .collect(),
    );
    assert_eq!(term, expected);
}

#[test]
fn core_obligation_plan_contract_rejects_unknown_obligations() {
    let src = r#"
            (core/obligation::plan
              ["core/obligation::unit-tests"
               "core/obligation::unknown"
               "core/obligation::determinism"])
            "#;
    let term = eval_gc_term(src);
    let expected = Term::Map(
        [
            (
                TermOrdKey(Term::symbol(":kind")),
                Term::Str("genesis/obligation-plan-v0.2".to_string()),
            ),
            (
                TermOrdKey(Term::symbol(":rejected")),
                Term::Vector(vec![Term::Str("core/obligation::unknown".to_string())]),
            ),
            (
                TermOrdKey(Term::symbol(":run")),
                Term::Vector(vec![
                    Term::Str("core/obligation::unit-tests".to_string()),
                    Term::Str("core/obligation::determinism".to_string()),
                ]),
            ),
        ]
        .into_iter()
        .collect(),
    );
    assert_eq!(term, expected);
}

#[test]
fn obligation_plan_symbols_routes_through_gc_contract() {
    let obligations = vec![
        "core/obligation::unit-tests".to_string(),
        "core/obligation::determinism".to_string(),
        "core/obligation::unit-tests".to_string(),
        "core/obligation::capabilities-declared".to_string(),
    ];
    let planned = obligation_plan_symbols(&obligations).expect("plan should succeed");
    assert_eq!(
        planned,
        vec![
            "core/obligation::unit-tests".to_string(),
            "core/obligation::determinism".to_string(),
            "core/obligation::capabilities-declared".to_string(),
        ]
    );
}

#[test]
fn obligation_plan_symbols_rejects_unknown_obligations() {
    let obligations = vec![
        "core/obligation::unit-tests".to_string(),
        "core/obligation::unknown".to_string(),
    ];
    let err = obligation_plan_symbols(&obligations).expect_err("plan should reject unknown");
    let msg = err.to_string();
    assert!(
        msg.contains("rejected obligation entries: core/obligation::unknown"),
        "unexpected error: {msg}"
    );
}

#[test]
fn core_obligation_acceptance_ok_contract_folds_result_ok_bits() {
    let src = r#"
            {
              :all-ok
                (core/obligation::acceptance-ok
                  [{:name "core/obligation::unit-tests" :ok true}
                   {:name "core/obligation::determinism" :ok true}])
              :has-failure
                (core/obligation::acceptance-ok
                  [{:name "core/obligation::unit-tests" :ok true}
                   {:name "core/obligation::determinism" :ok false}])
            }
            "#;
    let term = eval_gc_term(src);
    let Some(all_ok) = map_get(&term, ":all-ok") else {
        panic!("all-ok report missing");
    };
    let Some(has_failure) = map_get(&term, ":has-failure") else {
        panic!("has-failure report missing");
    };
    assert_eq!(map_get(all_ok, ":ok"), Some(&Term::Bool(true)));
    assert_eq!(map_get(has_failure, ":ok"), Some(&Term::Bool(false)));
}

#[test]
fn obligation_acceptance_ok_routes_through_gc_contract() {
    let results = vec![
        ObligationResult {
            name: "core/obligation::unit-tests".to_string(),
            ok: true,
            artifact: None,
            errors: Vec::new(),
        },
        ObligationResult {
            name: "core/obligation::determinism".to_string(),
            ok: false,
            artifact: None,
            errors: Vec::new(),
        },
    ];
    let ok = obligation_acceptance_ok(&results).expect("acceptance fold should succeed");
    assert!(!ok, "acceptance fold should reflect failed obligations");
}
