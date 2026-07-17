use super::*;
use crate::Shared;
use std::collections::BTreeMap;

#[path = "eval_prims/text_bytes.rs"]
mod text_bytes;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum PrimOp {
    IntAdd,
    IntSub,
    IntMul,
    IntEq,
    IntLt,
    DecParse,
    DecToStr,
    DecFromInt,
    DecAdd,
    DecSub,
    DecMul,
    DecEq,
    DecLt,
    CoreEq,
    PairCons,
    PairCar,
    PairCdr,
    ListIsNil,
    DataTag,
    PairAsProperList,
    MapGet,
    MapPut,
    MapMerge,
    MapLen,
    MapEntries,
    MapFromEntries,
    VecGet,
    VecLen,
    VecSet,
    VecPush,
    SymEq,
    SymToStr,
    SymFromStr,
    StrConcat,
    StrLen,
    StrToBytesUtf8,
    StrRepeat,
    StrJoin,
    CoreformEscapeStr,
    CoreformEscapeBytes,
    BytesLen,
    BytesGet,
    BytesSlice,
    BytesToStrUtf8,
    IntToStr,
    BytesToHex,
    BytesFromHex,
    Utf8EncodeCodepoint,
    CryptoBlake3,
    BytesConcat,
    BytesJoin,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum IntBinOp {
    Add,
    Sub,
    Mul,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum IntCmpOp {
    Eq,
    Lt,
}

impl PrimOp {
    #[cfg(test)]
    pub(crate) const ALL: &'static [PrimOp] = &[
        PrimOp::IntAdd,
        PrimOp::IntSub,
        PrimOp::IntMul,
        PrimOp::IntEq,
        PrimOp::IntLt,
        PrimOp::DecParse,
        PrimOp::DecToStr,
        PrimOp::DecFromInt,
        PrimOp::DecAdd,
        PrimOp::DecSub,
        PrimOp::DecMul,
        PrimOp::DecEq,
        PrimOp::DecLt,
        PrimOp::CoreEq,
        PrimOp::PairCons,
        PrimOp::PairCar,
        PrimOp::PairCdr,
        PrimOp::ListIsNil,
        PrimOp::DataTag,
        PrimOp::PairAsProperList,
        PrimOp::MapGet,
        PrimOp::MapPut,
        PrimOp::MapMerge,
        PrimOp::MapLen,
        PrimOp::MapEntries,
        PrimOp::MapFromEntries,
        PrimOp::VecGet,
        PrimOp::VecLen,
        PrimOp::VecSet,
        PrimOp::VecPush,
        PrimOp::SymEq,
        PrimOp::SymToStr,
        PrimOp::SymFromStr,
        PrimOp::StrConcat,
        PrimOp::StrLen,
        PrimOp::StrToBytesUtf8,
        PrimOp::StrRepeat,
        PrimOp::StrJoin,
        PrimOp::CoreformEscapeStr,
        PrimOp::CoreformEscapeBytes,
        PrimOp::BytesLen,
        PrimOp::BytesGet,
        PrimOp::BytesSlice,
        PrimOp::BytesToStrUtf8,
        PrimOp::IntToStr,
        PrimOp::BytesToHex,
        PrimOp::BytesFromHex,
        PrimOp::Utf8EncodeCodepoint,
        PrimOp::CryptoBlake3,
        PrimOp::BytesConcat,
        PrimOp::BytesJoin,
    ];

    pub(crate) fn from_str(op: &str) -> Option<Self> {
        match op {
            "int/add" => Some(PrimOp::IntAdd),
            "int/sub" => Some(PrimOp::IntSub),
            "int/mul" => Some(PrimOp::IntMul),
            "int/eq?" => Some(PrimOp::IntEq),
            "int/lt?" => Some(PrimOp::IntLt),
            "dec/parse" => Some(PrimOp::DecParse),
            "dec/to-str" => Some(PrimOp::DecToStr),
            "dec/from-int" => Some(PrimOp::DecFromInt),
            "dec/add" => Some(PrimOp::DecAdd),
            "dec/sub" => Some(PrimOp::DecSub),
            "dec/mul" => Some(PrimOp::DecMul),
            "dec/eq?" => Some(PrimOp::DecEq),
            "dec/lt?" => Some(PrimOp::DecLt),
            "core/eq?" => Some(PrimOp::CoreEq),
            "pair/cons" => Some(PrimOp::PairCons),
            "pair/car" => Some(PrimOp::PairCar),
            "pair/cdr" => Some(PrimOp::PairCdr),
            "list/is-nil?" => Some(PrimOp::ListIsNil),
            "data/tag" => Some(PrimOp::DataTag),
            "pair/as-proper-list" => Some(PrimOp::PairAsProperList),
            "map/get" => Some(PrimOp::MapGet),
            "map/put" => Some(PrimOp::MapPut),
            "map/merge" => Some(PrimOp::MapMerge),
            "map/len" => Some(PrimOp::MapLen),
            "map/entries" => Some(PrimOp::MapEntries),
            "map/from-entries" => Some(PrimOp::MapFromEntries),
            "vec/get" => Some(PrimOp::VecGet),
            "vec/len" => Some(PrimOp::VecLen),
            "vec/set" => Some(PrimOp::VecSet),
            "vec/push" => Some(PrimOp::VecPush),
            "sym/eq?" => Some(PrimOp::SymEq),
            "sym/to-str" => Some(PrimOp::SymToStr),
            "sym/from-str" => Some(PrimOp::SymFromStr),
            "str/concat" => Some(PrimOp::StrConcat),
            "str/len" => Some(PrimOp::StrLen),
            "str/to-bytes-utf8" => Some(PrimOp::StrToBytesUtf8),
            "str/repeat" => Some(PrimOp::StrRepeat),
            "str/join" => Some(PrimOp::StrJoin),
            "coreform/escape-str" => Some(PrimOp::CoreformEscapeStr),
            "coreform/escape-bytes" => Some(PrimOp::CoreformEscapeBytes),
            "bytes/len" => Some(PrimOp::BytesLen),
            "bytes/get" => Some(PrimOp::BytesGet),
            "bytes/slice" => Some(PrimOp::BytesSlice),
            "bytes/to-str-utf8" => Some(PrimOp::BytesToStrUtf8),
            "int/to-str" => Some(PrimOp::IntToStr),
            "bytes/to-hex" => Some(PrimOp::BytesToHex),
            "bytes/from-hex" => Some(PrimOp::BytesFromHex),
            "utf8/encode-codepoint" => Some(PrimOp::Utf8EncodeCodepoint),
            "crypto/blake3" => Some(PrimOp::CryptoBlake3),
            "bytes/concat" => Some(PrimOp::BytesConcat),
            "bytes/join" => Some(PrimOp::BytesJoin),
            _ => None,
        }
    }

    pub(crate) fn as_str(self) -> &'static str {
        match self {
            PrimOp::IntAdd => "int/add",
            PrimOp::IntSub => "int/sub",
            PrimOp::IntMul => "int/mul",
            PrimOp::IntEq => "int/eq?",
            PrimOp::IntLt => "int/lt?",
            PrimOp::DecParse => "dec/parse",
            PrimOp::DecToStr => "dec/to-str",
            PrimOp::DecFromInt => "dec/from-int",
            PrimOp::DecAdd => "dec/add",
            PrimOp::DecSub => "dec/sub",
            PrimOp::DecMul => "dec/mul",
            PrimOp::DecEq => "dec/eq?",
            PrimOp::DecLt => "dec/lt?",
            PrimOp::CoreEq => "core/eq?",
            PrimOp::PairCons => "pair/cons",
            PrimOp::PairCar => "pair/car",
            PrimOp::PairCdr => "pair/cdr",
            PrimOp::ListIsNil => "list/is-nil?",
            PrimOp::DataTag => "data/tag",
            PrimOp::PairAsProperList => "pair/as-proper-list",
            PrimOp::MapGet => "map/get",
            PrimOp::MapPut => "map/put",
            PrimOp::MapMerge => "map/merge",
            PrimOp::MapLen => "map/len",
            PrimOp::MapEntries => "map/entries",
            PrimOp::MapFromEntries => "map/from-entries",
            PrimOp::VecGet => "vec/get",
            PrimOp::VecLen => "vec/len",
            PrimOp::VecSet => "vec/set",
            PrimOp::VecPush => "vec/push",
            PrimOp::SymEq => "sym/eq?",
            PrimOp::SymToStr => "sym/to-str",
            PrimOp::SymFromStr => "sym/from-str",
            PrimOp::StrConcat => "str/concat",
            PrimOp::StrLen => "str/len",
            PrimOp::StrToBytesUtf8 => "str/to-bytes-utf8",
            PrimOp::StrRepeat => "str/repeat",
            PrimOp::StrJoin => "str/join",
            PrimOp::CoreformEscapeStr => "coreform/escape-str",
            PrimOp::CoreformEscapeBytes => "coreform/escape-bytes",
            PrimOp::BytesLen => "bytes/len",
            PrimOp::BytesGet => "bytes/get",
            PrimOp::BytesSlice => "bytes/slice",
            PrimOp::BytesToStrUtf8 => "bytes/to-str-utf8",
            PrimOp::IntToStr => "int/to-str",
            PrimOp::BytesToHex => "bytes/to-hex",
            PrimOp::BytesFromHex => "bytes/from-hex",
            PrimOp::Utf8EncodeCodepoint => "utf8/encode-codepoint",
            PrimOp::CryptoBlake3 => "crypto/blake3",
            PrimOp::BytesConcat => "bytes/concat",
            PrimOp::BytesJoin => "bytes/join",
        }
    }
}

pub(crate) fn prim(ctx: &mut EvalCtx, op: &str, args: Vec<Value>) -> Result<Value, KernelError> {
    let Some(op) = PrimOp::from_str(op) else {
        return Err(KernelError::new(
            KernelErrorKind::BadForm,
            format!("unknown prim op: {op}"),
        ));
    };
    prim_op(ctx, op, args)
}

pub(crate) fn prim_op(
    ctx: &mut EvalCtx,
    op: PrimOp,
    args: Vec<Value>,
) -> Result<Value, KernelError> {
    match op {
        PrimOp::IntAdd => prim_int_bin(ctx, &args, "int op expects 2 args", IntBinOp::Add),
        PrimOp::IntSub => prim_int_bin(ctx, &args, "int op expects 2 args", IntBinOp::Sub),
        PrimOp::IntMul => prim_int_bin(ctx, &args, "int op expects 2 args", IntBinOp::Mul),
        PrimOp::IntEq => prim_int_cmp(ctx, &args, "int cmp expects 2 args", IntCmpOp::Eq),
        PrimOp::IntLt => prim_int_cmp(ctx, &args, "int cmp expects 2 args", IntCmpOp::Lt),
        PrimOp::DecParse => prim_dec_parse(ctx, &args),
        PrimOp::DecToStr => prim_dec_to_str(ctx, &args),
        PrimOp::DecFromInt => prim_dec_from_int(ctx, &args),
        PrimOp::DecAdd => prim_dec_bin(ctx, &args, |a, b| Ok(a.add(&b))),
        PrimOp::DecSub => prim_dec_bin(ctx, &args, |a, b| Ok(a.sub(&b))),
        PrimOp::DecMul => prim_dec_bin(ctx, &args, |a, b| {
            a.mul(&b)
                .ok_or_else(|| "dec/mul scale overflow".to_string())
        }),
        PrimOp::DecEq => prim_dec_cmp(ctx, &args, |a, b| a.eq(&b)),
        PrimOp::DecLt => prim_dec_cmp(ctx, &args, |a, b| a.lt(&b)),
        PrimOp::CoreEq => {
            if args.len() != 2 {
                return type_err(ctx, "core/eq? expects 2 args");
            }
            let b = eq_value(&args[0], &args[1]);
            Ok(Value::data(Term::Bool(b)))
        }
        PrimOp::PairCons => {
            if args.len() != 2 {
                return type_err(ctx, "pair/cons expects 2 args");
            }
            let Some(a) = args[0].to_plain_term() else {
                return type_err(ctx, "pair/cons expects data");
            };
            let Some(d) = args[1].to_plain_term() else {
                return type_err(ctx, "pair/cons expects data");
            };
            ctx.mem_charge_pair_cells(1)?;
            Ok(Value::data(Term::Pair(Box::new(a), Box::new(d))))
        }
        PrimOp::PairCar => {
            if args.len() != 1 {
                return type_err(ctx, "pair/car expects 1 arg");
            }
            let Some(Term::Pair(a, _)) = args[0].as_data() else {
                return type_err(ctx, "pair/car expects a pair");
            };
            Ok(Value::data((**a).clone()))
        }
        PrimOp::PairCdr => {
            if args.len() != 1 {
                return type_err(ctx, "pair/cdr expects 1 arg");
            }
            let Some(Term::Pair(_, d)) = args[0].as_data() else {
                return type_err(ctx, "pair/cdr expects a pair");
            };
            Ok(Value::data((**d).clone()))
        }
        PrimOp::ListIsNil => {
            if args.len() != 1 {
                return type_err(ctx, "list/is-nil? expects 1 arg");
            }
            let is = matches!(args[0].as_data(), Some(Term::Nil));
            Ok(Value::data(Term::Bool(is)))
        }
        PrimOp::DataTag => {
            if args.len() != 1 {
                return type_err(ctx, "data/tag expects 1 arg");
            }
            if matches!(args[0], Value::Int(_)) {
                return Ok(Value::data(Term::Symbol(":int".to_string())));
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
            Ok(Value::data(Term::Symbol(tag.to_string())))
        }
        PrimOp::PairAsProperList => {
            if args.len() != 1 {
                return type_err(ctx, "pair/as-proper-list expects 1 arg");
            }
            let Some(t) = args[0].as_data() else {
                return type_err(ctx, "pair/as-proper-list expects datum");
            };
            match t {
                Term::Nil => Ok(Value::data(Term::Vector(Vec::new()))),
                Term::Pair(_, _) => {
                    let Some(items) = t.as_proper_list() else {
                        return Ok(Value::data(Term::Nil));
                    };
                    let items: Vec<Term> = items.into_iter().cloned().collect();
                    ctx.mem_observe_vec_len(items.len())?;
                    Ok(Value::data(Term::Vector(items)))
                }
                _ => type_err(ctx, "pair/as-proper-list expects pair or nil"),
            }
        }
        PrimOp::MapGet => {
            if args.len() != 2 {
                return type_err(ctx, "map/get expects 2 args");
            }
            let Some(k) = args[1].to_plain_term() else {
                return type_err(ctx, "map/get expects data key");
            };
            let key = TermOrdKey(k);
            match &args[0] {
                Value::Map(m) => Ok(m
                    .get(&key)
                    .cloned()
                    .unwrap_or_else(|| Value::data(Term::Nil))),
                Value::Data(t) => match t.as_ref() {
                    Term::Map(m) => {
                        let v = m.get(&key).cloned().unwrap_or(Term::Nil);
                        Ok(Value::data(v))
                    }
                    _ => type_err(ctx, "map/get expects a map"),
                },
                _ => type_err(ctx, "map/get expects a map"),
            }
        }
        PrimOp::MapPut => {
            if args.len() != 3 {
                return type_err(ctx, "map/put expects 3 args");
            }
            let mut args = args;
            let map = args.remove(0);
            let key_value = args.remove(0);
            let value = args.remove(0);
            let Some(k) = key_value.to_plain_term() else {
                return type_err(ctx, "map/put expects data key");
            };
            match map {
                Value::Map(mut m) => {
                    let key = TermOrdKey(k);
                    if ctx
                        .mem_map_len_limit()
                        .is_some_and(|limit| m.size() as u64 >= limit)
                    {
                        let existed = m.get(&key).is_some();
                        let new_len = m.size().saturating_add(if existed { 0 } else { 1 });
                        ctx.mem_observe_map_len(new_len)?;
                    }
                    Shared::make_mut(&mut m).insert_mut(key, value);
                    ctx.mem_observe_map_len(m.size())?;
                    Ok(Value::Map(m))
                }
                Value::Data(t) => match t.as_ref() {
                    Term::Map(m) => {
                        let Some(v) = value.to_plain_term() else {
                            return type_err(ctx, "map/put expects data value when map is data");
                        };
                        let key = TermOrdKey(k);
                        if ctx
                            .mem_map_len_limit()
                            .is_some_and(|limit| m.len() as u64 >= limit)
                        {
                            let existed = m.contains_key(&key);
                            let new_len = m.len().saturating_add(if existed { 0 } else { 1 });
                            ctx.mem_observe_map_len(new_len)?;
                        }
                        let mut out = m.clone();
                        out.insert(key, v);
                        ctx.mem_observe_map_len(out.len())?;
                        Ok(Value::data(Term::Map(out)))
                    }
                    _ => type_err(ctx, "map/put expects a map"),
                },
                _ => type_err(ctx, "map/put expects a map"),
            }
        }
        PrimOp::MapMerge => {
            if args.len() != 2 {
                return type_err(ctx, "map/merge expects 2 args");
            }
            let mut args = args;
            let left = args.remove(0);
            let right = args.remove(0);
            match (left, right) {
                (Value::Map(mut out), Value::Map(b)) => {
                    for (k, v) in b.iter() {
                        Shared::make_mut(&mut out).insert_mut(k.clone(), v.clone());
                    }
                    ctx.mem_observe_map_len(out.size())?;
                    Ok(Value::Map(out))
                }
                (Value::Data(a), Value::Data(b)) => match (a.as_ref(), b.as_ref()) {
                    (Term::Map(a), Term::Map(b)) => {
                        let mut out = a.clone();
                        for (k, v) in b.iter() {
                            out.insert(k.clone(), v.clone());
                        }
                        ctx.mem_observe_map_len(out.len())?;
                        Ok(Value::data(Term::Map(out)))
                    }
                    _ => type_err(ctx, "map/merge expects maps of the same kind"),
                },
                _ => type_err(ctx, "map/merge expects maps of the same kind"),
            }
        }
        PrimOp::MapLen => {
            if args.len() != 1 {
                return type_err(ctx, "map/len expects 1 arg");
            }
            let n: usize = match &args[0] {
                Value::Map(m) => m.size(),
                Value::Data(t) => match t.as_ref() {
                    Term::Map(m) => m.len(),
                    _ => return type_err(ctx, "map/len expects map"),
                },
                _ => return type_err(ctx, "map/len expects map"),
            };
            Ok(usize_to_int_value(n))
        }
        PrimOp::MapEntries => {
            if args.len() != 1 {
                return type_err(ctx, "map/entries expects 1 arg");
            }
            match &args[0] {
                Value::Map(m) => {
                    let mut out: Vec<Term> = Vec::with_capacity(m.size());
                    for (k, v) in m.iter() {
                        let Some(v) = v.to_plain_term() else {
                            return type_err(ctx, "map/entries expects data-compatible map values");
                        };
                        out.push(Term::Vector(vec![k.0.clone(), v]));
                    }
                    ctx.mem_observe_vec_len(out.len())?;
                    Ok(Value::data(Term::Vector(out)))
                }
                Value::Data(t) => {
                    let Term::Map(m) = t.as_ref() else {
                        return type_err(ctx, "map/entries expects map datum");
                    };
                    let mut out: Vec<Term> = Vec::with_capacity(m.len());
                    for (k, v) in m.iter() {
                        out.push(Term::Vector(vec![k.0.clone(), v.clone()]));
                    }
                    ctx.mem_observe_vec_len(out.len())?;
                    Ok(Value::data(Term::Vector(out)))
                }
                _ => type_err(ctx, "map/entries expects map datum"),
            }
        }
        PrimOp::MapFromEntries => {
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
            Ok(Value::data(Term::Map(out)))
        }
        PrimOp::VecGet => {
            if args.len() != 2 {
                return type_err(ctx, "vec/get expects 2 args");
            }
            let Some(i) = value_to_bigint(&args[1]) else {
                return type_err(ctx, "vec/get expects int index");
            };
            let idx: usize = match i.to_usize() {
                Some(x) => x,
                None => return type_err(ctx, "vec/get index out of range"),
            };
            match &args[0] {
                Value::Vector(xs) => Ok(xs
                    .get(idx)
                    .cloned()
                    .unwrap_or_else(|| Value::data(Term::Nil))),
                Value::Data(t) => match t.as_ref() {
                    Term::Vector(xs) => {
                        let v = xs.get(idx).cloned().unwrap_or(Term::Nil);
                        Ok(Value::data(v))
                    }
                    _ => type_err(ctx, "vec/get expects vector"),
                },
                _ => type_err(ctx, "vec/get expects vector"),
            }
        }
        PrimOp::VecLen => {
            if args.len() != 1 {
                return type_err(ctx, "vec/len expects 1 arg");
            }
            let n: usize = match &args[0] {
                Value::Vector(xs) => xs.len(),
                Value::Data(t) => match t.as_ref() {
                    Term::Vector(xs) => xs.len(),
                    _ => return type_err(ctx, "vec/len expects vector"),
                },
                _ => return type_err(ctx, "vec/len expects vector"),
            };
            Ok(usize_to_int_value(n))
        }
        PrimOp::VecSet => {
            if args.len() != 3 {
                return type_err(ctx, "vec/set expects 3 args");
            }
            let mut args = args;
            let vector = args.remove(0);
            let index_value = args.remove(0);
            let value = args.remove(0);
            let Some(i) = value_to_bigint(&index_value) else {
                return type_err(ctx, "vec/set expects int index");
            };
            let idx: usize = match i.to_usize() {
                Some(x) => x,
                None => return type_err(ctx, "vec/set index out of range"),
            };
            match vector {
                Value::Vector(mut xs) => {
                    if idx >= xs.len() {
                        return type_err(ctx, "vec/set index out of range");
                    }
                    if crate::value::ValueVector::set_shared(&mut xs, idx, value) {
                        Ok(Value::Vector(xs))
                    } else {
                        type_err(ctx, "vec/set index out of range")
                    }
                }
                Value::Data(t) => match t.as_ref() {
                    Term::Vector(xs) => {
                        if idx >= xs.len() {
                            return type_err(ctx, "vec/set index out of range");
                        }
                        let Some(v) = value.to_plain_term() else {
                            return type_err(ctx, "vec/set expects data when vector is data");
                        };
                        let mut out = xs.clone();
                        out[idx] = v;
                        Ok(Value::data(Term::Vector(out)))
                    }
                    _ => type_err(ctx, "vec/set expects vector"),
                },
                _ => type_err(ctx, "vec/set expects vector"),
            }
        }
        PrimOp::VecPush => {
            if args.len() != 2 {
                return type_err(ctx, "vec/push expects 2 args");
            }
            let mut args = args;
            let vector = args.remove(0);
            let value = args.remove(0);
            prim_vec_push_values(ctx, vector, value)
        }
        PrimOp::SymEq
        | PrimOp::SymToStr
        | PrimOp::SymFromStr
        | PrimOp::StrConcat
        | PrimOp::StrLen
        | PrimOp::StrToBytesUtf8
        | PrimOp::StrRepeat
        | PrimOp::StrJoin
        | PrimOp::CoreformEscapeStr
        | PrimOp::CoreformEscapeBytes
        | PrimOp::BytesLen
        | PrimOp::BytesGet
        | PrimOp::BytesSlice
        | PrimOp::BytesToStrUtf8
        | PrimOp::IntToStr
        | PrimOp::BytesToHex
        | PrimOp::BytesFromHex
        | PrimOp::Utf8EncodeCodepoint
        | PrimOp::CryptoBlake3
        | PrimOp::BytesConcat
        | PrimOp::BytesJoin => text_bytes::dispatch_text_bytes_prim(ctx, op, &args),
    }
}

pub(crate) fn prim_op2(
    ctx: &mut EvalCtx,
    op: PrimOp,
    a: Value,
    b: Value,
) -> Result<Value, KernelError> {
    match op {
        PrimOp::IntAdd => prim_int_bin_values(ctx, &a, &b, IntBinOp::Add),
        PrimOp::IntSub => prim_int_bin_values(ctx, &a, &b, IntBinOp::Sub),
        PrimOp::IntMul => prim_int_bin_values(ctx, &a, &b, IntBinOp::Mul),
        PrimOp::IntEq => prim_int_cmp_values(ctx, &a, &b, IntCmpOp::Eq),
        PrimOp::IntLt => prim_int_cmp_values(ctx, &a, &b, IntCmpOp::Lt),
        PrimOp::CoreEq => Ok(Value::data(Term::Bool(eq_value(&a, &b)))),
        PrimOp::VecPush => prim_vec_push_values(ctx, a, b),
        _ => prim_op(ctx, op, vec![a, b]),
    }
}

fn prim_int_bin(
    ctx: &mut EvalCtx,
    args: &[Value],
    arity_msg: &str,
    op: IntBinOp,
) -> Result<Value, KernelError> {
    if args.len() != 2 {
        return type_err(ctx, arity_msg);
    }
    prim_int_bin_values(ctx, &args[0], &args[1], op)
}

fn prim_int_bin_values(
    ctx: &mut EvalCtx,
    a_value: &Value,
    b_value: &Value,
    op: IntBinOp,
) -> Result<Value, KernelError> {
    if let (Some(a), Some(b)) = (
        value_to_i64_if_small(a_value),
        value_to_i64_if_small(b_value),
    ) {
        let result = match op {
            IntBinOp::Add => a.checked_add(b),
            IntBinOp::Sub => a.checked_sub(b),
            IntBinOp::Mul => a.checked_mul(b),
        };
        if let Some(result) = result {
            return Ok(Value::int(result));
        }
    }

    let Some(a) = value_to_bigint(a_value) else {
        return type_err(ctx, "int op expects ints");
    };
    let Some(b) = value_to_bigint(b_value) else {
        return type_err(ctx, "int op expects ints");
    };
    let out = match op {
        IntBinOp::Add => a + b,
        IntBinOp::Sub => a - b,
        IntBinOp::Mul => a * b,
    };
    Ok(bigint_to_int_value(out))
}

fn prim_int_cmp(
    ctx: &mut EvalCtx,
    args: &[Value],
    arity_msg: &str,
    op: IntCmpOp,
) -> Result<Value, KernelError> {
    if args.len() != 2 {
        return type_err(ctx, arity_msg);
    }
    prim_int_cmp_values(ctx, &args[0], &args[1], op)
}

fn prim_int_cmp_values(
    ctx: &mut EvalCtx,
    a_value: &Value,
    b_value: &Value,
    op: IntCmpOp,
) -> Result<Value, KernelError> {
    if let (Some(a), Some(b)) = (
        value_to_i64_if_small(a_value),
        value_to_i64_if_small(b_value),
    ) {
        let out = match op {
            IntCmpOp::Eq => a == b,
            IntCmpOp::Lt => a < b,
        };
        return Ok(Value::data(Term::Bool(out)));
    }

    let Some(a) = value_to_bigint(a_value) else {
        return type_err(ctx, "int cmp expects ints");
    };
    let Some(b) = value_to_bigint(b_value) else {
        return type_err(ctx, "int cmp expects ints");
    };
    let out = match op {
        IntCmpOp::Eq => a == b,
        IntCmpOp::Lt => a < b,
    };
    Ok(Value::data(Term::Bool(out)))
}

pub(super) fn value_to_bigint(value: &Value) -> Option<num_bigint::BigInt> {
    match value {
        Value::Int(n) => Some(num_bigint::BigInt::from(*n)),
        Value::Data(t) => match t.as_ref() {
            Term::Int(n) => Some(n.clone()),
            _ => None,
        },
        _ => None,
    }
}

fn value_to_i64_if_small(value: &Value) -> Option<i64> {
    match value {
        Value::Int(n) => Some(*n),
        Value::Data(t) => match t.as_ref() {
            Term::Int(n) => n.to_i64(),
            _ => None,
        },
        _ => None,
    }
}

fn bigint_to_int_value(n: num_bigint::BigInt) -> Value {
    match n.to_i64() {
        Some(n) => Value::int(n),
        None => Value::data(Term::Int(n)),
    }
}

pub(super) fn usize_to_int_value(n: usize) -> Value {
    match i64::try_from(n) {
        Ok(n) => Value::int(n),
        Err(_) => Value::data(Term::Int(num_bigint::BigInt::from(n))),
    }
}

fn prim_vec_push_values(
    ctx: &mut EvalCtx,
    vector: Value,
    value: Value,
) -> Result<Value, KernelError> {
    match vector {
        Value::Vector(mut xs) => {
            let new_len = xs.len().saturating_add(1);
            ctx.mem_observe_vec_len(new_len)?;
            crate::value::ValueVector::push_shared(&mut xs, value);
            Ok(Value::Vector(xs))
        }
        Value::Data(t) => match t.as_ref() {
            Term::Vector(xs) => {
                let Some(v) = value.to_plain_term() else {
                    return type_err(ctx, "vec/push expects data when vector is data");
                };
                let new_len = xs.len().saturating_add(1);
                ctx.mem_observe_vec_len(new_len)?;
                let mut out = xs.clone();
                out.push(v);
                Ok(Value::data(Term::Vector(out)))
            }
            _ => type_err(ctx, "vec/push expects vector"),
        },
        _ => type_err(ctx, "vec/push expects vector"),
    }
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
            payload: Box::new(Value::data(Term::Map(m))),
        });
    }
    Err(KernelError::new(KernelErrorKind::Type, msg))
}
