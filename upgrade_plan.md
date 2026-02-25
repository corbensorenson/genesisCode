# GenesisCode Upgrade Plan - Red-Team Backlog (Unresolved Only)

Last updated: 2026-02-25

Scope:
- Track only unresolved upgrades required for AI-first authoring reliability, selfhost closure, and productization trust.
- Keep this file machine-syncable with `.genesis/perf/selfhost_readiness_report.json` and `feature_matrix.md`.
- Keep completed work out of this file (git history + perf artifacts are closure evidence).

Open checklist items: 10

## Critical Path

- P0.1 - gcpm remote-first lock/install closure (remove local-only constraint)
- P0.2 - native FFI ABI family for high-throughput host interop
- P0.3 - strict-sound type/effect lane for protected refs and release workflows
- P0.4 - first-class agent intent-to-workflow planner contract (deterministic, policy-gated)

## Unresolved Backlog

- [ ] P0.1 `gcpm` remote dependency closure
Why: `genesis pkg lock --help` still advertises "local-only v0.1", and `genesis pkg install` verifies local presence instead of resolving/fetching missing lock entries from registries by default.
Done when:
  - `pkg lock` resolves/fetches missing refs/commits/snapshots from configured registries under policy.
  - `pkg install` can hydrate missing lock artifacts without separate manual sync steps.
  - deterministic lock-update rationale remains replayable and evidence-backed.
Evidence:
  - `.genesis/perf/domain_starter_registry_bootstrap_report.json`
  - `.genesis/perf/agent_capability_gauntlet_release_confidence_report.json`

- [ ] P0.2 Native FFI ABI family (`host/ffi::*`) with deterministic replay contract
Why: `docs/spec/HOST_ABI_INDEX_v0.1.json` currently has no `ffi` family; plugin surface is command-oriented (`host/plugin::command`) and does not provide zero-copy, typed native-call semantics needed for broad external ecosystem leverage.
Done when:
  - host ABI index includes `host/ffi` operations with schema IDs and capability policy gates.
  - replay logs encode FFI call boundaries deterministically (hash-anchored payload/result envelopes).
  - first-party docs define safety model (memory ownership, pinning, lifetime, deterministic mode limits).
Evidence:
  - `docs/spec/HOST_ABI_INDEX_v0.1.json`
  - `docs/spec/PLUGIN_ABI_SCHEMAS_v0.1.md`

- [ ] P0.3 Strict-sound type/effect lane (not gradual-only) for release gates
Why: `genesis typecheck --help` advertises a gradual checker; for autonomous high-scale refactors, protected refs need a fail-closed strict mode that rejects unknown/open effects and unresolved contract op signatures.
Done when:
  - `genesis typecheck` supports strict-sound mode with no unknown effect ops on release profiles.
  - `gcpm verify`/`gcpm doctor` expose strict-sound findings with deterministic machine-readable diagnostics.
  - strict-sound obligations are enforceable via policy for protected refs/tags.
Evidence:
  - `.genesis/perf/host_api_evolution_contract_report.json`
  - `.genesis/perf/gcpm_operation_contract_pack_report.json`

- [ ] P0.4 Agent intent-to-workflow planner contract
Why: `genesis --json agent-index` provides schemas/indices/workflows, but no built-in deterministic planner that maps an intent contract to executable workflow DAGs with capability and policy validation.
Done when:
  - a planner command/API emits deterministic workflow plans from structured intent input.
  - plans are policy-checked before execution and linked to effect-log/evidence lineage.
  - failure taxonomy includes deterministic repair hints for autonomous retries.
Evidence:
  - `docs/spec/AGENT_INDEX_v0.1.md`
  - `.genesis/perf/agent_workflow_runtime_parity_report.json`

- [ ] P1.1 Full-profile throughput and disk headroom hardening
Why: latest strict prepush profile run showed high wall-clock and disk-headroom soft-continue behavior (`.genesis/perf/prepush_standard_latest.log`), which slows autonomous CI loops.
Done when:
  - prepush-standard profile p95 is reduced and enforced under a tighter target.
  - low-disk paths fail predictably with guided remediation, not soft-continue in strict lanes.
  - gate sharding/caching policy minimizes lock contention regressions.
Evidence:
  - `.genesis/perf/upgrade_plan_health_profile_report.json`
  - `.genesis/perf/prepush_standard_latest.log`

- [ ] P1.2 Stage2 optimizer coverage floors in artifact policy
Why: selfhost artifact requirements currently permit zero minimum coverage floors (`:min-supported-modules 0`, `:min-validated-modules 0`), weakening release confidence against optimizer regressions.
Done when:
  - production artifact policy enforces non-zero minimum stage2 supported/validated module counts.
  - release profiles fail closed on stage2 coverage regression.
  - coverage floors are tracked historically in perf artifacts.
Evidence:
  - `selfhost/toolchain.gc`
  - `.genesis/perf/full_selfhost_cutover_profile_report.json`

- [ ] P1.3 External ecosystem bridge into GenesisPkg
Why: language-native package model is strong, but "build anything" adoption still needs deterministic mirror/bridge flows for external ecosystems (e.g., crates/npm/pypi) into GenesisPkg artifacts with provenance.
Done when:
  - mirrored external packages are transformed into signed GenesisPkg snapshots/commits.
  - dependency policy can pin mirrored provenance roots and replay conversion evidence.
  - bridge operations are capability-gated and auditable.
Evidence:
  - `docs/spec/GCPM_JSON_SCHEMAS_v0.1.md`
  - `.genesis/perf/gcpm_operation_contract_pack_report.json`

- [ ] P1.4 Regulated assurance integration closure for external controls
Why: assurance crosswalk still carries external objectives (e.g., IEC62304 device/QMS controls) that are outside current toolchain evidence closure.
Done when:
  - exportable integration contract maps Genesis evidence artifacts into external QMS/regulator workflows.
  - assurance-pack outputs include machine-readable bindings for those external controls.
  - unresolved external-control count is explicitly tracked and trended.
Evidence:
  - `docs/spec/ASSURANCE_STANDARDS_CROSSWALK_v0.1.json`
  - `.genesis/perf/assurance_profile_packs_report.json`

- [ ] P2.1 Domain kit/workflow expansion for wider autonomous build coverage
Why: current agent reference workflow set is broad but still finite; expanding tested domain kits improves "agent can build anything requested" practical coverage.
Done when:
  - workflow corpus adds new high-impact domains (multi-agent orchestration, realtime collaboration, data/ML pipeline variants, large-scale backend topologies).
  - new workflows are included in gauntlet/runtime parity gates.
Evidence:
  - `docs/spec/DOMAIN_KITS_v0.1.md`
  - `.genesis/perf/agent_capability_gauntlet_release_confidence_report.json`

- [ ] P2.2 Time-travel observability beyond dispatch traces
Why: current debug surface is strong for dispatch traces, but autonomous large-system repair needs deeper deterministic cross-layer traceability (planner -> compiler -> effect runner -> host responses).
Done when:
  - trace frames unify planner decisions, type/effect decisions, optimizer transforms, and host effect boundaries.
  - replay tooling can bisect nondeterministic-seeming failures deterministically across layers.
  - machine-readable diagnostics preserve stable schemas for agent remediation loops.
Evidence:
  - `docs/spec/CLI_JSON_SCHEMAS_v0.1.md`
  - `.genesis/perf/agent_scenario_perf_report.json`

## Evidence Anchors

- `.genesis/perf/selfhost_readiness_report.json`
- `.genesis/perf/full_selfhost_cutover_profile_report.json`
- `.genesis/perf/agent_capability_gauntlet_release_confidence_report.json`
- `.genesis/perf/agent_generative_workloads_report.json`
- `.genesis/perf/agent_workflow_runtime_parity_report.json`
- `.genesis/perf/gcpm_operation_contract_pack_report.json`
- `.genesis/perf/hot_path_runtime_report.json`
- `.genesis/perf/upgrade_plan_health_profile_report.json`
