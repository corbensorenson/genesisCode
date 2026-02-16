use std::collections::BTreeMap;
use std::rc::Rc;

use blake3::Hasher;

use gc_coreform::{
    Term, TermOrdKey, canonicalize_module, hash_module, hash_term, parse_module, parse_term,
    print_module, print_term,
};
use gc_kernel::{
    Apply, Contract, EffectProgram, EffectRequest, Env, EvalCtx, KernelError, KernelErrorKind,
    NativeFn, ProtocolTokens, SealId, Value, value_hash,
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
        Value::NativeFn(NativeFn::new("core/protocol::unhandled", 1, nf_unhandled)),
    );
    env = Env::with_binding(
        &env,
        "core/protocol::is-unhandled",
        Value::NativeFn(NativeFn::new(
            "core/protocol::is-unhandled",
            1,
            nf_is_unhandled,
        )),
    );
    env = Env::with_binding(
        &env,
        "core/protocol::effect",
        Value::NativeFn(NativeFn::new("core/protocol::effect", 1, nf_effect_wrap)),
    );
    env = Env::with_binding(
        &env,
        "core/protocol::is-effect",
        Value::NativeFn(NativeFn::new("core/protocol::is-effect", 1, nf_is_effect)),
    );
    env = Env::with_binding(
        &env,
        "core/protocol::error",
        Value::NativeFn(NativeFn::new("core/protocol::error", 1, nf_error)),
    );
    env = Env::with_binding(
        &env,
        "core/protocol::is-error",
        Value::NativeFn(NativeFn::new("core/protocol::is-error", 1, nf_is_error)),
    );
    env = Env::with_binding(
        &env,
        "core/protocol::unerror",
        Value::NativeFn(NativeFn::new("core/protocol::unerror", 1, nf_unerror)),
    );

    // Contract API.
    env = Env::with_binding(
        &env,
        "core/contract::make",
        Value::NativeFn(NativeFn::new("core/contract::make", 3, nf_contract_make)),
    );
    env = Env::with_binding(
        &env,
        "core/contract::extend",
        Value::NativeFn(NativeFn::new(
            "core/contract::extend",
            3,
            nf_contract_extend,
        )),
    );
    env = Env::with_binding(
        &env,
        "core/contract::dispatch",
        Value::NativeFn(NativeFn::new(
            "core/contract::dispatch",
            2,
            nf_contract_dispatch,
        )),
    );
    env = Env::with_binding(
        &env,
        "core/contract::explain",
        Value::NativeFn(NativeFn::new(
            "core/contract::explain",
            2,
            nf_contract_explain,
        )),
    );
    env = Env::with_binding(
        &env,
        "core/contract::meta",
        Value::NativeFn(NativeFn::new("core/contract::meta", 1, nf_contract_meta)),
    );
    env = Env::with_binding(
        &env,
        "core/contract::proto",
        Value::NativeFn(NativeFn::new("core/contract::proto", 1, nf_contract_proto)),
    );
    env = Env::with_binding(
        &env,
        "core/contract::shape",
        Value::NativeFn(NativeFn::new("core/contract::shape", 1, nf_contract_shape)),
    );

    // Message helpers.
    env = Env::with_binding(
        &env,
        "core/msg::make",
        Value::NativeFn(NativeFn::new("core/msg::make", 2, nf_msg_make)),
    );
    env = Env::with_binding(
        &env,
        "core/msg::op",
        Value::NativeFn(NativeFn::new("core/msg::op", 1, nf_msg_op)),
    );
    env = Env::with_binding(
        &env,
        "core/msg::payload",
        Value::NativeFn(NativeFn::new("core/msg::payload", 1, nf_msg_payload)),
    );

    // Effects.
    env = Env::with_binding(
        &env,
        "core/effect::pure",
        Value::NativeFn(NativeFn::new("core/effect::pure", 1, nf_effect_pure)),
    );
    env = Env::with_binding(
        &env,
        "core/effect::perform",
        Value::NativeFn(NativeFn::new("core/effect::perform", 3, nf_effect_perform)),
    );
    env = Env::with_binding(
        &env,
        "core/effect::bind",
        Value::NativeFn(NativeFn::new("core/effect::bind", 2, nf_effect_bind)),
    );

    // Bootstrap CoreForm API (pure, deterministic).
    //
    // These are used for wasm-first and self-host bootstrap stages. They intentionally expose
    // parser/printer/canonicalizer operations as pure functions inside the language so GenesisCode
    // tooling can be written in GenesisCode and hosted under WASM without a second bootstrap.
    env = Env::with_binding(
        &env,
        "core/coreform::parse-term",
        Value::NativeFn(NativeFn::new(
            "core/coreform::parse-term",
            1,
            nf_coreform_parse_term,
        )),
    );
    env = Env::with_binding(
        &env,
        "core/coreform::parse-module",
        Value::NativeFn(NativeFn::new(
            "core/coreform::parse-module",
            1,
            nf_coreform_parse_module,
        )),
    );
    env = Env::with_binding(
        &env,
        "core/coreform::canonicalize-module",
        Value::NativeFn(NativeFn::new(
            "core/coreform::canonicalize-module",
            1,
            nf_coreform_canonicalize_module,
        )),
    );
    env = Env::with_binding(
        &env,
        "core/coreform::print-term",
        Value::NativeFn(NativeFn::new(
            "core/coreform::print-term",
            1,
            nf_coreform_print_term,
        )),
    );
    env = Env::with_binding(
        &env,
        "core/coreform::print-module",
        Value::NativeFn(NativeFn::new(
            "core/coreform::print-module",
            1,
            nf_coreform_print_module,
        )),
    );
    env = Env::with_binding(
        &env,
        "core/coreform::fmt-module",
        Value::NativeFn(NativeFn::new(
            "core/coreform::fmt-module",
            1,
            nf_coreform_fmt_module,
        )),
    );
    env = Env::with_binding(
        &env,
        "core/coreform::hash-term",
        Value::NativeFn(NativeFn::new(
            "core/coreform::hash-term",
            1,
            nf_coreform_hash_term,
        )),
    );
    env = Env::with_binding(
        &env,
        "core/coreform::hash-module",
        Value::NativeFn(NativeFn::new(
            "core/coreform::hash-module",
            1,
            nf_coreform_hash_module,
        )),
    );
    env = Env::with_binding(
        &env,
        "core/coreform::hash-module-src",
        Value::NativeFn(NativeFn::new(
            "core/coreform::hash-module-src",
            1,
            nf_coreform_hash_module_src,
        )),
    );

    // Create core/contract::genesis as a base contract that always returns UNHANDLED.
    let genesis = make_genesis();
    env = Env::with_binding(&env, "core/contract::genesis", genesis);

    // Evaluate the embedded CoreForm prelude for stable convenience wrappers and helpers.
    // This is considered part of the toolchain; parse/canon/eval failures are internal bugs.
    {
        const PRELUDE_SRC: &str = include_str!("../../../prelude/prelude.gc");
        let forms = parse_module(PRELUDE_SRC).expect("embedded prelude must parse");
        let forms = canonicalize_module(forms).expect("embedded prelude must canonicalize");
        let _ = gc_kernel::eval_module(ctx, &mut env, &forms).expect("embedded prelude must eval");
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
    Value::Sealed {
        token: p.error,
        payload: Box::new(Value::Data(Term::Map(m))),
    }
}

fn mk_unhandled(ctx: &mut EvalCtx, msg_term: Term) -> Value {
    let p = proto(ctx);
    let payload = Term::list(vec![Term::Symbol("unhandled".to_string()), msg_term]);
    Value::Sealed {
        token: p.unhandled,
        payload: Box::new(Value::Data(payload)),
    }
}

fn sealed_matches(v: &Value, tok: SealId) -> Option<&Value> {
    match v {
        Value::Sealed { token, payload } if *token == tok => Some(payload.as_ref()),
        _ => None,
    }
}

fn value_to_data_term(v: &Value) -> Result<Term, KernelError> {
    match v {
        Value::Data(t) => Ok(t.clone()),
        Value::Vector(xs) => Ok(Term::Vector(
            xs.iter()
                .map(value_to_data_term)
                .collect::<Result<Vec<_>, _>>()?,
        )),
        Value::Map(m) => {
            let mut out = BTreeMap::new();
            for (k, vv) in m.iter() {
                out.insert(TermOrdKey(k.0.clone()), value_to_data_term(vv)?);
            }
            Ok(Term::Map(out))
        }
        _ => Err(KernelError::new(
            KernelErrorKind::Type,
            "expected immutable datum",
        )),
    }
}

fn hex32(h: [u8; 32]) -> String {
    const LUT: &[u8; 16] = b"0123456789abcdef";
    let mut out = String::with_capacity(64);
    for b in h {
        out.push(LUT[(b >> 4) as usize] as char);
        out.push(LUT[(b & 0x0f) as usize] as char);
    }
    out
}

fn arg_utf8_src(args: &[Value], idx: usize) -> Result<String, KernelError> {
    match args.get(idx) {
        Some(Value::Data(Term::Str(s))) => Ok(s.clone()),
        Some(Value::Data(Term::Bytes(bs))) => std::str::from_utf8(bs)
            .map(str::to_owned)
            .map_err(|e| KernelError::new(KernelErrorKind::BadForm, e.to_string())),
        Some(other) => Err(KernelError::new(
            KernelErrorKind::Type,
            format!("expected utf8 string or bytes, got {}", other.debug_repr()),
        )),
        None => Err(KernelError::new(
            KernelErrorKind::BadForm,
            "missing argument".to_string(),
        )),
    }
}

fn term_vec_from_value(v: &Value) -> Result<Vec<Term>, KernelError> {
    match v {
        Value::Vector(xs) => xs
            .iter()
            .map(value_to_data_term)
            .collect::<Result<Vec<_>, _>>(),
        Value::Data(Term::Vector(xs)) => Ok(xs.clone()),
        other => Err(KernelError::new(
            KernelErrorKind::Type,
            format!("expected vector of terms, got {}", other.debug_repr()),
        )),
    }
}

fn nf_coreform_parse_term(ctx: &mut EvalCtx, args: Vec<Value>) -> Result<Value, KernelError> {
    let src = match arg_utf8_src(&args, 0) {
        Ok(s) => s,
        Err(e) => return Ok(mk_error_with(ctx, "core/type-error", e.to_string(), None)),
    };
    match parse_term(&src) {
        Ok(t) => Ok(Value::Data(t)),
        Err(e) => {
            let (code, at) = match e {
                gc_coreform::ParseError::Eof => ("core/parse/eof", 0usize),
                gc_coreform::ParseError::Unexpected { at, .. } => ("core/parse/unexpected", at),
                gc_coreform::ParseError::Escape { at, .. } => ("core/parse/escape", at),
                gc_coreform::ParseError::Int { at, .. } => ("core/parse/int", at),
            };
            Ok(mk_error_with(ctx, code, e.to_string(), Some(at)))
        }
    }
}

fn nf_coreform_parse_module(ctx: &mut EvalCtx, args: Vec<Value>) -> Result<Value, KernelError> {
    let src = match arg_utf8_src(&args, 0) {
        Ok(s) => s,
        Err(e) => return Ok(mk_error_with(ctx, "core/type-error", e.to_string(), None)),
    };
    match parse_module(&src) {
        Ok(forms) => Ok(Value::Data(Term::Vector(forms))),
        Err(e) => {
            let (code, at) = match e {
                gc_coreform::ParseError::Eof => ("core/parse/eof", 0usize),
                gc_coreform::ParseError::Unexpected { at, .. } => ("core/parse/unexpected", at),
                gc_coreform::ParseError::Escape { at, .. } => ("core/parse/escape", at),
                gc_coreform::ParseError::Int { at, .. } => ("core/parse/int", at),
            };
            Ok(mk_error_with(ctx, code, e.to_string(), Some(at)))
        }
    }
}

fn nf_coreform_canonicalize_module(
    ctx: &mut EvalCtx,
    args: Vec<Value>,
) -> Result<Value, KernelError> {
    let Some(arg0) = args.first() else {
        return Ok(mk_error_with(
            ctx,
            "core/bad-form",
            "missing argument".to_string(),
            None,
        ));
    };
    let forms = match term_vec_from_value(arg0) {
        Ok(x) => x,
        Err(e) => return Ok(mk_error_with(ctx, "core/type-error", e.to_string(), None)),
    };
    match canonicalize_module(forms) {
        Ok(canon) => Ok(Value::Data(Term::Vector(canon))),
        Err(e) => Ok(mk_error_with(ctx, "core/bad-form", e.to_string(), None)),
    }
}

fn nf_coreform_print_term(ctx: &mut EvalCtx, args: Vec<Value>) -> Result<Value, KernelError> {
    let t = match value_to_data_term(
        args.first()
            .ok_or_else(|| KernelError::new(KernelErrorKind::BadForm, "missing argument"))?,
    ) {
        Ok(t) => t,
        Err(e) => return Ok(mk_error_with(ctx, "core/type-error", e.to_string(), None)),
    };
    Ok(Value::Data(Term::Str(print_term(&t))))
}

fn nf_coreform_print_module(ctx: &mut EvalCtx, args: Vec<Value>) -> Result<Value, KernelError> {
    let forms = match term_vec_from_value(
        args.first()
            .ok_or_else(|| KernelError::new(KernelErrorKind::BadForm, "missing argument"))?,
    ) {
        Ok(x) => x,
        Err(e) => return Ok(mk_error_with(ctx, "core/type-error", e.to_string(), None)),
    };
    Ok(Value::Data(Term::Str(print_module(&forms))))
}

fn nf_coreform_fmt_module(ctx: &mut EvalCtx, args: Vec<Value>) -> Result<Value, KernelError> {
    let src = match arg_utf8_src(&args, 0) {
        Ok(s) => s,
        Err(e) => return Ok(mk_error_with(ctx, "core/type-error", e.to_string(), None)),
    };
    let forms = match parse_module(&src) {
        Ok(f) => f,
        Err(e) => {
            let (code, at) = match e {
                gc_coreform::ParseError::Eof => ("core/parse/eof", 0usize),
                gc_coreform::ParseError::Unexpected { at, .. } => ("core/parse/unexpected", at),
                gc_coreform::ParseError::Escape { at, .. } => ("core/parse/escape", at),
                gc_coreform::ParseError::Int { at, .. } => ("core/parse/int", at),
            };
            return Ok(mk_error_with(ctx, code, e.to_string(), Some(at)));
        }
    };
    let canon = match canonicalize_module(forms) {
        Ok(c) => c,
        Err(e) => return Ok(mk_error_with(ctx, "core/bad-form", e.to_string(), None)),
    };
    Ok(Value::Data(Term::Str(print_module(&canon))))
}

fn nf_coreform_hash_term(ctx: &mut EvalCtx, args: Vec<Value>) -> Result<Value, KernelError> {
    let Some(arg0) = args.first() else {
        return Ok(mk_error_with(
            ctx,
            "core/bad-form",
            "missing argument".to_string(),
            None,
        ));
    };
    let t = match value_to_data_term(arg0) {
        Ok(x) => x,
        Err(e) => return Ok(mk_error_with(ctx, "core/type-error", e.to_string(), None)),
    };
    Ok(Value::Data(Term::Str(hex32(hash_term(&t)))))
}

fn nf_coreform_hash_module(ctx: &mut EvalCtx, args: Vec<Value>) -> Result<Value, KernelError> {
    let Some(arg0) = args.first() else {
        return Ok(mk_error_with(
            ctx,
            "core/bad-form",
            "missing argument".to_string(),
            None,
        ));
    };
    let forms = match term_vec_from_value(arg0) {
        Ok(x) => x,
        Err(e) => return Ok(mk_error_with(ctx, "core/type-error", e.to_string(), None)),
    };
    Ok(Value::Data(Term::Str(hex32(hash_module(&forms)))))
}

fn nf_coreform_hash_module_src(ctx: &mut EvalCtx, args: Vec<Value>) -> Result<Value, KernelError> {
    let src = match arg_utf8_src(&args, 0) {
        Ok(s) => s,
        Err(e) => return Ok(mk_error_with(ctx, "core/type-error", e.to_string(), None)),
    };
    let forms = match parse_module(&src) {
        Ok(f) => f,
        Err(e) => {
            let (code, at) = match e {
                gc_coreform::ParseError::Eof => ("core/parse/eof", 0usize),
                gc_coreform::ParseError::Unexpected { at, .. } => ("core/parse/unexpected", at),
                gc_coreform::ParseError::Escape { at, .. } => ("core/parse/escape", at),
                gc_coreform::ParseError::Int { at, .. } => ("core/parse/int", at),
            };
            return Ok(mk_error_with(ctx, code, e.to_string(), Some(at)));
        }
    };
    let canon = match canonicalize_module(forms) {
        Ok(c) => c,
        Err(e) => return Ok(mk_error_with(ctx, "core/bad-form", e.to_string(), None)),
    };
    Ok(Value::Data(Term::Str(hex32(hash_module(&canon)))))
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

fn make_genesis() -> Value {
    let empty_overrides = Value::Map(BTreeMap::new());
    // Avoid any fallible/alloc-heavy path during runtime init: construct the
    // partially-applied native function directly.
    let handler = Value::NativeFn(NativeFn {
        name: "core/contract::internal-override-handler",
        arity: 2,
        collected: vec![empty_overrides.clone()],
        func: nf_internal_override_handler,
    });

    let meta = Value::Map(
        [(
            TermOrdKey(Term::Symbol(":intent".to_string())),
            Value::Data(Term::Str("Root contract (proto base)".to_string())),
        )]
        .into_iter()
        .collect(),
    );

    let shape_id = shape_id(None, &BTreeMap::new());
    let contract_id = contract_id(&shape_id, &handler, &meta, None);
    Value::Contract(Rc::new(Contract {
        handler,
        proto: None,
        meta,
        overrides: BTreeMap::new(),
        shape_id,
        contract_id,
    }))
}

fn shape_id(proto: Option<&Contract>, overrides: &BTreeMap<String, Value>) -> [u8; 32] {
    let mut h = Hasher::new();
    h.update(b"GCv0.2\0shape\0");
    if let Some(p) = proto {
        h.update(&p.shape_id);
    } else {
        h.update(b"nil\0");
    }
    for k in overrides.keys() {
        h.update(k.as_bytes());
        h.update(b"\0");
    }
    *h.finalize().as_bytes()
}

fn contract_id(
    shape_id: &[u8; 32],
    handler: &Value,
    meta: &Value,
    proto: Option<&Contract>,
) -> [u8; 32] {
    let mut h = Hasher::new();
    h.update(b"GCv0.2\0contract\0");
    h.update(shape_id);
    if let Some(p) = proto {
        h.update(&p.contract_id);
    } else {
        h.update(b"nil\0");
    }
    h.update(&value_hash(handler));
    h.update(&value_hash(meta));
    *h.finalize().as_bytes()
}

fn hex(bytes: &[u8]) -> String {
    const HEX: &[u8; 16] = b"0123456789abcdef";
    let mut out = String::new();
    for &b in bytes {
        out.push(HEX[(b >> 4) as usize] as char);
        out.push(HEX[(b & 0x0f) as usize] as char);
    }
    out
}

// Native function implementations

fn nf_unhandled(ctx: &mut EvalCtx, args: Vec<Value>) -> Result<Value, KernelError> {
    let msg = value_to_data_term(&args[0])?;
    Ok(mk_unhandled(ctx, msg))
}

fn nf_is_unhandled(ctx: &mut EvalCtx, args: Vec<Value>) -> Result<Value, KernelError> {
    let p = proto(ctx);
    Ok(Value::Data(Term::Bool(
        sealed_matches(&args[0], p.unhandled).is_some(),
    )))
}

fn nf_effect_wrap(ctx: &mut EvalCtx, args: Vec<Value>) -> Result<Value, KernelError> {
    let p = proto(ctx);
    Ok(Value::Sealed {
        token: p.effect,
        payload: Box::new(args[0].clone()),
    })
}

fn nf_is_effect(ctx: &mut EvalCtx, args: Vec<Value>) -> Result<Value, KernelError> {
    let p = proto(ctx);
    Ok(Value::Data(Term::Bool(
        sealed_matches(&args[0], p.effect).is_some(),
    )))
}

fn nf_error(ctx: &mut EvalCtx, args: Vec<Value>) -> Result<Value, KernelError> {
    let p = proto(ctx);
    Ok(Value::Sealed {
        token: p.error,
        payload: Box::new(args[0].clone()),
    })
}

fn nf_is_error(ctx: &mut EvalCtx, args: Vec<Value>) -> Result<Value, KernelError> {
    let p = proto(ctx);
    Ok(Value::Data(Term::Bool(
        sealed_matches(&args[0], p.error).is_some(),
    )))
}

fn nf_unerror(ctx: &mut EvalCtx, args: Vec<Value>) -> Result<Value, KernelError> {
    let p = proto(ctx);
    if let Some(payload) = sealed_matches(&args[0], p.error) {
        Ok(payload.clone())
    } else {
        Ok(Value::Data(Term::Nil))
    }
}

fn nf_contract_make(ctx: &mut EvalCtx, args: Vec<Value>) -> Result<Value, KernelError> {
    let handler = args[0].clone();
    if !matches!(handler, Value::Closure { .. } | Value::NativeFn(_)) {
        return Ok(mk_error(ctx, "contract handler must be callable"));
    }
    let proto = match &args[1] {
        Value::Contract(c) => Some(c.clone()),
        Value::Data(Term::Nil) => None,
        _ => return Ok(mk_error(ctx, "proto must be a contract or nil")),
    };
    let meta = args[2].clone();

    let overrides = BTreeMap::new();
    let shape_id = shape_id(proto.as_deref(), &overrides);
    let contract_id = contract_id(&shape_id, &handler, &meta, proto.as_deref());

    Ok(Value::Contract(Rc::new(Contract {
        handler,
        proto,
        meta,
        overrides,
        shape_id,
        contract_id,
    })))
}

fn nf_contract_extend(ctx: &mut EvalCtx, args: Vec<Value>) -> Result<Value, KernelError> {
    let base = match &args[0] {
        Value::Contract(c) => c.clone(),
        _ => return Ok(mk_error(ctx, "extend base must be a contract")),
    };
    let overrides_val = args[1].clone();
    let overrides_map = match &overrides_val {
        Value::Map(m) => m,
        _ => return Ok(mk_error(ctx, "extend overrides must be a map literal")),
    };

    // Build a stable op->handler map for introspection/shape.
    let mut overrides: BTreeMap<String, Value> = BTreeMap::new();
    for (k, v) in overrides_map.iter() {
        let Term::Symbol(op) = &k.0 else {
            return Ok(mk_error(ctx, "override map keys must be symbols"));
        };
        if !matches!(v, Value::Closure { .. } | Value::NativeFn(_)) {
            return Ok(mk_error(ctx, format!("override for {op} must be callable")));
        }
        overrides.insert(op.clone(), v.clone());
    }

    let meta_plus = args[2].clone();
    let meta = merge_meta(&base.meta, &meta_plus);

    // Handler: internal override handler partially applied to the overrides map.
    let handler = {
        let f = Value::NativeFn(NativeFn::new(
            "core/contract::internal-override-handler",
            2,
            nf_internal_override_handler,
        ));
        f.apply(ctx, overrides_val)?
    };

    let shape_id = shape_id(Some(base.as_ref()), &overrides);
    let contract_id = contract_id(&shape_id, &handler, &meta, Some(base.as_ref()));
    Ok(Value::Contract(Rc::new(Contract {
        handler,
        proto: Some(base),
        meta,
        overrides,
        shape_id,
        contract_id,
    })))
}

fn merge_meta(base: &Value, plus: &Value) -> Value {
    match (base, plus) {
        (Value::Map(a), Value::Map(b)) => {
            let mut out = a.clone();
            for (k, v) in b.iter() {
                out.insert(k.clone(), v.clone());
            }
            Value::Map(out)
        }
        _ => plus.clone(),
    }
}

fn nf_internal_override_handler(ctx: &mut EvalCtx, args: Vec<Value>) -> Result<Value, KernelError> {
    let p = proto(ctx);
    let overrides = match &args[0] {
        Value::Map(m) => m,
        _ => return Ok(mk_error(ctx, "internal override handler expects map")),
    };
    let msg_term = match &args[1] {
        Value::Data(t) => t.clone(),
        _ => return Ok(mk_error(ctx, "msg must be a datum")),
    };
    let (op, _payload) = match parse_msg_term(&msg_term) {
        Ok(x) => x,
        Err(e) => {
            return Ok(Value::Sealed {
                token: p.error,
                payload: Box::new(Value::Data(Term::Str(e.msg))),
            });
        }
    };
    let Term::Symbol(op_s) = op else {
        return Ok(mk_error(ctx, "msg op must be a symbol"));
    };
    if let Some(h) = overrides.get(&TermOrdKey(Term::Symbol(op_s.clone()))) {
        return h.clone().apply(ctx, Value::Data(msg_term));
    }
    Ok(mk_unhandled(ctx, msg_term))
}

fn nf_contract_dispatch(ctx: &mut EvalCtx, args: Vec<Value>) -> Result<Value, KernelError> {
    let p = proto(ctx);
    let mut cur = match &args[0] {
        Value::Contract(c) => c.clone(),
        _ => return Ok(mk_error(ctx, "dispatch expects a contract")),
    };
    let msg = args[1].clone();
    let msg_term = value_to_data_term(&msg).map_err(|e| KernelError::new(e.kind, e.msg))?;

    loop {
        let r = cur
            .handler
            .clone()
            .apply(ctx, Value::Data(msg_term.clone()))?;
        if sealed_matches(&r, p.unhandled).is_some()
            && let Some(proto) = &cur.proto
        {
            cur = proto.clone();
            continue;
        }
        return Ok(r);
    }
}

fn nf_contract_explain(ctx: &mut EvalCtx, args: Vec<Value>) -> Result<Value, KernelError> {
    let p = proto(ctx);
    let mut cur = match &args[0] {
        Value::Contract(c) => c.clone(),
        _ => return Ok(mk_error(ctx, "explain expects a contract")),
    };
    let msg_term = match value_to_data_term(&args[1]) {
        Ok(t) => t,
        Err(_) => return Ok(mk_error(ctx, "msg must be a datum")),
    };

    let (op_term, _payload) = match parse_msg_term(&msg_term) {
        Ok(x) => x,
        Err(e) => return Ok(mk_error(ctx, e.msg)),
    };
    let op_sym = match op_term {
        Term::Symbol(s) => s,
        _ => "<non-symbol>".to_string(),
    };

    let mut steps: Vec<Term> = Vec::new();
    let result: Value = loop {
        let override_matched = cur.overrides.contains_key(&op_sym);
        let r = cur
            .handler
            .clone()
            .apply(ctx, Value::Data(msg_term.clone()))?;
        let unhandled = sealed_matches(&r, p.unhandled).is_some();

        steps.push(Term::Map(
            [
                (
                    TermOrdKey(Term::Symbol(":contract-id".to_string())),
                    Term::Str(hex(&cur.contract_id)),
                ),
                (
                    TermOrdKey(Term::Symbol(":shape-id".to_string())),
                    Term::Str(hex(&cur.shape_id)),
                ),
                (
                    TermOrdKey(Term::Symbol(":override".to_string())),
                    Term::Bool(override_matched),
                ),
                (
                    TermOrdKey(Term::Symbol(":unhandled".to_string())),
                    Term::Bool(unhandled),
                ),
                (
                    TermOrdKey(Term::Symbol(":has-proto".to_string())),
                    Term::Bool(cur.proto.is_some()),
                ),
            ]
            .into_iter()
            .collect(),
        ));

        if unhandled && let Some(proto) = &cur.proto {
            cur = proto.clone();
            continue;
        }
        break r;
    };

    let trace = Term::Map(
        [
            (
                TermOrdKey(Term::Symbol(":op".to_string())),
                Term::Symbol(op_sym),
            ),
            (
                TermOrdKey(Term::Symbol(":steps".to_string())),
                Term::Vector(steps),
            ),
            (
                TermOrdKey(Term::Symbol(":result".to_string())),
                result.to_term_for_log(Some(p.error)),
            ),
        ]
        .into_iter()
        .collect(),
    );
    Ok(Value::Data(trace))
}

fn nf_contract_meta(ctx: &mut EvalCtx, args: Vec<Value>) -> Result<Value, KernelError> {
    match &args[0] {
        Value::Contract(c) => Ok(c.meta.clone()),
        _ => Ok(mk_error(ctx, "meta expects a contract")),
    }
}

fn nf_contract_proto(ctx: &mut EvalCtx, args: Vec<Value>) -> Result<Value, KernelError> {
    match &args[0] {
        Value::Contract(c) => match &c.proto {
            Some(p) => Ok(Value::Contract(p.clone())),
            None => Ok(Value::Data(Term::Nil)),
        },
        _ => Ok(mk_error(ctx, "proto expects a contract")),
    }
}

fn nf_contract_shape(ctx: &mut EvalCtx, args: Vec<Value>) -> Result<Value, KernelError> {
    match &args[0] {
        Value::Contract(c) => Ok(Value::Data(Term::Str(hex(&c.shape_id)))),
        _ => Ok(mk_error(ctx, "shape expects a contract")),
    }
}

fn nf_msg_make(ctx: &mut EvalCtx, args: Vec<Value>) -> Result<Value, KernelError> {
    let op = match value_to_data_term(&args[0])? {
        Term::Symbol(s) => Term::Symbol(s),
        _ => return Ok(mk_error(ctx, "msg op must be a symbol datum")),
    };
    let payload = value_to_data_term(&args[1])?;
    Ok(Value::Data(Term::list(vec![
        Term::Symbol("msg".to_string()),
        op,
        payload,
    ])))
}

fn nf_msg_op(ctx: &mut EvalCtx, args: Vec<Value>) -> Result<Value, KernelError> {
    let msg = match &args[0] {
        Value::Data(t) => t.clone(),
        _ => return Ok(mk_error(ctx, "msg must be a datum")),
    };
    let (op, _) = match parse_msg_term(&msg) {
        Ok(x) => x,
        Err(e) => return Ok(mk_error(ctx, e.msg)),
    };
    Ok(Value::Data(op))
}

fn nf_msg_payload(ctx: &mut EvalCtx, args: Vec<Value>) -> Result<Value, KernelError> {
    let msg = match &args[0] {
        Value::Data(t) => t.clone(),
        _ => return Ok(mk_error(ctx, "msg must be a datum")),
    };
    let (_, payload) = match parse_msg_term(&msg) {
        Ok(x) => x,
        Err(e) => return Ok(mk_error(ctx, e.msg)),
    };
    Ok(Value::Data(payload))
}

fn nf_effect_pure(_ctx: &mut EvalCtx, args: Vec<Value>) -> Result<Value, KernelError> {
    Ok(Value::EffectProgram(Box::new(EffectProgram::Pure(
        Box::new(args[0].clone()),
    ))))
}

fn nf_effect_perform(ctx: &mut EvalCtx, args: Vec<Value>) -> Result<Value, KernelError> {
    let p = proto(ctx);
    let op = match value_to_data_term(&args[0])? {
        Term::Symbol(s) => s,
        _ => return Ok(mk_error(ctx, "effect op must be a symbol datum")),
    };
    let payload = value_to_data_term(&args[1])?;
    let k = args[2].clone();
    if !matches!(k, Value::Closure { .. } | Value::NativeFn(_)) {
        return Ok(mk_error(ctx, "effect continuation must be callable"));
    }
    let req = Value::EffectRequest(EffectRequest {
        op,
        payload,
        k: Box::new(k),
    });
    let sealed = Value::Sealed {
        token: p.effect,
        payload: Box::new(req),
    };
    Ok(Value::EffectProgram(Box::new(EffectProgram::Perform {
        request: Box::new(sealed),
    })))
}

fn lift_to_effect_program(v: Value) -> Value {
    match v {
        Value::EffectProgram(_) => v,
        other => Value::EffectProgram(Box::new(EffectProgram::Pure(Box::new(other)))),
    }
}

fn bind_impl(ctx: &mut EvalCtx, program: Value, f: Value) -> Result<Value, KernelError> {
    let Value::EffectProgram(p) = program else {
        return Ok(mk_error(ctx, "bind expects an effect program"));
    };
    match p.as_ref() {
        EffectProgram::Pure(v) => {
            let next = f.apply(ctx, (*v.as_ref()).clone())?;
            Ok(lift_to_effect_program(next))
        }
        EffectProgram::Perform { request } => {
            let tok = proto(ctx).effect;
            let payload = match sealed_matches(request.as_ref(), tok) {
                Some(x) => x,
                None => return Ok(mk_error(ctx, "bind expects an EFFECT-sealed request")),
            };
            let Value::EffectRequest(req) = payload else {
                return Ok(mk_error(ctx, "bind expects a well-formed effect request"));
            };

            let k = (*req.k).clone();
            let k2 = Value::NativeFn(NativeFn {
                name: "core/effect::internal-bind-cont",
                arity: 3,
                collected: vec![k, f],
                func: nf_effect_bind_cont,
            });

            let req2 = Value::EffectRequest(EffectRequest {
                op: req.op.clone(),
                payload: req.payload.clone(),
                k: Box::new(k2),
            });
            let sealed2 = Value::Sealed {
                token: tok,
                payload: Box::new(req2),
            };
            Ok(Value::EffectProgram(Box::new(EffectProgram::Perform {
                request: Box::new(sealed2),
            })))
        }
    }
}

fn nf_effect_bind(ctx: &mut EvalCtx, args: Vec<Value>) -> Result<Value, KernelError> {
    let f = args[1].clone();
    if !matches!(f, Value::Closure { .. } | Value::NativeFn(_)) {
        return Ok(mk_error(ctx, "bind function must be callable"));
    }
    bind_impl(ctx, args[0].clone(), f)
}

fn nf_effect_bind_cont(ctx: &mut EvalCtx, args: Vec<Value>) -> Result<Value, KernelError> {
    let k = args[0].clone();
    let f = args[1].clone();
    let resp = args[2].clone();
    let next = k.apply(ctx, resp)?;
    bind_impl(ctx, lift_to_effect_program(next), f)
}
