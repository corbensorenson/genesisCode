# Semantic Patch Apply Report v0.1

Status: normative.

## Purpose

`core/cli::patch-apply-report` is the sole production semantic producer for the final
patch outcome and report consumed by `apply-patch`. It binds the accepted normalized
patch, physical application observations, packed package identity, and stored
acceptance artifact into one deterministic `genesis/patch-apply-v0.3` report.

The binding is pure and loaded from the reviewed self-host artifact. Filesystem
mutation, transactional rollback, content-addressed storage, package packing, and
obligation execution remain declared host mechanisms. They provide bounded facts but
cannot construct, repair, or replace the semantic report.

## Request

The closed request contains exactly:

- `:kind` = `"genesis/patch-apply-report-request-v0.1"`
- `:profile` = `"genesis/patch-authority-v0.1"`
- `:v` = `1`
- `:patch` = accepted normalized semantic patch
- `:source-patch-h` = original submitted patch identity
- `:package-artifact` = content-addressed packed package identity
- `:acceptance-artifact` = verified content-addressed acceptance identity
- `:acceptance` = stored `genesis/acceptance-v0.2` term matching that identity
- `:semantic-edits` = ordered host observations emitted by artifact-governed semantic
  transformations

All three artifact identities are lowercase 64-character BLAKE3 hex at the host
boundary. The GenesisCode authority validates 32-byte hex decoding, acceptance kind
and boolean outcome, semantic-edit vector shape, portable module paths, and the full
patch through `core/cli::patch-normalize`.

## Report

The output is a closed map containing exactly:

- `:kind` = `"genesis/patch-apply-v0.3"`
- `:profile` = `"genesis/patch-authority-v0.1"`
- `:v` = `1`
- `:request-h` = `hash-term(request)`
- `:ok` = the stored acceptance artifact's boolean `:ok`; no host-provided success bit
- `:intent`, `:provenance`, `:ops-count`, `:patch-h`, and `:op-identities` = facts
  derived from GenesisCode patch normalization
- `:source-patch-h`, `:package-artifact`, and `:acceptance-artifact` = bound request
  identities
- `:semantic-edits` = the ordered bound observation vector

All fields are always present. This replaces the variable-field Rust-produced
`genesis/patch-apply-v0.2` report; v0.2 has no production writer after this cutover.

## Host Boundary

The host verifies the content-addressed acceptance bytes before parsing them, passes
the exact acceptance term to the authority, independently decodes the closed output,
and checks every report field against the normalized patch and observed artifacts. The
decoder exposes no report producer. Authority admission, execution, schema, identity,
or fact mismatch fails closed and triggers the existing workspace rollback transaction.

A separately compiled parity harness may use the Rust transformation oracle, but it
must still load `core/cli::patch-apply-report`; no parity or compatibility feature may
restore the retired Rust report producer.

## Determinism And Nonclaims

For fixed patch, artifact identities, acceptance term, semantic-edit observations,
self-host artifact, profile, and resource limits, the report is deterministic. Host
paths, timestamps, mutable workspace metadata, and presentation strings do not enter
the report.

This protocol establishes the production report-authority cutover, not aggregate
SD-PATCH H2 or R4.2.c closure. Promotion still requires strict whole-route fallback
absence, native/WASI parity, mutation controls, and durable independently reviewed
evidence.
