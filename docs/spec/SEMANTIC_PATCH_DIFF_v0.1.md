# Semantic Patch Diff v0.1

Status: normative.

## Purpose

`core/cli::patch-diff` derives one canonical GenesisCode semantic patch from two
canonical module workspaces. The binding is pure, loaded from the reviewed self-host
artifact, and has no production Rust producer or fallback.

This v0.1 profile covers module creation, removal, whole-module structural replacement,
and changed top-level forms. Manifest-field diff, rename/move inference, recursive
within-form minimization, semantic three-way merge, and final filesystem application
remain separate R4.2.c work. Absence of those features is an explicit profile boundary,
not permission for a host implementation to invent them.

## Request

The request is a closed map with exactly:

- `:kind` = `"genesis/patch-diff-request-v0.1"`
- `:profile` = `"genesis/patch-authority-v0.1"`
- `:v` = `1`
- `:intent` = string copied to the semantic patch
- `:provenance` = map copied to the semantic patch
- `:base` = workspace module vector
- `:target` = workspace module vector

Each workspace module is a closed map with exactly `:module-path` and `:forms`.
`:module-path` is a portable NFC package-relative path under `PATCH_SCHEMA.md`.
`:forms` is a canonical module-form vector. Duplicate paths, noncanonical forms,
unknown fields, wrong types, and malformed request identity fail closed.

Input module-vector order is not semantic. The authority canonicalizes each workspace
to ascending CoreForm map-key order before diffing and identity calculation.

## Canonical Diff

The authority visits the sorted union of base and target module paths exactly once.
For each path:

1. Equal modules emit no operation.
2. A target-only module emits one `:add-module` with canonical forms as `:content`.
3. A base-only module emits one `:remove-module`.
4. Changed modules with equal top-level form counts emit one `:replace-node` for each
   unequal form, in ascending form index order. Each path is exactly `[[:form i]]`.
5. Changed modules with unequal top-level form counts emit one `:remove-module`
   immediately followed by one `:add-module` for the same path.

This is the v0.1 minimality rule: no unchanged module or form is represented, no two
operations target the same equal-count form, and an arity-changing module uses the only
two-operation sequence expressible by patch schema v1 without index-shift ambiguity.
The rule minimizes operation count at top-form granularity. It does not claim globally
minimal bytes or infer higher-level refactors.

The raw patch is normalized by the GenesisCode-authoritative `core/cli::patch-normalize`
binding before publication. Therefore all operation schemas, set normalization,
semantic patch identity, and ordered operation identities remain governed by
`PATCH_SCHEMA.md`.

## Report

The closed report contains exactly:

- `:kind` = `"genesis/patch-diff-v0.1"`
- `:profile` = `"genesis/patch-authority-v0.1"`
- `:v` = `1`
- `:ok` = `true`
- `:request-h` = `hash-term(request)`
- `:base-h` = `hash-term(canonical base workspace vector)`
- `:target-h` = `hash-term(canonical target workspace vector)`
- `:patch` = normalized semantic patch
- `:patch-h` = `hash-term(:patch)`
- `:op-count` = exact operation count
- `:op-identities` = ordered patch-normalization identities
- `:stats` = closed `{:additions int :removals int :replacements int}` map

Every hash is lowercase hexadecimal with 64 characters at the host boundary.

## Host Boundary

The host may construct and validate requests, load the artifact, enforce deterministic
step/allocation limits, decode the closed report, and commit an accepted patch through
the separately governed transactional apply path. It cannot generate, repair, reorder,
or minimize a diff.

The independent decoder verifies:

- closed request/report/statistics/operation-identity schemas;
- exact request, canonical workspace, patch, and operation hashes;
- intent and provenance binding;
- exact sorted path and ascending form-index order;
- absence of no-op, overlapping, duplicate, missing, or trailing operations;
- exact add/remove pairing for arity-changing modules;
- complete reconstruction facts for every base/target module pair.

Malformed, poisoned, missing, non-artifact, resource-exhausted, or identity-mismatched
authority output fails closed. The verifier contains no callable patch producer and is
not a fallback.

## Determinism And Nonclaims

For fixed canonical workspace terms, intent, provenance, self-host artifact, semantic
profile, and resource limits, the report and patch are deterministic across native and
WASI consumers. Host paths, timestamps, file metadata, and workspace-vector ordering do
not enter semantic patch identity.

This protocol establishes neither aggregate SD-PATCH H2 nor R4.2.c closure. Those claims
still require GenesisCode-authoritative merge, complete apply/report outcomes, strict
legacy-producer absence, native/WASI observations, and independent durable evidence.
