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
  - [x] covered now: dashboard command-coverage tables include `policy/*` in both native and WASI markdown outputs (guarded by dedicated dashboard tests) to keep cutover percentages aligned with current command surface.
  - [x] covered now: `vcs hash` strict mode executes through selfhost tool handlers in native + WASI CLI tests
  - [x] covered now: `fmt`, `eval`, `explain`, `optimize`, `run`, `replay`, `test`, `pack`, `typecheck`, `apply-patch`, `selfhost-dashboard`, `vcs hash` strict-mode routing in WASI CLI tests
  - [x] covered now: `explain` strict-mode engine gating in WASI CLI tests
  - [x] covered now: native + WASI `fmt` auto-select selfhost via workspace fallback artifact `selfhost/toolchain.gc`
  - [x] covered now: native + WASI `run`/`replay` auto-select selfhost when a toolchain artifact is configured (guarded by bad-artifact bootstrap tests)
  - [x] covered now: CI runs `scripts/selfhost_strict_smoke.sh` (native + WASI strict selfhost smoke path), including `run`/`replay`
  - [x] covered now: strict smoke executes `explain` and `typecheck` under `--selfhost-only` for native + WASI CLIs with selfhost artifact bootstrap.
  - [x] covered now: strict smoke exercises non-frontend command groups under `--selfhost-only` (`store`, `refs`, `pkg`, `policy`, `sync`, `gc`, and effectful `vcs`) on native + WASI.
  - [x] covered now: CI runs `scripts/selfhost_strict_golden.sh` over `tests/spec/coreform/*` and all `tests/spec/pkg_*` fixtures, including native+WASI strict `run`/`replay` parity checks
  - [x] covered now: `gc_obligations` enforces `GENESIS_SELFHOST_ONLY` at library boundaries (`parse/canonicalize` + module loading), so strict mode also blocks Rust frontend fallback outside CLI command routing.
  - [x] covered now: CI default job env sets `GENESIS_ALLOW_RUST_ENGINE=0` and runs `scripts/selfhost_default_profile_guard.sh`, enforcing rust-engine rejection in the default selfhost profile for native + WASI CLIs.
  - [x] covered now: CI runs `scripts/check_rust_engine_compat.sh`, requiring explicit `GENESIS_ALLOW_RUST_ENGINE=1` opt-in in tests/scripts wherever `--engine rust` appears.

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
  - progress: added WASI `cli_eval_engine.rs` parity tests for `eval` (`rust` vs `selfhost`) plus selfhost parse-error surfacing.
  - progress: added WASI `cli_explain_engine.rs` parity tests for `explain` (`rust` vs `selfhost`) plus selfhost parse-error surfacing.
  - progress: added WASI `cli_fmt_engine.rs` parity tests for `fmt` (`rust` vs `selfhost`) including `--check` exit-code parity.
  - progress: added WASI `cli_typecheck_engine.rs` parity tests for `typecheck` (`rust` vs `selfhost`) on `pkg_basic`.
  - progress: native `cli_run_replay_engine.rs` now enforces deterministic `.gclog` parity between `--engine rust` and `--engine selfhost`.
  - progress: WASI `cli_run_replay_engine.rs` now enforces deterministic `.gclog` parity between `--engine rust` and `--engine selfhost`.
  - progress: native + WASI parity tests now cover denied-effect programs (`sys/time::now` with deny-by-default caps), asserting exit-code/output/log/replay parity across `rust` and `selfhost`.
  - progress: fixed run/replay selfhost hash divergence root cause by parsing in a bootstrap env and evaluating in a fresh prelude-only env (prevents selfhost tool bindings from contaminating continuation/request hashes).
  - progress: applied the same parse-bootstrap/eval-fresh env split to native `eval` and `explain` selfhost routes; `cli_explain_engine` now enforces `:contract-id` parity across `rust` and `selfhost`.
  - progress: added WASI `cli_explain_engine.rs` parity coverage for `explain` (`rust` vs `selfhost`) plus selfhost parse-error surfacing.
  - progress: added WASI `cli_optimize_engine.rs` parity coverage for `optimize` (`rust` vs `selfhost`) plus stage-gate and selfhost parse-error coverage.
  - progress: `gc_wasm` now has explicit selfhost effect-runtime parity coverage (`wasm_runtime_selfhost_hashes_match_native_effect_runner_entry`) matching payload/continuation/request/response hashes against native runner for the same selfhost-parsed forms.
  - progress: strict golden CI now includes native+WASI selfhost parity checks for `run`/`replay` against Rust baseline outputs and host-local `.gclog` parity (`rust` vs `selfhost`).
  - progress: strict smoke/golden scripts now enforce WASI `eval` parity against both WASI rust baseline and native rust baseline (not just strict selfhost output), tightening cross-engine/cross-host guardrails.
  - progress: added native + WASI `cli_pack_test_engine.rs` parity suites proving `pack` package artifacts and `test` acceptance artifacts are identical between rust and selfhost frontend paths on `pkg_basic`.
  - progress: added native `cli_typecheck_apply_patch_engine.rs` parity tests proving `typecheck` report output and `apply-patch` report/acceptance/package artifacts are identical between rust and selfhost frontend paths on `pkg_basic`.
  - progress: strict smoke script now enforces native `typecheck` parity (`rust` baseline vs strict selfhost) in addition to command-success checks.
  - progress: added WASI `cli_vcs_hash.rs` parity coverage for rust/selfhost engines on both term and module inputs.
  - progress: fixed a WASI/native divergence in `vcs hash` rust-engine parse precedence (now term-first, matching native CLI + selfhost handler) to eliminate cross-host hash-kind drift.
  - progress: strict smoke/golden scripts now enforce `vcs hash` parity for native and WASI rust baselines against strict selfhost outputs, and native-vs-WASI rust baseline parity.
  - progress: native `vcs hash` JSON envelope now matches WASI on v0.2 schema (`kind`, `in`, `hash_format`), while retaining legacy `input` for compatibility; native+WASI `cli_vcs_hash` tests now lock this schema and rust/selfhost parity.
  - progress: strict smoke script now enforces native+WASI parity for `pack` and `test` (rust baseline vs strict selfhost, plus native-vs-WASI rust baseline parity for package and acceptance artifacts).
  - progress: strict golden package sweep now enforces rust-vs-selfhost artifact parity for `pack` and `test` on all passing `tests/spec/pkg_*` fixtures, and confirms both engines fail expected `pkg_fail_*` fixtures.
  - progress: strict golden package sweep now enforces native rust-vs-selfhost `typecheck` parity (exit code + full output) across all `tests/spec/pkg_*` fixtures.
  - progress: strict golden now enforces rust-vs-selfhost `apply-patch` artifact parity on duplicated `pkg_basic` fixtures for native + WASI, along with WASI `pkg_basic` pack/test/typecheck parity, strict selfhost dashboard execution (including markdown row checks for `policy/*`), and per-coreform-fixture WASI optimize parity (rust baseline vs strict selfhost, plus native-vs-WASI rust baseline).
  - progress: added WASI `cli_apply_patch_engine.rs` parity coverage proving `apply-patch` artifact hashes (`patch/report/acceptance/package`) are identical across default, explicit `--selfhost-artifact`, and strict `--selfhost-only` frontend paths on `pkg_basic`.
  - progress: `gc_obligations` default library paths (`pack`, `test_package_with_step_limit`, `package_artifact_hash`) now default to selfhost frontend with env/cwd/workspace artifact resolution, reducing implicit Rust-frontend fallback in non-CLI call paths.
  - progress: `gc_patches::apply_patch_with_step_limit` default path now uses the shared selfhost default frontend (via `gc_obligations::default_coreform_frontend`) instead of defaulting to Rust frontend.
  - progress: `gc_obligations::verify_package_with_policy` now validates module/dependency hashes via the selfhost default frontend path instead of a hard-coded Rust frontend.
  - progress: remaining internal `gc_obligations` wrappers (`run_one_test`, `eval_package_once`, `eval_dependencies`) now default through `default_coreform_frontend()`, removing additional implicit Rust-frontend fallback from translation-validation and dependency-eval paths.
  - progress: native + WASI CLIs now disable `--engine rust` by default and require explicit compatibility opt-in (`GENESIS_ALLOW_RUST_ENGINE=1`), making selfhost the default profile path while preserving parity tooling.
  - progress: `gc_obligations` frontend guards now also require `GENESIS_ALLOW_RUST_ENGINE=1` for `CoreformFrontend::Rust`, so non-CLI library entry points match default-profile selfhost semantics.
  - progress: parity tests and strict smoke/golden scripts now opt into rust-engine compatibility mode explicitly, and new native+WASI tests assert rust-engine rejection without the compatibility env.

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
    - progress: `--selfhost-only` now permits command groups `store`, `refs`, `pkg`, `policy`, `sync`, and `gc` in native + WASI CLIs; regression coverage executes these groups end-to-end under strict mode (`sync` asserted by strict-gate tests + smoke on both CLIs).
    - progress: added concrete `policy` command surface in native + WASI CLIs (`policy list`, `policy show`, `policy set-default`) with deterministic local config management and schema-validated policy artifact resolution from the content-addressed store.
    - progress: added missing WASI `explain` command surface with native-parity engine routing and output schema (`genesis/explain-v0.2`), including selfhost term parsing for `--contract`/`--msg`.
    - progress: added missing WASI `typecheck` command surface with native-parity selfhost frontend routing and output schema (`genesis/typecheck-v0.2`).
    - progress: added missing WASI `optimize` command surface with native-parity engine routing, stage1/stage2 gate semantics, wasm emission, and JSON envelope (`genesis/optimize-v0.2`).
    - progress: added missing WASI `pkg publish` command surface with native-parity runtime routing (`core/pkg::publish` effect op), including obligation-failure exit semantics and commit-hash stdout behavior parity.
    - progress: added missing WASI `apply-patch` command surface with native-parity frontend routing and output schema (`genesis/apply-patch-v0.2`).
    - progress: added executable WASI `apply-patch` conformance coverage for JSON schema and content-addressed artifact emission (`patch/report/acceptance/package` artifacts in `.genesis/store`).
    - progress: added missing WASI `selfhost-dashboard` command surface with native-parity artifact/markdown emission and output schema (`genesis/selfhost-dashboard-v0.2`).
    - progress: strict selfhost smoke now explicitly validates `pkg publish --help` availability on both native and WASI CLIs to prevent command-surface regressions.
    - progress: added WASI `cli_spec_surface.rs` to lock top-level/subcommand help surfaces (including `explain` and `pkg publish`) and prevent future command-surface regressions.
    - progress: `--selfhost-only` now also permits effectful `vcs/*` in native + WASI CLIs (with `vcs hash` still engine-validated), with regression coverage for strict-gate acceptance and strict-smoke execution of `vcs log`.
    - progress: added executable WASI `vcs` effectful conformance tests covering `vcs diff/apply` snapshot roundtrip and `vcs log` commit-history traversal with content-addressed artifacts.
    - progress: added executable WASI conformance coverage for `vcs blame` and `vcs why`, including commit-attribution for unchanged vs changed symbols across commit ancestry and provenance message/why extraction.
    - progress: added executable WASI semantic-merge coverage for `vcs merge3` and `vcs resolve-conflict`, including disjoint-op contract merges, conflict artifact emission, left-strategy resolution, and patch/apply equivalence checks.
    - progress: added executable native + WASI `sync` conformance tests covering `sync push`/`sync pull` roundtrips with policy-gated remote `--set-ref` CAS, closure transfer verification, local/remote ref updates, and idempotent second-pull behavior.
    - progress: expanded native + WASI `sync` failure-path conformance to cover policy preflight hard-fail before any remote upload and `sync pull` local-ref conflict handling (`--force` required to overwrite divergent local heads).
    - progress: strict smoke now enforces WASI `optimize` parity for rust baseline vs strict selfhost output, and native-vs-WASI rust optimize parity.
    - progress: strict smoke now executes WASI `apply-patch` under `--selfhost-only` with a real patch artifact (`tests/spec/pkg_basic/pure.gcpatch`) to guard command-surface and runtime routing.
    - progress: strict smoke now executes WASI `selfhost-dashboard` under `--selfhost-only` and verifies artifact+markdown path emission in the strict path.
    - progress: strict smoke now enforces `apply-patch` artifact parity for native+WASI (rust baseline vs strict selfhost, plus native-vs-WASI rust baseline parity) using isolated package copies to avoid mutation coupling.
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
  - progress: added `pkg lock --strict` (native + WASI) and runner-level strict validation in `core/pkg::lock` for commit/snapshot/evidence integrity; strict lock now fails on obligation-bearing commits missing evidence.
  - progress: added executable WASI parity coverage for lock update semantics (`pin` vs tracked `ref`): pinned `commit:<hash>` dependencies remain stable while tracked `refs/...` dependencies advance deterministically on `pkg update`.
  - progress: added executable WASI roundtrip coverage for `pkg init/add(lock by snapshot selector)/lock/install --frozen/verify`, including deterministic repeated-lock hash behavior and install failure on missing store artifacts.
- [ ] Implement `.gpk` import/export planner in `.gc` (shallow + full history modes).
  - progress: upgraded `core/gpk::export` planner controls in capability runtime with explicit closure policy knobs:
    - root selectors now accept hashes plus `refs/...` / `ref:refs/...` (resolved through refs DB, policy-gated capability context)
    - `--include-evidence {required|all|none}` now maps to deterministic full-history evidence inclusion behavior (root-only required vs full vs none)
    - `--include-deps {none|locked|all}` now controls snapshot dependency-edge closure traversal (including lock-style `:deps` pointers)
    - native + WASI CLI surfaces now expose `--root` alias and include flags, with parity wiring into `core/gpk::export` payloads
    - regression coverage added for evidence exclusion, ref-root export, and dependency-closure mode differences (`none` vs `locked`)
    - added WASI parity coverage for shallow export dependency closure policy (`--include-deps none|locked`), verifying import/store reachability differences match native behavior.
    - added WASI parity coverage for full export evidence policy (`--include-evidence none` from `refs/...` root), verifying patch/snapshot closure inclusion while excluding evidence/data artifacts on import.
    - added WASI parity coverage for full export/import closure with embedded refs, verifying commit+snapshot+patch+evidence transitive inclusion and `.gpk` v2 ref section semantics match native behavior.
    - added WASI shallow-share roundtrip coverage (`snapshot -> export .gpk -> import -> lock/install --frozen`) validating deterministic artifact identity across source/destination stores and install-time lock workflow.
    - added WASI snapshot-kind closure coverage for shallow bundles (`:contract`, `:module`, `:workspace` roots), verifying transitive inclusion of override handlers/defs/module snapshots on import.
- [ ] Implement ref-policy gating in `.gc` per `docs/POLICY_DEFAULTS_v0.1.md`.
  - progress: moved `pkg publish` policy preflight/gating out of native CLI into capability runtime op `core/pkg::publish` (effect-runner path), so publish gate decisions are now enforced at the host capability boundary with deterministic logs.
  - progress: moved `pkg import --set-ref` local ref mutation chain out of CLI continuation logic into `core/gpk::import` runtime handling:
    - import payload now carries `:set-refs` entries (name/hash/policy/expected-old), applied in deterministic sorted order
    - ref updates now reuse centralized local refs policy gate logic in runtime (`core/refs` gate path), removing duplicated CLI-side orchestration
    - import ref updates remain policy-gated and covered by regression tests, including operation under caps that allow `core/gpk::import` without separately exposing `core/refs::set` to user programs
    - added explicit rejection coverage (native + WASI) proving `pkg import --set-ref` hard-fails and preserves ref state when commit artifacts do not satisfy policy-required obligations for protected refs
    - local refs storage now supports atomic batch updates (`RefsDb::set_many`), and `core/gpk::import` validates all `:set-refs` policy gates before committing refs in one write, preventing partial ref advancement on multi-ref failures
    - native + WASI `pkg import --set-ref` now support optimistic compare-and-set syntax (`<ref>=<hash|nil>@<expected-old|nil>`) with strict validation and regression coverage for success/failure paths
    - `core/sync::push` now performs local policy-gate preflight for remote `:set-refs` before upload/remote mutation, and runtime payload parsing now rejects duplicate/invalid `:set-refs` entries deterministically.
    - `core/refs::delete` now uses the shared local ref policy gate path (same validator as `core/refs::set`), with dedicated runner tests for frozen/no-class/CAS-conflict/success behavior.
- [ ] Implement local GC planning in `.gc` per `docs/GARBAGE_COLLECTION_RULES_v0.1.md`.
  - progress: expanded executable GC conformance coverage in CLI tests for `pin`/`unpin` lifecycle and `keep_refs` retention semantics under `--no-refs` root scanning, validating policy-driven root preservation behavior end-to-end.
  - progress: added tag-archival coverage for GC refs roots (`refs/tags/*`), asserting commit closure retention (patch/snapshot/evidence/attestation) while pruning unrelated artifacts.
  - progress: added WASI CLI parity for `gc` command surface (`plan`/`run`/`pin`/`unpin`/`purge`) and enabled executable WASI GC regressions for plan/run pruning, quarantine+purge, pin lifecycle, and pinned-ref retention under `--no-refs`.

Acceptance gate:
- [ ] End-to-end workspace flow (`pkg add/lock/install/test/publish/export/import`) executes through `.gc` plans and passes replay checks.

---

## P4: Remove Rust Semantic Bootstrap (Hard Cutover)
- [ ] Move legacy semantic implementations to `/deprecated`:
  - [ ] Rust parser/printer/hash frontend fallbacks
  - [ ] Rust-side toolchain command logic replaced by `.gc` handlers
  - [ ] Any Rust-only pipeline no longer used by default runtime path
- [ ] Add build profile `selfhost-strict` that excludes deprecated semantic crates/modules.
- [x] Make `selfhost-strict` the default CI profile.
  - progress: workspace now defines `[profile.selfhost-strict]` in `Cargo.toml` (inherits `test`, deterministic-oriented settings), and CI `Test` runs `cargo test --workspace --profile selfhost-strict`.
  - progress: CI now enforces strict default selfhost behavior with `GENESIS_ALLOW_RUST_ENGINE=0` and an explicit default-profile guard step (`scripts/selfhost_default_profile_guard.sh`).
- [ ] Keep a compatibility profile only for historical comparison tests.

Acceptance gate:
- [ ] `cargo test` in `selfhost-strict` passes without invoking deprecated Rust semantics.
  - progress: validated with `GENESIS_ALLOW_RUST_ENGINE=0 cargo test --workspace --profile selfhost-strict` (full workspace pass) plus compat-usage guardrails.
- [ ] GenesisCode can rebuild its own toolchain artifacts from `.gc` sources only (host bridge allowed).

---

## P5: Runtime Host Decomposition (Rust No Longer Language-Critical)
- [x] Publish stable host ABI spec (`docs/spec/HOST_ABI.md`) for capabilities and runtime embedding.
- [ ] Restrict Rust responsibilities to:
  - [ ] capability IO adapters (fs/net/time/window/gpu/input/audio)
  - [ ] store/ref physical persistence drivers
  - [ ] wasm/native embedding and process shell
- [ ] Add alternative host conformance harness (second host implementation can be minimal) proving ABI portability.
  - progress: added CI host ABI surface conformance guard (`scripts/check_host_abi_conformance.sh`) that enforces exact parity between documented ABI ops (`docs/spec/HOST_ABI.md`) and `gc_effects` dispatch surface.
  - progress: added executable runner conformance tests (`crates/gc_effects/tests/host_abi_surface.rs`) that iterate every documented host ABI op and fail if any op falls through to `core/caps/unknown-op`.

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
- [x] 3) Remove Rust fallback path for parser/printer/hash in default profile.
  - [x] default profile now rejects `--engine rust` unless `GENESIS_ALLOW_RUST_ENGINE=1` is set (native + WASI).
  - [x] default profile now rejects `CoreformFrontend::Rust` in obligations library entry points unless `GENESIS_ALLOW_RUST_ENGINE=1` is set.
  - [x] parity suites/scripts now declare rust-engine compatibility mode explicitly instead of relying on default fallback behavior.
- [x] 4) Route `test/apply-patch/pack` through `.gc`.
  - [x] `test`, `apply-patch`, and `pack` now have strict-mode selfhost frontend routing in native CLI
  - [x] `test`, `apply-patch`, and `pack` now auto-prefer selfhost frontend when a toolchain artifact is configured/present
  - [x] selfhost frontend is now the unconditional default for this command set in CLI frontend resolution
- [ ] 5) Move replaced Rust semantic modules to `/deprecated`.
- [x] 6) Enable `selfhost-strict` profile in CI as required.
  - [x] CI `Test` now executes `cargo test --workspace --profile selfhost-strict`.
  - [x] CI job now sets `GENESIS_ALLOW_RUST_ENGINE=0` by default and runs `scripts/selfhost_default_profile_guard.sh`.
  - [x] CI now also runs `scripts/check_rust_engine_compat.sh` to prevent implicit rust-engine fallback regressions in test/script surfaces.
  - [x] strict smoke/golden parity paths explicitly opt into compatibility mode (`GENESIS_ALLOW_RUST_ENGINE=1`) where Rust baseline comparison is required.
- [ ] 7) Complete `.gc` lock/resolver and `.gpk` planner cutover.
  - [x] added `pkg lock --strict` surface (native + WASI) and strict resolver checks in `core/pkg::lock`, with regression tests for missing-evidence failure paths.
  - [x] expanded strict lock integrity checks: `core/pkg::lock --strict` now validates locked selector invariants (`source_selector`/`resolved_ref`) and commit closure artifacts (`base`/`patch`/`result` + commit hash integrity), with regression coverage for missing patch closure.
  - [x] aligned integrity checks across `core/pkg::install --strict` and `core/pkg::verify`: commit closure validation now includes `base`/`patch`/`result`, evidence and attestation artifact/schema checks, and obligation-without-evidence rejection, with native + WASI CLI regressions for missing patch closure in lock/install/verify paths.
  - [x] added `.gpk` export closure controls (ref-root resolution, evidence/dependency inclusion modes) and end-to-end CLI tests proving deterministic inclusion/exclusion behavior across shallow/full export modes.
- [x] 8) Complete `.gc` ref-policy gate enforcement cutover.
  - [x] started routing `vcs/*`: `vcs hash` now executes through selfhost `.gc` tool handlers by default (native + WASI), with explicit `--engine rust` parity override
  - [x] `pkg publish` now delegates policy preflight + ref-class obligation checks to runtime capability op `core/pkg::publish` (instead of native CLI-local preflight), preserving `EX_OBLIGATIONS` on publish gate failures.
  - [x] added executable WASI `pkg publish` conformance coverage for policy-gated failure/success behavior against file-backed remotes (missing evidence rejected; valid evidence advances remote ref deterministically).
  - [x] `pkg import --set-ref` now delegates local refs updates to runtime capability handling in `core/gpk::import` (policy-gated via shared refs gate logic), eliminating CLI continuation-based `core/refs::set` orchestration.
  - [x] added runtime-gated import failure-path parity tests (native + WASI) for policy rejection and ref non-advancement, tightening ref-policy gate conformance coverage.
  - [x] hardened multi-ref import semantics with atomic local refs commit + native/WASI atomicity tests (no partial ref updates when one target fails policy).
  - [x] added native/WASI optimistic CAS support for `pkg import --set-ref` (`@<expected-old|nil>`) with strict parser validation and compare-and-set regression coverage.
  - [x] hardened `sync push --set-ref` parsing/gating: contract-style refs containing `::` now parse correctly, duplicate targets are rejected, and runtime preflight enforces policy obligations before any remote upload/ref mutation.
  - [x] removed residual sync parser ambiguity for contract refs without expected-old by standardizing sync set-ref CAS syntax to `@<expected-old|nil>` and adding parser regressions for both with/without CAS on `::` ref names.
  - [x] removed duplicated delete-side policy logic by routing `core/refs::delete` through the shared refs gate validator; added conformance tests for frozen/no-class/CAS success/failure paths.
- [x] 9) Add host ABI conformance harness.
  - [x] added `docs/spec/HOST_ABI.md` with normative v0.2 op surface and CI-enforced parity against `gc_effects` dispatch via `scripts/check_host_abi_conformance.sh`.
  - [x] added runtime host ABI surface tests (`crates/gc_effects/tests/host_abi_surface.rs`) to verify documented ops are recognized by the runner dispatch path.
- [ ] 10) Start P6 optimization wave in `.gc` (profiling + incremental build graph).
