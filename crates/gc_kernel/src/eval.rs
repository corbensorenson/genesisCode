use std::collections::BTreeMap;
use std::collections::BTreeSet;

use bytes::{Bytes, BytesMut};

use crate::env::Env;
use crate::error::{KernelError, KernelErrorKind};
use crate::value::{SealId, Value};
use gc_coreform::{Term, TermOrdKey};
use num_traits::ToPrimitive;

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
    decision_total: u64,
    decision_true: u64,
    decision_false: u64,
    statement_site_hits: BTreeMap<String, u64>,
    decision_site_hits: BTreeMap<String, DecisionCoverageCounters>,
    decision_samples: BTreeMap<String, Vec<DecisionSample>>,
    decision_stack: Vec<DecisionFrame>,
}

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub struct DecisionCoverageCounters {
    pub total: u64,
    pub taken_true: u64,
    pub taken_false: u64,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct DecisionSample {
    pub conditions: BTreeMap<String, bool>,
    pub outcome: bool,
}

#[derive(Clone, Debug, Eq, PartialEq)]
struct DecisionFrame {
    site_id: String,
    conditions: BTreeMap<String, bool>,
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
            decision_total: 0,
            decision_true: 0,
            decision_false: 0,
            statement_site_hits: BTreeMap::new(),
            decision_site_hits: BTreeMap::new(),
            decision_samples: BTreeMap::new(),
            decision_stack: Vec::new(),
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

    pub fn coverage_decision_counts(&self) -> Option<DecisionCoverageCounters> {
        self.coverage.as_ref().map(|c| DecisionCoverageCounters {
            total: c.decision_total,
            taken_true: c.decision_true,
            taken_false: c.decision_false,
        })
    }

    pub fn coverage_statement_site_hits(&self) -> Option<&BTreeMap<String, u64>> {
        self.coverage.as_ref().map(|c| &c.statement_site_hits)
    }

    pub fn coverage_decision_site_hits(
        &self,
    ) -> Option<&BTreeMap<String, DecisionCoverageCounters>> {
        self.coverage.as_ref().map(|c| &c.decision_site_hits)
    }

    pub fn coverage_decision_samples(&self) -> Option<&BTreeMap<String, Vec<DecisionSample>>> {
        self.coverage.as_ref().map(|c| &c.decision_samples)
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

    pub(crate) fn coverage_hit(&mut self, sym: &str, value: &Value) {
        let Some(c) = &mut self.coverage else { return };
        if !c.tracked.contains(sym) {
            if let Some(frame) = c.decision_stack.last_mut()
                && let Value::Data(Term::Bool(b)) = value
            {
                frame.conditions.entry(sym.to_string()).or_insert(*b);
            }
            return;
        }
        *c.hits.entry(sym.to_string()).or_insert(0) += 1;
        if let Some(frame) = c.decision_stack.last_mut()
            && let Value::Data(Term::Bool(b)) = value
        {
            frame.conditions.entry(sym.to_string()).or_insert(*b);
        }
    }

    pub(crate) fn coverage_statement_site(&mut self, site_id: &str) {
        let Some(c) = &mut self.coverage else { return };
        *c.statement_site_hits
            .entry(site_id.to_string())
            .or_insert(0) += 1;
    }

    pub(crate) fn coverage_decision(&mut self, truthy: bool) {
        let Some(c) = &mut self.coverage else { return };
        c.decision_total = c.decision_total.saturating_add(1);
        if truthy {
            c.decision_true = c.decision_true.saturating_add(1);
        } else {
            c.decision_false = c.decision_false.saturating_add(1);
        }
    }

    pub(crate) fn coverage_begin_decision_site(&mut self, site_id: &str) {
        let Some(c) = &mut self.coverage else { return };
        c.decision_stack.push(DecisionFrame {
            site_id: site_id.to_string(),
            conditions: BTreeMap::new(),
        });
    }

    pub(crate) fn coverage_abort_decision_site(&mut self) {
        let Some(c) = &mut self.coverage else { return };
        let _ = c.decision_stack.pop();
    }

    pub(crate) fn coverage_finish_decision_site(&mut self, truthy: bool) {
        self.coverage_decision(truthy);
        let Some(c) = &mut self.coverage else { return };
        let Some(frame) = c.decision_stack.pop() else {
            return;
        };
        let site_counts = c
            .decision_site_hits
            .entry(frame.site_id.clone())
            .or_default();
        site_counts.total = site_counts.total.saturating_add(1);
        if truthy {
            site_counts.taken_true = site_counts.taken_true.saturating_add(1);
        } else {
            site_counts.taken_false = site_counts.taken_false.saturating_add(1);
        }
        c.decision_samples
            .entry(frame.site_id)
            .or_default()
            .push(DecisionSample {
                conditions: frame.conditions,
                outcome: truthy,
            });
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

pub(super) enum EvalOutcome {
    Value(Value),
    Tail { env: Env, term: Term },
}

pub fn eval_module(ctx: &mut EvalCtx, env: &mut Env, forms: &[Term]) -> Result<Value, KernelError> {
    eval_forms::eval_module(ctx, env, forms)
}

pub fn eval_term(ctx: &mut EvalCtx, env: &Env, term: &Term) -> Result<Value, KernelError> {
    // Evaluator is structurally recursive; grow stack as needed.
    stacker::maybe_grow(32 * 1024, 1024 * 1024, || {
        eval_treewalk::eval_term_impl(ctx, env, term)
    })
}
