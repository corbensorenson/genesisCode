# Self-host replay authority v0.1

Status: normative H2 contract for `SD-REPLAY` under R4.2.d.

## Authority

`core/effects::replay-authority`, loaded from the exact artifact-only self-host toolchain, owns every semantic accept/reject decision for production effect-log replay. The authority receives closed, request-hash-bound observations and returns a closed decision. Production hosts must fail closed when the binding is absent, evaluation fails, the result is open or malformed, the request identity differs, or an accepted step cannot be executed by the bounded host mechanisms.

The authority decides:

- program/log identity;
- ordered entry presence and exhaustion;
- entry index and operation identity;
- payload, continuation, request, and response hash equality;
- decision/capability consistency, including the explicit legacy-v2 cap rule;
- response-load admission;
- deterministic schedule step, task ID, parent task, and await edge.

No production Rust comparison may approve, reject, repair, or override these facts. The legacy Rust replay checker may remain only behind the explicitly compiled parity harness and is not a production fallback.

## Host mechanisms

Rust remains responsible only for bounded mechanisms whose outcomes are transported to, or follow acceptance by, the authority:

- structural `.gclog` decoding under `GCLOG.md`;
- unforgeable EFFECT/ERROR seal checks and effect-program stepping;
- canonical term, value, continuation, and request hash observations;
- response artifact lookup and CoreForm decoding;
- continuation application after authority acceptance.

These mechanisms may fail closed. They do not infer a replay verdict. A response-load failure is transported as `:response-status :load-error`; the authority rejects it before the host can apply a continuation.

## Protocol

Requests have kind `genesis/effect-replay-authority-request-v0.1`, version `1`, and exactly one phase-specific field set:

- `:header`: expected and logged 32-byte program hashes;
- `:pure`: current index and entry count;
- `:perform`: index, log version, closed expected observation, and either a closed logged observation or `nil`.

Results have kind `genesis/effect-replay-authority-result-v0.1`, version `1`, and exactly `:code`, `:kind`, `:message`, `:ok`, `:request-h`, and `:v`. `:request-h` is the lowercase canonical CoreForm hash of the exact request. Acceptance requires `:ok true` and nil code/message. Rejection requires `:ok false` and nonempty string code/message.

## Bounds and failure posture

Each replay command loads an independent authority evaluator with 32,000,000 logical allocation units and 20,000,000 post-bootstrap evaluation steps. Trusted bootstrap runs under its separately bounded profile, after which counters reset so replay receives the complete declared envelope. Artifact-only bootstrap is mandatory in production. Missing artifacts, stale source identity, malformed protocol values, unknown phases, open maps, opaque values, sealed ERROR results, and exhausted bounds all fail closed.

## Verification and nonclaims

`scripts/lib/selfhost_replay_authority.py` independently checks source/artifact custody, protocol closure, CLI and obligation production routing, parity-only legacy reachability, host-mechanism limits, adversarial test inventory, and semantic-ownership ledger alignment. Runtime tests mutate every serialized replay fact and response-load state.

This profile does not claim H3/H4, bootstrap fixpoint, aggregate R4.2.d or SH-C closure, signing/evidence authority, or release qualification.
