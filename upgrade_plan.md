# GenesisCode Upgrade Plan - Red-Team Backlog (Unresolved Only)

Last updated: 2026-02-21

This file contains only unresolved findings from the latest red-team pass.
Completed items are intentionally removed.

Open checklist items: 4

## Selfhost + AI-First Surface

- [ ] P1.1 Reduce selfhost artifact bootstrap latency to budget.
  - Why: selfhost cutover is functionally complete, but the artifact build path is currently too slow for rapid AI iteration loops.
  - Evidence:
    - `scripts/check_perf_budgets.sh` fails on `selfhost_bootstrap_ms`.
    - `.genesis/perf/perf_budget_metrics.json` reports `selfhost_bootstrap_ms=108587` vs budget `15000`.
  - Done when:
    - `bash scripts/check_perf_budgets.sh` passes in `selfhost-strict` profile;
    - `selfhost-artifact` uses deterministic incremental reuse (unchanged-module/content cache), not full rebuild every run;
    - perf report records stable p95 under configured budget across history.

- [ ] P1.2 Expand host filesystem capability surface beyond read/write.
  - Why: AI-authored real projects need first-class directory and file lifecycle operations; `read/write` alone forces brittle process-shell fallbacks.
  - Evidence:
    - `docs/spec/HOST_ABI.md` currently lists only `io/fs::read` and `io/fs::write`.
  - Done when:
    - canonical ops exist for `io/fs::stat`, `io/fs::list`, `io/fs::mkdir`, `io/fs::remove`, `io/fs::rename` (and copy/move if separate);
    - deny-by-default policy controls are defined per op;
    - deterministic effect-log/replay semantics and success/deny tests are implemented.

- [ ] P1.3 Add process lifecycle and stream-oriented execution primitives.
  - Why: `sys/process::exec` one-shot is insufficient for long-running toolchains, service orchestration, and incremental agent workflows.
  - Evidence:
    - `docs/spec/HOST_ABI.md` exposes only `sys/process::exec`.
  - Done when:
    - canonical ops exist for spawn/wait/kill and stdio stream read/write (`sys/process::*`);
    - capability policy covers allowlists, limits, and timeout controls;
    - replay behavior is deterministic and covered by strict tests.

- [ ] P1.4 Publish machine-readable host capability payload/response schemas for agents.
  - Why: AI-first authoring quality depends on strict machine contracts; operation-name-only indices force heuristic prompting.
  - Evidence:
    - `docs/spec/HOST_ABI_INDEX_v0.1.json` currently contains operation/family listings only (no per-op payload/response schema contract).
  - Done when:
    - generated schema index exists with per-op required/optional fields, type/domain constraints, and response envelope shapes;
    - `agent-index` includes references to these schemas;
    - CI enforces schema/index freshness and implementation-doc parity.
