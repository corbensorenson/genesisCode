use super::EvalCtx;
use crate::error::KernelError;
use crate::fallible_alloc::{checked_add, string_with_capacity};
use crate::value::Value;

pub(super) fn eq_value(a: &Value, b: &Value) -> bool {
    match (a, b) {
        (Value::Data(x), Value::Data(y)) => x == y,
        (Value::Int(x), Value::Int(y)) => x == y,
        (Value::Int(x), Value::Data(y)) | (Value::Data(y), Value::Int(x)) => {
            matches!(y.as_ref(), gc_coreform::Term::Int(n) if n == &num_bigint::BigInt::from(*x))
        }
        (Value::Vector(x), Value::Vector(y)) => {
            x.len() == y.len() && x.iter().zip(y.iter()).all(|(a, b)| eq_value(a, b))
        }
        (Value::Map(x), Value::Map(y)) => {
            x.size() == y.size()
                && x.iter()
                    .zip(y.iter())
                    .all(|((ak, av), (bk, bv))| ak == bk && eq_value(av, bv))
        }
        (Value::SealToken(x), Value::SealToken(y)) => x == y,
        (
            Value::Sealed {
                token: xt,
                payload: xp,
            },
            Value::Sealed {
                token: yt,
                payload: yp,
            },
        ) => xt == yt && eq_value(xp, yp),
        (Value::NativeFn(x), Value::NativeFn(y)) => {
            x.name == y.name
                && x.arity == y.arity
                && x.collected.len() == y.collected.len()
                && x.collected
                    .iter()
                    .zip(y.collected.iter())
                    .all(|(a, b)| eq_value(a, b))
        }
        (Value::Contract(x), Value::Contract(y)) => x.contract_id == y.contract_id,
        _ => false,
    }
}

pub(super) fn escape_str(ctx: &mut EvalCtx, s: &str) -> Result<String, KernelError> {
    let mut len = 0;
    for ch in s.chars() {
        let width = match ch {
            '\\' | '"' | '\n' | '\r' | '\t' => 2,
            c if c.is_control() => 6,
            c => c.len_utf8(),
        };
        len = checked_add(len, width, "coreform/escape-str")?;
    }
    ctx.mem_observe_string_len(len)?;
    let mut out = string_with_capacity(len, "coreform/escape-str")?;
    const HEX: &[u8; 16] = b"0123456789ABCDEF";
    for ch in s.chars() {
        match ch {
            '\\' => out.push_str("\\\\"),
            '"' => out.push_str("\\\""),
            '\n' => out.push_str("\\n"),
            '\r' => out.push_str("\\r"),
            '\t' => out.push_str("\\t"),
            c if c.is_control() => {
                let value = c as u32;
                out.push_str("\\u");
                for shift in [12, 8, 4, 0] {
                    out.push(HEX[((value >> shift) & 0x0f) as usize] as char);
                }
            }
            c => out.push(c),
        }
    }
    Ok(out)
}

pub(super) fn escape_bytes(ctx: &mut EvalCtx, b: &[u8]) -> Result<String, KernelError> {
    let mut len = 0;
    for &byte in b {
        let width = match byte {
            b'\\' | b'"' | b'\n' | b'\r' | b'\t' => 2,
            0x20..=0x7E => 1,
            _ => 4,
        };
        len = checked_add(len, width, "coreform/escape-bytes")?;
    }
    ctx.mem_observe_string_len(len)?;
    let mut out = string_with_capacity(len, "coreform/escape-bytes")?;
    const HEX: &[u8; 16] = b"0123456789ABCDEF";
    for &x in b {
        match x {
            b'\\' => out.push_str("\\\\"),
            b'"' => out.push_str("\\\""),
            b'\n' => out.push_str("\\n"),
            b'\r' => out.push_str("\\r"),
            b'\t' => out.push_str("\\t"),
            0x20..=0x7E => out.push(x as char),
            _ => {
                out.push_str("\\x");
                out.push(HEX[(x >> 4) as usize] as char);
                out.push(HEX[(x & 0x0f) as usize] as char);
            }
        }
    }
    Ok(out)
}
