use std::collections::{BTreeMap, BTreeSet};

use crate::error::{KernelError, KernelErrorKind};
use crate::value::Value;
use gc_coreform::Term;

use super::EvalCtx;

#[derive(Debug, Clone)]
pub(super) struct CoverageState {
    tracked: BTreeSet<String>,
    hits: BTreeMap<String, u64>,
    decision_total: u64,
    decision_true: u64,
    decision_false: u64,
    statement_site_hits: BTreeMap<String, u64>,
    decision_site_hits: BTreeMap<String, DecisionCoverageCounters>,
    decision_samples: BTreeMap<String, Vec<DecisionSample>>,
    decision_stack: Vec<DecisionFrame>,
    indexed_runs: Vec<Option<IndexedCoverageRun>>,
}

impl CoverageState {
    fn new(tracked: BTreeSet<String>) -> Self {
        Self {
            tracked,
            hits: BTreeMap::new(),
            decision_total: 0,
            decision_true: 0,
            decision_false: 0,
            statement_site_hits: BTreeMap::new(),
            decision_site_hits: BTreeMap::new(),
            decision_samples: BTreeMap::new(),
            decision_stack: Vec::new(),
            indexed_runs: Vec::new(),
        }
    }
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

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) struct CoverageRunId(usize);

#[derive(Debug, Clone)]
struct IndexedCoverageRun {
    statement_site_hits: Vec<u64>,
    decision_site_hits: Vec<DecisionCoverageCounters>,
    decision_samples: Vec<Vec<DecisionSample>>,
}

#[derive(Clone, Debug, Eq, PartialEq)]
enum DecisionFrame {
    Named {
        site_id: String,
        conditions: BTreeMap<String, bool>,
    },
    Indexed {
        run_id: CoverageRunId,
        site_index: usize,
        conditions: BTreeMap<String, bool>,
    },
}

impl DecisionFrame {
    fn conditions_mut(&mut self) -> &mut BTreeMap<String, bool> {
        match self {
            DecisionFrame::Named { conditions, .. } => conditions,
            DecisionFrame::Indexed { conditions, .. } => conditions,
        }
    }
}

fn indexed_coverage_run_mut(
    c: &mut CoverageState,
    run_id: CoverageRunId,
) -> Result<&mut IndexedCoverageRun, KernelError> {
    let Some(run) = c.indexed_runs.get_mut(run_id.0).and_then(Option::as_mut) else {
        return Err(KernelError::new(
            KernelErrorKind::Internal,
            "compiled coverage run is not active",
        ));
    };
    Ok(run)
}

impl EvalCtx {
    pub fn enable_coverage(&mut self, tracked: BTreeSet<String>) {
        self.coverage = Some(CoverageState::new(tracked));
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

    pub(crate) fn coverage_enabled(&self) -> bool {
        self.coverage.is_some()
    }

    pub(crate) fn coverage_hit(&mut self, sym: &str, value: &Value) {
        let Some(c) = &mut self.coverage else { return };
        if !c.tracked.contains(sym) {
            if let Some(frame) = c.decision_stack.last_mut()
                && let Some(Term::Bool(b)) = value.as_data()
            {
                frame.conditions_mut().entry(sym.to_string()).or_insert(*b);
            }
            return;
        }
        *c.hits.entry(sym.to_string()).or_insert(0) += 1;
        if let Some(frame) = c.decision_stack.last_mut()
            && let Some(Term::Bool(b)) = value.as_data()
        {
            frame.conditions_mut().entry(sym.to_string()).or_insert(*b);
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
        c.decision_stack.push(DecisionFrame::Named {
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
        let DecisionFrame::Named {
            site_id,
            conditions,
        } = frame
        else {
            return;
        };
        let site_counts = c.decision_site_hits.entry(site_id.clone()).or_default();
        site_counts.total = site_counts.total.saturating_add(1);
        if truthy {
            site_counts.taken_true = site_counts.taken_true.saturating_add(1);
        } else {
            site_counts.taken_false = site_counts.taken_false.saturating_add(1);
        }
        c.decision_samples
            .entry(site_id)
            .or_default()
            .push(DecisionSample {
                conditions,
                outcome: truthy,
            });
    }

    pub(crate) fn coverage_begin_indexed_run(
        &mut self,
        statement_site_count: usize,
        decision_site_count: usize,
    ) -> Option<CoverageRunId> {
        let Some(c) = &mut self.coverage else {
            return None;
        };
        let run_id = CoverageRunId(c.indexed_runs.len());
        c.indexed_runs.push(Some(IndexedCoverageRun {
            statement_site_hits: vec![0; statement_site_count],
            decision_site_hits: vec![DecisionCoverageCounters::default(); decision_site_count],
            decision_samples: vec![Vec::new(); decision_site_count],
        }));
        Some(run_id)
    }

    pub(crate) fn coverage_statement_site_index(
        &mut self,
        run_id: CoverageRunId,
        site_index: u32,
    ) -> Result<(), KernelError> {
        let Some(c) = &mut self.coverage else {
            return Ok(());
        };
        let site_index = usize::try_from(site_index).map_err(|_| {
            KernelError::new(
                KernelErrorKind::Internal,
                "compiled coverage statement site index exceeds usize range",
            )
        })?;
        let run = indexed_coverage_run_mut(c, run_id)?;
        let Some(slot) = run.statement_site_hits.get_mut(site_index) else {
            return Err(KernelError::new(
                KernelErrorKind::Internal,
                format!("compiled coverage statement site index out of range: {site_index}"),
            ));
        };
        *slot = slot.saturating_add(1);
        Ok(())
    }

    pub(crate) fn coverage_begin_decision_site_index(
        &mut self,
        run_id: CoverageRunId,
        site_index: u32,
    ) -> Result<(), KernelError> {
        let Some(c) = &mut self.coverage else {
            return Ok(());
        };
        let site_index = usize::try_from(site_index).map_err(|_| {
            KernelError::new(
                KernelErrorKind::Internal,
                "compiled coverage decision site index exceeds usize range",
            )
        })?;
        let run = indexed_coverage_run_mut(c, run_id)?;
        if site_index >= run.decision_site_hits.len() {
            return Err(KernelError::new(
                KernelErrorKind::Internal,
                format!("compiled coverage decision site index out of range: {site_index}"),
            ));
        }
        c.decision_stack.push(DecisionFrame::Indexed {
            run_id,
            site_index,
            conditions: BTreeMap::new(),
        });
        Ok(())
    }

    pub(crate) fn coverage_finish_decision_site_index(
        &mut self,
        expected_run_id: CoverageRunId,
        truthy: bool,
    ) -> Result<(), KernelError> {
        self.coverage_decision(truthy);
        let Some(c) = &mut self.coverage else {
            return Ok(());
        };
        let Some(frame) = c.decision_stack.pop() else {
            return Ok(());
        };
        let DecisionFrame::Indexed {
            run_id,
            site_index,
            conditions,
        } = frame
        else {
            return Err(KernelError::new(
                KernelErrorKind::Internal,
                "compiled coverage finished a named decision frame",
            ));
        };
        if run_id != expected_run_id {
            return Err(KernelError::new(
                KernelErrorKind::Internal,
                "compiled coverage decision run mismatch",
            ));
        }
        let run = indexed_coverage_run_mut(c, run_id)?;
        let Some(site_counts) = run.decision_site_hits.get_mut(site_index) else {
            return Err(KernelError::new(
                KernelErrorKind::Internal,
                format!("compiled coverage decision site index out of range: {site_index}"),
            ));
        };
        site_counts.total = site_counts.total.saturating_add(1);
        if truthy {
            site_counts.taken_true = site_counts.taken_true.saturating_add(1);
        } else {
            site_counts.taken_false = site_counts.taken_false.saturating_add(1);
        }
        let Some(samples) = run.decision_samples.get_mut(site_index) else {
            return Err(KernelError::new(
                KernelErrorKind::Internal,
                format!("compiled coverage decision sample index out of range: {site_index}"),
            ));
        };
        samples.push(DecisionSample {
            conditions,
            outcome: truthy,
        });
        Ok(())
    }

    pub(crate) fn coverage_flush_indexed_run(
        &mut self,
        run_id: CoverageRunId,
        statement_sites: &[String],
        decision_sites: &[String],
    ) -> Result<(), KernelError> {
        let Some(c) = &mut self.coverage else {
            return Ok(());
        };
        let Some(slot) = c.indexed_runs.get_mut(run_id.0) else {
            return Err(KernelError::new(
                KernelErrorKind::Internal,
                "compiled coverage run id out of range",
            ));
        };
        let Some(run) = slot.take() else {
            return Err(KernelError::new(
                KernelErrorKind::Internal,
                "compiled coverage run already flushed",
            ));
        };
        if run.statement_site_hits.len() != statement_sites.len()
            || run.decision_site_hits.len() != decision_sites.len()
            || run.decision_samples.len() != decision_sites.len()
        {
            return Err(KernelError::new(
                KernelErrorKind::Internal,
                "compiled coverage run/table length mismatch",
            ));
        }
        for (site_id, hits) in statement_sites.iter().zip(run.statement_site_hits) {
            if hits != 0 {
                *c.statement_site_hits.entry(site_id.clone()).or_insert(0) += hits;
            }
        }
        for ((site_id, hits), samples) in decision_sites
            .iter()
            .zip(run.decision_site_hits)
            .zip(run.decision_samples)
        {
            if hits.total != 0 {
                let site_counts = c.decision_site_hits.entry(site_id.clone()).or_default();
                site_counts.total = site_counts.total.saturating_add(hits.total);
                site_counts.taken_true = site_counts.taken_true.saturating_add(hits.taken_true);
                site_counts.taken_false = site_counts.taken_false.saturating_add(hits.taken_false);
            }
            if !samples.is_empty() {
                c.decision_samples
                    .entry(site_id.clone())
                    .or_default()
                    .extend(samples);
            }
        }
        Ok(())
    }
}
