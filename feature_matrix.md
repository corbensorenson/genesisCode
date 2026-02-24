# GenesisCode Feature Matrix (Audit Date: 2026-02-24)

Last updated: 2026-02-24
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
| GPU device-runtime strict lane in default gauntlets | ✅ (release/full gauntlets are fail-closed `require-device`; dev/test lanes remain explicit fallback profiles) | ⚠️ | ⚠️ | ⚠️ | ⚠️ | ⚠️ |
| Graphics/window/input/audio capability families | ✅ | ⚠️ | ⚠️ | ⚠️ | ⚠️ | ⚠️ |
| Deployment target pipeline in core toolchain | ✅ (deterministic target artifacts + contract launch adapters with verified boot/smoke lanes) | ⚠️ | ⚠️ | ⚠️ | ⚠️ | ⚠️ |
| Native platform packaging/execution adapters | ⚠️ (target metadata + signatures present, native packager closure incomplete) | ✅ | ⚠️ | ⚠️ | ⚠️ | ⚠️ |
| Strict selfhost frontend default in production binaries | ✅ | ❌ | ❌ | ❌ | ❌ | ❌ |
| Full selfhost closure with minimal bounded Rust TCB | ⚠️ (explicit exception crates remain and migration rows are still in-progress) | ⚠️ | ⚠️ | ❌ | ❌ | ⚠️ |
| WASI CLI parity with native CLI for registry hosting | ✅ (WASI `registry serve` provides deterministic file-contract registry remote) | ⚠️ | ⚠️ | ⚠️ | ⚠️ | ⚠️ |
| Type/effect system maturity for large generated codebases | ⚠️ (gradual core remains, but strict-shapes and broader task/effect inference are enforced) | ✅ | ⚠️ | ⚠️ | ⚠️ | ⚠️ |
| Interactive deterministic debugger surface | ✅ | ✅ | ✅ | ✅ | ✅ | ⚠️ |
| LSP/editor server for human IDE workflows | ❌ (agent-first schema/edit surfaces are primary) | ✅ | ✅ | ✅ | ✅ | ⚠️ |
| Policy-gated supply-chain provenance workflow | ✅ | ⚠️ | ⚠️ | ⚠️ | ⚠️ | ⚠️ |
| Reachability-based artifact GC (refs/locks/pins) | ✅ | ❌ | ❌ | ❌ | ❌ | ❌ |

Known GenesisCode gaps identified in this audit (tracked in `/Users/corbensorenson/Documents/genesisCode/upgrade_plan.md`):
- P2.1 - obligation execution migration remains phase-1 in-progress.
- P2.2 - semantic workspace mutation path remains phase-1 in-progress.
- P2.3 - patch orchestration path remains phase-1 in-progress.

Primary evidence paths:
- `/Users/corbensorenson/Documents/genesisCode/docs/spec/CLI.md`
- `/Users/corbensorenson/Documents/genesisCode/docs/spec/TYPES.md`
- `/Users/corbensorenson/Documents/genesisCode/docs/spec/SELF_HOST_BOUNDARY.md`
- `/Users/corbensorenson/Documents/genesisCode/docs/spec/FULL_SELFHOST_CUTOVER_PROFILE_v0.1.md`
- `/Users/corbensorenson/Documents/genesisCode/docs/spec/GC_MODULE_BOUNDARIES_v0.1.md`
- `/Users/corbensorenson/Documents/genesisCode/policies/source_decomposition_progress.toml`
- `/Users/corbensorenson/Documents/genesisCode/upgrade_plan.md`
- `/Users/corbensorenson/Documents/genesisCode/.genesis/perf/selfhost_readiness_report.json`
- `/Users/corbensorenson/Documents/genesisCode/.genesis/perf/agent_capability_gauntlet_report.json`
- `/Users/corbensorenson/Documents/genesisCode/.genesis/perf/gpu_gfx_headroom_conformance_report.json`
