# GenesisCode Self-Host Cutover Plan (Rust Bootstrap Exit)

Date: 2026-02-16

## Objective
- Remove Rust bootstrap code from the language/toolchain critical path.
- Run GenesisCode tooling and language evolution from `.gc` implementations.
- Keep only a minimal host runtime boundary for capabilities (filesystem/network/window/gpu), with no language semantics in Rust.
- After cutover, do optimization and feature growth in `.gc` first.

## Non-Negotiables
- Kernel remains pure/deterministic.
- Effects stay capability-gated, deny-by-default, replayable.
- No mock/simulated product behavior.
- All cutover steps require passing tests, clippy, and deterministic hash/log checks.

## Definition of Done (Global)
- `genesis` default execution path uses self-hosted `.gc` toolchain for parse/canon/print/hash/eval/typecheck/optimize/patch.
- Rust does not implement language semantics (parser/printer/typechecker/optimizer/contracts/tooling logic).
- Rust only hosts capability adapters + runtime embedding and can be swapped without language rewrite.
- Legacy Rust bootstrap code is moved to `/deprecated` or `bootstrap_old/` and excluded from default builds/tests.

---

## P0: Freeze Bootstrap Surface
- [x] Declare bootstrap boundary in `docs/spec/SELF_HOST_BOUNDARY.md` with a strict "Rust host only" ABI list.
- [x] Add CI guard: fail PRs that add new language semantics in Rust crates outside approved host ABI modules.
  - enforced by `scripts/check_selfhost_boundary.sh` and `.github/workflows/ci.yml` (`Selfhost Boundary Guard` step)
- [x] Add `--selfhost-only` mode that hard-fails if any Rust semantic fallback is invoked (for routed commands).
  - implemented in native + WASI CLIs, with env alias `GENESIS_SELFHOST_ONLY=1|true|yes|on`
  - strict mode enforces `--engine selfhost`, requires `--selfhost-bootstrap artifact-only`, and rejects non-routed commands
- [x] Add a cutover dashboard artifact (`.genesis/store` + markdown mirror) tracking % of commands executing through `.gc` path.
  - implemented as `genesis selfhost-dashboard [--markdown <file>] [--store <dir>]`

Acceptance gate:
- [x] CI proves `selfhost-only` mode works for `fmt`, `eval`, `typecheck`, `optimize`, `test`, `apply-patch` on golden suites.
  - [x] covered now: `fmt`, `eval`, `optimize` strict-mode gating in native CLI tests
  - [x] covered now: `explain` strict-mode gating and engine routing in native CLI tests
  - [x] covered now: `run`, `replay` strict-mode gating and engine routing in native CLI tests
  - [x] covered now: `typecheck` strict mode executes through selfhost parse/canonicalize path in native CLI tests
  - [x] covered now: `test` strict mode executes through selfhost frontend module-loading path in native CLI tests
  - [x] covered now: `apply-patch` strict mode executes through selfhost frontend parse/canonicalize path in native CLI tests
  - [x] covered now: `selfhost-dashboard` runs in strict mode and emits content-addressed dashboard artifacts
  - [x] covered now: `vcs hash` strict mode executes through selfhost tool handlers in native + WASI CLI tests
  - [x] covered now: `fmt`, `eval`, `run`, `replay`, `test`, `pack`, `vcs hash` strict-mode routing in WASI CLI tests
  - [x] covered now: native + WASI `fmt` auto-select selfhost via workspace fallback artifact `selfhost/toolchain.gc`
  - [x] covered now: native + WASI `run`/`replay` auto-select selfhost when a toolchain artifact is configured (guarded by bad-artifact bootstrap tests)
  - [x] covered now: CI runs `scripts/selfhost_strict_smoke.sh` (native + WASI strict selfhost smoke path), including `run`/`replay`
  - [x] covered now: CI runs `scripts/selfhost_strict_golden.sh` over `tests/spec/coreform/*` and all `tests/spec/pkg_*` fixtures, including native+WASI strict `run`/`replay` parity checks

---

## P1: Self-Hosted Toolchain Completeness (.gc)
- [x] Fix self-host printer multiline-map emission so `fmt` output is parseable and `fmt --check` is idempotent.
  - fixed in `selfhost/printer.gc` (`fmt-map-entry-multiline`) and rebuilt `selfhost/toolchain.gc`
  - validated by `cargo test --workspace` and `cargo clippy --workspace --all-targets -- -D warnings`
- [ ] Finalize self-host parser/canon/printer/hash as canonical source of truth.
- [ ] Implement self-host Stage-1 transform pipeline fully in `.gc` (CoreForm -> CoreForm validated transforms).
- [ ] Implement self-host type/effect checker in `.gc` and wire to `core/obligation::typecheck`.
- [ ] Implement self-host optimizer pipeline in `.gc` and wire to `core/obligation::translation-validation`.
- [ ] Implement self-host patch schema validation + apply pipeline in `.gc`.
- [ ] Ensure all artifacts/evidence emitted by self-hosted paths are byte-for-byte deterministic.

Acceptance gate:
- [ ] Native and WASM runs produce identical hashes/evidence between Rust fallback and `.gc` self-host path on conformance suites.
  - progress: added WASI `cli_run_replay_engine.rs` parity tests for `run`/`replay` (`rust` vs `selfhost`) plus selfhost parse-error surfacing.
  - progress: native `cli_run_replay_engine.rs` now enforces deterministic `.gclog` parity between `--engine rust` and `--engine selfhost`.
  - progress: WASI `cli_run_replay_engine.rs` now enforces deterministic `.gclog` parity between `--engine rust` and `--engine selfhost`.
  - progress: native + WASI parity tests now cover denied-effect programs (`sys/time::now` with deny-by-default caps), asserting exit-code/output/log/replay parity across `rust` and `selfhost`.
  - progress: fixed run/replay selfhost hash divergence root cause by parsing in a bootstrap env and evaluating in a fresh prelude-only env (prevents selfhost tool bindings from contaminating continuation/request hashes).
  - progress: `gc_wasm` now has explicit selfhost effect-runtime parity coverage (`wasm_runtime_selfhost_hashes_match_native_effect_runner_entry`) matching payload/continuation/request/response hashes against native runner for the same selfhost-parsed forms.
  - progress: strict golden CI now includes native+WASI selfhost parity checks for `run`/`replay` against Rust baseline outputs and host-local `.gclog` parity (`rust` vs `selfhost`).

---

## P2: CLI Cutover to `.gc` Command Handlers
- [ ] Define CLI command contract interface in `.gc` (`core/cli::*`) for all stable subcommands.
- [ ] Route each subcommand through `.gc` handlers:
  - [ ] `fmt`
  - [ ] `eval`
  - [x] `explain`
  - [x] `run`
  - [x] `replay`
  - [ ] `test`
  - [ ] `typecheck`
  - [ ] `optimize`
  - [ ] `pack`
  - [ ] `apply-patch`
  - [ ] `store/*`, `refs/*`, `vcs/*`, `pkg/*`, `policy/*`, `gc/*`
    - progress: `vcs hash` now routes through `.gc` (`selfhost/tool::hash-src-with-kind`) by default (native + WASI), with `--engine rust` available for parity checks.
- [ ] Keep Rust CLI as thin argument parser + host bridge only.
- [ ] Remove duplicated Rust command logic once parity is proven.

Acceptance gate:
- [ ] CLI golden tests show output/log/evidence parity for old vs self-host route, then old route removed.

---

## P3: GenesisGraph + GenesisPkg Full Self-Hosted Core
- [ ] Implement graph object constructors/validators in `.gc`:
  - [ ] `:vcs/snapshot`
  - [ ] `:vcs/patch`
  - [ ] `:vcs/commit`
  - [ ] `:vcs/evidence`
  - [ ] `:vcs/attestation`
  - [ ] `:vcs/conflict`
- [ ] Implement reachability closure engine in `.gc` used by export/publish/install/gc.
- [ ] Implement lock generator/resolver in `.gc` per `docs/LOCK_GENERATOR_RULESET_v0.1.md`.
- [ ] Implement `.gpk` import/export planner in `.gc` (shallow + full history modes).
- [ ] Implement ref-policy gating in `.gc` per `docs/POLICY_DEFAULTS_v0.1.md`.
- [ ] Implement local GC planning in `.gc` per `docs/GARBAGE_COLLECTION_RULES_v0.1.md`.

Acceptance gate:
- [ ] End-to-end workspace flow (`pkg add/lock/install/test/publish/export/import`) executes through `.gc` plans and passes replay checks.

---

## P4: Remove Rust Semantic Bootstrap (Hard Cutover)
- [ ] Move legacy semantic implementations to `/deprecated`:
  - [ ] Rust parser/printer/hash frontend fallbacks
  - [ ] Rust-side toolchain command logic replaced by `.gc` handlers
  - [ ] Any Rust-only pipeline no longer used by default runtime path
- [ ] Add build profile `selfhost-strict` that excludes deprecated semantic crates/modules.
- [ ] Make `selfhost-strict` the default CI profile.
- [ ] Keep a compatibility profile only for historical comparison tests.

Acceptance gate:
- [ ] `cargo test` in `selfhost-strict` passes without invoking deprecated Rust semantics.
- [ ] GenesisCode can rebuild its own toolchain artifacts from `.gc` sources only (host bridge allowed).

---

## P5: Runtime Host Decomposition (Rust No Longer Language-Critical)
- [ ] Publish stable host ABI spec (`docs/spec/HOST_ABI.md`) for capabilities and runtime embedding.
- [ ] Restrict Rust responsibilities to:
  - [ ] capability IO adapters (fs/net/time/window/gpu/input/audio)
  - [ ] store/ref physical persistence drivers
  - [ ] wasm/native embedding and process shell
- [ ] Add alternative host conformance harness (second host implementation can be minimal) proving ABI portability.

Acceptance gate:
- [ ] Language/toolchain `.gc` code runs unchanged on at least two host implementations via same ABI.

---

## P6: Post-Cutover `.gc`-First Acceleration

### P6.1 Performance and Optimization
- [ ] Self-host profiling toolkit in `.gc` (deterministic trace/event artifacts).
- [ ] Self-host optimizer improvements (rewrite sets, inliner heuristics, allocation reduction).
- [ ] Incremental build graph and memoized artifact reuse in `.gc`.
- [ ] Faster package resolution and reachability traversal in `.gc`.

### P6.2 Missing Language/Platform Features in `.gc`
- [ ] Complete graphics library in `.gc` for production 2D + 3D authoring/runtime APIs.
- [ ] Integrate WebGPU capability surface and deterministic replay policy boundaries.
- [ ] Complete GUI editor in `.gc` with native GenesisGraph/GenesisPkg workflows.
- [ ] Ensure editor uses Genesis VCS operations directly (no git dependency paths).

### P6.3 AI/Agent Developer Experience
- [ ] Publish `docs/write_genesisCode_skill.md` as the canonical agent playbook after strict self-host cutover.
- [ ] Add agent-safe refactor protocol templates and evidence requirements for autonomous patching.

Acceptance gate:
- [ ] New major features are delivered in `.gc` first, with Rust host changes only when ABI/capability adapters are required.

---

## Immediate Next 10 Pushes (Highest ROI Sequence)
- [x] 1) Add `selfhost-only` hard mode + CI gate.
  - native + WASI CLIs now support `--selfhost-only` (also `GENESIS_SELFHOST_ONLY=1|true|yes|on`)
  - strict mode requires `--engine selfhost`, forbids non-`artifact-only` bootstrap, and rejects non-routed commands
  - CI gate coverage added via CLI tests: `cli_selfhost_only.rs` (native + WASI)
- [x] 2) Route `fmt/eval/optimize/typecheck` through `.gc` command handlers by default.
  - [x] `typecheck` now uses selfhost parse/canonicalize in native CLI when `--selfhost-only` is enabled
  - [x] `typecheck` now auto-prefers selfhost frontend when a toolchain artifact is configured/present (without `--selfhost-only`)
  - [x] `fmt`, `eval`, and `optimize` now auto-select `selfhost` engine when a toolchain artifact is configured/present (explicit `--engine rust` still supported)
  - [x] WASI `fmt`/`eval` now match native auto-engine selection behavior
  - [x] native + WASI now also discover workspace fallback artifact `selfhost/toolchain.gc` (in addition to `./.genesis/selfhost/toolchain.gc`)
  - [x] selfhost path is now the unconditional default for this command set (explicit `--engine rust` still supported for parity checks)
- [ ] 3) Remove Rust fallback path for parser/printer/hash in default profile.
- [x] 4) Route `test/apply-patch/pack` through `.gc`.
  - [x] `test`, `apply-patch`, and `pack` now have strict-mode selfhost frontend routing in native CLI
  - [x] `test`, `apply-patch`, and `pack` now auto-prefer selfhost frontend when a toolchain artifact is configured/present
  - [x] selfhost frontend is now the unconditional default for this command set in CLI frontend resolution
- [ ] 5) Move replaced Rust semantic modules to `/deprecated`.
- [ ] 6) Enable `selfhost-strict` profile in CI as required.
- [ ] 7) Complete `.gc` lock/resolver and `.gpk` planner cutover.
- [ ] 8) Complete `.gc` ref-policy gate enforcement cutover.
  - [x] started routing `vcs/*`: `vcs hash` now executes through selfhost `.gc` tool handlers by default (native + WASI), with explicit `--engine rust` parity override
- [ ] 9) Add host ABI conformance harness.
- [ ] 10) Start P6 optimization wave in `.gc` (profiling + incremental build graph).
