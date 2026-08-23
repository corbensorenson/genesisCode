# Self-hosted package snapshot authority v0.1

Status: normative partial authority contract for `R4.2.e`.

## Boundary

Artifact-loaded `core/pkg::snapshot-authority` is the exclusive production semantic producer for
direct `core/pkg-low::snapshot` module-object bytes and identities, module identity
recomputation, package snapshot construction, snapshot bytes, and snapshot identity. The request
is closed and bound to the exact package path, name, version, obligations, ordered module paths,
canonical forms, and module hashes. The result is closed and request-hash-bound.

The independently governed artifact-loaded package-manifest authority owns structural manifest
admission and normalization before snapshot facts are assembled. Rust retains bounded TOML and
filesystem transport, sandbox/path enforcement, source parsing and canonicalization pending their
source-frontend migration, capability and byte-budget enforcement, and exact authorized-byte
persistence. It must store every returned artifact in authority order, reject any term/bytes/hash
contradiction, and reject a store identity mismatch. Missing snapshot authority fails before package
or store I/O; missing package-manifest authority fails before manifest interpretation. There is no
production native package-manifest parser, Rust snapshot-object constructor, or hash fallback.

This contract does not promote aggregate `SD-PACKAGE-RESOLUTION`: package-manifest authority is
governed independently, while source-to-canonical-module transport on this low-level route, graph
resolution, registry and publish policy, workspace operations, and other package/VCS identities
remain open.

## Protocol

The request kind is `genesis/pkg-snapshot-authority-request-v0.1` with exactly `:facts`, `:kind`,
and `:v`. Facts contain exactly `:modules`, `:name`, `:obligations`, `:pkg`, and `:version`.
Each ordered module contains exactly `:module`, `:module-h`, and `:path`.

The result kind is `genesis/pkg-snapshot-authority-result-v0.1` with exactly `:code`, `:kind`,
`:message`, `:ok`, `:request-h`, `:v`, and `:value`. Success returns an ordered artifact vector,
the public module projection, and the final snapshot hash. Every artifact contains exact `:term`,
UTF-8 canonical `:bytes`, and raw BLAKE3 `:h`. Failure uses only the closed codes
`core/pkg/bad-authority-request` and `core/pkg/bad-package`.

## Verification

`scripts/lib/selfhost_pkg_snapshot_authority.py` independently checks profile/schema closure,
source and artifact custody, exact decoders, authority-before-I/O ordering, exact-write behavior,
retired constructor absence, focused controls, truthful ledger status, and mutation rejection.
