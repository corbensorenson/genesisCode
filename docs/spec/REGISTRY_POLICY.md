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
enforce their producer budget. Native server admission is specified below;
this client contract alone does not qualify server lifecycle or uploads.

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


## Native HTTP server admission and ownership v0.1

This section governs `gc_registry`'s native HTTP/1 file-backed server. It does
not add HTTP hosting to the WASI file adapter. `HttpRegistryServerConfig`
retains its existing fields. The existing spawn function selects default
`HttpRegistryServerLimits`; the additional spawn function accepts smaller
finite limits and validates them before binding or filesystem mutation.

| Admission | Default ceiling |
| --- | ---: |
| JSON request and encoded response bytes | 1 MiB |
| Direct object, downloaded object and assembled upload bytes | 64 MiB |
| Sum of declared upload sizes and sum of retained upload bytes | 128 MiB each |
| Live upload sessions | 32 |
| Chunk indices per session | 1024 |
| Hashes in a store/has request | 1024 |
| Parsed HTTP headers | 64 |
| HTTP framing buffer | 16 KiB |
| Absolute connection/request deadline | 30 seconds |
| Upload lifetime from creation | 300 seconds |

Limits MUST be positive and no greater than the defaults. JSON admission MUST
allow at least 256 bytes for the ordinary protocol envelope. The upload byte
budget MUST cover one admitted maximum-size object. Configured chunk bytes
MUST be positive and no greater than the object ceiling; the existing default
is 4 MiB. These limits introduce refusals for previously unbounded inputs;
successful route shapes, object identities and effect-log versions remain
unchanged. The server serves one connection at a time and closes it after its
request rather than retaining HTTP keep-alive connections.

### Request and response admission

A declared oversized Content-Length MUST be refused before body polling,
including before a 100 Continue response. Missing length and chunked framing
MUST still be bounded during reading. A body frame exceeding the remaining
application budget MUST be rejected before copying that frame. Application
byte buffers MUST use checked accounting and fallible, geometrically bounded
growth. The transport framing buffer has its separate ceiling; this server
contract does not promise the client's `limit + 1` socket-consumption bound.
Malformed or truncated framing MUST NOT dispatch a partial object. Excessive
headers and malformed connections end their owned connection without consuming
`max_requests` unless a request actually reached the service.

Store/has MUST admit its hash count during decoding and validate each identity
before backend dispatch. Encoded JSON responses MUST use a bounded writer.
Resource refusals use HTTP 413 and `payload_too_large` with the existing JSON
error envelope when it fits; an oversized error envelope falls back to `{}`.
Object get MUST use the shared bounded, hash-verifying client reader, including
for objects already present on disk. These byte/count contracts do not bound
all intermediate allocation in the existing reference database or arbitrary
file backends, nor establish physical RSS qualification.

### Upload accounting and finish

Upload start MUST reserve the declared size only after hash, object size,
minimum chunk count, live-session count and aggregate reservation checks.
Denied start MUST NOT spend an upload identity or reserve bytes. Chunk indices
MUST be less than the configured count ceiling. Admission MUST account for
replacement of an existing chunk: the new body fits the remaining declared
size and global retained-byte budget after subtracting the replaced bytes.
Rejected chunks MUST preserve prior chunks and accounting. Zero-byte chunks
still consume their bounded index entries.

Session lifetime is absolute from creation; chunk traffic MUST NOT refresh it.
Expiry MUST release both declared reservations and retained bytes, including
while idle or while another connection stalls. Maintenance runs at intervals
no longer than the smaller of one second and the configured lifetime when
network work yields. Status and mutation also perform expiry checks.

Finish is terminal, including size, index, hash or allocation failure. It MUST
release session accounting, check exact total size and contiguous zero-based
indices before assembly, reserve the bounded output fallibly once and consume
chunks without cloning them. Installation MUST use the shared hash-verifying
store put. Assembly can temporarily retain both chunk bytes and one output
object; the upload counters describe reservations and chunk payload bytes,
not total process memory. An active replacement request likewise temporarily
holds its admitted body alongside the old chunk. Server stop drops all owned
in-memory sessions. File-backed client upload sessions remain a separately
required boundary; these native server limits do not qualify them.

### Lifecycle and cancellation

`join()` MUST naturally wait without requesting shutdown. `shutdown()` signals
stop; `stop_and_join()` signals and then waits. A cloneable shutdown handle
MUST remain usable while the owner naturally waits. Dropping an unjoined owner
MUST stop and join its worker. Explicit joins return initialization/worker
errors; Drop cannot return an error. No server or connection worker may detach.
`max_requests = 0` completes without admission; a positive maximum completes
after that many service-admitted requests. Unlimited CLI serving remains alive
until terminated by its process owner.

The server MUST own its listener and active connection future and interrupt
idle accepts, incomplete headers, withheld bodies and response network work
on shutdown or their absolute deadline. Body progress MUST NOT reset the
deadline. Stop/deadline admission MUST be checked before semantic dispatch.
This is a network cancellation contract. Existing synchronous file operations,
reference locks, initialization and durability commits still require separate
bounded filesystem/cancellation evidence; they can delay worker completion.
The network deadline cannot interrupt a synchronous filesystem call or roll
back a commit already admitted. This contract does not grant whole-operation
atomicity, arbitrary filesystem hard cancellation, CLI graceful signal handling,
cross-host qualification, or independent F07/F09/F11 acceptance.
