# Self-Host Closure Levels v0.1

Status: normative.

## Purpose

This specification defines the only valid meanings of `H0` through `H4` for
GenesisCode self-host claims. Levels apply to one exact semantic decision under one
declared release profile. They do not describe a repository, command, crate, file,
or implementation language in the abstract.

The closed machine authority is
`docs/spec/SELFHOST_CLOSURE_LEVELS_v0.1.json`; its schema is
`docs/spec/SELFHOST_CLOSURE_LEVELS_v0.1.schema.json`. The machine authority binds
this exact prose, the stage0 trust contract, the cumulative predicates below, and
the evidence and anti-shortcut inventories.

## Assessment Unit

Every assessed row has an immutable decision key containing:

- semantic decision ID and normative specification/profile version;
- command or API surface and target/runtime profile;
- canonical input and output contract identities;
- producing implementation and accepted artifact identities;
- production authority and host-binding identities;
- verifier identity, fallback policy, and evidence-closure identity.

Changing any key field creates a new assessment. Evidence for one target, profile,
artifact, command route, or semantic decision cannot promote another.

## Applicability

`H0` through `H4` are ordered levels only for an `applicable` semantic decision.
`N/A` is a disposition, not a level and not evidence of completion.

- `applicable`: the decision is intended to migrate to GenesisCode authority.
- `residual-stage0`: the exact decision is an irreducible S0-K reference-semantic
  responsibility or unavoidable S0-H physical host adapter named by the stage0
  contract. The ledger must name the domain, retained trust, verifier, and reason.
- `not-semantic-decision`: the row is presentation, transport, documentation, or
  another surface that makes no semantic decision. The ledger must name the owning
  semantic decision or explain why none exists.

An `N/A` row cannot satisfy a prerequisite, raise an aggregate score, or conceal an
applicable decision. A wrapper around host semantics remains applicable and cannot
be relabeled `N/A` merely because it is written in GenesisCode.

## Cumulative Level Lattice

Levels are monotonic predicates: Hn requires every predicate and evidence class for
H0 through Hn. There is no averaging, partial credit, level skipping, or promotion
from a generated status view. Unknown, missing, expired, mismatched, or disputed
evidence fails closed to the greatest lower fully proven level.

### H0: Routed

H0 proves only that the declared production route invokes an authenticated
GenesisCode source or artifact for the assessed decision.

Required predicates:

1. Route selection is explicit, versioned, deterministic, and bound to the decision
   key.
2. The invoked `.gc` source/artifact identity and execution profile are recorded.
3. Every alternate route and fallback is enumerated and observable.
4. Positive routing tests and bypass/stale-artifact negative controls pass.

At H0, GenesisCode may be only a wrapper. A host implementation may still compute
the result. H0 does not claim GenesisCode implementation, semantic ownership,
production authority, no fallback, or bootstrap closure.

### H1: GenesisCode Implementation

H1 proves that reviewed GenesisCode code computes the semantic decision, but does
not yet require that implementation to control the production result.

Required predicates, in addition to H0:

1. Versioned `.gc` source contains the decision algorithm rather than delegating it
   through an opaque semantic host operation.
2. Inputs, outputs, diagnostics, resource accounting, and effect requests have
   canonical contracts and bounded behavior.
3. Host calls are limited to declared non-semantic S0-H transport or physical
   adapters; callback results cannot silently choose the semantic outcome.
4. Differential, golden, malformed-input, adversarial, and resource-bound tests cover
   the normative corpus against a separately identified reference oracle.
5. Source-to-artifact provenance and artifact admission are content-addressed.

At H1, a Rust or other host implementation may remain production authority. Parity
with that implementation does not itself establish H2.

### H2: GenesisCode Production Authority

H2 proves that the accepted GenesisCode implementation makes the result consumed by
every default, release, and policy-relevant production path in the declared profile.

Required predicates, in addition to H0 and H1:

1. The authenticated GenesisCode artifact is the sole reachable semantic producer
   on all declared production routes.
2. No host-semantic, embedded, source, compatibility, environment-variable, error,
   timeout, recovery, or debug fallback can produce an accepted result.
3. Host bindings can transport, contain, normalize, or reject, but cannot alter or
   replace the GenesisCode decision.
4. Failure of artifact admission or execution fails closed. Rollback selects only an
   explicitly approved prior GenesisCode artifact with its own evidence closure.
5. An independently controlled verifier checks route reachability, artifact identity,
   fallback absence, and result custody; mutation controls restore each forbidden
   host path and must be rejected.
6. Production, release, native, WASI, warm, and other in-profile entrypoints agree on
   authority and observable semantics.

H2 is not established by routing, wrappers, feature flags, differential agreement,
an unreachable-by-convention fallback, or a status document.

### H3: Reproducible Bootstrap Fixpoint

H3 proves that an H2 authority is reproducibly generated from reviewed source and
reaches a stable bootstrap fixpoint without trusting an unexplained compiler binary.

Required predicates, in addition to H0 through H2:

1. A hermetic, pinned recipe records source, dependencies, generators, compiler,
   linker, target, features, environment, and canonicalization profile.
2. On each qualified host, stage0 builds stage1, stage1 builds stage2, and stage2
   builds stage3; canonical stage2 and stage3 identities are equal.
3. At least two clean runs per host reproduce the same identities, and at least two
   independently administered qualified host classes produce the same canonical
   fixpoint identity.
4. Raw artifact identities and all excluded non-semantic envelope bytes are retained;
   normalization is specification-owned and cannot discard unexplained differences.
5. Diverse double compilation or an equivalent independently checked source-to-binary
   witness binds the accepted fixpoint to reviewed source and detects trusting-trust
   substitution.
6. Perturbed source, toolchain, dependency, stage, host-profile, and normalization
   controls either change the bound identity or fail closed as specified.

A one-stage build, one host, one run, stage1/stage2 agreement alone, or equality after
unreviewed normalization is not H3.

### H4: Independently Reimplemented and Conformant

H4 proves that the H3 decision or its complete acceptance predicate survives a
genuinely independent implementation path.

Required predicates, in addition to H0 through H3:

1. The independent implementation or complete proof-checking verifier is authored,
   reviewed, built, and operated under separate custody from the production producer.
2. It shares no producer source, generated implementation, semantic library, parser,
   evaluator, code generator, test oracle, or mutable evidence state. Shared material
   is limited to normative specifications, public vectors, standard cryptographic
   primitives, and explicitly declared platform facts.
3. A machine-readable independence manifest identifies authorship, dependencies,
   generators, build systems, custody, conflicts, and allowed shared roots.
4. Complete normative, adversarial, metamorphic, resource, malformed-input, and
   producer-mutation suites agree; every disagreement blocks promotion and release.
5. Hidden or held-back controls demonstrate that the independent path is not merely
   replaying public expected outputs or trusting producer verdicts.
6. The independent path verifies exact decision, artifact, profile, and evidence
   identities and cannot promote itself or rewrite its evaluator.

Independent review of the same code, a second wrapper, a generated port, a verifier
that trusts producer claims, or shared semantic dependencies is not H4. H4 is strong
implementation diversity and conformance evidence, not a claim of mathematical proof
or freedom from all hardware, operating-system, cryptographic, or human trust.

## Evidence Classes

Every promotion binds immutable evidence for all required classes at that level:

- `route-custody`: route inventory, artifact identity, profile, and bypass controls;
- `implementation-conformance`: source provenance, corpus results, diagnostics,
  bounds, malformed inputs, and differential oracles;
- `production-authority`: whole-program reachability, no-fallback mutations,
  entrypoint parity, failure behavior, and rollback custody;
- `bootstrap-fixpoint`: clean stage graph, repeated and cross-host identities,
  retained raw bytes, normalization rules, and DDC/source-binding witnesses;
- `independent-conformance`: independence manifest, separate implementation/verifier,
  complete conformance outcomes, hidden controls, and disagreement handling.

Evidence must name its producer, subject, inputs, outputs, environment, profile,
timestamps, expiration or supersession rule, verifier, and cryptographic identity.
Mutable local reports are observations only. A producer, optimizer, model, benchmark
solver, or generated view cannot verify or promote its own result.

## Aggregation and Promotion

The semantic-ownership ledger is the authority for individual rows. An aggregate
claim is the minimum proven level across every applicable decision in its closed
inventory; omitted, disputed, or unknown rows fail the aggregate closed.

- `GenesisCode H2 authority` requires every applicable release-profile decision to be
  H2 or higher and every `N/A` disposition to pass stage0-boundary review.
- `GenesisCode H3 bootstrap closure` additionally requires every member of the closed
  bootstrap dependency graph to satisfy H3 under the same fixpoint identity.
- `GenesisCode H4 independent conformance` additionally requires every critical
  semantic and artifact-acceptance root named by the release profile to satisfy H4.

Only an independently reviewed ledger update may promote a level. Evidence expiry,
profile drift, route drift, dependency drift, newly reachable fallback, verifier
conflict, or conformance disagreement immediately invalidates the affected promotion
and every aggregate depending on it.

## Nonclaims

- This specification assigns no current H-level and changes no production authority.
- It does not broaden stage0, declare bootstrap closure, or close R4.1.c-e.
- It does not authorize GenesisBench, Genesis Foundry, GenesisChallenge, or Genesis
  Model work.
- Passing the closure-level validator proves contract integrity, not that any semantic
  decision satisfies the contract.
