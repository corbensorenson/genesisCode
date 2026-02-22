# GenesisCode Upgrade Plan - Red-Team Backlog (Unresolved Only)

Last updated: 2026-02-22

This file tracks unresolved blockers only. Completed work belongs in git history and release notes.

Open checklist items: 8

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

- [x] P1.1 Restore real profile coverage for `prepush-standard`/`release-full` when backlog is non-zero.
  - Evidence:
    - `scripts/check_upgrade_plan_health.sh` branches to mandatory-local when `Open checklist items > 0`:
      - prints `backlog open; running mandatory local guard gates.`
      - lines near `declared_open` gate handling (`scripts/check_upgrade_plan_health.sh:479-535`).
    - Live runs for both heavy profiles short-circuit:
      - `bash scripts/check_upgrade_plan_health.sh --profile prepush-standard`
      - `bash scripts/check_upgrade_plan_health.sh --profile release-full`
      - both reported `mandatory-local` completion (not full profile gates).
  - Acceptance:
    - `prepush-standard` and `release-full` execute their profile-specific gates by default even with open backlog items.
    - Keep `mandatory-local` as `dev-fast` behavior (or explicit opt-in), not as an implicit replacement for heavier profiles.
    - Profile reports must reflect actual profile gate sets, and fail if profile substitution occurs without explicit override.

- [x] P1.2 Eliminate recurring ENOSPC failures from gate target-dir sprawl.
  - Evidence:
    - Reproduced ENOSPC during runtime matrix check:
      `bash scripts/check_runtime_backend_feature_matrix.sh`
      -> `No space left on device (os error 28)` while compiling.
    - Live disk state at failure:
      - `df -h .` -> `Avail 62Mi` (100% capacity).
      - `.genesis/build` footprint -> `14G` (`.genesis/build/health` `11G`, `.genesis/build/runtime_backend_feature_matrix` `2.6G`).
  - Acceptance:
    - Add deterministic cache lifecycle policy (TTL/LRU/size cap) for `.genesis/build/*`.
    - Add proactive cleanup integration into health/profile runners before heavy compile lanes.
    - Gate must attempt deterministic reclaim before surfacing manual remediation steps.

- [x] P1.3 Put a hard SLO/budget on runtime backend matrix checks.
  - Evidence:
    - Last successful report:
      `.genesis/perf/runtime_backend_feature_matrix_report.json` -> `elapsed_ms=306863`, `budget_ms=null`.
    - Stage hotspots are still large (`gc_cli_driver profile-headless` 69s in the last full report).
    - Fresh rerun hit ENOSPC before completion, indicating this lane is both slow and fragile under local constraints.
  - Acceptance:
    - Enable fail-closed runtime budget with historical regression tracking.
    - Split matrix into deterministic shards/cached phases to reduce tail latency.
    - Add explicit pass/fail thresholds for CI and local profiles.

- [x] P1.4 Reduce production CLI parse/help surface gate tail latency.
  - Evidence:
    - Fresh run:
      `bash scripts/check_production_cli_parse_surface.sh`
      -> `.genesis/perf/production_cli_parse_surface_report.json` `elapsed_ms=156006` (`history_p95_ms=209265`).
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

- [ ] P2.5 Consolidate documentation into a single AI-authoring source-of-truth map.
  - Evidence:
    - Repo currently contains high documentation surface area (`find . -name '*.md'` -> 105 files; `docs/` alone -> 80 files).
    - Capability, assurance, and authoring guidance is spread across matrix, specs, status docs, and skill docs; synchronization currently depends on multiple custom hygiene scripts.
  - Acceptance:
    - Publish one canonical doc topology (`authoring`, `runtime`, `assurance`, `operations`) with ownership and update workflow.
    - Remove/merge redundant docs and keep thin index stubs where split files remain necessary.
    - Add a drift check that fails when canonical docs and derived docs disagree on capability status/open-gap IDs.
