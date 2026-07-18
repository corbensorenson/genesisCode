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
- `R5.6.a`: Define the deterministic shared application architecture and lifecycle. Affected claims: `TARGET-APPLICATION-UI`.
- `R5.6.b`: Build the typed presentation and style intermediate representation. Affected claims: `TARGET-APPLICATION-UI`.
- `R5.6.c`: Implement the complete static, interactive, SSR, and PWA web product stack. Affected claims: `TARGET-WEB-INTERACTIVE-SSR-PWA`, `TARGET-WEB-STATIC`.
- `R5.6.d`: Implement accessible cross-target widgets and input semantics. Affected claims: `TARGET-ANDROID`, `TARGET-APPLICATION-UI`, `TARGET-DESKTOP-LINUX`, `TARGET-DESKTOP-MACOS`, `TARGET-DESKTOP-WINDOWS`, `TARGET-IOS`.
- `R5.6.e`: Make UI rendering efficient, incremental, validated, and inspectable. Affected claims: `TARGET-WEB-INTERACTIVE-SSR-PWA`.
- `R5.6.f`: Ship one product-grade cross-target UI testing API. Affected claims: `TARGET-APPLICATION-UI`, `TARGET-WEB-INTERACTIVE-SSR-PWA`.
- `R5.7.a`: Complete typed service routing, middleware, security, and protocol construction. Affected claims: `TARGET-SERVICE-DATA`.
- `R5.7.b`: Complete durable typed data, query, transaction, and migration APIs. Affected claims: `TARGET-SERVICE-DATA`.
- `R5.7.c`: Add production queues, jobs, cache, storage, and coordination primitives. Affected claims: `TARGET-SERVICE-DATA`.
- `R5.7.d`: Unify capability-scoped observability and operations. Affected claims: `TARGET-SERVICE-DATA`.
- `R5.7.e`: Build first-party data, numeric, tensor, and local ML libraries. Affected claims: `TARGET-DATA-ML`.
- `R5.8.a`: Define the deterministic game and simulation core. Affected claims: `TARGET-GAMES-MEDIA`.
- `R5.8.b`: Complete real 2D and 3D runtime systems. Affected claims: `TARGET-GAMES-MEDIA`.
- `R5.8.c`: Make shaders and the asset pipeline Genesis-authored. Affected claims: `TARGET-GAMES-MEDIA`.
- `R5.8.d`: Add production audio and creative timeline systems. Affected claims: `TARGET-GAMES-MEDIA`.
- `R5.8.f`: Ship Genesis-native game and media editing and debugging workflows. Affected claims: `TARGET-GAMES-MEDIA`.
- `R5.9.a`: Define Embedded Linux and constrained MCU profiles separately. Affected claims: `TARGET-EMBEDDED-LINUX`, `TARGET-MCU-LANGUAGE-AOT`.
- `R5.9.b`: Implement closed-world static flash, RAM, stack, timing, and resource verification. Affected claims: `TARGET-MCU-LANGUAGE-AOT`.
- `R5.9.c`: Specify real-time, interrupt, reset, watchdog, and power semantics. Affected claims: `TARGET-MCU-LANGUAGE-AOT`.
- `R5.9.d`: Define capability-safe typed board and peripheral HAL APIs. Affected claims: `TARGET-BOARD-HAL-FAMILIES`, `TARGET-EMBEDDED-LINUX`.
- `R5.9.e`: Build self-hostable IoT and robotics packages. Affected claims: `TARGET-BOARD-HAL-FAMILIES`.
- `R5.9.f`: Preserve and validate semantic identity across constrained compilation. Affected claims: `TARGET-MCU-LANGUAGE-AOT`.
- `R6.1.a`: Freeze and migrate deterministic package manifest and lock schemas. Affected claims: `CAP-PACKAGE-MANAGER`.
- `R6.2.d`: Enforce package evidence policy at registry acceptance. Affected claims: `CAP-EVIDENCE-GATED-PUBLISH`.
- `R6.3.a`: Define the supported v1 build target profiles. Affected claims: `CAP-RUNTIME-SURFACES`.
- `R6.3.b`: Implement the selfhosted genesis build graph and evidence outputs. Affected claims: `CAP-DEPLOYMENT-PIPELINE`.
- `R6.3.c`: Make target artifacts reproducible across independent builders. Affected claims: `CAP-DEPLOYMENT-PIPELINE`.
- `R6.3.f`: Require target-native executable entrypoints and authentic runtime workloads. Affected claims: `TARGET-ANDROID`, `TARGET-DESKTOP-LINUX`, `TARGET-DESKTOP-MACOS`, `TARGET-DESKTOP-WINDOWS`, `TARGET-IOS`.
- `R6.3.g`: Make all generated foreign glue disposable, reproducible, and non-authoritative. Affected claims: `TARGET-ANDROID`, `TARGET-DESKTOP-LINUX`, `TARGET-DESKTOP-MACOS`, `TARGET-DESKTOP-WINDOWS`, `TARGET-IOS`.
- `R6.5.a`: Ship the incremental Genesis web compiler and development server. Affected claims: `TARGET-WEB-INTERACTIVE-SSR-PWA`, `TARGET-WEB-STATIC`.
- `R6.5.b`: Produce standards-valid deployable static, SSR, edge, and PWA artifacts. Affected claims: `TARGET-WEB-INTERACTIVE-SSR-PWA`, `TARGET-WEB-STATIC`.
- `R6.5.c`: Prove supported behavior in real browser engines and mobile viewports. Affected claims: `TARGET-WEB-INTERACTIVE-SSR-PWA`, `TARGET-WEB-STATIC`.
- `R6.5.d`: Ship and maintain a complete self-hosted full-stack reference product. Affected claims: `TARGET-SERVICE-DATA`.
- `R6.6.a`: Build signed and installable real macOS, Windows, and Linux applications. Affected claims: `TARGET-DESKTOP-LINUX`, `TARGET-DESKTOP-MACOS`, `TARGET-DESKTOP-WINDOWS`.
- `R6.6.b`: Build complete executable iOS and Android applications. Affected claims: `TARGET-ANDROID`, `TARGET-IOS`.
- `R6.6.c`: Automate simulator, emulator, and physical mobile and desktop qualification. Affected claims: `TARGET-ANDROID`, `TARGET-DESKTOP-LINUX`, `TARGET-DESKTOP-MACOS`, `TARGET-DESKTOP-WINDOWS`, `TARGET-IOS`.
- `R6.6.d`: Prove shared product reuse without hiding target-specific work. Affected claims: `TARGET-ANDROID`, `TARGET-IOS`.
- `R6.7.a`: Promote reproducible Raspberry Pi-class Embedded Linux delivery. Affected claims: `TARGET-EMBEDDED-LINUX`.
- `R6.7.b`: Implement the closed-world MCU AOT, link, firmware, and evidence pipeline. Affected claims: `TARGET-MCU-LANGUAGE-AOT`.
- `R6.7.c`: Establish qualified RP2040, RISC-V ESP32, Xtensa ESP32, and Arduino-compatible board families. Affected claims: `TARGET-BOARD-HAL-FAMILIES`.
- `R6.7.d`: Build deterministic board simulation and physical hardware-in-the-loop infrastructure. Affected claims: `TARGET-DEVICE-FLASH-DEBUG-OTA`, `TARGET-HARDWARE-IN-THE-LOOP`.
- `R6.7.e`: Complete authorized device discovery, flash, debug, provisioning, crash, and OTA lifecycle. Affected claims: `TARGET-DEVICE-FLASH-DEBUG-OTA`.
- `R6.7.f`: Ship maintained Embedded Linux and physical MCU hardware products. Affected claims: `TARGET-BOARD-HAL-FAMILIES`, `TARGET-DEVICE-FLASH-DEBUG-OTA`, `TARGET-EMBEDDED-LINUX`, `TARGET-HARDWARE-IN-THE-LOOP`.
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
- `R8.2.m`: Prove a nontrivial cross-target game under real frame and resource budgets. Affected claims: `TARGET-GAMES-MEDIA`.
- `R8.2.n`: Prove a creative media tool with deterministic transforms and real preview/export. Affected claims: `TARGET-GAMES-MEDIA`.
- `R8.2.o`: Prove a Raspberry Pi-class Embedded Linux product on real hardware. Affected claims: `TARGET-EMBEDDED-LINUX`.
- `R8.2.p`: Prove RP2040 and ESP32-class constrained firmware on physical boards. Affected claims: `TARGET-HARDWARE-IN-THE-LOOP`, `TARGET-MCU-LANGUAGE-AOT`.
- `R8.2.q`: Prove an end-to-end IoT or robotics system with hardware-in-the-loop. Affected claims: `TARGET-BOARD-HAL-FAMILIES`, `TARGET-HARDWARE-IN-THE-LOOP`.
- `R8.2.r`: Prove a reproducible Genesis-native data science and local ML system. Affected claims: `TARGET-DATA-ML`.
- `R8.3.a`: Ship and maintain at least five evidence-backed flagship programs. Affected claims: `CAP-DOMAIN-STARTERS`, `CAP-GRAPHICS-RUNTIME`.
- `R9.2.c`: Publish and independently mirror immutable E4 release attestations. Affected claims: `CAP-TOOL-QUALIFICATION`.
- Active P0/P1 defect IDs: none. Roadmap gaps above remain open.

#### Evidence

- Release-note authority: `E1`; runtime gate results are not asserted.
- Retained fixture distribution class: `E2`; fixture authority: `false`.
- Authorized unqualified capability claims: none.
- Authorized product/target claims: none.
- Aggregate foundation claims never imply product/target qualification: `CAP-RUNTIME-SURFACES`, `CAP-DOMAIN-STARTERS`, `CAP-GPU-COMPUTE`, `CAP-GRAPHICS-RUNTIME`, `CAP-BROWSER-XR`, `CAP-DEPLOYMENT-PIPELINE`.
- Baseline `a3d6c7b809f1c1ba403bab0c4e18fce94154cbae4b35b23aa9e96cfb1c02e967` is signed `E0` observation only: 2 passing, 2 failing, 5 unavailable, 1 decision-gated; its signature grants no authority.

#### Dependencies

| Lockfile | Records | Registry | Git | SHA-256 |
|---|---:|---:|---:|---|
| `Cargo.lock` | 459 | 441 | 0 | `e0714f3cf7579ec40ece9b4bb341c69e039753d721bcb32ff467d819fb1a7ddc` |
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

Machine-readable identity: `ab21a3ded78eafe070359e75632b3598840cfb17838641605bd66e3697247819`.
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
