use gc_coreform::Term;
use gc_kernel::{
    Env, EvalCtx, KernelError, Value, compile_module, eval_compiled_module, eval_module,
};

const DISABLE_COMPILED_EVAL_ENV: &str = "GENESIS_DISABLE_COMPILED_EVAL";

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum ModuleEvalBackend {
    Compiled,
    TreeWalk,
}

impl ModuleEvalBackend {
    pub(crate) fn as_str(self) -> &'static str {
        match self {
            Self::Compiled => "compiled",
            Self::TreeWalk => "tree-walk",
        }
    }
}

fn compiled_eval_enabled() -> bool {
    std::env::var(DISABLE_COMPILED_EVAL_ENV)
        .ok()
        .map(|v| !is_truthy(&v))
        .unwrap_or(true)
}

fn is_truthy(value: &str) -> bool {
    matches!(
        value.trim().to_ascii_lowercase().as_str(),
        "1" | "true" | "yes" | "on"
    )
}

pub(crate) fn eval_module_default(
    ctx: &mut EvalCtx,
    env: &mut Env,
    forms: &[Term],
) -> Result<(Value, ModuleEvalBackend), KernelError> {
    if compiled_eval_enabled()
        && let Ok(compiled) = compile_module(forms)
    {
        let value = eval_compiled_module(ctx, env, &compiled)?;
        return Ok((value, ModuleEvalBackend::Compiled));
    }
    let value = eval_module(ctx, env, forms)?;
    Ok((value, ModuleEvalBackend::TreeWalk))
}

#[cfg(test)]
mod tests {
    use gc_coreform::parse_module;
    use gc_prelude::build_prelude;

    use super::{ModuleEvalBackend, eval_module_default};

    #[test]
    fn default_prefers_compiled_backend() {
        let forms = parse_module("(def sample/x 41)\n(prim int/add sample/x 1)\n").expect("parse");
        let mut ctx = gc_kernel::EvalCtx::with_step_limit(None);
        let prelude = build_prelude(&mut ctx);
        let mut env = prelude.env;
        let (_, backend) = eval_module_default(&mut ctx, &mut env, &forms).expect("eval");
        assert_eq!(backend, ModuleEvalBackend::Compiled);
    }
}
