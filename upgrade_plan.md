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
- [x] In-language convenience wrappers for GenesisGraph/GenesisPkg capability ops (pure constructors):
  - Implemented in `prelude/prelude.gc` for: `core/store::*`, `core/refs::*`, `core/vcs::*`, `core/pkg::*`, `core/sync::*`, `core/gpk::*`, `core/gc::*`
  - Validated via `crates/gc_prelude/tests/prelude_caps_wrappers.rs`

---

## P1: WASM Bootstrap (Rust Compiled To WASM)

### P1.1 WASI toolchain ("runs on WASM")

- [x] WASI CLI for pure subset (`fmt`, `eval`, `vcs hash`): `crates/gc_wasi_cli` producing `genesis_wasi.wasm`.
- [x] CI proves WASI smoke via wasmtime.
- [x] `gc_effects` builds on `wasm32-wasip1` (local-only; sync/remote is denied on WASI).
- [x] Expand WASI CLI to cover effectful local workflows:
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
- [x] Make the WASI CLI support package workflows without network:
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
- [x] Harden `core/coreform::*` bootstrap API to be total on user input:
  - parse/canonicalize failures return sealed protocol ERROR values (not Rust errors)
  - parse errors use stable `:error/code` (`core/parse/{eof,unexpected,escape,int}`) and `:error/context{:at ...}` for tooling/tests
- [x] Add pure primitives needed for a self-hosted printer/hash in GenesisCode:
  - UTF-8 conversions, string length, integer formatting
  - bytes indexing/slicing and hex conversion
  - raw `crypto/blake3` (bytes -> 32-byte digest)
- [x] Add pure term-introspection + canonical-print helpers needed to implement the CoreForm printer in GenesisCode:
  - `data/tag`, `pair/as-proper-list`, `map/entries`, `sym/to-str`
  - `str/repeat`, `str/join`
  - `coreform/escape-str`, `coreform/escape-bytes` (exactly matches canonical printer escaping rules)
- [x] Self-hosted CoreForm parser v1 (no Rust parser dependency):
  - `selfhost/parse.gc` parses terms/modules from either UTF-8 strings or UTF-8 bytes
  - Equivalence tests: `crates/gc_prelude/tests/selfhost_parse_equivalence.rs`
- [x] Self-hosted CoreForm tooling v1 (no Rust parser dependency):
  - `selfhost/tool_coreform_v1.gc` implements `selfhost/tool::{fmt-module,hash-module-src}` using:
    - self-hosted parsing via `selfhost/parse::*`
    - bootstrap canonicalize/print/hash via `core/coreform::{canonicalize-module,print-module,hash-module}`
  - Equivalence tests: `crates/gc_prelude/tests/selfhost_tool_coreform_equivalence.rs`
- [x] Bootstrap selfhost modules for canon/printer/hash remain available (fast, deterministic):
  - `selfhost/canon.gc`, `selfhost/printer.gc`, `selfhost/hash.gc`
  - Equivalence tests: `crates/gc_prelude/tests/selfhost_{canon,printer,hash}_equivalence.rs`
- [ ] Implement a truly self-hosted CoreForm frontend (no Rust CoreForm dependency at all):
  - canonicalization and canonical printing in GenesisCode (no `core/coreform::*` usage)
  - hashing from canonical bytes in GenesisCode
  - (parser already done)
- [x] Cutover tooling entrypoints to support a self-host toolchain engine (opt-in):
  - CLI: `genesis fmt --engine selfhost` uses `selfhost/tool::fmt-module` (honors `--step-limit/--no-step-limit`)
  - wasm-bindgen: expose `fmt_coreform_module_selfhost` and `hash_coreform_module_selfhost`
  - tests: assert `--engine selfhost` output matches Rust engine on fixtures
- [ ] Implement compilation stages suitable for WASM-first execution:
  - stage 1: CoreForm -> CoreForm transforms (optimized, validated)
  - stage 2: CoreForm -> WASM (behind translation validation obligation)
  - [ ] Cutover plan:
  - Rust produces the self-host toolchain artifact
  - then runtime uses the self-host toolchain under obligations
  - Rust becomes optional tooling only
- [ ] Make self-hosted tooling fast/practical under the kernel step limit:
  - add a compiled execution path (bytecode or WASM) for toolchain-grade workloads
  - [x] treat toolchain bootstrap as trusted init:
    - prelude + selfhost toolchain evaluation run without step/memory limits
    - user budgets start after init (`EvalCtx::reset_counters`)
  - [x] parser perf: `bytes/join` primitive + `core/bytes::join` wrapper to avoid O(n^2) byte concatenation in self-host parsing
  - [x] re-enable an end-to-end `io/fs` formatting test driven by `selfhost/tool_coreform_v1.gc`
  - [x] kernel: tail-call optimize final closure applies in tail position (prevents stack overflows on tail recursion)

---

## P4: Post-Selfhost (GenesisCode-Only, On WASM)

Everything in P4+ is blocked until we have a self-hosted GenesisCode toolchain running on WASM (Rust host only).

### P4.1 Level 2: Universal Graphics Stack (2D/3D)

Goal: a production-grade, extensible graphics library that can target “anything from websites to 3D games”.
Constraints:
- written exclusively in GenesisCode (no Rust-side rendering logic)
- runs on WASM (browser first), with a host bridge providing GPU/window/input as capabilities
- state-of-the-art performance (GPU-first, explicit resource lifetime, predictable allocations)

- [ ] Define the graphics host capability surface (effects) and policies:
  - `gfx/gpu::*` (WebGPU-backed): instance/device/queue, buffers, textures, samplers, shaders, pipelines, bind groups, command encoding, present
  - `gfx/window::*` (browser canvas + later native shell): create/surface resize, pixel ratio
  - `gfx/input::*` (events): pointer/keyboard/gamepad
  - `gfx/time::*` (frame time) as an effect input (no ambient time in kernel)
  - `gfx/audio::*` (optional, later)
  - determinism: input/time must be loggable and replayable; rendering is an effect-only sink
- [ ] Specify core data model + architecture for the Level 2 graphics library:
  - scene graph +/or ECS (define which is canonical, and how they interop)
  - render graph / frame graph with explicit passes
  - asset pipeline primitives (images, meshes, fonts) as GenesisGraph artifacts
  - UI foundation: layout (flex-like), vector graphics, text shaping, accessibility hooks
  - extension mechanism: plugins register render passes, components, and asset types (all via contracts)
- [ ] Implement the Level 2 graphics stack in GenesisCode:
  - low-level GPU wrapper layer (thin, stable API over `gfx/gpu::*`)
  - 2D renderer (shapes, sprites, text) + batching
  - 3D renderer (PBR baseline), cameras, lights, shadows (phased)
  - UI toolkit built on 2D primitives (widgets as contracts)
  - end-to-end demos: 2D UI app, 3D scene, and a hybrid “web app” view
- [ ] Add obligations for graphics correctness + performance:
  - golden image tests (headless browser, deterministic input logs)
  - frame time budgets (bench evidence artifacts)
  - API stability checks for the public Level 2 surface

### P4.2 Level 3: GenesisCode GUI Editor (First “Big” Self-Host App)

Goal: a GUI code editor written exclusively in GenesisCode, designed for GenesisGraph + GenesisPkg workflows,
and plugin/agent-friendly from day 1.

- [ ] Define editor host capabilities (effects) needed beyond graphics:
  - filesystem (workspace access), store/refs/sync, clipboard, OS dialogs
  - optional: language server–like background tasks (still effect-logged)
- [ ] Implement editor core (GenesisCode-only):
  - incremental parser integration (once self-host parser exists) + AST aware editing
  - CoreForm formatting + linting + typecheck + optimize flows as in-editor actions
  - GenesisGraph-native UX: commit/log/blame/why/evidence views
  - GenesisPkg UX: lock/install/update/publish/import/export, policy gating UI
- [ ] Implement a GenesisCode linter (GenesisCode-only) and integrate it into the editor:
  - fast, incremental lint for CoreForm and higher-level “Level 1 Foundation” conventions
  - includes deterministic autofix patches (semantic patch artifacts) where safe
  - lints are obligation-producible evidence artifacts (so they can gate refs/publish)
- [ ] Plugin + agent architecture (GenesisCode-only):
  - plugin API as contracts; sandboxed capabilities per plugin
  - agent actions as semantic patches + obligation-gated acceptance pipeline
  - deterministic “agent session logs” (effect logs + patch artifacts) for replay/audit

### P4.3 AI Authoring Skill (Codex / Agent Guidance)

Once the toolchain is fully self-hosted on WASM:
- [ ] Write a canonical AI authoring guide as a `SKILL.md` (GenesisCode coding skill):
  - language norms, canonical library usage (Levels 0–2), error convention, effect patterns
  - GenesisGraph/GenesisPkg workflows (patch-first, obligations-first)
  - performance + determinism rules for WASM targets
  - recommended “prompt protocol” for agentic refactors (plan -> patch -> evidence -> accept)
