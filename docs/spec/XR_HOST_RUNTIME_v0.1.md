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
- `gfx/xr::haptics-pulse`
- `gfx/xr::submit-frame`
- `gfx/xr::session-close`

These ops are first-party deterministic by default and can be explicitly bridge-routed via per-op bridge policy (`bridge_cmd` or `wasi_bridge_profile` response mapping).
In addition, a dedicated WebXR device lane is available via `xr_backend = "webxr-device"` with explicit bridge transport.

## Runtime Contract

First-party XR runtime exposes deterministic identity fields:

- `:backend = "xr-first-party-runtime"`
- `:adapter = "xr-headless-sim"`

WebXR device runtime lane (`xr_backend = "webxr-device"`) exposes:

- `:backend = "xr-webxr-device-runtime"`
- `:adapter = "webxr-device"` (or bridge-provided adapter if set)
- deterministic `:replay-envelope` map:
  - `:schema = :gfx/xr-webxr-device-replay-envelope.v1`
  - `:capture-seq` monotonically increasing deterministic capture index per runtime process
  - `:source = :webxr-device`
  - `:op` operation symbol
  - `:deterministic = true`

WebXR device lane policy requirements:

- per-op `xr_backend = "webxr-device"`
- explicit bridge profile (`bridge_cmd` or `wasi_bridge_profile` + deterministic bridge response mapping)
- fail-closed policy error if `xr_backend = "webxr-device"` is configured without bridge transport.
- canonical template: `docs/policies/xr_webxr_device_caps_v0.1.toml`

Session lifecycle contract:

- `session-open` returns a deterministic session id and normalized mode/reference-space metadata.
- `frame-poll` increments deterministic frame index and emits deterministic frame envelopes.
- `input-poll` emits deterministic controller/input envelopes with bounded `:max-inputs`.
- `haptics-pulse` applies deterministic bounded haptic intents for a session/input lane.
- `submit-frame` records deterministic accepted/submitted counters.
- `session-close` seals the session and deterministically rejects further use via stable error codes.

Haptics policy gate contract (`gfx/xr::haptics-pulse`):

- required per-op `allow_haptics_inputs = ["<input-id>" ...]` allowlist.
- optional per-op `max_haptics_amplitude` integer (`1..1000`, default `1000`).
- optional per-op `max_haptics_duration_ms` integer (`>0`, default `250`).
- requests outside policy bounds fail closed with deterministic `core/caps/policy-error` envelopes.

## Determinism + Replay

- First-party XR runtime state is process-local deterministic state (session table, frame counters, submit counters).
- `haptics-pulse` emits deterministic `:pulse-id` and cumulative `:submitted-haptics` counters.
- WebXR device lane emits deterministic per-op replay envelopes (`:replay-envelope`) that are hashed in run/replay equivalence checks.
- `run` and `replay` produce identical value hashes for the same program/log pair.
- Native/WASI lane parity is enforced by `scripts/check_agent_workflow_runtime_parity.sh` via the shared gauntlet workflow set.
