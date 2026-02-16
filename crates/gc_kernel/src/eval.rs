use std::collections::BTreeMap;
use std::collections::BTreeSet;

use bytes::{Bytes, BytesMut};

use crate::env::Env;
use crate::error::{KernelError, KernelErrorKind};
use crate::value::{Apply, SealId, Value};
use gc_coreform::{Term, TermOrdKey};
use num_traits::ToPrimitive;

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

/// Optional, deterministic memory safety valves for the kernel.
///
/// These limits are *not* an exact accounting of process RSS; they are stable, semantic measures
/// based on observed sizes of CoreForm values during evaluation.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub struct MemLimits {
    /// Maximum total number of `pair/cons` cells allocated during evaluation.
    pub max_pair_cells: Option<u64>,
    /// Maximum observed vector length (applies to vector literals and `vec/push`).
    pub max_vec_len: Option<u64>,
    /// Maximum observed map length (applies to map literals, `map/put`, and `map/merge`).
    pub max_map_len: Option<u64>,
    /// Maximum observed bytes length (applies to bytes literals and `bytes/concat`).
    pub max_bytes_len: Option<u64>,
    /// Maximum observed string length in UTF-8 bytes (applies to string literals and `str/concat`).
    pub max_string_len: Option<u64>,
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
    pub mem_limits: MemLimits,
    mem_state: MemState,
    coverage: Option<CoverageState>,
}

#[derive(Clone, Copy, Debug, Default)]
struct MemState {
    pair_cells: u64,
    max_vec_len: u64,
    max_map_len: u64,
    max_bytes_len: u64,
    max_string_len: u64,
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
            mem_limits: MemLimits::default(),
            mem_state: MemState::default(),
            coverage: None,
        }
    }

    pub fn set_mem_limits(&mut self, limits: MemLimits) {
        self.mem_limits = limits;
    }

    fn mem_enabled(&self) -> bool {
        self.mem_limits.max_pair_cells.is_some()
            || self.mem_limits.max_vec_len.is_some()
            || self.mem_limits.max_map_len.is_some()
            || self.mem_limits.max_bytes_len.is_some()
            || self.mem_limits.max_string_len.is_some()
    }

    fn mem_observe_data_term(&mut self, t: &Term) -> Result<(), KernelError> {
        if !self.mem_enabled() {
            return Ok(());
        }
        // Data terms can be deeply nested; avoid stack overflow while observing.
        stacker::maybe_grow(32 * 1024, 1024 * 1024, || {
            let mut stack: Vec<&Term> = vec![t];
            while let Some(cur) = stack.pop() {
                match cur {
                    Term::Str(s) => self.mem_observe_string_len(s.len())?,
                    Term::Bytes(b) => self.mem_observe_bytes_len(b.len())?,
                    Term::Vector(xs) => {
                        self.mem_observe_vec_len(xs.len())?;
                        for x in xs {
                            stack.push(x);
                        }
                    }
                    Term::Map(m) => {
                        self.mem_observe_map_len(m.len())?;
                        for (k, v) in m.iter() {
                            stack.push(&k.0);
                            stack.push(v);
                        }
                    }
                    Term::Pair(a, d) => {
                        stack.push(a);
                        stack.push(d);
                    }
                    Term::Nil | Term::Bool(_) | Term::Int(_) | Term::Symbol(_) => {}
                }
            }
            Ok(())
        })
    }

    pub fn enable_coverage(&mut self, tracked: BTreeSet<String>) {
        self.coverage = Some(CoverageState {
            tracked,
            hits: BTreeMap::new(),
        });
    }

    /// Reset counters that are used for budgeting (steps + semantic memory observations).
    ///
    /// This is intended for trusted toolchain initialization paths (prelude/selfhost toolchain),
    /// so user program budgets measure user code rather than bootstrap overhead.
    pub fn reset_counters(&mut self) {
        self.steps = 0;
        self.mem_state = MemState::default();
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

    fn mem_observe_max(
        kind: &'static str,
        slot: &mut u64,
        observed: u64,
        limit: Option<u64>,
    ) -> Result<(), KernelError> {
        if observed > *slot {
            *slot = observed;
        }
        if let Some(max) = limit
            && *slot > max
        {
            return Err(KernelError::new(
                KernelErrorKind::MemoryLimit,
                format!(
                    "memory limit exceeded: {kind} (observed={}, limit={max})",
                    *slot
                ),
            ));
        }
        Ok(())
    }

    fn mem_charge_pair_cells(&mut self, n: u64) -> Result<(), KernelError> {
        self.mem_state.pair_cells = self.mem_state.pair_cells.saturating_add(n);
        if let Some(max) = self.mem_limits.max_pair_cells
            && self.mem_state.pair_cells > max
        {
            return Err(KernelError::new(
                KernelErrorKind::MemoryLimit,
                format!(
                    "memory limit exceeded: pair-cells (observed={}, limit={max})",
                    self.mem_state.pair_cells
                ),
            ));
        }
        Ok(())
    }

    fn mem_observe_vec_len(&mut self, len: usize) -> Result<(), KernelError> {
        Self::mem_observe_max(
            "vec-len",
            &mut self.mem_state.max_vec_len,
            len as u64,
            self.mem_limits.max_vec_len,
        )
    }

    fn mem_observe_map_len(&mut self, len: usize) -> Result<(), KernelError> {
        Self::mem_observe_max(
            "map-len",
            &mut self.mem_state.max_map_len,
            len as u64,
            self.mem_limits.max_map_len,
        )
    }

    fn mem_observe_bytes_len(&mut self, len: usize) -> Result<(), KernelError> {
        Self::mem_observe_max(
            "bytes-len",
            &mut self.mem_state.max_bytes_len,
            len as u64,
            self.mem_limits.max_bytes_len,
        )
    }

    fn mem_observe_string_len(&mut self, len: usize) -> Result<(), KernelError> {
        Self::mem_observe_max(
            "string-len",
            &mut self.mem_state.max_string_len,
            len as u64,
            self.mem_limits.max_string_len,
        )
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
            env.set_local(name, v);
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

enum EvalOutcome {
    Value(Value),
    Tail { env: Env, term: Term },
}

fn eval_term_impl(ctx: &mut EvalCtx, env: &Env, term: &Term) -> Result<Value, KernelError> {
    // Implement a small tail-call optimization for:
    // - (if ...) branches
    // - (begin ...) last form
    // - general application where the final apply is a closure call
    //
    // This makes typical tail-recursive CoreForm code stack-safe without changing semantics.
    let mut cur_env = env.clone();
    let mut cur_term = term.clone();
    loop {
        ctx.tick()?;

        match &cur_term {
            Term::Nil | Term::Bool(_) | Term::Int(_) => return Ok(Value::Data(cur_term.clone())),
            Term::Str(s) => {
                ctx.mem_observe_string_len(s.len())?;
                return Ok(Value::Data(cur_term.clone()));
            }
            Term::Bytes(b) => {
                ctx.mem_observe_bytes_len(b.len())?;
                return Ok(Value::Data(cur_term.clone()));
            }
            Term::Vector(xs) => {
                ctx.mem_observe_vec_len(xs.len())?;
                for x in xs {
                    ctx.mem_observe_data_term(x)?;
                }
                return Ok(Value::Vector(xs.iter().cloned().map(Value::Data).collect()));
            }
            Term::Map(m) => {
                // Map literal: keys are data terms (not evaluated), values are expressions (evaluated).
                ctx.mem_observe_map_len(m.len())?;
                for (k, _v) in m.iter() {
                    ctx.mem_observe_data_term(&k.0)?;
                }
                let mut out = std::collections::BTreeMap::new();
                for (k, v) in m.iter() {
                    let vv = eval_term(ctx, &cur_env, v)?;
                    out.insert(k.clone(), vv);
                }
                return Ok(Value::Map(out));
            }
            Term::Symbol(s) => {
                ctx.coverage_hit(s);
                return cur_env.get(s).ok_or_else(|| {
                    KernelError::new(KernelErrorKind::Unbound, format!("unbound symbol: {s}"))
                });
            }
            Term::Pair(_, _) => match eval_list_tco(ctx, &cur_env, &cur_term)? {
                EvalOutcome::Value(v) => return Ok(v),
                EvalOutcome::Tail { env, term } => {
                    cur_env = env;
                    cur_term = term;
                }
            },
        }
    }
}

fn eval_list_tco(ctx: &mut EvalCtx, env: &Env, t: &Term) -> Result<EvalOutcome, KernelError> {
    let Some(items) = t.as_proper_list() else {
        return Err(KernelError::new(
            KernelErrorKind::BadForm,
            "improper list is not a valid form",
        ));
    };
    if items.is_empty() {
        return Ok(EvalOutcome::Value(Value::Data(Term::Nil)));
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
                ctx.mem_observe_data_term(items[1])?;
                return Ok(EvalOutcome::Value(Value::Data(items[1].clone())));
            }
            "fn" => {
                return Ok(EvalOutcome::Value(eval_fn(ctx, env, items)?));
            }
            "if" => {
                if items.len() != 4 {
                    return Err(KernelError::new(
                        KernelErrorKind::BadForm,
                        "(if c t e) expects exactly 3 arguments",
                    ));
                }
                let c = eval_term(ctx, env, items[1])?;
                let next = if c.truthy() { items[2] } else { items[3] };
                return Ok(EvalOutcome::Tail {
                    env: env.clone(),
                    term: next.clone(),
                });
            }
            "begin" => {
                if items.len() < 2 {
                    return Err(KernelError::new(
                        KernelErrorKind::BadForm,
                        "(begin ...) expects at least 1 argument",
                    ));
                }
                if items.len() == 2 {
                    return Ok(EvalOutcome::Tail {
                        env: env.clone(),
                        term: items[1].clone(),
                    });
                }
                for e in items.iter().skip(1).take(items.len() - 2) {
                    let _ = eval_term(ctx, env, e)?;
                }
                return Ok(EvalOutcome::Tail {
                    env: env.clone(),
                    term: items[items.len() - 1].clone(),
                });
            }
            "let" => {
                return eval_let_tco(ctx, env, items);
            }
            "prim" => {
                return Ok(EvalOutcome::Value(eval_prim(ctx, env, items)?));
            }
            "seal" => {
                return Ok(EvalOutcome::Value(eval_seal(ctx, env, items)?));
            }
            "unseal" => {
                return Ok(EvalOutcome::Value(eval_unseal(ctx, env, items)?));
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
    if items.len() == 1 {
        return Ok(EvalOutcome::Value(f));
    }

    // Apply all but the final argument normally.
    let mut acc = f;
    for a in items.iter().skip(1).take(items.len() - 2) {
        let av = eval_term(ctx, env, a)?;
        acc = acc.apply(ctx, av)?;
    }

    // Tail-call optimize the final apply when it is a closure call.
    let last_arg = eval_term(ctx, env, items[items.len() - 1])?;
    match acc {
        Value::Closure { param, body, env: fenv } => Ok(EvalOutcome::Tail {
            env: Env::with_binding(&fenv, param, last_arg),
            term: body,
        }),
        other => Ok(EvalOutcome::Value(other.apply(ctx, last_arg)?)),
    }
}

fn eval_let_tco(ctx: &mut EvalCtx, env: &Env, items: Vec<&Term>) -> Result<EvalOutcome, KernelError> {
    if items.len() < 3 {
        return Err(KernelError::new(
            KernelErrorKind::BadForm,
            "(let ((x e) ...) body...) expects bindings and body",
        ));
    }
    let bindings = items[1];
    let Some(bs) = bindings.as_proper_list() else {
        return Err(KernelError::new(
            KernelErrorKind::BadForm,
            "(let ...) bindings must be a list",
        ));
    };

    let mut env2 = env.clone();
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
                "(let ...) binding name must be symbol",
            ));
        };
        let rhs = eval_term(ctx, &env2, pair[1])?;
        env2 = Env::with_binding(&env2, name.clone(), rhs);
    }

    // Body: single => that term; multi => (begin ...)
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

    Ok(EvalOutcome::Tail { env: env2, term: body_term })
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
            let idx: usize = match ToUsize::to_usize(i) {
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
            let idx: usize = match ToUsize::to_usize(i) {
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
            let n: usize = match ToUsize::to_usize(n) {
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
            let idx: usize = match ToUsize::to_usize(i) {
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
            let start: usize = match ToUsize::to_usize(start_i) {
                Some(x) => x,
                None => return type_err(ctx, "bytes/slice start out of range"),
            };
            let len: usize = match ToUsize::to_usize(len_i) {
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
            Ok(Value::Data(Term::Bytes(Bytes::copy_from_slice(s.as_bytes()))))
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

fn escape_str(s: &str) -> String {
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

fn escape_bytes(b: &[u8]) -> String {
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
