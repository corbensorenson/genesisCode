use std::collections::BTreeMap;

use gc_coreform::{SpecialForm, Term, TermOrdKey, print_term};

use crate::ty::{EffRow, RowTail, Ty};

#[path = "infer_application.rs"]
mod infer_application;
#[path = "infer_effects.rs"]
mod infer_effects;
#[path = "infer_prim.rs"]
mod infer_prim;
#[path = "infer_typed_effects.rs"]
mod infer_typed_effects;

use infer_application::{arg_type_compatible, arg_type_match, infer_apply_types};
use infer_effects::{infer_core_effect_bind, infer_core_effect_perform, infer_core_effect_pure};
use infer_prim::prim_type;
pub use infer_typed_effects::{
    infer_effects_in_term_with_env, infer_effects_in_term_with_expected,
    infer_effects_in_terms_with_env, unknown_effect_signature_symbols_in_term,
};

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

    pub fn contains(&self, s: &str) -> bool {
        self.vars.contains_key(s)
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
            && matches!(items[0], Term::Symbol(s) if SpecialForm::from_symbol(s) == Some(SpecialForm::Def))
            && let Term::Symbol(name) = items[1]
        {
            let expected = env.get(name).cloned();
            let ty = infer_term_with_expected(items[2], &env, sess, expected.as_ref());
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

fn infer_term_with_expected(
    t: &Term,
    env: &TypeEnv,
    sess: &mut InferSession,
    expected: Option<&Ty>,
) -> Ty {
    let Some(items) = t.as_proper_list() else {
        return infer_term(t, env, sess);
    };
    if matches!(items.first(), Some(Term::Symbol(head)) if SpecialForm::from_symbol(head) == Some(SpecialForm::Fn))
    {
        return infer_fn(items, env, sess, expected);
    }
    infer_term(t, env, sess)
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

    if let Term::Symbol(head) = items[0]
        && let Some(form) = SpecialForm::from_symbol(head)
    {
        match form {
            SpecialForm::Quote => return Ty::Any,
            SpecialForm::Fn => return infer_fn(items, env, sess, None),
            SpecialForm::If => return infer_if(items, env, sess),
            SpecialForm::Begin => return infer_begin(items, env, sess),
            SpecialForm::Let => return infer_let(items, env, sess),
            SpecialForm::Prim => return infer_prim(items, env, sess),
            SpecialForm::Seal | SpecialForm::Unseal => {
                // Seals are intentionally opaque under gradual typing.
                for argument in items.iter().skip(1) {
                    let _ = infer_term(argument, env, sess);
                }
                return Ty::Any;
            }
            SpecialForm::Def => {
                // Module definitions are not expressions, but still traverse malformed use.
                for argument in items.iter().skip(1) {
                    let _ = infer_term(argument, env, sess);
                }
                return Ty::Any;
            }
        }
    }

    if let Some((head, args)) = flatten_app(t) {
        return infer_app(&head, &args, env, sess);
    }
    Ty::Any
}

fn infer_fn(
    items: Vec<&Term>,
    env: &TypeEnv,
    sess: &mut InferSession,
    expected: Option<&Ty>,
) -> Ty {
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
        return infer_term_with_expected(&cur, env, sess, expected);
    };

    let expected_param = match expected {
        Some(Ty::Fn { param, .. }) => Some(param.as_ref()),
        _ => None,
    };
    let mut env2 = env.clone();
    env2.set(names[0].clone(), expected_param.cloned().unwrap_or(Ty::Any));
    let ret = infer_term(&body, &env2, sess);
    let eff = {
        let inf = infer_effects_in_term_with_env(&body, &env2);
        let tail = if inf.unknown {
            RowTail::Any
        } else {
            RowTail::Closed
        };
        EffRow { ops: inf.ops, tail }
    };
    Ty::Fn {
        param: Box::new(expected_param.cloned().unwrap_or(Ty::Any)),
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
            infer_effects_in_term_with_env(items[2], &env2)
        } else {
            let mut xs = Vec::new();
            xs.push(Term::Symbol("begin".to_string()));
            for b in items.iter().skip(2) {
                xs.push((*b).clone());
            }
            infer_effects_in_term_with_env(&Term::list(xs), &env2)
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
