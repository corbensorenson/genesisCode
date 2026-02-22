# GenesisCode Upgrade Plan - Red-Team Backlog (Unresolved Only)

Last updated: 2026-02-22

This file contains only unresolved findings from the latest red-team pass.
Completed items are intentionally removed.

Open checklist items: 1

## P1 - Productization blockers for "agent can build anything" readiness

- [ ] P1.5 Add browser-native WebXR device conformance lane (real runtime behavior), not bridge-envelope conformance alone.
  - Evidence: XR contract still identifies first-party adapter as `"xr-headless-sim"` and treats `"webxr-device"` as a bridge-routed lane (`docs/spec/XR_HOST_RUNTIME_v0.1.md`).
  - Exit criteria: add first-class browser runtime conformance fixture(s) exercising real WebXR session/frame/input/haptics behavior under deterministic capture/replay rules.
