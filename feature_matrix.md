# GenesisCode Feature Matrix (Audit Date: 2026-02-25)

Last updated: 2026-02-25  
Scope: features that matter for agentic coding autonomy and selfhost reliability.

Legend:
- `✅` production-capable and validated in active gates
- `⚠️` available but with important closure/hardening debt
- `❌` not first-class

| Capability | GenesisCode | Rust | Go | TypeScript (Node) | Python | Zig |
|---|---|---|---|---|---|---|
| Pure deterministic kernel separated from effects | ✅ | ⚠️ | ❌ | ❌ | ❌ | ❌ |
| Canonical semantic IR + stable content hash identity | ✅ | ❌ | ❌ | ❌ | ❌ | ❌ |
| Sealed unforgeable effect/error protocol | ✅ | ❌ | ❌ | ❌ | ❌ | ❌ |
| Deny-by-default capability policy runtime | ✅ | ⚠️ | ⚠️ | ⚠️ | ⚠️ | ⚠️ |
| Deterministic effect logs + replay checks | ✅ | ⚠️ | ⚠️ | ⚠️ | ⚠️ | ⚠️ |
| Built-in semantic VCS (`commit/patch/refs`) | ✅ | ❌ | ❌ | ❌ | ❌ | ❌ |
| Built-in package/project manager (`pkg`/`gcpm`) | ✅ | ✅ | ✅ | ⚠️ | ⚠️ | ✅ |
| Selfhost frontend default in production CLIs | ✅ | ❌ | ❌ | ❌ | ❌ | ❌ |
| Full cutover profile wired into default inner-loop health | ✅ | ❌ | ❌ | ❌ | ❌ | ❌ |
| Selfhost guard robustness against stale local binaries | ✅ | ⚠️ | ⚠️ | ⚠️ | ⚠️ | ⚠️ |
| Agent workflow gauntlet (service/network/data/gfx/gpu/deploy/xr) | ✅ | ⚠️ | ⚠️ | ⚠️ | ⚠️ | ⚠️ |
| Runtime skill-pack conformance breadth across required domains | ✅ | ❌ | ❌ | ❌ | ❌ | ❌ |
| Deterministic concurrency/task replay surface | ✅ | ⚠️ | ⚠️ | ⚠️ | ⚠️ | ⚠️ |
| GPU compute capability independent of graphics surface | ✅ | ⚠️ | ⚠️ | ⚠️ | ⚠️ | ⚠️ |
| Graphics/window/input/audio capability families | ✅ | ⚠️ | ⚠️ | ⚠️ | ⚠️ | ⚠️ |
| Strict GPU/XR runtime evidence as default productization lane | ✅ | ⚠️ | ⚠️ | ⚠️ | ⚠️ | ⚠️ |
| Deployment target pipeline in core toolchain | ✅ | ⚠️ | ⚠️ | ⚠️ | ⚠️ | ⚠️ |
| Reachability-based artifact GC (`refs`/locks/pins) | ✅ | ❌ | ❌ | ❌ | ❌ | ❌ |
| Production module decomposition for AI maintainability | ✅ | ⚠️ | ⚠️ | ⚠️ | ⚠️ | ⚠️ |

Known GenesisCode gaps identified in this audit (tracked in `/Users/corbensorenson/Documents/genesisCode/upgrade_plan.md`):
- `P1.3` - deterministic external ecosystem bridge into GenesisPkg is not yet first-class.

Primary evidence paths:
- `/Users/corbensorenson/Documents/genesisCode/upgrade_plan.md`
- `/Users/corbensorenson/Documents/genesisCode/.genesis/perf/selfhost_readiness_report.json`
- `/Users/corbensorenson/Documents/genesisCode/docs/skill_pack/write_genesiscode_v1/manifest.json`
- `/Users/corbensorenson/Documents/genesisCode/.genesis/perf/write_genesiscode_skill_conformance_report.json`
- `/Users/corbensorenson/Documents/genesisCode/policies/source_decomposition_progress.toml`
- `/Users/corbensorenson/Documents/genesisCode/.genesis/perf/agent_capability_gauntlet_release_confidence_report.json`
- `/Users/corbensorenson/Documents/genesisCode/.genesis/perf/gpu_xr_productization_kits_report.json`
