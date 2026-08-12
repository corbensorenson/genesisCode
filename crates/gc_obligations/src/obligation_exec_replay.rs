use super::*;
use crate::obligation_authority::{
    ReplayEntryObservation, ReplayObservation, evaluate_replay_obligation_with_authority,
};

pub(crate) fn replay_observations(
    store: &EvidenceStore,
    pkg_dir: &Path,
    manifest: &PackageManifest,
    modules: &[LoadedModule],
    tests: &[TestRun],
    limits: KernelLimits,
) -> Result<Vec<ReplayObservation>, ObligationError> {
    let effect_store = gc_effects::ArtifactStore::open(&pkg_dir.join(".genesis").join("store"))
        .map_err(|error| ObligationError::Test(format!("artifact store open failed: {error}")))?;
    let mut observations = Vec::new();
    for test in tests {
        let Some(log) = &test.effect_log else {
            continue;
        };
        let log_artifact = store.put_term(&log.to_term())?;
        let entries = log
            .entries
            .iter()
            .enumerate()
            .map(|(position, entry)| ReplayEntryObservation {
                position: position as u64,
                op: entry.op.clone(),
                task_id: entry.task_id.clone(),
                schedule_step: entry.schedule_step,
                await_edge: entry.await_edge.clone(),
            })
            .collect();

        let mut ctx = mk_eval_ctx(limits);
        let prelude = build_prelude(&mut ctx);
        let mut base = prelude.env;
        base = eval_dependencies(&mut ctx, pkg_dir, &base, &manifest.dependencies)?;
        let evals = eval_modules(&mut ctx, &base, modules)?;
        let package = PackageEval::from_modules(base, evals)?;
        let suite = package.lookup_any(&test.id.suite_sym).ok_or_else(|| {
            ObligationError::Test(format!("missing test suite symbol {}", test.id.suite_sym))
        })?;
        let suite = value_as_map(&suite).ok_or_else(|| {
            ObligationError::Test(format!("test suite {} must be a map", test.id.suite_sym))
        })?;
        let (body, _) = parse_test_entry(
            suite
                .get(&TermOrdKey(Term::Str(test.id.test_name.clone())))
                .or_else(|| suite.get(&TermOrdKey(Term::Symbol(test.id.test_name.clone()))))
                .ok_or_else(|| {
                    ObligationError::Test(format!(
                        "missing test {} in suite {}",
                        test.id.test_name, test.id.suite_sym
                    ))
                })?,
        )?;
        let program = body
            .apply(&mut ctx, Value::data(Term::Nil))
            .map_err(|error| ObligationError::Test(format!("test apply failed: {error}")))?;
        let is_program = matches!(program, Value::EffectProgram(_));
        let replay_hash = if is_program {
            let replayed =
                gc_effects::replay_with_store(&mut ctx, program, log, Some(&effect_store))
                    .map_err(|error| ObligationError::Test(format!("replay failed: {error}")))?;
            Some(value_hash(&replayed))
        } else {
            None
        };
        observations.push(ReplayObservation {
            suite: test.id.suite_sym.clone(),
            name: test.id.test_name.clone(),
            log_artifact,
            program: is_program,
            actual_hash: test.value_hash,
            replay_hash,
            entries,
        });
    }
    Ok(observations)
}

pub(crate) fn run_replay_authority(
    operation: ObligationAuthorityOperation,
    store: &EvidenceStore,
    manifest: &PackageManifest,
    observations: &[ReplayObservation],
    frontend: &CoreformFrontend,
    limits: KernelLimits,
) -> Result<ObligationResult, ObligationError> {
    evaluate_replay_obligation_with_authority(
        operation,
        store,
        manifest,
        observations,
        frontend,
        limits,
    )
}
