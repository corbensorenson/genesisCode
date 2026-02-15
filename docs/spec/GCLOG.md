# Effect Log (`.gclog`) v0.2

This document is **normative** for the on-disk effect log format used by `genesis run` and `genesis replay`.

## Encoding

- A `.gclog` file is a single canonical CoreForm term (a map) optionally followed by trailing whitespace/newline.
- All hashes inside the log are 32-byte BLAKE3 digests stored as `Bytes` (length 32).

## Header Schema

Top-level keys:

- `:version` (int): log schema version (v0.2 uses `2`).
- `:program-hash` (bytes32): CoreForm module hash of the executed program/module.
- `:toolchain` (string): toolchain identifier (e.g. `genesis 0.1.0`).
- `:entries` (vector): ordered list of entries.

## Entry Schema

Each entry is a map with keys:

- `:i` (int): 0-based effect index.
- `:op` (symbol): fully-qualified op symbol.
- `:payload-h` (bytes32): `hash_term(payload)`.
- `:cont-h` (bytes32): `value_hash(continuation)`.
- `:req-h` (bytes32): request hash (see `docs/spec/VALUE_EFFECT_HASH.md`).
- `:decision` (symbol): `:allow` or `:deny`.
- `:cap` (term): stable policy descriptor (may be `nil` on deny).
- `:resp` (map): response descriptor (inline or artifact reference).
- `:resp-h` (bytes32): `value_hash(response_value)`.

Notes:
- `:cap` is intended for stable, non-secret configuration metadata. The v0.2 toolchain does not record filesystem paths (such as `base_dir`) in logs to avoid nondeterminism and path leakage.

## Response Schema (`:resp`)

Response is a map:

- `:kind` (symbol): one of:
  - `:ok` / `:error`: inline response stored in `:value`
  - `:ok-artifact` / `:error-artifact`: response stored as a CoreForm term artifact (UTF-8 CoreForm bytes)
  - `:ok-bytes-artifact` / `:error-bytes-artifact`: response stored as raw bytes artifact (reconstructed as `Bytes`)
- `:value` (term): present for `:ok` and `:error`
- `:artifact` (string): present for `*-artifact` kinds, content-addressed artifact hash (lowercase hex BLAKE3)

Artifact lookup is performed in the configured artifact store directory (see `docs/spec/CAPS_TOML.md` and `docs/spec/EVIDENCE_STORE.md`).
