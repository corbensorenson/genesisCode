# GenesisCode Upgrade Plan - Red-Team Backlog (Unresolved Only)

Last updated: 2026-02-21

This file contains only unresolved findings from the latest red-team pass.
Completed items are intentionally removed.

Open checklist items: 7

## Platform + Runtime Surface (P1)

- [ ] P1.1 Add browser runtime host profile + ABI family for wasm-host execution
  - Evidence: runtime profiles are currently `headless|interactive|desktop` (`docs/spec/CAPS_TOML.md`, `docs/spec/RUNTIME_BACKEND_PROFILES_v0.1.md`).
  - Exit criteria:
    - Add `browser` first-party profile with deterministic policy contract.
    - Add browser host ABI families (window/input/audio/storage baseline).
    - Add native/WASM bridge parity tests and schema index entries.

- [ ] P1.2 Add first-class WebXR support (`gfx/xr::*`) with deterministic bridge semantics
  - Evidence: host ABI index has no XR family (`docs/spec/HOST_ABI_INDEX_v0.1.json` contains no `gfx/xr::*` operations).
  - Exit criteria:
    - Introduce `gfx/xr::session-open|frame-poll|input-poll|submit-frame|session-close` contracts.
    - Add prelude wrappers + domain kit for XR render loop/state contracts.
    - Add deterministic replay/parity checks for XR frame/input streams.

- [ ] P1.5 Add deterministic deployment/bundle targets to `gcpm`
  - Evidence: CLI/gcpm command surface focuses on build/test/pack/run/env/assurance but not targeted web/desktop/service deployment bundles (`docs/spec/CLI.md`).
  - Exit criteria:
    - Add `gcpm build --target <web|desktop|service>` with stable manifest schema.
    - Emit reproducible target bundles + provenance metadata.
    - Add CI validation for bundle reproducibility and schema contracts.

- [ ] P1.6 Expand agent capability gauntlet domain coverage for missing product surfaces
  - Evidence: required gauntlet domains now include inbound-server and durable-data coverage but still omit browser runtime, XR, and deployment (`docs/spec/AGENT_CAPABILITY_GAUNTLET_v0.1.md`).
  - Exit criteria:
    - Extend gauntlet required domains and reference workflows for new families.
    - Keep native/WASI parity gate coverage for all added domains.
    - Enforce release/full profile failure on missing domain successes.

## AI-First Authoring + Optimization (P2)

- [ ] P2.4 Add media/asset pipeline contracts for AI-generated games/apps
  - Evidence: host ABI families currently include gfx/gpu/audio/input/window but no media asset decode/encode contract family (`docs/spec/HOST_ABI_INDEX_v0.1.json`).
  - Exit criteria:
    - Add `core/media::*` / host ABI contracts for image/audio asset processing.
    - Add deterministic asset hashing/transcoding policies and constraints.
    - Add domain-kit workflows for asset import/build pipelines.

- [ ] P2.7 Add full conformance lanes for browser/XR/server/data/deploy workflows
  - Evidence: current agent gauntlet + parity lanes cover 12 domains, with no browser/XR/deployment domain lanes yet.
  - Exit criteria:
    - Add new workflow suites and report contracts for each new domain.
    - Integrate into `prepush-standard` and `release-full` profile gates.
    - Persist history with p95 regression budgets per lane.

- [ ] P2.8 Tighten deterministic performance budgets for end-to-end agent workflows
  - Evidence: existing perf gates track core loops and selected suites, but not full user-facing multi-domain scenario latency budgets.
  - Exit criteria:
    - Add end-to-end scenario benchmarks (service + data + gfx + network).
    - Enforce median + p95 budgets with contention-aware sampling.
    - Fail release profiles when scenario budgets regress beyond configured thresholds.
