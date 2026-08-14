# Self-hosted package workspace-new authority v0.1

Status: normative partial authority contract for `R4.2.e`.

## Scope

The artifact-loaded `core/pkg::workspace-new-authority` binding is the exclusive production
semantic authority for `gcpm new`. It owns bounded member-spec parsing, default-root-member
construction, legacy-exact TOML string escaping, canonical workspace/default/profile rendering,
canonical empty lock rendering, the fixed two-file order, both body BLAKE3 identities, and the
exact public report.

Rust supplies the active runtime-backend profile and requested destination strings as closed
observations, loads and evaluates the artifact, strictly decodes the result, independently parses
and cross-checks both authorized documents, preflights both destinations, and persists the exact
authorized bytes. Rust MUST NOT parse a member spec, choose a default member, render either
document, substitute a backend, reconstruct a report, silently use the retained native oracle, or
write the lock before the complete authority result and both destinations pass validation.

## Closed Protocol

The request kind is `genesis/pkg-workspace-new-authority-request-v0.1`, version 1, and contains
exactly `[:active-backend :kind :lock :members :policy :registry-default :v :workspace
:workspace-file]`. Workspace and policy strings are non-empty and at most 1,024 UTF-8 bytes;
registry and destination observations are nil-or-string or string as applicable and at most 4,096
bytes; backend is at most 32 bytes and exactly one of `headless`, `gpu`, `gfx`, or `backend`; and
the member vector contains at most 256 strings of at most 4,096 bytes each.

An empty member vector produces one root member named after the workspace at path `.`. Otherwise,
each member is either the first-`=` split `name=path`, with both sides trimmed and non-empty, or a
trimmed path whose name is the final slash-delimited segment, with `member` used after a trailing
slash. Explicit members have role `package`; the default member has role `root`; input order is
preserved.

Every result contains exactly `[:code :kind :message :ok :request-h :v :value]`, uses kind
`genesis/pkg-workspace-new-authority-result-v0.1`, and binds the canonical complete request hash.
A rejection uses only `core/pkg/bad-workspace-new`, a closed message, and nil value. Success has nil
code and message and a value containing exactly `:files` and `:report`.

The successful file vector has exactly two entries, first the requested lock path and then the
requested workspace path. Each entry contains exactly `[:body :h :path]`, and `:h` is BLAKE3 of
the exact UTF-8 body. Dynamic TOML values preserve the retired Rust serializer byte-for-byte:
quote, reverse-solidus, newline, carriage-return, and tab use their short escapes; every other C0,
DEL, and C1 control uses uppercase `\\uXXXX`; all other valid UTF-8 is unchanged. Workspace profiles
are exactly `ci`, `dev`, and `release` in lexical order. The exact report binds workspace,
destinations, both hashes, member count, and `:ok true`.

## Host Admission And Writes

The adapter independently checks envelope and nested field closure, request identity, fixed file
order, both body hashes, exact destination echoes, report coherence, lock version and emptiness,
registry/policy/workspace projection, member validity, the closed profile inventory and every
profile/default field. Invalid, opaque, sealed, open, contradictory, or unavailable authority
results fail closed.

Before either write, the adapter rejects identical destinations and validates every existing
directory ancestor, both destination types, and all symlink boundaries. An authority rejection,
missing binding, malformed result, invalid document, unsafe parent, destination symlink, or
non-file destination therefore produces zero workspace-new mutation. Accepted files use
same-directory temporary files and atomic rename individually; temporary files are removed after
write or rename failure.

## Compatibility Oracle

The former complete Rust `gcpm new` implementation and member parser compile only for tests or the
explicit `parity-harness` feature. The retained oracle fixes representative document and report
identities and cannot be called by the production adapter. It is compatibility evidence, not a
fallback, verifier, or second authority.

## Nonclaims

This contract does not claim generic TOML or path semantics; workspace remove, migrate,
environment, task, manifest, or scaffold authority; filesystem policy; pairwise crash-atomic
two-file commit; WASI support; H2 workspace closure; `R4.2.e` or SH-C closure; bootstrap fixpoint;
or release qualification.
