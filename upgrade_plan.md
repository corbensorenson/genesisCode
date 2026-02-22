# GenesisCode Upgrade Plan - Red-Team Backlog (Unresolved Only)

Last updated: 2026-02-22

This file tracks unresolved blockers only. Completed work belongs in git history and release notes.

Open checklist items: 12

## P0 - Immediate blockers

- [ ] P0.1 Remove artifact bootstrap deadlock in production selfhost flow.
  - Evidence:
    - Production selfhost artifact generation requires an existing artifact seed in `artifact-only` mode:
      `crates/gc_cli_driver/src/cmd_selfhost.rs` (`selfhost-artifact requires an existing toolchain artifact ...`).
    - Missing-artifact probe fails fast:
      `target/debug/genesis --selfhost-artifact /tmp/genesis_missing_toolchain.gc selfhost-artifact --out /tmp/genesis_selfhost_artifact_probe.gc`
      -> `coreform frontend: selfhost artifact file does not exist`.
  - Why this blocks:
    - A corrupted/missing committed artifact can stall local recovery and cutover confidence.
    - "Selfhost-first" operation remains dependent on external bootstrap rescue paths.
  - Acceptance:
    - Add deterministic, fail-closed recovery path to regenerate `selfhost/toolchain.gc` from manifest sources without requiring a pre-existing artifact file.
    - Keep production runtime execution in `artifact-only`; recovery path must be explicit and auditable.
    - Add regression tests covering missing/corrupt artifact recovery.

## P1 - High-impact self-host and AI-first hardening

- [ ] P1.1 Bring `dev-fast` wall time back to an actual fast loop.
  - Evidence:
    - `.genesis/perf/upgrade_plan_health_profile_report.json` records `elapsed_ms=263873` for `profile=dev-fast`.
    - User-facing iteration objective is rapid agent loops; 4+ minute profile runs are too slow for tight edit/test cycles.
  - Acceptance:
    - Introduce a true inner-loop profile (`<=120000ms` on warm cache) with explicit gate contract.
    - Keep current heavy profile as prepush/release lane, not default local fast loop.
    - Publish profile/runtime matrix with expected latency tiers.

- [ ] P1.2 Eliminate recurring ENOSPC failures from gate target-dir sprawl.
  - Evidence:
    - Repeated runs accumulate large build caches; observed `.genesis/build` at multi-GB scale and prior ENOSPC failures during gate runs.
    - Many scripts allocate independent target dirs (`scripts/*` with `genesis_configure_cargo_target_dir`), increasing cache duplication.
  - Acceptance:
    - Add deterministic cache lifecycle policy (TTL/LRU/size cap) for `.genesis/build/*`.
    - Add proactive cleanup integration into health/profile runners (not manual-only remediation).
    - Gate must fail with clear remediation only after automated reclaim attempts.

- [ ] P1.3 Put a hard SLO/budget on runtime backend matrix checks.
  - Evidence:
    - `.genesis/perf/runtime_backend_feature_matrix_report.json` shows `elapsed_ms=306863`.
    - `scripts/check_runtime_backend_feature_matrix.sh` defaults budget to `0` (disabled).
  - Acceptance:
    - Enable fail-closed runtime budget with historical regression tracking.
    - Split matrix into deterministic shards/cached phases to reduce tail latency.
    - Add explicit pass/fail thresholds for CI and local profiles.

- [ ] P1.4 Reduce production CLI parse/help surface gate tail latency.
  - Evidence:
    - `.genesis/perf/production_cli_parse_surface_report.json` records `elapsed_ms=209265`.
    - Gate uses repeated `cargo run --release` invocations in `scripts/check_production_cli_parse_surface.sh` and `scripts/check_production_cli_help_surface.sh`.
  - Acceptance:
    - Reuse built release binaries inside gate execution (single build, multi-check invocation).
    - Preserve parse/help surface strictness while cutting steady-state runtime.
    - Add timing budget guard with regression history.

- [ ] P1.5 Make device-runtime requirement explicit for GPU compute release lanes.
  - Evidence:
    - Default GPU backend fallback policy is `allow-fallback` in `crates/gc_effects/src/runner_gpu_backend_policy.rs`.
    - This permits silent fallback away from requested device backends.
  - Why this matters:
    - AI-generated high-performance compute workloads need predictable backend semantics, not opportunistic fallback.
  - Acceptance:
    - Release/full profiles default to `require-device` for compute-critical ops.
    - Fallback must be opt-in and explicitly surfaced in policy/evidence artifacts.
    - Add tests proving fail-closed behavior in strict release profiles.

- [ ] P1.6 Add executable conformance for `write_genesisCode_skill` quality.
  - Evidence:
    - Current skill gates (`scripts/check_genesiscode_authoring_skill.sh`, `scripts/check_write_genesiscode_skill_pack.sh`) validate structure/references, not generated-code competence.
  - Acceptance:
    - Add benchmark suite where agent-authored GenesisCode tasks (service, game loop, GPU compute, package workflow) must compile/run/replay deterministically.
    - Publish scoring rubric and minimum pass thresholds.
    - Wire into health/release gates.

- [ ] P1.7 Correct overclaim on bootstrap independence and keep matrix claim-evidence aligned.
  - Evidence:
    - `feature_matrix.md` currently marks "Fully self-hosted toolchain with zero bootstrap-language dependency" as `✅`, while P0.1 evidence shows recovery still artifact-seed dependent.
  - Acceptance:
    - Update capability status/notes to reflect current state (`⚠️` until P0.1 is closed).
    - Keep `feature_matrix.md`, evidence ledger, and plan IDs in strict sync.

## P2 - Strategic completeness for "agent can build anything" scope

- [ ] P2.1 Expand `gcpm build` target model beyond `web|desktop|service`.
  - Evidence:
    - Build target parsing is hard-coded to `web|desktop|service` in `crates/gc_cli_driver/src/pkg_workspace_ops.rs`.
    - Specs mirror this limit (`docs/spec/CLI.md`, `docs/spec/GCPM_JSON_SCHEMAS_v0.1.md`).
  - Acceptance:
    - Add first-class mobile targets (at minimum iOS/Android profile contracts) and edge/service-runtime variants.
    - Extend schema/evidence docs and CI coverage accordingly.

- [ ] P2.2 Decouple GPU compute and graphics architecture while preserving shared primitives.
  - Evidence:
    - Dispatch path co-routes `gpu/compute::*` and `gfx/gpu::*` in shared backend and policy handling (`crates/gc_effects/src/runner_capability_dispatch.rs`, `crates/gc_effects/src/runner_gpu_host.rs`).
  - Acceptance:
    - Define separate high-level compute and graphics policy bundles with explicit cross-over points.
    - Keep shared resource primitives but split operational surfaces and profile gates.
    - Add independent conformance lanes for compute-only and gfx-only stacks.

- [ ] P2.3 Continue AI-maintainable decomposition for high-churn authoring modules.
  - Evidence:
    - Top remaining GC authoring files still include large, high-churn modules (e.g. `prelude/modules/20_editor.gc` at 632 lines from source-size reports).
  - Acceptance:
    - Split major agent-touched modules into narrower domain units with stable interfaces.
    - Keep source-size debt allowlists empty and preserve deterministic assembly ordering.

- [ ] P2.4 Ship `write_genesisCode_skill` v1 as a multi-agent, executable distribution kit.
  - Evidence:
    - Skill docs/contracts exist, but no shipped executable "starter corpus + expected outputs" pack for Codex/Claude/other agents.
  - Acceptance:
    - Publish detailed v1 skill pack with:
      - canonical prompts/task recipes,
      - runnable examples across core domains,
      - failure-mode playbooks,
      - deterministic verification scripts.
    - Integrate with onboarding and authoring bundle as primary AI entrypoint.
