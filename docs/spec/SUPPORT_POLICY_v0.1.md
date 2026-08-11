# GenesisCode Support Policy v0.1

## Status and authority

This document defines the support lifecycle for the GenesisCode v1 release family, stable source and API deprecations, format readers, security exceptions, and end-of-life. Its machine authority is `docs/spec/SUPPORT_POLICY_v0.1.json`, validated by `docs/spec/SUPPORT_POLICY_v0.1.schema.json`; the pure typed evaluator is `crates/gc_types/src/support_policy.rs`.

GenesisCode is currently release train `0.2.0`. The current phase is `preview`, no v1 stable release is active, and the reserved `genesis/compat/v1/*` identities remain candidates until exact reviewed R9.1.a promotion. Publishing this policy does not promote those candidates or convert current reader availability into a v1 promise.

## Deterministic lifecycle

Support state is an explicit reviewed release fact, never a consequence of ambient clock access, a parsed version label, package metadata, filename, model output, download count, or telemetry alone. A lifecycle record binds the release artifact identity, support-policy identity, phase, effective UTC date, predecessor record, evidence, and signer/reviewer authority. Tooling consumes that record as data and cannot advance it.

The ordered phases are:

1. `preview`: evaluation only; no stable compatibility or duration promise.
2. `standard`: supported source/API behavior, current writers, accepted readers, correctness fixes, and security fixes.
3. `maintenance`: compatibility and accepted readers remain; critical correctness, migration, and security fixes continue.
4. `security-only`: compatibility and accepted readers remain unless a bounded emergency quarantine is approved; only high/critical vulnerability and migration/recovery fixes are promised.
5. `end-of-life`: no new fixes are promised, but signed historical artifacts, verification metadata, migration instructions, and vulnerability notices remain available when legally and operationally possible.

Each stable v1 minor line receives at least 548 days of standard support, 365 additional days of maintenance, and 365 additional days of security-only support. Therefore ordinary EOL cannot occur before 1,278 elapsed days after GA. At least the newest supported v1 minor remains in standard or maintenance while v1 is the newest stable major. The final supported v1 line remains supported until at least 730 days after a stable successor-major release. A phase may be extended without changing semantics; it may not be shortened except by a validated security emergency, which quarantines an unsafe path rather than declaring it normally retired.

## Source and API deprecation

A stable source or API element can be removed only after all of the following are true:

- a reviewed release first marks it deprecated and names a stable replacement;
- migration and rollback instructions are published;
- at least 365 days and two subsequent stable minor releases have elapsed;
- positive, negative, migration, and compatibility tests cover the transition;
- release notes and generated agent/human references identify the removal;
- the removal introduces a new identity wherever observable semantics or bytes change.

Warnings are structured diagnostics, not prose-only notices. A deprecation cannot grant a capability, alter replay facts, reinterpret a stable identity, or hide an unsupported profile. The current active-deprecation set is explicitly empty.

## Format readers

The reader inventory is the exact intersection of `genesis.compatibility.json` and implemented accepted-reader identities. Current writers emit only the named current identity. A legacy identity is read-only, is tied to exactly one migration record, and cannot become a writer default. Unknown, missing except the named package pre-schema migration, ambiguous, or future identities fail before payload interpretation.

All listed readers are currently candidate/preview behavior. Stable v1 reader status begins only after R9.1.a promotion. Once stable, a format reader cannot be retired inside v1. Retirement requires:

- a published successor-major compatibility policy;
- at least 730 days of explicit deprecation;
- a bounded offline or authenticated migrator as appropriate to the format;
- retained current, legacy, malformed, future-version, round-trip, and downgrade goldens;
- corpus or privacy-preserving telemetry evidence sufficient to assess remaining use;
- rollback/recovery instructions and release-note disclosure;
- independent review that the replacement does not reinterpret an existing stable identity.

Passing retirement eligibility never removes a reader. The owning implementation, compatibility registry, release authority, and independent evidence must change in a separate reviewed transaction.

## Security exceptions

Security exceptions are temporary review candidates, not runtime grants. The active set is explicitly empty. A proposed exception must bind a portable exception ID, advisory, exact scope, rationale, rollback, 1 through 32 test-evidence references in strict lexical unique order, and a duration of 1 through 90 days. If it changes wire bytes or observable semantics, it must also name a replacement identity and migrator. Renewal is a new reviewed record, not an in-place expiry extension.

No exception may weaken or bypass kernel purity, seal unforgeability, explicit error handling, deny-by-default capabilities, resource bounds, hard cancellation where promised, deterministic logging, strict replay, canonical identity, evidence independence, signing authority, or release gates. An exploitable legacy reader may be quarantined immediately only under this process; quarantine does not fabricate EOL evidence, erase migration duties, or authorize reinterpretation of its identity.

## Decision API and bounds

`gc_types::support_policy` provides pure queries for the current support snapshot and exact reader triples plus evidence validators for removals and security exceptions. It performs no filesystem, network, process, environment, clock, randomness, model, or capability operation. Unknown components and readers fail closed with bounded errors.

Each supplied field is limited to 4,096 UTF-8 bytes and each security proposal to 32 test-evidence references. Decision identities bind the profile ID and complete normalized input/result payload using the canonical GenesisCode hash profile. A successful eligibility decision sets `grants_*_authority = false`; it is suitable for review but cannot mutate support state.

## Change procedure

1. Change this specification, closed schema, machine profile, typed evaluator, and tests in one transaction.
2. Update `genesis.compatibility.json` or `genesis.version-surfaces.json` only through their owning compatibility task and preserve every accepted predecessor or satisfy the retirement contract.
3. Add controls for unknown/future identities, downgrade attempts, incomplete windows, missing migration/evidence, overlong inputs, exception expiry, and protected-invariant bypass.
4. Regenerate derived release/reference authorities only through the canonical generated-authority updater and inspect the complete diff.
5. Activate stable v1 support only at R9.1.a with independent freeze evidence; activate EOL or an exception only through a separately reviewed authority record.

## Nonclaims

- This policy does not claim that v1 exists, is stable, or is currently supported.
- Reader availability in `0.2.0` is preview behavior, not a permanent v1 guarantee before promotion.
- Eligibility validation does not approve removal, EOL, exception, release, or authority changes.
- Compatibility is never inferred from semantic-version ordering or labels.
- No self-host authority, package, backend, target, benchmark, Foundry result, model, assurance level, or release level is promoted.
