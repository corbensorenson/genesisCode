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
  - canonical `gpu/compute::*` lifecycle (`create-*`, `write-buffer`, `read-buffer`, `destroy-resource`, `submit`, `limits`, `features`)
  - `gfx/gpu::*` lifecycle/data/submit/introspection lanes (`create-*`, `write-*`, `read-*`, `destroy-resource`, `submit-*`, `limits`, `features`)
  - `gfx/window::*`, `gfx/input::*`, `gfx/audio::*` (`headless` deterministic profile + `interactive` terminal-host adapter profile + `desktop` non-terminal adapter profile)
  - `editor/clipboard::*`, `editor/dialog::*`, `editor/watch::*`, `editor/task::*`
- Bridge-mediated runtime domains:
  - `io/net::dns-resolve` (policy-gated DNS lookup + bridge-backed execution)
  - `io/net::http-request` (policy-gated remote allowlist + bridge-backed execution)
  - `io/net::tcp-open`, `io/net::tcp-send`, `io/net::tcp-recv`, `io/net::tcp-close`
    (policy-gated TCP stream lifecycle + bridge-backed execution)
  - `io/net::udp-bind`, `io/net::udp-send`, `io/net::udp-recv`, `io/net::udp-close`
    (policy-gated UDP socket lifecycle + bridge-backed execution)
  - `io/net::ws-open`, `io/net::ws-send`, `io/net::ws-recv`, `io/net::ws-close`
    (policy-gated WebSocket stream lifecycle + bridge-backed execution)
  - `sys/process::*` (`exec|spawn|wait|kill|stdin-write|stdout-read|stderr-read`,
    policy-gated with program allowlists for launch ops and bridge-backed execution)
- Explicit per-op bridge policy (`bridge_cmd`, `bridge_args`, or WASI bridge response
  profile) overrides first-party backends and uses bridge transport.
- Bridge-mediated extension domains without first-party runtime:
  - `host/plugin::command` (generic host extension ABI)
  - `editor/plugin::command` (editor-domain wrapper over `host/plugin::command`)
  return deterministic sealed bridge errors when bridge policy is missing.
- Canonical compute ABI lives under `gpu/compute::*`.
  Legacy `gfx/gpu::create-compute-pipeline` and `gfx/gpu::submit-compute-graph`
  are compatibility aliases that normalize to canonical compute ops before dispatch.
- Under WASI profile, bridge-backed domains execute through deterministic response
  configuration (`wasi_bridge_response`, `wasi_bridge_response_file`, or
  `GENESIS_WASI_BRIDGE_RESPONSES`) instead of process spawning.
- Bridge transport framing and limits are normative in:
  `docs/spec/HOST_BRIDGE_PROTOCOL.md`.

## Stable Operation Surface (v0.2)

<!-- HOST_ABI_OPS_BEGIN -->
- `core/gc-low::pin`
- `core/gc-low::plan`
- `core/gc-low::purge`
- `core/gc-low::run`
- `core/gc-low::unpin`
- `core/gpk-low::export`
- `core/gpk-low::import`
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
- `io/fs::list`
- `io/fs::mkdir`
- `io/fs::read`
- `io/fs::remove`
- `io/fs::rename`
- `io/fs::stat`
- `io/fs::write`
- `io/net::dns-resolve`
- `io/net::http-request`
- `io/net::tcp-close`
- `io/net::tcp-open`
- `io/net::tcp-recv`
- `io/net::tcp-send`
- `io/net::udp-bind`
- `io/net::udp-close`
- `io/net::udp-recv`
- `io/net::udp-send`
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

## Network/Process Capability Contracts

- `io/net::http-request`
  - Required payload field: `:url` (string).
  - Policy-gated by per-op network controls (`url_allow`, `allow_http`, optional `wasi_network_profile`).
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
  - Required per-op policy controls:
    - `allow_plugins` (array<string>): explicit plugin allowlist.
  - Optional per-op policy controls:
    - `allow_commands` (array<string>): optional command allowlist.
  - Bridge execution:
    - same deterministic bridge framing contract as other bridge-backed domains (`docs/spec/HOST_BRIDGE_PROTOCOL.md`).
    - supports `bridge_cmd` / `bridge_args` and WASI bridge profile response controls.

- `editor/plugin::command`
  - Compatibility wrapper with editor-domain naming.
  - Uses the same payload/policy contract as `host/plugin::command`.
  - Preserves deterministic effect-log/replay behavior identical to generic host extension ops.
