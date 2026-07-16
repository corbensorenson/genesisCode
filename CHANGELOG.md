# Changelog

All notable GenesisCode changes are tracked here. The project is pre-1.0; format, CLI, and language semantics may change when the relevant specs and migration notes change in the same release.

## [Unreleased]

- Define GenesisCode, GenesisBench, and Genesis Model as independently versioned products with separate release authorities, typed compatibility, isolated acceptance lanes, and no model dependency for the language release.
- Expand the benchmark roadmap through signed public governance, lineage-correct statistics, fixed-scaffold model comparison, temporal challenge overlays, construct-validity studies, a training/evaluation firewall, profile-bound local models, and four-cell language/model co-evolution.
- Add the self-hostable signed GenesisBench result registry and deterministic lexicographic static leaderboard, preserving complete append-only result history and independently replayed scoring.
- Make the consolidated agent-authoring gate validate the frozen GC-AGENT profile and its negative controls, preventing stale profile source identities from escaping focused benchmark checks.
- Keep Cargo build caches within the generated-state quota through profile-aware cache classes, priority-based reclamation, explicit lease release, and exact disk telemetry for cache-sensitive gates.
- Harden generated-state startup and reclamation: clean checkouts create provenance-marked Cargo roots before admission, one-shot resolvers release transient leases, and deterministic cleanup tolerates bounded metadata recreation after atomic quarantine.
- Avoid false generated-state quota denial by keeping Python-only evidence gates lease-free while nested verifier builds reserve their own declared cache class.
- Render non-JSON failures from the structured diagnostic catalog with operation, redacted subject, primary cause, one safe next action, deterministic wrapping, and terminal-aware color.
- Consolidate governance entrypoints under a manifest-enforced one-in/one-out budget, retiring redundant feature-matrix check/update aliases in favor of capability-ledger authorities.

<!-- BEGIN GENERATED RELEASE NOTES: genesis/release-notes/v0.1 -->
### Generated Release Facts

This block is generated from canonical repository inputs by `scripts/update_release_notes.sh`. It is E1 traceability, not runtime or release authority.

#### Compatibility

V1 registry claim: `reserved-not-stable`. Reserved IDs are not stable compatibility promises.

| Surface | Current writer | Accepted readers | Migrations |
|---|---|---|---|
| `product-release` | `0.2.0` | `0.2.0` | none |
| `package-manifest` | `1` | `pre-schema`, `1` | `M-PACKAGE-PRESCHEMA-TO-1` |
| `workspace-config` | `1` | `1` | none |
| `genesis-lock` | `2` | `1`, `2` | `M-LOCK-1-TO-2` |
| `effect-log` | `3` | `2`, `3` | `M-GCLOG-2-TO-3` |
| `gpk-bundle` | `2` | `1`, `2` | `M-GPK-1-TO-2` |
| `canonical-hash-profile` | `genesis/hash-profile/gcv0.2-blake3` | `genesis/hash-profile/gcv0.2-blake3` | none |
| `compiled-module-blob` | `GCKM5\0` | `GCKM5\0` | none |
| `selfhost-compiled-cache` | `GCSHC1\0` | `GCSHC1\0` | none |
| `selfhost-toolchain-artifact` | `genesis/selfhost-toolchain-artifact-v0.2` | `genesis/selfhost-toolchain-artifact-v0.2` | none |

#### Migration Notes

- `M-PACKAGE-PRESCHEMA-TO-1`: No package semantics change; the discriminator makes future evolution fail closed. User action: Add schema = 1 as a top-level key. Support: Retain through the 0.x train; removal requires a major compatibility decision and corpus telemetry.
- `M-LOCK-1-TO-2`: Version 2 records source selectors, resolution strategies, tag policy, resolved refs, export hashes, and environment fingerprints. User action: Run a lock-producing gcpm operation and review the canonical version 2 diff. Support: Retain v1 reads through the 0.x train; remove only with an offline migrator and a release-note deprecation cycle.
- `M-GCLOG-2-TO-3`: Version 3 binds the current value hash encoding and deterministic task scheduling metadata. User action: Regenerate logs for archival provenance; replay may still consume explicit version 2 logs. Support: Retain v2 replay through the 0.x train; removal requires an authenticated log migrator and golden replay corpus.
- `M-GPK-1-TO-2`: Version 2 adds a deterministic, sorted refs section; payload object encoding is unchanged. User action: Re-export a v1 bundle to obtain canonical v2; imports continue to accept v1. Support: Retain v1 reads through the 0.x train; removal requires a streaming re-export tool and fixture census.

#### Known Gaps

- `R1.3.c`: Generate and verify the pinned MCP agent interface from canonical CLI schemas. Affected claims: `CAP-AGENT-JSON-CONTRACTS`.
- `R1.4.c`: Build deterministic generation, repair, refactor, and deployment task benchmarks. Affected claims: `CAP-AGENT-WORKLOAD-PARITY`.
- `R1.5.a`: Generate the authoring skill from canonical language and capability inputs. Affected claims: `CAP-AGENT-SKILL-PACK`.
- `R1.5.f`: Validate skill distribution, offline use, token budgets, and multi-agent compatibility. Affected claims: `CAP-AGENT-SKILL-PACK`.
- `R2.2.f`: Prove host-handle cleanup across success, failure, cancellation, timeout, and restart. Affected claims: `CAP-HOST-BRIDGE`.
- `R2.3.e`: Meet explicit incremental large-workspace agent-loop SLOs. Affected claims: `CAP-AGENT-WORKSPACE-PERF`.
- `R3.2.a`: Audit existing stage2/Wasm coverage and eliminate undeclared fallback. Affected claims: `CAP-RUNTIME-SURFACES`, `CAP-STAGE2-WASM-VALIDATION`.
- `R3.2.b`: Validate source-to-Wasm translation and embedded artifact identity. Affected claims: `CAP-STAGE2-WASM-VALIDATION`.
- `R4.1.c`: Maintain a semantic-ownership ledger for every production decision. Affected claims: `CAP-STRICT-NO-FALLBACK`.
- `R4.1.e`: Enforce selfhost and stage0 dependency boundaries structurally. Affected claims: `CAP-STRICT-NO-FALLBACK`.
- `R4.2.a`: Make frontend and canonicalization GenesisCode-produced with independent identity verification. Affected claims: `CAP-COREFORM-IDENTITY`, `CAP-SELFHOST-FRONTEND`.
- `R4.2.c`: Make semantic patch and refactor behavior GenesisCode-authoritative. Affected claims: `CAP-SEMANTIC-VCS`.
- `R4.2.d`: Make obligation and policy decision logic GenesisCode-authoritative. Affected claims: `CAP-DENY-DEFAULT-POLICY`, `CAP-EVIDENCE-GATED-PUBLISH`.
- `R4.2.e`: Make package, registry, and VCS logic GenesisCode-authoritative. Affected claims: `CAP-ARTIFACT-GC`, `CAP-PACKAGE-MANAGER`, `CAP-SEMANTIC-VCS`.
- `R4.2.g`: Make CLI and agent orchestration selfhost-authoritative without hidden Rust semantics. Affected claims: `CAP-SELFHOST-FRONTEND`.
- `R4.3.a`: Split oversized host crates and meet source concentration budgets. Affected claims: `CAP-MODULAR-BOUNDARIES`.
- `R4.3.b`: Generate repetitive host, Prelude, capability, MCP, codec, and parity bindings from one schema. Affected claims: `CAP-MODULAR-BOUNDARIES`.
- `R4.4.c`: Prove cross-host bootstrap fixpoint on the tier-1 host matrix. Affected claims: `CAP-SELFHOST-CUTOVER`.
- `R5.2.a`: Audit and reconcile deterministic concurrency specifications and runtime behavior. Affected claims: `CAP-CONCURRENCY-REPLAY`.
- `R5.2.e`: Differentially explore and replay bounded schedules across tiers and hosts. Affected claims: `CAP-CONCURRENCY-REPLAY`.
- `R5.5.a`: Adopt and pin the WebAssembly Component Model/WIT extension boundary. Affected claims: `CAP-PLUGIN-FFI`.
- `R5.5.c`: Sandbox extension memory, CPU, calls, effects, cancellation, and lifecycle. Affected claims: `CAP-PLUGIN-FFI`.
- `R6.1.a`: Freeze and migrate deterministic package manifest and lock schemas. Affected claims: `CAP-PACKAGE-MANAGER`.
- `R6.2.d`: Enforce package evidence policy at registry acceptance. Affected claims: `CAP-EVIDENCE-GATED-PUBLISH`.
- `R6.3.a`: Define the supported v1 build target profiles. Affected claims: `CAP-RUNTIME-SURFACES`.
- `R6.3.b`: Implement the selfhosted genesis build graph and evidence outputs. Affected claims: `CAP-DEPLOYMENT-PIPELINE`.
- `R6.3.c`: Make target artifacts reproducible across independent builders. Affected claims: `CAP-DEPLOYMENT-PIPELINE`.
- `R7.1.a`: Classify and own the complete layered verification suite. Affected claims: `CAP-ASSURANCE-PROFILES`.
- `R7.1.e`: Use mutation testing to demonstrate trust-check sensitivity. Affected claims: `CAP-TOOL-QUALIFICATION`.
- `R7.2.a`: Mechanize the pure core semantics and determinism theorem. Affected claims: `CAP-KERNEL-DETERMINISM`.
- `R7.2.b`: Prove seal unforgeability and protected-boundary behavior. Affected claims: `CAP-SEALED-PROTOCOL`.
- `R7.2.c`: Model and prove deterministic complete effect dispatch and replay checking. Affected claims: `CAP-EFFECT-REPLAY`.
- `R7.2.f`: Verify or independently cross-check canonical codec assumptions. Affected claims: `CAP-COREFORM-IDENTITY`.
- `R7.3.a`: Threat-model every kernel, effect, bridge, package, build, and release boundary. Affected claims: `CAP-ASSURANCE-PROFILES`, `CAP-DENY-DEFAULT-POLICY`.
- `R7.3.b`: Sandbox host execution with hard cancellation, least privilege, and bounded IPC. Affected claims: `CAP-HOST-BRIDGE`.
- `R8.2.a`: Prove the pure library and CLI agent archetype end to end. Affected claims: `CAP-AGENT-WORKLOAD-PARITY`.
- `R8.2.e`: Prove the browser application agent archetype and its sandboxed effects. Affected claims: `CAP-BROWSER-XR`.
- `R8.2.h`: Prove the GPU or numeric agent archetype with deterministic backend evidence. Affected claims: `CAP-GPU-COMPUTE`.
- `R8.3.a`: Ship and maintain at least five evidence-backed flagship programs. Affected claims: `CAP-DOMAIN-STARTERS`, `CAP-GRAPHICS-RUNTIME`.
- `R9.2.c`: Publish and independently mirror immutable E4 release attestations. Affected claims: `CAP-TOOL-QUALIFICATION`.
- Active P0/P1 defect IDs: none. Roadmap gaps above remain open.

#### Evidence

- Release-note authority: `E1`; runtime gate results are not asserted.
- Retained fixture distribution class: `E2`; fixture authority: `false`.
- Authorized unqualified capability claims: none.
- Baseline `a3d6c7b809f1c1ba403bab0c4e18fce94154cbae4b35b23aa9e96cfb1c02e967` is signed `E0` observation only: 2 passing, 2 failing, 5 unavailable, 1 decision-gated; its signature grants no authority.

#### Dependencies

| Lockfile | Records | Registry | Git | SHA-256 |
|---|---:|---:|---:|---|
| `Cargo.lock` | 461 | 443 | 0 | `43fb232af747e3224dbfd6715f5e9e68a7fd3dbb9b48188c98bf983855ed7bbf` |
| `tools/genesis-evidence-producer/Cargo.lock` | 41 | 40 | 0 | `a7aa895176386dcbde3de7e0d49a8511ca38227880ee30e82c12978cc5fa416e` |
| `tools/genesis-evidence-verifier/Cargo.lock` | 41 | 40 | 0 | `d3a5c9c2e7d3cb614d79b3360210c93f63317cdbb808250d0084ff4c1822f3eb` |
| `package-lock.json` | 3 | sha512 integrity | 0 | `f5b2fa938c2c572fa8172b0f57418c10dca846791855fbd25b1b662b188097ed` |

#### Security

No security gate is represented as passed by this static document. Release authority requires fresh retained evidence for every mandatory check:

- `scripts/check_dependency_mirror_contract.sh` (`test`, `prepush-standard`, network `deny`): required, not attested here.
- `scripts/check_evidence_adversarial_matrix.sh` (`test`, `prepush-standard`, network `deny`): required, not attested here.
- `scripts/check_genesis_evidence_verifier.sh` (`test`, `prepush-standard`, network `deny`): required, not attested here.
- `scripts/check_no_user_panics.sh` (`static`, `local-fast`, network `deny`): required, not attested here.
- `scripts/check_no_user_panics_compiler.sh` (`test`, `prepush-standard`, network `deny`): required, not attested here.
- `scripts/check_release_smoke.sh` (`release-only`, `release-full`, network `deny`): required, not attested here.
- `scripts/check_root_lock_policy.sh` (`static`, `local-fast`, network `deny`): required, not attested here.
- `scripts/check_supply_chain.sh` (`release-only`, `release-full`, network `deny`): required, not attested here.
- `scripts/check_versioning_release_hygiene.sh` (`release-only`, `release-full`, network `deny`): required, not attested here.

Machine-readable identity: `b30112500fc65dcaee215fca18ba71d4296f5c064c92c936d51571ea868d3217`.
<!-- END GENERATED RELEASE NOTES: genesis/release-notes/v0.1 -->

## [0.2.0] - 2026-07-02

### Added

- Canonical `ROADMAP.md` with R0-R9 phases plus a post-v1 frontier program, evidence rules, budgets, acceptance milestones, and risk register.
- Green-front-door gate aggregating docs, invariants, profile matrix, generated-artifact, versioning, supply-chain, release-smoke, and changed-fast checks.
- Root `genesis.lock` policy and docs quickstart gate.
- Versioning policy, release-smoke contract, changelog hygiene gate, and workspace-wide version inheritance.
- Generated perf-artifact source-control policy with explicit allowlist.
- Supply-chain gate using `cargo-deny` plus duplicate-major drift detection.

### Changed

- Workspace crates now inherit `version = "0.2.0"` from `[workspace.package]`.
- Selfhost generated artifact metadata is aligned to the package version.
- CI profile matrix includes release-hardening guards instead of relying on informal manual checks.

### Fixed

- WASI mirror tests were brought back into parity with native selfhost behavior for package lock fixtures, poisoned module handling, artifact path pinning, and non-scalar fallback hashing.
- Production no-panic, replay metadata, timeout, deterministic path, and GC log-version hardening issues from the earlier R0 review were addressed before this release train.

### Known Gaps

- GenesisCode is not v1.0-complete yet: high-performance runtime tiers, MCP-native warm loop, formal selfhost fixpoint closure, registry GA, and flagship examples are roadmap items, not completed release claims.
