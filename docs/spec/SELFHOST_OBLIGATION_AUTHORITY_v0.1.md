# Self-host Obligation Authority v0.1

Status: normative partial H2 migration contract for `SD-OBLIGATION`. This version
does not promote the ledger row or close R4.2.d.

`core/cli::obligation-authority` is the sole production semantic producer for the
`core/obligation::unit-tests`, `core/obligation::budgets`, and
`core/obligation::capabilities-declared` decisions. The host executes test bodies,
enforces previously authorized effects, records canonical value hashes and
sealed-error status, measures steps and effect-log sizes, transports canonical
module forms and ordered effect-operation observations, and persists opaque effect
logs. It does not decide whether an expectation matches, a budget is exceeded, a
suite belongs to a module, or an observed operation was declared.

## Closed Protocol

The request kind is `genesis/obligation-authority-request-v0.2`, has `:v` 2, and
contains exactly `:kind`, `:v`, `:operation`, `:package`, and `:inputs`. Each
operation has a distinct closed input map so later migrations cannot overload a
field whose name encodes another operation's semantics.

For `:unit-tests`, `:inputs` contains exactly `:tests`; every ordered observation contains exactly
`:suite`, `:name`, `:actual-h`, `:expected-h`, `:sealed-error`, and `:log-artifact`.
Hashes are 32 bytes; expected hash and log artifact are explicitly nullable. A test
passes only when it is not a sealed error and either has no expectation or its exact
canonical value hash equals the expected hash.

For `:budgets`, `:inputs` contains exactly `:limits` and `:tests`. `:limits`
contains exactly the three nullable nonnegative integer
fields `:max-steps-per-test`, `:max-effect-entries-per-test`, and
`:max-effect-log-bytes-per-test`. Every observation contains exactly `:suite`,
`:name`, `:steps`, `:effect-entries`, and `:effect-log-bytes`. A configured limit is
inclusive; only an observed value strictly greater than it fails.

For `:capabilities-declared`, `:inputs` contains exactly ordered `:modules` and
`:tests`. Each module contains exactly its base-relative manifest `:path` and
canonical `:forms`. Each test with an effect log contains exactly `:suite`, `:name`,
and the canonically ordered unique `:used-ops` observed in that log. GenesisCode
validates the complete module inventory even when no test has an effect log, then
resolves the first ordered module defining the suite, extracts its canonical
`::meta`, requires symbol-vector `:caps`, and emits one canonical error for every
observed operation absent from that declaration. Missing suite ownership is a
failed obligation; malformed module/meta/capability facts are sealed protocol
errors. Tests without effect logs produce no operation observations, preserving the
v0.2 obligation semantics.

The result kind is `genesis/obligation-authority-result-v0.2`, has `:v` 2, and contains exactly
`:errors`, `:kind`, `:name`, `:ok`, `:operation`, `:report`, `:request-h`, and `:v`.
`:request-h` is the 64-character lowercase `genesis/hash-profile/gcv0.2-blake3`
identity of the complete closed request. GenesisCode computes it through
`selfhost/hash::hash-term`; the host independently applies the same normative hash
profile to the exact request it invoked and rejects a result bound to any other
request without recomputing the GenesisCode policy decision. The embedded
report preserves the existing `genesis/unit-tests-v0.2`, `genesis/budgets-v0.2`, or
`genesis/caps-declared-v0.2`
artifact shape and ordering. The host decoder rejects open, missing, reordered,
renamed, contradictory, or observation-substituting output before persistence.
Malformed or open requests, unknown operations, invalid facts, negative counters,
and resource exhaustion return a sealed protocol error and never synthesize a pass.

## Authority Boundary

Production package testing passes the exact selected self-host artifact and resource
limits into this authority. Rust retains only execution, measurement, artifact-store
transport, strict decoding, and contradiction rejection. The former Rust unit-test,
budget, suite-ownership, and capability-membership decision implementations are
absent from production source. Neither an
environment variable nor a feature can silently restore them.

`policies/selfhost_obligation_authority_v0.1.json` binds the exact source, artifact,
entrypoint, migrated and residual obligation inventories, primitive host facts, and
nonclaims. `scripts/lib/selfhost_obligation_authority.py` independently validates
that profile, the production call sites and dependency graphs, removal of the three
host decision paths, and mutation controls. Focused Rust tests and native/WASI CLI
runtime observations cover matching, mismatch, sealed error, inclusive/exceeded
limits, declared/undeclared operations, missing suite ownership, open requests,
unknown operations, and valid/failing package routes.

## Residual Work And Promotion Rule

The other 17 obligation kinds remain host-authoritative or only partially routed.
This profile therefore cannot set `SD-OBLIGATION` to H2. The ledger row may be
promoted only after every residual kind has a closed primitive-fact contract, strict
production decoder, independent native/WASI evidence, no reachable host decision
fallback, and one reviewed profile identity. Aggregate planning and acceptance must
remain GenesisCode-authored throughout. This contract claims no effect-policy,
replay, signing, evidence-verification, bootstrap-fixpoint, release, or downstream
product authority.
