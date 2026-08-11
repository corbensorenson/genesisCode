# GenesisCode v0.2 Normative Spec (Lock-In)

This directory is the *normative* behavior surface. If code changes semantics, update this and add/adjust tests.

## Seals
- `seal()` returns a fresh, unforgeable seal token.
- `seal(v, tok)` seals value `v` under token `tok`.
- `unseal(w, tok)` returns the payload when `w` is sealed with `tok`.
- On token mismatch, `unseal` returns `nil`.

## Hardened protocol
- UNHANDLED/EFFECT/ERROR must be created by sealing under trusted protocol tokens.
- User code must not be able to create values recognized as UNHANDLED/EFFECT/ERROR unless given those tokens.

## Errors
- The runtime protocol identity is `genesis/error-v0.2`.
- Recoverable language, Prelude, primitive-domain, and host-boundary failures are values
  sealed by the trusted ERROR token. Their payload is immutable CoreForm data with
  `:error/code`, non-empty `:error/message`, and `:error/context`.
- A map with those fields is ordinary data. A value sealed by a user-created token is an
  opaque user value. Neither is recognized as protocol ERROR.
- Fatal evaluator failures return explicit `KernelError` to the embedding boundary; they
  are not silently converted to success or confused with a recoverable sealed value.
- Parse error `:at` values are zero-based UTF-8 byte offsets governed by
  `docs/spec/NORMATIVE_FORM_MATRIX_v0.1.md`. Missing source provenance is omitted, never
  fabricated as byte zero.

## Contract dispatch
- `dispatch(c, msg)` calls `c.handler(msg)`.
- If the result is sealed UNHANDLED and `c.proto != nil`, dispatch recurses to `c.proto`.
- Otherwise dispatch returns the result.

## Effects & replay
- Effect programs are represented as `Pure(v)` or `Perform(op, payload, k)`.
- Runner is deny-by-default per capability policy.
- Every performed effect appends a deterministic log entry.
- `replay(program, log)` must consume entries in order and fail on executable mismatches (index/op/hash/scheduler metadata) and on `:decision`/`:cap` structural mismatches.
