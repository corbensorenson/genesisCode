# Self-hosted package semver selection authority v0.1

Status: normative partial authority contract for `R4.2.e`.

## Scope

The artifact-loaded `core/pkg::semver-select-authority` binding is the exclusive production
semantic authority for choosing one package ref from host-observed semver candidates under the
normalized `:highest` or `:lowest` policy. It owns policy extremum selection, deterministic
lexicographically smallest ref tie-breaking among equal versions, exact-member selection, and the
empty-candidate no-match decision used by `core/pkg-low::{lock,update,install}`.

Rust remains a bounded mechanism for semver syntax parsing, range matching, precedence comparison,
ref observation, registry transport, and assigning equal nonnegative ranks to equal semver
precedence classes. It may reject malformed or substituted authority output, but it MUST NOT choose
the policy extremum or ref tie-break in production. The former Rust selector is compiled only for
tests or the explicit `parity-oracle` feature.

## Bootstrap and limits

The semver selector shares the package resolution authority's single artifact-loaded `EvalCtx` and
its 2,000,000-step, 4,000,000-allocation-unit, 64-KiB byte, 16-KiB string, and 64-entry limits.
Production uses `SelfhostBootstrapMode::ArtifactOnly` and fails closed with
`core/pkg/authority-error` when the artifact, binding, evaluation, or closed result is unavailable.

## Request

Every request is exactly:

```text
{
  :candidates [{:commit <hex64> :rank <nonnegative-int> :ref "refs/tags/..."} ...]
  :kind "genesis/pkg-semver-select-request-v0.1"
  :op :select
  :policy :highest | :lowest
  :v 1
}
```

Ranks are semver-mechanism facts: lower ranks have lower precedence and equal versions have equal
ranks. Candidate vector order has no meaning. Candidate maps are closed; commits are exact hex64;
refs must be under `refs/tags/`; and malformed types, negative ranks, open fields, unknown policy,
kind, operation, or version are rejected.

## Decision

`:lowest` selects the candidate with the smallest rank and `:highest` selects the largest rank.
When multiple candidates share that rank, the bytewise UTF-8 lexicographically smallest ref wins.
An empty candidate vector returns a successful no-match with nil ref, commit, and rank. A nonempty
candidate vector can never return no-match.

## Result

Every result has exactly:

```text
[:code :commit :kind :message :ok :rank :ref :request-h :v]
```

`:kind` is `genesis/pkg-semver-select-result-v0.1`, `:v` is 1, and `:request-h` binds the complete
request. Success has nil code/message and either an exact candidate tuple or the empty-set nil
triple. Rejection uses only `core/pkg/bad-authority-request` and has nil candidate fields. Rust
rejects open, mistyped, unbound, substituted, non-member, or false no-match results before package
state changes.

## Nonclaims

This contract does not claim semver grammar or range-matching authority, ref or registry transport,
complete package graph solving, generic lock codec authority, H2 package resolution, `R4.2.e` or
SH-C closure, bootstrap fixpoint, workspace authority, or release qualification.
