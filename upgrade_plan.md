# GenesisCode Upgrade Plan - Open Red-Team Backlog (Self-Hosted + AI-First v1)

Last updated: 2026-02-20

This plan contains only unresolved findings from the latest fine-tooth-comb red-team pass.

Open checklist items: 5

## P0 - Self-Host Correctness and Missing Core Surface

- [ ] P0.1 Implement `genesis commit` command surface (`new`, `show`) and align all CLI specs.
  Evidence:
  - `/Users/corbensorenson/Documents/genesisCode/docs/CLI_SPEC_GENESISPKG_GENESISGRAPH_v0.1.md:70`-`73` declares required `genesis commit new` and `genesis commit show`.
  - `/Users/corbensorenson/Documents/genesisCode/crates/gc_cli_driver/src/lib.rs:163` defines top-level `Cmd` without a `Commit` variant.
  - `/Users/corbensorenson/Documents/genesisCode/crates/gc_cli_driver/src/lib.rs:1070`-`1150` shows `VcsCmd` variants (hash/diff/apply/log/blame/why/merge/resolve) but no commit creation/show command.
  Acceptance:
  - Add top-level `genesis commit new` and `genesis commit show` (native + WASI).
  - Route commit creation through the same deterministic contract/evidence path as other selfhost-routed command groups.
  - Add parity/smoke tests for commit creation, artifact persistence, and JSON envelope kinds.
  - Keep `docs/spec/CLI.md` and `docs/CLI_SPEC_GENESISPKG_GENESISGRAPH_v0.1.md` consistent.

- [ ] P0.2 Implement registry chunked upload protocol (`upload/start|chunk|finish`) and client planner usage.
  Evidence:
  - `/Users/corbensorenson/Documents/genesisCode/docs/REGISTRY_PROTOCOL_MINIMAL_v0.1.md:43`-`48` defines required chunked upload endpoints.
  - `/Users/corbensorenson/Documents/genesisCode/crates/gc_registry/src/lib.rs:499`-`552` implements only monolithic `PUT /store/put/<hash>`.
  - `/Users/corbensorenson/Documents/genesisCode/crates/gc_registry/src/lib.rs:146` exposes `max_chunk_bytes` in ping response, but chunked transport path is absent.
  Acceptance:
  - Add registry client APIs for `upload/start`, `upload/chunk`, `upload/finish` (+ optional status).
  - Update sync push planning to switch between direct put vs chunked upload based on payload size and `max_chunk_bytes`.
  - Add resumability/integrity tests for interrupted uploads and hash mismatch fail-close behavior.

- [ ] P0.3 Remove Rust frontend/engine options from production CLI parser surface (parity binaries only).
  Evidence:
  - `/Users/corbensorenson/Documents/genesisCode/crates/gc_cli_driver/src/lib.rs:157`-`160` exposes `CoreformFrontendArg::{Rust,Selfhost}` in production parser.
  - `/Users/corbensorenson/Documents/genesisCode/crates/gc_cli_driver/src/lib.rs:493`-`497` exposes `FmtEngine::{Rust,Selfhost}` in production parser.
  - `/Users/corbensorenson/Documents/genesisCode/crates/gc_cli_driver/src/selfhost_frontend.rs:241`-`257` still carries runtime Rust-frontend branch logic.
  Acceptance:
  - Production binaries (`genesis`, `genesis_wasi`) accept only selfhost frontend/engine values.
  - Rust frontend/engine parser values and execution branches compile only in parity binaries (`genesis_parity`, `genesis_wasi_parity`).
  - Add guard/test that production `--help` output contains no Rust frontend/engine option values.

## P1 - Performance and Reliability Hardening

- [x] P1.1 Make perf/SLO measurements fail-closed on low disk in all budget-enforced paths (including local runs).
  Evidence:
  - `/Users/corbensorenson/Documents/genesisCode/scripts/check_disk_headroom.sh:89`-`94` defaults to non-strict mode outside CI.
  - `/Users/corbensorenson/Documents/genesisCode/scripts/check_disk_headroom.sh:107`-`114` exits success on low disk in non-strict mode.
  - `/Users/corbensorenson/Documents/genesisCode/scripts/test_changed_fast.sh:7` always invokes disk-headroom check with default strict mode.
  - Release-profile run in this pass logged: `test-changed-fast: insufficient disk headroom ... continuing in non-strict mode ...`.
  Acceptance:
  - Perf/SLO entrypoints (`check_perf_budgets.sh`, `check_ai_iteration_slo.sh`, `check_hot_path_budgets.sh`, `check_runtime_microbench_budgets.sh`) now force strict disk mode via `GENESIS_PERF_DISK_STRICT_MODE` (default `1`).
  - `test_changed_fast.sh` now supports explicit strict mode (`--strict-disk`, `GENESIS_TEST_CHANGED_STRICT_DISK`) and `check_ai_iteration_slo.sh` calls it in strict mode.
  - Low-disk conditions now fail budget checks in these gates instead of producing false-green measurements.

- [x] P1.2 Run performance budgets on release-equivalent artifacts and record profile metadata in reports.
  Evidence:
  - `/Users/corbensorenson/Documents/genesisCode/scripts/check_perf_budgets.sh:21` builds debug (`cargo build -p gc_cli`) and measures `target/debug/genesis`.
  - `/Users/corbensorenson/Documents/genesisCode/scripts/check_runtime_microbench_budgets.sh:23` uses `cargo run -p gc_runtime_bench` (debug by default).
  Acceptance:
  - Perf scripts now default to `GENESIS_PERF_CARGO_PROFILE=selfhost-strict` and consistently build/run `target/<profile>/...` artifacts.
  - Profile/build metadata is now emitted in perf artifacts (`perf_budget_metrics.json`, `hot_path_metrics.json`, `ai_iteration_slo_metrics.json`, runtime microbench + concurrency SLO outputs).
  - Runtime microbench report schema now includes `build_profile` and `build_mode` for auditability.

## P2 - AI-First Authoring and Agent Ergonomics

- [ ] P2.1 Tighten source-size budgets and split remaining oversized hot files/modules.
  Evidence:
  - `/Users/corbensorenson/Documents/genesisCode/policies/source_size_budget.toml:5` sets Rust max lines to 2200 and `/Users/corbensorenson/Documents/genesisCode/policies/source_size_budget.toml:10` sets `.gc` max lines to 1800.
  - Current production sizes from this pass:
    - `/Users/corbensorenson/Documents/genesisCode/crates/gc_cli_driver/src/lib.rs` = 2048 lines
    - `/Users/corbensorenson/Documents/genesisCode/crates/gc_opt/src/stage2_wasm.rs` = 1991 lines
    - `/Users/corbensorenson/Documents/genesisCode/crates/gc_effects/src/runner_cap_vcs_low.rs` = 1902 lines
    - `/Users/corbensorenson/Documents/genesisCode/crates/gc_effects/src/runner_cap_pkg_low.rs` = 1852 lines
    - `/Users/corbensorenson/Documents/genesisCode/prelude/modules/10_gfx.gc` = 1700 lines
  Acceptance:
  - Reduce budget targets to AI-editable levels (Rust <= 1600, `.gc` <= 1200) with staged rollout.
  - Split listed large files into focused modules with stable interfaces and ownership boundaries.
  - Add regression checks to prevent monolith regrowth.

- [ ] P2.2 Publish a full CLI JSON schema registry for non-gcpm commands (agent-safe contracts).
  Evidence:
  - `/Users/corbensorenson/Documents/genesisCode/docs/spec/CLI.md:177`-`183` documents only a generic envelope (`genesis/<command>-v0.2`) without per-command field contracts.
  - `/Users/corbensorenson/Documents/genesisCode/docs/spec/GCPM_JSON_SCHEMAS_v0.1.md` exists for gcpm, but no equivalent schema index exists for `store/*`, `refs/*`, `sync/*`, `gc/*`, `vcs/*`, `eval`, `run`, `replay`, etc.
  Acceptance:
  - Add `docs/spec/CLI_JSON_SCHEMAS_v0.1.md` (or equivalent split docs) covering every command kind and required/optional `data` fields.
  - Add conformance tests that assert each command’s emitted JSON matches documented schema (including error envelope variants).
  - Keep schema IDs stable and versioned to support autonomous agent planning.

## Execution Order (Recommended)

1. P0.1 -> P0.2 -> P0.3
2. P1.1 -> P1.2
3. P2.1 -> P2.2
