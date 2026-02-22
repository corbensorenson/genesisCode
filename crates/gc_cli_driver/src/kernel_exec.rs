use gc_coreform::{Term, canonicalize_module, hash_module, parse_module};
use gc_kernel::{Env, EvalCtx, KernelError, Value, compile_module, eval_compiled_module};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum ModuleEvalBackend {
    Compiled,
}

impl ModuleEvalBackend {
    pub(crate) fn as_str(self) -> &'static str {
        match self {
            Self::Compiled => "compiled",
        }
    }
}

pub(crate) fn eval_module_default(
    ctx: &mut EvalCtx,
    env: &mut Env,
    forms: &[Term],
) -> Result<(Value, ModuleEvalBackend), KernelError> {
    let compiled = compile_module(forms)?;
    let value = eval_compiled_module(ctx, env, &compiled)?;
    Ok((value, ModuleEvalBackend::Compiled))
}

pub(crate) fn parse_canonicalize_hash_module_source(
    src: &str,
) -> Result<(Vec<Term>, [u8; 32]), String> {
    let forms = parse_module(src).map_err(|e| e.to_string())?;
    let forms = canonicalize_module(forms).map_err(|e| e.to_string())?;
    let module_hash = hash_module(&forms);
    Ok((forms, module_hash))
}

pub(crate) fn eval_module_default_value(
    ctx: &mut EvalCtx,
    env: &mut Env,
    forms: &[Term],
) -> Result<Value, String> {
    let (value, _) = eval_module_default(ctx, env, forms).map_err(|e| e.to_string())?;
    Ok(value)
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
