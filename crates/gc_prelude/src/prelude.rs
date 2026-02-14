use std::collections::BTreeMap;
use std::rc::Rc;

use blake3::Hasher;

use gc_coreform::{Term, TermOrdKey};
use gc_kernel::{
    value_hash, Apply, Contract, EffectProgram, EffectRequest, Env, EvalCtx, KernelError,
    KernelErrorKind, NativeFn, ProtocolTokens, SealId, Value,
};

pub struct Prelude {
    pub env: Env,
    pub protocol: ProtocolTokens,
}

pub fn build_prelude(ctx: &mut EvalCtx) -> Prelude {
    // Allocate protocol seal IDs from the kernel's deterministic counter.
    let unhandled = SealId(ctx.state.next_seal_id);
    ctx.state.next_seal_id += 1;
    let effect = SealId(ctx.state.next_seal_id);
    ctx.state.next_seal_id += 1;
    let error = SealId(ctx.state.next_seal_id);
    ctx.state.next_seal_id += 1;
    let protocol = ProtocolTokens {
        unhandled,
        effect,
        error,
    };
    ctx.protocol = Some(protocol);

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
        Value::NativeFn(NativeFn::new("core/contract::extend", 3, nf_contract_extend)),
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
        Value::NativeFn(NativeFn::new(
            "core/msg::payload",
            1,
            nf_msg_payload,
        )),
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
        Value::NativeFn(NativeFn::new(
            "core/effect::perform",
            3,
            nf_effect_perform,
        )),
    );

    // Create core/contract::genesis as a base contract that always returns UNHANDLED.
    let genesis = make_genesis(ctx);
    env = Env::with_binding(&env, "core/contract::genesis", genesis);

    Prelude { env, protocol }
}

fn proto(ctx: &EvalCtx) -> ProtocolTokens {
    ctx.protocol.expect("prelude must set protocol tokens")
}

fn mk_error(ctx: &EvalCtx, msg: impl Into<String>) -> Value {
    let p = proto(ctx);
    let mut m = BTreeMap::new();
    m.insert(
        TermOrdKey(Term::Symbol(":error/code".to_string())),
        Term::Str("core/error".to_string()),
    );
    m.insert(
        TermOrdKey(Term::Symbol(":error/message".to_string())),
        Term::Str(msg.into()),
    );
    Value::Sealed {
        token: p.error,
        payload: Box::new(Value::Data(Term::Map(m))),
    }
}

fn mk_unhandled(ctx: &EvalCtx, msg_term: Term) -> Value {
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
                .map(|x| value_to_data_term(x))
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

fn parse_msg_term(t: &Term) -> Result<(Term, Term), KernelError> {
    let Some(items) = t.as_proper_list() else {
        return Err(KernelError::new(KernelErrorKind::BadForm, "msg must be a list"));
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

fn make_genesis(ctx: &mut EvalCtx) -> Value {
    let empty_overrides = Value::Map(BTreeMap::new());
    let handler = {
        let f = Value::NativeFn(NativeFn::new(
            "core/contract::internal-override-handler",
            2,
            nf_internal_override_handler,
        ));
        f.apply(ctx, empty_overrides).expect("partial apply")
    };

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
        Err(e) => return Ok(Value::Sealed {
            token: p.error,
            payload: Box::new(Value::Data(Term::Str(e.msg))),
        }),
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
        let r = cur.handler.clone().apply(ctx, Value::Data(msg_term.clone()))?;
        if sealed_matches(&r, p.unhandled).is_some() {
            if let Some(proto) = &cur.proto {
                cur = proto.clone();
                continue;
            }
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
        let r = cur.handler.clone().apply(ctx, Value::Data(msg_term.clone()))?;
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

        if unhandled {
            if let Some(proto) = &cur.proto {
                cur = proto.clone();
                continue;
            }
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
    Ok(Value::EffectProgram(Box::new(EffectProgram::Pure(Box::new(
        args[0].clone(),
    )))))
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
