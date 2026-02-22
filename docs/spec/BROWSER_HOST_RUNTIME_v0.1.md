> Bundle Entry: `docs/spec/HOST_RUNTIME_BUNDLE_v0.1.md`

# Browser Host Runtime v0.1

Status: normative for wasm-host/browser-aligned first-party runtime behavior.

## Goal

Define deterministic browser-oriented host capability behavior that can run identically in:

- native host CI/runtime (first-party runtime),
- WASI/wasm-host bridge profile lanes,
- browser wasm execution paths.

## Runtime Profile Contract

`caps.toml` per-op policy supports:

- `first_party_profile = "browser"` for `gfx/window::*`, `gfx/input::*`, `gfx/audio::*`.

Profile guarantees:

- deterministic event emission driven by effect log ordering,
- no ambient host-time/input dependence outside logged payload/response terms,
- stable backend/adapter identity fields:
  - `:backend = "browser-first-party-runtime"`
  - `:adapter = "browser-host"`

## Browser ABI Families

Baseline families:

- `browser/window::*`
  - `browser/window::open`
  - `browser/window::close`
  - `browser/window::info`
- `browser/input::*`
  - `browser/input::poll`
- `browser/audio::*`
  - `browser/audio::set-master`
  - `browser/audio::enqueue`
- `browser/storage::*`
  - `browser/storage::get`
  - `browser/storage::set`
  - `browser/storage::delete`

These ops are first-party deterministic by default and can be explicitly bridge-routed via per-op
bridge policy (`bridge_cmd` or `wasi_bridge_profile` response mapping).

## Determinism + Replay

- First-party browser runtime state is process-local deterministic state (window table, storage
  map, audio counters).
- `run` and `replay` must produce identical value hashes for the same program/log pair.
- WASI bridge-profile responses for browser families must preserve replay hash parity under
  `scripts/check_agent_workflow_runtime_parity.sh`.

