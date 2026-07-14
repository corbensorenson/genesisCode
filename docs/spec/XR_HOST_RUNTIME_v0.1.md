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
- `gfx/xr::hands-poll`
- `gfx/xr::hit-test`
- `gfx/xr::spatial-mesh-poll`
- `gfx/xr::anchor-create`
- `gfx/xr::anchor-update`
- `gfx/xr::anchor-destroy`
- `gfx/xr::layer-create`
- `gfx/xr::layer-update`
- `gfx/xr::layer-destroy`
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

Browser-native conformance lane:

- CI job: `webxr_browser_conformance` in `.github/workflows/ci.yml`
- checker: `scripts/check_webxr_browser_conformance.sh`
- runtime harness: `scripts/webxr_browser_conformance.mjs`
- artifact: `.genesis/perf/webxr_browser_conformance_report.json`
- explicit artifact producer: `scripts/update_webxr_browser_conformance_report.sh`
- CI invokes the explicit producer because it uploads the report; the checker always uses a
  private temporary output.
- conformance scope:
  - real browser `navigator.xr` session open (`inline`) + reference-space request
  - deterministic render-layer initialization (`XRWebGLLayer`) before frame probe
  - frame callback with bounded classification; functional pass requires `frame.status = ok`
  - real input-source snapshot from `session.inputSources`
  - real haptics attempt on available actuators, or deterministic `no-haptics-source` classification
  - session-close functional proof:
    - `session.end()` resolved close (`status = closed`), or
    - deterministic close-recovery proof (`status = closed-quiesced`) when browser runtime leaves
      `session.end()` unresolved but old-session frames quiesce and reopen+frame succeeds
  - deterministic replay assertion using capture hash equivalence across two independent runs

Session lifecycle contract:

- `session-open` returns a deterministic session id and normalized mode/reference-space metadata.
- `frame-poll` increments deterministic frame index and emits deterministic frame envelopes.
- `input-poll` emits deterministic controller/input envelopes with bounded `:max-inputs`.
- `hands-poll` emits deterministic left/right hand joint envelopes with policy-bounded joint counts.
- `hit-test` emits deterministic hit vectors and pose envelopes with policy-bounded result counts.
- `spatial-mesh-poll` emits deterministic mesh metadata envelopes with policy-bounded mesh/vertex counts.
- `anchor-create` / `anchor-update` / `anchor-destroy` maintain deterministic anchor lifecycle state per session.
- `layer-create` / `layer-update` / `layer-destroy` maintain deterministic compositor layer lifecycle state per session.
- `haptics-pulse` applies deterministic bounded haptic intents for a session/input lane.
- `submit-frame` records deterministic accepted/submitted counters.
- `session-close` seals the session and deterministically rejects further use via stable error codes.

Haptics policy gate contract (`gfx/xr::haptics-pulse`):

- required per-op `allow_haptics_inputs = ["<input-id>" ...]` allowlist.
- optional per-op `max_haptics_amplitude` integer (`1..1000`, default `1000`).
- optional per-op `max_haptics_duration_ms` integer (`>0`, default `250`).
- requests outside policy bounds fail closed with deterministic `core/caps/policy-error` envelopes.

Advanced XR policy gate contract:

- `gfx/xr::hands-poll`
  - optional per-op `allow_hand_tracking` (bool, default `true`)
  - optional per-op `max_hand_joints` (int > 0, default `25`)
- `gfx/xr::hit-test`
  - optional per-op `allow_hit_test` (bool, default `true`)
  - optional per-op `max_hit_results` (int > 0, default `8`)
- `gfx/xr::spatial-mesh-poll`
  - optional per-op `allow_spatial_mesh` (bool, default `true`)
  - optional per-op `max_meshes` (int > 0, default `4`)
  - optional per-op `max_mesh_vertices` (int > 0, default `4096`)
- `gfx/xr::anchor-create`
  - optional per-op `allow_anchor_spaces` (array<string>, default `["local","local-floor","bounded-floor","viewer"]`)
  - optional per-op `max_anchors` (int > 0, default `64`)
- `gfx/xr::layer-create` / `gfx/xr::layer-update`
  - optional per-op `allow_layer_types` (array<string>, default `["quad","cylinder","equirect"]`)
  - optional per-op `max_layers` (int > 0, default `16`)
  - optional per-op `max_layer_opacity` (int > 0, default `1000`)
- requests outside these policy bounds fail closed with deterministic `core/caps/policy-error` envelopes.

## Determinism + Replay

- First-party XR runtime state is process-local deterministic state (session table, frame counters, submit counters).
- `haptics-pulse` emits deterministic `:pulse-id` and cumulative `:submitted-haptics` counters.
- WebXR device lane emits deterministic per-op replay envelopes (`:replay-envelope`) that are hashed in run/replay equivalence checks.
- Browser-native WebXR lane enforces deterministic capture-replay equivalence:
  - `hash(run_a_capture) == hash(run_b_capture)`
  - replay rule persisted in `.genesis/perf/webxr_browser_conformance_report.json`.
- `run` and `replay` produce identical value hashes for the same program/log pair.
- Native/WASI lane parity is enforced by `scripts/check_agent_workflow_runtime_parity.sh` via the shared gauntlet workflow set.
