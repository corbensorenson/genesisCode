# GenesisCode Upgrade Plan - Red-Team Backlog (Unresolved Only)

Last updated: 2026-10-01

Scope:
- Track only unresolved upgrades required for AI-first authoring reliability, selfhost closure, and production runtime trust.
- This file is the canonical active P0/P1 defect-ID source. The capability ledger mirrors the exact IDs, and generated status views must match it.
- Keep completed work out of this file. Durable source history and E1-E4 evidence establish closure; mutable `.genesis/perf/` observations do not.

Open checklist items: 7

## Critical Path

The 2026-10-01 review restores seven open P1 records under the current R4.2.e transaction. Their exact acceptance contracts and serial ordering are in `ROADMAP.md` section 3.29. Local runtime observations used a pre-existing August binary; fresh revision-bound reproduction is required before selecting a repair through the active-defect route. F02/F04 also have reproductions from current helper bodies; F06 is a current-source static gate failure. None is fresh-build or independent release evidence.

- [ ] P1.9 (F01) Eliminate canonical symbol/literal and improper-pair identity collisions; enforce lexical and term-domain roundtrip admission across construction, parsing, printing, hashing, store and execution tiers. Owner: R4.2.e baseline, with R4.5.a/R7.2.f conformance. Observation: symbol `true` and boolean `true` share store hash `acc8a7699a2bf4cbd05f69678eac4fc236572041c28dfd0ab558e5fcf2ab6540` despite unequal runtime values. The 2026-10-02 serialization-admission repair has local positive/negative store/replay controls in ROADMAP.md section 3.29; lossless runtime value hashing, versioned replay compatibility, remaining boundary audit and independent acceptance are still required.
- [ ] P1.10 (F02) Authorize and anchor filesystem traversal before any creation; denied writes/mkdir must leave both inside and outside trees unchanged under symlink traversal and replacement races. Owner: R4.2.e baseline, R4.5.b/R7.3.b. Observation: `escape/created/file.txt` through an outside symlink creates the outside parent before containment rejection.
- [ ] P1.11 (F03) Preserve source/destination during self-rename and rejected overwrite; replace pre-delete behavior with the specified safe replacement protocol. Owner: R4.2.e baseline, R4.5.b. Observation: `{:from "same.txt" :to "same.txt" :overwrite true}` deletes the file and then errors.
- [ ] P1.12 (F04) Remove the authorized final filesystem entry without dereferencing its symlink target; specify native/WASI parity. Owner: R4.2.e baseline, R4.5.b. Observation: removing `alias.txt -> victim.txt` removes the victim and leaves the link.
- [ ] P1.13 (F05) Bound actual primitive work, including constant-work empty string repetition at maximum count, without hidden tier/accounting divergence. Owner: R4.2.e baseline before R2.1.h, R4.5.a. Observation: `(prim str/repeat "" 18446744073709551615)` exceeds a five-second watchdog despite finite evaluator step limits; the zero-count control finishes.
- [ ] P1.14 (F06) Repair package-verify custody expectations for artifact-backed commit authority and prove native-oracle restoration is rejected. Owner: R4.2.e. Observation: the current boundary verifier fails with `bounded verify mechanism missing marker: gc_vcs::Commit::from_term` while production invokes `CommitAuthority::validate_expected_commit`.
- [ ] P1.15 (F07) Keep registry serving alive until an explicit or policy-defined stop, then stop/join/reap all ownership paths. Owner: R4.2.e baseline, R4.5.b/R7.4.a. Observation: unbounded `registry serve` exits successfully in about 0.03 seconds because natural join first requests shutdown.

Source-only candidates F08 (redirect policy) and F09 (server admission/cancellation) require fresh negative-control reproduction before adding P0/P1 IDs. F10 (bounded file reads) and F11 (transport integrity before store mutation) are tracked P2 corrections in the same roadmap matrix. All eleven are required by the recovery goal; queue membership does not exclude the remaining four. Keep completed defects in Git history and dated evidence; do not remove any new record on a source edit or E0 success alone.

## Evidence Anchors

- `upgrade_plan.md`
- `ROADMAP.md`
- `docs/spec/CAPABILITY_EVIDENCE_LEDGER_v0.1.json`
- `feature_matrix.md`
- `docs/status/REDTEAM_REPORT.md`
- `docs/status/SELFHOST_AUTHORITY_v0.1.md`
- `docs/spec/CAPABILITY_COVERAGE_STATUS_v0.1.json`
- `docs/spec/CAPABILITY_COVERAGE_AUDIT_v0.1.json`
- `docs/spec/CAPABILITY_COVERAGE_AUDIT_v0.1.md`

## Local Observation Inputs (E0, Not Closure Authority)

- `.genesis/perf/selfhost_readiness_report.json`
- `.genesis/perf/agent_capability_gauntlet_release_confidence_report.json`
- `.genesis/perf/agent_generative_workloads_report.json`
- `.genesis/perf/gcpm_operation_contract_pack_report.json`
- `.genesis/perf/remote_registry_runtime_parity_report.json`
- `.genesis/perf/gpu_device_conformance_report.json`
- `.genesis/perf/gpu_compute_runtime_profile_runtime_report.json`
- `.genesis/perf/gfx_runtime_profile_runtime_report.json`
- `.genesis/perf/webxr_browser_conformance_report.json`
- `.genesis/perf/gcpm_target_runtime_evidence_report.json`
- `.genesis/perf/source_decomposition_progress_report.json`
- `.genesis/perf/source_decomposition_tracked_parity_report.json`
- `.genesis/perf/ai_iteration_slo_metrics.json`
- `.genesis/perf/test_changed_fast_metrics.json`
