# Self-host Type and Effect Authority Profile v0.1

Status: normative for the R4.2.b production-authority transition.

## Authority boundary

`core/cli::typecheck-package` is the sole production producer of package type,
effect, contract, profile-negotiation, module-resolution, and diagnostic facts.
The accepted producer is loaded from the content-addressed self-host toolchain
artifact and invoked under the request limits. Native and WASI hosts may load,
invoke, bound, decode, and reject the producer. They may not infer, repair,
default, reorder, or replace its semantic facts.

The host sends one closed `genesis/typecheck-request-v0.1` map containing the
exact ordered package closure. Each request module contains exactly `:path`,
`:forms`, and `:meta`. The producer returns one closed
`genesis/typecheck-v0.2` report. The decoder binds the response to the request,
including module count and order, unique paths, declared export inventory and
types, active profile state, and aggregate/module/export coherence. Missing or
malformed facts fail closed; an empty effect set, gradual type, successful
profile negotiation, or `unknown=false` is never synthesized.

## Frozen semantic pipeline

The observable pipeline is equivalent to this fixed order:

1. collect and validate package metadata, exports, definitions, signatures,
   contracts, capabilities, and strictness settings;
2. resolve the module profile and negotiate the closed package profiles;
3. compose and validate contracts, blame, refinements, and implementations;
4. resolve modules, imports, private boundaries, references, and identities;
5. check every module, export type, typed effect, syntactic effect, unknown
   signature, row, constructor, application, primitive, and contract form;
6. emit canonical module and aggregate reports and deterministic diagnostics.

The machine profile enumerates the owned decisions and every GenesisCode source
module that participates in this authority. Changing order, identity inputs,
the profile offer, report shape, diagnostic identity, or resource accounting is
a semantic-profile change, not an implementation detail.

## Consumers

The authoritative report supplies:

- the `typecheck` command and strict package checks;
- package ABI export types, effects, unknown-effect state, and capability
  summaries;
- determinism obligations and package acceptance diagnostics;
- incremental dependency facts and invalidation inputs.

Consumers must use the same decoded report. A consumer may not rerun Rust
effect inference or fill an absent report field. Cache entries bind all source,
metadata, dependency, capability, contract, profile, checker-artifact, and
semantic-profile identities; a stale, partial, cyclic, or cross-profile entry
is rejected and clean recomputation remains byte-equivalent.

## Rust oracle isolation

`gc_types` is a compatibility oracle, not production authority. It is absent
from the normal dependency graphs of `gc_obligations` and `gc_cli_driver`.
Only the compile-time `parity-oracle` feature reached through the dedicated
`gc_cli_driver_parity` package may link and invoke it. Production binaries have
no environment switch, library setter, CLI route, obligation route, ABI route,
or release-graph edge that can restore the Rust checker. Differential tests may
invoke the oracle directly for the bounded window recorded in the machine
profile; they cannot promote either implementation.

## Independent verification

`scripts/lib/selfhost_typecheck_authority.py` imports no GenesisCode
implementation crate and does not execute the Rust oracle. It validates the
closed machine profile, source-manifest closure, dependency isolation, decoder
and consumer custody, and mutation controls. In runtime mode it compares exact
canonical reports and structured diagnostics from the native and WASI
production binaries on positive and negative packages.

The independent verifier does not prove type soundness. Soundness proof remains
R7.2.g; independent full reimplementation remains R4.5; reproducible bootstrap
fixpoint remains R4.4.

## Resource and artifact contract

Toolchain loading is separately accounted bootstrap work. Request execution is
charged to the declared step and memory limits. The authority guard records
the artifact identity and size, complete toolchain and checker component
closures, explicit step and cumulative-allocation ceilings, and per-process
elapsed time and peak resident set size. A step ceiling of one and an allocation
ceiling of one are mandatory negative controls; both must fail closed rather
than silently becoming unlimited.

The runtime evidence is one bounded E0 observation, not an SLO sample or release
qualification. `cold` means the first fresh process after the harness build;
`warm` means an immediate fresh-process repeat with the operating-system page
cache retained. It does not claim a purged filesystem cache. The unrelated
`--help` observation points the artifact variable at a nonexistent path and
must still succeed, proving that command does not load the typechecker or the
combined artifact. This monolithic v0.1 distribution envelope therefore does
not satisfy component-snapshot closure; R2.3 owns the separately addressable
component implementation and PB-6 optimization. The guard records that
assignment with `performanceClaim=none`; observed latency cannot authorize a
PB-6 pass, reset its baseline, or block the H2 semantic-authority transition.

The focused shell harness separately invokes the compile-time parity binary
over every package selected by `tests/spec/pkg_*/package.toml`, from one copied,
lexically ordered 26-fixture corpus. It requires exact exit and output equality
between the Rust oracle and production self-host authority and publishes one
content identity over all copied fixture bytes. The independent Python verifier
remains oracle-free. Package, module/meta, dependency, capability-policy,
request-limit, checker-profile, checker-schema, diagnostic-catalog, and checker
artifact identities are cache-key inputs with mutation controls. Agent task
cards are not checker inputs; R2.3.c/d owns future component caches and must add
any new consumed card or schema identity before reuse is permitted.

The source closure rejects missing, stale, escaping, duplicate, unknown, or
over-budget manifest sources. Resource and performance observations never
weaken hard limits or establish H3/H4 authority.

## Nonclaims

- This profile does not establish H3/H4, type soundness, optimizer authority,
  package-manager authority, effect-policy authority, or code generation.
- Differential equality with Rust is migration evidence, not proof that Rust is
  correct and not authority for Rust to remain reachable in production.
- The local guard does not issue release evidence or activate GenesisBench,
  Genesis Foundry, GenesisChallenge, or Genesis Model.
