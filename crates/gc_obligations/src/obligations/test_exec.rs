fn collect_test_ids(eval: &PackageEval, suites: &[String]) -> Result<Vec<TestId>, ObligationError> {
    let mut ids = Vec::new();
    for suite in suites {
        let v = eval
            .lookup_any(suite)
            .ok_or_else(|| ObligationError::Test(format!("missing test suite symbol {suite}")))?;
        let suite_map = value_as_map(&v)
            .ok_or_else(|| ObligationError::Test(format!("test suite {suite} must be a map")))?;
        for (k, _vv) in suite_map.iter() {
            let name = match &k.0 {
                Term::Str(s) => s.clone(),
                Term::Symbol(s) => s.clone(),
                other => {
                    return Err(ObligationError::Test(format!(
                        "test key must be string/symbol, got {}",
                        print_term(other)
                    )));
                }
            };
            ids.push(TestId {
                suite_sym: suite.clone(),
                test_name: name,
            });
        }
    }
    Ok(ids)
}

fn configured_test_workers(max_tests: usize) -> usize {
    let default_workers = std::thread::available_parallelism()
        .map(|n| n.get())
        .unwrap_or(1)
        .clamp(1, 8);
    let parsed = std::env::var(OBLIGATION_TEST_WORKERS_ENV)
        .ok()
        .and_then(|v| v.trim().parse::<usize>().ok())
        .unwrap_or(default_workers);
    parsed.clamp(1, 64).min(max_tests.max(1))
}

fn run_test_batch_with_frontend(
    pkg_dir: &Path,
    manifest: &PackageManifest,
    modules: &[LoadedModule],
    caps: &CapsPolicy,
    limits: KernelLimits,
    frontend: &CoreformFrontend,
    batch: Vec<(usize, TestId)>,
) -> Result<Vec<(usize, TestRun)>, ObligationError> {
    if batch.is_empty() {
        return Ok(Vec::new());
    }
    let mut ctx = mk_eval_ctx(limits);
    let prelude = build_prelude(&mut ctx);
    let mut base = prelude.env;
    base = eval_dependencies_with_frontend(
        &mut ctx,
        pkg_dir,
        &base,
        &manifest.dependencies,
        limits,
        frontend,
    )?;
    let evals = eval_modules(&mut ctx, &base, modules)?;
    let pkg = PackageEval::from_modules(base, evals)?;
    let baseline_state = ctx.state;

    let mut out = Vec::with_capacity(batch.len());
    for (idx, id) in batch {
        ctx.state = baseline_state;
        ctx.step_limit = limits.step_limit.resolve();
        ctx.reset_counters();
        let run = run_test_from_package(&mut ctx, &pkg, caps, id)?;
        out.push((idx, run));
    }
    Ok(out)
}

fn run_tests_with_frontend(
    pkg_dir: &Path,
    manifest: &PackageManifest,
    modules: &[LoadedModule],
    caps: &CapsPolicy,
    limits: KernelLimits,
    frontend: &CoreformFrontend,
) -> Result<Vec<TestRun>, ObligationError> {
    if manifest.tests.is_empty() {
        return Ok(Vec::new());
    }

    // First pass builds a deterministic test-id list using one package evaluation.
    let mut ctx = mk_eval_ctx(limits);
    let prelude = build_prelude(&mut ctx);
    let mut base = prelude.env;
    base = eval_dependencies_with_frontend(
        &mut ctx,
        pkg_dir,
        &base,
        &manifest.dependencies,
        limits,
        frontend,
    )?;
    let evals = eval_modules(&mut ctx, &base, modules)?;
    let pkg = PackageEval::from_modules(base, evals)?;
    let test_ids = collect_test_ids(&pkg, &manifest.tests)?;
    if test_ids.is_empty() {
        return Ok(Vec::new());
    }
    let workers = configured_test_workers(test_ids.len());

    // Single-worker path reuses the prepared package snapshot and is the lowest-overhead option.
    if workers == 1 {
        let baseline_state = ctx.state;
        let mut out = Vec::with_capacity(test_ids.len());
        for id in test_ids {
            ctx.state = baseline_state;
            ctx.step_limit = limits.step_limit.resolve();
            ctx.reset_counters();
            out.push(run_test_from_package(&mut ctx, &pkg, caps, id)?);
        }
        return Ok(out);
    }

    // Multi-worker path: deterministic partitioning by original index, isolated eval contexts per worker.
    let mut buckets: Vec<Vec<(usize, TestId)>> = vec![Vec::new(); workers];
    for (i, id) in test_ids.iter().cloned().enumerate() {
        buckets[i % workers].push((i, id));
    }

    let pkg_dir = pkg_dir.to_path_buf();
    let manifest = manifest.clone();
    let modules = modules.to_vec();
    let caps = caps.clone();
    let frontend = frontend.clone();
    let mut worker_results: Vec<Vec<(usize, TestRun)>> = Vec::new();
    std::thread::scope(|scope| -> Result<(), ObligationError> {
        let mut handles = Vec::new();
        for batch in buckets {
            if batch.is_empty() {
                continue;
            }
            let pkg_dir = pkg_dir.clone();
            let manifest = manifest.clone();
            let modules = modules.clone();
            let caps = caps.clone();
            let frontend = frontend.clone();
            handles.push(scope.spawn(move || {
                run_test_batch_with_frontend(
                    &pkg_dir, &manifest, &modules, &caps, limits, &frontend, batch,
                )
            }));
        }

        for h in handles {
            match h.join() {
                Ok(Ok(rows)) => worker_results.push(rows),
                Ok(Err(e)) => return Err(e),
                Err(_) => {
                    return Err(ObligationError::Test(
                        "parallel test worker panicked".to_string(),
                    ));
                }
            }
        }
        Ok(())
    })?;

    let mut ordered: Vec<Option<TestRun>> = (0..test_ids.len()).map(|_| None).collect();
    for rows in worker_results {
        for (idx, run) in rows {
            if idx >= ordered.len() || ordered[idx].is_some() {
                return Err(ObligationError::Test(
                    "parallel test collation mismatch".to_string(),
                ));
            }
            ordered[idx] = Some(run);
        }
    }
    let mut out = Vec::with_capacity(test_ids.len());
    for row in ordered {
        let Some(run) = row else {
            return Err(ObligationError::Test(
                "parallel test collation dropped a test".to_string(),
            ));
        };
        out.push(run);
    }
    Ok(out)
}

fn run_test_from_package(
    ctx: &mut EvalCtx,
    pkg: &PackageEval,
    caps: &CapsPolicy,
    id: TestId,
) -> Result<TestRun, ObligationError> {
    let suite_v = pkg.lookup_any(&id.suite_sym).ok_or_else(|| {
        ObligationError::Test(format!("missing test suite symbol {}", id.suite_sym))
    })?;
    let suite_map = value_as_map(&suite_v).ok_or_else(|| {
        ObligationError::Test(format!("test suite {} must be a map", id.suite_sym))
    })?;
    let (test_body, expect) = parse_test_entry(
        suite_map
            .get(&TermOrdKey(Term::Str(id.test_name.clone())))
            .or_else(|| suite_map.get(&TermOrdKey(Term::Symbol(id.test_name.clone()))))
            .ok_or_else(|| {
                ObligationError::Test(format!(
                    "missing test {} in suite {}",
                    id.test_name, id.suite_sym
                ))
            })?,
    )?;

    let value = test_body
        .apply(ctx, Value::Data(Term::Nil))
        .map_err(|e| ObligationError::Test(format!("test apply failed: {e}")))?;

    let (final_value, effect_log) = match value {
        Value::EffectProgram(_) => {
            let prog_h = value_hash(&value);
            let toolchain = format!("genesis {}", env!("CARGO_PKG_VERSION"));
            let r = gc_effects::run(ctx, caps, value, prog_h, toolchain)
                .map_err(|e| ObligationError::Test(format!("effect run failed: {e}")))?;
            (r.value, Some(r.log))
        }
        other => (other, None),
    };
    let steps = ctx.steps;
    let effect_entries = effect_log
        .as_ref()
        .map(|l| l.entries.len() as u64)
        .unwrap_or(0);
    let effect_log_bytes = effect_log
        .as_ref()
        .map(|l| l.to_string_canonical().len() as u64)
        .unwrap_or(0);

    let is_error = ctx
        .protocol
        .is_some_and(|p| matches!(final_value, Value::Sealed { token, .. } if token == p.error));

    let fv_hash = value_hash(&final_value);
    let ok = if is_error {
        false
    } else if let Some(exp) = expect {
        fv_hash == value_hash(&Value::Data(exp))
    } else {
        true
    };

    Ok(TestRun {
        id,
        ok,
        effect_log,
        steps,
        effect_entries,
        effect_log_bytes,
        value_hash: fv_hash,
        error: if ok {
            None
        } else {
            Some("test failed".to_string())
        },
    })
}

fn run_one_test(
    pkg_dir: &Path,
    manifest: &PackageManifest,
    modules: &[LoadedModule],
    caps: &CapsPolicy,
    id: TestId,
    limits: KernelLimits,
) -> Result<TestRun, ObligationError> {
    run_one_test_with_frontend(
        pkg_dir,
        manifest,
        modules,
        caps,
        id,
        limits,
        &default_coreform_frontend(),
    )
}

fn run_one_test_with_frontend(
    pkg_dir: &Path,
    manifest: &PackageManifest,
    modules: &[LoadedModule],
    caps: &CapsPolicy,
    id: TestId,
    limits: KernelLimits,
    frontend: &CoreformFrontend,
) -> Result<TestRun, ObligationError> {
    let mut ctx = mk_eval_ctx(limits);
    let prelude = build_prelude(&mut ctx);
    let mut base = prelude.env;

    // Evaluate dependencies (export-only) into base env.
    base = eval_dependencies_with_frontend(
        &mut ctx,
        pkg_dir,
        &base,
        &manifest.dependencies,
        limits,
        frontend,
    )?;

    // Evaluate modules and collect module envs for internal lookup.
    let evals = eval_modules(&mut ctx, &base, modules)?;
    let pkg = PackageEval::from_modules(base, evals)?;
    ctx.reset_counters();
    run_test_from_package(&mut ctx, &pkg, caps, id)
}

fn eval_package_once(
    pkg_dir: &Path,
    manifest: &PackageManifest,
    modules: &[LoadedModule],
    limits: KernelLimits,
) -> Result<PackageEval, ObligationError> {
    eval_package_once_with_frontend(
        pkg_dir,
        manifest,
        modules,
        limits,
        &default_coreform_frontend(),
    )
}

fn eval_package_once_with_frontend(
    pkg_dir: &Path,
    manifest: &PackageManifest,
    modules: &[LoadedModule],
    limits: KernelLimits,
    frontend: &CoreformFrontend,
) -> Result<PackageEval, ObligationError> {
    let mut ctx = mk_eval_ctx(limits);
    let prelude = build_prelude(&mut ctx);
    let mut base = prelude.env;
    base = eval_dependencies_with_frontend(
        &mut ctx,
        pkg_dir,
        &base,
        &manifest.dependencies,
        limits,
        frontend,
    )?;
    let evals = eval_modules(&mut ctx, &base, modules)?;
    PackageEval::from_modules(base, evals)
}
