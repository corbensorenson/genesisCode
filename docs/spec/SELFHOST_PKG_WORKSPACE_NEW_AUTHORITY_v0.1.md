# Self-hosted package workspace authorities v0.1

Status: normative partial authority contract for `R4.2.e`. The filename is retained for stable
references; this document governs workspace-new, workspace-remove, workspace-migrate, and
workspace-environment-selection profiles.

## Workspace-new scope

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

## Workspace-new closed protocol

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

## Workspace-new host admission and writes

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

## Workspace-new compatibility oracle

The former complete Rust `gcpm new` implementation and member parser compile only for tests or the
explicit `parity-harness` feature. The retained oracle fixes representative document and report
identities and cannot be called by the production adapter. It is compatibility evidence, not a
fallback, verifier, or second authority.

## Workspace-remove contract

Production `gcpm remove` loads `core/pkg::workspace-remove-authority` from the exact self-host
artifact. The request is the closed map `{:kind :lock :model :name :v}` where `:model` is the
already-admitted typed lock model and `:name` is non-empty. The result is request-hash-bound and
has the closed envelope `{:code :kind :message :ok :request-h :v :value}`. Success contains the
exact updated lock model and removal disposition; rejection uses only
`core/pkg/bad-workspace-remove`.

GenesisCode exclusively owns deletion of the exact string key from both `:requirements` and
`:locked`, preservation of every other normalized lock-model fact, absent-name no-op disposition,
and issuance of the exact updated lock model for the independently governed canonical lock writer.

Rust remains a bounded mechanism adapter only. It reads and normalizes the existing TOML lock,
transports the closed model, evaluates both artifact bindings under existing limits, strictly
decodes every field, transports the authority-issued updated model unchanged through
`core/pkg::lock-write-authority`, reparses and cross-checks the proposed bytes, projects the
accepted hash and authority disposition into the stable report shape, preflights the regular
non-symlink destination, and atomically persists the exact accepted bytes. There is no production
native semantic fallback.

The boundary rejects open or same-cardinality-substituted requests/results, wrong request
identities, empty names, non-map lock sections, writer rejection, malformed or hash-contradictory
bytes, removal of the wrong key, mutation or loss of unrelated fields, false removal dispositions,
and symlink destinations. The prior Rust producer is compiled only for tests or `parity-harness`.

## Workspace-migrate contract

Production `gcpm migrate` loads `core/pkg::workspace-migrate-authority` from the exact self-host
artifact. Rust parses the legacy package manifest and supplies only bounded observations in the
closed request `[:dependencies :kind :lock :member-path :package-name :package-path
:registry-default :v :workspace :workspace-file]`. `:workspace` is nil or a non-empty override;
GenesisCode selects the package name when it is nil. Paths and names are non-empty strings bounded
to 4,096 and 1,024 UTF-8 bytes respectively, registry is nil or at most 4,096 bytes, and the
dependency vector contains at most 1,024 exact `[:hash :name :path]` maps. Each dependency name
and path is non-empty; hash is nil or a string of at most 128 bytes. A malformed dependency rejects
the complete request.

GenesisCode exclusively owns workspace-name defaulting, dependency snapshot eligibility, the
canonical lock model, the one-member workspace model, fixed `pack` and `test` tasks, canonical
workspace bytes, workspace identity, dependency count, and request-bound report facts. A dependency
becomes a manual pinned `default` requirement only when its hash is exactly 64 ASCII hexadecimal
characters; missing or unusable hashes are intentionally omitted while still contributing to the
legacy-compatible dependency count. Repeated valid names use ordered last-entry-wins map semantics.
The lock model is version 2 with `policy:default-v0.1`, optional `default` registry, empty locked and
artifact maps, and eligible requirements. The workspace is version 1 with one `package` member,
optional default registry, default policy, no profiles, and lexical `pack` then `test` task sections
bound to the observed package path.

Every result uses the closed request-hash-bound workspace authority envelope. Success contains
exactly `[:lock-model :report :workspace-body]`; the authority report contains exactly
`[:dep-count :lock :lock-h :ok :workspace :workspace-file :workspace-h]`, with nil `:lock-h` until
the independently governed `core/pkg::lock-write-authority` accepts the exact model. Rust may insert
only that writer-issued, byte-verified lock hash into the already closed report. Rejection uses only
`core/pkg/bad-workspace-migrate` and a nil value.

Rust remains a bounded mechanism adapter. It transports manifest/path observations, evaluates the
two artifact authorities separately, strictly decodes all field inventories and request identities,
independently parses both proposed documents, reconstructs no semantic output, and cross-checks
workspace, member, defaults, tasks, registries, requirements, selectors, strategies, policies, and
hashes against the original observations. Before any directory creation or file write it rejects
identical destinations, symlinked ancestors or destinations, and existing non-file destinations.
Accepted outputs use same-directory temporary files and atomic rename individually. The former
complete Rust migration producer is test/`parity-harness` only and is not a production fallback.

The boundary rejects open or substituted request/result maps, wrong request identities, empty
overrides, malformed dependency facts, writer rejection, malformed or hash-contradictory bytes,
document/report contradictions, identical paths, symlink boundaries, and non-file destinations.
Preflight guarantees these known destination conflicts cause no file mutation; this profile does
not claim pairwise crash atomicity after the first individually atomic rename.

## Workspace environment selection contract

Production `gcpm env` and `gcpm run` backend admission load
`core/pkg::workspace-env-select-authority` from the exact self-host artifact before creating an
environment directory, materializing a runtime bridge, or resolving a workspace task. Rust
supplies only the requested profile, optional command override, raw optional selected-profile and
default workspace observations, and the active compiled runtime-backend profile in the closed request
`[:active :default :kind :override :profile :profile-backend :v]`. Profile names are non-empty and
at most 256 UTF-8 bytes. Backend observations are nil or strings of at most 64 UTF-8 bytes. The
active value must normalize to the closed backend inventory. `gcpm run` selects the fixed `dev`
profile with no command override; `gcpm env` uses its requested profile and optional override.

GenesisCode exclusively owns precedence (`override` then selected profile then workspace default
then built-in `headless`), Unicode whitespace trimming, ASCII case folding, `profile-` alias
normalization, selected-source attribution, and compatibility with the active backend. Canonical
backends are exactly `headless`, `gpu`, `gfx`, and `backend`. `headless` is universally compatible;
`gpu` and `gfx` require their matching active backend or `backend`; and selected `backend` requires
active `backend`. Bounded but semantically invalid lower-precedence strings are intentionally
masked by a present valid higher-precedence value. An invalid selected value rejects the complete
request.

Every result uses the closed request-hash-bound envelope
`[:code :kind :message :ok :request-h :v :value]` and kind
`genesis/pkg-workspace-env-select-authority-result-v0.1`. Success has nil code/message and a value
containing exactly `[:active :compatible :selected :source]`; source is one of `:override`,
`:profile`, `:default`, or `:builtin`. Rejection uses only
`core/pkg/bad-workspace-env-selection` and nil value.

Rust performs bounded TOML and active-backend observation, preserves raw backend strings while the
shared host parser admits every non-backend workspace structure, evaluates the artifact, and
strictly decodes the complete response and request identity. It may reject non-string TOML values
as transport-shape violations, but MUST NOT normalize, validate, select, default, or determine
compatibility for either route. The admitted workspace is passed directly into post-selection task
resolution, so `gcpm run` cannot reload it through a native backend validator. No native selector
is reachable from either production route.

Authority rejection, missing artifact/binding, nonclosed request or result, request-hash mismatch,
invalid selected/source/active result, malformed workspace structure, incompatible selection, or
missing required profile/capability input fails before environment materialization or task
resolution. The adapter
independently checks the closed selected and active inventory but does not recompute precedence,
normalization, source, or compatibility.

## Nonclaims

These profiles do not claim generic TOML or path semantics; workspace environment descriptor,
projection, hashing, or materialization authority; general task resolution; manifest or remaining
scaffold authority; filesystem policy; pairwise crash-atomic two-file commit or recovery; WASI
support; H2 workspace closure; `R4.2.e` or SH-C closure; bootstrap fixpoint; or release
qualification.
