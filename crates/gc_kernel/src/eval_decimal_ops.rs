use crate::error::KernelError;
use crate::value::Value;
use gc_coreform::{FixedDecimal, Term};

use super::{EvalCtx, eval_prims::value_to_bigint, type_err};

pub(super) fn prim_dec_parse(ctx: &mut EvalCtx, args: &[Value]) -> Result<Value, KernelError> {
    if args.len() != 1 {
        return type_err(ctx, "dec/parse expects 1 arg");
    }
    let Some(Term::Str(s)) = args[0].as_data() else {
        return type_err(ctx, "dec/parse expects string");
    };
    let d = match FixedDecimal::parse(s) {
        Ok(x) => x,
        Err(msg) => return type_err(ctx, &msg),
    };
    let t = d.to_term();
    ctx.mem_observe_map_len(3)?;
    Ok(Value::data(t))
}

pub(super) fn prim_dec_to_str(ctx: &mut EvalCtx, args: &[Value]) -> Result<Value, KernelError> {
    if args.len() != 1 {
        return type_err(ctx, "dec/to-str expects 1 arg");
    }
    let d = match as_fixed_decimal(ctx, &args[0]) {
        Ok(x) => x,
        Err(e) => return e,
    };
    let s = d.to_canonical_string();
    ctx.mem_observe_string_len(s.len())?;
    Ok(Value::data(Term::Str(s)))
}

pub(super) fn prim_dec_from_int(ctx: &mut EvalCtx, args: &[Value]) -> Result<Value, KernelError> {
    if args.len() != 1 {
        return type_err(ctx, "dec/from-int expects 1 arg");
    }
    let Some(i) = value_to_bigint(&args[0]) else {
        return type_err(ctx, "dec/from-int expects int");
    };
    let d = FixedDecimal::from_int(i);
    let t = d.to_term();
    ctx.mem_observe_map_len(3)?;
    Ok(Value::data(t))
}

pub(super) fn prim_dec_bin<F>(ctx: &mut EvalCtx, args: &[Value], f: F) -> Result<Value, KernelError>
where
    F: FnOnce(FixedDecimal, FixedDecimal) -> Result<FixedDecimal, String>,
{
    if args.len() != 2 {
        return type_err(ctx, "decimal op expects 2 args");
    }
    let a = match as_fixed_decimal(ctx, &args[0]) {
        Ok(x) => x,
        Err(e) => return e,
    };
    let b = match as_fixed_decimal(ctx, &args[1]) {
        Ok(x) => x,
        Err(e) => return e,
    };
    let out = match f(a, b) {
        Ok(x) => x,
        Err(msg) => return type_err(ctx, &msg),
    };
    let t = out.to_term();
    ctx.mem_observe_map_len(3)?;
    Ok(Value::data(t))
}

pub(super) fn prim_dec_cmp<F>(ctx: &mut EvalCtx, args: &[Value], f: F) -> Result<Value, KernelError>
where
    F: FnOnce(FixedDecimal, FixedDecimal) -> bool,
{
    if args.len() != 2 {
        return type_err(ctx, "decimal cmp expects 2 args");
    }
    let a = match as_fixed_decimal(ctx, &args[0]) {
        Ok(x) => x,
        Err(e) => return e,
    };
    let b = match as_fixed_decimal(ctx, &args[1]) {
        Ok(x) => x,
        Err(e) => return e,
    };
    Ok(Value::data(Term::Bool(f(a, b))))
}

fn as_fixed_decimal(
    ctx: &mut EvalCtx,
    v: &Value,
) -> Result<FixedDecimal, Result<Value, KernelError>> {
    let Some(t) = v.as_data() else {
        return Err(type_err(ctx, "decimal op expects decimal datum"));
    };
    FixedDecimal::from_term(t)
        .ok_or_else(|| type_err(ctx, "decimal op expects fixed decimal datum"))
}
