use std::collections::{BTreeMap, BTreeSet};

use gc_coreform::{Term, TermOrdKey, print_term};

use crate::ty::{EffRow, RowTail, Ty};

#[derive(Default)]
pub struct InferSession {
    pub errors: Vec<String>,
    pub warnings: Vec<String>,
}

#[derive(Default, Clone)]
pub struct TypeEnv {
    vars: BTreeMap<String, Ty>,
}

impl TypeEnv {
    pub fn with_prelude(declared: &BTreeMap<String, Ty>) -> Self {
        // Treat prelude-provided bindings as gradual/unknown unless they are core builtins
        // we special-case in `infer_app`.
        let mut vars = BTreeMap::new();

        // Seed declared export types so inference can be row-polymorphic across uses.
        for (k, v) in declared {
            vars.insert(k.clone(), v.clone());
        }

        // Provide a stable type for genesis contract root so contract-row tails can be preserved.
        vars.insert(
            "core/contract::genesis".to_string(),
            Ty::Contract {
                methods: BTreeMap::new(),
                tail: RowTail::Var("r".to_string()),
            },
        );

        Self { vars }
    }

    pub fn get(&self, s: &str) -> Option<&Ty> {
        self.vars.get(s)
    }

    pub fn set(&mut self, s: String, t: Ty) {
        self.vars.insert(s, t);
    }
}

pub fn infer_module_types(
    forms: &[Term],
    sess: &mut InferSession,
    declared: &BTreeMap<String, Ty>,
) -> (TypeEnv, BTreeMap<String, Ty>) {
    let mut env = TypeEnv::with_prelude(declared);
    let mut defs = BTreeMap::new();
    for f in forms {
        let Some(items) = f.as_proper_list() else {
            continue;
        };
        if items.len() == 3
            && matches!(items[0], Term::Symbol(s) if s == "def")
            && let Term::Symbol(name) = items[1]
        {
            let ty = infer_term(items[2], &env, sess);
            env.set(name.clone(), ty.clone());
            defs.insert(name.clone(), ty);
        }
    }
    (env, defs)
}

pub fn infer_term(t: &Term, env: &TypeEnv, sess: &mut InferSession) -> Ty {
    match t {
        Term::Nil => Ty::Nil,
        Term::Bool(_) => Ty::Bool,
        Term::Int(_) => Ty::Int,
        Term::Str(_) => Ty::Str,
        Term::Bytes(_) => Ty::Bytes,
        Term::Symbol(s) => env.get(s).cloned().unwrap_or(Ty::Any),
        Term::Vector(_xs) => Ty::Any, // vectors are data in v0.2
        Term::Map(m) => infer_map_literal(m, env, sess),
        Term::Pair(_, _) => infer_list_form(t, env, sess),
    }
}

fn infer_map_literal(m: &BTreeMap<TermOrdKey, Term>, env: &TypeEnv, sess: &mut InferSession) -> Ty {
    let mut fields = BTreeMap::new();
    let mut tail = RowTail::Closed;
    for (k, v) in m {
        let key = match &k.0 {
            Term::Symbol(s) => Some(s.clone()),
            Term::Str(s) => Some(s.clone()),
            _ => None,
        };
        if let Some(lbl) = key {
            fields.insert(lbl, infer_term(v, env, sess));
        } else {
            tail = RowTail::Any;
            // Still traverse for side knowledge (effects live in syntax).
            let _ = infer_term(v, env, sess);
        }
    }
    Ty::Rec { fields, tail }
}

fn infer_list_form(t: &Term, env: &TypeEnv, sess: &mut InferSession) -> Ty {
    let Some(items) = t.as_proper_list() else {
        sess.errors
            .push("improper list is not a valid expression".to_string());
        return Ty::Any;
    };
    if items.is_empty() {
        return Ty::Nil;
    }

    if matches!(items[0], Term::Symbol(s) if s == "quote") {
        return Ty::Any;
    }
    if matches!(items[0], Term::Symbol(s) if s == "fn") {
        return infer_fn(items, env, sess);
    }
    if matches!(items[0], Term::Symbol(s) if s == "if") {
        return infer_if(items, env, sess);
    }
    if matches!(items[0], Term::Symbol(s) if s == "begin") {
        return infer_begin(items, env, sess);
    }
    if matches!(items[0], Term::Symbol(s) if s == "let") {
        return infer_let(items, env, sess);
    }
    if matches!(items[0], Term::Symbol(s) if s == "prim") {
        return infer_prim(items, env, sess);
    }
    if matches!(items[0], Term::Symbol(s) if s == "seal" || s == "unseal") {
        // Seals are intentionally treated as opaque under gradual typing.
        for a in items.iter().skip(1) {
            let _ = infer_term(a, env, sess);
        }
        return Ty::Any;
    }
    if matches!(items[0], Term::Symbol(s) if s == "def") {
        // (def ...) is a top-level form; if it appears as an expression, treat as Any but walk it.
        for a in items.iter().skip(1) {
            let _ = infer_term(a, env, sess);
        }
        return Ty::Any;
    }

    if let Some((head, args)) = flatten_app(t) {
        return infer_app(&head, &args, env, sess);
    }
    Ty::Any
}

fn infer_fn(items: Vec<&Term>, env: &TypeEnv, sess: &mut InferSession) -> Ty {
    if items.len() < 3 {
        sess.errors
            .push("(fn (x) body...) expects at least 2 arguments".to_string());
        return Ty::Any;
    }
    let params = items[1].as_proper_list();
    let Some(params) = params else {
        sess.errors.push(format!(
            "fn params must be a list of symbols, got {}",
            print_term(items[1])
        ));
        return Ty::Any;
    };
    let names: Vec<String> = params
        .iter()
        .filter_map(|p| match p {
            Term::Symbol(s) => Some(s.clone()),
            _ => None,
        })
        .collect();
    if names.is_empty() {
        sess.errors
            .push("fn must have at least 1 param".to_string());
        return Ty::Any;
    }

    // Canonical form is unary; support multi-arg by nesting.
    let body: Term = if names.len() == 1 {
        if items.len() == 3 {
            items[2].clone()
        } else {
            let mut xs = Vec::new();
            xs.push(Term::Symbol("begin".to_string()));
            for b in items.iter().skip(2) {
                xs.push((*b).clone());
            }
            Term::list(xs)
        }
    } else {
        // Convert (fn (x y) body...) into (fn (x) (fn (y) body...))
        // Reconstruct using the original params and bodies.
        let mut bodies: Vec<Term> = items.iter().skip(2).cloned().cloned().collect();
        if bodies.len() > 1 {
            let mut xs = Vec::new();
            xs.push(Term::Symbol("begin".to_string()));
            xs.append(&mut bodies);
            bodies = vec![Term::list(xs)];
        }
        let mut inner = bodies[0].clone();
        for p in names.iter().skip(1).rev() {
            inner = Term::list(vec![
                Term::Symbol("fn".to_string()),
                Term::list(vec![Term::Symbol(p.clone())]),
                inner,
            ]);
        }
        let cur = Term::list(vec![
            Term::Symbol("fn".to_string()),
            Term::list(vec![Term::Symbol(names[0].clone())]),
            inner,
        ]);
        return infer_term(&cur, env, sess);
    };

    let mut env2 = env.clone();
    env2.set(names[0].clone(), Ty::Any);
    let ret = infer_term(&body, &env2, sess);
    let eff = {
        let inf = crate::infer_effects_in_term(&body);
        let tail = if inf.unknown {
            RowTail::Any
        } else {
            RowTail::Closed
        };
        EffRow { ops: inf.ops, tail }
    };
    Ty::Fn {
        param: Box::new(Ty::Any),
        ret: Box::new(ret),
        eff,
    }
}

fn infer_if(items: Vec<&Term>, env: &TypeEnv, sess: &mut InferSession) -> Ty {
    if items.len() != 4 {
        sess.errors
            .push("(if c t e) expects exactly 3 arguments".to_string());
        return Ty::Any;
    }
    let _c = infer_term(items[1], env, sess);
    let t1 = infer_term(items[2], env, sess);
    let t2 = infer_term(items[3], env, sess);
    join_types(t1, t2)
}

fn infer_begin(items: Vec<&Term>, env: &TypeEnv, sess: &mut InferSession) -> Ty {
    let mut last = Ty::Nil;
    for e in items.iter().skip(1) {
        last = infer_term(e, env, sess);
    }
    last
}

fn infer_let(items: Vec<&Term>, env: &TypeEnv, sess: &mut InferSession) -> Ty {
    if items.len() < 3 {
        sess.errors
            .push("(let (bindings) body...) expects at least 2 arguments".to_string());
        return Ty::Any;
    }
    let Some(binds) = items[1].as_proper_list() else {
        sess.errors.push(format!(
            "let bindings must be a list, got {}",
            print_term(items[1])
        ));
        return Ty::Any;
    };
    let mut env2 = env.clone();
    for b in binds {
        let Some(pair) = b.as_proper_list() else {
            continue;
        };
        if pair.len() != 2 {
            continue;
        }
        let Term::Symbol(name) = pair[0] else {
            continue;
        };
        let ty = infer_term(pair[1], &env2, sess);
        env2.set(name.clone(), ty);
    }
    let mut last = Ty::Nil;
    for e in items.iter().skip(2) {
        last = infer_term(e, &env2, sess);
    }
    last
}

fn infer_prim(items: Vec<&Term>, env: &TypeEnv, sess: &mut InferSession) -> Ty {
    if items.len() < 2 {
        sess.errors.push("prim missing op symbol".to_string());
        return Ty::Any;
    }
    let Term::Symbol(op) = items[1] else {
        sess.errors.push(format!(
            "prim op must be a symbol, got {}",
            print_term(items[1])
        ));
        return Ty::Any;
    };
    let arg_terms: Vec<&Term> = items.iter().skip(2).copied().collect();
    let args: Vec<Ty> = arg_terms.iter().map(|a| infer_term(a, env, sess)).collect();
    prim_type(op.as_str(), &args, &arg_terms, sess)
}

fn prim_type(op: &str, args: &[Ty], arg_terms: &[&Term], sess: &mut InferSession) -> Ty {
    match op {
        "int/add" | "int/sub" | "int/mul" => {
            if args.len() != 2 {
                sess.errors
                    .push(format!("prim {op} expects 2 args, got {}", args.len()));
                return Ty::Any;
            }
            if args[0] != Ty::Int || args[1] != Ty::Int {
                sess.errors.push(format!("prim {op} expects Int, Int"));
                return Ty::Any;
            }
            Ty::Int
        }
        "int/eq?" | "int/lt?" => {
            if args.len() != 2 {
                sess.errors
                    .push(format!("prim {op} expects 2 args, got {}", args.len()));
                return Ty::Any;
            }
            if args[0] != Ty::Int || args[1] != Ty::Int {
                sess.errors.push(format!("prim {op} expects Int, Int"));
                return Ty::Any;
            }
            Ty::Bool
        }
        "core/eq?" | "sym/eq?" => Ty::Bool,
        "str/concat" => Ty::Str,
        "bytes/len" => Ty::Int,
        "bytes/concat" => Ty::Bytes,
        "pair/cons" | "pair/car" | "pair/cdr" | "list/is-nil?" => Ty::Any,
        "map/get" => {
            if args.len() != 2 {
                sess.errors
                    .push(format!("prim {op} expects 2 args, got {}", args.len()));
                return Ty::Any;
            }
            match &args[0] {
                Ty::Rec { fields, tail } => {
                    let Some(key) = literal_map_key(arg_terms[1]) else {
                        return Ty::Any;
                    };
                    if let Some(found) = fields.get(&key) {
                        return found.clone();
                    }
                    if !tail.is_open() {
                        sess.warnings
                            .push(format!("prim map/get missing closed-row key {key}"));
                    }
                    Ty::Any
                }
                Ty::Any => Ty::Any,
                _ => {
                    sess.errors
                        .push("prim map/get expects Rec, key".to_string());
                    Ty::Any
                }
            }
        }
        "map/put" => {
            if args.len() != 3 {
                sess.errors
                    .push(format!("prim {op} expects 3 args, got {}", args.len()));
                return Ty::Any;
            }
            match &args[0] {
                Ty::Rec { fields, tail } => {
                    let mut next_fields = fields.clone();
                    if let Some(key) = literal_map_key(arg_terms[1]) {
                        next_fields.insert(key, args[2].clone());
                        Ty::Rec {
                            fields: next_fields,
                            tail: tail.clone(),
                        }
                    } else {
                        Ty::Rec {
                            fields: next_fields,
                            tail: RowTail::Any,
                        }
                    }
                }
                Ty::Any => Ty::Any,
                _ => {
                    sess.errors
                        .push("prim map/put expects Rec, key, value".to_string());
                    Ty::Any
                }
            }
        }
        "map/merge" => {
            if args.len() != 2 {
                sess.errors
                    .push(format!("prim {op} expects 2 args, got {}", args.len()));
                return Ty::Any;
            }
            match (&args[0], &args[1]) {
                (
                    Ty::Rec {
                        fields: lf,
                        tail: lt,
                    },
                    Ty::Rec {
                        fields: rf,
                        tail: rt,
                    },
                ) => {
                    let mut fields = lf.clone();
                    for (k, v) in rf {
                        fields.insert(k.clone(), v.clone());
                    }
                    let tail = if matches!(lt, RowTail::Closed) && matches!(rt, RowTail::Closed) {
                        RowTail::Closed
                    } else {
                        RowTail::Any
                    };
                    Ty::Rec { fields, tail }
                }
                (Ty::Any, _) | (_, Ty::Any) => Ty::Any,
                _ => {
                    sess.errors
                        .push("prim map/merge expects Rec, Rec".to_string());
                    Ty::Any
                }
            }
        }
        "vec/get" | "vec/push" => Ty::Any,
        _ => Ty::Any,
    }
}

fn infer_app(head: &Term, args: &[Term], env: &TypeEnv, sess: &mut InferSession) -> Ty {
    if let Term::Symbol(h) = head {
        match h.as_str() {
            "core/msg::make" => return infer_core_msg_make(args, env, sess),
            "core/msg::op" => return Ty::Symbol,
            "core/msg::payload" => return infer_core_msg_payload(args, env, sess),
            "core/contract::make" => {
                return Ty::Contract {
                    methods: BTreeMap::new(),
                    tail: RowTail::Any,
                };
            }
            "core/contract::extend" => return infer_core_contract_extend(args, env, sess),
            "core/contract::dispatch" => return infer_core_contract_dispatch(args, env, sess),
            "core/effect::pure" => return infer_core_effect_pure(args, env, sess),
            "core/effect::bind" => return infer_core_effect_bind(args, env, sess),
            "core/effect::perform" => return infer_core_effect_perform(args, env, sess),
            _ => {}
        }
    }

    // Fallback typed application: preserve precision for let-bound/curried function values.
    let head_ty = infer_term(head, env, sess);
    let arg_tys: Vec<Ty> = args.iter().map(|a| infer_term(a, env, sess)).collect();
    if let Some(applied) = infer_apply_types(head_ty, &arg_tys, sess) {
        return applied;
    }

    // Unknown application: children were traversed above; stay gradual.
    Ty::Any
}

fn infer_core_msg_make(args: &[Term], env: &TypeEnv, sess: &mut InferSession) -> Ty {
    if args.len() != 2 {
        sess.errors.push(format!(
            "core/msg::make expects 2 args (op, payload), got {}",
            args.len()
        ));
        return Ty::Any;
    }
    let op = literal_op_symbol(&args[0]);
    let payload = infer_term(&args[1], env, sess);
    Ty::Msg {
        op,
        payload: Box::new(payload),
    }
}

fn infer_core_msg_payload(args: &[Term], env: &TypeEnv, sess: &mut InferSession) -> Ty {
    if args.len() != 1 {
        sess.errors.push(format!(
            "core/msg::payload expects 1 arg, got {}",
            args.len()
        ));
        return Ty::Any;
    }
    let t = infer_term(&args[0], env, sess);
    match t {
        Ty::Msg { payload, .. } => *payload,
        Ty::Any => Ty::Any,
        _ => {
            sess.errors
                .push("core/msg::payload expects a Msg".to_string());
            Ty::Any
        }
    }
}

fn infer_core_contract_extend(args: &[Term], env: &TypeEnv, sess: &mut InferSession) -> Ty {
    if args.len() != 3 {
        sess.errors.push(format!(
            "core/contract::extend expects 3 args (base overrides meta), got {}",
            args.len()
        ));
        return Ty::Any;
    }
    let base = infer_term(&args[0], env, sess);
    let (mut methods, mut tail) = match base {
        Ty::Contract { methods, tail } => (methods, tail),
        Ty::Any => (BTreeMap::new(), RowTail::Any),
        _ => {
            sess.errors
                .push("core/contract::extend base must be a Contract".to_string());
            (BTreeMap::new(), RowTail::Any)
        }
    };

    // Overrides must be a map literal to refine method types; otherwise keep open.
    match &args[1] {
        Term::Map(m) => {
            for (k, v) in m {
                let Term::Symbol(op) = &k.0 else {
                    tail = RowTail::Any;
                    continue;
                };
                let mt = infer_contract_method(op, v, env, sess);
                methods.insert(op.clone(), mt);
            }
        }
        _ => {
            tail = RowTail::Any;
            let _ = infer_term(&args[1], env, sess);
        }
    }
    let _ = infer_term(&args[2], env, sess);
    Ty::Contract { methods, tail }
}

fn infer_contract_method(op: &str, v: &Term, env: &TypeEnv, sess: &mut InferSession) -> Ty {
    let Some(items) = v.as_proper_list() else {
        sess.warnings.push(format!(
            "contract method {op} is not a function literal; treating as ?"
        ));
        let _ = infer_term(v, env, sess);
        return Ty::Any;
    };
    if items.len() < 3 || !matches!(items[0], Term::Symbol(s) if s == "fn") {
        sess.warnings.push(format!(
            "contract method {op} is not a (fn ...) form; treating as ?"
        ));
        let _ = infer_term(v, env, sess);
        return Ty::Any;
    }
    let Some(params) = items[1].as_proper_list() else {
        sess.warnings.push(format!(
            "contract method {op} has invalid param list; treating as ?"
        ));
        return Ty::Any;
    };
    let param_name = params.first().and_then(|p| match p {
        Term::Symbol(s) => Some(s.clone()),
        _ => None,
    });
    let mut env2 = env.clone();
    if let Some(pn) = param_name {
        env2.set(
            pn,
            Ty::Msg {
                op: Some(op.to_string()),
                payload: Box::new(Ty::Any),
            },
        );
    }
    let body_ty = if items.len() == 3 {
        infer_term(items[2], &env2, sess)
    } else {
        let mut xs = Vec::new();
        xs.push(Term::Symbol("begin".to_string()));
        for b in items.iter().skip(2) {
            xs.push((*b).clone());
        }
        infer_term(&Term::list(xs), &env2, sess)
    };
    let eff = {
        // Only treat effects in the handler body as latent effects; not in quoted data.
        let inf = if items.len() == 3 {
            crate::infer_effects_in_term(items[2])
        } else {
            let mut xs = Vec::new();
            xs.push(Term::Symbol("begin".to_string()));
            for b in items.iter().skip(2) {
                xs.push((*b).clone());
            }
            crate::infer_effects_in_term(&Term::list(xs))
        };
        let tail = if inf.unknown {
            RowTail::Any
        } else {
            RowTail::Closed
        };
        EffRow { ops: inf.ops, tail }
    };
    Ty::Fn {
        param: Box::new(Ty::Msg {
            op: Some(op.to_string()),
            payload: Box::new(Ty::Any),
        }),
        ret: Box::new(body_ty),
        eff,
    }
}

fn infer_core_contract_dispatch(args: &[Term], env: &TypeEnv, sess: &mut InferSession) -> Ty {
    if args.len() != 2 {
        sess.errors.push(format!(
            "core/contract::dispatch expects 2 args (contract msg), got {}",
            args.len()
        ));
        return Ty::Any;
    }
    let c = infer_term(&args[0], env, sess);
    let m = infer_term(&args[1], env, sess);
    let (methods, tail) = match c {
        Ty::Contract { methods, tail } => (methods, tail),
        Ty::Any => return Ty::Any,
        _ => {
            sess.errors
                .push("core/contract::dispatch contract must be a Contract".to_string());
            return Ty::Any;
        }
    };
    let Ty::Msg { op, .. } = m else {
        sess.errors
            .push("core/contract::dispatch msg must be a Msg".to_string());
        return Ty::Any;
    };
    let Some(op) = op else {
        sess.warnings
            .push("core/contract::dispatch msg op is not literal; return type is ?".to_string());
        return Ty::Any;
    };
    let Some(mt) = methods.get(&op) else {
        if tail.is_open() {
            sess.warnings.push(format!(
                "dispatch on op {op} against open contract row; return type is ?"
            ));
            return Ty::Any;
        }
        sess.errors.push(format!(
            "dispatch on op {op} against closed contract with no such method"
        ));
        return Ty::Any;
    };
    match mt {
        Ty::Fn { ret, .. } => *ret.clone(),
        Ty::Any => Ty::Any,
        _ => Ty::Any,
    }
}

fn infer_core_effect_pure(args: &[Term], env: &TypeEnv, sess: &mut InferSession) -> Ty {
    if args.len() != 1 {
        sess.errors.push(format!(
            "core/effect::pure expects 1 arg, got {}",
            args.len()
        ));
        return Ty::Any;
    }
    let ret = infer_term(&args[0], env, sess);
    Ty::Prog {
        ret: Box::new(ret),
        eff: EffRow::empty(),
    }
}

fn infer_core_effect_bind(args: &[Term], env: &TypeEnv, sess: &mut InferSession) -> Ty {
    if args.len() != 2 {
        sess.errors.push(format!(
            "core/effect::bind expects 2 args (prog k), got {}",
            args.len()
        ));
        return Ty::Any;
    }
    let p_ty = infer_term(&args[0], env, sess);

    let (p_ret, mut p_eff) = match p_ty {
        Ty::Prog { ret, eff } => (ret, eff),
        Ty::Any => {
            return Ty::Prog {
                ret: Box::new(Ty::Any),
                eff: EffRow {
                    ops: BTreeSet::new(),
                    tail: RowTail::Any,
                },
            };
        }
        _ => {
            sess.errors
                .push("core/effect::bind first arg must be a Prog".to_string());
            return Ty::Any;
        }
    };

    let k_ty = infer_bind_continuation_with_param(&args[1], &p_ret, env, sess);
    let (k_param, k_ret, k_fn_eff) = match k_ty {
        Ty::Fn { param, ret, eff } => (param, ret, eff),
        Ty::Any => {
            return Ty::Prog {
                ret: Box::new(Ty::Any),
                eff: EffRow {
                    ops: BTreeSet::new(),
                    tail: RowTail::Any,
                },
            };
        }
        _ => {
            sess.errors
                .push("core/effect::bind continuation must be a function".to_string());
            return Ty::Any;
        }
    };

    if !arg_type_compatible(&p_ret, &k_param) {
        sess.errors.push(format!(
            "core/effect::bind continuation param mismatch; prog returns {}, continuation expects {}",
            print_term(&p_ret.to_term()),
            print_term(&k_param.to_term())
        ));
    }

    let (ret, k_prog_eff) = match *k_ret {
        Ty::Prog { ret, eff } => (ret, eff),
        Ty::Any => (
            Box::new(Ty::Any),
            EffRow {
                ops: BTreeSet::new(),
                tail: RowTail::Any,
            },
        ),
        other => {
            sess.errors.push(format!(
                "core/effect::bind continuation must return Prog, got {}",
                print_term(&other.to_term())
            ));
            (
                Box::new(Ty::Any),
                EffRow {
                    ops: BTreeSet::new(),
                    tail: RowTail::Any,
                },
            )
        }
    };

    p_eff = merge_eff_rows(p_eff, &k_fn_eff);
    p_eff = merge_eff_rows(p_eff, &k_prog_eff);
    Ty::Prog { ret, eff: p_eff }
}

fn infer_bind_continuation_with_param(
    continuation: &Term,
    param_ty: &Ty,
    env: &TypeEnv,
    sess: &mut InferSession,
) -> Ty {
    let Some(items) = continuation.as_proper_list() else {
        return infer_term(continuation, env, sess);
    };
    if items.len() < 3 || !matches!(items[0], Term::Symbol(s) if s == "fn") {
        return infer_term(continuation, env, sess);
    }
    let Some(params) = items[1].as_proper_list() else {
        return infer_term(continuation, env, sess);
    };
    if params.len() != 1 {
        return infer_term(continuation, env, sess);
    }
    let Term::Symbol(param_name) = params[0] else {
        return infer_term(continuation, env, sess);
    };
    let body = if items.len() == 3 {
        items[2].clone()
    } else {
        let mut xs = Vec::new();
        xs.push(Term::Symbol("begin".to_string()));
        for b in items.iter().skip(2) {
            xs.push((*b).clone());
        }
        Term::list(xs)
    };
    let mut env2 = env.clone();
    env2.set(param_name.clone(), param_ty.clone());
    let ret = infer_term(&body, &env2, sess);
    let inf = crate::infer_effects_in_term(&body);
    let tail = if inf.unknown {
        RowTail::Any
    } else {
        RowTail::Closed
    };
    Ty::Fn {
        param: Box::new(param_ty.clone()),
        ret: Box::new(ret),
        eff: EffRow { ops: inf.ops, tail },
    }
}

fn infer_core_effect_perform(args: &[Term], env: &TypeEnv, sess: &mut InferSession) -> Ty {
    if args.len() != 3 {
        sess.errors.push(format!(
            "core/effect::perform expects 3 args (op payload k), got {}",
            args.len()
        ));
        return Ty::Any;
    }
    let op = literal_op_symbol(&args[0]);
    let _payload = infer_term(&args[1], env, sess);
    let k_ty = infer_term(&args[2], env, sess);
    let (k_ret, mut eff) = match k_ty {
        Ty::Fn { ret, eff, .. } => (ret, eff),
        Ty::Any => (
            Box::new(Ty::Any),
            EffRow {
                ops: BTreeSet::new(),
                tail: RowTail::Any,
            },
        ),
        _ => {
            sess.errors
                .push("core/effect::perform k must be a function".to_string());
            (
                Box::new(Ty::Any),
                EffRow {
                    ops: BTreeSet::new(),
                    tail: RowTail::Any,
                },
            )
        }
    };

    // The continuation must return a program; the performed op extends that program's effect row.
    let (ret, k_eff) = match *k_ret {
        Ty::Prog { ret, eff: keff } => (ret, keff),
        Ty::Any => (
            Box::new(Ty::Any),
            EffRow {
                ops: BTreeSet::new(),
                tail: RowTail::Any,
            },
        ),
        other => {
            sess.errors.push(format!(
                "core/effect::perform continuation must return a Prog, got {}",
                print_term(&other.to_term())
            ));
            (
                Box::new(Ty::Any),
                EffRow {
                    ops: BTreeSet::new(),
                    tail: RowTail::Any,
                },
            )
        }
    };

    // Merge effects: the performed op plus downstream effects from the returned program.
    eff.ops.extend(k_eff.ops);
    if k_eff.tail.is_open() {
        eff.tail = k_eff.tail;
    }

    if let Some(op) = op {
        eff.ops.insert(op);
    } else {
        eff.tail = RowTail::Any;
    }

    Ty::Prog { ret, eff }
}

fn infer_apply_types(head: Ty, args: &[Ty], sess: &mut InferSession) -> Option<Ty> {
    let mut cur = head;
    for arg in args {
        cur = apply_once(cur, arg, sess)?;
    }
    Some(cur)
}

fn apply_once(f_ty: Ty, arg_ty: &Ty, sess: &mut InferSession) -> Option<Ty> {
    match f_ty {
        Ty::Fn { param, ret, eff: _ } => {
            if !arg_type_compatible(arg_ty, &param) {
                sess.errors.push(format!(
                    "application arg type mismatch: expected {}, got {}",
                    print_term(&param.to_term()),
                    print_term(&arg_ty.to_term())
                ));
                return Some(Ty::Any);
            }
            Some(*ret)
        }
        Ty::Any => Some(Ty::Any),
        _ => None,
    }
}

fn arg_type_compatible(inferred: &Ty, declared: &Ty) -> bool {
    if matches!(declared, Ty::Any) || matches!(inferred, Ty::Any) {
        return true;
    }
    match (inferred, declared) {
        (Ty::Int, Ty::Int)
        | (Ty::Bool, Ty::Bool)
        | (Ty::Nil, Ty::Nil)
        | (Ty::Str, Ty::Str)
        | (Ty::Bytes, Ty::Bytes)
        | (Ty::Symbol, Ty::Symbol) => true,
        (
            Ty::Msg {
                op: iop,
                payload: ip,
            },
            Ty::Msg {
                op: dop,
                payload: dp,
            },
        ) => {
            if let Some(d) = dop
                && iop.as_deref() != Some(d.as_str())
            {
                return false;
            }
            arg_type_compatible(ip, dp)
        }
        (
            Ty::Fn {
                param: ip,
                ret: ir,
                eff: _,
            },
            Ty::Fn {
                param: dp,
                ret: dr,
                eff: _,
            },
        ) => arg_type_compatible(ip, dp) && arg_type_compatible(ir, dr),
        (Ty::Prog { ret: ir, eff: _ }, Ty::Prog { ret: dr, eff: _ }) => arg_type_compatible(ir, dr),
        (
            Ty::Rec {
                fields: ifs,
                tail: _,
            },
            Ty::Rec {
                fields: dfs,
                tail: _,
            },
        ) => dfs
            .iter()
            .all(|(k, dt)| ifs.get(k).is_some_and(|it| arg_type_compatible(it, dt))),
        (
            Ty::Contract {
                methods: ims,
                tail: _,
            },
            Ty::Contract {
                methods: dms,
                tail: _,
            },
        ) => dms
            .iter()
            .all(|(k, dt)| ims.get(k).is_some_and(|it| arg_type_compatible(it, dt))),
        _ => false,
    }
}

fn merge_eff_rows(mut left: EffRow, right: &EffRow) -> EffRow {
    left.ops.extend(right.ops.iter().cloned());
    if right.tail.is_open() {
        left.tail = right.tail.clone();
    }
    left
}

fn join_types(a: Ty, b: Ty) -> Ty {
    if a == b {
        return a;
    }
    if matches!(a, Ty::Any) || matches!(b, Ty::Any) {
        return Ty::Any;
    }
    Ty::Any
}

fn flatten_app(t: &Term) -> Option<(Term, Vec<Term>)> {
    let items = t.as_proper_list()?;
    if items.len() == 2 {
        let f = items[0].clone();
        let x = items[1].clone();
        if let Some((head, mut args)) = flatten_app(&f) {
            args.push(x);
            return Some((head, args));
        }
        return Some((f, vec![x]));
    }
    if !items.is_empty() {
        let head = items[0].clone();
        let args = items.into_iter().skip(1).cloned().collect();
        return Some((head, args));
    }
    None
}

fn literal_op_symbol(t: &Term) -> Option<String> {
    let items = t.as_proper_list()?;
    if items.len() == 2
        && matches!(items[0], Term::Symbol(s) if s == "quote")
        && let Term::Symbol(s) = items[1]
    {
        return Some(s.clone());
    }
    None
}

fn literal_map_key(t: &Term) -> Option<String> {
    match t {
        Term::Symbol(s) => Some(s.clone()),
        Term::Str(s) => Some(s.clone()),
        _ => {
            let items = t.as_proper_list()?;
            if items.len() == 2 && matches!(items[0], Term::Symbol(s) if s == "quote") {
                match items[1] {
                    Term::Symbol(s) => Some(s.clone()),
                    Term::Str(s) => Some(s.clone()),
                    _ => None,
                }
            } else {
                None
            }
        }
    }
}
