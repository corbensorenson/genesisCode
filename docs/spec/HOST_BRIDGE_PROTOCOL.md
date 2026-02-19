# Host Bridge Protocol v0.1

This document is normative for bridge-backed host capabilities:
- `editor/*`
- `gfx/*`
- `gpu/compute::*`

## Goals

- Deterministic request/response transport for host-integrated capabilities.
- Uniform policy enforcement (`bridge_cmd`, `bridge_args`, `timeout_ms`, `max_bytes`).
- Stable behavior across native and WASI host profiles.

## Invocation Contract

For a single capability request:

1. Runner resolves and executes `bridge_cmd` under op `base_dir`.
2. Runner appends the requested op symbol as the final CLI arg.
3. Runner sets env vars:
   - `GENESIS_HOST_BRIDGE_OP`
   - `GENESIS_HOST_BRIDGE_FAMILY`
4. Runner writes one framed request payload to bridge stdin.
5. Bridge writes one framed response payload to stdout.

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
- `timeout_ms` applies to bridge execution.
- `max_bytes` applies to both request payload size and response payload size.
- Violations return deterministic sealed errors with family-scoped codes:
  - `<family>/bridge-required`
  - `<family>/bridge-timeout`
  - `<family>/bridge-payload-too-large`
  - `<family>/bridge-response-too-large`
  - `<family>/bridge-parse`
  - `<family>/bridge-exit`

## Determinism

- Payload hashing and continuation hashing remain owned by the effect runner (`.gclog` semantics unchanged).
- Bridge transport errors are represented as sealed ERROR values and are replay-stable.

## WASI Profile

- If bridge process execution is unavailable, runtime returns deterministic `*/bridge-not-supported`.
- WASI hosts that implement bridge transport must preserve the same framing and policy semantics.

## Conformance

Conformance tests:
- Native bridge framing + budget tests: `crates/gc_effects/src/runner_host_bridge.rs` test module.
- End-to-end bridge replay tests: `crates/gc_effects/tests/gfx_gpu_bridge.rs`, `crates/gc_effects/tests/editor_bridge.rs`.
