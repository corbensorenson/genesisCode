use super::*;

#[path = "eval_prims/text_bytes.rs"]
mod text_bytes;

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
        _ => match text_bytes::dispatch_text_bytes_prim(ctx, op, &args) {
            Some(v) => v,
            None => Err(KernelError::new(
                KernelErrorKind::BadForm,
                format!("unknown prim op: {op}"),
            )),
        },
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
