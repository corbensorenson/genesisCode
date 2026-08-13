# Self-hosted Store Authority v0.1

## Status and scope

This specification defines the first production-authority slice of `SD-STORE` under `R4.2.e`. The artifact-loaded binding `core/store::authority` is the sole production semantic producer for `core/store::put` payload admission, canonical artifact bytes, per-operation and cumulative byte-budget admission, and content hash identity.

This slice does not promote `SD-STORE` above H0. `core/store::{get,has,verify}`, local/remote source selection, store-wide integrity traversal, package/registry/VCS decisions, and non-store canonical identities remain host-authoritative and must be migrated before the row or `R4.2.e` can close.

## Protocol

The request kind is `genesis/store-authority-request-v0.1`. A put request is an exact map with `:budget-limit`, `:budget-used`, `:kind`, `:max-bytes`, `:payload`, `:phase`, and `:v`. `:phase` is `:put`; `:payload` must contain exactly `:artifact`; byte limits are nonnegative integers and the cumulative limit may be `nil`.

The authority prints the artifact with the self-hosted canonical printer, converts that exact string to UTF-8 bytes, enforces the operation and cumulative limits, computes plain BLAKE3 over those bytes, and returns an exact `genesis/store-authority-result-v0.1` envelope bound to the canonical request hash. A semantic denial is an accepted protocol result with `:action :error`. An open, mistyped, unsupported, or version-mismatched request is a protocol rejection and cannot authorize a write.

For `:action :write`, the result includes the exact bytes, lowercase hash, and byte count. Rust rejects open or contradictory results, independently checks only the mechanical byte-count and BLAKE3 binding, writes exactly the authorized bytes through the existing write-once atomic store mechanism, and requires the mechanism's returned hash to equal the authorized hash. No write occurs before authority acceptance.

## Production and fallback boundary

Production CLI policy loading records the exact self-host bootstrap mode and artifact path. A run that admits `core/store::put` loads `core/store::authority` from that artifact once and fails closed if the authority is absent or invalid. The prior Rust producer is compiled only for unit tests and the explicit `parity-oracle` feature used by dedicated parity binaries; it is not a standard production fallback.

Rust retains TOML transport, already-authorized operation-limit extraction, bounded artifact bootstrap/evaluation, BLAKE3 contradiction checking, directory creation, atomic write-once filesystem mechanics, durability sync, concurrent-writer handling, and sealed host-I/O error transport. Those mechanisms cannot alter the authority's payload admission, bytes, limits, or identity.

## Resource bounds and evidence

Authority evaluation is bounded to 20,000,000 steps, 160,000,000 allocation units, 40 MiB byte/string values, 16,384 vector entries, and 32 map entries per request. The store capability retains the lower 32 MiB hard artifact ceiling.

`scripts/lib/selfhost_store_authority.py` verifies profile and source identities, artifact custody, exact protocol tokens, production routing, authority-before-write ordering, strict result binding, parity-only fallback isolation, CLI artifact propagation, truthful H0 ledger scope, and permanent source/route mutations. Its report is evidence for this partial slice only and cannot promote `SD-STORE`, close `R4.2.e`, replace later runtime tests, or authorize a release.
