use bytes::Bytes;

use crate::env::Env;
use crate::error::{KernelError, KernelErrorKind};
use crate::value::{SealId, Value};
use gc_coreform::{Term, TermOrdKey};
use num_traits::ToPrimitive;
use std::sync::Arc;

#[cfg(test)]
use std::cell::Cell;

#[path = "eval_coverage.rs"]
mod eval_coverage;
#[path = "eval_decimal_ops.rs"]
mod eval_decimal_ops;
#[path = "eval_forms.rs"]
mod eval_forms;
#[path = "eval_prims.rs"]
mod eval_prims;
#[path = "eval_treewalk.rs"]
mod eval_treewalk;
#[path = "eval_value_ops.rs"]
mod eval_value_ops;

pub(crate) use eval_coverage::CoverageRunId;
use eval_coverage::CoverageState;
pub use eval_coverage::{DecisionCoverageCounters, DecisionSample};
use eval_decimal_ops::{
    prim_dec_bin, prim_dec_cmp, prim_dec_from_int, prim_dec_parse, prim_dec_to_str,
};
pub(crate) use eval_prims::{PrimOp, prim, prim_op, prim_op2, type_err};
use eval_value_ops::{eq_value, escape_bytes, escape_str};

#[cfg(test)]
thread_local! {
    static EVALUATOR_CALL_DEPTH: Cell<u32> = const { Cell::new(0) };
    static EVALUATOR_MAX_CALL_DEPTH: Cell<u32> = const { Cell::new(0) };
}

#[cfg(test)]
pub(crate) struct EvaluatorDepthGuard;

#[cfg(test)]
impl EvaluatorDepthGuard {
    pub(crate) fn enter() -> Self {
        EVALUATOR_CALL_DEPTH.with(|depth| {
            let next = depth.get().saturating_add(1);
            depth.set(next);
            EVALUATOR_MAX_CALL_DEPTH.with(|maximum| maximum.set(maximum.get().max(next)));
        });
        Self
    }
}

#[cfg(test)]
impl Drop for EvaluatorDepthGuard {
    fn drop(&mut self) {
        EVALUATOR_CALL_DEPTH.with(|depth| depth.set(depth.get().saturating_sub(1)));
    }
}

#[cfg(test)]
pub(crate) fn reset_evaluator_max_call_depth() {
    EVALUATOR_CALL_DEPTH.with(|depth| assert_eq!(depth.get(), 0));
    EVALUATOR_MAX_CALL_DEPTH.with(|maximum| maximum.set(0));
}

#[cfg(test)]
pub(crate) fn evaluator_max_call_depth() -> u32 {
    EVALUATOR_MAX_CALL_DEPTH.with(Cell::get)
}

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
    /// Maximum cumulative logical allocation units in one evaluation session.
    pub max_alloc_units: Option<u64>,
    /// Maximum logical units reachable from declared evaluator roots at a safe point.
    pub max_live_units: Option<u64>,
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
    pub(crate) mem_limits: MemLimits,
    mem_state: MemState,
    allocation_ledger: Arc<crate::logical_heap::AllocationLedger>,
    coverage: Option<CoverageState>,
    panic_guard_depth: u32,
}

#[derive(Clone, Copy, Debug, Default)]
struct MemState {
    live_units: u64,
    max_live_units: u64,
    pair_cells: u64,
    max_vec_len: u64,
    max_map_len: u64,
    max_bytes_len: u64,
    max_string_len: u64,
}

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub struct MemObservedCounters {
    pub allocated_units: u64,
    pub live_units: u64,
    pub max_live_units: u64,
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
            allocation_ledger: Arc::new(crate::logical_heap::AllocationLedger::new()),
            coverage: None,
            panic_guard_depth: 0,
        }
    }

    pub(crate) fn panic_guard_active(&self) -> bool {
        self.panic_guard_depth > 0
    }

    pub(crate) fn run_panic_guarded<T>(
        &mut self,
        boundary: &'static str,
        f: impl FnOnce(&mut Self) -> Result<T, KernelError>,
    ) -> Result<T, KernelError> {
        if self.panic_guard_active() {
            return f(self);
        }
        self.run_panic_guarded_always(boundary, f)
    }

    pub(crate) fn run_panic_guarded_always<T>(
        &mut self,
        boundary: &'static str,
        f: impl FnOnce(&mut Self) -> Result<T, KernelError>,
    ) -> Result<T, KernelError> {
        let outermost = !self.panic_guard_active();
        self.panic_guard_depth = self.panic_guard_depth.saturating_add(1);
        let _allocation_guard = self.mem_limits.max_alloc_units.map(|_| {
            crate::logical_heap::ActiveAllocationGuard::enter(self.allocation_ledger.clone())
        });
        let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| f(self)));
        self.panic_guard_depth = self.panic_guard_depth.saturating_sub(1);
        if outermost
            && std::panic::catch_unwind(std::panic::AssertUnwindSafe(crate::cycle::collect_cycles))
                .is_err()
        {
            return Err(KernelError::new(
                KernelErrorKind::Internal,
                format!("{boundary} cycle collection panicked"),
            ));
        }
        if outermost && let Some(limit) = self.allocation_ledger.limit() {
            let observed = self.allocation_ledger.observed();
            if observed > limit {
                return Err(KernelError::memory_limit(
                    "allocation-units",
                    observed,
                    limit,
                ));
            }
        }
        match result {
            Ok(result) => result,
            Err(_) => Err(KernelError::new(
                KernelErrorKind::Internal,
                format!("{boundary} panicked"),
            )),
        }
    }

    pub fn set_mem_limits(&mut self, limits: MemLimits) {
        self.mem_limits = limits;
        self.allocation_ledger.set_limit(limits.max_alloc_units);
    }

    pub fn mem_limits(&self) -> MemLimits {
        self.mem_limits
    }

    fn mem_shape_enabled(&self) -> bool {
        self.mem_limits.max_pair_cells.is_some()
            || self.mem_limits.max_vec_len.is_some()
            || self.mem_limits.max_map_len.is_some()
            || self.mem_limits.max_bytes_len.is_some()
            || self.mem_limits.max_string_len.is_some()
    }

    pub(crate) fn mem_observe_data_term(&mut self, t: &Term) -> Result<(), KernelError> {
        if !self.mem_shape_enabled() {
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

    /// Reset counters that are used for budgeting (steps + semantic memory observations).
    ///
    /// This is intended for trusted toolchain initialization paths (prelude/selfhost toolchain),
    /// so user program budgets measure user code rather than bootstrap overhead.
    pub fn reset_counters(&mut self) {
        self.steps = 0;
        self.mem_state = MemState::default();
        self.allocation_ledger.reset();
    }

    pub fn observed_counters(&self) -> EvalObservedCounters {
        EvalObservedCounters {
            steps: self.steps,
            mem: MemObservedCounters {
                allocated_units: self.allocation_ledger.observed(),
                live_units: self.mem_state.live_units,
                max_live_units: self.mem_state.max_live_units,
                pair_cells: self.mem_state.pair_cells,
                max_vec_len: self.mem_state.max_vec_len,
                max_map_len: self.mem_state.max_map_len,
                max_bytes_len: self.mem_state.max_bytes_len,
                max_string_len: self.mem_state.max_string_len,
            },
        }
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
            return Err(KernelError::memory_limit(kind, *slot, max));
        }
        Ok(())
    }

    fn mem_charge_pair_cells(&mut self, n: u64) -> Result<(), KernelError> {
        self.mem_state.pair_cells = self.mem_state.pair_cells.saturating_add(n);
        if let Some(max) = self.mem_limits.max_pair_cells
            && self.mem_state.pair_cells > max
        {
            return Err(KernelError::memory_limit(
                "pair-cells",
                self.mem_state.pair_cells,
                max,
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

    pub(crate) fn mem_map_len_limit(&self) -> Option<u64> {
        self.mem_limits.max_map_len
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

    pub(crate) fn mem_observe_live_roots(
        &mut self,
        values: &[&Value],
        environments: &[&Env],
    ) -> Result<(), KernelError> {
        let Some(limit) = self.mem_limits.max_live_units else {
            return Ok(());
        };
        let observed = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
            crate::logical_heap::logical_live_units(values, environments)
        }))
        .map_err(|_| {
            KernelError::new(
                KernelErrorKind::Internal,
                "logical live-heap traversal panicked",
            )
        })?;
        self.mem_state.live_units = observed;
        self.mem_state.max_live_units = self.mem_state.max_live_units.max(observed);
        if observed > limit {
            return Err(KernelError::memory_limit("live-units", observed, limit));
        }
        Ok(())
    }

    pub(crate) fn finish_with_live_roots(
        &mut self,
        result: Value,
        environments: &[&Env],
    ) -> Result<Value, KernelError> {
        if !self.panic_guard_active() {
            self.mem_observe_live_roots(&[&result], environments)?;
        }
        Ok(result)
    }

    /// Convert a structured resource-limit error into the reserved, unforgeable ERROR protocol.
    pub fn seal_resource_error(&self, error: &KernelError) -> Option<Value> {
        let resource = error.resource_limit.as_ref()?;
        let token = self.protocol?.error;
        let context = Term::Map(
            [
                (
                    TermOrdKey(Term::symbol(":dimension")),
                    Term::Str(resource.dimension.to_string()),
                ),
                (
                    TermOrdKey(Term::symbol(":observed")),
                    Term::Int(resource.observed.into()),
                ),
                (
                    TermOrdKey(Term::symbol(":limit")),
                    Term::Int(resource.limit.into()),
                ),
            ]
            .into_iter()
            .collect(),
        );
        let payload = Term::Map(
            [
                (
                    TermOrdKey(Term::symbol(":error/code")),
                    Term::Str("core/resource-exhausted".to_string()),
                ),
                (
                    TermOrdKey(Term::symbol(":error/message")),
                    Term::Str(error.msg.clone()),
                ),
                (TermOrdKey(Term::symbol(":error/context")), context),
            ]
            .into_iter()
            .collect(),
        );
        Some(Value::sealed(token, Value::data(payload)))
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

pub(super) enum EvalOutcome {
    Value(Value),
    Tail { env: Env, term: Term },
}

pub fn eval_module(ctx: &mut EvalCtx, env: &mut Env, forms: &[Term]) -> Result<Value, KernelError> {
    let result = ctx.run_panic_guarded("eval_module", |ctx| {
        eval_forms::eval_module(ctx, env, forms)
    })?;
    ctx.finish_with_live_roots(result, &[env])
}

pub fn eval_term(ctx: &mut EvalCtx, env: &Env, term: &Term) -> Result<Value, KernelError> {
    #[cfg(test)]
    let _depth_guard = EvaluatorDepthGuard::enter();
    let result = ctx.run_panic_guarded("eval_term", |ctx| {
        // Evaluator is structurally recursive; grow stack as needed.
        stacker::maybe_grow(32 * 1024, 1024 * 1024, || {
            eval_treewalk::eval_term_impl(ctx, env, term)
        })
    })?;
    ctx.finish_with_live_roots(result, &[env])
}
