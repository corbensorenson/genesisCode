> Bundle Entry: `docs/spec/HOST_RUNTIME_BUNDLE_v0.1.md`

# Self-Host Boundary (WASM-First) v0.2

Goal: reach a state where the GenesisCode toolchain can run **on top of WASM** with a minimal Rust TCB,
then progressively replace Rust components with GenesisCode implementations until the toolchain is
self-hosted.

Non-goals (v0.2):
- Refinement proofs (extension points only).
- Replacing the kernel evaluator (Gλ) in the short term. The kernel remains the trusted “execution engine”.

## Self-Host v1 Exit Path (No Rust Semantic Fallback)

To close the transition from v0.2 bootstrap to a self-hosted v1 release posture, the project uses
the following measurable cutover gates.

### Stage A - Semantic Surface Lockdown

- Production binaries (`genesis`, `genesis_wasi`) run selfhost frontend/tooling paths only.
- Rust-engine parity paths are restricted to explicit parity harness binaries and not used by default workflows.
- Gate:
  - `scripts/check_rust_engine_compat.sh` passes in CI and zero-open health mode.

### Stage B - Reproducible Selfhost Artifact Authority

- `selfhost/toolchain.gc` remains the canonical artifact generated from modular selfhost sources.
- Release/runtime profiles enforce `artifact-only` bootstrap mode.
- Gate:
  - `scripts/check_selfhost_artifact_fresh.sh` passes.
  - strict selfhost suites pass:
    - `scripts/selfhost_strict_smoke.sh`
    - `scripts/selfhost_strict_golden.sh` (full profile).

### Stage C - Bootstrap Archive Retirement

When Stage A+B hold continuously, bootstrap-only Rust compatibility surfaces move to the legacy bootstrap archive
and are no longer referenced by production code paths.

Measurable retirement criteria:

1. `scripts/check_bootstrap_retirement_gate.sh` passes with strict release checks enabled in CI
   (`GENESIS_BOOTSTRAP_RETIREMENT_STRICT_RELEASE=1`).
   Local constrained-disk runs may return an explicit `degraded` status via
   `GENESIS_BOOTSTRAP_RETIREMENT_LOCAL_DEGRADED_MODE=1`, but that status is non-release and does
   not satisfy retirement sign-off criteria.
2. `docs/spec/BOOTSTRAP_OLD.md` keeps retirement checklist fully checked and explicitly approved.
3. The archive-specific retirement checker reports zero production references to archived bootstrap semantics.
4. `scripts/check_selfhost_boundary.sh --strict` passes after retirement move.

Only after these criteria are satisfied should bootstrap-era Rust semantic helpers be considered
fully retired from active production usage.

## Stage0 Trust Contract v0.1

### Claim

Stage0 is the smallest independently versioned host implementation needed to ingest a
reviewed GenesisCode artifact, establish the unforgeable protocol environment, execute
the pure language, and mediate explicitly authorized effects. Stage0 is not synonymous
with every Rust crate, the CLI, a routed `.gc` wrapper, the package manager, or the full
release toolchain.

The stage0 implementation has multiple trust domains. Only S0-K is TCB-A. This contract
assigns responsibilities and identifies current implementation anchors; it does not yet
claim exhaustive file membership. Exact source ownership and allowed dependency edges
are established later by R4.1.e. Neither language, crate, binary, repository location,
nor a historical label determines trust by itself.

The closed machine authority is `docs/spec/STAGE0_TRUST_CONTRACT_v0.1.json`, validated
against `docs/spec/STAGE0_TRUST_CONTRACT_v0.1.schema.json` by the existing self-host
boundary gate. Its content identity excludes only the identity field itself.

### Threat and Failure Model

Stage0 treats source, compiled artifacts, manifests, caches, effect payloads, provider
responses, filesystem state, environment state, and all user-authored code as untrusted.
It also assumes that any optimized producer, self-hosted tool, model, benchmark solver,
or Foundry candidate may be incorrect or adversarial. Those producers may propose bytes
but may not verify or promote their own output.

The current residual assumptions are a non-malicious but fallible operating system,
hardware, pinned host compiler/linker, cryptographic provider, and independent release
verifier/signing root. Compromise below those assumptions is outside a single stage0
process's proof boundary and must be addressed by reproducible cross-host bootstrap,
diverse double compilation, independent verification, and signed provenance rather than
by an in-process self-check.

Stage0 binds `docs/spec/NUMERIC_PROFILE_v0.1.json`. Self-hosted parsing, typechecking, optimization,
and code generation may reproduce or propose numeric artifacts but cannot redefine integer width,
decimal bounds, serialization, hashing, or backend acceptance. A self-host implementation that
narrows arbitrary-precision integers, accepts scale above 4096, or emits an unvalidated Stage 2
artifact fails profile equivalence and cannot advance bootstrap authority.

Every boundary is fail closed. Unknown versions, malformed structure, exhausted limits,
identity mismatch, verifier disagreement, unsupported host behavior, cleanup failure,
and missing evidence return a typed rejection. None may select a broader fallback,
continue with partially verified state, or silently reduce a claim.

### Exact Stage0 Identity

A stage0 identity binds, at minimum:
- the exact source tree and build recipe;
- compiler, linker, target, architecture, and enabled feature identities;
- every normal/build dependency and generated source input;
- S0-K/S0-R/S0-P/S0-X/S0-A/S0-H domain versions and source memberships;
- CoreForm, language, value/effect hash, compiled-artifact, Prelude, host-ABI, capability,
  resource-accounting, and error profiles;
- the accepted self-host artifact, manifest, and bootstrap mode;
- independent verifier and trust-root identities when making a release claim.

Two binaries with the same user-facing version but a different bound field are distinct
stage0 implementations. Host paths, timestamps, mutable cache state, and secrets are not
identity inputs; a build that cannot normalize or explicitly profile a host-dependent
field is not reproducible-bootstrap evidence.

### Trust Domains

#### S0-K: Pure semantic kernel (TCB-A)

Authority:
- immutable runtime values and persistent collection semantics;
- the reference G-lambda evaluator and total pure primitive allowlist;
- lexical scope, closures, deterministic resource accounting, and sealed failures;
- seal creation, sealing, unsealing, and token identity.

Must not contain:
- source parsing, canonical printing, package/artifact policy, bytecode codecs,
  optimized execution, effect interpretation, filesystem, time, randomness, network,
  process, environment, UI, model access, or release authority.

Why trusted now:
- some executable must define the irreducible reference semantics and unforgeable token
  mechanism used to check all higher layers.

Demotion path:
- replaceability is established by the independent kernel conformance work in R4.5;
  changing implementation language alone does not reduce this semantic trust.

#### S0-R: CoreForm representation and identity

Authority:
- source decoding, canonicalization, canonical printing, term ordering, and canonical
  content hashes for the exact declared profile.

Must not contain:
- evaluation, seal issuance, capability decisions, effects, artifact promotion, or
  fallback selection.

Failure impact:
- can change the program presented to S0-K or the identity bound to evidence, but cannot
  directly perform an effect.

Why trusted now:
- stage0 must parse and identify the initial reviewed artifact before a self-hosted
  frontend can become H2 authority.

Demotion path:
- R4.2.a moves production decisions to GenesisCode; R7.2.f and R4.5 validate the codec
  independently. The Rust reference may remain unreachable test material.

#### S0-P: Protocol and bootstrap assembly

Authority:
- reserve UNHANDLED, EFFECT, and ERROR tokens before user evaluation;
- bind the minimal trusted protocol constructors/predicates and load the reviewed
  Prelude/self-host artifact into a fresh environment;
- fail closed on malformed, mismatched, stale, or unsupported bootstrap inputs.

Must not contain:
- ambient effects, policy grants, hidden semantic fallback, package resolution,
  optimizer acceptance, or release promotion.

Failure impact:
- can misbind initial names or expose protocol authority, so its exact assembly surface
  remains trusted even though its execution is pure.

Why trusted now:
- protocol seals must be established outside untrusted user code and an initial loader
  is unavoidable before the GenesisCode toolchain can run.

Demotion path:
- shrink to seal reservation plus a content-addressed loader after self-hosted Prelude
  and frontend authority are H2 and bootstrap fixpoint evidence is H3.

#### S0-X: Compiled artifact decoder and optimized executor

Authority:
- decode only the exact versioned compiled artifact format;
- validate structural/resource invariants before allocation or execution;
- execute compiled forms under the same observable semantics and accounting as S0-K.

Must not contain:
- capability access, policy authority, acceptance of unknown format versions, semantic
  shortcuts keyed to source/workload identity, or self-issued equivalence claims.

Failure impact:
- malformed acceptance can violate memory/resource safety; execution drift can change
  pure results. This domain is therefore separate from TCB-A and may not redefine S0-K.

Why trusted now:
- the current production tier executes this representation; differential checks reduce
  risk but are not proof and do not make the implementation part of the reference core.

Demotion path:
- R3 translation validation, R7 proof obligations, and independent conformance make
  compiled output proposal-only. Unsupported or unverified output is rejected or routed
  explicitly to a profile-permitted lower tier before evaluation, never silently.

#### S0-A: Bootstrap artifact identity and admission

Authority:
- bind exact source, manifest, profile, dependency, compiled-cache, and artifact hashes;
- reject missing, stale, malformed, noncanonical, over-budget, or wrong-profile inputs;
- select only an explicitly configured bootstrap mode.

Must not contain:
- language semantics, silent source/embedded/Rust fallback, package publication,
  obligation waiver, or equivalence self-approval.

Failure impact:
- can substitute the program/toolchain that higher layers execute, so its acceptance
  decision is a separate trust domain even where it reuses S0-R or S0-X codecs.

Why trusted now:
- a stage0 loader must authenticate the first executable self-host artifact before that
  artifact can participate in later bootstrap stages.

Demotion path:
- H3 witnesses and diverse double compilation bind the accepted bytes; H4 adds an
  independently implemented verifier.

#### S0-H: Effect-host ABI and containment

Authority:
- deny-by-default capability decisions and bounded host operation dispatch;
- normalize host inputs/errors, enforce resource and payload limits, and keep secrets
  outside deterministic artifacts;
- hard cancellation where promised, explicit resource closure, deterministic logging,
  and strict replay;
- platform transport and embedding only for declared ABI operations.

Must not contain:
- pure language semantics, seal minting, source canonicalization, self-host semantic
  fallback, optimizer verification, package policy waiver, or release promotion.

Failure impact:
- controls real-world authority and containment, but may not alter pure S0-K results.

Why trusted now:
- physical hosts necessarily implement filesystem, network, process, clock, GPU/UI,
  device, plugin, and model effects. Self-hosting orchestration does not eliminate this
  platform trust.

Demotion path:
- generated ABIs, sandboxed components, capability conformance, replay, and independent
  host implementations reduce implementation trust while the physical host boundary
  remains explicit.

### Layer Order

1. S0-R decodes and canonically identifies source/bootstrap data.
2. S0-A authenticates exact inputs and profile before loading.
3. S0-P creates protocol seals and the initial environment.
4. S0-K defines reference pure semantics.
5. S0-X may execute only accepted compiled artifacts under S0-K equivalence obligations.
6. S0-H receives only explicit EFFECT requests and returns normalized sealed results.

No later layer may grant authority to an earlier layer, verify itself, mint protocol
tokens, or convert its own observation into H-level or release completion.

### Current Host Semantics That Remain Trusted

- memory allocation and process isolation provided by the operating system/runtime;
- exact Rust numeric, UTF-8/byte, BLAKE3, and persistent-value behavior where named by
  S0-K/S0-R profiles;
- safe bounded decoding and structural validation in S0-X/S0-A;
- platform syscalls, TLS/crypto providers, drivers, and device APIs reached only through
  S0-H;
- compiler/linker/toolchain correctness for the stage0 binary until H3/DDC witnesses;
- the independent verifier and signing roots used for release evidence.

These are explicit residual assumptions, not GenesisCode semantic-ownership claims.
Routing a command through `.gc`, matching a Rust reference, or producing a local report
does not remove them.

### Claims and Nonclaims

- This contract does not claim H1-H4 for any semantic decision.
- TCB-A means S0-K only; it is not a crate list or all code linked into a binary.
- Stage0 includes the other named trust domains, but they remain separately reviewable,
  replaceable, and forbidden from acquiring S0-K authority.
- H0/H1/H2/H3/H4 apply per semantic decision, never to the repository as a whole.
- S0-K and unavoidable S0-H host semantics may be marked not-applicable for self-host
  migration only when this contract names the residual trust; they cannot be mislabeled
  H2 merely because a GenesisCode wrapper invokes them.
- Bytecode speed, artifact hash agreement, differential parity, and routing are evidence
  inputs, not authority or independent verification.

### Follow-on Enforcement

- R4.1.b freezes exact H-level predicates against this domain model.
- R4.1.c gives every command and semantic decision one producing implementation,
  production authority, verifier, fallback status, host binding, and H-level.
- R4.1.d regenerates status views from that ledger.
- R4.1.e enforces exact source membership and allowed dependency edges for every domain.

## Rust Host-Only ABI (Strict)

The Rust host boundary is intentionally narrow and versioned. Existing Rust stage0 semantics are
partitioned into S0-K/S0-R/S0-P/S0-X/S0-A above; effectful adaptation belongs to S0-H. Outside
those reviewed responsibilities, Rust may provide only transport and embedding glue. New language
semantics must not expand in Rust during cutover.

The following v0.2 path list is a coarse migration guard for semantic-token growth, not a stage0
membership list or authority grant. An allowed path acquires no trust merely by matching this list.
R4.1.e must replace this coarse allowlist with exact source membership and dependency edges.

Approved Rust host-side modules (v0.2):
- `crates/gc_effects/src/lib.rs`
- `crates/gc_effects/src/runner.rs`
- `crates/gc_effects/src/runner_capability_dispatch.rs`
- `crates/gc_effects/src/runner_cap_*.rs`
- `crates/gc_effects/src/runner_*_host.rs`
- `crates/gc_effects/src/runner_remote_ops.rs`
- `crates/gc_effects/src/runner_response_budget.rs`
- `crates/gc_effects/src/store.rs`
- `crates/gc_effects/src/refs.rs`
- `crates/gc_effects/src/policy.rs`
- `crates/gc_effects/src/log.rs`
- `crates/gc_effects/src/lock.rs`
- `crates/gc_obligations/src/store.rs`
- `crates/gc_cli_driver/src/*.rs`
- `crates/gc_cli/src/main.rs`
- `crates/gc_wasi_cli/src/main.rs`
- `crates/gc_wasm/src/lib.rs`

Approved host ABI operation families (qualified op names):
- `core/store::*`
- `core/refs::*`
- `core/sync::*`
- `io/fs::*`
- `sys/time::now`
- `gfx/window::*`
- `gfx/input::*`
- `gfx/audio::*`
- `gfx/gpu::*`
- `gpu/compute::*`
- `editor/*`

Guardrail rule:
- New parser/canonicalizer/typechecker/optimizer/contract semantics should be implemented in `.gc`
  modules and routed through selfhost execution paths; Rust host modules may only marshal inputs,
  call the kernel/runtime, and materialize capability effects.

Package low-level semantic bridge status:
- The temporary `gc_pkg::parse_canonical_module_source` bridge is retired.
- `core/pkg-low::{load-package,snapshot}` must not depend on `gc_pkg` semantic helper APIs.
- Enforcement:
  `scripts/check_pkg_low_semantic_boundary.sh` and
  `crates/gc_cli/tests/pkg_low_semantic_boundary.rs`.

CI enforcement:
- `scripts/check_selfhost_boundary.sh` fails when a change adds core semantic API usage
  (`parse_module`, `canonicalize_module`, `print_module`, `hash_module`, `eval_module`, `eval_term`)
  in non-approved Rust files.
  - Rust test files under `crates/*/tests/*` are excluded from this guard so conformance and
    adversarial fixtures can exercise semantic APIs without widening the production runtime TCB.
  - Strict mode (`--strict`) scans production `crates/*/src/**/*.rs` and excludes benchmark-only
    crate `crates/gc_runtime_bench/*`; default diff mode (`--diff`) remains optimized for local iteration.
- `scripts/check_prelude_capability_coverage.sh` fails when a shipped
  `prelude/modules/10_gfx_00_gpu_scene.gc` (plus split gfx module siblings),
  `prelude/modules/11_gpu_compute.gc`, or
  `prelude/modules/20_editor.gc` wrapper op is not explicitly dispatched by
  `crates/gc_effects/src/runner_capability_dispatch.rs`.
- `scripts/check_kernel_tcb_contract.sh` fail-closes kernel evaluator-surface growth by enforcing:
  - explicit kernel source-set contract (`policies/kernel_tcb_contract.toml`)
  - eval-file line budgets for high-churn evaluator modules
  - `eval.rs` boundary markers (must delegate treewalk through `eval_treewalk::eval_term_impl`)
    and forbidden in-file treewalk implementation markers.
- `scripts/check_vcs_selfhost_contract.sh` fail-closes VCS command routing drift by enforcing:
  - parity-only cfg-gating for Rust VCS program builders in `crates/gc_cli_driver/src/cmd_vcs.rs`
  - production-driver compile check without parity harness features
  - parity-driver compile check with parity harness enabled.
- `scripts/check_host_api_evolution_contracts.sh` fail-closes high-churn host API drift by enforcing:
  - coverage and schema-envelope contracts for GPU/XR/editor/network/plugin domains
  - deterministic domain-level and overall host API contract hashes.
- `scripts/check_gcpm_operation_contract_pack.sh` fail-closes `gcpm` contract drift by enforcing:
  - versioned operation contract pack parity (`build/run/test/trace/qualify`)
  - deterministic failure taxonomy constant stability.
## Historical v0.2 Migration Plan (Non-Status)

The following sections preserve the v0.2 migration design. They do not report current semantic
ownership, assign an H-level, or supersede the stage0 contract above. Current authority is
established only by the future R4.1.c semantic-ownership ledger and its independently checked
evidence.

## Self-Host Definition (v0.2)

We call the toolchain “self-hosted” when:
1. The **frontend** (parser + canonical printer + canonical hash) is implemented in GenesisCode.
2. A GenesisCode “compiler/tool” can be executed by the kernel-on-WASM host (`gc_wasm` Runtime stepping),
   producing the same canonical outputs as the Rust implementation.
3. Safety is maintained via **translation validation** and obligation gating:
   - whenever a GenesisCode tool produces a transformed artifact, a verifier checks equivalence against
     a trusted baseline (initially the Rust implementation, later an older self-hosted release).

## Minimal Frontend Subset (Bootstrap-Friendly)

To self-host the frontend, we need a GenesisCode subset that can:
- operate on CoreForm terms (lists/pairs, vectors, maps, symbols, strings, bytes)
- implement deterministic printers (exact whitespace and ordering rules)
- compute BLAKE3 hashes of canonical bytes
- read/write artifacts via effects (host-provided `io/fs::*` or `core/store::*`)

This subset must avoid:
- reliance on ambient time or randomness
- any host-specific floating point or locale behavior

## Phased Cutover Plan

This section records the historical migration sequence, not current authority status.
`docs/spec/SEMANTIC_OWNERSHIP_LEDGER_v0.1.json` is authoritative for each exact
decision. Partial obligation migration is constrained by
`docs/spec/SELFHOST_OBLIGATION_AUTHORITY_v0.1.md`; it does not promote the umbrella
`SD-OBLIGATION` row.

### Phase 0: Rust-Defined Norms (historical baseline)

Rust is the source of truth for:
- CoreForm canonical printer + hash
- effect request/response hashing
- obligations and policy enforcement

Bootstrap note:
- To enable writing tooling logic in GenesisCode before the frontend is fully self-hosted, the
  prelude exposes a minimal **pure** CoreForm bootstrap API (parser/printer/hash) described in
  `docs/spec/SELF_HOST_BOOTSTRAP_API.md`.

### Phase 1: Self-Hosted Canonical Printer + Hash (frontend v0)

Deliverables:
- GenesisCode module `selfhost/printer` that prints CoreForm terms/modules canonically
- GenesisCode module `selfhost/hash` that hashes canonical bytes with the `GCv0.2\\0` prefix scheme
- Golden tests:
  - Rust fmt/hash output == selfhost fmt/hash output for `tests/spec/**` fixtures

Acceptance:
- `gc_wasm` can load and run the selfhost printer/hash module and match Rust outputs exactly.

### Phase 2: Self-Hosted Parser (frontend v1)

Deliverables:
- GenesisCode parser for CoreForm syntax, including strings/bytes/maps/vectors and comment handling
- Roundtrip and golden tests against Rust parser

Acceptance:
- parse -> print -> parse stability matches Rust for fixtures.

### Phase 3: Self-Hosted Tool Commands (toolchain v0)

Targets (in priority order):
- `fmt` (file -> canonical bytes)
- `vcs hash` (file -> hash)
- `optimize` on the pure subset
- `pkg snapshot` (construct a snapshot datum from module/package sources)

Execution model:
- Host (Rust/WASM) provides I/O via effects.
- Tool commands are pure functions from inputs to outputs + effect requests.

### Phase 3.5: Compiled Evaluator Path (toolchain throughput)

To keep selfhost tooling practical under deterministic step budgets, the kernel provides:
- `compile_module` (CoreForm terms -> compiled expression graph)
- `eval_compiled_module` / `eval_module_compiled`

Design constraints:
- No semantic changes vs tree-walking evaluator.
- Same protocol/error behavior (`UNHANDLED` / `EFFECT` / `ERROR`).
- Value hashing/logging remain stable (compiled closures hash like regular closures by source body + env).

#### Primitive-call normalization contract

Source application remains left-associated curried application, including n-ary sugar. The compiled
evaluator may normalize a fully supplied chain of unary closures whose final body is a known
primitive applied only to resolved lexical parameters. Eligibility is derived from lexical
`(depth, slot)` resolution and the primitive allowlist, never source names, literals, benchmark
identity, or expected results.

The normalization plan is evaluator metadata. It is not serialized in `GCKM5`, does not participate
in value/effect hashing, and is recomputed from decoded semantic IR. Execution must preserve the
source argument evaluation order, intermediate closure and final primitive step charges, variable
coverage hits and statement sites, sealed values/errors, boundary error provenance, memory limits,
and over-application order. A step limit or enabled coverage must not disable or alter the path.
Partially supplied calls use ordinary closure semantics and retain their existing body, environment,
hash, and later completion behavior. Unsupported shapes fall back before evaluating any argument;
the plan cannot add a primitive or broaden an existing primitive's accepted values.

Native n-ary calls may accumulate newly evaluated arguments without materializing intermediate
wrapper values. A partial native function is materialized exactly when that partial value becomes
observable; a fully supplied call invokes once after the same left-to-right argument evaluation.

Current usage:
- Prelude bootstrap runs through the compiled evaluator.
- Selfhost toolchain bootstrap modules (`selfhost/{parse,canon,printer,hash,tool_coreform_v1}.gc`) run through the compiled evaluator.

### Phase 4: Obligation-Guarded Cutover

Once selfhost tools exist:
- The Rust CLI becomes a thin host that:
  - loads the selfhost tool module
  - runs it under effect policies
  - checks translation validation obligations (and/or cross-checks with Rust for a time)

Eventually:
- Rust becomes optional tooling and the “release toolchain” is a GenesisGraph artifact (installable via `.gpk`).

Current cutover mechanism (implemented):
- Rust can produce a canonical selfhost toolchain artifact:
  - `genesis selfhost-artifact --out <path>`
- Runtime can load that artifact instead of embedded bootstrap sources by setting:
  - `GENESIS_SELFHOST_TOOLCHAIN_ARTIFACT=<path>`
- Loader validation before activation:
  - artifact schema + kind/version checks
  - required selfhost module set present (parse/canon/printer/hash/tool)
  - per-module canonical `:forms` hash matches declared module hash
  - `:stage1-ok` must be true for every module
  - when `:stage2-supported` is true, `:stage2-ok` must be true
  - production profile rejects source-only modules (no Rust source parse fallback)

This makes artifact-based bootstrap testable today while retaining embedded fallback for explicit
development builds.

Artifact module contract:
- production bootstrap requires `:forms` on each module entry and validates those canonical forms
  against `:module-h`.
- `:source` remains informational in production; it is not parsed during release bootstrap.
- parity-harness/development profile may allow source-parse fallback for diagnostics and migration.

Host tooling defaults:
- native CLI (`genesis`) and WASI CLI now default to `artifact-only` bootstrap mode for selfhost paths.
- routed frontend commands now default to selfhost execution; explicit Rust engine selection is
  retained only for parity/comparison workflows.
- runtime flags:
  - `--selfhost-artifact <file>` choose artifact explicitly
  - production binaries: `--selfhost-bootstrap artifact-only` (only accepted value)
  - parity-harness binaries: `--selfhost-bootstrap artifact-only|artifact-preferred|embedded`
  - `--selfhost-only` enforce hard selfhost mode (also `GENESIS_SELFHOST_ONLY=1`)
- `embedded` mode remains available only in parity-harness/development workflows.
- production parse surface does not accept `artifact-preferred` or `embedded`.

Selfhost-only hard mode:
- commands with `--engine` must use `--engine selfhost`
- bootstrap mode must be `artifact-only` (no embedded fallback)
- commands outside the routed selfhost command groups are rejected early with stable
  verification exit code `50`, so CI can gate on strict selfhost surface only.
- `docs/status/SELFHOST_CUTOVER.md` (generated by `genesis selfhost-dashboard`) is the
  canonical routed/default command-coverage source only. It does not establish semantic
  implementation authority or bootstrap closure; those maturity claims are generated from
  the capability ledger in `docs/status/SELFHOST_AUTHORITY_v0.1.md`.

Release hardening:
- `gc_prelude::load_selfhost_coreform_toolchain_v1` now defaults to `artifact-only`.
- feature `gc_prelude/embedded-bootstrap` is development-only and rejected in release builds.

WASM host bridge support:
- `gc_wasm` now supports explicit artifact bootstrap for selfhost frontend/tooling paths:
  - `fmt_coreform_module_selfhost_with_artifact`
  - `hash_coreform_module_selfhost_with_artifact`
  - `eval_coreform_module_selfhost_with_artifact`
  - `Runtime.eval_module_selfhost_with_artifact`
- This allows browser/Node hosts to pass a verified artifact directly without filesystem coupling.
- Production wasm selfhost paths fail closed when no explicit artifact is provided.
  Implicit embedded bootstrap fallback is not allowed in `wasm32` selfhost APIs.

## Translation Validation Strategy

Translation validation is treated as an **obligation**:
- any transformation (printer, optimizer, compiler) must produce evidence that:
  - the output canonical form hashes as expected
  - executing tests on output matches tests on input (for pure programs, value hash equality)

During early self-hosting:
- validate selfhost outputs directly against Rust (same fixtures, same hashes).
Later:
- validate against the last accepted selfhost release (bootstrapping chain).
