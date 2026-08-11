# Semantic Refactor Plan v0.1

Status: normative for production `semantic-edit refactor-plan` and `apply-plan`.

## Purpose

This contract makes workspace rename, move, and extract planning a closed,
deterministic GenesisCode decision. The artifact-loaded binding
`core/cli::refactor-plan` is the sole production planner. The host may load inputs,
enforce resource bounds, decode the closed report, and independently reject an
invalid result; it must not discover affected modules, select conflicts, construct a
replacement plan, or silently fall back to a Rust planner.

The profile is `genesis/patch-authority-v0.1`. This contract composes with the
canonical patch format in `PATCH_SCHEMA.md`; it does not define a second patch
identity.

## Closed Request

The request kind is `genesis/refactor-plan-request-v0.1`. The request is a CoreForm
map containing exactly:

- `:kind`: the request-kind string.
- `:profile`: `genesis/patch-authority-v0.1`.
- `:v`: integer `1`.
- `:refactor-kind`: one of `rename`, `move`, or `extract`.
- `:from-symbol`: the non-empty source symbol string.
- `:to-symbol`: the non-empty destination symbol string.
- `:target-module-path`: an empty string for no target, or a portable
  package-relative module path.
- `:modules`: the complete package module vector in manifest order.

Each module record contains exactly `:module-path` and `:forms`.
`:module-path` is unique and satisfies the portable path rules in
`PATCH_SCHEMA.md`; `:forms` is the canonical module-form vector produced by the
selected source frontend. Duplicate paths, malformed records, or omitted manifest
modules fail before planning.

The canonical request identity is `hash-term(request)` under the repository hash
profile. Host paths, timestamps, process identities, and presentation-only CLI data
are absent from the request.

## Closed Report

The report kind is `genesis/refactor-plan-v0.1`. A report contains exactly:

- `:kind`, `:profile`, and `:v` with the fixed identities above.
- `:request-h`, equal to the canonical request identity.
- `:ok` and `:safe-to-apply`, equal booleans.
- `:module-count`, equal to the request module count.
- `:replacement-count`, the exact number of source-symbol terms rewritten by all
  rename operations.
- `:op-count`, equal to the canonical patch operation count.
- `:op-identities`, one closed `{:op-h string :ordinal int}` record per operation,
  in operation order.
- `:conflicts`, a vector of closed conflict records.
- `:patch`, either the normalized canonical patch or `nil`.
- `:patch-h`, either `hash-term(:patch)` or the empty string.

Each conflict record contains exactly `:code`, `:message`, `:module-path`, and
`:path-repr`. The final two fields use an empty string when no location applies.
Conflict codes are closed to:

- `refactor/kind-invalid`
- `refactor/source-symbol-invalid`
- `refactor/destination-symbol-invalid`
- `refactor/no-op`
- `refactor/source-symbol-missing`
- `refactor/source-symbol-ambiguous`
- `refactor/destination-symbol-exists`
- `refactor/target-module-required`
- `refactor/target-module-invalid`
- `refactor/target-module-exists`
- `refactor/target-order-dependency`

A conflicted report has non-empty `:conflicts`, false status booleans, `nil :patch`,
empty `:patch-h` and `:op-identities`, and zero operation and replacement counts. A
safe report has no conflicts, true status booleans, a normalized patch, a non-empty
canonical patch hash, exact operation identities, and positive operation and
replacement counts. A conflicted report can never carry patch authority.

## Planning Semantics

A workspace definition is a canonical top-level `(def <symbol> <rhs>)` form. The
planner derives definitions and source-symbol occurrence counts from the supplied
canonical forms; the host does not supply a symbol index as authority.

All modes reject an absent or ambiguous source definition, an existing destination
definition, malformed symbols, and equal source and destination symbols. Operations
are minimized at module granularity: one `:rename-symbol` operation is emitted for
every and only manifest-ordered module containing at least one source-symbol term.
Each operation carries the exact requested `:from` and `:to` values. Descendant
`:replace-node` operations are forbidden because they would overlap the module-level
rename.

### Rename

`rename` emits only the manifest-ordered `:rename-symbol` operations. An optional
target argument has no semantic effect and does not turn a rename into a move.

### Move and Extract

`move` and `extract` have the same v0.1 structural operation contract. They require a
new portable target module path. After the rename operations, the plan emits exactly:

1. one `:split-module` from the source-definition module to the requested target,
   with `:symbols` equal to a one-element vector containing the destination symbol;
2. one final `:update-manifest` whose only update is `:set {:modules ...}`.

The manifest vector inserts the target immediately before the source module and
otherwise preserves request order. Every module record is exactly
`{"hash" "" "path" <path>}`; package apply re-pins hashes after mutation. Placing the
target before the source makes the moved definition available to the source module's
remaining forms.

The move is rejected with `refactor/target-order-dependency` when the moved
definition's right-hand side references any workspace definition in the source
module or a later module. Such a dependency would not be available at the target's
new position. This conservative rule is deterministic; a future broadening requires
a new version and explicit dependency-order semantics.

## Independent Host Verification

The host decoder is fail-closed and separate from the GenesisCode producer. Before
exposing or applying a safe plan it verifies:

- exact request/report identities, fields, types, status relations, counts, and
  conflict-code allowlist;
- canonical patch and per-operation hashes;
- exact requested rename operands;
- exact affected-module set and replacement count by an independent read-only scan
  of the request forms;
- unique manifest-ordered rename operations;
- the exact split source, target, and symbol vector for move/extract;
- the exact target-before-source manifest payload and absence of unrelated manifest
  edits;
- absence of unsupported, duplicate, missing, or out-of-order operations.

Any mismatch is a verification error, not a planner conflict. Production requires an
artifact-only selfhost frontend with an explicit toolchain artifact. Missing bindings,
resource exhaustion, malformed reports, and attempted overrides fail closed. There
is no reachable host planner fallback.

`apply-plan` applies only the verified normalized patch through the ordinary patch
preflight, transactional apply, obligation, evidence, and rollback pipeline. The
planner cannot grant capabilities or bypass those controls.

## Determinism and Resources

Module order comes only from the package manifest. Definition collection, affected
module order, conflicts, operations, identities, and manifest replacement are pure
functions of the closed request. Evaluation is charged to the caller's explicit step
and memory limits. The protocol performs no filesystem, time, random, network,
process, environment, model, or UI effect.

## Nonclaims

- This protocol establishes production planning authority for workspace refactors;
  it does not by itself close the broader `SD-PATCH` decision or R4.2.c.
- Semantic diff and three-way merge authority remain separate unfinished work.
- Artifact-loaded routing and a passing decoder do not prove H3 bootstrap fixpoint
  or H4 independent implementation.
- `SELFHOST_REFACTOR_PIPELINE_v0.1.md` governs decomposition of the selfhost source
  tree and is not the application-facing semantic refactor planner.
