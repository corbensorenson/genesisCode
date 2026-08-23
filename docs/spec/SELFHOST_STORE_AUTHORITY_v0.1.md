# Self-hosted Store and Artifact GC Authority v0.1

## Status and scope

This specification defines the content-store production-authority slice of `SD-STORE` and the H2 artifact-GC authority boundary of `SD-ARTIFACT-GC` under `R4.2.e`. The artifact-loaded bindings `core/store::authority` and `core/store::verify-authority` are the sole production semantic producers for `core/store::{put,has,get,verify}`: payload and lowercase-hash admission; canonical artifact bytes; local-integrity verdicts; local/remote source selection; operation and cumulative cache-write budgets; self-hosted CoreForm parsing; canonical whole-store inventory selection and order; verification bounds; first-failure attribution; and content identity. The separate `core/gc::authority` contract and retained host mechanisms are normative under the Artifact GC H2 authority section below.

This slice does not promote `SD-STORE` above H0. Internal direct-store consumers, package/registry/VCS storage decisions, and non-store canonical identities remain host-authoritative and must be migrated before the row or `R4.2.e` can close.

## Protocol

The request kind is `genesis/store-authority-request-v0.1`. A put request is an exact map with `:budget-limit`, `:budget-used`, `:kind`, `:max-bytes`, `:payload`, `:phase`, and `:v`. `:phase` is `:put`; `:payload` must contain exactly `:artifact`; byte limits are nonnegative integers and the cumulative limit may be `nil`.

The authority prints the artifact with the self-hosted canonical printer, converts that exact string to UTF-8 bytes, enforces the operation and cumulative limits, computes plain BLAKE3 over those bytes, and returns an exact `genesis/store-authority-result-v0.1` envelope bound to the canonical request hash. A semantic denial is an accepted protocol result with `:action :error`. An open, mistyped, unsupported, or version-mismatched request is a protocol rejection and cannot authorize a write.

For `:action :write`, the result includes the exact bytes, lowercase hash, and byte count. Rust rejects open or contradictory results, independently checks only the mechanical byte-count and BLAKE3 binding, writes exactly the authorized bytes through the existing write-once atomic store mechanism, and requires the mechanism's returned hash to equal the authorized hash. No write occurs before authority acceptance.

`has` uses exact `genesis/store-has-authority-request-v0.1` and `genesis/store-has-authority-result-v0.1` envelopes. An initial `:plan` request validates the payload and returns `:observe-local` with the request-bound lowercase hash before Rust performs any path operation. Rust then reports only a bounded local byte/hash observation. GenesisCode returns presence, corruption, I/O/resource failure, or `:fetch-remote`; only the latter permits a policy-authorized remote presence probe, whose raw boolean or mechanism error is returned for the final verdict.

`get` analogously uses exact `genesis/store-get-authority-request-v0.1` and `genesis/store-get-authority-result-v0.1` envelopes. After `:observe-local`, Rust reports bounded stable bytes, missing, size overflow, or I/O failure. GenesisCode checks BLAKE3 identity, parses the exact bytes with `selfhost/parse::parse-term`, decides local corruption versus remote hash mismatch, and either returns the artifact or authorizes a remote fetch. The remote mechanism reports bounded bytes or an exact transport integrity outcome; GenesisCode assigns the public verdict. Remote bytes are admitted only after identity, parse, operation-limit, and cumulative cache-write checks; `:cache-return` binds the exact bytes, hash, artifact, and byte count before the host may perform its write-once cache mechanism.

`verify` uses exact `genesis/store-verify-authority-request-v0.1` and `genesis/store-verify-authority-result-v0.1` envelopes. The `:plan` phase validates the exact `{:hash nil|string}` payload before any inventory or path observation. A specific lowercase hash directly selects one bounded hash observation. A whole-store request first authorizes `:observe-inventory`; Rust returns only a bounded, raw-byte-sorted vector of exact `{:kind symbol :name bytes}` entries. GenesisCode independently verifies strict ordering and shape, selects only regular files whose raw names are exactly 64 lowercase hexadecimal bytes, and returns the ordered hash vector to observe.

Rust then streams each selected file through a bounded BLAKE3 mechanism without retaining the full artifact, returning exact hash, observed-byte count, and closed status observations. GenesisCode re-derives the selected hash inventory from the original entries, rejects order or substitution drift, enforces the 32 MiB per-artifact and 512 MiB cumulative ceilings, compares each observed hash to its selected identity, and returns success or the first raw-byte-ordered failure with an exact checked count. Missing specific artifacts are `core/store/not-found`; disappearing scan entries and hash mismatches are `core/store/corruption`; host read and inventory failures use stable nondisclosing `core/store/io-error`; bounded overflow is `core/caps/resource-limit`. Unsupported observation statuses are protocol rejections, not host-selected semantic errors.

## Production and fallback boundary

Production CLI policy loading records the exact self-host bootstrap mode and artifact path. A run that admits `core/store::{put,has,get,verify}` loads both store authority bindings from that artifact once and fails closed if either authority is absent or invalid. The prior Rust producers are compiled only for unit tests and the explicit `parity-oracle` feature used by dedicated parity binaries; they are not standard production fallbacks.

Rust retains TOML transport, already-authorized operation-limit extraction, bounded artifact bootstrap/evaluation, stable bounded file reads, policy-authorized remote HTTP/auth/integrity mechanisms, BLAKE3 and byte-count contradiction checks, bounded raw directory enumeration, regular-file type observation, raw-byte sorting, bounded streamed hashing, directory creation, atomic write-once filesystem mechanics, durability sync, concurrent-writer handling, and sealed host-I/O error transport using stable nondisclosing messages. GenesisCode validates the host-provided order, entry shape, selection, observation binding, and all semantic verdicts. Host mechanisms cannot select an unapproved hash or source, parse an artifact, alter payload admission, bytes or limits, or assign final presence/integrity outcomes.

## Resource bounds and evidence

Authority evaluation is bounded to 20,000,000 steps, 160,000,000 allocation units, 40 MiB byte/string values, 16,384 vector entries, and 32 map entries per request. The store capability retains the lower 32 MiB hard artifact ceiling. Whole-store verification additionally admits at most 8,192 raw entries, 2 MiB of cumulative raw entry-name bytes, and 512 MiB of cumulative artifact bytes. Limit arithmetic is saturating in the host mechanism and rechecked from exact observations by GenesisCode.

`scripts/lib/selfhost_store_authority.py` verifies profile and both source identities, artifact custody, all four exact protocols, planner-before-I/O and authority-before-write ordering, bounded raw inventory and streamed hash observations, exact parse/cache/inventory binding, strict result decoding, parity-only fallback isolation, native CLI coverage, truthful H0 ledger scope, and permanent source/route mutations. Its report is evidence for this partial slice only and cannot promote `SD-STORE`, close `R4.2.e`, replace later runtime tests, or authorize a release.

## Artifact GC H2 authority

Status: normative H2 contract for `SD-ARTIFACT-GC` under R4.2.e.

## Authority

`core/gc::authority`, loaded from the exact artifact-only self-host toolchain, is the sole production semantic producer for `core/gc-low::{plan,run,pin,unpin,purge}`. It owns pins admission and normalization, pin/unpin target admission, canonical pins bytes, reference-tombstone and pinned-reference resolution, canonical roots and provenance, artifact-edge selection, dead-set selection, reclaim-byte accounting, largest-artifact ranking, and quarantine purge selection.

Production Rust MUST fail closed when the binding is absent, artifact bootstrap or evaluation fails, an input or output map is open or malformed, the result is not bound to the exact canonical request hash, a result contains a noncanonical identity, or an authorized object changes before mutation. No Rust semantic fallback, success-capable default, repair path, or alternate dead/purge planner is permitted.

## Closed protocol

Requests have kind `genesis/gc-authority-request-v0.1`, version `1`, and exactly `:kind`, `:op`, `:payload`, and `:v`. Results have kind `genesis/gc-authority-result-v0.1`, version `1`, and exactly `:code`, `:kind`, `:message`, `:ok`, `:request-h`, `:v`, and `:value`. `:request-h` is the lowercase canonical CoreForm hash of the exact request. Acceptance requires `:ok true`, nil code/message, and an operation-specific closed value. Rejection requires `:ok false`, nil value, and one of the closed codes `core/gc/bad-authority-request` or `core/gc/bad-pins`.

The operation inventory is:

- `:roots` accepts exact refs, lock, generic pins-document, and include flags. It normalizes all 32-byte artifact hashes, ignores valid reference tombstones, resolves every pinned reference against the observed refs snapshot even when ordinary refs are excluded, invokes `core/gc/reach::roots-plan`, deduplicates by canonical identity, and returns one provenance entry per sorted root.
- `:artifact-edges` accepts an exact artifact and three boolean inclusion controls. It invokes `core/vcs/reach::artifact-ref-plan`, rejects malformed produced identities, and returns sorted unique ordinary and parent edges. The host MUST enqueue every returned ordinary edge at the current parent depth and every returned parent edge at depth minus one; it cannot add, remove, repair, or reinterpret edges.
- `:dead-plan` accepts sorted live identities and a content-verified store inventory. It rejects duplicate or malformed inventory entries, selects every inventory identity absent from the live set, sums exact reclaim bytes, and returns the first 25 dead artifacts ordered by descending size with canonical-hash tie order.
- `:pins-update` accepts `:pin` or `:unpin`, a target, and a generic TOML document observation. A missing document is empty. For compatibility, version defaults to 1 and absent `keep` or `keep_refs` arrays default empty; unknown TOML keys are ignored exactly as in the frozen v1 behavior. Present versions other than 1, mistyped fields, malformed hashes, and non-`refs/` reference names fail closed. The authority returns the complete canonical UTF-8 pins document and normalized sorted unique projections; Rust writes those exact bytes atomically while holding the pins lock.
- `:purge-plan` accepts a nonnegative TTL in seconds and a content-verified quarantine inventory whose values are host-observed ages in whole seconds. It returns exactly the sorted identities whose observed age is greater than or equal to the TTL.

## Host mechanisms

Rust may perform only bounded, contradiction-checking mechanisms:

- load and evaluate the artifact-only authority with declared limits;
- transport already-authorized lock models and refs snapshots as neutral terms;
- open pins through a nonblocking descriptor, prove the descriptor is a regular file, read at most 4 MiB plus one byte, decode UTF-8 and generic TOML without applying pins semantics;
- lock the store before inventory/dead planning, lock pins across read-authorize-write, and lock quarantine across inventory-authorize-purge;
- enumerate lowercase 64-hex names in canonical order, prove each artifact's bytes match its name before authority evaluation, and repeat that proof immediately before mutation;
- decode a content-verified artifact as CoreForm, ask the authority for exact edges, execute the mandated bounded work queue, and stop above 50,000 distinct objects;
- observe system time and file modification time to calculate nonnegative whole-second age;
- atomically write exact authority-produced pins bytes, or rename/delete only exact authority-produced identities.

An unparseable but content-valid artifact is a leaf, preserving the artifact-store contract. Missing or corrupt live objects fail before dead planning. Corrupt dead or quarantined objects also fail rather than being silently reclaimed. A disappearing object, nonregular replacement, content replacement, preexisting quarantine destination, malformed pins document, lock failure, or I/O error aborts the operation. Partial mutation caused by a later external race remains an explicit failed operation and is never reported as a successful complete plan.

## Determinism and resource bounds

The authority evaluator resets counters after trusted bootstrap and receives 80,000,000 post-bootstrap steps, 320,000,000 logical allocation units, 8 MiB byte/string values, and 65,536 map/vector entries. Pins observations are capped at 4 MiB. Closure is capped at 50,000 distinct objects. The largest-dead projection is capped at 25 entries. Host inventory and work-queue order are canonical, every authority result is request-hash-bound, and outer effect logging binds the request and response under the ordinary strict replay contract.

System time and file metadata are explicit host observations used only by purge selection. They do not enter pure kernel semantics. Replay consumes the recorded effect result and does not re-observe time or storage.

## Fallback and verification

The former Rust pins parser/writer, roots planner, dead-set planner, and purge selector are removed from production rather than retained as reachable fallback. The generic TOML decoder, bounded work queue, inventory scanner, file locks, artifact verifier, atomic writer, rename, delete, and clock are mechanisms and cannot produce an accepting semantic verdict.

`scripts/lib/selfhost_gc_authority.py` independently verifies profile/schema/source/artifact custody, closed protocol markers, artifact-only runner loading, strict adapter decoding, no-fallback source reachability, operation ordering, bounded descriptor reads, lock coverage, repeated content-identity checks, exact mutation sets, adversarial tests, and the H2 semantic-ownership row. Its self-test must reject mutations that remove authority loading, restore native planners, move mutation before authority, weaken identity checks, open the result schema, weaken bounds, or inflate claims.

This contract does not claim H3/H4, bootstrap fixpoint, aggregate R4.2.e or SH-C closure, GPK/sync/store/refs/package authority, or release qualification.
