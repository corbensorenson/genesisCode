use super::*;

pub(super) fn dispatch_text_bytes_prim(
    ctx: &mut EvalCtx,
    op: &str,
    args: &[Value],
) -> Option<Result<Value, KernelError>> {
    let out = match op {
        "sym/eq?" => {
            if args.len() != 2 {
                return Some(type_err(ctx, "sym/eq? expects 2 args"));
            }
            let Some(Term::Symbol(a)) = args[0].as_data() else {
                return Some(type_err(ctx, "sym/eq? expects symbols"));
            };
            let Some(Term::Symbol(b)) = args[1].as_data() else {
                return Some(type_err(ctx, "sym/eq? expects symbols"));
            };
            Ok(Value::Data(Term::Bool(a == b)))
        }
        "sym/to-str" => {
            if args.len() != 1 {
                return Some(type_err(ctx, "sym/to-str expects 1 arg"));
            }
            let Some(Term::Symbol(s)) = args[0].as_data() else {
                return Some(type_err(ctx, "sym/to-str expects symbol"));
            };
            if let Err(e) = ctx.mem_observe_string_len(s.len()) {
                return Some(Err(e));
            }
            Ok(Value::Data(Term::Str(s.clone())))
        }
        "sym/from-str" => {
            if args.len() != 1 {
                return Some(type_err(ctx, "sym/from-str expects 1 arg"));
            }
            let Some(Term::Str(s)) = args[0].as_data() else {
                return Some(type_err(ctx, "sym/from-str expects string"));
            };
            if s.is_empty() {
                return Some(type_err(ctx, "sym/from-str expects non-empty string"));
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
                    return Some(type_err(ctx, "sym/from-str invalid symbol text"));
                }
            }
            if let Err(e) = ctx.mem_observe_string_len(s.len()) {
                return Some(Err(e));
            }
            Ok(Value::Data(Term::Symbol(s.clone())))
        }
        "str/concat" => {
            if args.len() != 2 {
                return Some(type_err(ctx, "str/concat expects 2 args"));
            }
            let Some(Term::Str(a)) = args[0].as_data() else {
                return Some(type_err(ctx, "str/concat expects strings"));
            };
            let Some(Term::Str(b)) = args[1].as_data() else {
                return Some(type_err(ctx, "str/concat expects strings"));
            };
            let new_len = a.len().saturating_add(b.len());
            if let Err(e) = ctx.mem_observe_string_len(new_len) {
                return Some(Err(e));
            }
            Ok(Value::Data(Term::Str(format!("{a}{b}"))))
        }
        "str/len" => {
            if args.len() != 1 {
                return Some(type_err(ctx, "str/len expects 1 arg"));
            }
            let Some(Term::Str(s)) = args[0].as_data() else {
                return Some(type_err(ctx, "str/len expects string"));
            };
            Ok(Value::Data(Term::Int((s.len() as i64).into())))
        }
        "str/to-bytes-utf8" => {
            if args.len() != 1 {
                return Some(type_err(ctx, "str/to-bytes-utf8 expects 1 arg"));
            }
            let Some(Term::Str(s)) = args[0].as_data() else {
                return Some(type_err(ctx, "str/to-bytes-utf8 expects string"));
            };
            let bytes = Bytes::copy_from_slice(s.as_bytes());
            if let Err(e) = ctx.mem_observe_bytes_len(bytes.len()) {
                return Some(Err(e));
            }
            Ok(Value::Data(Term::Bytes(bytes)))
        }
        "str/repeat" => {
            if args.len() != 2 {
                return Some(type_err(ctx, "str/repeat expects 2 args"));
            }
            let Some(Term::Str(s)) = args[0].as_data() else {
                return Some(type_err(ctx, "str/repeat expects string"));
            };
            let Some(Term::Int(n)) = args[1].as_data() else {
                return Some(type_err(ctx, "str/repeat expects int count"));
            };
            let n: usize = match n.to_usize() {
                Some(x) => x,
                None => return Some(type_err(ctx, "str/repeat count out of range")),
            };
            let out = s.repeat(n);
            if let Err(e) = ctx.mem_observe_string_len(out.len()) {
                return Some(Err(e));
            }
            Ok(Value::Data(Term::Str(out)))
        }
        "str/join" => {
            if args.len() != 2 {
                return Some(type_err(ctx, "str/join expects 2 args"));
            }
            let Some(Term::Str(sep)) = args[1].as_data() else {
                return Some(type_err(ctx, "str/join expects string separator"));
            };

            let mut parts: Vec<&str> = Vec::new();
            match &args[0] {
                Value::Vector(xs) => {
                    parts.reserve(xs.len());
                    for x in xs {
                        let Some(Term::Str(s)) = x.as_data() else {
                            return Some(type_err(ctx, "str/join expects vector of strings"));
                        };
                        parts.push(s);
                    }
                }
                Value::Data(Term::Vector(xs)) => {
                    parts.reserve(xs.len());
                    for x in xs {
                        let Term::Str(s) = x else {
                            return Some(type_err(ctx, "str/join expects vector of strings"));
                        };
                        parts.push(s);
                    }
                }
                _ => return Some(type_err(ctx, "str/join expects vector")),
            }

            let out = parts.join(sep);
            if let Err(e) = ctx.mem_observe_string_len(out.len()) {
                return Some(Err(e));
            }
            Ok(Value::Data(Term::Str(out)))
        }
        "coreform/escape-str" => {
            if args.len() != 1 {
                return Some(type_err(ctx, "coreform/escape-str expects 1 arg"));
            }
            let Some(Term::Str(s)) = args[0].as_data() else {
                return Some(type_err(ctx, "coreform/escape-str expects string"));
            };
            let out = escape_str(s);
            if let Err(e) = ctx.mem_observe_string_len(out.len()) {
                return Some(Err(e));
            }
            Ok(Value::Data(Term::Str(out)))
        }
        "coreform/escape-bytes" => {
            if args.len() != 1 {
                return Some(type_err(ctx, "coreform/escape-bytes expects 1 arg"));
            }
            let Some(Term::Bytes(b)) = args[0].as_data() else {
                return Some(type_err(ctx, "coreform/escape-bytes expects bytes"));
            };
            let out = escape_bytes(b.as_ref());
            if let Err(e) = ctx.mem_observe_string_len(out.len()) {
                return Some(Err(e));
            }
            Ok(Value::Data(Term::Str(out)))
        }
        "bytes/len" => {
            if args.len() != 1 {
                return Some(type_err(ctx, "bytes/len expects 1 arg"));
            }
            let Some(Term::Bytes(b)) = args[0].as_data() else {
                return Some(type_err(ctx, "bytes/len expects bytes"));
            };
            Ok(Value::Data(Term::Int((b.len() as i64).into())))
        }
        "bytes/get" => {
            if args.len() != 2 {
                return Some(type_err(ctx, "bytes/get expects 2 args"));
            }
            let Some(Term::Bytes(b)) = args[0].as_data() else {
                return Some(type_err(ctx, "bytes/get expects bytes"));
            };
            let Some(Term::Int(i)) = args[1].as_data() else {
                return Some(type_err(ctx, "bytes/get expects int index"));
            };
            let idx: usize = match i.to_usize() {
                Some(x) => x,
                None => return Some(type_err(ctx, "bytes/get index out of range")),
            };
            let Some(x) = b.get(idx) else {
                return Some(type_err(ctx, "bytes/get index out of range"));
            };
            Ok(Value::Data(Term::Int(num_bigint::BigInt::from(*x))))
        }
        "bytes/slice" => {
            if args.len() != 3 {
                return Some(type_err(ctx, "bytes/slice expects 3 args"));
            }
            let Some(Term::Bytes(b)) = args[0].as_data() else {
                return Some(type_err(ctx, "bytes/slice expects bytes"));
            };
            let Some(Term::Int(start_i)) = args[1].as_data() else {
                return Some(type_err(ctx, "bytes/slice expects int start"));
            };
            let Some(Term::Int(len_i)) = args[2].as_data() else {
                return Some(type_err(ctx, "bytes/slice expects int len"));
            };
            let start: usize = match start_i.to_usize() {
                Some(x) => x,
                None => return Some(type_err(ctx, "bytes/slice start out of range")),
            };
            let len: usize = match len_i.to_usize() {
                Some(x) => x,
                None => return Some(type_err(ctx, "bytes/slice len out of range")),
            };
            let end = start.saturating_add(len);
            if start > b.len() || end > b.len() {
                return Some(type_err(ctx, "bytes/slice out of range"));
            }
            let out = b.slice(start..end);
            if let Err(e) = ctx.mem_observe_bytes_len(out.len()) {
                return Some(Err(e));
            }
            Ok(Value::Data(Term::Bytes(out)))
        }
        "bytes/to-str-utf8" => {
            if args.len() != 1 {
                return Some(type_err(ctx, "bytes/to-str-utf8 expects 1 arg"));
            }
            let Some(Term::Bytes(b)) = args[0].as_data() else {
                return Some(type_err(ctx, "bytes/to-str-utf8 expects bytes"));
            };
            let s = match std::str::from_utf8(b.as_ref()) {
                Ok(x) => x,
                Err(_) => return Some(type_err(ctx, "bytes/to-str-utf8 invalid utf8")),
            };
            if let Err(e) = ctx.mem_observe_string_len(s.len()) {
                return Some(Err(e));
            }
            Ok(Value::Data(Term::Str(s.to_string())))
        }
        "int/to-str" => {
            if args.len() != 1 {
                return Some(type_err(ctx, "int/to-str expects 1 arg"));
            }
            let Some(Term::Int(i)) = args[0].as_data() else {
                return Some(type_err(ctx, "int/to-str expects int"));
            };
            let s = i.to_string();
            if let Err(e) = ctx.mem_observe_string_len(s.len()) {
                return Some(Err(e));
            }
            Ok(Value::Data(Term::Str(s)))
        }
        "bytes/to-hex" => {
            if args.len() != 1 {
                return Some(type_err(ctx, "bytes/to-hex expects 1 arg"));
            }
            let Some(Term::Bytes(b)) = args[0].as_data() else {
                return Some(type_err(ctx, "bytes/to-hex expects bytes"));
            };
            const LUT: &[u8; 16] = b"0123456789abcdef";
            let mut out = String::with_capacity(b.len().saturating_mul(2));
            for &x in b.as_ref() {
                out.push(LUT[(x >> 4) as usize] as char);
                out.push(LUT[(x & 0x0f) as usize] as char);
            }
            if let Err(e) = ctx.mem_observe_string_len(out.len()) {
                return Some(Err(e));
            }
            Ok(Value::Data(Term::Str(out)))
        }
        "bytes/from-hex" => {
            if args.len() != 1 {
                return Some(type_err(ctx, "bytes/from-hex expects 1 arg"));
            }
            let Some(Term::Str(s)) = args[0].as_data() else {
                return Some(type_err(ctx, "bytes/from-hex expects string"));
            };
            let bs = s.as_bytes();
            if bs.len() % 2 != 0 {
                return Some(type_err(
                    ctx,
                    "bytes/from-hex expects even-length hex string",
                ));
            }
            fn nybble(b: u8) -> Option<u8> {
                match b {
                    b'0'..=b'9' => Some(b - b'0'),
                    b'a'..=b'f' => Some(b - b'a' + 10),
                    b'A'..=b'F' => Some(b - b'A' + 10),
                    _ => None,
                }
            }
            let mut out = Vec::with_capacity(bs.len() / 2);
            let mut i = 0usize;
            while i < bs.len() {
                let Some(hi) = nybble(bs[i]) else {
                    return Some(type_err(ctx, "bytes/from-hex invalid hex digit"));
                };
                let Some(lo) = nybble(bs[i + 1]) else {
                    return Some(type_err(ctx, "bytes/from-hex invalid hex digit"));
                };
                out.push((hi << 4) | lo);
                i = i.saturating_add(2);
            }
            if let Err(e) = ctx.mem_observe_bytes_len(out.len()) {
                return Some(Err(e));
            }
            Ok(Value::Data(Term::Bytes(Bytes::from(out))))
        }
        "utf8/encode-codepoint" => {
            if args.len() != 1 {
                return Some(type_err(ctx, "utf8/encode-codepoint expects 1 arg"));
            }
            let Some(Term::Int(i)) = args[0].as_data() else {
                return Some(type_err(ctx, "utf8/encode-codepoint expects int"));
            };
            let Some(cp) = i.to_u32() else {
                return Some(type_err(
                    ctx,
                    "utf8/encode-codepoint codepoint out of range",
                ));
            };
            if cp > 0x10FFFF || (0xD800..=0xDFFF).contains(&cp) {
                return Some(type_err(
                    ctx,
                    "utf8/encode-codepoint invalid unicode codepoint",
                ));
            }
            let Some(ch) = char::from_u32(cp) else {
                return Some(type_err(
                    ctx,
                    "utf8/encode-codepoint invalid unicode codepoint",
                ));
            };
            let mut buf = [0u8; 4];
            let s = ch.encode_utf8(&mut buf);
            if let Err(e) = ctx.mem_observe_bytes_len(s.len()) {
                return Some(Err(e));
            }
            Ok(Value::Data(Term::Bytes(Bytes::copy_from_slice(
                s.as_bytes(),
            ))))
        }
        "crypto/blake3" => {
            if args.len() != 1 {
                return Some(type_err(ctx, "crypto/blake3 expects 1 arg"));
            }
            let Some(Term::Bytes(b)) = args[0].as_data() else {
                return Some(type_err(ctx, "crypto/blake3 expects bytes"));
            };
            let h = blake3::hash(b.as_ref());
            let out = Bytes::copy_from_slice(h.as_bytes());
            if let Err(e) = ctx.mem_observe_bytes_len(out.len()) {
                return Some(Err(e));
            }
            Ok(Value::Data(Term::Bytes(out)))
        }
        "bytes/concat" => {
            if args.len() != 2 {
                return Some(type_err(ctx, "bytes/concat expects 2 args"));
            }
            let Some(Term::Bytes(a)) = args[0].as_data() else {
                return Some(type_err(ctx, "bytes/concat expects bytes"));
            };
            let Some(Term::Bytes(b)) = args[1].as_data() else {
                return Some(type_err(ctx, "bytes/concat expects bytes"));
            };
            let new_len = a.len().saturating_add(b.len());
            if let Err(e) = ctx.mem_observe_bytes_len(new_len) {
                return Some(Err(e));
            }
            let mut out = BytesMut::with_capacity(new_len);
            out.extend_from_slice(a.as_ref());
            out.extend_from_slice(b.as_ref());
            Ok(Value::Data(Term::Bytes(out.freeze())))
        }
        "bytes/join" => {
            if args.len() != 1 {
                return Some(type_err(ctx, "bytes/join expects 1 arg"));
            }
            let mut total_len: usize = 0;
            let mut parts: Vec<Bytes> = Vec::new();
            match &args[0] {
                Value::Vector(xs) => {
                    parts.reserve(xs.len());
                    for x in xs {
                        let Some(Term::Bytes(b)) = x.as_data() else {
                            return Some(type_err(ctx, "bytes/join expects vector of bytes"));
                        };
                        total_len = total_len.saturating_add(b.len());
                        parts.push(b.clone());
                    }
                }
                Value::Data(Term::Vector(xs)) => {
                    parts.reserve(xs.len());
                    for x in xs {
                        let Term::Bytes(b) = x else {
                            return Some(type_err(ctx, "bytes/join expects vector of bytes"));
                        };
                        total_len = total_len.saturating_add(b.len());
                        parts.push(b.clone());
                    }
                }
                _ => return Some(type_err(ctx, "bytes/join expects vector")),
            }
            if let Err(e) = ctx.mem_observe_bytes_len(total_len) {
                return Some(Err(e));
            }
            let mut out = BytesMut::with_capacity(total_len);
            for p in parts {
                out.extend_from_slice(p.as_ref());
            }
            Ok(Value::Data(Term::Bytes(out.freeze())))
        }
        _ => return None,
    };
    Some(out)
}
