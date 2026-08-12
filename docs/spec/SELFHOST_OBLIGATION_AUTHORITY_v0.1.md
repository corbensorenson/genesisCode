# Self-host Obligation Authority v0.1

Status: normative partial H2 migration contract for `SD-OBLIGATION`. This version
does not promote the ledger row or close R4.2.d.

`core/cli::obligation-authority` is the sole production semantic producer for the
`core/obligation::unit-tests`, `core/obligation::budgets`,
`core/obligation::capabilities-declared`, `core/obligation::determinism`,
`core/obligation::lint`, `core/obligation::ai-style`,
`core/obligation::replayable-tests`, `core/obligation::concurrency-replay`,
`core/obligation::property-tests`,
`core/obligation::stage1-validation`,
`core/obligation::typecheck`, and `core/obligation::typecheck-strict` decisions.
The host executes test bodies,
enforces previously authorized effects, records canonical value hashes and
sealed-error status, measures steps and effect-log sizes, transports canonical
module forms and ordered effect-operation observations, and persists opaque effect
logs. It does not decide whether an expectation matches, a budget is exceeded, a
suite belongs to a module, an observed operation was declared, a pure declaration
is contradicted by inferred or runtime effects, a replay value matches, a task-log
entry satisfies scheduling metadata rules, or a package typechecks.

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

For `:typecheck` and `:typecheck-strict`, `:inputs` contains exactly ordered `:modules`, each with only its
base-relative `:path` and canonical `:forms`. GenesisCode validates the complete
closed inventory, derives each module's `::meta` from those forms, constructs the
closed `genesis/typecheck-request-v0.1`, and invokes the already H2
`core/cli::typecheck-package` authority. The host cannot supply or substitute
metadata. For `:typecheck-strict`, GenesisCode replaces a missing or non-map
`::meta` with an empty map and sets both `:strict-effects` and `:strict-shapes` to
`true` before invoking the checker. Rust independently reconstructs this closed
input only while decoding the returned report; it does not produce the obligation
decision or provide metadata to GenesisCode.

For `:lint`, `:inputs` contains exactly ordered `:modules` in the same closed
base-relative shape. GenesisCode invokes the Prelude
`core/editor/lint::lint-module` authority for every module, preserves diagnostic
order and bytes, and fails only diagnostics whose normalized level is `:error`.
When a canonical `::meta` has a symbol-vector `:exports` and either lacks a map
`:types` or omits an exported symbol, GenesisCode produces exactly one version-1
`lint/autofix-types` semantic patch. The patch preserves every unrelated metadata
field, inserts `?` only for missing exported-symbol types, and replaces the exact
metadata form by base-relative module path and form index. Warnings remain
non-failing in the lint obligation even when they have an autofix.

For `:ai-style`, `:inputs` contains exactly the same ordered `:modules` and
GenesisCode derives the lint report directly rather than accepting a host-authored
lint artifact. It normalizes diagnostic levels to `:error`, `:warn`, or `:info`;
all errors fail, and warnings fail only for the closed strict-code set
`missing-meta`, `malformed-meta`, `missing-exports`, `export-not-symbol`,
`missing-types-map`, `missing-type`, `missing-intent`, `intent-not-string`,
`missing-caps`, and `caps-not-vector` in the `editor/lint/` namespace. Ordered
diagnostic IDs bind module path, diagnostic index, and code. Canonical fix records
and patch intents may reference only the lint autofix produced for the same module.

For `:determinism`, `:inputs` contains exactly the same ordered `:modules` and
`:tests` observation shapes as `:capabilities-declared`. GenesisCode derives each
module's metadata, invokes the H2 package typechecker, and applies the two existing
v0.2 rules in order. First, a module with a valid `:caps` vector that filters to the
empty symbol set fails when its aligned typecheck report has unknown or nonempty
inferred operations. Second, each observed test with at least one unique effect
operation fails when its first defining module has that empty capability set.
Missing or malformed metadata/capability fields and tests whose suite has no
defining module preserve the legacy no-decision behavior rather than inventing a
new failure. Ordered static errors precede ordered runtime errors. Open module or
test observations are sealed protocol errors.

For `:replayable-tests` and `:concurrency-replay`, `:inputs` contains exactly
ordered `:tests` for tests that emitted effect logs. Each observation contains
exactly `:suite`, `:name`, `:log-artifact`, `:program`, `:actual-h`, `:replay-h`,
and `:entries`. The host re-evaluates the named test body under the declared
kernel limits, reports whether its raw runtime kind is an effect program, and, only
for an effect program, executes strict effect-log replay and reports the resulting
canonical value hash. A non-program observation has a nil replay hash; a program
observation has a 32-byte replay hash. Each ordered entry contains exactly its
zero-based `:position`, raw `:op`, and nullable `:task-id`, `:schedule-step`, and
`:await-edge`. The log artifact is persisted as raw provenance, but no host field
asserts replayability, concurrency eligibility, scheduling validity, or obligation
success.

GenesisCode makes the replay decisions. `:replayable-tests` requires every
observed test to produce an effect program and its replay hash to equal the original
value hash. `:concurrency-replay` selects observations containing at least one
`core/task::` or `editor/task::` operation, counts those tests, applies scheduling
rules only to task-like entries, and then applies the same program/hash rule.
Every task-like entry requires `:schedule-step` equal to its zero-based position;
`core/task::await` requires an await edge; and await, cancel, status, editor poll,
and editor cancel require a task identity. Error order is test order, then entry
order, then schedule, await-edge, task-id, and replay result. Tests without effect
logs are absent, preserving the existing obligation behavior. Replay execution or
log decoding failure remains an explicit host-boundary error rather than a
synthetic policy result.

For `:property-tests`, authority is two-phase and both requests are bound in full.
The `:plan` phase contains exactly `:configured`, `:default-cases`, `:phase`, and
ordered `:suites`. Suite and entry observations carry only manifest position, raw
shape, printable invalid values, callable presence, and the raw optional case
integer. GenesisCode validates those facts, preserves legacy error order, derives
every case count, constructs each seed with the normative
`GCv0.2\0property\0seed\0` BLAKE3 domain and little-endian case index, and emits
the exact ordered test plan with `:stop-rule :first-non-pass`. The host strictly
checks request binding and plan contradictions, then invokes only the referenced
callables with the authorized seeds. It records ordered raw value, apply-error, or
effect-program outcomes and stops only when the plan's declared rule requires it.
The `:finalize` phase contains the same immutable inputs plus exactly those raw
outcomes. GenesisCode rejects omitted, additional, reordered, seed-substituted, or
post-failure attempts and produces the canonical `genesis/property-tests-v0.2`
report and errors. Rust independently reconstructs the report only to reject a
contradiction before persistence; it does not supply the production verdict.

For `:stage1-validation`, `:inputs` contains exactly ordered `:modules`. Each
module observation contains exactly its base-relative `:path`, original and
transformed canonical module hashes, original and transformed pure-evaluation
outcomes, and four nonnegative optimizer counters. An evaluation outcome is
closed: success has a 32-byte value hash and nil error, while failure has nil
value hash and a raw string error. Rust performs the conservative transform,
canonicalization, and caller-bounded isolated Prelude evaluation as mechanisms;
it does not
transport a gate verdict or error list. GenesisCode validates every closed
observation, derives original/transformed evaluation failures and pure-value hash
mismatches in normative order, prefixes aggregate errors by module path, and emits
the exact `genesis/stage1-validation-v0.2` report. The host independently
reconstructs that report only to reject request substitution, malformed outcomes,
or contradictory output before persistence.

The result kind is `genesis/obligation-authority-result-v0.2`, has `:v` 2, and contains exactly
`:errors`, `:kind`, `:name`, `:ok`, `:operation`, `:report`, `:request-h`, and `:v`.
`:request-h` is the 64-character lowercase `genesis/hash-profile/gcv0.2-blake3`
identity of the complete closed request. GenesisCode computes it through
`selfhost/hash::hash-term`; the host independently applies the same normative hash
profile to the exact request it invoked and rejects a result bound to any other
request without recomputing the GenesisCode policy decision. The embedded
report preserves the existing `genesis/unit-tests-v0.2`, `genesis/budgets-v0.2`,
`genesis/caps-declared-v0.2`, `genesis/determinism-v0.2`, `genesis/lints-v0.2`,
`genesis/ai-style-v0.1`, or `genesis/typecheck-v0.2` artifact shape and ordering.
Replay reports preserve `genesis/replayable-tests-v0.2` and
`genesis/concurrency-replay-v0.1`; both contain ordered errors and the latter binds
the exact concurrent-test count. Property reports preserve
`genesis/property-tests-v0.2`; the intermediate closed plan is
`genesis/property-test-plan-v0.1` and is never acceptance evidence by itself.
Stage1 reports preserve `genesis/stage1-validation-v0.2`, including ordered
per-module optimizer observations and aggregate path-prefixed errors.
The host decoder rejects open, missing, reordered,
renamed, contradictory, or observation-substituting output before persistence.
Malformed or open requests, unknown operations, invalid facts, negative counters,
and resource exhaustion return a sealed protocol error and never synthesize a pass.

The `:lint` and `:ai-style` result `:report` is a closed transport map containing
exactly `:artifact-terms` and `:final`. Each side-artifact row contains exactly a
lowercase 64-character `:hash` and canonical `:term`; the hash is the EvidenceStore
BLAKE3 identity of the canonical `print-term` UTF-8 bytes. Lint transports only
autofix patches. AI-style transports those patches plus its complete derived lint
report, whose hash is the final report's `:lint-artifact`. Rust independently
recomputes every side hash, reconstructs the exact metadata-preserving lint patch
from the input module, derives every AI-style diagnostic/fix/failure, rejects any
extra, missing, duplicate, or contradictory artifact, and persists nothing until
the complete final report validates. It then requires EvidenceStore persistence to
return the same hash.

## Authority Boundary

Production package testing passes the exact selected self-host artifact and resource
limits into this authority. Rust retains only execution, measurement, artifact-store
transport, strict decoding, and contradiction rejection. The former Rust unit-test,
budget, suite-ownership, capability-membership, determinism-policy, ordinary
typecheck-obligation, and strict typecheck-obligation decision implementations are
absent from production source. The former Rust lint traversal, autofix producer,
strict-warning classifier, AI-style diagnostic producer, and artifact-loading
composition path are also absent. The former Rust task-operation classifier,
scheduling-policy checks, replay-hash comparison, concurrent-test counter, and
replay report producer are absent; one bounded host observation pass is shared by
both replay obligations. The former reachable Rust property inventory, seed-plan,
failure-decision, and report path is replaced by the two-phase authority; the host
retains callable invocation and an independently checked implementation of the
authorized first-non-pass stop mechanism. Neither an
environment variable nor a feature can silently restore them.

The former reachable Rust stage1 gate-report path is absent from production
obligation execution. Rust retains optimizer transformation, canonical hashing,
caller-bounded pure module evaluation, raw evaluation-error transport, optimizer
counters, and a
strict contradiction decoder; GenesisCode alone derives stage1 equivalence policy,
errors, pass/fail, and the persisted report.

`policies/selfhost_obligation_authority_v0.1.json` binds the exact ordered source
set, artifact,
entrypoint, migrated and residual obligation inventories, primitive host facts, and
nonclaims. `sourceSetSha256` is SHA-256 over the domain
`genesis/selfhost-obligation-authority-source-set-v0.1\0`, followed for each
declared module by its UTF-8 path length as an unsigned 64-bit big-endian integer,
path bytes, source byte length in the same encoding, and exact source bytes.
`scripts/lib/selfhost_obligation_authority.py` independently validates
that profile, the production call sites and dependency graphs, removal of the
previous host decision paths, and mutation controls. Focused Rust tests and native/WASI CLI
runtime observations cover matching, mismatch, sealed error, inclusive/exceeded
limits, declared/undeclared operations, missing suite ownership, open requests,
unknown operations, host metadata and strictness injection, static/runtime
determinism failures, lint errors and warnings, canonical autofix persistence,
strict AI-style warnings, side-artifact substitution, contradictory final reports,
valid/failing ordinary and strict package routes, replay hash disagreement, open
replay observations, missing task scheduling fields, contradictory concurrent
counts, exact property seeds, full passing case execution, first-case failure, and
seed-plan tampering. Stage1 controls cover pure equivalence, raw evaluation
failure, pure-value mismatch, open observation rejection, exact request binding,
and report tampering. Runtime
fixtures execute from isolated temporary copies so effectful tests cannot mutate
the normative source corpus.

## Residual Work And Promotion Rule

The other 8 obligation kinds remain host-authoritative or only partially routed.
This profile therefore cannot set `SD-OBLIGATION` to H2. The ledger row may be
promoted only after every residual kind has a closed primitive-fact contract, strict
production decoder, independent native/WASI evidence, no reachable host decision
fallback, and one reviewed profile identity. Aggregate planning and acceptance must
remain GenesisCode-authored throughout. This contract claims no effect-policy,
effect-replay execution, signing, evidence-verification, bootstrap-fixpoint,
release, or downstream product authority.
