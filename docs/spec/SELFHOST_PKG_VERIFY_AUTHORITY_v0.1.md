# Self-hosted package verify authority v0.1

Status: normative partial authority contract for `R4.2.e`.

## Scope

The artifact-loaded `core/pkg::verify-authority` binding is the exclusive production semantic
authority for `core/pkg-low::verify` around bounded artifact-store and VCS schema mechanisms. It
owns deterministic dependency order, exact request/plan/observation binding, canonical hash
observation order, fail-fast outcome classification, sealed error code and message selection,
checked and missing accounting, and the exact public verify report.

Rust may perform bounded file-presence checks, BLAKE3 byte-integrity checks, CoreForm decoding,
snapshot/commit/evidence/attestation schema parsing, and commit-closure traversal. Those are typed
mechanism observations. Rust MUST NOT choose the final error class, construct the success report,
reorder dependencies or hashes, continue host reads after a terminal observation, silently fall
back to native verify semantics, or accept a result not bound to the exact request and plan.

## Causal Protocol

1. `:plan` receives the exact typed lock model before any artifact-store access.
2. The authority emits every locked dependency in canonical map order with only its name,
   snapshot, and optional commit, plus the exact lock path. The plan carries a BLAKE3 identity.
3. Rust executes only that plan. Snapshot shallow references are parsed as a mechanism, then
   sorted and deduplicated before presence and integrity observation.
4. Missing snapshots and shallow references are nonterminal report facts. Corrupt or malformed
   snapshots and any commit-closure failure are terminal. Rust stops observation immediately and
   supplies the exact terminal prefix; otherwise it supplies all observations.
5. `:finalize` recomputes and binds the plan, validates exact prefix coverage, strict hash order,
   observation types and status coherence, rejects facts after a terminal status, and emits either
   the exact public report or one closed semantic error.

The closed terminal inventory is artifact corruption or commit-closure absence, malformed
snapshot, commit, evidence, or attestation, commit/snapshot mismatch, and missing evidence for a
commit with obligations. The authority derives contextual messages for missing/corrupt hashes,
snapshot mismatch, and missing evidence. Parser detail is transported only for the corresponding
closed malformed-artifact status.

## Request And Result

Both requests use kind `genesis/pkg-verify-request-v0.1`, version 1, and operation `:plan` or
`:finalize`. Plan requests contain exactly `[:kind :model :op :v]`. Finalize requests contain
exactly `[:kind :model :observations :op :plan :plan-h :v]`.

Every result contains exactly `[:code :kind :message :ok :request-h :v :value]`, uses kind
`genesis/pkg-verify-result-v0.1`, and binds the canonical complete request hash. Protocol rejection
uses only `core/pkg/bad-authority-request`. A plan value contains only `:plan` and `:plan-h`; the
plan contains only `:dependencies` and `:lock`; each step contains only `:commit`, `:name`, and
`:snapshot`.

Finalize success contains exactly `:code`, `:decision`, `:message`, and `:report`. A report decision
uses nil code/message and a report containing exactly `:checked`, `:lock`, `:missing`, and `:ok`.
An error decision uses a nil report and one code from the closed semantic inventory. The strict
GenesisCode authority requires positive checked counts for successful commit closure. The Rust
adapter independently verifies field closure, request identity, plan hash and domains, nonnegative
report counts, lowercase hash forms, decision closure, report/error shape, and exact
`:ok`/`:missing` coherence.

## Bounds And Failure

Verify shares the package-resolution artifact-only `EvalCtx`; no additional bootstrap is
introduced. Each call has a 20,000,000-step, 40,000,000-allocation, 4 MiB bytes/string, and 65,536
map/vector-entry ceiling. Missing artifact or binding, exhausted evaluation, sealed or opaque
results, open maps, plan substitution, reordered or duplicate hashes, observation gaps, trailing
post-terminal facts, negative accounting, and open statuses fail closed before a public report.

## Compatibility Oracle

The former native Rust verify workflow is reachable only under tests or the explicit
`parity-oracle` feature. It supports differential compatibility checks and is not a production
fallback. Production also fails closed before store access when the GenesisCode authority is
unavailable.

## Nonclaims

This contract does not claim snapshot, commit, evidence, or attestation schema parsing; BLAKE3 or
artifact-store mechanisms; package graph solving; registry or ref transport; generic lock or TOML
syntax authority; workspace scaffolding; H2 package resolution; `R4.2.e` or SH-C closure;
bootstrap fixpoint; or release qualification.
