# Semantic Patch Merge v0.1

Status: normative.

## Purpose

`core/cli::patch-merge` performs deterministic three-way semantic merge over canonical
GenesisCode module workspaces. The producer is pure, loaded from the reviewed self-host
artifact, and has no production Rust producer or fallback. A successful result embeds
the independently specified `core/cli::patch-diff` report from the base workspace to
the merged workspace, so the merge result and its executable semantic patch cannot
diverge.

This profile operates at module and top-level-form granularity. It does not infer symbol
renames, module moves, or refactors; recursively merge sub-form structure; merge package
manifest fields; or perform filesystem/VCS operations.

## Request

The request is a closed map containing exactly:

- `:kind` = `"genesis/patch-merge-request-v0.1"`
- `:profile` = `"genesis/patch-authority-v0.1"`
- `:v` = `1`
- `:intent` = string copied into the successful semantic patch
- `:provenance` = map copied into the successful semantic patch
- `:base`, `:left`, and `:right` = canonical workspace module vectors

Workspace admission, path portability, module canonicality, duplicate rejection, and
input-order independence are identical to `SEMANTIC_PATCH_DIFF_v0.1.md`. Unknown fields,
wrong types, malformed profile identity, or noncanonical modules fail closed.

## Merge Rules

The authority visits the sorted union of base, left, and right module paths. For each
path, it applies these rules in order:

1. If left and right are equal, select that value, including mutual deletion.
2. If left equals base, select right.
3. If right equals base, select left.
4. If both branches add the absent path differently, emit `module/divergent-add`.
5. If one branch deletes a base module changed by the other, emit
   `module/delete-modify`.
6. If base, left, and right modules have equal top-form counts, merge each form with
   rules 1-3. If both branches changed one form differently, emit
   `form/divergent-edit` at that exact form index.
7. Otherwise, emit `module/structural-divergence`.

Rules 1-3 resolve identical edits, one-sided edits, independent module changes, and
disjoint top-form changes without conflict. A conflict at any path makes the whole
operation non-applicable: no partial merged workspace or patch is returned.

## Conflict Contract

Each conflict is a closed map with exactly:

- `:code` = one of the four codes above
- `:explanation` = deterministic code-specific explanation
- `:module-path` = affected portable module path
- `:form-index` = nonnegative index for `form/divergent-edit`, otherwise `nil`
- `:base-h`, `:left-h`, `:right-h` = `hash-term` of the relevant form/module vector,
  using `nil` for an absent branch value
- `:conflict-h` = `hash-term` of the conflict map before adding `:conflict-h`

Conflicts are ordered by module path and then ascending form index. The report contains
all conflicts, not merely the first, so agents receive a complete deterministic repair
surface. Explanations are informational; codes, locations, and identities are the
machine contract.

## Report

The closed report contains exactly:

- `:kind` = `"genesis/patch-merge-v0.1"`
- `:profile` = `"genesis/patch-authority-v0.1"`
- `:v` = `1`
- `:request-h`, `:base-h`, `:left-h`, and `:right-h` = exact canonical identities
- `:ok` = merge applicability
- `:conflict-count` and `:conflicts` = exact canonical conflict set
- `:merged` and `:merged-h` = canonical merged workspace and identity on success,
  otherwise `nil`
- `:diff` = complete `genesis/patch-diff-v0.1` report from base to merged workspace on
  success, otherwise `nil`

A successful report has `:ok true`, no conflicts, a non-nil merged workspace, and a
valid embedded diff. A conflicted report has `:ok false`, one or more conflicts, and
nil `:merged`, `:merged-h`, and `:diff` fields.

## Host Boundary

The host may validate canonical inputs, load the artifact, enforce step/allocation
limits, decode the closed report, and independently reconstruct the expected merge for
verification. The verifier compares every selected form, conflict, ordering fact, hash,
and embedded diff fact. It is private validation code, exposes no merge producer, and
cannot repair or replace malformed artifact output.

Missing/non-artifact authority, poisoned output, schema drift, identity mismatch,
resource exhaustion, partial conflict output, or an invalid embedded diff fails closed.

## Determinism And Nonclaims

For fixed canonical workspaces, intent, provenance, self-host artifact, profile, and
resource limits, the report is deterministic and independent of input module-vector
order, host paths, timestamps, and file metadata.

This protocol does not establish aggregate SD-PATCH H2 or close R4.2.c. Remaining work
includes strict whole-route legacy-producer absence, native/WASI observations, and
durable independent review evidence. Final apply/report authority is separately
governed by `SEMANTIC_PATCH_APPLY_REPORT_v0.1.md`.
