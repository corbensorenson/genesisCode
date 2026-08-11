use super::{bigint_to_int_value, value_to_bigint};
use crate::error::KernelError;
use crate::value::Value;

use super::super::{EvalCtx, numeric_err, type_err};

pub(super) fn prim_int_div_mod(
    ctx: &mut EvalCtx,
    args: &[Value],
    quotient: bool,
) -> Result<Value, KernelError> {
    if args.len() != 2 {
        let op = if quotient { "int/div" } else { "int/mod" };
        return type_err(ctx, &format!("{op} expects 2 args"));
    }
    prim_int_div_mod_values(ctx, &args[0], &args[1], quotient)
}

pub(super) fn prim_int_div_mod_values(
    ctx: &mut EvalCtx,
    a_value: &Value,
    b_value: &Value,
    quotient: bool,
) -> Result<Value, KernelError> {
    let op = if quotient { "int/div" } else { "int/mod" };
    let Some(a) = value_to_bigint(a_value) else {
        return type_err(ctx, &format!("{op} expects ints"));
    };
    let Some(b) = value_to_bigint(b_value) else {
        return type_err(ctx, &format!("{op} expects ints"));
    };
    if b == num_bigint::BigInt::from(0u8) {
        return numeric_err(ctx, &format!("{op} divisor must not be zero"));
    }

    let mut q = &a / &b;
    let mut r = &a % &b;
    if r.sign() == num_bigint::Sign::Minus {
        if b.sign() == num_bigint::Sign::Minus {
            q += 1u8;
            r -= &b;
        } else {
            q -= 1u8;
            r += &b;
        }
    }
    Ok(bigint_to_int_value(if quotient { q } else { r }))
}
