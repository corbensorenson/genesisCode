# GenesisCode Upgrade Plan (WASM-First -> Self-Host)

Date: 2026-02-15

North star:
- Minimal Rust bootstrap that can be compiled to WASM (WASI/wasmtime and wasm-bindgen hosts).
- Then replace the bootstrap with a self-hosted GenesisCode toolchain running on WASM.

Non-negotiables:
- Kernel stays pure and deterministic (effects only via runner + `.gclog` + replay).
- Keep `cargo fmt --all`, `cargo test --workspace`, `cargo clippy --workspace --all-targets -- -D warnings` green.
- No mock/simulated product behavior.

Key docs to treat as authoritative:
- `docs/spec/WASM.md`, `docs/spec/WASI.md`, `docs/spec/WASM_HOST_BRIDGE.md`
- `docs/spec/MODULE_SCOPE.md`, `docs/spec/VALUE_EFFECT_HASH.md`
- `docs/spec/CLI.md`, `docs/GENESISGRAPH_GENESISPKG_v0.2.md`
- Design guidance: `docs/STACKS_v0.2.md`, `docs/FOUNDATION_STDLIB_v0.2.md`
- `docs/CLI_SPEC_GENESISPKG_GENESISGRAPH_v0.1.md`, `docs/POLICY_DEFAULTS_v0.1.md`
- `docs/LOCK_GENERATOR_RULESET_v0.1.md`, `docs/REGISTRY_PROTOCOL_MINIMAL_v0.1.md`
- `docs/REACHABILITY_RULES_v0.1.md`, `docs/GARBAGE_COLLECTION_RULES_v0.1.md`

---

## P0: "Useful Today" (Native)

- [x] Write a short "getting started" program and tutorial that uses real features:
  - canonical formatting, eval, contracts, effects, run/replay, package snapshot + gpk export/import
  - See `docs/GETTING_STARTED.md` and `examples/hello_pkg/`.
- [x] Provide standard effect program combinators used in docs/style-guide:
  - `core/effect::bind` intrinsic (chains effect programs deterministically)
  - `core/effect::map` and `core/effect::then` helpers (in `prelude/prelude.gc`)

---

## P0.5: Level 1 Foundation Stack (Canonical Libraries + Conventions)

Goal: "complete enough" day-to-day programming without Level 2 subsystems.

- [x] Standard data layer utilities in `prelude/prelude.gc`:
  - `core/list::{len,reverse,append,map,filter,foldl}` with proper-list validation
  - add tests in `gc_prelude` for the list utilities
- [x] Message/contract convenience helpers:
  - `core/contract::call` wrapper (dispatch + msg make)
  - optional aliases for protocol predicates under `core/contract::*`
- [x] Effect programming toolkit:
  - `core/effect::{catch,catch-payload}` (error-as-value) + tests
  - document canonical effect payload shapes (maps with keyword keys)
- [ ] In-language convenience wrappers for GenesisGraph/GenesisPkg capability ops (pure constructors):
  - `core/store::*`, `core/refs::*`, `core/vcs::*`, `core/pkg::*`, `core/sync::*`, `core/gc::*`
  - wrappers should only construct effect programs; runners remain capability-gated

---

## P1: WASM Bootstrap (Rust Compiled To WASM)

### P1.1 WASI toolchain ("runs on WASM")

- [x] WASI CLI for pure subset (`fmt`, `eval`, `vcs hash`): `crates/gc_wasi_cli` producing `genesis_wasi.wasm`.
- [x] CI proves WASI smoke via wasmtime.
- [x] `gc_effects` builds on `wasm32-wasip1` (local-only; sync/remote is denied on WASI).
- [ ] Expand WASI CLI to cover effectful local workflows:
  - [x] `run` and `replay` with deterministic `.gclog`
  - [x] `store put/get/has` (local `.genesis/store`)
  - [x] `refs get/set/list/delete` (local refs db)
  - Acceptance: WASI outputs match native for the same inputs and logs.
- [x] Add WASI smoke tests for `run` and `replay` (compare native vs wasmtime) in CI.

### P1.2 wasm-bindgen hosts (Node and browser)

- [x] Node wasm-bindgen smoke for stepping interface (`docs/spec/WASM_HOST_BRIDGE.md`).
- [x] Browser build + harness:
  - `wasm-bindgen --target web` artifacts via `scripts/wasm_bindgen_web.sh`
  - headless browser CI smoke via Playwright (`scripts/wasm_web_smoke.mjs`) asserting cross-host hash equivalence

---

## P2: WASM-First Toolchain Features (Still Rust, But Runs Under WASI)

- [x] Implement WASI-safe policies for host boundary:
  - filesystem sandboxing and canonical path rules (`docs/spec/FS_SANDBOX.md`)
  - network denied by default (explicit capability only; `core/sync::*` denied under WASI bootstrap)
  - deterministic time only via effect logs (no ambient time in kernel)
- [ ] Make the WASI CLI support package workflows without network:
  - [x] `genesis pkg init/add/lock/update/install/verify/list/info` using local store and refs.
  - [x] `genesis pkg export/import` using `.gpk` bundles (shallow/full), local-only.
  - [x] WASI smoke asserts native vs WASI equivalence for `pkg init/add/lock/install` (plus store+refs).
  - [x] WASI supports `genesis pack` and `genesis test` for local workspaces.
  - [x] Acceptance: a workspace can be built and tested inside wasmtime (smoke covers `pack` + `test`).

---

## P3: Self-Host Boundary And Cutover

- [x] Write `docs/spec/SELF_HOST_BOUNDARY.md` (bootstrapping stages + translation validation plan).
- [x] Add a pure CoreForm bootstrap API to the prelude so GenesisCode tooling can be written in GenesisCode:
  - `core/coreform::{parse-term,parse-module,canonicalize-module,print-term,print-module,fmt-module,hash-term,hash-module,hash-module-src}`
  - Spec: `docs/spec/SELF_HOST_BOOTSTRAP_API.md`
- [x] Add pure primitives needed for a self-hosted printer/hash in GenesisCode:
  - UTF-8 conversions, string length, integer formatting
  - bytes indexing/slicing and hex conversion
  - raw `crypto/blake3` (bytes -> 32-byte digest)
- [x] Add pure term-introspection + canonical-print helpers needed to implement the CoreForm printer in GenesisCode:
  - `data/tag`, `pair/as-proper-list`, `map/entries`, `sym/to-str`
  - `str/repeat`, `str/join`
  - `coreform/escape-str`, `coreform/escape-bytes` (exactly matches canonical printer escaping rules)
- [ ] Implement a self-hosted "frontend v0" in GenesisCode:
  - [x] CoreForm printer equivalence tests against Rust (see `selfhost/printer.gc` + `crates/gc_prelude/tests/selfhost_printer_equivalence.rs`)
  - [x] CoreForm canonicalizer equivalence tests against Rust (rewrite-only pass to canonical form) (see `selfhost/canon.gc` + `crates/gc_prelude/tests/selfhost_canon_equivalence.rs`)
  - module loader and package resolver on GenesisGraph objects
- [ ] Implement compilation stages suitable for WASM-first execution:
  - stage 1: CoreForm -> CoreForm transforms (optimized, validated)
  - stage 2: CoreForm -> WASM (behind translation validation obligation)
  - [ ] Cutover plan:
  - Rust produces the self-host toolchain artifact
  - then runtime uses the self-host toolchain under obligations
  - Rust becomes optional tooling only
