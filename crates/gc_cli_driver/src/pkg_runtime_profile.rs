use std::path::Path;
use std::path::PathBuf;
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::Instant;

use gc_coreform::{Term, TermOrdKey, hash_term};
use gc_effects::{ArtifactStore, CapsPolicy};
use gc_kernel::{EvalCtx, EvalObservedCounters};
use gc_prelude::build_prelude;

use super::kernel_exec::{eval_module_default_value, parse_canonicalize_hash_module_source};
use super::pkg_workspace_ops::LocalPkgResult;

#[derive(Clone, Copy, Debug)]
pub(crate) struct RuntimeProfileBudgets {
    pub task_budget_us: u64,
    pub io_budget_us: u64,
    pub memory_budget_us: u64,
}

#[derive(Clone, Copy, Debug, serde::Deserialize, serde::Serialize)]
struct RuntimeProfileHistoryEntry {
    task_elapsed_us: u64,
    io_elapsed_us: u64,
    memory_elapsed_us: u64,
}

#[derive(Clone, Copy, Debug)]
struct RuntimeProbeTrace {
    elapsed_us: u64,
    observed: EvalObservedCounters,
    effect_entries: u64,
}

#[derive(Clone, Copy, Debug)]
struct IoProbeTrace {
    elapsed_us: u64,
    bytes: u64,
}

#[derive(Clone, Debug)]
struct RuntimeProfileResult {
    task: RuntimeProbeTrace,
    io: IoProbeTrace,
    memory: RuntimeProbeTrace,
}

#[derive(Clone, Debug)]
struct RuntimeRegressionSummary {
    history_samples: usize,
    min_history: usize,
    max_regression_percent: u64,
    applied: bool,
    task_p95_us: Option<u64>,
    io_p95_us: Option<u64>,
    memory_p95_us: Option<u64>,
    task_ok: bool,
    io_ok: bool,
    memory_ok: bool,
}

pub(crate) fn handle_runtime_profile(
    out: &Path,
    history: &Path,
    min_history: usize,
    max_regression_percent: u64,
    append_history: bool,
    budgets: RuntimeProfileBudgets,
) -> Result<LocalPkgResult, String> {
    let result = run_runtime_profile_probes()?;
    let regression = evaluate_runtime_regression(
        history,
        min_history,
        max_regression_percent,
        RuntimeProfileHistoryEntry {
            task_elapsed_us: result.task.elapsed_us,
            io_elapsed_us: result.io.elapsed_us,
            memory_elapsed_us: result.memory.elapsed_us,
        },
    )?;

    if append_history {
        append_runtime_history(
            history,
            RuntimeProfileHistoryEntry {
                task_elapsed_us: result.task.elapsed_us,
                io_elapsed_us: result.io.elapsed_us,
                memory_elapsed_us: result.memory.elapsed_us,
            },
        )?;
    }

    let task_budget_ok = result.task.elapsed_us <= budgets.task_budget_us;
    let io_budget_ok = result.io.elapsed_us <= budgets.io_budget_us;
    let memory_budget_ok = result.memory.elapsed_us <= budgets.memory_budget_us;
    let ok = task_budget_ok
        && io_budget_ok
        && memory_budget_ok
        && regression.task_ok
        && regression.io_ok
        && regression.memory_ok;

    let profile_term = build_profile_term(
        &result,
        budgets,
        task_budget_ok,
        io_budget_ok,
        memory_budget_ok,
        &regression,
        ok,
    );
    write_profile_term(out, &profile_term)?;

    let profile_hash = hex32(hash_term(&profile_term));
    let value = Term::Map(
        [
            (TermOrdKey(Term::symbol(":ok")), Term::Bool(ok)),
            (
                TermOrdKey(Term::symbol(":out")),
                Term::Str(out.display().to_string()),
            ),
            (
                TermOrdKey(Term::symbol(":history")),
                Term::Str(history.display().to_string()),
            ),
            (
                TermOrdKey(Term::symbol(":history-samples")),
                Term::Int((regression.history_samples as i64).into()),
            ),
            (
                TermOrdKey(Term::symbol(":profile-h")),
                Term::Str(profile_hash),
            ),
            (
                TermOrdKey(Term::symbol(":task-elapsed-us")),
                Term::Int((result.task.elapsed_us as i64).into()),
            ),
            (
                TermOrdKey(Term::symbol(":io-elapsed-us")),
                Term::Int((result.io.elapsed_us as i64).into()),
            ),
            (
                TermOrdKey(Term::symbol(":memory-elapsed-us")),
                Term::Int((result.memory.elapsed_us as i64).into()),
            ),
            (
                TermOrdKey(Term::symbol(":task-budget-ok")),
                Term::Bool(task_budget_ok),
            ),
            (
                TermOrdKey(Term::symbol(":io-budget-ok")),
                Term::Bool(io_budget_ok),
            ),
            (
                TermOrdKey(Term::symbol(":memory-budget-ok")),
                Term::Bool(memory_budget_ok),
            ),
            (
                TermOrdKey(Term::symbol(":regression-ok")),
                Term::Bool(regression.task_ok && regression.io_ok && regression.memory_ok),
            ),
        ]
        .into_iter()
        .collect(),
    );

    Ok(LocalPkgResult {
        kind: "genesis/pkg-runtime-profile-v0.1",
        log_op: "pkg-runtime-profile",
        program_hash: hash_term(&value),
        value,
    })
}

fn build_profile_term(
    result: &RuntimeProfileResult,
    budgets: RuntimeProfileBudgets,
    task_budget_ok: bool,
    io_budget_ok: bool,
    memory_budget_ok: bool,
    regression: &RuntimeRegressionSummary,
    ok: bool,
) -> Term {
    Term::Map(
        [
            (
                TermOrdKey(Term::symbol(":kind")),
                Term::symbol(":runtime-profile"),
            ),
            (TermOrdKey(Term::symbol(":v")), Term::Int(1.into())),
            (TermOrdKey(Term::symbol(":ok")), Term::Bool(ok)),
            (
                TermOrdKey(Term::symbol(":task-scheduler")),
                runtime_probe_term(
                    ":runtime/task-scheduler-probe",
                    result.task,
                    budgets.task_budget_us,
                    task_budget_ok,
                ),
            ),
            (
                TermOrdKey(Term::symbol(":io-store-cycle")),
                io_probe_term(result.io, budgets.io_budget_us, io_budget_ok),
            ),
            (
                TermOrdKey(Term::symbol(":memory-pressure")),
                runtime_probe_term(
                    ":runtime/memory-pressure-probe",
                    result.memory,
                    budgets.memory_budget_us,
                    memory_budget_ok,
                ),
            ),
            (
                TermOrdKey(Term::symbol(":regression")),
                regression_term(regression),
            ),
        ]
        .into_iter()
        .collect(),
    )
}

fn runtime_probe_term(op: &str, probe: RuntimeProbeTrace, budget_us: u64, budget_ok: bool) -> Term {
    let observed = probe.observed;
    let mem = observed.mem;
    Term::Map(
        [
            (TermOrdKey(Term::symbol(":op")), Term::symbol(op)),
            (
                TermOrdKey(Term::symbol(":elapsed-us")),
                Term::Int((probe.elapsed_us as i64).into()),
            ),
            (
                TermOrdKey(Term::symbol(":effect-entries")),
                Term::Int((probe.effect_entries as i64).into()),
            ),
            (
                TermOrdKey(Term::symbol(":steps")),
                Term::Int((observed.steps as i64).into()),
            ),
            (
                TermOrdKey(Term::symbol(":mem/pair-cells")),
                Term::Int((mem.pair_cells as i64).into()),
            ),
            (
                TermOrdKey(Term::symbol(":mem/max-vec-len")),
                Term::Int((mem.max_vec_len as i64).into()),
            ),
            (
                TermOrdKey(Term::symbol(":mem/max-map-len")),
                Term::Int((mem.max_map_len as i64).into()),
            ),
            (
                TermOrdKey(Term::symbol(":mem/max-bytes-len")),
                Term::Int((mem.max_bytes_len as i64).into()),
            ),
            (
                TermOrdKey(Term::symbol(":mem/max-string-len")),
                Term::Int((mem.max_string_len as i64).into()),
            ),
            (
                TermOrdKey(Term::symbol(":budget-us")),
                Term::Int((budget_us as i64).into()),
            ),
            (
                TermOrdKey(Term::symbol(":budget-ok")),
                Term::Bool(budget_ok),
            ),
        ]
        .into_iter()
        .collect(),
    )
}

fn io_probe_term(probe: IoProbeTrace, budget_us: u64, budget_ok: bool) -> Term {
    Term::Map(
        [
            (
                TermOrdKey(Term::symbol(":op")),
                Term::symbol(":runtime/io-store-cycle-probe"),
            ),
            (
                TermOrdKey(Term::symbol(":elapsed-us")),
                Term::Int((probe.elapsed_us as i64).into()),
            ),
            (
                TermOrdKey(Term::symbol(":bytes")),
                Term::Int((probe.bytes as i64).into()),
            ),
            (
                TermOrdKey(Term::symbol(":budget-us")),
                Term::Int((budget_us as i64).into()),
            ),
            (
                TermOrdKey(Term::symbol(":budget-ok")),
                Term::Bool(budget_ok),
            ),
        ]
        .into_iter()
        .collect(),
    )
}

fn regression_term(regression: &RuntimeRegressionSummary) -> Term {
    Term::Map(
        [
            (
                TermOrdKey(Term::symbol(":history-samples")),
                Term::Int((regression.history_samples as i64).into()),
            ),
            (
                TermOrdKey(Term::symbol(":min-history")),
                Term::Int((regression.min_history as i64).into()),
            ),
            (
                TermOrdKey(Term::symbol(":max-regression-percent")),
                Term::Int((regression.max_regression_percent as i64).into()),
            ),
            (
                TermOrdKey(Term::symbol(":applied")),
                Term::Bool(regression.applied),
            ),
            (
                TermOrdKey(Term::symbol(":task-p95-us")),
                regression
                    .task_p95_us
                    .map(|v| Term::Int((v as i64).into()))
                    .unwrap_or(Term::symbol(":none")),
            ),
            (
                TermOrdKey(Term::symbol(":io-p95-us")),
                regression
                    .io_p95_us
                    .map(|v| Term::Int((v as i64).into()))
                    .unwrap_or(Term::symbol(":none")),
            ),
            (
                TermOrdKey(Term::symbol(":memory-p95-us")),
                regression
                    .memory_p95_us
                    .map(|v| Term::Int((v as i64).into()))
                    .unwrap_or(Term::symbol(":none")),
            ),
            (
                TermOrdKey(Term::symbol(":task-ok")),
                Term::Bool(regression.task_ok),
            ),
            (
                TermOrdKey(Term::symbol(":io-ok")),
                Term::Bool(regression.io_ok),
            ),
            (
                TermOrdKey(Term::symbol(":memory-ok")),
                Term::Bool(regression.memory_ok),
            ),
        ]
        .into_iter()
        .collect(),
    )
}

fn write_profile_term(path: &Path, profile_term: &Term) -> Result<(), String> {
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent).map_err(|e| format!("create {}: {e}", parent.display()))?;
    }
    std::fs::write(path, format!("{}\n", gc_coreform::print_term(profile_term)))
        .map_err(|e| format!("write {}: {e}", path.display()))
}

fn append_runtime_history(path: &Path, entry: RuntimeProfileHistoryEntry) -> Result<(), String> {
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent).map_err(|e| format!("create {}: {e}", parent.display()))?;
    }
    let json =
        serde_json::to_string(&entry).map_err(|e| format!("serialize runtime history: {e}"))?;
    let mut f = std::fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(path)
        .map_err(|e| format!("open {}: {e}", path.display()))?;
    use std::io::Write;
    writeln!(f, "{json}").map_err(|e| format!("append {}: {e}", path.display()))
}

fn evaluate_runtime_regression(
    history_path: &Path,
    min_history: usize,
    max_regression_percent: u64,
    current: RuntimeProfileHistoryEntry,
) -> Result<RuntimeRegressionSummary, String> {
    let history = load_history_entries(history_path)?;
    if history.len() < min_history {
        return Ok(RuntimeRegressionSummary {
            history_samples: history.len(),
            min_history,
            max_regression_percent,
            applied: false,
            task_p95_us: None,
            io_p95_us: None,
            memory_p95_us: None,
            task_ok: true,
            io_ok: true,
            memory_ok: true,
        });
    }

    let task_p95 = p95_us(history.iter().map(|x| x.task_elapsed_us).collect());
    let io_p95 = p95_us(history.iter().map(|x| x.io_elapsed_us).collect());
    let memory_p95 = p95_us(history.iter().map(|x| x.memory_elapsed_us).collect());
    let task_ok =
        within_regression_budget(current.task_elapsed_us, task_p95, max_regression_percent);
    let io_ok = within_regression_budget(current.io_elapsed_us, io_p95, max_regression_percent);
    let memory_ok = within_regression_budget(
        current.memory_elapsed_us,
        memory_p95,
        max_regression_percent,
    );

    Ok(RuntimeRegressionSummary {
        history_samples: history.len(),
        min_history,
        max_regression_percent,
        applied: true,
        task_p95_us: Some(task_p95),
        io_p95_us: Some(io_p95),
        memory_p95_us: Some(memory_p95),
        task_ok,
        io_ok,
        memory_ok,
    })
}

fn load_history_entries(path: &Path) -> Result<Vec<RuntimeProfileHistoryEntry>, String> {
    if !path.exists() {
        return Ok(Vec::new());
    }
    let src = std::fs::read_to_string(path).map_err(|e| format!("read {}: {e}", path.display()))?;
    let mut out = Vec::new();
    for (idx, line) in src.lines().enumerate() {
        if line.trim().is_empty() {
            continue;
        }
        let entry: RuntimeProfileHistoryEntry = serde_json::from_str(line).map_err(|e| {
            format!(
                "parse {} line {} as runtime profile history entry: {e}",
                path.display(),
                idx + 1
            )
        })?;
        out.push(entry);
    }
    Ok(out)
}

fn p95_us(mut values: Vec<u64>) -> u64 {
    values.sort_unstable();
    let idx = (((values.len() as f64) * 0.95).ceil() as usize).saturating_sub(1);
    values[idx]
}

fn within_regression_budget(observed: u64, baseline_p95: u64, max_regression_percent: u64) -> bool {
    let allowed = baseline_p95.saturating_mul(100 + max_regression_percent) / 100;
    observed <= allowed
}

fn run_runtime_profile_probes() -> Result<RuntimeProfileResult, String> {
    let task = run_task_scheduler_probe()?;
    let io = run_io_store_cycle_probe()?;
    let memory = run_memory_pressure_probe()?;
    Ok(RuntimeProfileResult { task, io, memory })
}

fn run_task_scheduler_probe() -> Result<RuntimeProbeTrace, String> {
    let src = r#"
(def bench/prog
  ((core/effect::bind
     (((core/task::spawn "runtime-profile") "task-1")
       {
         :task/program
         (core/task::program
           [
             (core/task::step/set 1)
             (core/task::step/int-add 2)
             (core/task::step/return 3)
           ])
       }))
    (fn (spawn-resp)
      (core/task::await ((core/map::get spawn-resp) (quote :task-id))))))
bench/prog
"#;
    let (forms, program_hash) =
        parse_canonicalize_hash_module_source(src).map_err(|e| format!("task source: {e}"))?;
    let policy = CapsPolicy::from_toml_str("allow = [\"core/task::spawn\", \"core/task::await\"]")
        .map_err(|e| format!("task profile policy: {e}"))?;
    let start = Instant::now();
    let mut ctx = EvalCtx::with_step_limit(None);
    let prelude = build_prelude(&mut ctx);
    let mut env = prelude.env;
    let program = eval_module_default_value(&mut ctx, &mut env, &forms)
        .map_err(|e| format!("task eval: {e}"))?;
    let run_out = gc_effects::run(
        &mut ctx,
        &policy,
        program,
        program_hash,
        "runtime-profile".to_string(),
    )
    .map_err(|e| format!("task run: {e}"))?;
    let elapsed_us = saturating_u64(start.elapsed().as_micros());
    Ok(RuntimeProbeTrace {
        elapsed_us,
        observed: ctx.observed_counters(),
        effect_entries: run_out.log.entries.len() as u64,
    })
}

fn run_io_store_cycle_probe() -> Result<IoProbeTrace, String> {
    let store_root = io_probe_store_root();
    std::fs::create_dir_all(&store_root)
        .map_err(|e| format!("create io probe store root {}: {e}", store_root.display()))?;

    let run_result = (|| {
        let store =
            ArtifactStore::open(&store_root).map_err(|e| format!("open io probe store: {e}"))?;
        let payload = vec![7u8; 16 * 1024];

        let start = Instant::now();
        let hash = store
            .put_bytes(&payload)
            .map_err(|e| format!("io probe put_bytes: {e}"))?;
        let got = store
            .get_bytes(&hash)
            .map_err(|e| format!("io probe get_bytes: {e}"))?;
        if got.len() != payload.len() {
            return Err(format!(
                "io probe payload length mismatch: {} != {}",
                got.len(),
                payload.len()
            ));
        }
        let elapsed_us = saturating_u64(start.elapsed().as_micros());
        Ok(IoProbeTrace {
            elapsed_us,
            bytes: payload.len() as u64,
        })
    })();

    let _cleanup = std::fs::remove_dir_all(&store_root);
    run_result
}

fn io_probe_store_root() -> PathBuf {
    static NEXT_ID: AtomicU64 = AtomicU64::new(0);
    let id = NEXT_ID.fetch_add(1, Ordering::Relaxed);
    std::env::temp_dir().join(format!(
        "genesis-runtime-profile-store-{}-{id}",
        crate::platform_process_id()
    ))
}

fn run_memory_pressure_probe() -> Result<RuntimeProbeTrace, String> {
    let src = r#"
(def bench/memory
  (let ((v (prim vec/push [1 2 3 4 5 6 7 8 9 10] 11))
        (m (prim map/put {"k1" 1 "k2" 2 "k3" 3} "k4" 4))
        (s (prim str/concat
              "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa"
              "bbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb")))
    [v m s]))
bench/memory
"#;
    let (forms, _program_hash) =
        parse_canonicalize_hash_module_source(src).map_err(|e| format!("memory source: {e}"))?;
    let start = Instant::now();
    let mut ctx = EvalCtx::with_step_limit(None);
    let prelude = build_prelude(&mut ctx);
    let mut env = prelude.env;
    let _value = eval_module_default_value(&mut ctx, &mut env, &forms)
        .map_err(|e| format!("memory profile eval: {e}"))?;
    let elapsed_us = saturating_u64(start.elapsed().as_micros());
    Ok(RuntimeProbeTrace {
        elapsed_us,
        observed: ctx.observed_counters(),
        effect_entries: 0,
    })
}

fn saturating_u64(v: u128) -> u64 {
    if v > u64::MAX as u128 {
        u64::MAX
    } else {
        v as u64
    }
}

fn hex32(bytes: [u8; 32]) -> String {
    const LUT: &[u8; 16] = b"0123456789abcdef";
    let mut out = String::with_capacity(64);
    for b in bytes {
        out.push(LUT[(b >> 4) as usize] as char);
        out.push(LUT[(b & 0x0f) as usize] as char);
    }
    out
}

#[cfg(test)]
mod tests {
    use super::{p95_us, within_regression_budget};

    #[test]
    fn p95_uses_sorted_upper_quantile() {
        assert_eq!(p95_us(vec![1, 2, 3, 4, 5]), 5);
        assert_eq!(p95_us(vec![10, 20, 30, 40, 50, 60, 70, 80, 90, 100]), 100);
    }

    #[test]
    fn regression_budget_thresholds_are_enforced() {
        assert!(within_regression_budget(110, 100, 10));
        assert!(!within_regression_budget(111, 100, 10));
        assert!(within_regression_budget(100, 100, 0));
    }
}
