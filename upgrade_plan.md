# GenesisCode Upgrade Plan (WASM-First -> Self-Host)

Date: 2026-02-15

North star:
- Minimal Rust bootstrap that can be compiled to WASM (WASI/wasmtime and wasm-bindgen hosts).
- Then replace the bootstrap with a self-hosted GenesisCode toolchain running on WASM.

Non-negotiables:
- Kernel stays pure and deterministic (effects only via runner + `.gclog` + replay).
- Keep `cargo fmt --all`, `cargo test --workspace`, `cargo clippy --workspace --all-targets -- -D warnings` green.
- No mock/simulated product behavior.
- Keep test output clean (no noisy property-test persistence warnings or other nondeterministic spew).

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
- [x] CI/test hygiene:
  - proptest-based fuzz/property tests do not emit failure-persistence warnings (disable persistence for fuzz-style invariants)

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
    - self-hosted canonicalize/print/hash via `selfhost/{canon,printer,hash}.gc` (no `core/coreform::*` on the tool path)
  - Equivalence tests: `crates/gc_prelude/tests/selfhost_tool_coreform_equivalence.rs`
- [x] Bootstrap selfhost modules for canon/printer/hash remain available (fast, deterministic):
  - `selfhost/canon.gc`, `selfhost/printer.gc`, `selfhost/hash.gc`
  - Equivalence tests: `crates/gc_prelude/tests/selfhost_{canon,printer,hash}_equivalence.rs`
- [x] Implement a truly self-hosted CoreForm frontend (no Rust CoreForm dependency at all on the tool path):
  - canonicalization and canonical printing in GenesisCode (`selfhost/{canon,printer}.gc`)
  - hashing from canonical bytes in GenesisCode (`selfhost/hash.gc`)
  - regression coverage:
    - singleton list grouping canonicalizes like the kernel (`(y)` formats to `y`) via `crates/gc_prelude/tests/selfhost_singleton_parens_regression.rs`
- [x] Cutover tooling entrypoints to support a self-host toolchain engine (opt-in):
  - CLI: `genesis fmt --engine selfhost` uses `selfhost/tool::fmt-module` (honors `--step-limit/--no-step-limit`)
  - CLI: `genesis eval --engine selfhost` uses `selfhost/parse::parse-module` + `selfhost/canon::canonicalize-module` (honors `--step-limit/--no-step-limit`)
  - WASI CLI mirrors the same `eval --engine selfhost` behavior
  - wasm-bindgen: expose `fmt_coreform_module_selfhost`, `hash_coreform_module_selfhost`, and `eval_coreform_module_selfhost`
  - wasm-bindgen runtime: `Runtime.eval_module_selfhost` for step/resume hosts
  - tests:
    - `crates/gc_cli/tests/cli_fmt_engine.rs` asserts `fmt --engine selfhost` output matches Rust engine
    - `crates/gc_cli/tests/cli_eval_engine.rs` asserts `eval --engine selfhost` parity + parse error surfacing
    - `crates/gc_wasm/src/lib.rs` test `eval_coreform_module_selfhost_matches_rust_frontend_eval`
- [ ] Implement compilation stages suitable for WASM-first execution:
  - [x] stage 1: CoreForm -> CoreForm transforms (optimized, validated)
    - `gc_opt::stage1_pipeline` now runs optimize + canonicalize + validation gate report
    - validation gate is `core/obligation::stage1-validation` (pure/hash-equivalence on pure programs)
    - CLI integrates obligation gating:
      - `genesis eval --stage1-pipeline [--stage1-gate]`
      - `genesis optimize --engine rust|selfhost [--stage1-gate]`
    - `gc_obligations` supports `core/obligation::stage1-validation` for package/module runs
    - tests: `crates/gc_cli/tests/cli_stage1_pipeline.rs` and `gc_opt` stage1 validation tests
  - [x] stage 2 baseline: CoreForm -> WASM for pure scalar subset (int/bool) behind translation validation obligation
    - `gc_opt::stage2_compile_module` lowers supported CoreForm into deterministic WASM bytes
    - `gc_opt::stage2_validation_report` executes lowered WASM and compares kernel vs WASM value hashes
    - `genesis optimize` adds `--stage2-gate` and `--emit-wasm <file>`
    - `core/obligation::translation-validation` now records `:stage2` evidence and fails when supported Stage-2 translations mismatch
    - remaining scope: widen Stage-2 coverage beyond scalar pure subset to full module/test surface
  - [ ] Cutover plan:
    - [x] Rust produces a self-host toolchain artifact:
      - `genesis selfhost-artifact --out <file>` emits a canonical CoreForm artifact with per-module Stage-1/Stage-2 validation metadata.
    - [x] Runtime can consume a validated self-host artifact:
      - `gc_prelude::load_selfhost_coreform_toolchain_v1` now supports `${GENESIS_SELFHOST_TOOLCHAIN_ARTIFACT}` and verifies schema, module hashes, and gate flags before loading.
    - [x] make artifact-based bootstrap the default distribution path and reduce embedded-source fallback:
      - host CLIs default selfhost bootstrap mode to `artifact-only` (`--selfhost-bootstrap artifact-only|artifact-preferred|embedded`).
      - `--engine selfhost` now requires a validated artifact by default (`--selfhost-artifact` or `./.genesis/selfhost/toolchain.gc`).
      - embedded bootstrap is now an explicit dev fallback (`--selfhost-bootstrap embedded`).
    - [ ] Rust becomes optional tooling only (remaining):
      - remove/feature-gate embedded-source bootstrap from release profiles once artifact distribution is fully standardized across host runtimes.
- [x] Make self-hosted tooling fast/practical under the kernel step limit:
  - add a compiled execution path (bytecode-like in-kernel compiled evaluator) for toolchain-grade workloads
  - `gc_kernel::{compile_module, eval_compiled_module, eval_module_compiled}`
  - prelude + selfhost toolchain bootstrap now execute via compiled evaluator
  - [x] treat toolchain bootstrap as trusted init:
    - prelude + selfhost toolchain evaluation run without step/memory limits
    - user budgets start after init (`EvalCtx::reset_counters`)
  - [x] parser perf: `bytes/join` primitive + `core/bytes::join` wrapper to avoid O(n^2) byte concatenation in self-host parsing
  - [x] re-enable an end-to-end `io/fs` formatting test driven by `selfhost/tool_coreform_v1.gc`
  - [x] kernel: tail-call optimize final closure applies in tail position (prevents stack overflows on tail recursion)
  - [ ] After full self-host cutover (toolchain + compilation stages), archive bootstrap-only implementation artifacts:
    - add `bootstrap_old/` and move any legacy build scripts/tooling (Python/Node) that are no longer required for the self-hosted workflow
    - document what remains required for reproducible builds and why (WASM host bridges, etc.)

---

## P4: Post-Selfhost (GenesisCode-Only, On WASM)

Everything in P4+ is blocked until we have a self-hosted GenesisCode toolchain running on WASM (Rust host only).

### P4.1 Level 2: Universal Graphics Stack (2D/3D)

Goal: a production-grade, extensible graphics library that can target “anything from websites to 3D games”.
Constraints:
- written exclusively in GenesisCode (no Rust-side rendering logic)
- runs on WASM (browser first), with a host bridge providing GPU/window/input as capabilities
- state-of-the-art performance (GPU-first, explicit resource lifetime, predictable allocations)

- [x] Define the graphics host capability surface (effects) and policies:
  - Draft spec in `docs/spec/GFX_CAPS.md`
  - Runtime surface hardened in `gc_effects`:
    - `gfx/time::frame-tick` implemented with replayable `{:time-ms ...}` responses
    - remaining `gfx/*` ops return stable sealed `core/caps/not-supported` (not `unknown-op`) until host bridge backends land
  - `gfx/gpu::*` (WebGPU-backed): instance/device/queue, buffers, textures, samplers, shaders, pipelines, bind groups, command encoding, present
  - `gfx/window::*` (browser canvas + later native shell): create/surface resize, pixel ratio
  - `gfx/input::*` (events): pointer/keyboard/gamepad
  - `gfx/time::*` (frame time) as an effect input (no ambient time in kernel)
  - `gfx/audio::*` (optional, later)
  - determinism: input/time must be loggable and replayable; rendering is an effect-only sink
- [x] Introduce deterministic graphics data foundations in Rust (`crates/gc_gfx`):
  - frame graph + render/compute command schemas
  - 2D/3D scene graph + PBR material schema
  - canonical CoreForm term projection + stable hashes (`frame_graph_hash`, `scene_hash`)
- [x] Specify core data model + architecture for the Level 2 graphics library:
  - Locked in `docs/spec/GFX_ARCH.md` (layer model, deterministic scene/frame planning, ECS interop, plugin contracts, UI foundation, obligations)
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

- [x] Define editor architecture + host capability requirements (draft):
  - `docs/spec/EDITOR_ARCH.md`
- [x] Define editor host capabilities (effects) needed beyond graphics:
  - Spec: `docs/spec/EDITOR_CAPS.md`
  - Prelude wrappers added in `prelude/prelude.gc` for `editor/clipboard::*`, `editor/dialog::*`, `editor/task::*`, `editor/watch::*` and `gfx/*`
  - Runtime behavior normalized in `gc_effects`:
    - known editor ops return sealed `core/caps/not-supported` on hosts without editor bridges
  - Request-shape coverage in `crates/gc_prelude/tests/prelude_caps_wrappers.rs`
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
