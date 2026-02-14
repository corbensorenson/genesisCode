# Value Hashing + Effect Request Hashing v0.2

This document is **normative** for GenesisCode v0.2 because effect logs and replay depend on stable hashing.

## Value Hash (`gc_kernel::value_hash`)

`value_hash(v)` is BLAKE3 over a structured tagged encoding. Each variant contributes a domain tag and then its fields.

Important properties:
- Hashing is stable and deterministic.
- Hashing is total for all runtime values.
- Hashing of closures includes the closure body and the captured environment so that continuation hashes are replayable.

This version (v0.2) uses an encoding that is amenable to caching of shared environment prefixes; this does not change the output semantics, but avoids pathological blowups when many closures capture large shared environments.

### Data Values

- `Value::Data(t)` hashes as `hash_term(t)` tagged with `V:data`.

### Closures

- Hash includes:
  - tag `V:closure`
  - parameter name bytes
  - the closure body as `hash_term(body)` (canonical CoreForm hash)
  - the captured environment hash

Environment hashing:
- hashed as a persistent chain of frames (parent-first)
- each frame hashes its bindings in stable key order (by binding name string)
- values inside the environment are hashed recursively by `value_hash`

### Seal Tokens and Sealed Values

- Tokens hash by identity (`SealId`).
- Sealed values hash as tag `V:sealed`, token id, and payload hash.

### Native Functions

- Hash includes:
  - name
  - arity
  - collected (partially applied) arguments, hashed recursively

### Contracts

- Contracts hash by stable `contract_id` only (not by the whole structure).

### Effect Programs / Requests

- Effect programs hash their structure (`pure` vs `perform`) and contained values.
- Effect requests hash op symbol, payload term hash, and continuation hash.

## Effect Request Hash (Log `:req-h`)

For a performed effect request with:
- op symbol `op` (string)
- payload datum hash `payload_h` (bytes32)
- continuation hash `cont_h` (bytes32)

the request hash is:

`BLAKE3( "GCv0.2\\0effect-req\\0" || op || "\\0" || payload_h || cont_h )`

This hash is recorded in logs and must match during replay.

## Stability Requirements

- Any change to `value_hash` or request hashing is a compatibility break for `.gclog` replay.
- If such a change is required, bump log version and/or the version tag prefix so mixed logs are rejected deterministically.

## Log Version Note

GenesisCode v0.2 uses `.gclog :version = 2` for the current `value_hash` encoding.
