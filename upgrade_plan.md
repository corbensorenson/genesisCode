# GenesisCode Upgrade Plan - Red-Team Backlog (Unresolved Only)

Last updated: 2026-02-21

This file contains only unresolved findings from the latest red-team pass.
Completed items are intentionally removed.

Open checklist items: 14

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

- [ ] P1.3 Add inbound/server networking primitives (not just client flows)
  - Evidence: `io/net` currently provides `http-request`, `tcp-open/send/recv/close`, `udp-*`, `ws-open/send/recv/close` only (`docs/spec/HOST_ABI_INDEX_v0.1.json`).
  - Exit criteria:
    - Add listener/acceptor ops (`tcp-listen`, `tcp-accept`, `http-listen`, `http-respond`, `ws-accept`).
    - Add policy gates for bind addresses, ports, and request-size bounds.
    - Add deterministic service workflow examples and gauntlet coverage.

- [ ] P1.4 Add durable data capability family for real application state
  - Evidence: host ABI operations currently have no `io/db::*` family (`docs/spec/HOST_ABI_INDEX_v0.1.json`).
  - Exit criteria:
    - Add deterministic `io/db::*` contract family (transactional KV/SQL minimum slice).
    - Add policy controls for DSN/paths/query classes and row/bytes limits.
    - Add replay-stable result envelopes and migration-safe schema docs.

- [ ] P1.5 Add deterministic deployment/bundle targets to `gcpm`
  - Evidence: CLI/gcpm command surface focuses on build/test/pack/run/env/assurance but not targeted web/desktop/service deployment bundles (`docs/spec/CLI.md`).
  - Exit criteria:
    - Add `gcpm build --target <web|desktop|service>` with stable manifest schema.
    - Emit reproducible target bundles + provenance metadata.
    - Add CI validation for bundle reproducibility and schema contracts.

- [ ] P1.6 Expand agent capability gauntlet domain coverage for missing product surfaces
  - Evidence: required gauntlet domains currently omit browser runtime, XR, durable data, inbound servers, and deployment (`docs/spec/AGENT_CAPABILITY_GAUNTLET_v0.1.md`).
  - Exit criteria:
    - Extend gauntlet required domains and reference workflows for new families.
    - Keep native/WASI parity gate coverage for all added domains.
    - Enforce release/full profile failure on missing domain successes.

## AI-First Authoring + Optimization (P2)

- [ ] P2.1 Replace generic plugin payloads with typed plugin ABI schemas
  - Evidence: extension surface is currently generic `host/plugin::command` / `editor/plugin::command` wrappers (`docs/spec/HOST_ABI.md`, `prelude/modules/00_core_plugin.gc`).
  - Exit criteria:
    - Add schema-id-based plugin request/response typing.
    - Add preflight schema validation at runtime boundaries.
    - Add policy contract updates and backward-compat adapters.

- [ ] P2.2 Add workspace-wide semantic graph/refactor API for agents
  - Evidence: CLI surface currently exposes `semantic-edit index` (module-level index) without a first-class workspace graph API (`docs/spec/CLI.md`).
  - Exit criteria:
    - Add deterministic workspace symbol graph and dependency-edge export.
    - Add refactor-plan artifacts (rename/move/extract) with conflict previews.
    - Add machine-mergeable patch planning for multi-file transformations.

- [ ] P2.3 Add first-class deterministic runtime profiler for non-gfx workloads
  - Evidence: current profiling is mostly benchmark/report scripts and gfx runtime trace artifacts (`docs/spec/CONCURRENCY_GPU_SLO_v0.1.md`, `prelude/modules/10_gfx_runtime_trace.gc`).
  - Exit criteria:
    - Add runtime profile ops/contracts for task scheduler, IO, and memory pressure traces.
    - Emit deterministic profile artifacts directly from CLI/gcpm workflows.
    - Add SLO regression gates on profile artifacts (not only wall-time).

- [ ] P2.4 Add media/asset pipeline contracts for AI-generated games/apps
  - Evidence: host ABI families currently include gfx/gpu/audio/input/window but no media asset decode/encode contract family (`docs/spec/HOST_ABI_INDEX_v0.1.json`).
  - Exit criteria:
    - Add `core/media::*` / host ABI contracts for image/audio asset processing.
    - Add deterministic asset hashing/transcoding policies and constraints.
    - Add domain-kit workflows for asset import/build pipelines.

- [ ] P2.5 Consolidate documentation to reduce retrieval overhead and drift
  - Evidence: repository currently carries a large Markdown surface (`119` `.md` files), plus legacy split docs maintained beside bundles.
  - Exit criteria:
    - Collapse overlapping split docs into canonical bundles where possible.
    - Keep deprecation map current with explicit replacements.
    - Add doc-lint gates for duplicate normative sections and stale references.

- [ ] P2.6 Promote `write_genesisCode_skill` into a versioned, machine-consumable authoring contract
  - Evidence: `docs/write_genesisCode_skill.md` is currently a pointer document to `.agents/skills/genesiscode-authoring/SKILL.md`.
  - Exit criteria:
    - Add versioned schema/checklist artifact consumable by multiple agent runtimes.
    - Add conformance tests verifying skill/bundle alignment with CLI/ABI indices.
    - Add drift gate that fails when skill guidance contradicts current specs.

- [ ] P2.7 Add full conformance lanes for browser/XR/server/data/deploy workflows
  - Evidence: current agent gauntlet + parity lanes cover 10 domains, none for browser/XR/durable-data/deployment.
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
