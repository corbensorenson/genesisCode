use std::collections::{BTreeMap, BTreeSet};

use gc_coreform::{Term, TermOrdKey, print_term};

use crate::ty::{EffRow, Ty};

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
    pub fn with_prelude() -> Self {
        // Treat prelude-provided bindings as gradual/unknown unless they are core builtins
        // we special-case in `infer_app`.
        Self {
            vars: BTreeMap::new(),
        }
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
) -> (TypeEnv, BTreeMap<String, Ty>) {
    let mut env = TypeEnv::with_prelude();
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
    let mut open = false;
    for (k, v) in m {
        let key = match &k.0 {
            Term::Symbol(s) => Some(s.clone()),
            Term::Str(s) => Some(s.clone()),
            _ => None,
        };
        if let Some(lbl) = key {
            fields.insert(lbl, infer_term(v, env, sess));
        } else {
            open = true;
            // Still traverse for side knowledge (effects live in syntax).
            let _ = infer_term(v, env, sess);
        }
    }
    Ty::Rec { fields, open }
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
    Ty::Fn {
        param: Box::new(Ty::Any),
        ret: Box::new(ret),
        eff: EffRow::empty(),
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
    let args: Vec<Ty> = items
        .iter()
        .skip(2)
        .map(|a| infer_term(a, env, sess))
        .collect();
    prim_type(op.as_str(), &args, sess)
}

fn prim_type(op: &str, args: &[Ty], sess: &mut InferSession) -> Ty {
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
        "map/get" | "map/put" | "map/merge" => Ty::Any,
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
                    open: true,
                };
            }
            "core/contract::extend" => return infer_core_contract_extend(args, env, sess),
            "core/contract::dispatch" => return infer_core_contract_dispatch(args, env, sess),
            "core/effect::pure" => return infer_core_effect_pure(args, env, sess),
            "core/effect::perform" => return infer_core_effect_perform(args, env, sess),
            _ => {}
        }
    }

    // Unknown application: still traverse children for side constraints but be gradual.
    let _ = infer_term(head, env, sess);
    for a in args {
        let _ = infer_term(a, env, sess);
    }
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
    let (mut methods, mut open) = match base {
        Ty::Contract { methods, open } => (methods, open),
        Ty::Any => (BTreeMap::new(), true),
        _ => {
            sess.errors
                .push("core/contract::extend base must be a Contract".to_string());
            (BTreeMap::new(), true)
        }
    };

    // Overrides must be a map literal to refine method types; otherwise keep open.
    match &args[1] {
        Term::Map(m) => {
            for (k, v) in m {
                let Term::Symbol(op) = &k.0 else {
                    open = true;
                    continue;
                };
                let mt = infer_contract_method(op, v, env, sess);
                methods.insert(op.clone(), mt);
            }
        }
        _ => {
            open = true;
            let _ = infer_term(&args[1], env, sess);
        }
    }
    let _ = infer_term(&args[2], env, sess);
    Ty::Contract { methods, open }
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
    Ty::Fn {
        param: Box::new(Ty::Msg {
            op: Some(op.to_string()),
            payload: Box::new(Ty::Any),
        }),
        ret: Box::new(body_ty),
        eff: EffRow::empty(),
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
    let (methods, open) = match c {
        Ty::Contract { methods, open } => (methods, open),
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
        if open {
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
    let (ret, mut eff) = match k_ty {
        Ty::Fn { ret, eff, .. } => (ret, eff),
        Ty::Any => (
            Box::new(Ty::Any),
            EffRow {
                ops: BTreeSet::new(),
                open: true,
            },
        ),
        _ => {
            sess.errors
                .push("core/effect::perform k must be a function".to_string());
            (
                Box::new(Ty::Any),
                EffRow {
                    ops: BTreeSet::new(),
                    open: true,
                },
            )
        }
    };
    if let Some(op) = op {
        eff.ops.insert(op);
    } else {
        eff.open = true;
    }
    Ty::Prog { ret, eff }
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
