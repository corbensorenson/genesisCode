# GenesisCode Upgrade Plan - Red-Team Backlog (Unresolved Only)

Last updated: 2026-02-21

This file contains only unresolved findings from the latest red-team pass.
Completed items are intentionally removed.

Open checklist items: 6

## Release-Gate Integrity

- [ ] P1.1 Restore `release-full` hard-gate health (clippy-clean workspace).
  - Why: if `release-full` health does not pass, self-hosted production readiness is not defensible.
  - Evidence:
    - `bash scripts/check_upgrade_plan_health.sh --profile release-full` fails at `cargo clippy --workspace --all-targets -- -D warnings`.
    - `cargo clippy -p gc_obligations --all-targets -- -D warnings` reports `items_after_test_module` at `crates/gc_obligations/src/obligation_exec.rs:258`.
  - Done when:
    - `bash scripts/check_upgrade_plan_health.sh --profile release-full` passes end-to-end with no lint overrides;
    - no `-A clippy::items-after-test-module` suppression is needed.

## AI-First Iteration + Runtime Performance

- [ ] P2.1 Make selfhost artifact freshness checks fast-path by default.
  - Why: rebuild-based freshness verification wastes iteration budget in hot development loops.
  - Evidence:
    - `bash scripts/check_selfhost_artifact_fresh.sh` reports `ok (slow-path rebuild compare)` and emits `hint -> run scripts/update_selfhost_freshness_metadata.sh to enable fast-path`.
  - Done when:
    - normal regeneration flow updates freshness metadata automatically;
    - `check_selfhost_artifact_fresh.sh` uses metadata fast-path in the common case and only falls back to rebuild on mismatch/corruption.

- [ ] P2.2 Harden AI iteration SLO gates against host contention noise.
  - Why: agent-facing iteration gates must be stable under normal workstation background load, not only isolated runs.
  - Evidence:
    - under shared-load execution, `check_ai_iteration_slo.sh` observed `incremental_warm_ms=2665` vs regression budget `2268`;
    - isolated rerun passed with `incremental_warm_ms=807`, indicating contention sensitivity.
  - Done when:
    - perf gate runner enforces measurement isolation (or robust multi-sample statistics) and documents contention policy;
    - the SLO check remains deterministic/reliable across repeated local runs.

- [ ] P2.3 Require explicit GPU backend intent in release-facing runtime profiles.
  - Why: silent fallback can hide performance or capability regressions for GPU-heavy agent workloads.
  - Evidence:
    - `.genesis/perf/runtime_microbench_metrics.json` shows `gpu_compute_backend_policy=dev-allow-fallback` and `gpu_compute_backend=deterministic-fallback` even on hardware where device runtime is available.
    - `.genesis/perf/gpu_device_conformance_report.json` confirms `gpu_compute_backend=device-runtime` for this host when required.
  - Done when:
    - `gcpm`/runtime profiles provide explicit fail-open vs fail-closed backend policy selection per environment;
    - production/release profiles default to device-required unless explicitly overridden.

## AI Authoring Surface + Assurance Readiness

- [ ] P2.4 Consolidate docs into an agent-first canonical set.
  - Why: AI coding agents perform better with smaller, canonical doc surfaces and fewer overlapping specs.
  - Evidence:
    - repository currently has `89` markdown files under `docs/`, increasing retrieval ambiguity.
  - Done when:
    - canonical docs index defines source-of-truth files per domain;
    - superseded/overlapping docs are merged or explicitly deprecated;
    - agent-facing onboarding path is reduced to a minimal, stable subset.

- [ ] P2.5 Close regulated-assurance process gaps beyond current partial alignment.
  - Why: requested government/high-assurance use cases require lifecycle-process evidence, not only runtime semantics.
  - Evidence:
    - `feature_matrix.md` marks `DO-178C DAL A/B`, `NASA NPR 7150.2 Class A/B`, and `IEC 62304 Class C` as `partial alignment`.
  - Done when:
    - `gcpm` emits certification-oriented assurance packs (trace matrix, independence attestations, qualified-tool manifest, coverage exports);
    - policy-gated release workflows can produce reproducible audit bundles for target standard/profile.
