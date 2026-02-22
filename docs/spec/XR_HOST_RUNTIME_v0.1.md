> Bundle Entry: `docs/spec/HOST_RUNTIME_BUNDLE_v0.1.md`

# XR Host Runtime v0.1

Status: normative for deterministic XR runtime behavior.

## Goal

Define deterministic WebXR-aligned capability behavior that runs consistently across:

- native host CI/runtime (first-party XR runtime),
- WASI/wasm-host bridge profile lanes,
- browser-facing wasm execution paths.

## XR ABI Families

Baseline families:

- `gfx/xr::session-open`
- `gfx/xr::frame-poll`
- `gfx/xr::input-poll`
- `gfx/xr::submit-frame`
- `gfx/xr::session-close`

These ops are first-party deterministic by default and can be explicitly bridge-routed via per-op bridge policy (`bridge_cmd` or `wasi_bridge_profile` response mapping).

## Runtime Contract

First-party XR runtime exposes deterministic identity fields:

- `:backend = "xr-first-party-runtime"`
- `:adapter = "xr-headless-sim"`

Session lifecycle contract:

- `session-open` returns a deterministic session id and normalized mode/reference-space metadata.
- `frame-poll` increments deterministic frame index and emits deterministic frame envelopes.
- `input-poll` emits deterministic controller/input envelopes with bounded `:max-inputs`.
- `submit-frame` records deterministic accepted/submitted counters.
- `session-close` seals the session and deterministically rejects further use via stable error codes.

## Determinism + Replay

- First-party XR runtime state is process-local deterministic state (session table, frame counters, submit counters).
- `run` and `replay` produce identical value hashes for the same program/log pair.
- Native/WASI lane parity is enforced by `scripts/check_agent_workflow_runtime_parity.sh` via the shared gauntlet workflow set.
