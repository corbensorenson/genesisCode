# GenesisCode Upgrade Plan - Red-Team Backlog (Unresolved Only)

Last updated: 2026-02-22

This file contains only unresolved findings from the latest red-team pass.
Completed items are intentionally removed.

Open checklist items: 10

## P1 - Productization blockers for "agent can build anything" readiness

- [ ] P1.3 Enforce statistically meaningful regression gates for generative agent workloads and per-workflow gauntlet lanes.
  - Evidence: `.genesis/perf/agent_generative_workloads_report.json` and `.genesis/perf/agent_generative_workloads_parity_report.json` show `history_samples = 1` and `regression_enforced = false`; gauntlet workflow entries still report `p95_enforced = false`.
  - Exit criteria: seed and enforce per-workflow history minima with fail-closed p95/regression gates (native + wasi parity lanes), not just aggregate scenario metrics.

- [ ] P1.4 Expand XR runtime surface beyond session/frame/input/haptics to production-grade AR/VR primitives.
  - Evidence: `docs/spec/XR_HOST_RUNTIME_v0.1.md` currently standardizes only six ops (`session-open`, `frame-poll`, `input-poll`, `haptics-pulse`, `submit-frame`, `session-close`).
  - Exit criteria: add canonical contracts/policies/wrappers/tests for anchors, hand tracking, hit-test/spatial mesh, and compositor/layer primitives with replay-stable envelopes.

- [ ] P1.5 Add browser-native WebXR device conformance lane (real runtime behavior), not bridge-envelope conformance alone.
  - Evidence: XR contract still identifies first-party adapter as `"xr-headless-sim"` and treats `"webxr-device"` as a bridge-routed lane (`docs/spec/XR_HOST_RUNTIME_v0.1.md`).
  - Exit criteria: add first-class browser runtime conformance fixture(s) exercising real WebXR session/frame/input/haptics behavior under deterministic capture/replay rules.

- [ ] P1.6 Expand typechecker inference/verification coverage for complex agent-authored programs.
  - Evidence: `docs/spec/TYPES.md` states v0.2 checker is conservative and treats unknown applications as `?`, reducing strong conformance guarantees.
  - Exit criteria: broaden inference coverage (collection/contract/effect-heavy patterns), reduce unknown leakage, and add parity tests proving stable behavior across rust/selfhost paths.

- [ ] P1.7 Expand semantic patch schema from node-replacement primitives to high-level refactor ops needed by autonomous agents.
  - Evidence: `docs/spec/PATCH_SCHEMA.md` currently supports `:replace-node`, `:replace-node-id`, `:add-module`, `:remove-module`, `:update-manifest` only.
  - Exit criteria: add canonical ops for symbol rename, module move/split, import/export rewrites, and contract signature migration with deterministic validation and replay-aware evidence.

- [ ] P1.8 Cut repeated Cargo rebuild/lock contention across health scripts to improve iteration throughput.
  - Evidence: health runs repeatedly emit `Blocking waiting for file lock on build directory/package cache` while recompiling overlapping crates in multiple scripts.
  - Exit criteria: introduce shared prebuild/cache orchestration for health profiles (dev-fast/prepush/release-full) and demonstrate reduced end-to-end wall time without weakening gates.

## P2 - Hardening, security posture, and ecosystem completeness

- [ ] P2.1 Tighten release-profile plugin/bridge execution defaults (allowlists + binary pinning).
  - Evidence: host/plugin bridge controls like `allow_commands` and `bridge_cmd_sha256` are optional in current policy docs (`docs/spec/HOST_ABI.md`, `docs/spec/CAPS_TOML.md`).
  - Exit criteria: release/full profiles require explicit command allowlists and bridge binary digest pinning for plugin command surfaces, with fail-closed enforcement and conformance tests.

- [ ] P2.2 Expand GPU device conformance matrix across real hardware/OS combinations.
  - Evidence: current conformance artifacts show Apple M1 and deterministic CI adapter lanes, but not broad vendor/OS coverage in the tracked reports under `.genesis/perf/`.
  - Exit criteria: add enforced lane matrix for representative NVIDIA/AMD/Intel and Linux/macOS/Windows targets with parity contracts and adapter-specific artifact retention.

- [ ] P2.3 Continue documentation consolidation and reduce long-tail markdown surface.
  - Evidence: repository currently has `127` markdown files (`103` under `docs/`), and doc hygiene still tracks `deprecated_docs=7`.
  - Exit criteria: fold remaining split docs into canonical bundles/index entries, shrink redirect-stub surface, and keep doc-hygiene + planning-doc freshness green.

- [ ] P2.4 Add first-class cryptography capability family for secure agent-built applications.
  - Evidence: canonical host ABI op list in `docs/spec/HOST_ABI.md` has no `core/crypto::*` capability family.
  - Exit criteria: define deterministic crypto contracts (hash/sign/verify/KDF/AEAD envelopes as policy-gated host effects), add prelude wrappers, schema indices, replay tests, and safety guidance.
