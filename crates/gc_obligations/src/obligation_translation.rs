use super::*;
use crate::obligation_authority::{
    TranslationModuleObservation, TranslationObservation, TranslationOriginalTestObservation,
    TranslationStage2Observation, TranslationStage2Status, TranslationTestObservation,
    evaluate_translation_obligation_with_authority,
};

fn stage2_value_kind(kind: gc_opt::Stage2ValueKind) -> String {
    match kind {
        gc_opt::Stage2ValueKind::Int => ":int",
        gc_opt::Stage2ValueKind::Bool => ":bool",
        gc_opt::Stage2ValueKind::Nil => ":nil",
        gc_opt::Stage2ValueKind::Sym => ":sym",
        gc_opt::Stage2ValueKind::Str => ":str",
        gc_opt::Stage2ValueKind::Bytes => ":bytes",
        gc_opt::Stage2ValueKind::Term => ":term",
    }
    .to_string()
}

fn stage2_observation(report: gc_opt::Stage2ValidationReport) -> TranslationStage2Observation {
    let complete = report.wasm_value_hash.is_some();
    // A complete stage-2 report can fail only its two value-equivalence checks.
    // Convert that mechanism result to the primitive equality fact; do not
    // transport the legacy gate verdict or its derived mismatch messages.
    let result_equal = complete.then_some(report.ok);
    let status = if complete {
        TranslationStage2Status::Complete
    } else if report.supported {
        TranslationStage2Status::Failed
    } else {
        TranslationStage2Status::Unsupported
    };
    let mechanism_errors = if complete { Vec::new() } else { report.errors };
    TranslationStage2Observation {
        status,
        module_hash: report.module_hash,
        wasm_hash: report.wasm_hash,
        value_kind: report.value_kind.map(stage2_value_kind),
        original_value_hash: report.original_value_hash,
        result_equal,
        wasm_value_hash: report.wasm_value_hash,
        wasm_bytes_len: report.wasm_bytes_len.map(|value| value as u64),
        mechanism_errors,
    }
}

fn original_test_observation(test: &TestRun) -> TranslationOriginalTestObservation {
    TranslationOriginalTestObservation {
        suite: test.id.suite_sym.clone(),
        name: test.id.test_name.clone(),
        sealed_error: test.sealed_error,
        expected_hash: test.expected_hash,
        actual_hash: test.value_hash,
    }
}

fn initialize_selfhost_optimizer(
    frontend: &CoreformFrontend,
    limits: KernelLimits,
) -> Result<(Option<EvalCtx>, Option<Env>), ObligationError> {
    let CoreformFrontend::Selfhost(config) = frontend else {
        return Ok((None, None));
    };
    let mut context = EvalCtx::with_step_limit(None);
    context.set_mem_limits(limits.mem_limits);
    let prelude = build_prelude(&mut context);
    let mut environment = prelude.env;
    load_selfhost_coreform_toolchain_v1_with_mode(
        &mut context,
        &mut environment,
        config.bootstrap_mode,
        config.artifact.as_deref(),
    )
    .map_err(|error| ObligationError::Opt(format!("selfhost/init: {error}")))?;
    context.steps = 0;
    context.step_limit = limits.step_limit.resolve();
    Ok((Some(context), Some(environment)))
}

fn optimized_module_forms(
    module: &LoadedModule,
    rust_forms: &[Term],
    frontend: &CoreformFrontend,
    selfhost_context: &mut Option<EvalCtx>,
    selfhost_environment: &Option<Env>,
) -> Result<Vec<Term>, ObligationError> {
    if coreform_frontend_is_rust(frontend) {
        return Ok(rust_forms.to_vec());
    }
    let context = selfhost_context.as_mut().ok_or_else(|| {
        ObligationError::Opt("selfhost optimizer context was not initialized".to_string())
    })?;
    let environment = selfhost_environment.as_ref().ok_or_else(|| {
        ObligationError::Opt("selfhost optimizer environment was not initialized".to_string())
    })?;
    let raw = selfhost_optimize_module_forms(context, environment, &module.forms)?;
    let selfhost_forms = canonicalize_module(raw).map_err(|error| {
        ObligationError::Opt(format!("selfhost optimize canonicalize: {error}"))
    })?;
    if selfhost_forms != rust_forms {
        return Err(ObligationError::Opt(format!(
            "selfhost core/cli::optimize-module parity mismatch for {} (rust={} selfhost={})",
            module.entry.path,
            hex32(hash_module(rust_forms)),
            hex32(hash_module(&selfhost_forms)),
        )));
    }
    Ok(selfhost_forms)
}

#[expect(
    clippy::too_many_arguments,
    reason = "translation execution requires explicit package/test/frontend context"
)]
pub(super) fn obligation_translation_validation(
    store: &EvidenceStore,
    pkg_dir: &Path,
    manifest: &PackageManifest,
    modules: &[LoadedModule],
    caps: &CapsPolicy,
    test_runs: &[TestRun],
    limits: KernelLimits,
    frontend: &CoreformFrontend,
) -> Result<ObligationResult, ObligationError> {
    if test_runs.is_empty() {
        return evaluate_translation_obligation_with_authority(
            store,
            manifest,
            &TranslationObservation {
                modules: Vec::new(),
                original_tests: Vec::new(),
                optimized_tests: Vec::new(),
            },
            frontend,
            limits,
        );
    }

    let (mut selfhost_context, selfhost_environment) =
        initialize_selfhost_optimizer(frontend, limits)?;
    let mut optimized_modules = Vec::with_capacity(modules.len());
    let mut observations = Vec::with_capacity(modules.len());
    for module in modules {
        let original_hash = hash_module(&module.forms);
        let (raw, optimize_report) = gc_opt::optimize_module_with_report(&module.forms);
        let rust_forms = canonicalize_module(raw)
            .map_err(|error| ObligationError::Opt(format!("stage1 canonicalize: {error}")))?;
        let optimized_forms = optimized_module_forms(
            module,
            &rust_forms,
            frontend,
            &mut selfhost_context,
            &selfhost_environment,
        )?;
        let optimized_hash = hash_module(&optimized_forms);
        let stage2 = stage2_observation(gc_opt::stage2_validation_report(&optimized_forms));
        let stats = optimize_report.stats;
        observations.push(TranslationModuleObservation {
            path: module.entry.path.clone(),
            original_hash,
            optimized_hash,
            egg_runs: stats.egg_runs,
            egg_iterations: stats.iterations,
            egg_eclasses: stats.eclasses,
            egg_enodes: stats.enodes,
            rewrites: stats.rewrites_applied,
            stage2,
        });
        optimized_modules.push(LoadedModule {
            entry: module.entry.clone(),
            abs_path: module.abs_path.clone(),
            hash: optimized_hash,
            meta: extract_meta_static(&optimized_forms),
            forms: optimized_forms,
        });
    }

    let mut optimized_tests = Vec::with_capacity(test_runs.len());
    for original in test_runs {
        let optimized = run_one_test(
            pkg_dir,
            manifest,
            &optimized_modules,
            caps,
            original.id.clone(),
            limits,
        )?;
        optimized_tests.push(TranslationTestObservation {
            suite: original.id.suite_sym.clone(),
            name: original.id.test_name.clone(),
            original_hash: original.value_hash,
            optimized_hash: optimized.value_hash,
        });
    }
    evaluate_translation_obligation_with_authority(
        store,
        manifest,
        &TranslationObservation {
            modules: observations,
            original_tests: test_runs.iter().map(original_test_observation).collect(),
            optimized_tests,
        },
        frontend,
        limits,
    )
}
