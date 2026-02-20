use super::*;

pub(crate) fn prim(ctx: &mut EvalCtx, op: &str, args: Vec<Value>) -> Result<Value, KernelError> {
    match op {
        "int/add" => prim_int_bin(ctx, &args, |a, b| a + b),
        "int/sub" => prim_int_bin(ctx, &args, |a, b| a - b),
        "int/mul" => prim_int_bin(ctx, &args, |a, b| a * b),
        "int/eq?" => prim_int_cmp(ctx, &args, |a, b| a == b),
        "int/lt?" => prim_int_cmp(ctx, &args, |a, b| a < b),
        "dec/parse" => prim_dec_parse(ctx, &args),
        "dec/to-str" => prim_dec_to_str(ctx, &args),
        "dec/from-int" => prim_dec_from_int(ctx, &args),
        "dec/add" => prim_dec_bin(ctx, &args, |a, b| Ok(a.add(&b))),
        "dec/sub" => prim_dec_bin(ctx, &args, |a, b| Ok(a.sub(&b))),
        "dec/mul" => prim_dec_bin(ctx, &args, |a, b| {
            a.mul(&b)
                .ok_or_else(|| "dec/mul scale overflow".to_string())
        }),
        "dec/eq?" => prim_dec_cmp(ctx, &args, |a, b| a.eq(&b)),
        "dec/lt?" => prim_dec_cmp(ctx, &args, |a, b| a.lt(&b)),
        "core/eq?" => {
            if args.len() != 2 {
                return type_err(ctx, "core/eq? expects 2 args");
            }
            let b = eq_value(&args[0], &args[1]);
            Ok(Value::Data(Term::Bool(b)))
        }
        "pair/cons" => {
            if args.len() != 2 {
                return type_err(ctx, "pair/cons expects 2 args");
            }
            let Some(a) = args[0].as_data() else {
                return type_err(ctx, "pair/cons expects data");
            };
            let Some(d) = args[1].as_data() else {
                return type_err(ctx, "pair/cons expects data");
            };
            ctx.mem_charge_pair_cells(1)?;
            Ok(Value::Data(Term::Pair(
                Box::new(a.clone()),
                Box::new(d.clone()),
            )))
        }
        "pair/car" => {
            if args.len() != 1 {
                return type_err(ctx, "pair/car expects 1 arg");
            }
            let Some(Term::Pair(a, _)) = args[0].as_data() else {
                return type_err(ctx, "pair/car expects a pair");
            };
            Ok(Value::Data((**a).clone()))
        }
        "pair/cdr" => {
            if args.len() != 1 {
                return type_err(ctx, "pair/cdr expects 1 arg");
            }
            let Some(Term::Pair(_, d)) = args[0].as_data() else {
                return type_err(ctx, "pair/cdr expects a pair");
            };
            Ok(Value::Data((**d).clone()))
        }
        "list/is-nil?" => {
            if args.len() != 1 {
                return type_err(ctx, "list/is-nil? expects 1 arg");
            }
            let is = matches!(args[0].as_data(), Some(Term::Nil));
            Ok(Value::Data(Term::Bool(is)))
        }
        "data/tag" => {
            if args.len() != 1 {
                return type_err(ctx, "data/tag expects 1 arg");
            }
            let Some(t) = args[0].as_data() else {
                return type_err(ctx, "data/tag expects datum");
            };
            // Tags must be source-representable symbols (avoid the reserved reader token `nil`).
            let tag = match t {
                Term::Nil => ":nil",
                Term::Bool(_) => ":bool",
                Term::Int(_) => ":int",
                Term::Str(_) => ":str",
                Term::Bytes(_) => ":bytes",
                Term::Symbol(_) => ":sym",
                Term::Pair(_, _) => ":pair",
                Term::Vector(_) => ":vec",
                Term::Map(_) => ":map",
            };
            Ok(Value::Data(Term::Symbol(tag.to_string())))
        }
        "pair/as-proper-list" => {
            if args.len() != 1 {
                return type_err(ctx, "pair/as-proper-list expects 1 arg");
            }
            let Some(t) = args[0].as_data() else {
                return type_err(ctx, "pair/as-proper-list expects datum");
            };
            match t {
                Term::Nil => Ok(Value::Data(Term::Vector(Vec::new()))),
                Term::Pair(_, _) => {
                    let Some(items) = t.as_proper_list() else {
                        return Ok(Value::Data(Term::Nil));
                    };
                    let items: Vec<Term> = items.into_iter().cloned().collect();
                    ctx.mem_observe_vec_len(items.len())?;
                    Ok(Value::Data(Term::Vector(items)))
                }
                _ => type_err(ctx, "pair/as-proper-list expects pair or nil"),
            }
        }
        "map/get" => {
            if args.len() != 2 {
                return type_err(ctx, "map/get expects 2 args");
            }
            let Some(k) = args[1].as_data() else {
                return type_err(ctx, "map/get expects data key");
            };
            match &args[0] {
                Value::Map(m) => Ok(m
                    .get(&TermOrdKey(k.clone()))
                    .cloned()
                    .unwrap_or(Value::Data(Term::Nil))),
                Value::Data(Term::Map(m)) => {
                    let v = m.get(&TermOrdKey(k.clone())).cloned().unwrap_or(Term::Nil);
                    Ok(Value::Data(v))
                }
                _ => type_err(ctx, "map/get expects a map"),
            }
        }
        "map/put" => {
            if args.len() != 3 {
                return type_err(ctx, "map/put expects 3 args");
            }
            let Some(k) = args[1].as_data() else {
                return type_err(ctx, "map/put expects data key");
            };
            match &args[0] {
                Value::Map(m) => {
                    let existed = m.contains_key(&TermOrdKey(k.clone()));
                    let new_len = m.len().saturating_add(if existed { 0 } else { 1 });
                    ctx.mem_observe_map_len(new_len)?;
                    let mut out = m.clone();
                    out.insert(TermOrdKey(k.clone()), args[2].clone());
                    Ok(Value::Map(out))
                }
                Value::Data(Term::Map(m)) => {
                    let Some(v) = args[2].as_data() else {
                        return type_err(ctx, "map/put expects data value when map is data");
                    };
                    let existed = m.contains_key(&TermOrdKey(k.clone()));
                    let new_len = m.len().saturating_add(if existed { 0 } else { 1 });
                    ctx.mem_observe_map_len(new_len)?;
                    let mut out = m.clone();
                    out.insert(TermOrdKey(k.clone()), v.clone());
                    Ok(Value::Data(Term::Map(out)))
                }
                _ => type_err(ctx, "map/put expects a map"),
            }
        }
        "map/merge" => {
            if args.len() != 2 {
                return type_err(ctx, "map/merge expects 2 args");
            }
            match (&args[0], &args[1]) {
                (Value::Map(a), Value::Map(b)) => {
                    let mut out = a.clone();
                    for (k, v) in b.iter() {
                        out.insert(k.clone(), v.clone());
                    }
                    ctx.mem_observe_map_len(out.len())?;
                    Ok(Value::Map(out))
                }
                (Value::Data(Term::Map(a)), Value::Data(Term::Map(b))) => {
                    let mut out = a.clone();
                    for (k, v) in b.iter() {
                        out.insert(k.clone(), v.clone());
                    }
                    ctx.mem_observe_map_len(out.len())?;
                    Ok(Value::Data(Term::Map(out)))
                }
                _ => type_err(ctx, "map/merge expects maps of the same kind"),
            }
        }
        "map/len" => {
            if args.len() != 1 {
                return type_err(ctx, "map/len expects 1 arg");
            }
            let n: usize = match &args[0] {
                Value::Map(m) => m.len(),
                Value::Data(Term::Map(m)) => m.len(),
                _ => return type_err(ctx, "map/len expects map"),
            };
            Ok(Value::Data(Term::Int((n as i64).into())))
        }
        "map/entries" => {
            if args.len() != 1 {
                return type_err(ctx, "map/entries expects 1 arg");
            }
            let Some(Term::Map(m)) = args[0].as_data() else {
                return type_err(ctx, "map/entries expects map datum");
            };
            let mut out: Vec<Term> = Vec::with_capacity(m.len());
            for (k, v) in m.iter() {
                out.push(Term::Vector(vec![k.0.clone(), v.clone()]));
            }
            ctx.mem_observe_vec_len(out.len())?;
            Ok(Value::Data(Term::Vector(out)))
        }
        "map/from-entries" => {
            if args.len() != 1 {
                return type_err(ctx, "map/from-entries expects 1 arg");
            }
            let Some(Term::Vector(es)) = args[0].as_data() else {
                return type_err(ctx, "map/from-entries expects vector datum");
            };
            let mut out: BTreeMap<TermOrdKey, Term> = BTreeMap::new();
            for e in es {
                let Term::Vector(pair) = e else {
                    return type_err(ctx, "map/from-entries expects vector of [k v] entries");
                };
                if pair.len() != 2 {
                    return type_err(ctx, "map/from-entries expects entries of length 2");
                }
                let k = pair[0].clone();
                let v = pair[1].clone();
                out.insert(TermOrdKey(k), v);
            }
            ctx.mem_observe_map_len(out.len())?;
            Ok(Value::Data(Term::Map(out)))
        }
        "vec/get" => {
            if args.len() != 2 {
                return type_err(ctx, "vec/get expects 2 args");
            }
            let Some(Term::Int(i)) = args[1].as_data() else {
                return type_err(ctx, "vec/get expects int index");
            };
            let idx: usize = match i.to_usize() {
                Some(x) => x,
                None => return type_err(ctx, "vec/get index out of range"),
            };
            match &args[0] {
                Value::Vector(xs) => Ok(xs.get(idx).cloned().unwrap_or(Value::Data(Term::Nil))),
                Value::Data(Term::Vector(xs)) => {
                    let v = xs.get(idx).cloned().unwrap_or(Term::Nil);
                    Ok(Value::Data(v))
                }
                _ => type_err(ctx, "vec/get expects vector"),
            }
        }
        "vec/len" => {
            if args.len() != 1 {
                return type_err(ctx, "vec/len expects 1 arg");
            }
            let n: usize = match &args[0] {
                Value::Vector(xs) => xs.len(),
                Value::Data(Term::Vector(xs)) => xs.len(),
                _ => return type_err(ctx, "vec/len expects vector"),
            };
            Ok(Value::Data(Term::Int((n as i64).into())))
        }
        "vec/set" => {
            if args.len() != 3 {
                return type_err(ctx, "vec/set expects 3 args");
            }
            let Some(Term::Int(i)) = args[1].as_data() else {
                return type_err(ctx, "vec/set expects int index");
            };
            let idx: usize = match i.to_usize() {
                Some(x) => x,
                None => return type_err(ctx, "vec/set index out of range"),
            };
            match &args[0] {
                Value::Vector(xs) => {
                    if idx >= xs.len() {
                        return type_err(ctx, "vec/set index out of range");
                    }
                    let mut out = xs.clone();
                    out[idx] = args[2].clone();
                    Ok(Value::Vector(out))
                }
                Value::Data(Term::Vector(xs)) => {
                    if idx >= xs.len() {
                        return type_err(ctx, "vec/set index out of range");
                    }
                    let Some(v) = args[2].as_data() else {
                        return type_err(ctx, "vec/set expects data when vector is data");
                    };
                    let mut out = xs.clone();
                    out[idx] = v.clone();
                    Ok(Value::Data(Term::Vector(out)))
                }
                _ => type_err(ctx, "vec/set expects vector"),
            }
        }
        "vec/push" => {
            if args.len() != 2 {
                return type_err(ctx, "vec/push expects 2 args");
            }
            match &args[0] {
                Value::Vector(xs) => {
                    let new_len = xs.len().saturating_add(1);
                    ctx.mem_observe_vec_len(new_len)?;
                    let mut out = xs.clone();
                    out.push(args[1].clone());
                    Ok(Value::Vector(out))
                }
                Value::Data(Term::Vector(xs)) => {
                    let Some(v) = args[1].as_data() else {
                        return type_err(ctx, "vec/push expects data when vector is data");
                    };
                    let new_len = xs.len().saturating_add(1);
                    ctx.mem_observe_vec_len(new_len)?;
                    let mut out = xs.clone();
                    out.push(v.clone());
                    Ok(Value::Data(Term::Vector(out)))
                }
                _ => type_err(ctx, "vec/push expects vector"),
            }
        }
        "sym/eq?" => {
            if args.len() != 2 {
                return type_err(ctx, "sym/eq? expects 2 args");
            }
            let Some(Term::Symbol(a)) = args[0].as_data() else {
                return type_err(ctx, "sym/eq? expects symbols");
            };
            let Some(Term::Symbol(b)) = args[1].as_data() else {
                return type_err(ctx, "sym/eq? expects symbols");
            };
            Ok(Value::Data(Term::Bool(a == b)))
        }
        "sym/to-str" => {
            if args.len() != 1 {
                return type_err(ctx, "sym/to-str expects 1 arg");
            }
            let Some(Term::Symbol(s)) = args[0].as_data() else {
                return type_err(ctx, "sym/to-str expects symbol");
            };
            ctx.mem_observe_string_len(s.len())?;
            Ok(Value::Data(Term::Str(s.clone())))
        }
        "sym/from-str" => {
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
            Ok(Value::Data(Term::Symbol(s.clone())))
        }
        "str/concat" => {
            if args.len() != 2 {
                return type_err(ctx, "str/concat expects 2 args");
            }
            let Some(Term::Str(a)) = args[0].as_data() else {
                return type_err(ctx, "str/concat expects strings");
            };
            let Some(Term::Str(b)) = args[1].as_data() else {
                return type_err(ctx, "str/concat expects strings");
            };
            let new_len = a.len().saturating_add(b.len());
            ctx.mem_observe_string_len(new_len)?;
            Ok(Value::Data(Term::Str(format!("{a}{b}"))))
        }
        "str/len" => {
            if args.len() != 1 {
                return type_err(ctx, "str/len expects 1 arg");
            }
            let Some(Term::Str(s)) = args[0].as_data() else {
                return type_err(ctx, "str/len expects string");
            };
            Ok(Value::Data(Term::Int((s.len() as i64).into())))
        }
        "str/to-bytes-utf8" => {
            if args.len() != 1 {
                return type_err(ctx, "str/to-bytes-utf8 expects 1 arg");
            }
            let Some(Term::Str(s)) = args[0].as_data() else {
                return type_err(ctx, "str/to-bytes-utf8 expects string");
            };
            let bytes = Bytes::copy_from_slice(s.as_bytes());
            ctx.mem_observe_bytes_len(bytes.len())?;
            Ok(Value::Data(Term::Bytes(bytes)))
        }
        "str/repeat" => {
            if args.len() != 2 {
                return type_err(ctx, "str/repeat expects 2 args");
            }
            let Some(Term::Str(s)) = args[0].as_data() else {
                return type_err(ctx, "str/repeat expects string");
            };
            let Some(Term::Int(n)) = args[1].as_data() else {
                return type_err(ctx, "str/repeat expects int count");
            };
            let n: usize = match n.to_usize() {
                Some(x) => x,
                None => return type_err(ctx, "str/repeat count out of range"),
            };
            let out = s.repeat(n);
            ctx.mem_observe_string_len(out.len())?;
            Ok(Value::Data(Term::Str(out)))
        }
        "str/join" => {
            if args.len() != 2 {
                return type_err(ctx, "str/join expects 2 args");
            }
            let Some(Term::Str(sep)) = args[1].as_data() else {
                return type_err(ctx, "str/join expects string separator");
            };

            let mut parts: Vec<&str> = Vec::new();
            match &args[0] {
                Value::Vector(xs) => {
                    parts.reserve(xs.len());
                    for x in xs {
                        let Some(Term::Str(s)) = x.as_data() else {
                            return type_err(ctx, "str/join expects vector of strings");
                        };
                        parts.push(s);
                    }
                }
                Value::Data(Term::Vector(xs)) => {
                    parts.reserve(xs.len());
                    for x in xs {
                        let Term::Str(s) = x else {
                            return type_err(ctx, "str/join expects vector of strings");
                        };
                        parts.push(s);
                    }
                }
                _ => return type_err(ctx, "str/join expects vector"),
            }

            let out = parts.join(sep);
            ctx.mem_observe_string_len(out.len())?;
            Ok(Value::Data(Term::Str(out)))
        }
        "coreform/escape-str" => {
            if args.len() != 1 {
                return type_err(ctx, "coreform/escape-str expects 1 arg");
            }
            let Some(Term::Str(s)) = args[0].as_data() else {
                return type_err(ctx, "coreform/escape-str expects string");
            };
            let out = escape_str(s);
            ctx.mem_observe_string_len(out.len())?;
            Ok(Value::Data(Term::Str(out)))
        }
        "coreform/escape-bytes" => {
            if args.len() != 1 {
                return type_err(ctx, "coreform/escape-bytes expects 1 arg");
            }
            let Some(Term::Bytes(b)) = args[0].as_data() else {
                return type_err(ctx, "coreform/escape-bytes expects bytes");
            };
            let out = escape_bytes(b.as_ref());
            ctx.mem_observe_string_len(out.len())?;
            Ok(Value::Data(Term::Str(out)))
        }
        "bytes/len" => {
            if args.len() != 1 {
                return type_err(ctx, "bytes/len expects 1 arg");
            }
            let Some(Term::Bytes(b)) = args[0].as_data() else {
                return type_err(ctx, "bytes/len expects bytes");
            };
            Ok(Value::Data(Term::Int((b.len() as i64).into())))
        }
        "bytes/get" => {
            if args.len() != 2 {
                return type_err(ctx, "bytes/get expects 2 args");
            }
            let Some(Term::Bytes(b)) = args[0].as_data() else {
                return type_err(ctx, "bytes/get expects bytes");
            };
            let Some(Term::Int(i)) = args[1].as_data() else {
                return type_err(ctx, "bytes/get expects int index");
            };
            let idx: usize = match i.to_usize() {
                Some(x) => x,
                None => return type_err(ctx, "bytes/get index out of range"),
            };
            let Some(x) = b.get(idx) else {
                return type_err(ctx, "bytes/get index out of range");
            };
            Ok(Value::Data(Term::Int(num_bigint::BigInt::from(*x))))
        }
        "bytes/slice" => {
            if args.len() != 3 {
                return type_err(ctx, "bytes/slice expects 3 args");
            }
            let Some(Term::Bytes(b)) = args[0].as_data() else {
                return type_err(ctx, "bytes/slice expects bytes");
            };
            let Some(Term::Int(start_i)) = args[1].as_data() else {
                return type_err(ctx, "bytes/slice expects int start");
            };
            let Some(Term::Int(len_i)) = args[2].as_data() else {
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
            Ok(Value::Data(Term::Bytes(out)))
        }
        "bytes/to-str-utf8" => {
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
            Ok(Value::Data(Term::Str(s.to_string())))
        }
        "int/to-str" => {
            if args.len() != 1 {
                return type_err(ctx, "int/to-str expects 1 arg");
            }
            let Some(Term::Int(i)) = args[0].as_data() else {
                return type_err(ctx, "int/to-str expects int");
            };
            let s = i.to_string();
            ctx.mem_observe_string_len(s.len())?;
            Ok(Value::Data(Term::Str(s)))
        }
        "bytes/to-hex" => {
            if args.len() != 1 {
                return type_err(ctx, "bytes/to-hex expects 1 arg");
            }
            let Some(Term::Bytes(b)) = args[0].as_data() else {
                return type_err(ctx, "bytes/to-hex expects bytes");
            };
            const LUT: &[u8; 16] = b"0123456789abcdef";
            let mut out = String::with_capacity(b.len().saturating_mul(2));
            for &x in b.as_ref() {
                out.push(LUT[(x >> 4) as usize] as char);
                out.push(LUT[(x & 0x0f) as usize] as char);
            }
            ctx.mem_observe_string_len(out.len())?;
            Ok(Value::Data(Term::Str(out)))
        }
        "bytes/from-hex" => {
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
            let mut out = Vec::with_capacity(bs.len() / 2);
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
            ctx.mem_observe_bytes_len(out.len())?;
            Ok(Value::Data(Term::Bytes(Bytes::from(out))))
        }
        "utf8/encode-codepoint" => {
            if args.len() != 1 {
                return type_err(ctx, "utf8/encode-codepoint expects 1 arg");
            }
            let Some(Term::Int(i)) = args[0].as_data() else {
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
            Ok(Value::Data(Term::Bytes(Bytes::copy_from_slice(
                s.as_bytes(),
            ))))
        }
        "crypto/blake3" => {
            if args.len() != 1 {
                return type_err(ctx, "crypto/blake3 expects 1 arg");
            }
            let Some(Term::Bytes(b)) = args[0].as_data() else {
                return type_err(ctx, "crypto/blake3 expects bytes");
            };
            let h = blake3::hash(b.as_ref());
            let out = Bytes::copy_from_slice(h.as_bytes());
            ctx.mem_observe_bytes_len(out.len())?;
            Ok(Value::Data(Term::Bytes(out)))
        }
        "bytes/concat" => {
            if args.len() != 2 {
                return type_err(ctx, "bytes/concat expects 2 args");
            }
            let Some(Term::Bytes(a)) = args[0].as_data() else {
                return type_err(ctx, "bytes/concat expects bytes");
            };
            let Some(Term::Bytes(b)) = args[1].as_data() else {
                return type_err(ctx, "bytes/concat expects bytes");
            };
            let new_len = a.len().saturating_add(b.len());
            ctx.mem_observe_bytes_len(new_len)?;
            let mut out = BytesMut::with_capacity(new_len);
            out.extend_from_slice(a.as_ref());
            out.extend_from_slice(b.as_ref());
            Ok(Value::Data(Term::Bytes(out.freeze())))
        }
        "bytes/join" => {
            if args.len() != 1 {
                return type_err(ctx, "bytes/join expects 1 arg");
            }
            let mut total_len: usize = 0;
            let mut parts: Vec<Bytes> = Vec::new();
            match &args[0] {
                Value::Vector(xs) => {
                    parts.reserve(xs.len());
                    for x in xs {
                        let Some(Term::Bytes(b)) = x.as_data() else {
                            return type_err(ctx, "bytes/join expects vector of bytes");
                        };
                        total_len = total_len.saturating_add(b.len());
                        parts.push(b.clone());
                    }
                }
                Value::Data(Term::Vector(xs)) => {
                    parts.reserve(xs.len());
                    for x in xs {
                        let Term::Bytes(b) = x else {
                            return type_err(ctx, "bytes/join expects vector of bytes");
                        };
                        total_len = total_len.saturating_add(b.len());
                        parts.push(b.clone());
                    }
                }
                _ => return type_err(ctx, "bytes/join expects vector"),
            }
            ctx.mem_observe_bytes_len(total_len)?;
            let mut out = BytesMut::with_capacity(total_len);
            for p in parts {
                out.extend_from_slice(p.as_ref());
            }
            Ok(Value::Data(Term::Bytes(out.freeze())))
        }
        _ => Err(KernelError::new(
            KernelErrorKind::BadForm,
            format!("unknown prim op: {op}"),
        )),
    }
}

fn prim_int_bin<F>(ctx: &mut EvalCtx, args: &[Value], f: F) -> Result<Value, KernelError>
where
    F: FnOnce(num_bigint::BigInt, num_bigint::BigInt) -> num_bigint::BigInt,
{
    if args.len() != 2 {
        return type_err(ctx, "int op expects 2 args");
    }
    let Some(Term::Int(a)) = args[0].as_data() else {
        return type_err(ctx, "int op expects ints");
    };
    let Some(Term::Int(b)) = args[1].as_data() else {
        return type_err(ctx, "int op expects ints");
    };
    Ok(Value::Data(Term::Int(f(a.clone(), b.clone()))))
}

fn prim_int_cmp<F>(ctx: &mut EvalCtx, args: &[Value], f: F) -> Result<Value, KernelError>
where
    F: FnOnce(num_bigint::BigInt, num_bigint::BigInt) -> bool,
{
    if args.len() != 2 {
        return type_err(ctx, "int cmp expects 2 args");
    }
    let Some(Term::Int(a)) = args[0].as_data() else {
        return type_err(ctx, "int cmp expects ints");
    };
    let Some(Term::Int(b)) = args[1].as_data() else {
        return type_err(ctx, "int cmp expects ints");
    };
    Ok(Value::Data(Term::Bool(f(a.clone(), b.clone()))))
}

pub(crate) fn type_err(ctx: &mut EvalCtx, msg: &str) -> Result<Value, KernelError> {
    if let Some(p) = ctx.protocol {
        let mut m = BTreeMap::new();
        m.insert(
            TermOrdKey(Term::Symbol(":error/code".to_string())),
            Term::Str("core/type-error".to_string()),
        );
        m.insert(
            TermOrdKey(Term::Symbol(":error/message".to_string())),
            Term::Str(msg.to_string()),
        );
        m.insert(
            TermOrdKey(Term::Symbol(":error/context".to_string())),
            Term::Map(
                [(
                    TermOrdKey(Term::Symbol(":kind".to_string())),
                    Term::Str("type".to_string()),
                )]
                .into_iter()
                .collect(),
            ),
        );
        return Ok(Value::Sealed {
            token: p.error,
            payload: Box::new(Value::Data(Term::Map(m))),
        });
    }
    Err(KernelError::new(KernelErrorKind::Type, msg))
}
