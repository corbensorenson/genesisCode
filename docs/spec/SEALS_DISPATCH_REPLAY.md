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

## Contract dispatch
- `dispatch(c, msg)` calls `c.handler(msg)`.
- If the result is sealed UNHANDLED and `c.proto != nil`, dispatch recurses to `c.proto`.
- Otherwise dispatch returns the result.

## Effects & replay
- Effect programs are represented as `Pure(v)` or `Perform(op, payload, k)`.
- Runner is deny-by-default per capability policy.
- Every performed effect appends a deterministic log entry.
- `replay(program, log)` must consume entries in order and fail on any mismatch.
