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

- `Value::Data(t)` hashes as `BLAKE3("GCv0.2\\0value\\0data\\0" || hash_term(t))`.

### Closures

- Hash includes:
  - tag `GCv0.2\0value\0closure\0`
  - parameter name bytes
  - the closure body as `hash_term(body)` (canonical CoreForm hash)
  - the captured environment hash

Environment hashing:
- hashed as a persistent chain of frames (parent-first)
- each frame hashes its bindings in stable key order (by binding name string)
- values inside the environment are hashed recursively by `value_hash`

### Recursive Environments (Cycle Handling)

Top-level `def` bindings are evaluated in a *recursive module scope*: later `def`s become visible to earlier closures,
and mutual recursion is supported. This can introduce cycles in the runtime graph (e.g., a function bound in a scope
closes over that same scope).

`value_hash` remains total by defining a cycle break rule:
- While hashing an environment frame, if hashing re-enters the *same* frame before completing, the re-entrant
  `hash_env` call returns `BLAKE3("GCv0.2\\0env-cycle\\0")`.

This rule is stable and deterministic and prevents stack overflows during hashing. Implementations may cache
environment hashes keyed by `(frame_identity, revision)` but must respect this cycle break rule.

### Seal Tokens and Sealed Values

- Tokens hash by identity (`SealId`).
- Sealed values hash as tag `GCv0.2\0value\0sealed\0`, token id, and payload hash.

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

GenesisCode v0.2 uses `.gclog :version = 3` for the current `value_hash` encoding (parser remains backward-compatible with legacy `:version = 2` logs).
