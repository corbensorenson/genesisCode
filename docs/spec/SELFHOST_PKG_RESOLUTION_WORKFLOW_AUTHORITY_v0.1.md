# Self-hosted package resolution workflow authority v0.1

Status: normative partial authority contract for `R4.2.e`.

## Scope

The artifact-loaded `core/pkg::resolution-workflow-authority` binding is the exclusive production
semantic authority for the causal `core/pkg-low::{lock,update}` workflow around individual
resolver mechanisms. It normalizes `--only`, emits the complete ordered pre-mechanism step plan,
validates exact step observations, constructs the final locked map, classifies every action,
computes selected/updated/locked counts, projects dependency provenance, and constructs the exact
canonical rationale and workspace snapshot objects and their BLAKE3 identities.

Rust may decode the already admitted typed lock model, execute only returned resolver steps,
observe refs/registries/artifacts, validate non-publish artifact closure, parse commit objects,
persist exact authority-issued bytes, and atomically replace the lock file through the separately
governed writer. It MUST NOT select dependencies, classify update outcomes, compare locked entries
for update policy, construct rationale/workspace terms, project lock/update provenance, or invent
an unplanned resolver operation in production.

## Causal Protocol

The two phases share one exact typed model fact.

1. `:plan` runs before registry or resolver mechanisms and returns `{:plan :plan-h}`.
2. Rust executes only `:resolve` or `:consider` steps, records typed resolver-plan/results, and
   records empty observations for skipped or missing selections.
3. `:finalize` receives the original model term, exact plan and identity, ordered observations,
   raw commit parse observations, and strictness.
4. Finalization recomputes the plan identity, rejects any missing, extra, reordered, mistyped, or
   contradictory observation, and returns exact semantic state and storage objects.
5. Strict artifact/commit closure validation runs before object persistence. Rust then writes only
   exact returned bytes and verifies the store identity before the separate lock writer runs.

The normalized `--only` set trims ASCII boundary whitespace, removes empty entries and duplicates,
and is ordered by canonical string-map order. Lock plans resolve every requirement. Update plans
emit `:consider` for selected requirements, `:skip-unselected` for unselected requirements, and one
`:missing-requirement` step for each selected name absent from requirements.

## Request And Result

Both requests use kind `genesis/pkg-resolution-workflow-request-v0.1`, version 1, and an operation
of `:plan` or `:finalize`. Plan requests contain exactly `[:kind :model :only :op :v :workflow]`.
Finalize requests contain exactly:

```text
[:commit-observations :kind :model :observations :only :op :plan :plan-h
 :strict :v :workflow]
```

Every result contains exactly `[:code :kind :message :ok :request-h :v :value]`, uses kind
`genesis/pkg-resolution-workflow-result-v0.1`, and binds the canonical complete request hash.
Rejection uses the closed `core/pkg/bad-authority-request` class. Success plan values contain only
`:plan` and `:plan-h`. Success final values contain only:

```text
[:locked :locked-count :provenance :rationale :rationale-object
 :selected-count :updated-count :workspace-object]
```

Each object is exactly `[:bytes :h :term]`. The Rust adapter independently verifies canonical
printing, rationale newline policy, raw BLAKE3 identity, exact field closure, integer domains,
locked-entry typing, and request binding. Storage rejects any returned/store identity mismatch.

## Bounds And Failure

Workflow, selector-plan, semver-selection, and requirement-identity bindings share one artifact-only
`EvalCtx`; no additional toolchain bootstrap is introduced. Each authority call has a fixed
20,000,000-step, 40,000,000-allocation, 4 MiB bytes/string, and 65,536 map/vector-entry ceiling.
Missing artifacts/bindings, evaluation exhaustion, sealed or opaque results, open maps, plan drift,
observation substitution, malformed commit facts, contradictory object identities, and false
strict provenance fail closed before lock persistence.

## Compatibility Oracle

The former Rust selection, action classification, locked-entry comparison, rationale construction,
workspace construction, and lock/update provenance projection are reachable only under tests or the
explicit `parity-oracle` feature. They support differential compatibility checks and are not a
production fallback. Install-time provenance remains a separately named residual compatibility
path until its later R4.2.e migration.

## Nonclaims

This contract does not claim semver grammar/range/rank authority, ref or registry transport
authority, complete dependency graph solving, non-publish artifact/commit validation authority,
install workflow authority, workspace scaffolding, generic lock syntax authority, H2 package
resolution, `R4.2.e` or SH-C closure, bootstrap fixpoint, or release qualification.
