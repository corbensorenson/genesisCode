# Self-host Obligation Authority v0.1

Status: normative partial H2 migration contract for `SD-OBLIGATION`. This version
does not promote the ledger row or close R4.2.d.

`core/cli::obligation-authority` is the sole production semantic producer for the
`core/obligation::unit-tests` and `core/obligation::budgets` decisions. The host
executes test bodies, enforces previously authorized effects, records canonical
value hashes and sealed-error status, measures steps and effect-log sizes, and
persists opaque effect logs. It does not decide whether an expectation matches or a
budget is exceeded.

## Closed Protocol

The request kind is `genesis/obligation-authority-request-v0.1` and contains exactly
`:kind`, `:v`, `:operation`, `:package`, `:limits`, and `:tests`.

For `:unit-tests`, `:limits` is empty and every ordered observation contains exactly
`:suite`, `:name`, `:actual-h`, `:expected-h`, `:sealed-error`, and `:log-artifact`.
Hashes are 32 bytes; expected hash and log artifact are explicitly nullable. A test
passes only when it is not a sealed error and either has no expectation or its exact
canonical value hash equals the expected hash.

For `:budgets`, `:limits` contains exactly the three nullable nonnegative integer
fields `:max-steps-per-test`, `:max-effect-entries-per-test`, and
`:max-effect-log-bytes-per-test`. Every observation contains exactly `:suite`,
`:name`, `:steps`, `:effect-entries`, and `:effect-log-bytes`. A configured limit is
inclusive; only an observed value strictly greater than it fails.

The result kind is `genesis/obligation-authority-result-v0.1` and contains exactly
`:errors`, `:kind`, `:name`, `:ok`, `:operation`, `:report`, and `:v`. The embedded
report preserves the existing `genesis/unit-tests-v0.2` or `genesis/budgets-v0.2`
artifact shape and ordering. The host decoder rejects open, missing, reordered,
renamed, contradictory, or observation-substituting output before persistence.
Malformed or open requests, unknown operations, invalid facts, negative counters,
and resource exhaustion return a sealed protocol error and never synthesize a pass.

## Authority Boundary

Production package testing passes the exact selected self-host artifact and resource
limits into this authority. Rust retains only execution, measurement, artifact-store
transport, strict decoding, and contradiction rejection. The former Rust unit-test
and budget decision implementations are absent from production source. Neither an
environment variable nor a feature can silently restore them.

`policies/selfhost_obligation_authority_v0.1.json` binds the exact source, artifact,
entrypoint, migrated and residual obligation inventories, primitive host facts, and
nonclaims. `scripts/lib/selfhost_obligation_authority.py` independently validates
that profile, the production call sites and dependency graphs, removal of the two
host decision paths, and mutation controls. Focused Rust tests and native/WASI CLI
runtime observations cover matching, mismatch, sealed error, inclusive/exceeded
limits, open requests, unknown operations, and valid/failing package routes.

## Residual Work And Promotion Rule

The other 18 obligation kinds remain host-authoritative or only partially routed.
This profile therefore cannot set `SD-OBLIGATION` to H2. The ledger row may be
promoted only after every residual kind has a closed primitive-fact contract, strict
production decoder, independent native/WASI evidence, no reachable host decision
fallback, and one reviewed profile identity. Aggregate planning and acceptance must
remain GenesisCode-authored throughout. This contract claims no effect-policy,
replay, signing, evidence-verification, bootstrap-fixpoint, release, or downstream
product authority.
