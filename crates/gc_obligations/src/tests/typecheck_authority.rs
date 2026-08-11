use super::*;

fn input_module(path: &str, exports: &[&str]) -> TypecheckModuleInput {
    let source = exports
        .iter()
        .map(|name| format!("(def {name} 1)"))
        .collect::<Vec<_>>()
        .join("\n");
    let forms = canonicalize_module(parse_module(&source).expect("parse fixture"))
        .expect("canonicalize fixture");
    let export_terms = exports.iter().map(|name| Term::symbol(*name)).collect();
    let declared_types = exports
        .iter()
        .map(|name| (TermOrdKey(Term::symbol(*name)), Term::symbol("?")))
        .collect();
    let meta = Term::Map(
        [
            (
                TermOrdKey(Term::symbol(":exports")),
                Term::Vector(export_terms),
            ),
            (TermOrdKey(Term::symbol(":caps")), Term::Vector(Vec::new())),
            (
                TermOrdKey(Term::symbol(":types")),
                Term::Map(declared_types),
            ),
        ]
        .into_iter()
        .collect(),
    );
    TypecheckModuleInput {
        path: path.to_string(),
        forms,
        meta: Some(meta),
    }
}

fn input_module_from_source(path: &str, source: &str) -> TypecheckModuleInput {
    let forms = canonicalize_module(parse_module(source).expect("parse fixture"))
        .expect("canonicalize fixture");
    let meta = extract_meta_static(&forms);
    TypecheckModuleInput {
        path: path.to_string(),
        forms,
        meta,
    }
}

fn active_profile_source(
    export: &str,
    capability_mode: &str,
    capability_profile: &str,
    target_profile: &str,
    caps: &str,
) -> String {
    format!(
        r#"
        (def ::meta '{{
          :module-profile genesis/module-resolution-profile-v0.1
          :requires-profiles {{
            genesis/coreform-profile genesis/coreform/v0.2
            genesis/hash-profile genesis/hash-profile/gcv0.2-blake3
            genesis/language-profile genesis/language-profile/v0.2
            genesis/module-resolution-profile genesis/module-resolution-profile-v0.1}}
          :profile-negotiation genesis/profile-negotiation-v0.1
          :package-profile-requirements {{
            genesis/profile-family/language {{:mode exact :profile genesis/language-profile/v0.2}}
            genesis/profile-family/capability {{:mode {capability_mode} :profile {capability_profile}}}
            genesis/profile-family/artifact {{:mode exact :profile genesis/artifact-profile/coreform-v0.2}}
            genesis/profile-family/target {{:mode exact :profile {target_profile}}}}}
          :imports []
          :exports [{export}]
          :caps {caps}
          :types {{{export} Int}}
          :strict-shapes true
          :strict-effects true}})
        (def {export} 1)
        "#
    )
}

fn report_term(inputs: &[TypecheckModuleInput]) -> Term {
    let modules = inputs
        .iter()
        .map(|module| gc_types::ModuleForTypecheck {
            path: module.path.clone(),
            forms: module.forms.clone(),
            meta: module.meta.clone(),
        })
        .collect::<Vec<_>>();
    gc_types::typecheck_package(&modules).to_term()
}

fn field_mut<'a>(term: &'a mut Term, field: &str) -> &'a mut Term {
    let Term::Map(map) = term else {
        panic!("fixture field parent must be a map")
    };
    map.get_mut(&TermOrdKey(Term::symbol(field)))
        .expect("fixture field must exist")
}

fn source_typecheck_environment() -> (EvalCtx, Env) {
    let root = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../..");
    let manifest = parse_term(
        &std::fs::read_to_string(root.join("selfhost/toolchain_manifest.gc"))
            .expect("read toolchain manifest"),
    )
    .expect("parse toolchain manifest");
    let Term::Map(manifest) = manifest else {
        panic!("toolchain manifest must be a map")
    };
    let Some(Term::Vector(module_paths)) = manifest.get(&TermOrdKey(Term::symbol(":module-paths")))
    else {
        panic!("toolchain manifest must declare module paths")
    };

    let mut ctx = EvalCtx::with_step_limit(None);
    let prelude = build_prelude(&mut ctx);
    let mut env = prelude.env;
    for module_path in module_paths {
        let Term::Str(module_path) = module_path else {
            panic!("toolchain module path must be a string")
        };
        let source =
            std::fs::read_to_string(root.join(module_path)).expect("read toolchain module");
        let forms = canonicalize_module(parse_module(&source).expect("parse toolchain module"))
            .expect("canonicalize toolchain module");
        gc_kernel::eval_module(&mut ctx, &mut env, &forms).expect("evaluate toolchain module");
        if module_path == "selfhost/typecheck_package_report_v1.gc" {
            break;
        }
    }

    (ctx, env)
}

fn source_typecheck_report(inputs: &[TypecheckModuleInput]) -> AuthoritativeTypecheckReport {
    let (mut ctx, env) = source_typecheck_environment();

    ctx.steps = 0;
    ctx.step_limit = StepLimit::Default.resolve();
    let checker = env
        .get("core/cli::typecheck-package")
        .expect("source toolchain typecheck binding");
    let value = checker
        .apply(&mut ctx, Value::data(typecheck_request_term(inputs)))
        .expect("invoke source toolchain typecheck");
    let term = value
        .as_data()
        .cloned()
        .unwrap_or_else(|| value.to_term_for_log(ctx.protocol.map(|protocol| protocol.error)));
    decode_typecheck_report(term.clone(), inputs).unwrap_or_else(|error| {
        panic!(
            "decode selfhost typecheck report: {error}\nraw report: {}",
            print_term(&term)
        )
    })
}

#[test]
fn typecheck_report_binds_exact_module_order_count_and_paths() {
    let inputs = vec![input_module("a.gc", &["a"]), input_module("b.gc", &["b"])];
    decode_typecheck_report(report_term(&inputs), &inputs).expect("valid report");

    let mut reordered = report_term(&inputs);
    let Term::Vector(modules) = field_mut(&mut reordered, ":modules") else {
        panic!("fixture modules must be a vector")
    };
    modules.swap(0, 1);
    let error = decode_typecheck_report(reordered, &inputs).expect_err("reordered report");
    assert!(error.to_string().contains("module 0 path mismatch"));

    let mut omitted = report_term(&inputs);
    let Term::Vector(modules) = field_mut(&mut omitted, ":modules") else {
        panic!("fixture modules must be a vector")
    };
    modules.pop();
    let error = decode_typecheck_report(omitted, &inputs).expect_err("omitted report");
    assert!(error.to_string().contains("module count mismatch"));

    let duplicate_inputs = vec![inputs[0].clone(), inputs[0].clone()];
    let error = decode_typecheck_report(report_term(&duplicate_inputs), &duplicate_inputs)
        .expect_err("duplicate request path");
    assert!(error.to_string().contains("duplicate module path a.gc"));
}

#[test]
fn typecheck_report_binds_declared_export_inventory_and_types() {
    let inputs = vec![input_module("exports.gc", &["alpha", "beta"])];
    decode_typecheck_report(report_term(&inputs), &inputs).expect("valid report");

    let mut omitted_export = report_term(&inputs);
    let Term::Vector(modules) = field_mut(&mut omitted_export, ":modules") else {
        panic!("fixture modules must be a vector")
    };
    let Term::Vector(exports) = field_mut(&mut modules[0], ":exports") else {
        panic!("fixture exports must be a vector")
    };
    exports.pop();
    let error = decode_typecheck_report(omitted_export, &inputs).expect_err("omitted export");
    assert!(error.to_string().contains("export inventory mismatch"));

    let mut changed_declared_type = report_term(&inputs);
    let Term::Vector(modules) = field_mut(&mut changed_declared_type, ":modules") else {
        panic!("fixture modules must be a vector")
    };
    let Term::Vector(types) = field_mut(&mut modules[0], ":types") else {
        panic!("fixture types must be a vector")
    };
    *field_mut(&mut types[0], ":declared") = Term::symbol("Int");
    let error =
        decode_typecheck_report(changed_declared_type, &inputs).expect_err("changed declared type");
    assert!(error.to_string().contains("declared type mismatch"));

    let mut duplicate_export_input = input_module("duplicate.gc", &["same", "same"]);
    duplicate_export_input.forms = input_module("duplicate.gc", &["same"]).forms;
    let duplicate_inputs = vec![duplicate_export_input];
    let error = decode_typecheck_report(report_term(&duplicate_inputs), &duplicate_inputs)
        .expect_err("duplicate requested export");
    assert!(error.to_string().contains("duplicate export same"));
}

#[test]
fn selfhost_typecheck_producer_satisfies_bound_report_contract() {
    let inputs = vec![input_module("selfhost.gc", &["answer"])];
    let report = source_typecheck_report(&inputs);
    assert_eq!(report.modules.len(), 1);
    assert_eq!(report.modules[0].path, "selfhost.gc");
    assert_eq!(report.modules[0].export_effects[0].name, "answer");
    assert_eq!(report.modules[0].export_types[0].name, "answer");
}

#[test]
fn selfhost_typecheck_marks_export_calls_through_local_any_as_unknown() {
    let source = std::fs::read_to_string(
        Path::new(env!("CARGO_MANIFEST_DIR")).join("../../tests/spec/pkg_fail_budgets/budget.gc"),
    )
    .expect("read budget regression fixture");
    let inputs = vec![input_module_from_source("budget.gc", &source)];
    assert_eq!(
        source_typecheck_report(&inputs).to_term(),
        report_term(&inputs)
    );
}

#[test]
fn selfhost_typecheck_matches_real_failure_corpus_regressions() {
    let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("../..");
    for (package, module) in [
        ("pkg_fail_caps_declared", "fail.gc"),
        ("pkg_fail_coverage", "fail.gc"),
        ("pkg_fail_determinism", "fail.gc"),
        ("pkg_fail_lint", "lint.gc"),
        ("pkg_fail_unit", "fail.gc"),
    ] {
        let source = std::fs::read_to_string(root.join("tests/spec").join(package).join(module))
            .expect("read typecheck authority corpus fixture");
        let inputs = vec![input_module_from_source(module, &source)];
        assert_eq!(
            source_typecheck_report(&inputs).to_term(),
            report_term(&inputs),
            "self-host authority drifted on {package}"
        );
    }
}

#[test]
fn selfhost_typecheck_active_profile_report_matches_rust_oracle() {
    let source = active_profile_source(
        "pkg/main::value",
        "minimum",
        "genesis/capability-profile/pure-v0.1",
        "genesis/target-profile/portable-host-v0.1",
        "[]",
    );
    let inputs = vec![input_module_from_source("pkg/main.gc", &source)];
    let source_report = source_typecheck_report(&inputs);

    assert_eq!(
        source_report.to_term(),
        report_term(&inputs),
        "self-host authority must preserve every serialized type/effect and profile fact"
    );
}

#[test]
fn selfhost_typecheck_active_profile_failures_match_rust_oracle() {
    let fixtures = [
        active_profile_source(
            "pkg/unsupported::value",
            "exact",
            "genesis/capability-profile/pure-v0.1",
            "genesis/target-profile/browser-v9",
            "[]",
        ),
        active_profile_source(
            "pkg/impure::value",
            "exact",
            "genesis/capability-profile/pure-v0.1",
            "genesis/target-profile/portable-host-v0.1",
            "[io/fs::read]",
        ),
    ];

    for (index, source) in fixtures.iter().enumerate() {
        let path = format!("pkg/failure-{index}.gc");
        let inputs = vec![input_module_from_source(&path, source)];
        let source_report = source_typecheck_report(&inputs);
        assert!(
            !source_report.ok,
            "negative control {index} unexpectedly passed"
        );
        assert_eq!(
            source_report.to_term(),
            report_term(&inputs),
            "self-host authority drifted on active profile failure {index}"
        );
    }
}

#[test]
fn typecheck_report_profile_negotiation_is_request_bound_and_coherent() {
    let inputs = vec![input_module("profile.gc", &["value"])];

    let mut unexpected_active = report_term(&inputs);
    let profile = field_mut(&mut unexpected_active, ":profile-negotiation");
    *field_mut(profile, ":active") = Term::Bool(true);
    let error = decode_typecheck_report(unexpected_active, &inputs).expect_err("unexpected active");
    assert!(error.to_string().contains(":active disagrees"));

    let mut active_inputs = inputs.clone();
    let Some(Term::Map(meta)) = active_inputs[0].meta.as_mut() else {
        panic!("fixture metadata must be a map")
    };
    meta.insert(
        TermOrdKey(Term::symbol(":profile-negotiation")),
        Term::symbol("genesis/profile-negotiation-v0.1"),
    );
    let error = decode_typecheck_report(report_term(&inputs), &active_inputs)
        .expect_err("inactive report for active request");
    assert!(error.to_string().contains(":active disagrees"));

    let mut contradictory_identity = report_term(&inputs);
    let profile = field_mut(&mut contradictory_identity, ":profile-negotiation");
    *field_mut(profile, ":identity") = Term::Bytes(vec![7; 32].into());
    let error = decode_typecheck_report(contradictory_identity, &inputs)
        .expect_err("identity on inactive report");
    assert!(error.to_string().contains("identity presence disagrees"));
}
