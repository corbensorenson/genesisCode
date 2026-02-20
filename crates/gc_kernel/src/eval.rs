use std::collections::BTreeMap;
use std::collections::BTreeSet;

use bytes::{Bytes, BytesMut};

use crate::env::Env;
use crate::error::{KernelError, KernelErrorKind};
use crate::value::{Apply, SealId, Value};
use gc_coreform::{Term, TermOrdKey};
use num_traits::ToPrimitive;

#[path = "eval_decimal_ops.rs"]
mod eval_decimal_ops;
#[path = "eval_prims.rs"]
mod eval_prims;
#[path = "eval_value_ops.rs"]
mod eval_value_ops;

use eval_decimal_ops::{
    prim_dec_bin, prim_dec_cmp, prim_dec_from_int, prim_dec_parse, prim_dec_to_str,
};
pub(crate) use eval_prims::{prim, type_err};
use eval_value_ops::{eq_value, escape_bytes, escape_str};

/// Toolchain default evaluation step limit.
///
/// This is a DoS safety valve, not a semantic constraint. Tooling may allow
/// overriding or disabling it for trusted inputs.
pub const DEFAULT_STEP_LIMIT: u64 = 50_000_000;

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

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub struct MemObservedCounters {
    pub pair_cells: u64,
    pub max_vec_len: u64,
    pub max_map_len: u64,
    pub max_bytes_len: u64,
    pub max_string_len: u64,
}

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub struct EvalObservedCounters {
    pub steps: u64,
    pub mem: MemObservedCounters,
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

    pub(crate) fn mem_observe_data_term(&mut self, t: &Term) -> Result<(), KernelError> {
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

    pub fn observed_counters(&self) -> EvalObservedCounters {
        EvalObservedCounters {
            steps: self.steps,
            mem: MemObservedCounters {
                pair_cells: self.mem_state.pair_cells,
                max_vec_len: self.mem_state.max_vec_len,
                max_map_len: self.mem_state.max_map_len,
                max_bytes_len: self.mem_state.max_bytes_len,
                max_string_len: self.mem_state.max_string_len,
            },
        }
    }

    pub(crate) fn coverage_hit(&mut self, sym: &str) {
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

    pub(crate) fn mem_observe_vec_len(&mut self, len: usize) -> Result<(), KernelError> {
        Self::mem_observe_max(
            "vec-len",
            &mut self.mem_state.max_vec_len,
            len as u64,
            self.mem_limits.max_vec_len,
        )
    }

    pub(crate) fn mem_observe_map_len(&mut self, len: usize) -> Result<(), KernelError> {
        Self::mem_observe_max(
            "map-len",
            &mut self.mem_state.max_map_len,
            len as u64,
            self.mem_limits.max_map_len,
        )
    }

    pub(crate) fn mem_observe_bytes_len(&mut self, len: usize) -> Result<(), KernelError> {
        Self::mem_observe_max(
            "bytes-len",
            &mut self.mem_state.max_bytes_len,
            len as u64,
            self.mem_limits.max_bytes_len,
        )
    }

    pub(crate) fn mem_observe_string_len(&mut self, len: usize) -> Result<(), KernelError> {
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
        Value::Closure {
            param,
            body,
            env: fenv,
        } => Ok(EvalOutcome::Tail {
            env: Env::with_binding(&fenv, param, last_arg),
            term: body,
        }),
        other => Ok(EvalOutcome::Value(other.apply(ctx, last_arg)?)),
    }
}

fn eval_let_tco(
    ctx: &mut EvalCtx,
    env: &Env,
    items: Vec<&Term>,
) -> Result<EvalOutcome, KernelError> {
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

    Ok(EvalOutcome::Tail {
        env: env2,
        term: body_term,
    })
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
            return Err(KernelError::new(
                KernelErrorKind::Internal,
                "internal fn desugaring expected symbol parameter",
            ));
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
