# Self-Host Boundary (WASM-First) v0.2

Goal: reach a state where the GenesisCode toolchain can run **on top of WASM** with a minimal Rust TCB,
then progressively replace Rust components with GenesisCode implementations until the toolchain is
self-hosted.

Non-goals (v0.2):
- Refinement proofs (extension points only).
- Replacing the kernel evaluator (Gλ) in the short term. The kernel remains the trusted “execution engine”.

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

## Translation Validation Strategy

Translation validation is treated as an **obligation**:
- any transformation (printer, optimizer, compiler) must produce evidence that:
  - the output canonical form hashes as expected
  - executing tests on output matches tests on input (for pure programs, value hash equality)

During early self-hosting:
- validate selfhost outputs directly against Rust (same fixtures, same hashes).
Later:
- validate against the last accepted selfhost release (bootstrapping chain).
