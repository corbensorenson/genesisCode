> Bundle Entry: `docs/spec/HOST_RUNTIME_BUNDLE_v0.1.md`
> Legacy Split Doc: Prefer the bundle entrypoint for agent retrieval; this file retains detailed, topic-local semantics.

# Host Bridge Protocol v0.1

This document is normative for bridge-backed host capabilities:
- `editor/*`
- `gfx/*`
- `gpu/compute::*`
- `io/net::*`, `io/db::*`, `sys/process::*`, `core/crypto::*`
- `host/plugin::*`, `host/ffi::*`, and model-provider families

## Goals

- Deterministic request/response transport for host-integrated capabilities.
- Uniform policy enforcement (`bridge_cmd`, `bridge_args`, `timeout_ms`, `max_bytes`).
- Stable behavior across native and WASI host profiles.

## Invocation Contract

For a single capability request (`bridge_transport = "spawn-per-op"`):

1. Runner resolves and executes `bridge_cmd` under op `base_dir`.
2. Runner appends the requested op symbol as the final CLI arg.
3. Runner sets env vars:
   - `GENESIS_HOST_BRIDGE_OP`
   - `GENESIS_HOST_BRIDGE_FAMILY`
4. Runner writes one framed request payload to bridge stdin.
5. Bridge writes one framed response payload to stdout.

For persistent capability requests (`bridge_transport = "persistent-stdio"`):

1. Runner resolves and executes `bridge_cmd` under op `base_dir` once per deterministic session key within one runner execution.
2. Runner appends the requested op symbol as the final CLI arg.
3. Runner sets env vars:
   - `GENESIS_HOST_BRIDGE_OP`
   - `GENESIS_HOST_BRIDGE_FAMILY`
   - `GENESIS_HOST_BRIDGE_TRANSPORT=persistent-stdio`
4. Runner reuses the live bridge process and writes one framed request payload per op invocation.
5. Bridge writes one framed response payload per request and remains alive for the next frame.

A persistent session is owned by the current effect runner, never by a process-global cache. Its
key cannot carry a process, socket, database, GPU, graphics, plugin, FFI, or model session across
runner requests. Returning success or a sealed error from the runner, unwinding the runner,
cancelling its worker, or replacing/restarting the daemon drops the owner and closes every session.

`stderr` is reserved for diagnostics and is included in deterministic error mapping when the bridge exits non-zero.

## Framing (Normative)

Request and response payloads use UTF-8 CoreForm terms with text framing:

- Header: ASCII decimal byte length of payload text.
- Delimiter: single `\n`.
- Body: exact payload bytes (length must match header exactly).

Format:

`<len>\n<payload-bytes>`

Example:

`17\n{:ok true :id "x"}`

## Policy Enforcement

- `bridge_cmd` is required per op.
- `bridge_transport` is optional per op:
  - `spawn-per-op` (default)
  - `persistent-stdio` (session reuse)
- Optional bridge identity constraints:
  - `bridge_cmd_allowlist` (array<string>): explicit command identity allowlist.
    - entries may match `bridge_cmd` token, resolved absolute path, or executable filename.
  - `bridge_cmd_sha256` (string): expected executable digest (64 hex, optional `sha256:` prefix).
- `timeout_ms` is a hard process-tree deadline for both transports.
  - On hosts advertising process-tree termination support, `spawn-per-op` creates a separately killable process tree. Success, non-zero exit, protocol error, and timeout terminate residual descendants, reap the child, and join all I/O pumps before returning.
  - `persistent-stdio` timeout signals the process tree, closes the request channel, joins the sole worker that owns and reaps the child, verifies no process-group member remains, and evicts the session. It never retries the uncertain timed-out request.
- Hard bridge timeouts require platform process-tree termination support. Current Unix hosts use a dedicated process group per bridge tree. Other hosts fail closed with `<family>/bridge-policy` instead of advertising or attempting a cooperative timeout.
- `max_bytes` applies to both request payload size and response payload size.
- Violations return deterministic sealed errors with family-scoped codes:
  - `<family>/bridge-required`
  - `<family>/bridge-identity-denied`
  - `<family>/bridge-timeout`
  - `<family>/bridge-payload-too-large`
  - `<family>/bridge-response-too-large`
  - `<family>/bridge-parse`
  - `<family>/bridge-exit`

## Determinism

- Payload hashing and continuation hashing remain owned by the effect runner (`.gclog` semantics unchanged).
- Bridge transport errors are represented as sealed ERROR values and are replay-stable.

## Ownership And Teardown

- `HostBridgeRuntime` is the explicit owner for all persistent bridge sessions in one runner execution.
- The owner contains no ambient process-global session map. Production capability dispatch must receive the owner explicitly.
- A persistent worker exclusively owns its `Child`. Teardown must signal before join, let that owner reap the leader, then perform the bounded residual-group verification. Waiting for group disappearance before joining the child owner is forbidden because it misclassifies the owner's unreaped leader as a surviving process.
- Teardown is bounded. Failure to signal, join, reap, or eliminate a live residual member returns a family-scoped `bridge-reap` error; it is never rewritten as successful cancellation.
- Recreating a runner after daemon restart creates a fresh bridge generation. Logical IDs or processes from the retired owner cannot be reused.

## WASI Profile

- If bridge process execution is unavailable, runtime returns deterministic `*/bridge-not-supported`.
- WASI hosts that implement bridge transport must preserve the same framing and policy semantics.

## Conformance

Conformance tests:
- Native framing, owner lifetime, success/error descendant reap, timeout/cancellation, restart, and repeated-load tests: `crates/gc_effects/src/runner_host_bridge_tests.rs`.
- End-to-end bridge replay tests: `crates/gc_effects/tests/gfx_gpu_bridge.rs`, `crates/gc_effects/tests/editor_bridge.rs`.
- Mandatory aggregate gate and machine report: `scripts/check_host_bridge_fault_injection.sh` and `.genesis/perf/host_bridge_fault_injection_report.json`.
