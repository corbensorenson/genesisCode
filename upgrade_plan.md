# GenesisCode Upgrade Plan - Open Red-Team Backlog (Self-Hosted v1)

Last updated: 2026-02-20

This plan contains only unresolved findings from the current red-team pass.

Open checklist items: 12

## P0 - Immediate Breakages and Signal Gaps

- [ ] P0.1 Fix broken hot-path budget gate (`check_hot_path_budgets.sh`).
  Evidence:
  - `/Users/corbensorenson/Documents/genesisCode/scripts/check_hot_path_budgets.sh:92` uses retired `core/pkg::*` ops (`core/pkg::new`, `core/pkg::lock`, `core/pkg::install`, `core/pkg::update`).
  - `/Users/corbensorenson/Documents/genesisCode/crates/gc_effects/src/policy.rs:314` rejects those high-level ops in favor of `core/pkg-low::*`.
  - Local run currently fails: `bash /Users/corbensorenson/Documents/genesisCode/scripts/check_hot_path_budgets.sh`.
  Acceptance:
  - Script uses the current low-level capability surface and passes locally + in CI.

- [ ] P0.2 Expand `upgrade_plan` health gate so zero-open cannot hide failing performance/security gates.
  Evidence:
  - `/Users/corbensorenson/Documents/genesisCode/scripts/check_upgrade_plan_health.sh:37`-`44` enforces only a subset of gates and omits `check_hot_path_budgets.sh`.
  - Current state: `check_upgrade_plan_health.sh` passes while `check_hot_path_budgets.sh` fails.
  Acceptance:
  - Health gate includes all mandatory red-team gates or delegates to one canonical gate script.

- [ ] P0.3 Fix caps parse diagnostics that currently hide root cause details.
  Evidence:
  - `/Users/corbensorenson/Documents/genesisCode/crates/gc_cli_driver/src/cmd_pkg.rs:12`
  - `/Users/corbensorenson/Documents/genesisCode/crates/gc_cli_driver/src/cmd_core.rs:386`
  - Current `--json` error for invalid caps often collapses to `read <path>` with no actionable parse detail.
  Acceptance:
  - `caps/parse` errors include underlying policy parse/validation cause (offending op/key/value), not only file-read context.

- [ ] P0.4 Add disk-capacity preflight + cleanup path for local fast loops.
  Evidence:
  - Local fast loop failed with `No space left on device` during `scripts/test_fast.sh`.
  - Current workspace usage snapshot: `target/` ~= 17G, available filesystem space ~= 53MiB.
  Acceptance:
  - `test_fast.sh` and `test_changed_fast.sh` fail early with clear disk guidance and optional deterministic cleanup path.

## P1 - Self-Host Completion Roadblocks

- [ ] P1.1 Remove the temporary Rust semantic bridge for package low-level ops.
  Evidence:
  - `/Users/corbensorenson/Documents/genesisCode/docs/spec/SELF_HOST_BOUNDARY.md:76`-`83` explicitly allows temporary bridge behavior for `core/pkg-low::{load-package,snapshot}`.
  Acceptance:
  - Package load/snapshot semantics are implemented in selfhost `.gc` modules with no bridge exception in boundary spec.

- [ ] P1.2 Route remaining native command surfaces through strict selfhost-only flow.
  Evidence:
  - `/Users/corbensorenson/Documents/genesisCode/docs/spec/CLI.md:27`-`29` lists native selfhost-only routed set; security ops (`verify`, `keygen`, `sign`, `transparency-verify`) are not in the native routed list.
  Acceptance:
  - Native strict selfhost mode covers all production CLI commands (or non-routed commands are explicitly moved to bootstrap-only scope).

- [ ] P1.3 Close artifact-resolution ambiguity by pinning selfhost toolchain identity in workspace state.
  Evidence:
  - `/Users/corbensorenson/Documents/genesisCode/docs/spec/CLI.md:36`-`40` allows fallback discovery (`./.genesis/selfhost/toolchain.gc`, `selfhost/toolchain.gc`).
  Acceptance:
  - Production flows require an explicit artifact hash pin (lock/workspace metadata), and fallback discovery is dev-only.

- [ ] P1.4 Retire Rust parity engine paths from default developer workflow after final parity cutoff.
  Evidence:
  - `/Users/corbensorenson/Documents/genesisCode/docs/spec/CLI.md:44`-`66` keeps `--engine rust` compatibility via dedicated parity binaries.
  Acceptance:
  - Rust parity path is archived under explicit bootstrap tooling, not part of day-to-day production workflow or default docs.

## P2 - Hardening, AI-First Maintainability, and Throughput

- [ ] P2.1 Extend no-panic guard coverage to all production binaries and WASM host crates.
  Evidence:
  - `/Users/corbensorenson/Documents/genesisCode/scripts/check_no_user_panics.sh:8`-`14` only checks a subset of library crates.
  Acceptance:
  - Panic/unwrap/expect guard includes `gc_cli`/`gc_wasi_cli` binaries and `gc_wasm` production code paths.

- [ ] P2.2 Decompose monolithic hotspot modules to improve AI editability and reduce compile churn.
  Evidence:
  - Current large files include:
    - `/Users/corbensorenson/Documents/genesisCode/crates/gc_opt/src/stage2_wasm.rs` (4599 LOC)
    - `/Users/corbensorenson/Documents/genesisCode/crates/gc_obligations/src/lib.rs` (2541 LOC)
    - `/Users/corbensorenson/Documents/genesisCode/crates/gc_effects/src/runner_cap_pkg_low.rs` (2217 LOC)
    - `/Users/corbensorenson/Documents/genesisCode/crates/gc_cli_driver/src/lib.rs` (2032 LOC)
  Acceptance:
  - Split into focused modules with stable boundaries; no single production source file over agreed size budget (set budget in policy).

- [ ] P2.3 Strengthen CI profile strategy to catch standard/full regressions earlier on PRs.
  Evidence:
  - `/Users/corbensorenson/Documents/genesisCode/.github/workflows/ci.yml:35` defaults push/PR to `fast`.
  - Performance and stress gates run only on `standard|full` (`ci.yml:150`-`168`).
  Acceptance:
  - PR policy runs at least one standard-profile lane (full on schedule remains), or path-based escalation enforces standard lanes for runtime/runner/cli changes.

- [ ] P2.4 Advance deterministic concurrency + GPU/compute throughput coverage from v0.1 API to production SLO contracts.
  Evidence:
  - `/Users/corbensorenson/Documents/genesisCode/docs/spec/CONCURRENCY_v0.1.md` is normative for ABI shape but does not yet define production SLO/budget contracts for scheduler contention classes.
  - Host ABI already exposes separate compute and graphics op families:
    `/Users/corbensorenson/Documents/genesisCode/docs/spec/HOST_ABI.md`.
  Acceptance:
  - Add concurrency and GPU/compute budget obligations (throughput/latency/replay) with CI-enforced thresholds and artifacted reports.

## Execution Order (Recommended)

1. P0.1 -> P0.2 -> P0.3 (restore truthful gate signal).
2. P0.4 (stabilize local iteration reliability).
3. P1.1 -> P1.2 -> P1.3 -> P1.4 (finish selfhost cutover boundaries).
4. P2.1 -> P2.2 -> P2.3 -> P2.4 (hardening + throughput).
