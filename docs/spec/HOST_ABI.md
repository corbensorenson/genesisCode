> Bundle Entry: `docs/spec/HOST_RUNTIME_BUNDLE_v0.1.md`
> Legacy Split Doc: Prefer the bundle entrypoint for agent retrieval; this file retains detailed, topic-local semantics.

# Host ABI (v0.2)

This document defines the stable host capability ABI for GenesisCode v0.2.

Scope:
- This ABI covers the effect operation surface implemented by `gc_effects`.
- Kernel semantics remain out of scope for this ABI and are covered separately by kernel/coreform specs.

Rules:
- The operation surface is deny-by-default and policy-gated (`caps.toml`).
- Unknown operations must return deterministic sealed `core/caps/unknown-op` errors.
- Stable host-integrated ops without an available backend path must return deterministic
  sealed `core/caps/backend-unavailable` errors with actionable bridge/runtime guidance.
- Any ABI surface change requires updating this file and passing the host ABI conformance guard in CI.

Compatibility notes:
- `core/sync::*` is part of the ABI surface and is enforced by explicit WASI remote profiles (`none|local|preview2`), deny-by-default.
- Adding or removing an op is a versioned ABI change and must be reflected in release notes.
- Host-integrated runtime domains now support first-party backends by default:
  - `core/media::*` deterministic in-process asset hashing + image/audio transcode lanes
    (`asset-hash`, `image-transcode`, `audio-transcode`) with explicit policy bounds
  - canonical `gpu/compute::*` lifecycle (`create-*`, `write-buffer`, `read-buffer`, `destroy-resource`, `submit`, `limits`, `features`)
  - `gfx/gpu::*` lifecycle/data/submit/introspection lanes (`create-*`, `write-*`, `read-*`, `destroy-resource`, `submit-*`, `limits`, `features`)
  - `gfx/window::*`, `gfx/input::*`, `gfx/audio::*` (`headless` deterministic profile + `interactive` terminal-host adapter profile + `desktop` non-terminal adapter profile + `browser` wasm-host/browser profile)
  - `gfx/xr::*` (`session-open`, `frame-poll`, `input-poll`, `haptics-pulse`, `submit-frame`, `session-close`) with deterministic first-party session/frame/input/haptics semantics plus dedicated `xr_backend = "webxr-device"` bridge lane for device capture envelopes
  - `browser/window::*`, `browser/input::*`, `browser/audio::*`, `browser/storage::*` (deterministic browser host runtime baseline; explicit bridge policy may override)
  - `editor/clipboard::*`, `editor/dialog::*`, `editor/watch::*`, `editor/task::*`
- Bridge-mediated runtime domains:
  - `io/db::connect`, `io/db::tx-begin`, `io/db::query`, `io/db::exec`, `io/db::tx-commit`, `io/db::tx-rollback`
    (policy-gated durable SQL lifecycle/query execution + bridge-backed execution)
  - `io/db::kv-open`, `io/db::kv-get`, `io/db::kv-put`, `io/db::kv-delete`
    (policy-gated durable key/value lifecycle + bridge-backed execution)
  - `io/net::dns-resolve` (policy-gated DNS lookup + bridge-backed execution)
  - `io/net::http-listen` (policy-gated inbound HTTP listener bind + request-size bounds + bridge-backed execution)
  - `io/net::http-request` (policy-gated remote allowlist + bridge-backed execution)
  - `io/net::http-respond` (bridge-backed HTTP response emit for inbound listener flows)
  - `io/net::tcp-listen`, `io/net::tcp-accept`, `io/net::tcp-open`, `io/net::tcp-send`, `io/net::tcp-recv`, `io/net::tcp-close`
    (policy-gated TCP stream lifecycle + bridge-backed execution)
  - `io/net::udp-bind`, `io/net::udp-send`, `io/net::udp-recv`, `io/net::udp-close`
    (policy-gated UDP socket lifecycle + bridge-backed execution)
  - `io/net::ws-open`, `io/net::ws-accept`, `io/net::ws-send`, `io/net::ws-recv`, `io/net::ws-close`
    (policy-gated WebSocket stream lifecycle + bridge-backed execution)
  - `sys/process::*` (`exec|spawn|wait|kill|stdin-write|stdout-read|stderr-read`,
    policy-gated with program allowlists for launch ops and bridge-backed execution)
- Explicit per-op bridge policy (`bridge_cmd`, `bridge_args`, or WASI bridge response
  profile) overrides first-party backends and uses bridge transport.
- Bridge-mediated extension domains without first-party runtime:
  - `host/plugin::command` (generic host extension ABI)
  - `editor/plugin::command` (editor-domain wrapper over `host/plugin::command`)
  return deterministic sealed bridge errors when bridge policy is missing.
- Canonical compute ABI lives under `gpu/compute::*`; graphics and compute
  capabilities are decoupled surfaces in production runtime paths.
- Under WASI profile, bridge-backed domains execute through deterministic response
  configuration (`wasi_bridge_response`, `wasi_bridge_response_file`, or
  `GENESIS_WASI_BRIDGE_RESPONSES`) instead of process spawning.
- Bridge transport framing and limits are normative in:
  `docs/spec/HOST_BRIDGE_PROTOCOL.md`.

## Stable Operation Surface (v0.2)

<!-- HOST_ABI_OPS_BEGIN -->
- `browser/audio::enqueue`
- `browser/audio::set-master`
- `browser/input::poll`
- `browser/storage::delete`
- `browser/storage::get`
- `browser/storage::set`
- `browser/window::close`
- `browser/window::info`
- `browser/window::open`
- `core/gc-low::pin`
- `core/gc-low::plan`
- `core/gc-low::purge`
- `core/gc-low::run`
- `core/gc-low::unpin`
- `core/gpk-low::export`
- `core/gpk-low::import`
- `core/media::asset-hash`
- `core/media::audio-transcode`
- `core/media::image-transcode`
- `core/pkg-low::add`
- `core/pkg-low::info`
- `core/pkg-low::init`
- `core/pkg-low::install`
- `core/pkg-low::list`
- `core/pkg-low::load-lock`
- `core/pkg-low::load-package`
- `core/pkg-low::lock`
- `core/pkg-low::publish`
- `core/pkg-low::save-lock`
- `core/pkg-low::snapshot`
- `core/pkg-low::update`
- `core/pkg-low::verify`
- `core/refs::delete`
- `core/refs::get`
- `core/refs::list`
- `core/refs::set`
- `core/store::get`
- `core/store::has`
- `core/store::put`
- `core/store::verify`
- `core/sync::pull`
- `core/sync::push`
- `core/task::await`
- `core/task::cancel`
- `core/task::channel-close`
- `core/task::channel-open`
- `core/task::channel-recv`
- `core/task::channel-send`
- `core/task::channel-status`
- `core/task::scope`
- `core/task::spawn`
- `core/task::status`
- `core/vcs-low::apply`
- `core/vcs-low::apply-patch`
- `core/vcs-low::blame`
- `core/vcs-low::diff`
- `core/vcs-low::diff-terms`
- `core/vcs-low::log`
- `core/vcs-low::merge3`
- `core/vcs-low::merge3-contract-snapshots`
- `core/vcs-low::resolve-conflict`
- `core/vcs-low::why`
- `editor/clipboard::get`
- `editor/clipboard::set`
- `editor/dialog::open`
- `editor/dialog::save`
- `editor/plugin::command`
- `editor/task::cancel`
- `editor/task::fmt-coreform`
- `editor/task::lint-module`
- `editor/task::optimize-module`
- `editor/task::parse-module`
- `editor/task::poll`
- `editor/task::spawn`
- `editor/task::test-pkg`
- `editor/task::typecheck-pkg`
- `editor/watch::poll`
- `editor/watch::subscribe`
- `editor/watch::unsubscribe`
- `gfx/audio::enqueue`
- `gfx/audio::set-master`
- `gfx/gpu::create-bind-group`
- `gfx/gpu::create-bind-group-layout`
- `gfx/gpu::create-buffer`
- `gfx/gpu::create-pipeline-layout`
- `gfx/gpu::create-render-pipeline`
- `gfx/gpu::create-sampler`
- `gfx/gpu::create-shader-module`
- `gfx/gpu::create-texture`
- `gfx/gpu::destroy-resource`
- `gfx/gpu::features`
- `gfx/gpu::limits`
- `gfx/gpu::read-buffer`
- `gfx/gpu::read-texture`
- `gfx/gpu::submit-frame-graph`
- `gfx/gpu::write-buffer`
- `gfx/gpu::write-texture`
- `gfx/input::poll-events`
- `gfx/input::set-cursor-mode`
- `gfx/time::frame-tick`
- `gfx/window::create-surface`
- `gfx/window::request-redraw`
- `gfx/window::resize-surface`
- `gfx/window::set-title`
- `gfx/window::surface-info`
- `gfx/xr::frame-poll`
- `gfx/xr::haptics-pulse`
- `gfx/xr::input-poll`
- `gfx/xr::session-close`
- `gfx/xr::session-open`
- `gfx/xr::submit-frame`
- `gpu/compute::create-bind-group`
- `gpu/compute::create-bind-group-layout`
- `gpu/compute::create-buffer`
- `gpu/compute::create-compute-pipeline`
- `gpu/compute::create-kernel`
- `gpu/compute::create-pipeline-layout`
- `gpu/compute::create-shader-module`
- `gpu/compute::destroy-resource`
- `gpu/compute::features`
- `gpu/compute::limits`
- `gpu/compute::read-buffer`
- `gpu/compute::submit`
- `gpu/compute::write-buffer`
- `host/plugin::command`
- `io/db::connect`
- `io/db::exec`
- `io/db::kv-delete`
- `io/db::kv-get`
- `io/db::kv-open`
- `io/db::kv-put`
- `io/db::query`
- `io/db::tx-begin`
- `io/db::tx-commit`
- `io/db::tx-rollback`
- `io/fs::list`
- `io/fs::mkdir`
- `io/fs::read`
- `io/fs::remove`
- `io/fs::rename`
- `io/fs::stat`
- `io/fs::write`
- `io/net::dns-resolve`
- `io/net::http-listen`
- `io/net::http-request`
- `io/net::http-respond`
- `io/net::tcp-accept`
- `io/net::tcp-close`
- `io/net::tcp-listen`
- `io/net::tcp-open`
- `io/net::tcp-recv`
- `io/net::tcp-send`
- `io/net::udp-bind`
- `io/net::udp-close`
- `io/net::udp-recv`
- `io/net::udp-send`
- `io/net::ws-accept`
- `io/net::ws-close`
- `io/net::ws-open`
- `io/net::ws-recv`
- `io/net::ws-send`
- `sys/process::exec`
- `sys/process::kill`
- `sys/process::spawn`
- `sys/process::stderr-read`
- `sys/process::stdin-write`
- `sys/process::stdout-read`
- `sys/process::wait`
- `sys/time::now`
<!-- HOST_ABI_OPS_END -->

## Conformance

CI must run `scripts/check_host_abi_conformance.sh`, which diffs this op list against the dispatch surface in `crates/gc_effects/src/runner_capability_dispatch.rs`.

Machine-readable indices for agent planning:

- `docs/spec/HOST_ABI_INDEX_v0.1.json` (derived from Rust dispatch sources)
- `docs/spec/HOST_ABI_SCHEMA_INDEX_v0.1.json` (derived per-op payload/response contracts)
- `docs/spec/PRELUDE_CAPABILITY_INDEX_v0.1.json` (derived from prelude `core/caps::perform` wrappers)

CI drift check:

- `scripts/check_capability_indices.sh`

## Browser Host Capability Contracts

- `browser/window::open`
  - Optional payload: `:opts` map (`:width` int, `:height` int, `:title` string, `:visible` bool).
  - First-party runtime returns deterministic `:window-id` plus window metadata.
- `browser/window::close`
  - Required payload field: `:window-id` (string).
- `browser/window::info`
  - Required payload field: `:window-id` (string).
- `browser/input::poll`
  - Required payload field: `:window-id` (string).
  - Optional payload field: `:max-events` (int).
  - First-party runtime emits deterministic browser event envelopes (`:animation-frame`).
- `browser/audio::set-master`
  - Optional payload field: `:gain` (int, defaults to `1`).
- `browser/audio::enqueue`
  - Payload map accepted; first-party runtime increments deterministic queue counters.
- `browser/storage::set`
  - Required payload fields: `:key` (string), `:value` (term).
- `browser/storage::get`
  - Required payload field: `:key` (string).
  - Response map includes `:found` bool and `:value` term|nil.
- `browser/storage::delete`
  - Required payload field: `:key` (string).
  - Response map includes `:deleted` bool.

## XR Host Capability Contracts

- `gfx/xr::session-open`
  - Optional payload field: `:opts` map (`:mode` string/symbol, `:reference-space` string/symbol, `:app` string/symbol).
  - First-party runtime returns deterministic `:session-id` with normalized mode/reference-space metadata.
  - Optional per-op policy: `xr_backend = "webxr-device"` to force explicit bridge transport and WebXR device replay envelopes.
- `gfx/xr::frame-poll`
  - Required payload field: `:session-id` (string).
  - Response map includes deterministic frame envelopes (`:frame-index`, `:predicted-display-time-ms`, stereo `:views`).
- `gfx/xr::input-poll`
  - Required payload field: `:session-id` (string).
  - Optional payload field: `:max-inputs` (int).
  - Response map includes deterministic bounded input/controller vector under `:inputs`.
- `gfx/xr::haptics-pulse`
  - Required payload fields: `:session-id` (string), `:input-id` (string), `:amplitude` (int), `:duration-ms` (int).
  - Policy-gated by per-op XR haptics controls (`allow_haptics_inputs`, optional `max_haptics_amplitude`, optional `max_haptics_duration_ms`).
  - Response map includes deterministic `:pulse-id`, accepted pulse metadata, and cumulative `:submitted-haptics`.
  - Execution path is first-party deterministic by default; explicit bridge profile may override transport.
  - When `xr_backend = "webxr-device"` is set, explicit bridge transport is required and responses include deterministic `:replay-envelope` metadata (`:schema`, `:capture-seq`, `:source`, `:op`, `:deterministic`).
- `gfx/xr::submit-frame`
  - Required payload fields: `:session-id` (string), `:frame` (map).
  - Response map includes deterministic submit acceptance metadata and cumulative `:submitted-frames`.
- `gfx/xr::session-close`
  - Required payload field: `:session-id` (string).
  - Response map includes deterministic `:closed` flag; subsequent use is rejected via stable XR error codes.

## Network/Process Capability Contracts

- `io/db::connect`
  - Required payload field: `:target` (string DSN/path-like target).
  - Policy-gated by per-op durable-data controls (`db_target_allow`).
  - Execution path is bridge-backed (`bridge_cmd` or WASI bridge profile response config).
- `io/db::tx-begin`
  - Required payload field: `:connection-id` (string).
  - Execution path is bridge-backed (`bridge_cmd` or WASI bridge profile response config).
- `io/db::query`
  - Required payload fields: `:connection-id` (string), `:query-class` (string/symbol), `:query` (string).
  - Policy-gated by query controls (`allow_query_classes`, `max_row_count`, `max_result_bytes`).
  - Runner injects `:max-row-count` and `:max-result-bytes` into the bridge payload from policy.
  - Execution path is bridge-backed (`bridge_cmd` or WASI bridge profile response config).
- `io/db::exec`
  - Required payload fields: `:connection-id` (string), `:query-class` (string/symbol), `:statement` (string).
  - Policy-gated by query controls (`allow_query_classes`, `max_result_bytes`).
  - Runner injects `:max-result-bytes` into the bridge payload from policy.
  - Execution path is bridge-backed (`bridge_cmd` or WASI bridge profile response config).
- `io/db::tx-commit`
  - Required payload field: `:tx-id` (string).
  - Execution path is bridge-backed (`bridge_cmd` or WASI bridge profile response config).
- `io/db::tx-rollback`
  - Required payload field: `:tx-id` (string).
  - Execution path is bridge-backed (`bridge_cmd` or WASI bridge profile response config).
- `io/db::kv-open`
  - Required payload field: `:target` (string DSN/path-like target).
  - Policy-gated by per-op durable-data controls (`db_target_allow`).
  - Execution path is bridge-backed (`bridge_cmd` or WASI bridge profile response config).
- `io/db::kv-get`
  - Required payload fields: `:store-id` (string), `:key` (string).
  - Policy-gated by per-op result bound (`max_result_bytes`).
  - Runner injects `:max-result-bytes` into the bridge payload from policy.
  - Execution path is bridge-backed (`bridge_cmd` or WASI bridge profile response config).
- `io/db::kv-put`
  - Required payload fields: `:store-id` (string), `:key` (string), `:value` (term).
  - Policy-gated by per-op value bound (`max_value_bytes`).
  - Runner injects `:max-value-bytes` into the bridge payload from policy.
  - Execution path is bridge-backed (`bridge_cmd` or WASI bridge profile response config).
- `io/db::kv-delete`
  - Required payload fields: `:store-id` (string), `:key` (string).
  - Execution path is bridge-backed (`bridge_cmd` or WASI bridge profile response config).
- `io/net::http-request`
  - Required payload field: `:url` (string).
  - Policy-gated by per-op network controls (`url_allow`, `allow_http`, optional `wasi_network_profile`).
  - Execution path is bridge-backed (`bridge_cmd` or WASI bridge profile response config).
- `io/net::http-listen`
  - Required payload field: `:local` (string URL-like target, e.g. `http://127.0.0.1:8080`).
  - Policy-gated by per-op network controls (`url_allow`, `allow_http`, optional `wasi_network_profile`) plus inbound bind controls (`allow_bind_hosts`, `allow_bind_ports`) and request-size bound (`max_request_bytes`).
  - Runner injects `:max-request-bytes` into the bridge payload from policy.
  - Execution path is bridge-backed (`bridge_cmd` or WASI bridge profile response config).
- `io/net::http-respond`
  - Required payload fields: `:listener-id` (string), `:request-id` (string), `:status` (int).
  - Optional payload fields: `:headers` (map/vector), `:body` (bytes/string).
  - Execution path is bridge-backed (`bridge_cmd` or WASI bridge profile response config).
- `io/net::tcp-listen`
  - Required payload field: `:local` (string URL-like target, e.g. `tcp://127.0.0.1:9000`).
  - Policy-gated by per-op network controls (`url_allow`, optional `wasi_network_profile`) plus inbound bind controls (`allow_bind_hosts`, `allow_bind_ports`).
  - Execution path is bridge-backed (`bridge_cmd` or WASI bridge profile response config).
- `io/net::tcp-accept`
  - Required payload field: `:listener-id` (string).
  - Policy requires `max_request_bytes`; runner injects `:max-request-bytes` into the bridge payload from policy.
  - Execution path is bridge-backed (`bridge_cmd` or WASI bridge profile response config).
- `io/net::tcp-open`
  - Required payload field: `:remote` (string URL-like target, e.g. `tcp://host:port`).
  - Policy-gated by per-op network controls (`url_allow`, `allow_http`, optional `wasi_network_profile`).
  - Execution path is bridge-backed (`bridge_cmd` or WASI bridge profile response config).
- `io/net::tcp-send`
  - Required payload fields: `:stream-id` (string), `:data` (term; usually bytes/string).
  - Execution path is bridge-backed (`bridge_cmd` or WASI bridge profile response config).
- `io/net::tcp-recv`
  - Required payload field: `:stream-id` (string).
  - Execution path is bridge-backed (`bridge_cmd` or WASI bridge profile response config).
- `io/net::tcp-close`
  - Required payload field: `:stream-id` (string).
  - Execution path is bridge-backed (`bridge_cmd` or WASI bridge profile response config).
- `io/net::udp-bind`
  - Required payload field: `:local` (string URL-like target, e.g. `udp://ip:port`).
  - Policy-gated by per-op network controls (`url_allow`, `allow_http`, optional `wasi_network_profile`).
  - Execution path is bridge-backed (`bridge_cmd` or WASI bridge profile response config).
- `io/net::udp-send`
  - Required payload fields: `:socket-id` (string), `:remote` (string URL-like target), `:data` (term; usually bytes/string).
  - Policy-gated by per-op network controls (`url_allow`, `allow_http`, optional `wasi_network_profile`).
  - Execution path is bridge-backed (`bridge_cmd` or WASI bridge profile response config).
- `io/net::udp-recv`
  - Required payload field: `:socket-id` (string).
  - Execution path is bridge-backed (`bridge_cmd` or WASI bridge profile response config).
- `io/net::udp-close`
  - Required payload field: `:socket-id` (string).
  - Execution path is bridge-backed (`bridge_cmd` or WASI bridge profile response config).
- `io/net::dns-resolve`
  - Required payload field: `:name` (string DNS name).
  - Policy-gated by per-op network controls (`url_allow`, `allow_http`, optional `wasi_network_profile`).
  - Execution path is bridge-backed (`bridge_cmd` or WASI bridge profile response config).
- `io/net::ws-open`
  - Required payload field: `:url` (string, typically `wss://...`).
  - Policy-gated by per-op network controls (`url_allow`, `allow_http`, optional `wasi_network_profile`).
  - Execution path is bridge-backed (`bridge_cmd` or WASI bridge profile response config).
- `io/net::ws-accept`
  - Required payload fields: `:listener-id` (string), `:request-id` (string).
  - Policy requires `max_request_bytes`; runner injects `:max-request-bytes` into the bridge payload from policy.
  - Execution path is bridge-backed (`bridge_cmd` or WASI bridge profile response config).
- `io/net::ws-send`
  - Required payload fields: `:stream-id` (string), `:data` (term; typically bytes/string).
  - Execution path is bridge-backed (`bridge_cmd` or WASI bridge profile response config).
- `io/net::ws-recv`
  - Required payload field: `:stream-id` (string).
  - Execution path is bridge-backed (`bridge_cmd` or WASI bridge profile response config).
- `io/net::ws-close`
  - Required payload field: `:stream-id` (string).
  - Execution path is bridge-backed (`bridge_cmd` or WASI bridge profile response config).
- `sys/process::exec`
  - Required payload field: `:program` (string).
  - Policy-gated by per-op `allow_programs` allowlist.
  - Execution path is bridge-backed (`bridge_cmd` or WASI bridge profile response config).
- `sys/process::spawn`
  - Required payload field: `:program` (string).
  - Policy-gated by per-op `allow_programs` allowlist.
  - Execution path is bridge-backed (`bridge_cmd` or WASI bridge profile response config).
- `sys/process::wait`
  - Required payload field: `:process-id` (string).
  - Execution path is bridge-backed (`bridge_cmd` or WASI bridge profile response config).
- `sys/process::kill`
  - Required payload field: `:process-id` (string).
  - Execution path is bridge-backed (`bridge_cmd` or WASI bridge profile response config).
- `sys/process::stdin-write`
  - Required payload fields: `:process-id` (string), `:data` (term; typically bytes/string).
  - Execution path is bridge-backed (`bridge_cmd` or WASI bridge profile response config).
- `sys/process::stdout-read`
  - Required payload field: `:process-id` (string).
  - Execution path is bridge-backed (`bridge_cmd` or WASI bridge profile response config).
- `sys/process::stderr-read`
  - Required payload field: `:process-id` (string).
  - Execution path is bridge-backed (`bridge_cmd` or WASI bridge profile response config).

## Filesystem Capability Contracts

- `io/fs::read`
  - Required payload field: `:path` (string).
  - Optional per-op policy controls: `base_dir`, `max_bytes`.
  - Deterministic semantics: returns `bytes` data or sealed io/resource-limit errors; replay uses logged responses.
- `io/fs::write`
  - Required payload fields: `:path` (string), `:data` (bytes/string term).
  - Optional per-op policy controls: `base_dir`, `create_dirs`.
  - Deterministic semantics: returns `nil` on success; replay uses logged responses.
- `io/fs::stat`
  - Required payload field: `:path` (string).
  - Optional per-op policy controls: `base_dir`.
  - Response envelope: map with `:path`, `:exists`, `:kind`, `:len-bytes`, `:readonly`.
- `io/fs::list`
  - Required payload field: `:path` (string directory).
  - Optional per-op policy controls: `base_dir`.
  - Response envelope: vector of maps with `:name`, `:path`, `:kind`, `:len-bytes` sorted deterministically.
- `io/fs::mkdir`
  - Required payload field: `:path` (string).
  - Optional payload field: `:parents` (bool, default `true`).
  - Optional per-op policy controls: `base_dir`.
  - Deterministic semantics: returns `nil` on success.
- `io/fs::remove`
  - Required payload field: `:path` (string).
  - Optional payload field: `:recursive` (bool, default `false`).
  - Optional per-op policy controls: `base_dir`.
  - Deterministic semantics: returns `nil` on success; missing path is a no-op.
- `io/fs::rename`
  - Required payload fields: `:from` (string), `:to` (string).
  - Optional payload field: `:overwrite` (bool, default `false`).
  - Optional per-op policy controls: `base_dir`, `create_dirs`.
  - Deterministic semantics: returns `nil` on success.

## Media Capability Contracts

- `core/media::asset-hash`
  - Required payload field: `:data` (bytes|string).
  - Optional payload fields: `:algorithm` (string/symbol, currently `blake3` only), `:kind` (string/symbol metadata).
  - Optional per-op policy controls: `max_input_bytes`.
  - Deterministic semantics: returns stable hash envelope `{:ok true :algorithm "blake3" :hash <hex64> :bytes <int> ...}`.
- `core/media::image-transcode`
  - Required payload fields: `:data` (bytes|string), `:source-format` (string/symbol), `:target-format` (string/symbol), `:width` (int), `:height` (int).
  - Supported formats: `rgba8`, `gray8`.
  - Optional per-op policy controls:
    - `allow_source_formats`, `allow_target_formats` (string arrays)
    - `max_input_bytes`, `max_output_bytes`, `max_pixels` (positive integers)
  - Deterministic semantics: policy-gated format conversion with stable grayscale coefficients and deterministic hash/byte metadata in response.
- `core/media::audio-transcode`
  - Required payload fields: `:data` (bytes|string), `:source-format` (string/symbol), `:target-format` (string/symbol), `:channels` (int), `:sample-rate` (int).
  - Supported formats: `pcm-s16le`, `pcm-f32le`.
  - Optional per-op policy controls:
    - `allow_source_formats`, `allow_target_formats` (string arrays)
    - `max_input_bytes`, `max_output_bytes`, `max_frames` (positive integers)
    - `min_sample_rate`, `max_sample_rate` (positive integers)
  - Deterministic semantics: stable numeric conversion + clamp rules, bounded frame/sample-rate policy checks, and stable hash/byte metadata in response.

Determinism:
- Run-time responses for these ops are effect-logged as normal capability outcomes.
- Replay uses logged responses and does not re-invoke host network/process side effects.

## Host Extension Capability Contract

- `host/plugin::command`
  - Required payload fields:
    - `:plugin` (string or symbol)
    - `:command` (string or symbol)
  - Optional payload field:
    - `:payload` (arbitrary CoreForm term, forwarded to bridge)
    - `:request-schema-id` (string or symbol, alias `:request-schema`)
    - `:response-schema-id` (string or symbol, alias `:response-schema`)
      - when either schema field is present, request/response are validated against
        schema-id contracts in `docs/spec/PLUGIN_ABI_SCHEMAS_v0.1.md`.
  - Required per-op policy controls:
    - `allow_plugins` (array<string>): explicit plugin allowlist.
    - `allow_commands` (array<string>): explicit command allowlist.
  - `allow_schema_ids` (array<string>): required when typed schema ids are used;
      every request/response schema id must be allowlisted.
  - Bridge hardening controls:
    - when `bridge_cmd` transport is configured for plugin ops, `bridge_cmd_sha256` is required and enforced fail-closed.
    - `wasi_bridge_profile` transport does not require bridge binary pinning because no host executable is spawned.
  - Bridge execution:
    - same deterministic bridge framing contract as other bridge-backed domains (`docs/spec/HOST_BRIDGE_PROTOCOL.md`).
    - supports `bridge_cmd` / `bridge_args` and WASI bridge profile response controls.

- `editor/plugin::command`
  - Compatibility wrapper with editor-domain naming.
  - Uses the same payload/policy contract as `host/plugin::command`.
  - Preserves deterministic effect-log/replay behavior identical to generic host extension ops.
