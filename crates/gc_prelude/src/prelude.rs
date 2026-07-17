use std::collections::BTreeMap;

use blake3::Hasher;

use gc_coreform::{Term, TermOrdKey, canonicalize_module, parse_module};
use gc_kernel::{
    Apply, Contract, EffectProgram, EffectRequest, Env, EvalCtx, KernelError, KernelErrorKind,
    NativeFn, ProtocolTokens, SealId, Shared, Value, ValueMap, value_hash,
};

mod prelude_assembled {
    include!(concat!(env!("OUT_DIR"), "/prelude_assembled.rs"));
}
#[path = "prelude_contract_effect.rs"]
mod prelude_contract_effect;
#[path = "prelude_coreform_api.rs"]
mod prelude_coreform_api;
use prelude_contract_effect::*;
use prelude_coreform_api::{
    nf_coreform_canonicalize_module, nf_coreform_fmt_module, nf_coreform_hash_module,
    nf_coreform_hash_module_src, nf_coreform_hash_term, nf_coreform_parse_module,
    nf_coreform_parse_term, nf_coreform_print_module, nf_coreform_print_term,
};

pub struct Prelude {
    pub env: Env,
    pub protocol: ProtocolTokens,
}

pub fn build_prelude(ctx: &mut EvalCtx) -> Prelude {
    // Protocol tokens are reserved by default in EvalCtx::new(); keep this total as a
    // defense-in-depth fallback if a caller constructed an EvalCtx differently.
    let protocol = match ctx.protocol {
        Some(p) => p,
        None => {
            let unhandled = SealId(ctx.state.next_seal_id);
            ctx.state.next_seal_id = ctx.state.next_seal_id.saturating_add(1);
            let effect = SealId(ctx.state.next_seal_id);
            ctx.state.next_seal_id = ctx.state.next_seal_id.saturating_add(1);
            let error = SealId(ctx.state.next_seal_id);
            ctx.state.next_seal_id = ctx.state.next_seal_id.saturating_add(1);
            let p = ProtocolTokens {
                unhandled,
                effect,
                error,
            };
            ctx.protocol = Some(p);
            p
        }
    };

    let mut env = Env::empty();

    // Protocol constructors / predicates.
    env = Env::with_binding(
        &env,
        "core/protocol::unhandled",
        Value::native_fn(NativeFn::new("core/protocol::unhandled", 1, nf_unhandled)),
    );
    env = Env::with_binding(
        &env,
        "core/protocol::is-unhandled",
        Value::native_fn(NativeFn::new(
            "core/protocol::is-unhandled",
            1,
            nf_is_unhandled,
        )),
    );
    env = Env::with_binding(
        &env,
        "core/protocol::effect",
        Value::native_fn(NativeFn::new("core/protocol::effect", 1, nf_effect_wrap)),
    );
    env = Env::with_binding(
        &env,
        "core/protocol::is-effect",
        Value::native_fn(NativeFn::new("core/protocol::is-effect", 1, nf_is_effect)),
    );
    env = Env::with_binding(
        &env,
        "core/protocol::error",
        Value::native_fn(NativeFn::new("core/protocol::error", 1, nf_error)),
    );
    env = Env::with_binding(
        &env,
        "core/protocol::is-error",
        Value::native_fn(NativeFn::new("core/protocol::is-error", 1, nf_is_error)),
    );
    env = Env::with_binding(
        &env,
        "core/protocol::unerror",
        Value::native_fn(NativeFn::new("core/protocol::unerror", 1, nf_unerror)),
    );

    // Contract API.
    env = Env::with_binding(
        &env,
        "core/contract::make",
        Value::native_fn(NativeFn::new("core/contract::make", 3, nf_contract_make)),
    );
    env = Env::with_binding(
        &env,
        "core/contract::extend",
        Value::native_fn(NativeFn::new(
            "core/contract::extend",
            3,
            nf_contract_extend,
        )),
    );
    env = Env::with_binding(
        &env,
        "core/contract::dispatch",
        Value::native_fn(NativeFn::new(
            "core/contract::dispatch",
            2,
            nf_contract_dispatch,
        )),
    );
    env = Env::with_binding(
        &env,
        "core/contract::explain",
        Value::native_fn(NativeFn::new(
            "core/contract::explain",
            2,
            nf_contract_explain,
        )),
    );
    env = Env::with_binding(
        &env,
        "core/contract::meta",
        Value::native_fn(NativeFn::new("core/contract::meta", 1, nf_contract_meta)),
    );
    env = Env::with_binding(
        &env,
        "core/contract::proto",
        Value::native_fn(NativeFn::new("core/contract::proto", 1, nf_contract_proto)),
    );
    env = Env::with_binding(
        &env,
        "core/contract::shape",
        Value::native_fn(NativeFn::new("core/contract::shape", 1, nf_contract_shape)),
    );

    // Message helpers.
    env = Env::with_binding(
        &env,
        "core/msg::make",
        Value::native_fn(NativeFn::new("core/msg::make", 2, nf_msg_make)),
    );
    env = Env::with_binding(
        &env,
        "core/msg::op",
        Value::native_fn(NativeFn::new("core/msg::op", 1, nf_msg_op)),
    );
    env = Env::with_binding(
        &env,
        "core/msg::payload",
        Value::native_fn(NativeFn::new("core/msg::payload", 1, nf_msg_payload)),
    );

    // Effects.
    env = Env::with_binding(
        &env,
        "core/effect::pure",
        Value::native_fn(NativeFn::new("core/effect::pure", 1, nf_effect_pure)),
    );
    env = Env::with_binding(
        &env,
        "core/effect::perform",
        Value::native_fn(NativeFn::new("core/effect::perform", 3, nf_effect_perform)),
    );
    env = Env::with_binding(
        &env,
        "core/effect::bind",
        Value::native_fn(NativeFn::new("core/effect::bind", 2, nf_effect_bind)),
    );

    // Bootstrap CoreForm API (pure, deterministic).
    //
    // These are used for wasm-first and self-host bootstrap stages. They intentionally expose
    // parser/printer/canonicalizer operations as pure functions inside the language so GenesisCode
    // tooling can be written in GenesisCode and hosted under WASM without a second bootstrap.
    env = Env::with_binding(
        &env,
        "core/coreform::parse-term",
        Value::native_fn(NativeFn::new(
            "core/coreform::parse-term",
            1,
            nf_coreform_parse_term,
        )),
    );
    env = Env::with_binding(
        &env,
        "core/coreform::parse-module",
        Value::native_fn(NativeFn::new(
            "core/coreform::parse-module",
            1,
            nf_coreform_parse_module,
        )),
    );
    env = Env::with_binding(
        &env,
        "core/coreform::canonicalize-module",
        Value::native_fn(NativeFn::new(
            "core/coreform::canonicalize-module",
            1,
            nf_coreform_canonicalize_module,
        )),
    );
    env = Env::with_binding(
        &env,
        "core/coreform::print-term",
        Value::native_fn(NativeFn::new(
            "core/coreform::print-term",
            1,
            nf_coreform_print_term,
        )),
    );
    env = Env::with_binding(
        &env,
        "core/coreform::print-module",
        Value::native_fn(NativeFn::new(
            "core/coreform::print-module",
            1,
            nf_coreform_print_module,
        )),
    );
    env = Env::with_binding(
        &env,
        "core/coreform::fmt-module",
        Value::native_fn(NativeFn::new(
            "core/coreform::fmt-module",
            1,
            nf_coreform_fmt_module,
        )),
    );
    env = Env::with_binding(
        &env,
        "core/coreform::hash-term",
        Value::native_fn(NativeFn::new(
            "core/coreform::hash-term",
            1,
            nf_coreform_hash_term,
        )),
    );
    env = Env::with_binding(
        &env,
        "core/coreform::hash-module",
        Value::native_fn(NativeFn::new(
            "core/coreform::hash-module",
            1,
            nf_coreform_hash_module,
        )),
    );
    env = Env::with_binding(
        &env,
        "core/coreform::hash-module-src",
        Value::native_fn(NativeFn::new(
            "core/coreform::hash-module-src",
            1,
            nf_coreform_hash_module_src,
        )),
    );

    // Create core/contract::genesis as a base contract that always returns UNHANDLED.
    let genesis = make_genesis();
    env = Env::with_binding(&env, "core/contract::genesis", genesis);

    // Evaluate the embedded CoreForm prelude for stable convenience wrappers and helpers.
    //
    // Toolchain bootstrap must not consume user step/memory budgets: run without limits and then
    // reset counters before returning.
    {
        let saved_step_limit = ctx.step_limit;
        let saved_mem_limits = ctx.mem_limits();
        ctx.step_limit = None;
        ctx.set_mem_limits(gc_kernel::MemLimits::default());

        let prelude_bootstrap_err = match parse_module(prelude_assembled::PRELUDE_SRC) {
            Ok(forms) => match canonicalize_module(forms) {
                Ok(forms) => gc_kernel::eval_module_compiled(ctx, &mut env, &forms)
                    .map(|_| ())
                    .err()
                    .map(|e| e.to_string()),
                Err(e) => Some(e.to_string()),
            },
            Err(e) => Some(e.to_string()),
        };

        ctx.step_limit = saved_step_limit;
        ctx.set_mem_limits(saved_mem_limits);
        ctx.reset_counters();
        if let Some(err) = prelude_bootstrap_err {
            env = Env::with_binding(
                &env,
                "core/prelude::bootstrap-error",
                mk_error_with(
                    ctx,
                    "core/prelude/bootstrap-failed",
                    format!("embedded prelude bootstrap failed: {err}"),
                    None,
                ),
            );
        }
    }

    Prelude { env, protocol }
}

fn proto(ctx: &mut EvalCtx) -> ProtocolTokens {
    match ctx.protocol {
        Some(p) => p,
        None => {
            // Mirror build_prelude's fallback; this should only happen if called without init.
            let unhandled = SealId(ctx.state.next_seal_id);
            ctx.state.next_seal_id = ctx.state.next_seal_id.saturating_add(1);
            let effect = SealId(ctx.state.next_seal_id);
            ctx.state.next_seal_id = ctx.state.next_seal_id.saturating_add(1);
            let error = SealId(ctx.state.next_seal_id);
            ctx.state.next_seal_id = ctx.state.next_seal_id.saturating_add(1);
            let p = ProtocolTokens {
                unhandled,
                effect,
                error,
            };
            ctx.protocol = Some(p);
            p
        }
    }
}

fn mk_error(ctx: &mut EvalCtx, msg: impl Into<String>) -> Value {
    mk_error_with(ctx, "core/error", msg.into(), None)
}

fn mk_error_with(ctx: &mut EvalCtx, code: &str, msg: String, at: Option<usize>) -> Value {
    let p = proto(ctx);
    let mut m = BTreeMap::new();
    m.insert(
        TermOrdKey(Term::Symbol(":error/code".to_string())),
        Term::Str(code.to_string()),
    );
    m.insert(
        TermOrdKey(Term::Symbol(":error/message".to_string())),
        Term::Str(msg),
    );
    let mut ctxm: BTreeMap<TermOrdKey, Term> = [(
        TermOrdKey(Term::Symbol(":subsystem".to_string())),
        Term::Str("prelude".to_string()),
    )]
    .into_iter()
    .collect();
    if let Some(at) = at {
        ctxm.insert(
            TermOrdKey(Term::Symbol(":at".to_string())),
            Term::Int((at as i64).into()),
        );
    }
    m.insert(
        TermOrdKey(Term::Symbol(":error/context".to_string())),
        Term::Map(ctxm),
    );
    Value::sealed(p.error, Value::data(Term::Map(m)))
}

fn mk_unhandled(ctx: &mut EvalCtx, msg_term: Term) -> Value {
    let p = proto(ctx);
    let payload = Term::list(vec![Term::Symbol("unhandled".to_string()), msg_term]);
    Value::sealed(p.unhandled, Value::data(payload))
}

fn sealed_matches(v: &Value, tok: SealId) -> Option<&Value> {
    match v {
        Value::Sealed { token, payload } if *token == tok => Some(payload.as_ref()),
        _ => None,
    }
}

fn value_to_data_term(v: &Value) -> Result<Term, KernelError> {
    v.to_plain_term()
        .ok_or_else(|| KernelError::new(KernelErrorKind::Type, "expected immutable datum"))
}

fn parse_msg_term(t: &Term) -> Result<(Term, Term), KernelError> {
    let Some(items) = t.as_proper_list() else {
        return Err(KernelError::new(
            KernelErrorKind::BadForm,
            "msg must be a list",
        ));
    };
    if items.len() != 3 {
        return Err(KernelError::new(
            KernelErrorKind::BadForm,
            "msg must have shape (msg op payload)",
        ));
    }
    if !matches!(items[0], Term::Symbol(s) if s == "msg") {
        return Err(KernelError::new(
            KernelErrorKind::BadForm,
            "msg must start with symbol 'msg'",
        ));
    }
    let op = items[1].clone();
    let payload = items[2].clone();
    Ok((op, payload))
}
