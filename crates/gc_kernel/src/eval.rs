use std::collections::BTreeMap;
use std::collections::BTreeSet;

use crate::env::Env;
use crate::error::{KernelError, KernelErrorKind};
use crate::value::{Apply, SealId, Value};
use gc_coreform::{Term, TermOrdKey};

/// Toolchain default evaluation step limit.
///
/// This is a DoS safety valve, not a semantic constraint. Tooling may allow
/// overriding or disabling it for trusted inputs.
pub const DEFAULT_STEP_LIMIT: u64 = 5_000_000;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum StepLimit {
    /// Use the toolchain default (`DEFAULT_STEP_LIMIT`).
    Default,
    /// Disable the step limit.
    Unlimited,
    /// Use an explicit limit.
    Limit(u64),
}

impl StepLimit {
    pub fn resolve(self) -> Option<u64> {
        match self {
            StepLimit::Default => Some(DEFAULT_STEP_LIMIT),
            StepLimit::Unlimited => None,
            StepLimit::Limit(n) => Some(n),
        }
    }
}

#[derive(Clone, Copy, Debug)]
pub struct EvalState {
    pub next_seal_id: u64,
}

impl EvalState {
    pub fn new() -> Self {
        Self { next_seal_id: 1 }
    }
}

impl Default for EvalState {
    fn default() -> Self {
        Self::new()
    }
}

#[derive(Clone, Copy, Debug)]
pub struct ProtocolTokens {
    pub unhandled: SealId,
    pub effect: SealId,
    pub error: SealId,
}

#[derive(Debug)]
pub struct EvalCtx {
    pub state: EvalState,
    pub protocol: Option<ProtocolTokens>,
    pub steps: u64,
    pub step_limit: Option<u64>,
    coverage: Option<CoverageState>,
}

#[derive(Debug, Clone)]
struct CoverageState {
    tracked: BTreeSet<String>,
    hits: BTreeMap<String, u64>,
}

impl EvalCtx {
    pub fn new() -> Self {
        Self::with_step_limit(Some(DEFAULT_STEP_LIMIT))
    }

    pub fn with_step_limit(step_limit: Option<u64>) -> Self {
        // Reserve protocol seal tokens at runtime init so:
        // - kernel primitives can always return sealed ERROR values
        // - protocol constructors can be installed without relying on a separate init step
        // - user-visible (seal) IDs are deterministic (start after the reserved tokens)
        let mut state = EvalState::new();
        let unhandled = SealId(state.next_seal_id);
        state.next_seal_id = state.next_seal_id.saturating_add(1);
        let effect = SealId(state.next_seal_id);
        state.next_seal_id = state.next_seal_id.saturating_add(1);
        let error = SealId(state.next_seal_id);
        state.next_seal_id = state.next_seal_id.saturating_add(1);

        let protocol = ProtocolTokens {
            unhandled,
            effect,
            error,
        };

        Self {
            state,
            protocol: Some(protocol),
            steps: 0,
            step_limit,
            coverage: None,
        }
    }

    pub fn enable_coverage(&mut self, tracked: BTreeSet<String>) {
        self.coverage = Some(CoverageState {
            tracked,
            hits: BTreeMap::new(),
        });
    }

    pub fn coverage_hits(&self) -> Option<&BTreeMap<String, u64>> {
        self.coverage.as_ref().map(|c| &c.hits)
    }

    fn coverage_hit(&mut self, sym: &str) {
        let Some(c) = &mut self.coverage else { return };
        if !c.tracked.contains(sym) {
            return;
        }
        *c.hits.entry(sym.to_string()).or_insert(0) += 1;
    }

    pub fn tick(&mut self) -> Result<(), KernelError> {
        self.steps = self.steps.saturating_add(1);
        if let Some(limit) = self.step_limit
            && self.steps > limit
        {
            return Err(KernelError::new(
                KernelErrorKind::StepLimit,
                "step limit exceeded",
            ));
        }
        Ok(())
    }
}

impl Default for EvalCtx {
    fn default() -> Self {
        Self::new()
    }
}

pub fn eval_module(ctx: &mut EvalCtx, env: &mut Env, forms: &[Term]) -> Result<Value, KernelError> {
    let mut last = Value::Data(Term::Nil);
    for form in forms {
        if let Some((name, expr)) = parse_def(form) {
            let v = eval_term(ctx, env, &expr)?;
            *env = Env::with_binding(env, name, v);
            last = Value::Data(Term::Nil);
            continue;
        }
        last = eval_term(ctx, env, form)?;
    }
    Ok(last)
}

fn parse_def(t: &Term) -> Option<(String, Term)> {
    let items = t.as_proper_list()?;
    if items.len() != 3 {
        return None;
    }
    if !matches!(items[0], Term::Symbol(s) if s == "def") {
        return None;
    }
    let Term::Symbol(name) = items[1] else {
        return None;
    };
    Some((name.clone(), items[2].clone()))
}

pub fn eval_term(ctx: &mut EvalCtx, env: &Env, term: &Term) -> Result<Value, KernelError> {
    // Evaluator is structurally recursive; grow stack as needed.
    stacker::maybe_grow(32 * 1024, 1024 * 1024, || eval_term_impl(ctx, env, term))
}

fn eval_term_impl(ctx: &mut EvalCtx, env: &Env, term: &Term) -> Result<Value, KernelError> {
    ctx.tick()?;

    match term {
        Term::Nil | Term::Bool(_) | Term::Int(_) | Term::Str(_) | Term::Bytes(_) => {
            Ok(Value::Data(term.clone()))
        }
        Term::Vector(xs) => Ok(Value::Vector(xs.iter().cloned().map(Value::Data).collect())),
        Term::Map(m) => {
            // Map literal: keys are data terms (not evaluated), values are expressions (evaluated).
            let mut out = std::collections::BTreeMap::new();
            for (k, v) in m.iter() {
                let vv = eval_term(ctx, env, v)?;
                out.insert(k.clone(), vv);
            }
            Ok(Value::Map(out))
        }
        Term::Symbol(s) => {
            ctx.coverage_hit(s);
            env.get(s).ok_or_else(|| {
                KernelError::new(KernelErrorKind::Unbound, format!("unbound symbol: {s}"))
            })
        }
        Term::Pair(_, _) => eval_list(ctx, env, term),
    }
}

fn eval_list(ctx: &mut EvalCtx, env: &Env, t: &Term) -> Result<Value, KernelError> {
    let Some(items) = t.as_proper_list() else {
        return Err(KernelError::new(
            KernelErrorKind::BadForm,
            "improper list is not a valid form",
        ));
    };
    if items.is_empty() {
        return Ok(Value::Data(Term::Nil));
    }

    // Special forms keyed by head symbol.
    if let Term::Symbol(h) = items[0] {
        match h.as_str() {
            "quote" => {
                if items.len() != 2 {
                    return Err(KernelError::new(
                        KernelErrorKind::BadForm,
                        "(quote datum) expects exactly 1 argument",
                    ));
                }
                return Ok(Value::Data(items[1].clone()));
            }
            "fn" => {
                return eval_fn(ctx, env, items);
            }
            "if" => {
                if items.len() != 4 {
                    return Err(KernelError::new(
                        KernelErrorKind::BadForm,
                        "(if c t e) expects exactly 3 arguments",
                    ));
                }
                let c = eval_term(ctx, env, items[1])?;
                if c.truthy() {
                    return eval_term(ctx, env, items[2]);
                }
                return eval_term(ctx, env, items[3]);
            }
            "begin" => {
                if items.len() < 2 {
                    return Err(KernelError::new(
                        KernelErrorKind::BadForm,
                        "(begin ...) expects at least 1 argument",
                    ));
                }
                let mut last = Value::Data(Term::Nil);
                for e in items.iter().skip(1) {
                    last = eval_term(ctx, env, e)?;
                }
                return Ok(last);
            }
            "let" => {
                return eval_let(ctx, env, items);
            }
            "prim" => {
                return eval_prim(ctx, env, items);
            }
            "seal" => {
                return eval_seal(ctx, env, items);
            }
            "unseal" => {
                return eval_unseal(ctx, env, items);
            }
            "def" => {
                return Err(KernelError::new(
                    KernelErrorKind::BadForm,
                    "(def ...) is only allowed at module top-level",
                ));
            }
            _ => {}
        }
    }

    // General application (supports sugar forms with more than one argument).
    let f = eval_term(ctx, env, items[0])?;
    let mut acc = f;
    for a in items.iter().skip(1) {
        let av = eval_term(ctx, env, a)?;
        acc = acc.apply(ctx, av)?;
    }
    Ok(acc)
}

fn eval_fn(_ctx: &mut EvalCtx, env: &Env, items: Vec<&Term>) -> Result<Value, KernelError> {
    if items.len() < 3 {
        return Err(KernelError::new(
            KernelErrorKind::BadForm,
            "(fn (x) body...) expects params and body",
        ));
    }
    let params = items[1];
    let Some(ps) = params.as_proper_list() else {
        return Err(KernelError::new(
            KernelErrorKind::BadForm,
            "(fn ...) params must be a list",
        ));
    };
    if ps.is_empty() {
        return Err(KernelError::new(
            KernelErrorKind::BadForm,
            "(fn ...) requires at least 1 parameter",
        ));
    }
    for p in &ps {
        if !matches!(p, Term::Symbol(_)) {
            return Err(KernelError::new(
                KernelErrorKind::BadForm,
                "(fn ...) params must be symbols",
            ));
        }
    }

    let body_term = if items.len() == 3 {
        items[2].clone()
    } else {
        // multi-body => (begin ...)
        let mut xs = Vec::with_capacity(items.len() - 1);
        xs.push(Term::Symbol("begin".to_string()));
        for b in items.iter().skip(2) {
            xs.push((*b).clone());
        }
        Term::list(xs)
    };

    // Desugar multi-arg lambda into nested unary closures.
    let mut out = body_term;
    for p in ps.into_iter().rev() {
        let Term::Symbol(name) = p else {
            unreachable!()
        };
        out = Term::list(vec![
            Term::Symbol("fn".to_string()),
            Term::list(vec![Term::Symbol(name.clone())]),
            out,
        ]);
    }

    // Now out is a unary fn; build closure from it.
    let Some(items2) = out.as_proper_list() else {
        return Err(KernelError::new(
            KernelErrorKind::Internal,
            "internal fn desugaring failed",
        ));
    };
    if items2.len() != 3 {
        return Err(KernelError::new(
            KernelErrorKind::Internal,
            "internal fn desugaring produced unexpected shape",
        ));
    }
    let params2 = &items2[1];
    let Some(ps2) = params2.as_proper_list() else {
        return Err(KernelError::new(
            KernelErrorKind::Internal,
            "internal fn desugaring produced bad params",
        ));
    };
    if ps2.len() != 1 {
        return Err(KernelError::new(
            KernelErrorKind::Internal,
            "internal fn desugaring produced non-unary params",
        ));
    }
    let Term::Symbol(param) = ps2[0] else {
        return Err(KernelError::new(
            KernelErrorKind::Internal,
            "internal fn desugaring produced non-symbol param",
        ));
    };
    Ok(Value::Closure {
        param: param.clone(),
        body: items2[2].clone(),
        env: env.clone(),
    })
}

fn eval_let(ctx: &mut EvalCtx, env: &Env, items: Vec<&Term>) -> Result<Value, KernelError> {
    if items.len() < 3 {
        return Err(KernelError::new(
            KernelErrorKind::BadForm,
            "(let ((x e) ...) body...) expects bindings and body",
        ));
    }
    let binds = items[1];
    let Some(bs) = binds.as_proper_list() else {
        return Err(KernelError::new(
            KernelErrorKind::BadForm,
            "(let ...) bindings must be a list",
        ));
    };

    let mut cur_env = env.clone();
    for b in bs {
        let Some(pair) = b.as_proper_list() else {
            return Err(KernelError::new(
                KernelErrorKind::BadForm,
                "(let ...) binding must be a list (name expr)",
            ));
        };
        if pair.len() != 2 {
            return Err(KernelError::new(
                KernelErrorKind::BadForm,
                "(let ...) binding must have exactly 2 forms",
            ));
        }
        let Term::Symbol(name) = pair[0] else {
            return Err(KernelError::new(
                KernelErrorKind::BadForm,
                "(let ...) binding name must be a symbol",
            ));
        };
        let v = eval_term(ctx, &cur_env, pair[1])?;
        cur_env = Env::with_binding(&cur_env, name.clone(), v);
    }

    let body_term = if items.len() == 3 {
        items[2].clone()
    } else {
        let mut xs = Vec::with_capacity(items.len() - 1);
        xs.push(Term::Symbol("begin".to_string()));
        for b in items.iter().skip(2) {
            xs.push((*b).clone());
        }
        Term::list(xs)
    };
    eval_term(ctx, &cur_env, &body_term)
}

fn eval_seal(ctx: &mut EvalCtx, env: &Env, items: Vec<&Term>) -> Result<Value, KernelError> {
    match items.len() {
        1 => {
            let id = ctx.state.next_seal_id;
            ctx.state.next_seal_id = ctx.state.next_seal_id.saturating_add(1);
            Ok(Value::SealToken(SealId(id)))
        }
        3 => {
            let v = eval_term(ctx, env, items[1])?;
            let tok = eval_term(ctx, env, items[2])?;
            let Value::SealToken(id) = tok else {
                return type_err(ctx, "seal expects a seal token as second argument");
            };
            Ok(Value::Sealed {
                token: id,
                payload: Box::new(v),
            })
        }
        _ => Err(KernelError::new(
            KernelErrorKind::BadForm,
            "(seal) or (seal v tok)",
        )),
    }
}

fn eval_unseal(ctx: &mut EvalCtx, env: &Env, items: Vec<&Term>) -> Result<Value, KernelError> {
    if items.len() != 3 {
        return Err(KernelError::new(
            KernelErrorKind::BadForm,
            "(unseal w tok) expects exactly 2 arguments",
        ));
    }
    let w = eval_term(ctx, env, items[1])?;
    let tok = eval_term(ctx, env, items[2])?;
    let Value::SealToken(id) = tok else {
        return type_err(ctx, "unseal expects a seal token as second argument");
    };
    if let Value::Sealed { token, payload } = w
        && token == id
    {
        return Ok(*payload);
    }
    Ok(Value::Data(Term::Nil))
}

fn eval_prim(ctx: &mut EvalCtx, env: &Env, items: Vec<&Term>) -> Result<Value, KernelError> {
    if items.len() < 2 {
        return Err(KernelError::new(
            KernelErrorKind::BadForm,
            "(prim op ...) expects at least an op",
        ));
    }
    let Term::Symbol(op) = items[1] else {
        return Err(KernelError::new(
            KernelErrorKind::BadForm,
            "(prim ...) op must be a symbol",
        ));
    };
    let mut args = Vec::with_capacity(items.len().saturating_sub(2));
    for a in items.iter().skip(2) {
        args.push(eval_term(ctx, env, a)?);
    }
    prim(ctx, op, args)
}

fn prim(ctx: &mut EvalCtx, op: &str, args: Vec<Value>) -> Result<Value, KernelError> {
    match op {
        "int/add" => prim_int_bin(ctx, &args, |a, b| a + b),
        "int/sub" => prim_int_bin(ctx, &args, |a, b| a - b),
        "int/mul" => prim_int_bin(ctx, &args, |a, b| a * b),
        "int/eq?" => prim_int_cmp(ctx, &args, |a, b| a == b),
        "int/lt?" => prim_int_cmp(ctx, &args, |a, b| a < b),
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
                    let mut out = m.clone();
                    out.insert(TermOrdKey(k.clone()), args[2].clone());
                    Ok(Value::Map(out))
                }
                Value::Data(Term::Map(m)) => {
                    let Some(v) = args[2].as_data() else {
                        return type_err(ctx, "map/put expects data value when map is data");
                    };
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
                    Ok(Value::Map(out))
                }
                (Value::Data(Term::Map(a)), Value::Data(Term::Map(b))) => {
                    let mut out = a.clone();
                    for (k, v) in b.iter() {
                        out.insert(k.clone(), v.clone());
                    }
                    Ok(Value::Data(Term::Map(out)))
                }
                _ => type_err(ctx, "map/merge expects maps of the same kind"),
            }
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
        "vec/push" => {
            if args.len() != 2 {
                return type_err(ctx, "vec/push expects 2 args");
            }
            match &args[0] {
                Value::Vector(xs) => {
                    let mut out = xs.clone();
                    out.push(args[1].clone());
                    Ok(Value::Vector(out))
                }
                Value::Data(Term::Vector(xs)) => {
                    let Some(v) = args[1].as_data() else {
                        return type_err(ctx, "vec/push expects data when vector is data");
                    };
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
            Ok(Value::Data(Term::Str(format!("{a}{b}"))))
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
            let mut out = a.clone();
            out.extend_from_slice(b);
            Ok(Value::Data(Term::Bytes(out)))
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

fn type_err(ctx: &mut EvalCtx, msg: &str) -> Result<Value, KernelError> {
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

fn eq_value(a: &Value, b: &Value) -> bool {
    match (a, b) {
        (Value::Data(x), Value::Data(y)) => x == y,
        (Value::Vector(x), Value::Vector(y)) => {
            x.len() == y.len() && x.iter().zip(y.iter()).all(|(a, b)| eq_value(a, b))
        }
        (Value::Map(x), Value::Map(y)) => {
            x.len() == y.len()
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

trait ToUsize {
    fn to_usize(&self) -> Option<usize>;
}

impl ToUsize for num_bigint::BigInt {
    fn to_usize(&self) -> Option<usize> {
        num_traits::ToPrimitive::to_usize(self)
    }
}
