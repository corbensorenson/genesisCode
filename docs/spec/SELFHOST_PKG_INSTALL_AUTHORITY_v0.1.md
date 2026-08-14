# Self-hosted package install authority v0.1

Status: normative partial authority contract for `R4.2.e`.

## Scope

The artifact-loaded `core/pkg::install-authority` binding is the exclusive production semantic
authority for `core/pkg-low::install` around bounded host hydration mechanisms. It decides frozen
missing-lock admission, emits the complete ordered dependency plan, applies locked-entry registry
precedence, decides whether a missing snapshot may invoke resolution, binds exact ordered host
observations to that plan, and constructs the final install report and dependency provenance.

Rust may decode the separately governed typed lock model, read and hydrate content-addressed
objects, invoke an authority-planned resolver operation, parse snapshots and commits, validate
strict non-publish commit closure, and render sealed diagnostics. It MUST NOT select dependency
order, choose registry precedence, admit an incomplete frozen lock, invent resolution eligibility,
accept substituted observations, or construct the final install verdict or provenance in
production.

## Causal Protocol

1. `:plan` receives the exact typed lock model and the frozen, strict, and refs-available facts
   before artifact-store access or hydration.
2. The authority either rejects a frozen incomplete lock with the canonical ordered missing set or
   returns an ordered dependency plan and its BLAKE3 identity.
3. Rust executes only returned steps. For each step it records initial snapshot presence,
   resolution disposition, an ordered snapshot-first hash-presence inventory, optional commit
   presence, and the nonnegative count returned by strict closure validation.
4. `:finalize` receives the original model, exact plan and identity, exact ordered observations,
   and raw commit parse observations.
5. Finalization recomputes the plan, rejects missing, extra, reordered, mistyped, negative, or
   contradictory observations, then returns the exact checked count, missing hashes, verdict,
   workspace root, and dependency provenance.

The registry for each step is the locked entry registry when present, otherwise the corresponding
requirement registry. Resolution is eligible only when the snapshot was initially absent, refs are
available, and a requirement exists. The planned snapshot is always the first hash observation so
the authority can bind host hydration to the intended dependency.

## Request And Result

Both requests use kind `genesis/pkg-install-request-v0.1`, version 1, and an operation of `:plan`
or `:finalize`. Plan requests contain exactly:

```text
[:frozen :kind :model :op :refs-available :strict :v]
```

Finalize requests contain exactly:

```text
[:commit-observations :frozen :kind :model :observations :op :plan :plan-h
 :refs-available :strict :v]
```

Every result contains exactly `[:code :kind :message :ok :request-h :v :value]`, uses kind
`genesis/pkg-install-result-v0.1`, and binds the canonical complete request hash. Rejection uses the
closed `core/pkg/bad-authority-request` class. A plan success contains only `:admit`,
`:missing-locks`, `:plan`, and `:plan-h`. The admitted plan contains only `:dependencies`,
`:frozen`, `:refs-available`, `:strict`, and `:workspace-root`. Each dependency step contains only
`:commit`, `:name`, `:registry`, `:resolve-if-missing`, and `:snapshot`.

The final report contains exactly `:checked`, `:lock`, `:missing`, `:ok`, `:provenance`, and
`:workspace-root`. The Rust adapter independently verifies envelope and field closure, hash and
integer domains, plan identity, flag coherence, ordered step typing, final provenance/report
coherence, and request binding.

## Bounds And Failure

Install, workflow, selector-plan, semver-selection, and requirement-identity bindings share one
artifact-only `EvalCtx`; no additional bootstrap is introduced. Each authority call has a fixed
20,000,000-step, 40,000,000-allocation, 4 MiB bytes/string, and 65,536 map/vector-entry ceiling.
Missing artifacts or bindings, evaluation exhaustion, sealed or opaque results, open maps, plan
drift, observation substitution, malformed commit facts, negative resource counts, and
contradictory provenance fail closed before a successful report.

## Compatibility Oracle

The former Rust install workflow and install-time provenance construction are reachable only under
tests or the explicit `parity-oracle` feature. They support differential compatibility checks and
are not a production fallback.

## Nonclaims

This contract does not claim semver grammar/range/rank authority, ref or registry transport
authority, complete dependency graph solving, non-publish artifact/commit validation authority,
generic lock or TOML syntax authority, workspace scaffolding, H2 package resolution, `R4.2.e` or
SH-C closure, bootstrap fixpoint, or release qualification.
