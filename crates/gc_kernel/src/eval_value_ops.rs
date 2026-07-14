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

pub(super) fn escape_str(s: &str) -> String {
    let mut out = String::new();
    for ch in s.chars() {
        match ch {
            '\\' => out.push_str("\\\\"),
            '"' => out.push_str("\\\""),
            '\n' => out.push_str("\\n"),
            '\r' => out.push_str("\\r"),
            '\t' => out.push_str("\\t"),
            c if c.is_control() => out.push_str(&format!("\\u{:04X}", c as u32)),
            c => out.push(c),
        }
    }
    out
}

pub(super) fn escape_bytes(b: &[u8]) -> String {
    let mut out = String::new();
    for &x in b {
        match x {
            b'\\' => out.push_str("\\\\"),
            b'"' => out.push_str("\\\""),
            b'\n' => out.push_str("\\n"),
            b'\r' => out.push_str("\\r"),
            b'\t' => out.push_str("\\t"),
            0x20..=0x7E => out.push(x as char),
            _ => out.push_str(&format!("\\x{:02X}", x)),
        }
    }
    out
}
