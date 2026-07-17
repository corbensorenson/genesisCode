use super::*;
use gc_coreform::HASH_DOMAIN_PREFIX;

pub(super) fn make_genesis() -> Value {
    let empty_overrides = Value::map(ValueMap::new());
    // Avoid any fallible/alloc-heavy path during runtime init: construct the
    // partially-applied native function directly.
    let handler = Value::native_fn(NativeFn {
        name: "core/contract::internal-override-handler",
        arity: 2,
        collected: vec![empty_overrides.clone()],
        func: nf_internal_override_handler,
    });

    let meta = Value::map(
        [(
            TermOrdKey(Term::Symbol(":intent".to_string())),
            Value::data(Term::Str("Root contract (proto base)".to_string())),
        )]
        .into_iter()
        .collect(),
    );

    let shape_id = shape_id(None, &BTreeMap::new());
    let contract_id = contract_id(&shape_id, &handler, &meta, None);
    Value::contract(Contract {
        handler,
        proto: None,
        meta,
        overrides: BTreeMap::new(),
        shape_id,
        contract_id,
    })
}

fn shape_id(proto: Option<&Contract>, overrides: &BTreeMap<String, Value>) -> [u8; 32] {
    let mut h = Hasher::new();
    h.update(HASH_DOMAIN_PREFIX);
    h.update(b"shape\0");
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
    h.update(HASH_DOMAIN_PREFIX);
    h.update(b"contract\0");
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

pub(super) fn nf_unhandled(ctx: &mut EvalCtx, args: Vec<Value>) -> Result<Value, KernelError> {
    let msg = value_to_data_term(&args[0])?;
    Ok(mk_unhandled(ctx, msg))
}

pub(super) fn nf_is_unhandled(ctx: &mut EvalCtx, args: Vec<Value>) -> Result<Value, KernelError> {
    let p = proto(ctx);
    Ok(Value::data(Term::Bool(
        sealed_matches(&args[0], p.unhandled).is_some(),
    )))
}

pub(super) fn nf_effect_wrap(ctx: &mut EvalCtx, args: Vec<Value>) -> Result<Value, KernelError> {
    let p = proto(ctx);
    Ok(Value::sealed(p.effect, args[0].clone()))
}

pub(super) fn nf_is_effect(ctx: &mut EvalCtx, args: Vec<Value>) -> Result<Value, KernelError> {
    let p = proto(ctx);
    Ok(Value::data(Term::Bool(
        sealed_matches(&args[0], p.effect).is_some(),
    )))
}

pub(super) fn nf_error(ctx: &mut EvalCtx, args: Vec<Value>) -> Result<Value, KernelError> {
    let p = proto(ctx);
    Ok(Value::sealed(p.error, args[0].clone()))
}

pub(super) fn nf_is_error(ctx: &mut EvalCtx, args: Vec<Value>) -> Result<Value, KernelError> {
    let p = proto(ctx);
    Ok(Value::data(Term::Bool(
        sealed_matches(&args[0], p.error).is_some(),
    )))
}

pub(super) fn nf_unerror(ctx: &mut EvalCtx, args: Vec<Value>) -> Result<Value, KernelError> {
    let p = proto(ctx);
    if let Some(payload) = sealed_matches(&args[0], p.error) {
        Ok(payload.clone())
    } else {
        Ok(Value::data(Term::Nil))
    }
}

pub(super) fn nf_contract_make(ctx: &mut EvalCtx, args: Vec<Value>) -> Result<Value, KernelError> {
    let handler = args[0].clone();
    if !matches!(
        handler,
        Value::Closure(_) | Value::CompiledClosure(_) | Value::NativeFn(_)
    ) {
        return Ok(mk_error(ctx, "contract handler must be callable"));
    }
    let proto = match &args[1] {
        Value::Contract(c) => Some(c.clone()),
        Value::Data(t) if matches!(t.as_ref(), Term::Nil) => None,
        _ => return Ok(mk_error(ctx, "proto must be a contract or nil")),
    };
    let meta = args[2].clone();

    let overrides = BTreeMap::new();
    let shape_id = shape_id(proto.as_deref(), &overrides);
    let contract_id = contract_id(&shape_id, &handler, &meta, proto.as_deref());

    Ok(Value::contract(Contract {
        handler,
        proto,
        meta,
        overrides,
        shape_id,
        contract_id,
    }))
}

pub(super) fn nf_contract_extend(
    ctx: &mut EvalCtx,
    args: Vec<Value>,
) -> Result<Value, KernelError> {
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
        if !matches!(
            v,
            Value::Closure(_) | Value::CompiledClosure(_) | Value::NativeFn(_)
        ) {
            return Ok(mk_error(ctx, format!("override for {op} must be callable")));
        }
        overrides.insert(op.clone(), v.clone());
    }

    let meta_plus = args[2].clone();
    let meta = merge_meta(&base.meta, &meta_plus);

    // Handler: internal override handler partially applied to the overrides map.
    let handler = {
        let f = Value::native_fn(NativeFn::new(
            "core/contract::internal-override-handler",
            2,
            nf_internal_override_handler,
        ));
        f.apply(ctx, overrides_val)?
    };

    let shape_id = shape_id(Some(base.as_ref()), &overrides);
    let contract_id = contract_id(&shape_id, &handler, &meta, Some(base.as_ref()));
    Ok(Value::contract(Contract {
        handler,
        proto: Some(base),
        meta,
        overrides,
        shape_id,
        contract_id,
    }))
}

fn merge_meta(base: &Value, plus: &Value) -> Value {
    match (base, plus) {
        (Value::Map(a), Value::Map(b)) => {
            let mut out = a.clone();
            for (k, v) in b.iter() {
                Shared::make_mut(&mut out).insert_mut(k.clone(), v.clone());
            }
            Value::map_shared(out)
        }
        _ => plus.clone(),
    }
}

pub(super) fn nf_internal_override_handler(
    ctx: &mut EvalCtx,
    args: Vec<Value>,
) -> Result<Value, KernelError> {
    let p = proto(ctx);
    let overrides = match &args[0] {
        Value::Map(m) => m,
        _ => return Ok(mk_error(ctx, "internal override handler expects map")),
    };
    let msg_term = match &args[1] {
        Value::Data(t) => t.as_ref().clone(),
        _ => return Ok(mk_error(ctx, "msg must be a datum")),
    };
    let (op, _payload) = match parse_msg_term(&msg_term) {
        Ok(x) => x,
        Err(e) => {
            return Ok(Value::sealed(p.error, Value::data(Term::Str(e.msg))));
        }
    };
    let Term::Symbol(op_s) = op else {
        return Ok(mk_error(ctx, "msg op must be a symbol"));
    };
    if let Some(h) = overrides.get(&TermOrdKey(Term::Symbol(op_s.clone()))) {
        return h.clone().apply(ctx, Value::data(msg_term));
    }
    Ok(mk_unhandled(ctx, msg_term))
}

pub(super) fn nf_contract_dispatch(
    ctx: &mut EvalCtx,
    args: Vec<Value>,
) -> Result<Value, KernelError> {
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
            .apply(ctx, Value::data(msg_term.clone()))?;
        if sealed_matches(&r, p.unhandled).is_some()
            && let Some(proto) = &cur.proto
        {
            cur = proto.clone();
            continue;
        }
        return Ok(r);
    }
}

pub(super) fn nf_contract_explain(
    ctx: &mut EvalCtx,
    args: Vec<Value>,
) -> Result<Value, KernelError> {
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
            .apply(ctx, Value::data(msg_term.clone()))?;
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
    Ok(Value::data(trace))
}

pub(super) fn nf_contract_meta(ctx: &mut EvalCtx, args: Vec<Value>) -> Result<Value, KernelError> {
    match &args[0] {
        Value::Contract(c) => Ok(c.meta.clone()),
        _ => Ok(mk_error(ctx, "meta expects a contract")),
    }
}

pub(super) fn nf_contract_proto(ctx: &mut EvalCtx, args: Vec<Value>) -> Result<Value, KernelError> {
    match &args[0] {
        Value::Contract(c) => match &c.proto {
            Some(p) => Ok(Value::Contract(p.clone())),
            None => Ok(Value::data(Term::Nil)),
        },
        _ => Ok(mk_error(ctx, "proto expects a contract")),
    }
}

pub(super) fn nf_contract_shape(ctx: &mut EvalCtx, args: Vec<Value>) -> Result<Value, KernelError> {
    match &args[0] {
        Value::Contract(c) => Ok(Value::data(Term::Str(hex(&c.shape_id)))),
        _ => Ok(mk_error(ctx, "shape expects a contract")),
    }
}

pub(super) fn nf_msg_make(ctx: &mut EvalCtx, args: Vec<Value>) -> Result<Value, KernelError> {
    let op = match value_to_data_term(&args[0])? {
        Term::Symbol(s) => Term::Symbol(s),
        _ => return Ok(mk_error(ctx, "msg op must be a symbol datum")),
    };
    let payload = value_to_data_term(&args[1])?;
    Ok(Value::data(Term::list(vec![
        Term::Symbol("msg".to_string()),
        op,
        payload,
    ])))
}

pub(super) fn nf_msg_op(ctx: &mut EvalCtx, args: Vec<Value>) -> Result<Value, KernelError> {
    let msg = match &args[0] {
        Value::Data(t) => t.as_ref().clone(),
        _ => return Ok(mk_error(ctx, "msg must be a datum")),
    };
    let (op, _) = match parse_msg_term(&msg) {
        Ok(x) => x,
        Err(e) => return Ok(mk_error(ctx, e.msg)),
    };
    Ok(Value::data(op))
}

pub(super) fn nf_msg_payload(ctx: &mut EvalCtx, args: Vec<Value>) -> Result<Value, KernelError> {
    let msg = match &args[0] {
        Value::Data(t) => t.as_ref().clone(),
        _ => return Ok(mk_error(ctx, "msg must be a datum")),
    };
    let (_, payload) = match parse_msg_term(&msg) {
        Ok(x) => x,
        Err(e) => return Ok(mk_error(ctx, e.msg)),
    };
    Ok(Value::data(payload))
}

pub(super) fn nf_effect_pure(_ctx: &mut EvalCtx, args: Vec<Value>) -> Result<Value, KernelError> {
    Ok(Value::pure_effect(args[0].clone()))
}

pub(super) fn nf_effect_perform(ctx: &mut EvalCtx, args: Vec<Value>) -> Result<Value, KernelError> {
    let p = proto(ctx);
    let op = match value_to_data_term(&args[0])? {
        Term::Symbol(s) => s,
        _ => return Ok(mk_error(ctx, "effect op must be a symbol datum")),
    };
    let payload = value_to_data_term(&args[1])?;
    let k = args[2].clone();
    if !matches!(
        k,
        Value::Closure(_) | Value::CompiledClosure(_) | Value::NativeFn(_)
    ) {
        return Ok(mk_error(ctx, "effect continuation must be callable"));
    }
    let req = Value::effect_request(EffectRequest {
        op,
        payload,
        k: Box::new(k),
    });
    let sealed = Value::sealed(p.effect, req);
    Ok(Value::perform_effect(sealed))
}

fn lift_to_effect_program(v: Value) -> Value {
    match v {
        Value::EffectProgram(_) => v,
        other => Value::pure_effect(other),
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
            let k2 = Value::native_fn(NativeFn {
                name: "core/effect::internal-bind-cont",
                arity: 3,
                collected: vec![k, f],
                func: nf_effect_bind_cont,
            });

            let req2 = Value::effect_request(EffectRequest {
                op: req.op.clone(),
                payload: req.payload.clone(),
                k: Box::new(k2),
            });
            let sealed2 = Value::sealed(tok, req2);
            Ok(Value::perform_effect(sealed2))
        }
    }
}

pub(super) fn nf_effect_bind(ctx: &mut EvalCtx, args: Vec<Value>) -> Result<Value, KernelError> {
    let f = args[1].clone();
    if !matches!(
        f,
        Value::Closure(_) | Value::CompiledClosure(_) | Value::NativeFn(_)
    ) {
        return Ok(mk_error(ctx, "bind function must be callable"));
    }
    bind_impl(ctx, args[0].clone(), f)
}

pub(super) fn nf_effect_bind_cont(
    ctx: &mut EvalCtx,
    args: Vec<Value>,
) -> Result<Value, KernelError> {
    let k = args[0].clone();
    let f = args[1].clone();
    let resp = args[2].clone();
    let next = k.apply(ctx, resp)?;
    bind_impl(ctx, lift_to_effect_program(next), f)
}
