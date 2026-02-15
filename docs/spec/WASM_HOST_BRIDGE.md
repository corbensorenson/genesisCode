# WASM Host Bridge (Effectful Stepping) v0.2

This document is normative for running effectful GenesisCode programs when the kernel is hosted in WASM.

## Goals

- Keep the kernel pure and deterministic.
- Allow effectful programs to run in WASM by delegating capabilities to the host (Node/browser/other).
- Preserve auditability:
  - effect requests and responses are hashed deterministically
  - effect logs are replayable and equivalent to the native runner
- Preserve protocol hardening:
  - protocol seal tokens are never exposed to untrusted code
  - the host cannot forge sealed ERROR/EFFECT/UNHANDLED values via source terms

## Scope

In-scope:
- a step/resume interface for running `EffectProgram` values produced by evaluation
- deterministic request/response hashing compatible with `docs/spec/VALUE_EFFECT_HASH.md`
- deterministic printing/serialization for payloads and responses as CoreForm terms

Out of scope:
- a specific JS/TS SDK design
- bundling/distribution details (handled in the CLI and packaging plan)

## Terminology

- **Kernel**: `gc_coreform` + `gc_kernel` + `gc_prelude`, compiled to WASM.
- **Host**: the embedding runtime (Node/browser/native) that provides capabilities and policy decisions.
- **Effect step**: advancing an `EffectProgram` until it is `Pure(v)` or yields an effect request.

## Data Encoding Across The Boundary

All payloads and responses are represented as **CoreForm terms** serialized with the canonical printer:
- The host passes a CoreForm **term string** to the WASM module as input for responses.
- The WASM module returns CoreForm **term strings** for payloads and final values.

Terms are required to parse as a *single* term (no trailing tokens).

## Hashing (Normative)

The WASM runtime MUST compute hashes exactly as the native runner.

### Payload hash

Given an effect request payload datum `payload: Term`:
- `payload_h = hash_term(payload)` (see `docs/spec/COREFORM_CANON_HASH.md`).

### Continuation hash

Given an effect request continuation closure `k: Value`:
- `cont_h = value_hash(k)` (see `docs/spec/VALUE_EFFECT_HASH.md`).

### Request hash

Given:
- `op: String`
- `payload_h: [u8; 32]`
- `cont_h: [u8; 32]`

Compute:
- `req_h = BLAKE3("GCv0.2\\0effect-req\\0" || op || "\\0" || payload_h || cont_h)`.

### Response hash

The response is a kernel `Value` (not a Term):
- for data responses supplied by the host, the kernel wraps the parsed term as `Value::Data(term)`
- for denied capabilities or host-signaled failures, the kernel constructs a **sealed ERROR** value internally

Then:
- `resp_h = value_hash(resp_val)`.

## Step/Resume Interface (Normative Behavior)

The WASM runtime exposes a stateful stepping API (shape is language-binding-specific, e.g. `wasm-bindgen` class).

### 1) Load + Evaluate

Input:
- CoreForm module bytes `src` (string)
- evaluation limits (step/memory limits as supported by the kernel)

Behavior:
- parse + canonicalize the module
- evaluate with the embedded prelude
- if the result is not an `EffectProgram`, the runtime is considered **done**
- if the result is an `EffectProgram`, the runtime transitions into **effect stepping** state

### 2) Step

If the current state is:
- `Pure(v)`: return `done` with the final value term string and `value_h`
- `Perform(sealed_req)`:
  - unseal under `S_EFFECT` and extract `EffectRequest { op, payload, k }`
  - compute `payload_h`, `cont_h`, `req_h`
  - return `effect` with:
    - `op` (string)
    - `payload` (canonical term string)
    - `payload_h` (32-byte hash string)
    - `cont_h` (32-byte hash string)
    - `req_h` (32-byte hash string)

### 3) Resume With Host Response

The host MUST respond using one of:
- **data response**: a CoreForm term string (parsed and wrapped as `Value::Data`)
- **deny**: signal denial for this `op` (kernel constructs `core/caps/denied` sealed ERROR)
- **error**: kernel constructs a sealed ERROR from structured fields (`:error/code`, `:error/message`, optional context)

Behavior:
- compute `resp_h`
- apply the captured continuation `k` to the response `resp_val`
- the result MUST be an `EffectProgram` (or a kernel ERROR is produced)
- transition to the new current program and continue stepping

## Equivalence Requirement

For the same program/module and the same sequence of host decisions + responses:
- the sequence of `(payload_h, cont_h, req_h, resp_h)` MUST match native runs
- replay checking MUST succeed when given a log recorded from either host

