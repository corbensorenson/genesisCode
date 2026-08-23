# Self-hosted package workspace authorities v0.1

Status: normative partial authority contract for `R4.2.e`. The filename is retained for stable
references; this document governs workspace-new, workspace-remove, workspace-migrate,
workspace-environment-selection, and workspace-task profiles.

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

Production `gcpm env` loads `core/pkg::workspace-env-select-authority` directly. Production
`gcpm run` loads `core/pkg::workspace-task-authority`, which invokes the same backend authority
inside the exact self-host artifact before task interpretation. Both routes complete backend
admission before creating an environment directory, materializing a runtime bridge, joining a task
path, reading a task file, or dispatching a task. Rust supplies only the requested profile, optional
command override where applicable, raw optional selected-profile and default workspace
observations, and the active compiled runtime-backend profile in the closed request
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
compatibility for either route. The structurally admitted workspace is passed directly into the
composed GenesisCode task authority, so `gcpm run` cannot reload it through a native backend
validator or native task parser. No native backend selector is reachable from either production
route.

Authority rejection, missing artifact/binding, nonclosed request or result, request-hash mismatch,
invalid selected/source/active result, malformed workspace structure, incompatible selection, or
missing required profile/capability input fails before environment materialization or task
resolution. The adapter
independently checks the closed selected and active inventory but does not recompute precedence,
normalization, source, or compatibility.

## Workspace environment authority contract

Production `genesis gcpm env` evaluates `core/pkg::workspace-env-authority` from the exact admitted
self-host toolchain artifact. This is a request-bound two-phase authority so filesystem-dependent
facts are observations rather than ambient GenesisCode effects. The plan request kind is
`genesis/pkg-workspace-env-plan-authority-request-v0.1`; it contains closed, bounded structural
workspace and lock facts, exact admitted workspace/lock bytes, raw profile/default values, active
backend and command override observations, member package-manifest presence and hashes, the output
root prefix and platform separator, and explicit workspace, lock, store, and WASI bridge paths.
GenesisCode first composes `core/pkg::workspace-env-select-authority`, rejects missing locked
objects, and returns a request-bound canonical plan plus its canonical term hash.

GenesisCode exclusively owns member and dependency projection; profile/default precedence for
policy, registry, toolchain, and capability-policy source; backend-required status; canonical
member/dependency bodies; workspace, lock, member, and dependency hashes; and the complete closed
plan. Only after strict host decoding may Rust observe the exact plan-selected capability policy
and optional toolchain as regular non-symlink files, resolve a bounded optional registry bridge
root, and plan the active backend launcher and effective capability bytes. Those observations are
closed, bounded, path- and hash-bearing facts; they grant no semantic authority.

Finalization uses kind `genesis/pkg-workspace-env-finalize-authority-request-v0.1` and embeds the
exact original plan request, plan, plan hash, and observations. The authority revalidates the
request, recomputes the plan, compares its canonical identity, and rejects substitution or any
contradictory observation. Both phases return the closed request-hash-bound result kind
`genesis/pkg-workspace-env-authority-result-v0.1`. Rejections use only
`core/pkg/bad-workspace-env` or the composed `core/pkg/bad-workspace-env-selection`.

The final GenesisCode decision owns the canonical `:gcpm/env` v2 identity term and body. Its
environment hash binds workspace, lock, members, dependencies, capability-policy bytes,
effective policy, selected and active backend, toolchain path and bytes, backend effective-policy
bytes, backend launcher digest, registry/WASI roots, and every profile fact that can change the
materialized environment. It also owns canonical profile, provenance, and WASI runtime descriptor
bodies; every body hash; exact ordered immutable filenames and bytes; the separately scoped
external runtime descriptor; required directory inventory; immutable root path; and the complete
public result. Different admitted capability-policy bytes, effective policies, toolchain paths, or
backend artifacts MUST produce different environment identities.

Rust remains a bounded mechanism. It strictly decodes every envelope, plan, file scope, filename,
body, hash, path, mkdir, and public field; independently recomputes canonical term and byte hashes;
cross-checks admitted observations; rejects unsafe relative filenames and substituted absolute
paths; and performs no environment projection, canonical body construction, identity choice, file
ordering, or report construction. Before any write, one preflight rejects symlinked/nonregular
inputs and destinations, an existing immutable root with any missing, extra, or changed file, and
all invalid external or backend destinations. Backend launcher and external descriptors use atomic
same-directory file replacement. A new immutable environment root is staged completely in a
sibling directory and published with one atomic directory rename. A corrupt existing root must
therefore fail before an external descriptor or backend launcher is changed.

Atomic immutable-root publication is not a cross-root crash transaction: backend and external
runtime paths can live outside the environment root, and the profile provides no rollback journal
across those separately atomic destinations. Generic TOML interpretation, filesystem/path policy,
backend bridge binary semantics, WASI command support, bootstrap fixpoint, and H2 workspace closure
remain outside this profile.

## Workspace task authority contract

Production `genesis gcpm run <task>` evaluates `core/pkg::workspace-task-authority` from the exact
admitted self-host toolchain artifact. The authority composes
`core/pkg::workspace-env-select-authority` before task interpretation, so an invalid or
incompatible `dev` runtime backend fails before task lookup, command normalization, argument
interpretation, path joining, file access, or command dispatch.

The closed request kind is `genesis/pkg-workspace-task-authority-request-v0.1` with exactly these
fields:

- `:active`: the bounded active executable backend observation.
- `:default`: nil or the bounded raw workspace default backend observation.
- `:engines`: the bounded closed inventory of engines compiled into the active executable.
- `:kind`: the exact request kind.
- `:profile`: exactly `dev`.
- `:profile-backend`: nil or the bounded raw `dev` backend observation.
- `:task`: the bounded exact requested task name.
- `:tasks`: at most 256 uniquely named closed task observations with exact `:args`, `:cmd`,
  `:file`, `:name`, and `:pkg` fields; each task has at most 64 bounded string arguments.
- `:v`: integer `1`.

The request hash is the canonical GenesisCode term hash of the complete request. The result kind
is `genesis/pkg-workspace-task-authority-result-v0.1`; every envelope is closed and binds that
request hash. Backend failures retain `core/pkg/bad-workspace-env-selection`. Lookup, command,
field, argument, engine, and task-shape failures use `core/pkg/bad-workspace-task`.

GenesisCode exclusively owns exact task-name lookup; duplicate and bounded task admission; Unicode
trim plus ASCII-lower command normalization; `build -> pack`, `lint -> typecheck`, and
`bench -> run` aliases; package/file precedence and the `package.toml` default; the complete
action-specific option grammar; engine normalization and membership in the observed executable
inventory; contract-hash normalization and 64-character ASCII-hex validation; required fields;
and construction of the closed canonical action.

The canonical action has exactly `:action`, `:caps`, `:check`, `:contract-h`, `:emit-wasm`,
`:engine`, `:file`, `:log`, `:out`, `:pkg`, `:stage1-gate`, `:stage1-pipeline`, `:stage2-gate`, and
`:task`. Unused optional fields are nil and unused flags are false. Ignored arguments are
forbidden: every argument must be consumed by the selected action grammar.

Rust may parse bounded TOML into structural observations, report the compiled backend and engine
inventory, load and evaluate the exact artifact, strictly decode the complete request-bound
result, join authority-selected strings to the workspace directory, verify the selected contract
file's BLAKE3 bytes against the authority-normalized hash, and dispatch the decoded typed action.
The decoder may reject malformed, contradictory, unavailable, open, or request-divergent artifact
output, but it MUST NOT look up tasks, normalize commands or engines, choose aliases or paths,
interpret arguments, default fields, or provide a production fallback.

The former Rust grammar is compiled only for unit tests or the explicit `parity-harness` feature.
It is not a production fallback, verifier, or promotion authority.

## Nonclaims

These profiles do not claim generic TOML or path semantics; package task-command implementation
authority; manifest or remaining scaffold authority; filesystem policy; cross-root crash-atomic
commit or recovery; backend bridge binary semantics; WASI command support; H2 workspace closure;
`R4.2.e` or SH-C closure; bootstrap fixpoint; or release qualification.
