# Self-hosted package scaffold authority v0.1

Status: normative partial authority contract for `R4.2.e`.

## Scope

The artifact-loaded `core/pkg::scaffold-authority` binding is the exclusive production semantic
authority for `gcpm scaffold`. It owns ASCII identifier normalization, the closed six-archetype
inventory, runtime-backend alias normalization and defaults, primary and release target selection,
the ten-file order, all six dynamic document bodies, admission of four identity-pinned static
capability templates, every body BLAKE3 identity, the aggregate scaffold identity, and the exact
public report.

Rust supplies the exact bytes of the four static capability templates, loads and evaluates the
artifact, strictly decodes the result, cross-checks the report against the authorized workspace and
lock documents, preflights the complete destination, and persists exact authorized bytes. Rust MUST
NOT normalize names, choose an archetype behavior, select a backend or target, render a dynamic
document, reorder files, recompute a replacement report, silently use the retained native oracle,
or write any file before authority and whole-plan validation succeed.

## Closed Protocol

The request kind is `genesis/pkg-scaffold-authority-request-v0.1`, version 1, and contains exactly
`[:archetype :kind :name :policy :registry-default :root :runtime-backend :static-files :v]`.
Archetype is exactly one of `web`, `service`, `desktop`, `mobile`, `xr-game`, or `data-ai`.
Name, policy, registry, root, backend, and static observations are bounded. Optional values are nil
or strings. The four static observations have exact paths, bodies, and pinned BLAKE3 identities.

Every result contains exactly `[:code :kind :message :ok :request-h :v :value]`, uses kind
`genesis/pkg-scaffold-authority-result-v0.1`, and binds the canonical complete request hash. A
rejection uses only `core/pkg/bad-scaffold`, a closed message, and nil value. Success has nil code
and message and a value containing exactly `:files` and `:report`.

The successful file vector has exactly ten entries in this order:

1. `genesis.workspace.toml`
2. `genesis.lock`
3. `package.toml`
4. `src/main.gc`
5. `deploy/presets.toml`
6. `caps.toml`
7. `caps.ci.toml`
8. `caps.release.toml`
9. `caps.backend.toml`
10. `README.gcpm.md`

Each entry contains exactly `[:body :h :path]`; `:h` is BLAKE3 of the exact UTF-8 body. The
aggregate `:scaffold-h` is BLAKE3 of lexically sorted `path:body-h` records joined by one newline.
The exact report contains archetype, ordered paths, count, package, requested root, selected
backend, aggregate identity, normalized workspace, and `:ok true`.

## Host Admission And Writes

The adapter independently checks envelope and nested field closure, request identity, fixed file
order, every body hash, static observation equality, aggregate identity, report/root/path/count
coherence, requested archetype, workspace/package/backend projection, policy and registry defaults,
and lock/workspace coherence. Invalid, opaque, sealed, open, contradictory, or unavailable
authority results fail closed.

Before creating a directory or file, the adapter validates every relative path, every existing
directory ancestor, every destination type, every overwrite conflict, and all symlink boundaries.
An authority rejection, missing binding, malformed result, invalid backend, unsafe parent, or any
non-force collision therefore produces zero scaffold mutation. Accepted files use same-directory
temporary files and atomic rename individually; temporary files are removed after write or rename
failure.

## Bounds And Compatibility Oracle

Names are at most 256 UTF-8 bytes, policies 1,024, registry and root strings 4,096, and backend
overrides 32. The fixed plan has four static observations and ten output files. Evaluation uses the
shared bounded artifact-only CLI context.

The former complete Rust scaffold implementation is compiled only for tests or the explicit
`parity-harness` feature. Its retained sample has exact per-file and aggregate identities and cannot
be called by the production adapter. It is a compatibility oracle, not a fallback or verifier.

## Nonclaims

This contract does not claim generic TOML parsing or serialization; arbitrary workspace init,
migration, environment, task, or manifest authority; static capability-template generation;
filesystem or path-policy authority; crash-atomic multi-file commit; WASI scaffold support; H2
workspace closure; `R4.2.e` or SH-C closure; bootstrap fixpoint; or release qualification.
