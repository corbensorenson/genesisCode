# GenesisCode Upgrade Plan - Red-Team Backlog (Unresolved Only)

Last updated: 2026-02-21

This file contains only unresolved findings from the latest red-team pass.
Completed items are intentionally removed.

Open checklist items: 6

## Runtime + Language Breadth (P2)

- [ ] P2.1 Promote device-runtime GPU compute to the default agent gauntlet posture (fallback only in explicit dev lanes)
  Evidence: current gauntlet report includes workflows running with backend `"deterministic-fallback"` even while device-runtime conformance lanes exist separately.
  Done when: primary gauntlet workflows require `device-runtime` in release/full profiles, with deterministic fallback kept only for explicit dev diagnostics.

- [ ] P2.2 Add deterministic stress workloads for multithreaded task/channel semantics
  Evidence: concurrency APIs exist (`core/task::*`) but there is no dedicated high-contention stress matrix validating replay determinism under cancellation, channel close races, and bounded parallel map/reduce patterns.
  Done when: stress suite exists with replay equivalence assertions and profile budgets, and is wired into health gates.

- [ ] P2.3 Reduce default health/test wall-time with staged caches and prebuilt artifact reuse
  Evidence: full `check_upgrade_plan_health.sh` can still trigger heavy compile/test cycles and lock contention (`Blocking waiting for file lock on build directory`) in common local loops.
  Done when: dev-fast default path consistently stays under budget with deterministic cache reuse and no lock-contention regressions across repeated runs.

- [ ] P2.4 Add fault-injection conformance for host capability bridges
  Evidence: capability paths are broadly implemented, but there is no unified failure-injection matrix proving deterministic sealed errors and replay stability across fs/net/process/plugin failure modes.
  Done when: a host bridge fault-injection suite runs in CI and validates stable error envelopes + replay outputs for each capability family.

- [ ] P2.5 Expand cross-platform runtime parity for agent workflows (native + WASI + wasm-host bridge)
  Evidence: cross-host checks exist, but the agent reference workflow gauntlet currently executes as local shell workflows and does not enforce full workflow parity across WASI/wasm-host targets.
  Done when: the same agent domain workflows are validated under native and WASI/wasm-host execution profiles with comparable replay hashes.

- [ ] P2.6 Ship an extremely detailed agent authoring skill pack for GenesisCode
  Evidence: `docs/write_genesisCode_skill.md` is currently a pointer entry; `.agents/skills/genesiscode-authoring/SKILL.md` is concise and does not yet provide deep playbooks for all core domains.
  Done when: authoring skill docs provide comprehensive patterns, anti-patterns, contract templates, workflow recipes, and validation loops for building end-to-end products in GenesisCode.
