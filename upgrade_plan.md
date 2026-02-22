# GenesisCode Upgrade Plan - Red-Team Backlog (Unresolved Only)

Last updated: 2026-02-22

This file contains only unresolved findings from the latest red-team pass.
Completed items are intentionally removed.

Open checklist items: 5

## P1 - Productization blockers for "agent can build anything" readiness

- [ ] P1.4 Expand XR runtime surface beyond session/frame/input/haptics to production-grade AR/VR primitives.
  - Evidence: `docs/spec/XR_HOST_RUNTIME_v0.1.md` currently standardizes only six ops (`session-open`, `frame-poll`, `input-poll`, `haptics-pulse`, `submit-frame`, `session-close`).
  - Exit criteria: add canonical contracts/policies/wrappers/tests for anchors, hand tracking, hit-test/spatial mesh, and compositor/layer primitives with replay-stable envelopes.

- [ ] P1.5 Add browser-native WebXR device conformance lane (real runtime behavior), not bridge-envelope conformance alone.
  - Evidence: XR contract still identifies first-party adapter as `"xr-headless-sim"` and treats `"webxr-device"` as a bridge-routed lane (`docs/spec/XR_HOST_RUNTIME_v0.1.md`).
  - Exit criteria: add first-class browser runtime conformance fixture(s) exercising real WebXR session/frame/input/haptics behavior under deterministic capture/replay rules.

- [ ] P1.6 Expand typechecker inference/verification coverage for complex agent-authored programs.
  - Evidence: `docs/spec/TYPES.md` states v0.2 checker is conservative and treats unknown applications as `?`, reducing strong conformance guarantees.
  - Exit criteria: broaden inference coverage (collection/contract/effect-heavy patterns), reduce unknown leakage, and add parity tests proving stable behavior across rust/selfhost paths.

## P2 - Hardening, security posture, and ecosystem completeness

- [ ] P2.2 Expand GPU device conformance matrix across real hardware/OS combinations.
  - Evidence: current conformance artifacts show Apple M1 and deterministic CI adapter lanes, but not broad vendor/OS coverage in the tracked reports under `.genesis/perf/`.
  - Exit criteria: add enforced lane matrix for representative NVIDIA/AMD/Intel and Linux/macOS/Windows targets with parity contracts and adapter-specific artifact retention.

- [ ] P2.4 Add first-class cryptography capability family for secure agent-built applications.
  - Evidence: canonical host ABI op list in `docs/spec/HOST_ABI.md` has no `core/crypto::*` capability family.
  - Exit criteria: define deterministic crypto contracts (hash/sign/verify/KDF/AEAD envelopes as policy-gated host effects), add prelude wrappers, schema indices, replay tests, and safety guidance.
