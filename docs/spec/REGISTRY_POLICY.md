# Registry Policy and Transport (v0.2)

This document specifies a minimal, local “registry policy” format and verification behavior for GenesisCode v0.2.

The intent is to let CI and release tooling enforce supply-chain rules without requiring a network registry service.

## Policy File (`policy.toml`)

The policy file is TOML with:

- `version = 1` (required)
- `min_signatures = <int>` (optional, default `0`)
- `allowed_public_keys = ["<base64-32-bytes>", ...]` (optional, default `[]`)

If `min_signatures > 0`, `allowed_public_keys` MUST be non-empty.

## Signature Set File

The signature set file is a CoreForm term stored on disk (default `.genesis/signatures.gc`) containing:

- a vector of 64-hex signature artifact hashes, e.g. `["<hex>" "<hex2>" ...]`

The set is treated as order-insensitive; tooling SHOULD sort and deduplicate when writing.

## Verification (`genesis verify --policy`)

When invoked with `--policy <policy.toml>`, `genesis verify` MUST:

1. Perform standard package verification (module hashes, dependency hashes, acceptance artifact integrity).
2. If `min_signatures > 0`:
   - require an acceptance artifact hash (from `--acceptance` or `.genesis/last_acceptance`)
   - load the signature set file (from `--signatures` or `.genesis/signatures.gc`)
   - for each signature artifact hash in the set:
     - verify the artifact exists in `.genesis/store/` and its name matches its content hash
     - parse it as `genesis/acceptance-signature-v0.2`
     - require `:acceptance-h` to match the acceptance artifact hash
     - require `:pk` to be in `allowed_public_keys`
     - verify the Ed25519 signature over the message specified in `docs/spec/SIGNING.md`
3. Fail verification if the number of valid signatures is less than `min_signatures`.

## Registry transport boundary v0.1

This contract governs the mechanical transport and content-addressed object
admission in `gc_registry::RegistryClient`. Package, commit, signing and
capability decisions remain with their existing GenesisCode authorities.

### Object admission

Object identities are exactly 64 lowercase ASCII hexadecimal digits, naming the
BLAKE3-256 digest of the object's bytes. Get, optional get, has, put and upload
start MUST reject other spellings before backend dispatch, path construction or
filesystem mutation. Put and automatic/chunked put MUST check supplied bytes
before starting an upload or contacting a backend. Upload start alone admits an
identity and size; it cannot verify bytes not yet supplied.

Every successful get, including optional and bounded variants, MUST verify the
returned bytes against the requested identity before returning them. File, HTTP,
in-process and the WASI file-backed HTTP bridge share this rule. A mismatch is a
typed `RegistryError::HashMismatch`, retaining operation, expected and actual
identity. Corrupt objects are errors, never optional absence. File absence means
`NotFound` from opening the object; other I/O errors remain errors. HTTP absence
means status 404. The legacy in-process absence convention is exactly
`RegistryError::Http("store/get: status 404")`; arbitrary messages containing
that substring MUST NOT become absence. Read-only file has/get operations do
not initialize registry directories.

### Bounded body reads

With `Some(limit)`, file and HTTP get bodies use a shared streaming reader.
They MUST consume at most `limit + 1` bytes (the final byte is an oversize probe),
retain at most `limit` bytes, use checked byte accounting and fallible buffer
growth, and reject rather than truncate oversize objects. Exact-limit and empty
objects still require an EOF probe. Metadata and Content-Length may reject a
known oversize object before reading; they do not replace the streaming bound
when a file grows or a response lacks Content-Length. Interrupted reads retry;
other read errors remain errors. All limit and allocation failures retain the
`resource-limit:` protocol classification.

`None` preserves the legacy absence of a byte ceiling, with fallible buffer
growth. It does not establish a finite memory contract. An in-process registry
returns an owned `Vec`; the client verifies its size and hash, but cannot bound
allocation inside an arbitrary callback. Such backends must independently
enforce their producer budget. Server request/upload/session limits and stalled
body cancellation are separately required by roadmap F09; this client contract
does not qualify them.

### HTTP authority

The shared native client MUST disable redirects for all methods. A 3xx response
returns a transport error without contacting its Location, including same-origin
locations, chains and loops. A caller can request a different remote only through
its ordinary capability authorization. No redirect can inherit the initial
remote's permission or credential authority. Honest nonredirected requests,
existing authentication and timeout settings remain supported. WASI bridges use
their configured file adapter and do not perform HTTP redirect hops.

### Sync installation and compatibility

Sync MUST preflight all results in a downloaded batch for transport success,
resource budgets and byte identity before installing any member of that batch.
Corrupt bytes never enter the destination store. Hash mismatches retain sealed
`core/sync/hash-mismatch`; artifact-backed store get projects the typed failure
to its existing `:remote-hash-mismatch` authority observation. Package artifact
hydration and the explicit store parity route retain `core/store/hash-mismatch`
and the stable message `remote bytes hash mismatch`. Strict replay
retains the recorded sealed result without registry access.

Previously verified batches can remain if a later batch fails; whole-closure
rollback is still an open F11 acceptance obligation. This contract does not
silently grant transaction atomicity for I/O failure or cancellation during
installation. The wire object format, BLAKE3 identity, language profile and
effect-log version are unchanged. The new typed Rust error variant requires
downstream exhaustive matches to handle hash mismatch; successful API results
retain their existing types and bytes. Previously accepted malformed identities,
corrupt responses and redirects now fail closed.
