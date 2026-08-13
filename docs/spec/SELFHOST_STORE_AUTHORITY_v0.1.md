# Self-hosted Store Authority v0.1

## Status and scope

This specification defines the point-object production-authority slice of `SD-STORE` under `R4.2.e`. The artifact-loaded binding `core/store::authority` is the sole production semantic producer for `core/store::put`, `core/store::has`, and `core/store::get`: payload and lowercase-hash admission; canonical artifact bytes; local-integrity verdicts; local/remote source selection; operation and cumulative cache-write budgets; self-hosted CoreForm parsing; and content identity.

This slice does not promote `SD-STORE` above H0. `core/store::verify`, store-wide inventory selection and integrity traversal, package/registry/VCS storage decisions, internal direct-store consumers, and non-store canonical identities remain host-authoritative and must be migrated before the row or `R4.2.e` can close.

## Protocol

The request kind is `genesis/store-authority-request-v0.1`. A put request is an exact map with `:budget-limit`, `:budget-used`, `:kind`, `:max-bytes`, `:payload`, `:phase`, and `:v`. `:phase` is `:put`; `:payload` must contain exactly `:artifact`; byte limits are nonnegative integers and the cumulative limit may be `nil`.

The authority prints the artifact with the self-hosted canonical printer, converts that exact string to UTF-8 bytes, enforces the operation and cumulative limits, computes plain BLAKE3 over those bytes, and returns an exact `genesis/store-authority-result-v0.1` envelope bound to the canonical request hash. A semantic denial is an accepted protocol result with `:action :error`. An open, mistyped, unsupported, or version-mismatched request is a protocol rejection and cannot authorize a write.

For `:action :write`, the result includes the exact bytes, lowercase hash, and byte count. Rust rejects open or contradictory results, independently checks only the mechanical byte-count and BLAKE3 binding, writes exactly the authorized bytes through the existing write-once atomic store mechanism, and requires the mechanism's returned hash to equal the authorized hash. No write occurs before authority acceptance.

`has` uses exact `genesis/store-has-authority-request-v0.1` and `genesis/store-has-authority-result-v0.1` envelopes. An initial `:plan` request validates the payload and returns `:observe-local` with the request-bound lowercase hash before Rust performs any path operation. Rust then reports only a bounded local byte/hash observation. GenesisCode returns presence, corruption, I/O/resource failure, or `:fetch-remote`; only the latter permits a policy-authorized remote presence probe, whose raw boolean or mechanism error is returned for the final verdict.

`get` analogously uses exact `genesis/store-get-authority-request-v0.1` and `genesis/store-get-authority-result-v0.1` envelopes. After `:observe-local`, Rust reports bounded stable bytes, missing, size overflow, or I/O failure. GenesisCode checks BLAKE3 identity, parses the exact bytes with `selfhost/parse::parse-term`, decides local corruption versus remote hash mismatch, and either returns the artifact or authorizes a remote fetch. The remote mechanism reports bounded bytes or an exact transport integrity outcome; GenesisCode assigns the public verdict. Remote bytes are admitted only after identity, parse, operation-limit, and cumulative cache-write checks; `:cache-return` binds the exact bytes, hash, artifact, and byte count before the host may perform its write-once cache mechanism.

## Production and fallback boundary

Production CLI policy loading records the exact self-host bootstrap mode and artifact path. A run that admits `core/store::{put,has,get}` loads `core/store::authority` from that artifact once and fails closed if the authority is absent or invalid. The prior Rust producers are compiled only for unit tests and the explicit `parity-oracle` feature used by dedicated parity binaries; they are not standard production fallbacks.

Rust retains TOML transport, already-authorized operation-limit extraction, bounded artifact bootstrap/evaluation, stable bounded file reads, policy-authorized remote HTTP/auth/integrity mechanisms, BLAKE3 and byte-count contradiction checks, directory creation, atomic write-once filesystem mechanics, durability sync, concurrent-writer handling, and sealed host-I/O error transport using stable nondisclosing messages. Those mechanisms cannot select an unapproved hash or source, parse an artifact, alter payload admission, bytes or limits, or assign the final presence/integrity verdict.

## Resource bounds and evidence

Authority evaluation is bounded to 20,000,000 steps, 160,000,000 allocation units, 40 MiB byte/string values, 16,384 vector entries, and 32 map entries per request. The store capability retains the lower 32 MiB hard artifact ceiling.

`scripts/lib/selfhost_store_authority.py` verifies profile and source identities, artifact custody, all three exact protocols, planner-before-I/O and authority-before-write ordering, bounded raw observations, exact parse/cache binding, strict result decoding, parity-only fallback isolation, CLI artifact propagation, truthful H0 ledger scope, and permanent source/route mutations. Its report is evidence for this partial slice only and cannot promote `SD-STORE`, close `R4.2.e`, replace later runtime tests, or authorize a release.
