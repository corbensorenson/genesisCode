use super::*;
use crate::fallible_alloc::{
    checked_add, checked_mul, clone_str, string_with_capacity, vec_with_capacity,
};

pub(super) fn dispatch_text_bytes_prim(
    ctx: &mut EvalCtx,
    op: PrimOp,
    args: &[Value],
) -> Result<Value, KernelError> {
    match op {
        PrimOp::SymEq => {
            if args.len() != 2 {
                return type_err(ctx, "sym/eq? expects 2 args");
            }
            let Some(Term::Symbol(a)) = args[0].as_data() else {
                return type_err(ctx, "sym/eq? expects symbols");
            };
            let Some(Term::Symbol(b)) = args[1].as_data() else {
                return type_err(ctx, "sym/eq? expects symbols");
            };
            Ok(Value::data(Term::Bool(a == b)))
        }
        PrimOp::SymToStr => {
            if args.len() != 1 {
                return type_err(ctx, "sym/to-str expects 1 arg");
            }
            let Some(Term::Symbol(s)) = args[0].as_data() else {
                return type_err(ctx, "sym/to-str expects symbol");
            };
            ctx.mem_observe_string_len(s.len())?;
            Ok(Value::data(Term::Str(clone_str(s, "sym/to-str")?)))
        }
        PrimOp::SymFromStr => {
            if args.len() != 1 {
                return type_err(ctx, "sym/from-str expects 1 arg");
            }
            let Some(Term::Str(s)) = args[0].as_data() else {
                return type_err(ctx, "sym/from-str expects string");
            };
            if s.is_empty() {
                return type_err(ctx, "sym/from-str expects non-empty string");
            }
            // Match lexer delimiter constraints: symbols may not contain whitespace or delimiters.
            let bs = s.as_bytes();
            for &b in bs {
                if matches!(
                    b,
                    b' ' | b'\t'
                        | b'\n'
                        | b'\r'
                        | b'('
                        | b')'
                        | b'['
                        | b']'
                        | b'{'
                        | b'}'
                        | b'\''
                        | b'"'
                        | b';'
                ) {
                    return type_err(ctx, "sym/from-str invalid symbol text");
                }
            }
            ctx.mem_observe_string_len(s.len())?;
            Ok(Value::data(Term::Symbol(clone_str(s, "sym/from-str")?)))
        }
        PrimOp::StrConcat => {
            if args.len() != 2 {
                return type_err(ctx, "str/concat expects 2 args");
            }
            let Some(Term::Str(a)) = args[0].as_data() else {
                return type_err(ctx, "str/concat expects strings");
            };
            let Some(Term::Str(b)) = args[1].as_data() else {
                return type_err(ctx, "str/concat expects strings");
            };
            let new_len = checked_add(a.len(), b.len(), "str/concat")?;
            ctx.mem_observe_string_len(new_len)?;
            let mut out = string_with_capacity(new_len, "str/concat")?;
            out.push_str(a);
            out.push_str(b);
            Ok(Value::data(Term::Str(out)))
        }
        PrimOp::StrLen => {
            if args.len() != 1 {
                return type_err(ctx, "str/len expects 1 arg");
            }
            let Some(Term::Str(s)) = args[0].as_data() else {
                return type_err(ctx, "str/len expects string");
            };
            Ok(usize_to_int_value(s.len()))
        }
        PrimOp::StrToBytesUtf8 => {
            if args.len() != 1 {
                return type_err(ctx, "str/to-bytes-utf8 expects 1 arg");
            }
            let Some(Term::Str(s)) = args[0].as_data() else {
                return type_err(ctx, "str/to-bytes-utf8 expects string");
            };
            ctx.mem_observe_bytes_len(s.len())?;
            let mut bytes = vec_with_capacity(s.len(), "str/to-bytes-utf8")?;
            bytes.extend_from_slice(s.as_bytes());
            Ok(Value::data(Term::Bytes(Bytes::from(bytes))))
        }
        PrimOp::StrRepeat => {
            if args.len() != 2 {
                return type_err(ctx, "str/repeat expects 2 args");
            }
            let Some(Term::Str(s)) = args[0].as_data() else {
                return type_err(ctx, "str/repeat expects string");
            };
            let Some(n) = value_to_bigint(&args[1]) else {
                return type_err(ctx, "str/repeat expects int count");
            };
            let n: usize = match n.to_usize() {
                Some(x) => x,
                None => return type_err(ctx, "str/repeat count out of range"),
            };
            let out_len = checked_mul(s.len(), n, "str/repeat")?;
            ctx.mem_observe_string_len(out_len)?;
            let mut out = string_with_capacity(out_len, "str/repeat")?;
            for _ in 0..n {
                out.push_str(s);
            }
            Ok(Value::data(Term::Str(out)))
        }
        PrimOp::StrJoin => {
            if args.len() != 2 {
                return type_err(ctx, "str/join expects 2 args");
            }
            let Some(Term::Str(sep)) = args[1].as_data() else {
                return type_err(ctx, "str/join expects string separator");
            };

            let part_count = match &args[0] {
                Value::Vector(xs) => xs.len(),
                Value::Data(t) => match t.as_ref() {
                    Term::Vector(xs) => xs.len(),
                    _ => return type_err(ctx, "str/join expects vector"),
                },
                _ => return type_err(ctx, "str/join expects vector"),
            };
            let mut parts = vec_with_capacity(part_count, "str/join parts")?;
            let mut content_len = 0;
            match &args[0] {
                Value::Vector(xs) => {
                    for x in xs.iter() {
                        let Some(Term::Str(s)) = x.as_data() else {
                            return type_err(ctx, "str/join expects vector of strings");
                        };
                        content_len = checked_add(content_len, s.len(), "str/join")?;
                        parts.push(s.as_str());
                    }
                }
                Value::Data(t) => match t.as_ref() {
                    Term::Vector(xs) => {
                        for x in xs {
                            let Term::Str(s) = x else {
                                return type_err(ctx, "str/join expects vector of strings");
                            };
                            content_len = checked_add(content_len, s.len(), "str/join")?;
                            parts.push(s);
                        }
                    }
                    _ => return type_err(ctx, "str/join expects vector"),
                },
                _ => return type_err(ctx, "str/join expects vector"),
            }
            let separator_len = checked_mul(sep.len(), parts.len().saturating_sub(1), "str/join")?;
            let out_len = checked_add(content_len, separator_len, "str/join")?;
            ctx.mem_observe_string_len(out_len)?;
            let mut out = string_with_capacity(out_len, "str/join")?;
            for (index, part) in parts.into_iter().enumerate() {
                if index != 0 {
                    out.push_str(sep);
                }
                out.push_str(part);
            }
            Ok(Value::data(Term::Str(out)))
        }
        PrimOp::CoreformEscapeStr => {
            if args.len() != 1 {
                return type_err(ctx, "coreform/escape-str expects 1 arg");
            }
            let Some(Term::Str(s)) = args[0].as_data() else {
                return type_err(ctx, "coreform/escape-str expects string");
            };
            let out = escape_str(ctx, s)?;
            Ok(Value::data(Term::Str(out)))
        }
        PrimOp::CoreformEscapeBytes => {
            if args.len() != 1 {
                return type_err(ctx, "coreform/escape-bytes expects 1 arg");
            }
            let Some(Term::Bytes(b)) = args[0].as_data() else {
                return type_err(ctx, "coreform/escape-bytes expects bytes");
            };
            let out = escape_bytes(ctx, b.as_ref())?;
            Ok(Value::data(Term::Str(out)))
        }
        PrimOp::BytesLen => {
            if args.len() != 1 {
                return type_err(ctx, "bytes/len expects 1 arg");
            }
            let Some(Term::Bytes(b)) = args[0].as_data() else {
                return type_err(ctx, "bytes/len expects bytes");
            };
            Ok(usize_to_int_value(b.len()))
        }
        PrimOp::BytesGet => {
            if args.len() != 2 {
                return type_err(ctx, "bytes/get expects 2 args");
            }
            let Some(Term::Bytes(b)) = args[0].as_data() else {
                return type_err(ctx, "bytes/get expects bytes");
            };
            let Some(i) = value_to_bigint(&args[1]) else {
                return type_err(ctx, "bytes/get expects int index");
            };
            let idx: usize = match i.to_usize() {
                Some(x) => x,
                None => return type_err(ctx, "bytes/get index out of range"),
            };
            let Some(x) = b.get(idx) else {
                return type_err(ctx, "bytes/get index out of range");
            };
            Ok(Value::int(i64::from(*x)))
        }
        PrimOp::BytesSlice => {
            if args.len() != 3 {
                return type_err(ctx, "bytes/slice expects 3 args");
            }
            let Some(Term::Bytes(b)) = args[0].as_data() else {
                return type_err(ctx, "bytes/slice expects bytes");
            };
            let Some(start_i) = value_to_bigint(&args[1]) else {
                return type_err(ctx, "bytes/slice expects int start");
            };
            let Some(len_i) = value_to_bigint(&args[2]) else {
                return type_err(ctx, "bytes/slice expects int len");
            };
            let start: usize = match start_i.to_usize() {
                Some(x) => x,
                None => return type_err(ctx, "bytes/slice start out of range"),
            };
            let len: usize = match len_i.to_usize() {
                Some(x) => x,
                None => return type_err(ctx, "bytes/slice len out of range"),
            };
            let end = start.saturating_add(len);
            if start > b.len() || end > b.len() {
                return type_err(ctx, "bytes/slice out of range");
            }
            let out = b.slice(start..end);
            ctx.mem_observe_bytes_len(out.len())?;
            Ok(Value::data(Term::Bytes(out)))
        }
        PrimOp::BytesToStrUtf8 => {
            if args.len() != 1 {
                return type_err(ctx, "bytes/to-str-utf8 expects 1 arg");
            }
            let Some(Term::Bytes(b)) = args[0].as_data() else {
                return type_err(ctx, "bytes/to-str-utf8 expects bytes");
            };
            let s = match std::str::from_utf8(b.as_ref()) {
                Ok(x) => x,
                Err(_) => return type_err(ctx, "bytes/to-str-utf8 invalid utf8"),
            };
            ctx.mem_observe_string_len(s.len())?;
            Ok(Value::data(Term::Str(clone_str(s, "bytes/to-str-utf8")?)))
        }
        PrimOp::IntToStr => {
            if args.len() != 1 {
                return type_err(ctx, "int/to-str expects 1 arg");
            }
            let Some(i) = value_to_bigint(&args[0]) else {
                return type_err(ctx, "int/to-str expects int");
            };
            let s = i.to_string();
            ctx.mem_observe_string_len(s.len())?;
            Ok(Value::data(Term::Str(s)))
        }
        PrimOp::BytesToHex => {
            if args.len() != 1 {
                return type_err(ctx, "bytes/to-hex expects 1 arg");
            }
            let Some(Term::Bytes(b)) = args[0].as_data() else {
                return type_err(ctx, "bytes/to-hex expects bytes");
            };
            const LUT: &[u8; 16] = b"0123456789abcdef";
            let out_len = checked_mul(b.len(), 2, "bytes/to-hex")?;
            ctx.mem_observe_string_len(out_len)?;
            let mut out = string_with_capacity(out_len, "bytes/to-hex")?;
            for &x in b.as_ref() {
                out.push(LUT[(x >> 4) as usize] as char);
                out.push(LUT[(x & 0x0f) as usize] as char);
            }
            Ok(Value::data(Term::Str(out)))
        }
        PrimOp::BytesFromHex => {
            if args.len() != 1 {
                return type_err(ctx, "bytes/from-hex expects 1 arg");
            }
            let Some(Term::Str(s)) = args[0].as_data() else {
                return type_err(ctx, "bytes/from-hex expects string");
            };
            let bs = s.as_bytes();
            if bs.len() % 2 != 0 {
                return type_err(ctx, "bytes/from-hex expects even-length hex string");
            }
            fn nybble(b: u8) -> Option<u8> {
                match b {
                    b'0'..=b'9' => Some(b - b'0'),
                    b'a'..=b'f' => Some(b - b'a' + 10),
                    b'A'..=b'F' => Some(b - b'A' + 10),
                    _ => None,
                }
            }
            let out_len = bs.len() / 2;
            ctx.mem_observe_bytes_len(out_len)?;
            let mut out = vec_with_capacity(out_len, "bytes/from-hex")?;
            let mut i = 0usize;
            while i < bs.len() {
                let Some(hi) = nybble(bs[i]) else {
                    return type_err(ctx, "bytes/from-hex invalid hex digit");
                };
                let Some(lo) = nybble(bs[i + 1]) else {
                    return type_err(ctx, "bytes/from-hex invalid hex digit");
                };
                out.push((hi << 4) | lo);
                i = i.saturating_add(2);
            }
            Ok(Value::data(Term::Bytes(Bytes::from(out))))
        }
        PrimOp::Utf8EncodeCodepoint => {
            if args.len() != 1 {
                return type_err(ctx, "utf8/encode-codepoint expects 1 arg");
            }
            let Some(i) = value_to_bigint(&args[0]) else {
                return type_err(ctx, "utf8/encode-codepoint expects int");
            };
            let Some(cp) = i.to_u32() else {
                return type_err(ctx, "utf8/encode-codepoint codepoint out of range");
            };
            if cp > 0x10FFFF || (0xD800..=0xDFFF).contains(&cp) {
                return type_err(ctx, "utf8/encode-codepoint invalid unicode codepoint");
            }
            let Some(ch) = char::from_u32(cp) else {
                return type_err(ctx, "utf8/encode-codepoint invalid unicode codepoint");
            };
            let mut buf = [0u8; 4];
            let s = ch.encode_utf8(&mut buf);
            ctx.mem_observe_bytes_len(s.len())?;
            Ok(Value::data(Term::Bytes(Bytes::copy_from_slice(
                s.as_bytes(),
            ))))
        }
        PrimOp::CryptoBlake3 => {
            if args.len() != 1 {
                return type_err(ctx, "crypto/blake3 expects 1 arg");
            }
            let Some(Term::Bytes(b)) = args[0].as_data() else {
                return type_err(ctx, "crypto/blake3 expects bytes");
            };
            let h = blake3::hash(b.as_ref());
            let out = Bytes::copy_from_slice(h.as_bytes());
            ctx.mem_observe_bytes_len(out.len())?;
            Ok(Value::data(Term::Bytes(out)))
        }
        PrimOp::BytesConcat => {
            if args.len() != 2 {
                return type_err(ctx, "bytes/concat expects 2 args");
            }
            let Some(Term::Bytes(a)) = args[0].as_data() else {
                return type_err(ctx, "bytes/concat expects bytes");
            };
            let Some(Term::Bytes(b)) = args[1].as_data() else {
                return type_err(ctx, "bytes/concat expects bytes");
            };
            let new_len = checked_add(a.len(), b.len(), "bytes/concat")?;
            ctx.mem_observe_bytes_len(new_len)?;
            let mut out = vec_with_capacity(new_len, "bytes/concat")?;
            out.extend_from_slice(a.as_ref());
            out.extend_from_slice(b.as_ref());
            Ok(Value::data(Term::Bytes(Bytes::from(out))))
        }
        PrimOp::BytesJoin => {
            if args.len() != 1 {
                return type_err(ctx, "bytes/join expects 1 arg");
            }
            let mut total_len: usize = 0;
            let part_count = match &args[0] {
                Value::Vector(xs) => xs.len(),
                Value::Data(t) => match t.as_ref() {
                    Term::Vector(xs) => xs.len(),
                    _ => return type_err(ctx, "bytes/join expects vector"),
                },
                _ => return type_err(ctx, "bytes/join expects vector"),
            };
            let mut parts = vec_with_capacity(part_count, "bytes/join parts")?;
            match &args[0] {
                Value::Vector(xs) => {
                    for x in xs.iter() {
                        let Some(Term::Bytes(b)) = x.as_data() else {
                            return type_err(ctx, "bytes/join expects vector of bytes");
                        };
                        total_len = checked_add(total_len, b.len(), "bytes/join")?;
                        parts.push(b.clone());
                    }
                }
                Value::Data(t) => match t.as_ref() {
                    Term::Vector(xs) => {
                        for x in xs {
                            let Term::Bytes(b) = x else {
                                return type_err(ctx, "bytes/join expects vector of bytes");
                            };
                            total_len = checked_add(total_len, b.len(), "bytes/join")?;
                            parts.push(b.clone());
                        }
                    }
                    _ => return type_err(ctx, "bytes/join expects vector"),
                },
                _ => return type_err(ctx, "bytes/join expects vector"),
            }
            ctx.mem_observe_bytes_len(total_len)?;
            let mut out = vec_with_capacity(total_len, "bytes/join")?;
            for p in parts {
                out.extend_from_slice(p.as_ref());
            }
            Ok(Value::data(Term::Bytes(Bytes::from(out))))
        }
        _ => Err(KernelError::new(
            KernelErrorKind::Internal,
            format!(
                "non text/bytes prim routed to text dispatcher: {}",
                op.as_str()
            ),
        )),
    }
}
