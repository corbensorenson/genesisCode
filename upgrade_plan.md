# GenesisCode Upgrade Plan — Self-Hosted v1 Fast Path

Last updated: 2026-02-18

Completed items from the prior plan were intentionally removed. This file now tracks only unresolved blockers and high-impact upgrades.

Open checklist items: 17

## Self-Hosted v1 Exit Criteria
- [ ] All production command semantics are owned by `.gc` contracts.
- [ ] Rust runtime is limited to kernel + low-level host ABI + transport.
- [ ] Deterministic multithreading/parallel execution is available through Genesis capabilities and replayable logs.
- [ ] Performance is sufficient for fast AI iteration (sub-minute incremental inner loop, materially reduced full-suite runtime).
- [ ] Rust semantic fallbacks are disabled in production mode.

## Workstream A — Final Semantic Extraction (Rust -> `.gc`)
- [ ] Move `core/pkg::snapshot` semantics fully into `.gc` contracts (keep only host primitives in Rust).
- [ ] Move `core/pkg::publish` semantics fully into `.gc` contracts (including closure planning, policy prechecks, and report shaping).
- [ ] Remove remaining high-level `core/pkg::*` execution branches from `/Users/corbensorenson/Documents/genesisCode/crates/gc_effects/src/runner.rs` after parity lock.
- [ ] Remove remaining high-level `core/vcs::*`, `core/gc::*`, and `core/gpk::*` execution branches from `/Users/corbensorenson/Documents/genesisCode/crates/gc_effects/src/runner.rs` once low-level seam parity is complete.
- [ ] Keep Rust capability surface to low-level ops only: `core/store::*`, `core/refs::*`, `core/sync::*`, `io/fs::*`, `sys/time::now`, plus graphics/editor host ops.
- [x] Add CI guard that fails if new high-level semantic ops are added back into runner dispatch without explicit waiver.

## Workstream B — Deterministic Multithreading/Parallelism (AI-First)

### B1. Spec + ABI
- [x] Add normative spec doc for deterministic concurrency (`docs/spec/CONCURRENCY_v0.1.md`) covering scheduling, replay, cancellation, and failure semantics.
- [x] Freeze task/capability ABI in `docs/spec/HOST_ABI.md` for:
  - `core/task::spawn`
  - `core/task::await`
  - `core/task::cancel`
  - `core/task::status`
  - `core/task::scope`

### B2. Language/Prelude Surface
- [x] Add `.gc` contracts for structured concurrency (scope-based spawn/await/cancel) in prelude modules.
- [x] Add deterministic combinators optimized for AI-generated workflows (`core/task::all`, `core/task::race`, bounded parallel map over vectors).
- [x] Define clear data contracts for task handles/results/errors (stable map schema, no ad hoc shapes).

### B3. Runtime Scheduler
- [x] Implement deterministic logical scheduler in runner (stable ordering by task-id + explicit policy knobs).
- [x] Add bounded worker pool for host-side parallel execution where allowed, while preserving deterministic commit order.
- [x] Replace per-operation ad hoc thread spawning in timeout path (`with_timeout`) with pooled execution/timers.
- [x] Enforce policy-based limits: max tasks, max workers, queue depth, per-task step/time budgets.

### B4. Replay + Evidence
- [x] Extend effect log schema with task/schedule events (`:task-id`, `:parent-task`, `:schedule-step`, `:await-edge`).
- [x] Implement replay verifier for concurrent runs (schedule mismatch, missing task events, response mismatch).
- [x] Add obligation `core/obligation::concurrency-replay` for effectful concurrent tests.

### B5. Type/Effects Integration
- [x] Extend `gc_types` effect-row tracking for task ops so concurrency usage is explicit and checkable.
- [x] Add deterministic-safety checks for AI-authored parallel code patterns (unknown effect tails + undeclared caps fail in strict mode).

## Workstream C — Throughput and Latency Gains

### C1. Hot Path Runtime
- [x] Optimize artifact reads in `/Users/corbensorenson/Documents/genesisCode/crates/gc_effects/src/store.rs` to avoid double-read verification on every `get`.
- [x] Add optional integrity cache mode (hash memo with invalidation) to keep strong guarantees without repeated full rehash in tight loops.
- [x] Batch and parallelize remote sync transfers with deterministic result collation (upload/download worker pool + stable ordering).

### C2. Obligation Engine
- [x] Remove per-test full package re-evaluation in `/Users/corbensorenson/Documents/genesisCode/crates/gc_obligations/src/lib.rs` by reusing a package-eval snapshot for test closure lookup.
- [x] Add deterministic parallel test execution for independent tests (stable result ordering; isolated contexts; reproducible logs).
- [x] Add incremental obligation cache keyed by `(module hashes, caps policy hash, obligation config)` to skip unchanged work.

### C3. Selfhost Frontend Startup
- [x] Add cross-process cache for compiled selfhost artifact modules (not only in-process cache) to reduce repeated parse/canonicalize/compile on CLI invocations.
- [x] Add warm startup mode for CLI/daemonized execution to amortize toolchain bootstrap across command bursts used by AI agents.

### C4. Test/CI Iteration Speed
- [x] Split integration tests into fast/standard/full lanes and gate expensive parity matrices behind explicit CI profile.
- [x] Remove redundant native/WASI duplicate coverage where the same invariant is already proven by shared harness.
- [x] Add automatic test sharding support with deterministic seed/order and artifact collation.
- [x] Publish performance budgets in CI (test wall-time, selfhost bootstrap time, obligation runtime) and fail on regressions.

## Workstream D — AI-First Self-Hosting Completion
- [ ] Complete Stage-2 selfhost path so toolchain evolution is authored and validated in Genesis code first.
- [ ] Remove production fallback to Rust semantic implementations once parity + replay + obligation gates pass.
- [ ] Move remaining bootstrap-only Rust semantic code under `/Users/corbensorenson/Documents/genesisCode/old_bootstrap` after cutover.
- [ ] Add an AI-oriented contract/API style pass (stable machine-readable diagnostics, canonical fix schemas, patch-intent metadata) as required quality gate for new modules.

## Acceptance Checks (Must Pass Before Declaring v1)
- [ ] `--selfhost-only` path exercises full production workflow with no Rust semantic fallbacks.
- [x] Concurrency test suite validates deterministic replay under mixed task scheduling.
- [ ] Package publish/install workflows are fully `.gc`-owned semantics with low-level host caps only.
- [ ] Full CI includes performance regression checks and passes within target runtime budget.
