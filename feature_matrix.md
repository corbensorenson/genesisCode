# GenesisCode Feature Matrix (Audit Date: 2026-02-23)

Last updated: 2026-02-23
Scope: capabilities that matter for AI-first autonomous coding workflows.

Legend:
- `✅` production-usable and verified by active gates
- `⚠️` present but with closure debt, profile constraints, or maturity limits
- `❌` not first-class today

| Capability | GenesisCode | Rust | Go | TypeScript (Node) | Python | Zig |
|---|---|---|---|---|---|---|
| Pure deterministic kernel separated from effects | ✅ | ⚠️ | ❌ | ❌ | ❌ | ❌ |
| Canonical CoreForm + stable semantic hash identity | ✅ | ❌ | ❌ | ❌ | ❌ | ❌ |
| Unforgeable sealed effect/error protocol | ✅ | ❌ | ❌ | ❌ | ❌ | ❌ |
| Deny-by-default capability policy runtime | ✅ | ⚠️ | ⚠️ | ⚠️ | ⚠️ | ⚠️ |
| Deterministic effect logs + replay mismatch fail-fast | ✅ | ⚠️ | ⚠️ | ⚠️ | ⚠️ | ⚠️ |
| Language-native semantic VCS graph (`commit/refs/patch`) | ✅ | ❌ | ❌ | ❌ | ❌ | ❌ |
| Built-in package/project manager (`pkg` / `gcpm`) | ✅ | ✅ | ✅ | ⚠️ | ⚠️ | ✅ |
| Agent planning schema (`cli-schema`, `agent-index`) | ✅ | ⚠️ | ❌ | ❌ | ❌ | ❌ |
| Semantic edit/refactor CLI (`semantic-edit`) | ✅ | ❌ | ❌ | ❌ | ❌ | ❌ |
| Deterministic task concurrency primitives | ✅ | ⚠️ | ⚠️ | ⚠️ | ⚠️ | ⚠️ |
| GPU compute capability independent of graphics surface | ✅ | ⚠️ | ⚠️ | ⚠️ | ⚠️ | ⚠️ |
| GPU device-runtime strict lane in default gauntlets | ⚠️ (dev/test lanes still rely on deterministic fallback profiles) | ⚠️ | ⚠️ | ⚠️ | ⚠️ | ⚠️ |
| Graphics/window/input/audio capability families | ✅ | ⚠️ | ⚠️ | ⚠️ | ⚠️ | ⚠️ |
| Deployment target pipeline in core toolchain | ⚠️ (deterministic target artifacts exist, but launchers are shell wrappers) | ⚠️ | ⚠️ | ⚠️ | ⚠️ | ⚠️ |
| Native platform packaging/execution adapters | ⚠️ (target metadata + signatures present, native packager closure incomplete) | ✅ | ⚠️ | ⚠️ | ⚠️ | ⚠️ |
| Strict selfhost frontend default in production binaries | ✅ | ❌ | ❌ | ❌ | ❌ | ❌ |
| Full selfhost closure with minimal bounded Rust TCB | ⚠️ (explicit exception crates remain and migration rows are still in-progress) | ⚠️ | ⚠️ | ❌ | ❌ | ⚠️ |
| WASI CLI parity with native CLI for registry hosting | ⚠️ (`registry serve` unsupported in WASI binaries) | ⚠️ | ⚠️ | ⚠️ | ⚠️ | ⚠️ |
| Type/effect system maturity for large generated codebases | ⚠️ (gradual subset; inference intentionally conservative) | ✅ | ⚠️ | ⚠️ | ⚠️ | ⚠️ |
| Interactive deterministic debugger surface | ✅ | ✅ | ✅ | ✅ | ✅ | ⚠️ |
| LSP/editor server for human IDE workflows | ❌ (agent-first schema/edit surfaces are primary) | ✅ | ✅ | ✅ | ✅ | ⚠️ |
| Policy-gated supply-chain provenance workflow | ✅ | ⚠️ | ⚠️ | ⚠️ | ⚠️ | ⚠️ |
| Reachability-based artifact GC (refs/locks/pins) | ✅ | ❌ | ❌ | ❌ | ❌ | ❌ |

Known GenesisCode gaps identified in this audit (tracked in `/Users/corbensorenson/Documents/genesisCode/upgrade_plan.md`):
- P2.1 - obligation execution migration remains phase-1 in-progress.
- P2.2 - semantic workspace mutation path remains phase-1 in-progress.
- P2.3 - patch orchestration path remains phase-1 in-progress.
- P2.4 - kernel/eval migration row remains phase-1 in-progress under bounded Rust TCB.
- P2.5 - VCS command orchestration remains phase-1 in-progress.
- P2.6 - legacy compatibility aliases still exist in production code paths.
- P2.7 - strict device-runtime GPU requirement is not the universal default lane.
- P2.8 - deployment launch artifacts are shell stubs instead of target-native packagers.
- P2.9 - registry serving is native-only (`registry serve` unavailable in WASI CLI).
- P2.10 - typechecker remains gradual/conservative for large autonomous codegen workloads.
- P2.11 - high-churn host API evolution contracts need stronger machine-checkable guarantees.
- P2.12 - GC-native project ops contract pack needs stronger versioned failure taxonomy.
- P3.1 - strict profile wall-time still needs tighter fast-loop optimization policy.
- P3.2 - remaining high-churn Rust files need further decomposition for agent edit locality.

Primary evidence paths:
- `/Users/corbensorenson/Documents/genesisCode/docs/spec/CLI.md`
- `/Users/corbensorenson/Documents/genesisCode/docs/spec/TYPES.md`
- `/Users/corbensorenson/Documents/genesisCode/docs/spec/SELF_HOST_BOUNDARY.md`
- `/Users/corbensorenson/Documents/genesisCode/docs/spec/FULL_SELFHOST_CUTOVER_PROFILE_v0.1.md`
- `/Users/corbensorenson/Documents/genesisCode/docs/spec/GC_MODULE_BOUNDARIES_v0.1.md`
- `/Users/corbensorenson/Documents/genesisCode/policies/source_decomposition_progress.toml`
- `/Users/corbensorenson/Documents/genesisCode/crates/gc_cli_driver/src/cmd_registry.rs`
- `/Users/corbensorenson/Documents/genesisCode/crates/gc_cli_driver/src/pkg_workspace_ops_build_artifacts.rs`
- `/Users/corbensorenson/Documents/genesisCode/crates/gc_effects/src/policy_tests.rs`
- `/Users/corbensorenson/Documents/genesisCode/upgrade_plan.md`
- `/Users/corbensorenson/Documents/genesisCode/.genesis/perf/selfhost_readiness_report.json`
- `/Users/corbensorenson/Documents/genesisCode/.genesis/perf/agent_capability_gauntlet_report.json`
- `/Users/corbensorenson/Documents/genesisCode/.genesis/perf/gpu_gfx_headroom_conformance_report.json`
