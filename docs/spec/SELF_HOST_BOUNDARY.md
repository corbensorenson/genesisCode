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

### Stage C - Bootstrap Archive Retirement (`/old_bootstrap`)

When Stage A+B hold continuously, bootstrap-only Rust compatibility surfaces move to `old_bootstrap/`
and are no longer referenced by production code paths.

Measurable retirement criteria:

1. `scripts/check_bootstrap_retirement_gate.sh` passes with strict release checks enabled.
2. `docs/spec/BOOTSTRAP_OLD.md` keeps retirement checklist fully checked and explicitly approved.
3. `scripts/check_old_bootstrap_retirement.sh` reports zero production references to archived bootstrap semantics.
4. `scripts/check_selfhost_boundary.sh --strict` passes after retirement move.

Only after these criteria are satisfied should bootstrap-era Rust semantic helpers be considered
fully retired from active production usage.

## Trust Boundaries

### TCB-A: Pure Kernel (must stay tiny)

This is the minimal trusted base that executes GenesisCode:
- CoreForm canonicalization + hashing (`gc_coreform`)
- Pure evaluator + value hashing (`gc_kernel`)
- Prelude protocol hardening (`gc_prelude`)

This TCB is what we compile to `wasm32-unknown-unknown` (`crates/gc_wasm`) and embed in browser/Node.

### TCB-B: Effect Runner + Tooling (allowed to do I/O)

This layer is outside kernel purity and may do filesystem/network I/O:
- capability runner + `.gclog` + replay (`gc_effects`)
- package/GenesisGraph tooling, bundling, policies
- obligations and evidence store

Under WASI, this layer runs as `genesis_wasi.wasm`.

Self-hosting targets this layer first: we want tooling logic (formatter, packager, optimizer passes, etc.)
to be written *in GenesisCode* and run on TCB-A.

## Rust Host-Only ABI (Strict)

The Rust host boundary is intentionally narrow and versioned. Rust is allowed to provide only
transport/adaptation for effectful capabilities and embedding glue. Language semantics must not
expand in Rust beyond existing TCB-A crates during cutover.

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
  `prelude/modules/10_gfx.gc`, `prelude/modules/11_gpu_compute.gc`, or
  `prelude/modules/20_editor.gc` wrapper op is not explicitly dispatched by
  `crates/gc_effects/src/runner_capability_dispatch.rs`.

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

### Phase 0: Rust-Defined Norms (today)

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
- commands not yet selfhost-routed are rejected early with a stable verification exit code
  so CI can gate on strict selfhost surface only.

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

## Translation Validation Strategy

Translation validation is treated as an **obligation**:
- any transformation (printer, optimizer, compiler) must produce evidence that:
  - the output canonical form hashes as expected
  - executing tests on output matches tests on input (for pure programs, value hash equality)

During early self-hosting:
- validate selfhost outputs directly against Rust (same fixtures, same hashes).
Later:
- validate against the last accepted selfhost release (bootstrapping chain).
