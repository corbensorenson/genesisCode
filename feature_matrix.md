# GenesisCode Feature Matrix (Audit Date: 2026-02-23)

Last updated: 2026-02-23
Scope: language/runtime/toolchain capabilities relevant to AI-first, agentic software development.

Legend:
- `✅` first-class and production-usable
- `⚠️` implemented but partial, profile-scoped, or contract-level only
- `❌` not present as first-class capability

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
| Deployment/bundle target pipeline in core toolchain | ⚠️ (deterministic target bundles + signatures; launch artifacts are contract scripts) | ⚠️ | ⚠️ | ⚠️ | ⚠️ | ⚠️ |
| Real native deploy packaging/execution artifacts | ❌ | ⚠️ | ⚠️ | ⚠️ | ⚠️ | ⚠️ |
| Strict selfhost frontend default in production binaries | ✅ | ❌ | ❌ | ❌ | ❌ | ❌ |
| Full no-bootstrap-language self-host closure | ⚠️ (bounded permanent Rust TCB contract) | ⚠️ | ⚠️ | ❌ | ❌ | ⚠️ |
| Machine-readable agent planning index + schema contracts | ✅ | ⚠️ | ❌ | ❌ | ❌ | ❌ |
| Semantic edit/refactor primitives as first-class CLI surface | ✅ | ❌ | ❌ | ❌ | ❌ | ❌ |
| LSP/editor server surface | ❌ | ✅ | ✅ | ✅ | ✅ | ⚠️ |
| Interactive debugger/breakpoint surface | ✅ (`debug step/break/inspect/continue/frames` deterministic trace API) | ✅ | ✅ | ✅ | ✅ | ⚠️ |
| GPU compute + graphics capability surfaces | ✅ | ⚠️ | ⚠️ | ⚠️ | ⚠️ | ⚠️ |
| Deterministic task concurrency runtime with replay semantics | ✅ | ⚠️ | ⚠️ | ⚠️ | ⚠️ | ⚠️ |
| WASM runtime + WASI CLI surfaces | ✅ | ✅ | ⚠️ | ✅ | ⚠️ | ⚠️ |
| Supply-chain policy + provenance gating in primary CLI | ✅ | ⚠️ | ⚠️ | ⚠️ | ⚠️ | ⚠️ |
| Local artifact GC by semantic reachability (refs/locks/pins) | ✅ | ❌ | ❌ | ❌ | ❌ | ❌ |
| Regulated assurance profile packs in core workflow | ✅ (engineering coverage) | ❌ | ❌ | ❌ | ❌ | ❌ |

Known GenesisCode gaps identified in this audit (tracked in `/Users/corbensorenson/Documents/genesisCode/upgrade_plan.md`):
- P0.3 Rust-to-GC migration still has `in-progress` rows not cut over to GC-first dispatch.
- P2.2 Signed domain bootstrap bundle set is not yet complete for broad agent bootstrapping.

Primary evidence paths:
- `/Users/corbensorenson/Documents/genesisCode/docs/spec/CLI.md`
- `/Users/corbensorenson/Documents/genesisCode/docs/spec/SELF_HOST_BOUNDARY.md`
- `/Users/corbensorenson/Documents/genesisCode/docs/spec/GC_MODULE_BOUNDARIES_v0.1.md`
- `/Users/corbensorenson/Documents/genesisCode/crates/gc_cli_driver/src/pkg_workspace_ops_build_artifacts.rs`
- `/Users/corbensorenson/Documents/genesisCode/scripts/check_gcpm_target_runtime_pipelines.sh`
- `/Users/corbensorenson/Documents/genesisCode/scripts/check_disk_headroom.sh`
- `/Users/corbensorenson/Documents/genesisCode/upgrade_plan.md`
- `/Users/corbensorenson/Documents/genesisCode/.genesis/perf/selfhost_readiness_report.json`
- `/Users/corbensorenson/Documents/genesisCode/.genesis/perf/upgrade_plan_health_profile_report.json`
