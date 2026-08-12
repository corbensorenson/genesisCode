use super::*;
use crate::obligation_authority::{Stage1Observation, evaluate_stage1_obligation_with_authority};

pub(super) fn observe_stage1_eval(
    forms: &[Term],
    limits: KernelLimits,
) -> (Option<[u8; 32]>, Option<String>) {
    let mut ctx = EvalCtx::with_step_limit(limits.step_limit.resolve());
    ctx.set_mem_limits(limits.mem_limits);
    let prelude = build_prelude(&mut ctx);
    let mut env = prelude.env;
    match gc_kernel::eval_module(&mut ctx, &mut env, forms) {
        Ok(Value::EffectProgram(_)) => {
            (None, Some("effect program produced (not pure)".to_string()))
        }
        Ok(value) => (Some(value_hash(&value)), None),
        Err(error) => (None, Some(error.to_string())),
    }
}

pub(super) fn obligation_stage1_validation(
    store: &EvidenceStore,
    manifest: &PackageManifest,
    modules: &[LoadedModule],
    frontend: &CoreformFrontend,
    limits: KernelLimits,
) -> Result<ObligationResult, ObligationError> {
    let mut observations = Vec::with_capacity(modules.len());
    for m in modules {
        let (optimized, optimize_report) = gc_opt::optimize_module_with_report(&m.forms);
        let transformed = canonicalize_module(optimized)
            .map_err(|error| ObligationError::Opt(format!("stage1 canonicalize: {error}")))?;
        let (original_value_hash, original_eval_error) = observe_stage1_eval(&m.forms, limits);
        let (transformed_value_hash, transformed_eval_error) =
            observe_stage1_eval(&transformed, limits);
        let optimizer_stats = optimize_report.stats;
        observations.push(Stage1Observation {
            path: m.entry.path.clone(),
            original_module_hash: hash_module(&m.forms),
            transformed_module_hash: hash_module(&transformed),
            original_value_hash,
            transformed_value_hash,
            original_eval_error,
            transformed_eval_error,
            egg_runs: optimizer_stats.egg_runs,
            egg_iterations: optimizer_stats.iterations,
            egg_eclasses: optimizer_stats.eclasses,
            egg_enodes: optimizer_stats.enodes,
        });
    }
    evaluate_stage1_obligation_with_authority(store, manifest, &observations, frontend, limits)
}

pub(super) struct PackageEval {
    modules: Vec<ModuleEval>,
    pub(super) exports_env: Env,
    internal_index: BTreeMap<String, usize>,
}

impl PackageEval {
    pub(super) fn from_modules(
        base_env: Env,
        modules: Vec<ModuleEval>,
    ) -> Result<Self, ObligationError> {
        let mut exports = BTreeMap::new();
        let mut internal_index = BTreeMap::new();
        for (index, module) in modules.iter().enumerate() {
            for name in module.defined.keys() {
                internal_index.entry(name.clone()).or_insert(index);
            }
            for export in &module.exports {
                let value = module.defined.get(export).ok_or_else(|| {
                    ObligationError::Module(format!(
                        "module {} exports {} but does not define it",
                        module.path.display(),
                        export
                    ))
                })?;
                exports.insert(export.clone(), value.clone());
            }
        }
        Ok(Self {
            modules,
            exports_env: Env::with_bindings(&base_env, exports),
            internal_index,
        })
    }

    pub(super) fn lookup_any(&self, name: &str) -> Option<Value> {
        if let Some(value) = self.exports_env.get(name) {
            return Some(value);
        }
        let index = self.internal_index.get(name)?;
        self.modules[*index].env.get(name)
    }
}
