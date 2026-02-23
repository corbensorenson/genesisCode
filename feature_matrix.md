# GenesisCode Feature Matrix (Red-Team Baseline)

Last updated: 2026-02-23  
Scope: first-class language/runtime/toolchain capabilities, not third-party ecosystem breadth.

Legend:
- `✅` first-class in language/toolchain/runtime
- `⚠️` partial, profile-gated, or ecosystem-dependent
- `❌` absent as first-class capability

| Capability | GenesisCode | Rust | Go | TypeScript (Node) | Python | Zig |
|---|---|---|---|---|---|---|
| Pure deterministic kernel separated from effects | ✅ | ⚠️ | ❌ | ❌ | ❌ | ❌ |
| Canonical language form + stable semantic hashing | ✅ | ❌ | ❌ | ❌ | ❌ | ❌ |
| Unforgeable protocol values (sealed error/effect channels) | ✅ | ❌ | ❌ | ❌ | ❌ | ❌ |
| Deny-by-default capability runtime | ✅ | ⚠️ | ⚠️ | ⚠️ | ⚠️ | ⚠️ |
| Deterministic effect logs + replay checker | ✅ | ⚠️ | ⚠️ | ⚠️ | ⚠️ | ⚠️ |
| Obligations + evidence artifacts in core workflow | ✅ | ❌ | ❌ | ❌ | ❌ | ❌ |
| Language-native semantic VCS graph + refs + bundles | ✅ | ❌ | ❌ | ❌ | ❌ | ❌ |
| Built-in package/project manager | ✅ (`gcpm`/`pkg`) | ✅ (`cargo`) | ✅ (`go mod`) | ⚠️ (npm/pnpm/yarn) | ⚠️ (pip/poetry/pixi) | ✅ (`zig build`) |
| Deployment/bundle target pipeline in core toolchain | ✅ (`gcpm build --target`) | ⚠️ | ⚠️ | ⚠️ | ⚠️ | ⚠️ |
| Strict selfhost frontend default in production binaries | ✅ | ❌ | ❌ | ❌ | ❌ | ❌ |
| Full no-bootstrap-language self-host closure | ⚠️ (bounded permanent Rust TCB contract) | ⚠️ | ⚠️ | ❌ | ❌ | ⚠️ |
| GPU compute + graphics capability surfaces | ✅ | ⚠️ | ⚠️ | ⚠️ | ⚠️ | ⚠️ |
| Deterministic concurrency/task runtime with replay semantics | ✅ | ❌ | ❌ | ❌ | ❌ | ❌ |
| WASM runtime + WASI CLI surfaces | ✅ | ✅ | ⚠️ | ✅ | ⚠️ | ⚠️ |
| Schema-stable machine JSON contracts for CLI/tooling | ✅ | ⚠️ | ❌ | ❌ | ❌ | ❌ |
| Supply-chain policy + provenance gating in primary CLI | ✅ | ⚠️ | ⚠️ | ⚠️ | ⚠️ | ⚠️ |
| Local artifact GC by semantic reachability (refs/locks/pins) | ✅ | ❌ | ❌ | ❌ | ❌ | ❌ |
| Built-in regulated-assurance profile packs | ⚠️ (engineering readiness built-in; certification program external) | ❌ | ❌ | ❌ | ❌ | ❌ |

Known GenesisCode gaps
- P0.1 - strict prepush lane runtime is too slow for AI-first iteration loops.
- P1.1 - high-churn Rust hotspots remain near decomposition caps in core paths.
- P1.2 - largest selfhost/prelude `.gc` modules still need further decomposition for AI edit locality.

Primary evidence paths
- `/Users/corbensorenson/Documents/genesisCode/docs/spec/SELF_HOST_BOUNDARY.md`
- `/Users/corbensorenson/Documents/genesisCode/docs/spec/FULL_SELFHOST_CUTOVER_PROFILE_v0.1.md`
- `/Users/corbensorenson/Documents/genesisCode/docs/spec/CLI.md`
- `/Users/corbensorenson/Documents/genesisCode/docs/spec/WASI.md`
- `/Users/corbensorenson/Documents/genesisCode/scripts/check_upgrade_plan_health.sh`
- `/Users/corbensorenson/Documents/genesisCode/scripts/check_full_selfhost_cutover_profile.sh`
- `/Users/corbensorenson/Documents/genesisCode/scripts/check_production_cli_help_surface.sh`
- `/Users/corbensorenson/Documents/genesisCode/crates/gc_effects/src/runner_gpu_backend_policy.rs`
- `/Users/corbensorenson/Documents/genesisCode/scripts/check_task_concurrency_stress.sh`
- `/Users/corbensorenson/Documents/genesisCode/scripts/check_host_bridge_fault_injection.sh`
- `/Users/corbensorenson/Documents/genesisCode/.genesis/perf/upgrade_plan_health_profile_report.json`
- `/Users/corbensorenson/Documents/genesisCode/.genesis/perf/source_decomposition_progress_report.json`
- `/Users/corbensorenson/Documents/genesisCode/.genesis/perf/selfhost_readiness_report.json`
- `/Users/corbensorenson/Documents/genesisCode/upgrade_plan.md`
