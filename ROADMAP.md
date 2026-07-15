# GenesisCode Roadmap: Agent-Native, Efficient, Self-Hosted, and Trustworthy

Last audited: 2026-07-14

Status: canonical strategic plan for the path from v0.2 to v1.0 and the post-v1 frontier. This file is not release evidence. Repository history, clean-clone verification, and remote CI evidence are mandatory under R0.1.

Active project goal: fully complete this roadmap end-to-end in a beyond state-of-the-art manner: AI-first, super efficient, fully self-hosted above a minimal auditable host, reproducible, formally hardened, and validated by executable evidence.

GenesisCode is not trying to be another general-purpose language with an AI plugin. It is a deterministic software substrate designed for AI-authored and AI-iterated systems. Its differentiator is the combination of compact machine-facing semantics, content-addressed identity, sealed effects, deny-by-default capabilities, deterministic replay, semantic patches, evidence-carrying packages, and a self-hosted toolchain whose claims can be independently checked.

The roadmap is intentionally ambitious, but it is not a list of unrelated ambitions. It is a dependency-ordered release program. The first priority is making GenesisCode safe and effective for agents now. Performance tiers, broad domain support, and recursive self-improvement follow only after the semantic and evidence foundations they depend on are real.

---

## 1. Product thesis and boundaries

### 1.1 North-star outcome

A local or remote AI agent should be able to learn the stable GenesisCode profile from a small, generated context bundle; create or repair a program; receive structured diagnostics; run it under explicit capabilities and resource bounds; inspect or replay every effect; package it with provenance and obligation evidence; and deploy it without trusting raw model output.

The v1 toolchain should be self-hosted above a deliberately small stage0 host. "Fully self-hosted" means the language-level frontend, typechecker, formatter, optimizer, patch engine, obligation logic, package/registry logic, documentation generators, and build orchestration are authored in GenesisCode and bootstrap to a reproducible fixpoint. It does not mean pretending that hardware, an operating system, a WebAssembly engine, or the minimal evaluator/effect host does not exist.

### 1.2 What "amazing" means here

GenesisCode v1 must be strong in all of these dimensions at once:

1. **Agent legibility:** the useful language surface fits within explicit context budgets and exposes machine-readable schemas, diagnostics, repair hints, examples, and capability contracts.
2. **Mechanical trust:** generated code is never accepted because it looks plausible. Semantics, effects, policy decisions, patches, packages, and releases carry independently checkable evidence.
3. **Determinism:** pure evaluation, canonical identity, effect logging, replay, concurrency, builds, and bootstrap outputs have specified cross-machine behavior.
4. **Efficiency:** the common agent edit-check-run loop is interactive, the reference interpreter is credible, compiled tiers are validated, and long-lived services have bounded memory and resource behavior.
5. **Self-host authority:** self-host claims describe semantic ownership and bootstrap closure, not merely a GenesisCode wrapper that routes to Rust implementations.
6. **Self-hostable infrastructure:** no required cloud control plane, hosted registry, proprietary model, or external telemetry service is needed to build, verify, package, or deploy software.
7. **Practical reach:** real programs across the declared v1 domain matrix work end-to-end, while unsupported domains are named rather than implied by broad marketing language.
8. **Replaceability:** an independent implementation can pass the kernel and artifact conformance suites from the published specifications.

### 1.3 Non-goals before v1

- Syntax breadth is not a goal. New syntax must reduce total agent/human complexity or unlock a required semantic capability.
- A production JIT is not automatically a v1 requirement. It becomes critical-path work only if the interpreter, bytecode VM, prelude snapshot, and warm daemon cannot meet the measured product budgets.
- "Supports anything" is not an acceptable release claim. v1 supports the explicit domain matrix in R5/R6/R8; other domains remain experimental until they satisfy the same evidence bar.
- Raw LLM inference never enters the pure kernel. Model access is an explicit host effect with capability, provenance, budget, redaction, and replay rules.
- GenesisCode does not require agents to modify generated Rust or shell glue. Any unavoidable host surface must be small, generated where possible, versioned, and independently tested.
- Passing one local script or the existence of a JSON report is not release proof.

---

## 2. Truth model

The project currently has broad functionality and many gates, but a single green checkmark hides important differences. From R0 onward, every capability and public claim uses this maturity ladder:

| Level | Name | Meaning |
|---|---|---|
| L0 | Specified | Normative behavior, failure semantics, resource model, and compatibility rules exist. |
| L1 | Implemented | A reachable implementation exists; experimental wiring and wrappers qualify only for L1. |
| L2 | Verified | Positive, negative, differential, property, and boundary tests pass on the reference host. |
| L3 | Reproducible | Hermetic checks pass from a clean checkout on the supported host matrix with pinned inputs. |
| L4 | Product-proven | The feature succeeds in representative agent and domain workloads under declared SLOs. |
| L5 | Release-attested | A signed, immutable release evidence bundle binds source, toolchain, tests, artifacts, and claims. |

Rules:

- `feature_matrix.md` must report the level, not a binary checkmark.
- A checked roadmap task cites durable evidence and its exact input identity. Mutable `.genesis/perf/` files are local observations, not L5 evidence.
- A `check_*` command is read-only. It must never repair state, regenerate reports, download undeclared inputs, or turn a missing artifact into a pass.
- An `update_*` command may regenerate derived material, but its output must be reviewed and then checked separately.
- Release evidence is content-addressed, schema-versioned, reproducible offline from declared inputs, and independently verifiable without trusting the producing CLI.
- Routing a command through a `.gc` entrypoint establishes reachability, not self-host semantic ownership.
- Public documentation states the lowest level that all supported platforms satisfy.

### 2.1 Evidence classes

| Class | Purpose | Durability |
|---|---|---|
| E0 local observation | Development timing, debug output, exploratory report | Ignored and disposable |
| E1 checked test | Named test/golden/property result tied to a source revision | Reproducible in-tree |
| E2 gate manifest | Machine-readable inputs, commands, versions, outputs, durations, and resource use | Versioned schema; generated artifact may be ignored |
| E3 release candidate bundle | Full host-matrix evidence, negative controls, SBOM, provenance, benchmark samples | Immutable candidate artifact |
| E4 release attestation | Signed hash tree over source, binaries, evidence, compatibility profile, and claims | Published and independently mirrored |

### 2.2 Definition of done for a task

A task may be checked only when its line contains `done YYYY-MM-DD; evidence: <stable path or test>; input: <revision/hash>`. The cited check must include at least one negative control when the task protects a trust boundary. Performance tasks include raw samples, machine profile, variance, warm/cold distinction, and a reproducible command. Documentation alone cannot close implementation work.

### 2.3 Open-standards posture

GenesisCode should define new formats only for genuinely GenesisCode-specific semantics. It should pin and profile mature open standards for envelopes, transport, components, updates, and inventories rather than create gratuitously incompatible substitutes. Versions below are the audited 2026-07-10 baseline; R0 must pin exact schemas and provide an explicit upgrade process rather than following `latest` implicitly.

| Area | Adopt/profile | GenesisCode-specific extension |
|---|---|---|
| Agent transport | [MCP 2025-11-25](https://modelcontextprotocol.io/specification/2025-11-25) over stdio first, with version/capability negotiation; experimental Tasks are negotiated and never required by the core profile | Semantic workspace snapshots, capability minimization, evidence IDs, and deterministic tool-result contracts |
| Attestation framework | [in-toto Attestation Framework v1.2](https://github.com/in-toto/attestation/blob/main/spec/README.md), Statement v1, and an explicitly pinned authenticated envelope/bundle profile | Predicates for semantic patches, obligations, replay verification, bootstrap witnesses, and agent provenance |
| Build/source provenance | [SLSA 1.2](https://slsa.dev/spec/v1.2/) provenance and verification expectations | Hermetic/reproducible-build claims, Genesis profiles, semantic inputs, and independent verifier results |
| Portable extension ABI | WebAssembly Component Model and WIT with [WASI 0.3](https://wasi.dev/releases) as the async-capable candidate profile and a declared WASI 0.2 compatibility strategy | Mapping Genesis values/effects/capabilities/resource charges to component worlds |
| Package/update trust | [TUF 1.0.34](https://github.com/theupdateframework/specification/blob/master/tuf-spec.md) threshold roles, delegation, freshness, rollback/freeze protection, and recovery | Content-addressed package graph, evidence policy, transparency proofs, and offline promotion |
| Software inventory | [SPDX 3.0.1](https://spdx.github.io/spdx-spec/) or a later explicitly profiled compatible version | Genesis package/profile/capability/evidence relationships not represented by the base SBOM |

The project may use narrower subsets or stricter rules. Every deviation records why the standard is insufficient, how interoperability is retained, and who independently verifies the extension.

---

## 3. Audited baseline: 2026-07-10

This baseline prevents the roadmap from planning already-existing work as if it were greenfield, while also preventing existing wiring from being overstated as complete.

### 3.1 What exists

- An 18-crate Rust workspace implements the kernel, effects, CLI, types, optimizer, obligations, patches, packages, registry, VCS, WASI, WebAssembly, graphics, and runtime benchmarks.
- The pure-kernel, seal, no-user-panic, deterministic effect log/replay, deny-by-default capability, and self-host-boundary gates exist and currently pass when their prerequisites are built.
- Production command routing is artifact-only/self-host-first, strict fallback guards exist, and stage2 WebAssembly execution uses `wasmi`.
- Effect rows and deterministic concurrency specifications/implementations already exist. Their roadmap work is audit, completion, and product proof, not invention from scratch.
- The `warm` command and JSON request protocol exist. A generated MCP server does not yet appear to be the default agent interface.
- The authoring skill, pointer guide, prompt pack, recipes, agent index, generative workloads, and capability gauntlets exist and pass static structure checks.
- The project has substantial domain wiring across services, browser, data, process/network, GPU, hardware, mobile deployment, collaboration, UI, graphics, and XR.
- Recent interpreter work added inline small integers, hybrid vectors, compiled parse fast paths, fast primitive wrappers, n-ary application support, and inline environment slots.
- A local strict workload measured approximately: `fib(25)=163ms`, vector build `=6ms`, map build `=402ms`, string concatenate `=15ms`, self-host parse `=3656ms`, dispatch `=1211ms`. These are E0 observations, not release claims.

### 3.2 Material gaps found by this audit

1. `ROADMAP.md` is untracked, so the nominal canonical plan is not yet repository history.
2. `feature_matrix.md` reports a nearly all-green binary view and "Known gaps: none" despite open semantic, performance, self-host, agent-training, evidence, and release work.
3. Several evidence checks treat absent mutable reports as a reason to regenerate them. Audit and mutation are therefore conflated.
4. Resolved 2026-07-10: `scripts/check_root_lock_policy.sh` no longer assumes Python 3.11 `tomllib`; its required TOML subset is checked by a POSIX `awk` parser with embedded adversarial controls.
5. Cold hardening checks can rebuild much of the workspace. One no-panic/TCB run took about 182 seconds and regenerated roughly 4.8 GiB under `.genesis/build/cargo`, making the default agent loop too expensive.
6. The two largest Rust crates, `gc_effects` and `gc_cli_driver`, are roughly 40k and 32k Rust lines respectively. This concentration increases review, compile, and semantic-ownership risk.
7. Self-host `.gc` sources are much smaller than the remaining Rust semantic surface. Current command routing does not prove that typechecking, obligation decisions, packaging, canonicalization, or patch semantics are GenesisCode-authoritative.
8. The current authoring bundle is broad but not yet a compact, versioned, training-ready SDK. It lacks a stable diagnostic catalog, held-out task corpus, leakage policy, and model-agnostic scorecard.
9. Interpreter closure capture, linked lexical frames, `let` allocation, long tail-loop behavior, and map performance remain open.
10. Heap ownership, cycles, allocation accounting, deterministic out-of-memory behavior, and long-lived daemon leak limits are not yet closed as language/runtime contracts.
11. The existing roadmap places bytecode/JIT work before the agent loop and treats a heavyweight JIT as mandatory without a measured decision gate.
12. Version/profile compatibility, migration tooling, release evidence authority, gate cost, and disk lifecycle need first-class ownership.

### 3.3 Immediate interpretation

GenesisCode is ready for controlled experimentation and agent-assisted development inside the repository. It is not yet ready to be presented as a stable training target or trusted production language. The shortest path to that status is R0 plus R1, not a JIT. The shortest path to a defensible "fully self-hosted" claim is the ownership ledger and bootstrap program in R4, not more routing coverage.

### 3.4 Independent review delta: 2026-07-14

An independent dirty-worktree review exercised the newly built trust and agent surfaces rather than accepting their generated reports. It confirmed that the MCP server, warm protocol, structured diagnostics, compact cards, independent evidence verifier, signed failing baselines, and most of the gate architecture are substantive. Its full local run reported 1,565 passes, five failures, and 15 ignored tests across 269 binaries. Those counts are E0 observations, not clean-clone or release evidence.

The review also established the following release-blocking or roadmap-relevant deltas:

1. Denied-effect replay diverges between Rust and self-host engines on both native and WASI mirrors. This is one deterministic trust-path defect, not two unrelated test failures; R0.2.f owns its closure.
2. Spawn and persistent host-bridge kill/reap stress tests race under load, `cli_agent_plan` is load-sensitive, and a default regression test recursively launches a multi-minute changed-fast pipeline. R0.2.g must make the default suite hermetic and repeatably load-stable without hiding failures behind ignores.
3. The new MCP/warm code has 14 Clippy warnings. R0.2.h makes warning-free all-target/all-feature builds part of the ordinary checkpoint, including newly generated modules.
4. The current work accumulated hundreds of dirty/untracked files without a remote checkpoint, so neither bisectability nor the rewritten GitHub CI has been proven. R0.1.a and R0.1.g are the first priority.
5. Generated state reached about 59 GiB and deterministic cleanup classified about 51 GiB as rebuildable. Cleanup safety is strong, but automatic quota/lease enforcement is still absent; R0.4.g owns bounded steady-state caches.
6. Startup and `fib` observations drifted backward while PB-5 map construction and PB-7 self-host parsing remain honestly below budget. R2.1 and R2.3 prioritize general runtime fixes and regressions over microbenchmark-specific shortcuts.
7. Workload-specific recognizers remain in `crates/gc_kernel/src/compiled_runtime/patterns.rs`. Separation is not retirement; R2.1.g requires their deletion from production execution and prevents equivalent benchmark dispatch from returning under another name.
8. MCP can suppress accepted in-flight responses when stdin closes, and concise human diagnostics are sometimes too terse. R1.3.e and R1.2.f close those lifecycle and usability gaps without creating a second semantic API.
9. The trust-infrastructure investment was necessary, but the project now has enough governance machinery. R0.4.h imposes consolidation, and milestone effort shifts to transactional agent sessions, corpus/held-out evaluation, the generated authoring skill, instruction/content separation, PB-5/PB-7, and product proofs.

---

## 4. Non-negotiable invariants

These invariants apply to every phase and every execution tier:

1. **Pure kernel:** G-lambda evaluation has no filesystem, time, randomness, network, process, environment, UI, model, or hidden global-state access.
2. **Unforgeable seals:** `UNHANDLED`, `EFFECT`, and `ERROR` are observable only through Prelude-created seal tokens. User terms cannot forge them.
3. **No panic on user input:** parse, evaluation, effect, bridge, package, registry, compiler, replay, and deployment boundaries return explicit internal errors or sealed language errors. Production `panic!`, `unreachable!`, unchecked indexing, and abort paths require mechanically proven non-user reachability.
4. **Deny by default:** every effect, host bridge, plugin, package hook, deployment action, model call, and inbound service operation requires explicit authority.
5. **Deterministic facts:** canonical terms/hashes, value hashes, ordering, paths, numeric behavior, effect logs, replay decisions, scheduling, artifacts, and bootstrap outputs are specified and versioned.
6. **Strict replay:** replay checks every serialized fact or the specification explicitly marks a field derived and recomputes it canonically. Unknown/missing fields fail closed.
7. **Bounded execution:** step, allocation, heap, payload, response, effect, concurrency, process, wall-clock, and artifact budgets have defined enforcement and failure semantics. Cooperative timeout alone is insufficient for blocking host work.
8. **Semantic identity across tiers:** treewalk, compiled AST, bytecode, WebAssembly, and any JIT produce identical observable values, hashes, effects, errors, and resource-accounting semantics, subject only to explicitly versioned performance counters.
9. **Optimizers remain outside trust:** optimization, specialization, JIT, and AI-generated rewrites require differential or translation-validation evidence against authoritative semantics.
10. **Compatibility is explicit:** canonical identity and serialized formats never drift silently. Every incompatible change has a profile/version bump, migration, dual-read window where safe, and deprecation record.
11. **Evidence cannot bless itself:** the implementation under test cannot be the sole authority that validates its own release claim. Critical formats have small independent verifiers and negative-control corpora.
12. **Self-hosting reduces semantic trust:** migrating code to `.gc` must remove or demote the prior authority. Duplicating semantics without retiring an authority does not advance closure.

---

## 5. Release ladder and critical path

| Milestone | Intended release | User-visible result | Hard gate |
|---|---|---|---|
| M0 Truthful Green Door | v0.2.x | Fresh clones build and checks report facts without mutating them | R0 |
| M1 Agent Preview | v0.3 | Agents receive a compact SDK, stable diagnostics, local warm/MCP interface, and scored training corpus | R1 |
| M2 Runtime Beta | v0.4 | Interactive, resource-bounded execution with validated bytecode and bounded long-lived processes | R2-R3 core |
| M3 Self-Host Authority Beta | v0.5 | Frozen core semantics above stage0 are GenesisCode-authoritative and bootstrap to a cross-host fixpoint | R5.1 and R5.3 core freeze, then R4 |
| M4 Platform Beta | v0.6 | Versioned language profile, declared stdlib/domain matrix, registry, and target builds work offline/self-hosted | R5-R6 |
| M5 Trust Release Candidate | v1.0-rc | Formal/fuzz/security evidence and real agent-authored flagship systems meet release SLOs | R7-R8 |
| M6 Trust Release | v1.0 | Reproducible artifacts, signed evidence, compatibility guarantees, and operational support are published | R9 |
| F1 Frontier Lab | post-v1 | Bounded self-improvement and optional validated JIT/research backends | F1-F4 |

Critical path:

```text
R0 truth/evidence
  -> R1 agent SDK + diagnostics + warm/MCP + training corpus
  -> R2 interpreter/resource/startup closure
  -> R3 validated bytecode and tiering
  -> R5.1 + R5.3 core semantic/profile freeze
  -> R4 semantic self-host authority + bootstrap fixpoint
  -> R5.2 + R5.4 + R5.5 platform completion
  -> R6 ecosystem/deployment
  -> R7 assurance
  -> R8 product proof
  -> R9 v1 release
```

Parallel, non-blocking lanes:

- Human UX can progress after R1 schemas stabilize.
- Registry and build-target prototypes can progress after R0, but cannot claim M4 before R5 compatibility is frozen.
- R4.1 ownership/TCB work and self-host prototypes can progress after R0; no R4.2 production-authority switch occurs before the relevant R5.1/R5.3 semantic contract is frozen.
- Formal models can start immediately and hard-gate R7.
- JIT research can start after the bytecode semantics are stable, but cannot displace critical-path work without failing a published performance decision gate.
- Additional capability families remain experimental until existing families meet the maturity and domain-proof rules.

---

## 6. Program-wide budgets

Budgets are acceptance contracts, not aspirations. R0 records the reference machines, raw samples, and confidence method. Budgets may be tightened freely. Loosening one requires a dated rationale, before/after evidence, and an explicit milestone decision.

### 6.1 Agent experience budgets

| ID | Budget | M1 target | v1 target |
|---|---|---|---|
| AB-1 | Core language card | <= 4,000 tokenizer-independent approximate tokens | <= 3,000 |
| AB-2 | Task-specific context bundle | <= 12,000 tokens including capability cards and examples | <= 8,000 |
| AB-3 | Warm request latency, no execution | p50 <= 5ms; p95 <= 20ms | p50 <= 3ms; p95 <= 10ms |
| AB-4 | Changed-file edit/check loop | p95 <= 2s on a 100-module workspace | p95 <= 1s |
| AB-5 | Reference-agent parse success | >= 95% first attempt on public test split | >= 98% |
| AB-6 | Diagnostic-guided repair | >= 85% recovery within two repair turns | >= 95% |
| AB-7 | Held-out task completion | >= 8/10 core archetypes without private repo context | >= 9/10 across two independent model families |
| AB-8 | Structured-output stability | 100% protocol/schema conformance or explicit typed failure | 100% |

Agent benchmarks always publish model/runtime version, decoding settings, context, seeds where available, attempts, failures, and cost. A model result never substitutes for deterministic compiler/runtime conformance.

### 6.2 Runtime budgets

| ID | Budget | Current E0 observation | Target |
|---|---|---|---|
| PB-1 | Interpreter `fib(25)` strict workload | signed baseline upper median-confidence bound about 211ms; independent later observation about 220ms | <= 250ms, then ratchet |
| PB-2 | Bytecode `fib(25)` | not implemented | <= 40ms |
| PB-3 | Optional validated JIT `fib(25)` | not implemented | decision-gated; <= 8ms if built |
| PB-4 | 1M-element vector construction | about 6ms on smaller audited workload; normalize corpus | <= 150ms on normative workload |
| PB-5 | 100k-entry map construction | signed baseline about 489ms; failing | <= 300ms interpreter |
| PB-6 | Cold CLI parse/check trivial module | process startup about 285ms; deterministic snapshot not implemented | <= 30ms with deterministic snapshot |
| PB-7 | Self-host parse throughput | signed lower bound about 42.8 KiB/s; failing | >= 1 MiB/s, then ratchet |
| PB-8 | Warm daemon leak | not established | <= 5% RSS growth after 100k bounded requests and quiescence |
| PB-9 | Semantic tier parity | broad partial gates exist | 100% normative corpus, all values/effects/errors/hashes |
| PB-10 | Bootstrap fixpoint | not established cross-host | byte-identical stage2/stage3 on all tier-1 hosts |

### 6.3 Engineering and gate budgets

| ID | Budget | Target |
|---|---|---|
| GB-1 | Static policy/doc checks | <= 15s warm, no compilation |
| GB-2 | Default changed-file gate | <= 2 minutes warm; no network; <= 1 GiB additional disk |
| GB-3 | Standard local pre-commit profile | <= 8 minutes warm; <= 3 GiB additional disk |
| GB-4 | Full release profile | <= 45 minutes on reference CI shard set; peak workspace artifacts <= 20 GiB |
| GB-5 | Clean development footprint | <= 8 GiB after a normal workspace build; `genesis clean --cache-policy dev` returns to <= 2 GiB generated state |
| GB-6 | Evidence verification | <= 5 minutes offline with no compiler invocation for an already-built release bundle |
| GB-7 | Crate/module concentration | no production Rust source file > 1,000 lines without exemption; no semantic crate > 20k lines by M3 |
| GB-8 | Fresh-clone prerequisites | one checked manifest; no undeclared Python package/module assumptions |

### 6.4 Assurance budgets

- Zero known P0/P1 trust-boundary defects at every milestone.
- Zero user-reachable panic/abort paths in release profiles.
- 100% negative-control rejection for seals, capabilities, replay tampering, package signatures, bootstrap artifacts, and evidence manifests.
- At least 30 continuous days of sanitizer/fuzz/property soak with no unresolved high-severity finding before v1.
- All tier-1 release artifacts reproduced by two independent builders; at least one uses an independently maintained verifier.

---

## 7. Execution protocol

1. Work the critical path in phase order unless a task explicitly names an earlier-safe parallel lane.
2. Before implementation, add or identify the failing test, negative control, benchmark, or proof obligation.
3. Prefer library behavior over CLI special cases and GenesisCode-authored semantics over new host semantics.
4. Keep the pure reference evaluator readable and authoritative until a published self-host/TCB decision changes that authority.
5. Run `bash scripts/test_changed_fast.sh` for ordinary changes plus the phase-specific checks. Full workspace/release profiles run only where their manifest says they are required.
6. Never hand-edit generated evidence or generated matrices. Use an explicit update command, inspect the diff, then run a read-only check.
7. Record wall time, peak RSS, disk delta, cache state, and network access for every gate profile while R0 is active.
8. If a gate is broken, add it to `upgrade_plan.md` as an active P0/P1 defect with a roadmap back-reference. Strategic unfinished work stays here.
9. If implementation reality contradicts this plan, fix the plan and evidence ledger in the same reviewed change. Do not preserve an inaccurate checkbox.
10. Commit every coherent green checkpoint in task-scoped chunks and push it to a reviewed remote before starting the next high-risk tranche. Dirty-state evidence may be useful, but the local worktree may never be the only copy. Do not combine semantic format changes, migrations, and unrelated optimization.
11. Default test profiles never invoke another governed aggregate pipeline and never absorb stress, soak, or performance loops. A test that fails under supported parallel load is a defect to reproduce and fix, not an ignore candidate.
12. Until M1, net implementation effort must favor R1.3-R1.6, PB-5/PB-7, and executable agent product proof. New governance entrypoints require proof that an existing schema, gate, or negative-control matrix cannot be extended and normally replace or consolidate at least one equivalent mechanism.

---

## R0. Truthful Green Door and evidence authority

**Goal:** make the repository, planning state, checks, evidence, versions, and resource costs tell the truth from a clean clone.

### R0.1 Canonical planning and status

- [x] **R0.1.a Track this roadmap.** Add `ROADMAP.md` to repository history, retain its README/index registration, and record the replacement mapping in section 11. Evidence: clean-clone visibility and planning-doc gates. done 2026-07-14; evidence: `scripts/check_planning_docs_fresh.sh`, `scripts/check_doc_topology_drift.sh`, and `scripts/check_capability_evidence_ledger_contract.sh`; input: `git-commit-object-sha256:1a0d9bdc8d967af45835abeb4391a1601ab6e9e56cc6a16db35882ee198b527e`
  Evidence 2026-07-14: a network-fetched single-branch clone of the published revision resolved exactly to `43dfb0e02fc612a6636b49636fbc676426ac59ab`, contained tracked `ROADMAP.md` blob `69b038a8cd53c65658ed5ed771a75601f79c9e34`, retained the README and `docs/INDEX.md` registrations, and retained section 11's complete prior-area replacement table. Planning freshness, documentation topology/reference freshness, the capability/evidence ledger with all 42 roadmap evidence identities, and the 247-task execution-manifest contract passed read-only; the clone remained clean after verification.
- [x] **R0.1.b Replace binary feature status.** Convert `feature_matrix.md` to the L0-L5 maturity model, include supported-host scope, and replace "Known gaps: none" with generated gaps from the ledger. done 2026-07-10; evidence: `scripts/check_capability_evidence_ledger_contract.sh`; input: `capability-ledger-bundle-sha256:57d05b05b508f7beaa7db4298632f25b07164a5adbba3030a9f465db20517724`
- [x] **R0.1.c Create a capability/evidence ledger.** Add a schema and source file mapping each claim to spec, implementation authority, host binding, tests, maturity by platform, owner, and immutable evidence IDs. done 2026-07-10; evidence: `scripts/check_capability_evidence_ledger_contract.sh`; input: `capability-ledger-bundle-sha256:57d05b05b508f7beaa7db4298632f25b07164a5adbba3030a9f465db20517724`
- [x] **R0.1.d Generate views.** Generate `feature_matrix.md`, self-host status, compatibility tables, and release claim tables from the ledger. Drift checks compare generated output without rewriting it. done 2026-07-10; evidence: `scripts/check_capability_evidence_ledger_contract.sh`; input: `status-authority-bundle-sha256:97cabb8e772c3978006c57ad11e9ebf7b5b889c7fd5799d0e6b3a8c3173a815b`
- [x] **R0.1.e Reconcile planning docs.** Keep `upgrade_plan.md` as the active red-team defect queue, this file as strategic sequence, and status docs as generated/audited views. Remove contradictory hand-maintained status claims. done 2026-07-10; evidence: `scripts/check_selfhost_doc_runtime_parity.sh` and `scripts/check_redteam_report.sh`; input: `status-authority-bundle-sha256:97cabb8e772c3978006c57ad11e9ebf7b5b889c7fd5799d0e6b3a8c3173a815b`
- [x] **R0.1.f Add a machine-readable execution manifest.** Mirror every roadmap task ID into a versioned manifest with prerequisites, risk class, expected inputs/outputs, owning spec/implementation surfaces, required checks and negative controls, resource class, rollback, and acceptance evidence. A drift check proves one-to-one coverage; the manifest schedules approved work but cannot mark its own task complete or override this roadmap's definition of done. done 2026-07-10; evidence: `scripts/check_roadmap_execution_manifest.sh`; input: `roadmap-execution-contract-bundle-sha256:053b3ebc6a44c422b35e9e9e2376980a5dbbcd0008f6f82f7de18a7f658011e1`
- [x] **R0.1.g Establish checkpoint and remote-CI discipline.** Publish each coherent task-scoped checkpoint before the next high-risk tranche, require a clean-clone reconstruction from the published revision, run the complete GitHub CI matrix at least once as a shakedown, classify every remote-only failure, and protect release branches from bypassing required checks. A draft pull request is a valid backup checkpoint but cannot close this task until its exact revision is green remotely. done 2026-07-14; evidence: `.github/workflows/ci.yml`, `.github/workflows/docs-site.yml`, and `scripts/check_docs_quickstart.sh`; input: `git-commit-object-sha256:1a0d9bdc8d967af45835abeb4391a1601ab6e9e56cc6a16db35882ee198b527e`
  Evidence 2026-07-14: task-scoped commits were published through PR 1 and the exact head revision passed the complete push CI run `29383561115` with all 63 executed stages successful. The same revision passed PR-only strict selfhost/WASM equivalence, deterministic GPU-device, WebXR browser, and 170-page Quarto/Playwright lanes; the only remote-only defect found was the documentation-maintainer shell fence being executed as a language quickstart, fixed by the explicit `genesis-doc-skip` contract and revalidated locally and on Linux. A fresh network clone independently reconstructed the revision and passed the R0.1 checks. GitHub exposes exactly one default branch, `main`; it requires an up-to-date `test` status, enforces protection for administrators and linear history, and disables force pushes and branch deletion.

### R0.2 Evidence lifecycle

- [x] **R0.2.a Separate check from update.** Audit every `check_*.sh`; a check fails with an exact update command when required material is absent or stale. It never invokes `update_*`, writes `.genesis/perf`, warms caches, or compiles unless compilation is its declared subject. done 2026-07-10; evidence: `scripts/check_check_update_boundary.sh`; input: `check-update-boundary-audit-sha256:08c0082749a597997094a308f9e1645191353df0f092cf441622db33cce2913b`
  Progress 2026-07-10: the reachable-helper audit covers all 114 check entrypoints and their sourced/executed local closure; all 114 are compliant, no legacy persistent-output producers remain, all 45 conservatively detected compilation closures and the single network subject are declared, and refresh/update/mutation-helper counts are zero. The permanent smoke suite enforces 51 renderer contracts, 45 heavy-wrapper contracts, missing-output rejection, caller-output-override rejection, retained-tree immutability, and maintenance-mutation rejection. The final aggregate health migration uses input-only copied histories, private check reports, no persistent gate-result cache, no explicit Cargo prewarm, and an explicit retained updater. Write-skill conformance retained 100/100 across 28 rubric rows and 100 generative cases with hashed portable inputs; tracked source decomposition passed 9/9 reviewed parity commands, enforces calendar-valid non-expired waivers, and canonicalizes diagnostic host paths; the release-scale 10,000-module workspace passed lock/build/test/selfhost refresh in 16.553 seconds with a byte-identical retained tree during the check and portable retained output from the updater. The parity run exposed and fixed stage2 handling of slim `Value::Int`; all 117 `gc_opt` library tests pass. Full-cross, runtime-backend, GCPM target-runtime, AI SLO/stress, GPU/XR, task, bridge, assurance, package/deployment, in-toto/DSSE/SLSA profile, independent evidence-verifier, adversarial evidence/replay, immutable evidence-storage, dependency-mirror, version-surface, v1 compatibility-registry, gate-manifest, resource-telemetry, and deterministic-cleanup lanes retain the same check/renderer/updater boundary and hashed portable inputs.
- [x] **R0.2.b Version the evidence manifest.** Profile in-toto Attestation Framework v1.2 with Statement v1 and a pinned authenticated envelope/bundle representation, plus SLSA 1.2 build/source provenance where its semantics fit; define versioned Genesis predicates for source revision, dirty-state policy, toolchain hashes, environment profile, declared network inputs, commands, negative controls, artifact hashes, raw samples, durations, peak RSS, disk delta, and verifier version. done 2026-07-10; evidence: `scripts/check_genesis_evidence_profile.sh`; input: `genesis-evidence-profile-bundle-sha256:1c0d3c7305ccdd70e2dc0bf5177a08a2598e5e6f57a74457e827d496af5ddaf0`
  Evidence 2026-07-10: four Draft 2020-12 schemas define the closed Genesis predicate, Statement v1 profile, strict SLSA Provenance v1 producer profile, and DSSE bundle. The deterministic E3 vector binds identical Genesis/SLSA subjects, canonical statement bytes, DSSE PAE, and two valid Ed25519 signatures from a public non-trust fixture key. Sixteen in-memory adversarial controls plus a duplicate-key file fixture reject version/type confusion, implicit dirty inputs, host paths, shell strings, undeclared network access, failed controls, floats, subject/payload substitution, missing authentication, malformed signatures, and statement reordering. Normative policy separates SLSA build provenance from source-only evidence, signature validity from authorization, and authenticated measurements from semantic identity.
- [x] **R0.2.c Add an independent verifier.** Build a small read-only verifier for the profiled in-toto/SLSA envelopes, Genesis predicates, hash trees, signatures, compatibility profiles, and negative-control outcomes. Keep it outside the main CLI dependency graph and publish test vectors. done 2026-07-10; evidence: `scripts/check_genesis_evidence_verifier.sh`; input: `genesis-evidence-verifier-bundle-sha256:d0f8ed72e85d3293eecac0dcdfa5f12d669f9eee410777fc6446cc3bcc828004`
  Evidence 2026-07-10: the nested standalone Rust workspace has an independent lockfile, no Genesis crate or root-workspace membership, no production signing/write/network path, exact duplicate-key and integer-only JSON parsing, externally pinned policy SHA-256, Ed25519 DSSE role/threshold verification, strict Genesis/SLSA compatibility and companion linkage, bounded inputs, normalized non-symlink artifact paths, streamed SHA-256, and a domain-separated Merkle tree. Three additional Draft 2020-12 schemas cover trust policy, artifact tree, and negative vectors. Four positive artifacts verify twice to byte-identical reports; all 30 published re-signed or structural adversarial cases fail at their expected boundary, and an authenticated future-field fixture proves SLSA monotonic extension compatibility. The permanent offline gate runs format, Clippy with warnings denied, six Rust tests, dependency separation, production capability scans, deterministic reports, and retained-tree immutability.
- [x] **R0.2.d Establish evidence storage classes.** Keep E0 observations under ignored `.genesis/`; keep E1/E2 schemas and goldens in-tree; publish E3/E4 bundles as immutable release assets with mirror instructions. done 2026-07-10; evidence: `scripts/check_evidence_storage_classes.sh`; input: `evidence-storage-classes-bundle-sha256:a84f3998bacd9227a4822d005cc25a3a1497c8a6df277ae97bf6df5bc55c41af`
  Evidence 2026-07-10: a closed E0-E4 policy and exact generated fixture catalog keep mutable observations ignored, schemas/examples at E1, and every retained signed fixture at non-authoritative E2 regardless of its claimed profile. The E3/E4 renderer requires an independently verified matching profile and class threshold, emits normalized deterministic USTAR only below ignored release roots, excludes trust roots, binds exact payload hashes and verifier policy, names assets by SHA-256, and creates sidecar plus executable mirror instructions. The E3 conformance candidate renders byte-identically twice, verifies without extraction, and mirrors create-new with byte parity. Eight controls reject overwrite, mirror replacement, E3-to-E4 escalation, non-release classes, archive mutation, duplicate policy keys, fixture authority escalation, and archive traversal. E4 policy requires two signatures and two mirrors; no E4 release is claimed until those independent external controls exist.
- [x] **R0.2.e Add adversarial fixtures.** Reject missing fields, duplicate keys, path aliases, unsupported schema versions, reordered/altered replay facts, forged signatures, stale source identities, and reports produced from dirty inputs without an explicit dirty policy. done 2026-07-10; evidence: `scripts/check_evidence_adversarial_matrix.sh`; input: `evidence-adversarial-matrix-bundle-sha256:3fbeef44b18a1155d32b27445395bb292d8448bb9c2a62f69d736ef09350cb4f`
  Evidence 2026-07-10: a Draft 2020-12 schema and reviewed matrix bind all eight requirements to eleven exact controls with one-time coverage, source authority, fixture, command, and expected diagnostic. The standalone verifier now rejects 30 published cases, including compound `./` path aliases, missing fields, unsupported bundle versions, malformed dirty declarations, missing dirty-path identities, and repository/revision/tree identities that differ from the out-of-band trust policy. The runtime replay control mutates 16 executable facts spanning order/count/index, operation and request/response hashes, response body, decision/cap, and every deterministic scheduler field. The offline aggregate gate rejects duplicate matrix keys, orphaned or multiply referenced controls, fixture/diagnostic drift, and failures at the wrong boundary.
- [x] **R0.2.f Restore denied-effect replay parity.** Make denied capability decisions produce the same value/error boundary, exit contract, canonical log facts, and replay outcome across Rust and self-host engines on native and WASI. Add one shared regression corpus for allow, deny, unhandled, malformed, and tampered entries; compare every serialized fact including decision and capability; fail closed on any engine disagreement. done 2026-07-14; evidence: `scripts/check_evidence_adversarial_matrix.sh`; input: `evidence-adversarial-matrix-bundle-sha256:3fbeef44b18a1155d32b27445395bb292d8448bb9c2a62f69d736ef09350cb4f`
  Evidence 2026-07-14: replay command success is now defined by complete log verification rather than by whether the faithfully reconstructed terminal value is a sealed ERROR. The shared runtime corpus covers allow, deny, allowed-but-unhandled operations, malformed unsealed requests, reordered/count-altered logs, and mutation of index, op, payload/continuation/request/response hashes, response body, decision, capability, and every scheduler field. The governed gate passes all 30 replay-focused library cases and both six-case Rust/selfhost mirrors on native and WASI; denied runs retain exit 41 and byte-identical canonical logs, while verified denied replays return the same sealed value and exit successfully.
- [x] **R0.2.g Make test profiles hermetic and load-stable.** Remove aggregate-pipeline recursion from default Rust tests; place process-kill loops, performance probes, and dirty-tree stress in declared isolated lanes. Replace PID-log timing assumptions with explicit fixture readiness/ownership handshakes, prove descendant kill/reap deterministically, and run the affected host-bridge and agent-plan tests repeatedly under supported parallel CPU/I/O load. No closure through `ignore`, retry-until-green, reduced assertions, or serialized global execution unless the contract itself requires serialization. done 2026-07-14; evidence: `scripts/check_test_execution_profile_matrix.sh`, `scripts/check_host_bridge_fault_injection.sh`, and `scripts/check_gc_agent_task_cards.sh`; input: `test-hermeticity-bundle-sha256:70507631bee3bfbce4ac89ab9be0a7adf4758e0aae8a05bac77d06d90afdcf4d`
  Evidence 2026-07-14: the nested changed-fast probe and both hard-cancellation loops are excluded from default Rust tests but are non-vacuously executed by their declared perf/stress gates. Spawn and persistent bridge timeouts terminate and reap process groups, perform a bounded post-reap sweep for fork/zombie races, quiesce I/O pumps/workers, reject uncertain retries, and recover for healthy calls; three consecutive 151-test parallel-load runs plus the dedicated gate passed without retries. Agent-plan parity now compares Rust and Python over one immutable generated registry snapshot while the separate freshness check binds that snapshot to authorities; the permanent gate passes 16 concurrent selector pairs. Default effect, planner, and shell suites pass with aggregate/stress cases skipped only where their dedicated gates execute them.
- [x] **R0.2.h Enforce a warning-free workspace.** Run format plus Clippy with warnings denied for all workspace targets and supported feature profiles, including MCP, warm, WASI, build scripts, tests, examples, and generated Rust. Remove the current warnings at their cause; any lint suppression must be narrow, justified, reviewed, and covered by a negative control that prevents module-level or workspace-wide suppression. done 2026-07-14; evidence: `cargo clippy --workspace --all-targets --locked --offline -- -D warnings`, `scripts/check_runtime_backend_feature_matrix.sh`, and `scripts/check_test_execution_profile_matrix.sh`; input: `warning-free-workspace-bundle-sha256:df92b0a330e3a6735dc5b1df97abdcececf64224b23e171d6d97b48deaa81ff6`
  Evidence 2026-07-14: all workspace targets, MCP/warm paths, WASI CLI, tests, build scripts, standalone evidence producer/verifier, and the four mutually exclusive headless/GPU/graphics/combined runtime profiles compile offline with warnings denied. The fourteen observed MCP/warm diagnostics and three inherited package-report exceptions were removed at their API-shape causes using typed option/request records, named MCP annotation policies, and direct worker-state predicates; no Clippy suppression remains. The existing backend matrix now owns thirteen non-duplicative warning/semantic stages and its end-to-end package-environment test proves the exact active profile. Seven permanent negative controls reject module, workspace, broad, reasonless, and build-flag suppression while permitting only a narrow reviewed `#[expect(..., reason = "...")]` escape hatch.

### R0.3 Hermetic bootstrap and versions

- [x] **R0.3.a Fix the root-lock check.** Remove the undeclared Python 3.11 `tomllib` assumption from `scripts/check_root_lock_policy.sh`; use a Rust helper, a POSIX-safe parser for the required subset, or a checked fallback that works with the declared minimum Python. done 2026-07-10; evidence: `scripts/check_root_lock_policy.sh` (`parser=posix-awk`, three negative controls) and `scripts/check_green_front_door.sh`; input: `root-lock-policy-bundle-sha256:da5bce85a48b6e89b160c9ea720356f00ba3a63384978a6cc08d26d0c9cdc059`
- [x] **R0.3.b Declare one prerequisite manifest.** Pin Rust, WASI/WebAssembly tools, Lean, shell/Python minimums, platform SDK expectations, and optional feature tools. Add a command that reports missing or mismatched prerequisites without mutating the host. done 2026-07-10; evidence: `scripts/check_prerequisite_manifest.sh`; input: `prerequisite-manifest-bundle-sha256:b69bfe93745e6508c15ac547becf003441764087b3c854e70c3c0978fa09bc23`
  Evidence 2026-07-10: `genesis.prerequisites.json` is the single semantic authority for nine profiles, four tiered host-platform SDK envelopes, and 27 probeable tool identities. It pins Rust/Cargo/components/targets, Bash and Python floors, Git, Node/npm/Playwright, wasm-bindgen, maintained Wasmtime, Lean/Lake, CI tools, fuzz tooling, and Apple/Android device helpers while checking tool-native mirrors in `rust-toolchain.toml`, npm locks, and CI. The read-only doctor emits deterministic JSON or human diagnostics, separates required failures from optional gaps, and only executes an implementation-sealed allowlist of bounded version/presence argv. Seven controls reject duplicate keys, arbitrary or mutating probes, general source drift, Rust-target mirror drift, incomplete profiles, unknown profiles, and unsupported platform selection; repeated core reports are byte-identical and retained inputs remain unchanged.
- [x] **R0.3.c Verify clean and offline modes.** A fresh clone may fetch declared dependencies once; the offline profile then builds/checks from a content-addressed dependency mirror. Undeclared network access fails. done 2026-07-10; evidence: `scripts/check_dependency_mirror_contract.sh` and `scripts/test_offline_dependency_mirror.sh`; input: `dependency-mirror-bundle-sha256:be1a7b84bbb6e5f6dca7edceb566df7d4f868a533b588ec52f3e6d25df355201`
  Evidence 2026-07-10: the closed policy covers six authorities, two Cargo workspaces, 483 lockfile records converging to 443 unique registry payloads, and three npm SRI-addressed tarballs. Preparation invokes sealed direct argv, rejects non-crates.io or unlocked Cargo sources and npm origin/SRI/redirect drift, normalizes 864,570,518 expanded bytes and 26,498 files into a 103,712,661-byte canonical archive, and installs create-new by canonical-manifest SHA-256. An empty `CARGO_HOME` network fetch and a warm-cache fetch converged on mirror `62b01f75ac64665463886de0ffe55dde30b9c5873f3974cd7b27ffb1ec962b37`; the ignored retained mirror occupies about 116 MiB. From clean Git inventory, empty Cargo/npm caches, and a new target directory, the offline profile built `gc_cli`, checked all workspace targets plus the independently locked verifier, installed/imported Playwright from local blobs, and preserved source authorities and mirror bytes. Every command used explicit offline/locked flags behind a live-canary-proven Darwin kernel sandbox; Linux full CI uses a fail-closed network namespace, while Windows remains explicitly unsupported rather than emitting false evidence. Eleven fixture controls plus ten internal controls reject policy, source, integrity, path, generated-manifest, archive traversal/link/duplication/bounds, and isolation downgrades; the fast gate is retained-input read-only and the full CI profile runs fetch-once followed by hard-isolated reconstruction.
- [x] **R0.3.d Align version surfaces.** Reconcile crate versions, CLI output, package schema, GCLOG writer/parser/spec, canonical hash profile, compiled artifact magic, docs, and release metadata. Add migration notes for every dual-read format. done 2026-07-10; evidence: `scripts/check_version_surfaces.sh`, `scripts/check_versioning_release_hygiene.sh`, and `scripts/check_release_smoke.sh`; input: `version-surface-bundle-sha256:7048c82df3ab75c2526def68ddd2bf3e8ffcb38055f568765da2ba0d1e503315`
  Evidence 2026-07-10: a closed ten-surface registry separates release, package, workspace, lock, GCLOG, GPK, canonical hash, compiled-module, selfhost-cache, and selfhost-artifact identities. Current writers emit package schema 1, lock v2, GCLOG v3, and GPK v2 even with zero refs; explicit legacy package, lock v1, GCLOG v2, and GPK v1 readers are governed by four migration records, while missing lock/workspace/GCLOG and all unknown future versions fail closed. One exported canonical hash prefix preserves existing bytes; compiled and selfhost cache magics reject obsolete identities. Thirty-three maintained manifests are explicit schema 1, both production CLIs report `genesis 0.2.0`, Python 3.9 needs no undeclared TOML module, six registry adversarial controls pass, byte-level migration tests pass, and the docs ceiling remains 132 after consolidation.
- [x] **R0.3.e Create the v1 compatibility registry.** Reserve stable IDs for language profile, CoreForm, value/effect hash, log, evidence, package, patch, bytecode, snapshot, and bootstrap formats. done 2026-07-10; evidence: `scripts/check_v1_compatibility.sh`; input: `v1-compatibility-registry-bundle-sha256:633998c63afbb4995fba3b8f1a3489b5f1b0e9ee54114e0c7e11733dde4a6022`
  Evidence 2026-07-10: `genesis.compatibility.json` permanently reserves ten exact `genesis/compat/v1/*` identities while explicitly claiming `reserved-not-stable`; nine bind to source-checked pre-v1 candidates and bytecode remains unbound. A closed Draft 2020-12 schema and normative lifecycle define semantic/wire classes, the acyclic dependency contract, writer/readers and migrations per component, monotonic promotion, retirement policy, and seven mandatory R9 freeze requirements. Runtime authorities now name language, CoreForm, hash, log, evidence, package, semantic/VCS patch, snapshot, and bootstrap candidates without changing existing bytes; semantic patches reject absent versions and bootstrap seed reuse rejects absent or mismatched versions. The read-only Python 3.9 gate checks exact reservations, source drift, schema closure, migration links, path portability, candidate/stable separation, complete promotion coverage, and 13 adversarial controls. CI, release hygiene, and the green front door enforce it; the check-boundary audit remains 114/114 compliant, focused tests pass, and all workspace targets compile.

### R0.4 Gate architecture and disk lifecycle

- [x] **R0.4.a Add a gate manifest.** Describe gate inputs, outputs, dependencies, profile, expected duration, disk budget, network policy, platform scope, sharding, and whether the gate is static, build, test, benchmark, proof, or release-only. done 2026-07-10; evidence: `scripts/check_gate_manifest.sh`; input: `gate-manifest-bundle-sha256:a5a46654fdad43d1d08f9e7318f26e3eb39b347fc0c93dd9d1d51a2c1856457f`
  Evidence 2026-07-10: `genesis.gates.json` resolves all 114 governed checks into 47 static, four build, 33 test, 17 benchmark, three proof, and ten release-only gates with 59 acyclic direct dependency edges. Every entry declares source and execution SHA-256, a transitive exact repository-input identity covering non-shell helpers, conservative input sets, read-only output classes, minimum profile, expected duration, disk envelope, prerequisite tools, tier-1 platform scope, network mode, and shard/cache isolation. All 45 compiling closures declare rebuildable Cargo output, all benchmarks require isolated worktree/cache execution, and only `check_selfhost_boundary.sh` has optional external network with a named Git input. A reviewed policy, closed Draft 2020-12 schema, explicit updater, and byte-equivalent Python 3.9 checker independently rediscover the live inventory and recompute source/helper identities; the generated self-input uses one documented domain-separated exclusion to avoid a hash fixed point. Fourteen adversarial controls reject inventory, identity, dependency, path, network, write, budget, profile, isolation, output, and duplicate-key violations. The check is retained-input read-only, deterministic across repeated renders, completes within its 30-second local-fast planning envelope, is enforced by CI/profile-matrix/green-front-door gates, and defers history-backed resource envelope enforcement to R0.4.f while R0.4.d supplies measured observations.
- [x] **R0.4.b Eliminate accidental rebuild islands.** Use one declared Cargo target/cache strategy per profile; stop scripts from creating redundant nested targets; key caches by toolchain/features/input hashes. done 2026-07-10; evidence: `scripts/check_cargo_target_dir_policy.sh`; input: `cargo-cache-bundle-sha256:023d0f79d5262de3d4134b6b37e47657e5ace32305592351e15e0243e687e0f4`
  Evidence 2026-07-10, strengthened 2026-07-14: the closed `policies/cargo_cache_v0.1.json` policy and schema declare four semantic scopes: root host, root WASI, root browser/Node Wasm, and the independently locked evidence-verifier host workspace. The canonical SHA-256 key binds strategy, resolved target triple, pinned and observed Rust identities, every declared manifest, lock/config/toolchain bytes, feature and Cargo-profile definitions, and six build-affecting environment inputs while deliberately leaving source edits to Cargo's incremental fingerprints. Sixty-five helper-using scripts and all 58 source-detected Cargo scripts now converge by scope rather than report, stress, release, or health-profile name; all Rust CI jobs resolve the same policy before `rust-cache` or raw Cargo, and Wasm/WASI transitions are explicit. The docs gate executes inside a locked stable runtime mirror but rewrites Cargo manifest selection to the authoritative checkout, preserving hermetic runtime writes while sharing exact source and target identities. Script-specific target overrides, direct exports, legacy build paths, and arbitrary inherited targets are absent and fail closed; only `GENESIS_CARGO_CACHE_ROOT` may relocate the hierarchy without changing the key. Canonical path-free metadata detects stale or colliding materializations. Twenty adversarial controls execute the resolver in a Git-initialized checkout with no `.genesis` directory, proving it safely creates the reviewed root, writes its producer marker before admission, materializes the cache, and leaves zero active leases for non-shell output modes. Shell mode transfers a process-bound lease to the actual Cargo caller; JSON/path/GitHub-environment modes use a self-owned transient lease and release it after materialization, preventing long-lived CI parent processes from pinning an 8-GiB reservation. Parent symlinks and conflicting roots still fail closed. The cleanup command never invokes Cargo or creates the cache it is reclaiming. Size-budget enforcement remains owned by R0.4.f.
- [x] **R0.4.c Implement changed-impact selection.** Map files and generated schemas to affected crates/gates, conservatively falling back to broader profiles when dependency certainty is incomplete. done 2026-07-10; evidence: `scripts/check_changed_impact.sh`; input: `changed-impact-bundle-sha256:b67aba0bf1e5adfde2e37d6351a6acee7f2089f7e3b52b4fb349380002363e0a`
  Evidence 2026-07-10: `policies/changed_impact_v0.1.json`, its closed schema, and `scripts/lib/changed_impact.py` replace the hard-coded direct-crate/CLI heuristic with a deterministic plan that binds policy and gate-manifest identities. Locked offline Cargo metadata supplies all 18 workspace packages and path dependencies; changes compute the complete reverse dependency closure, so `gc_coreform` reaches all 18 crates while leaf `gc_wasm` remains precise. Exact gate inputs, declared input-set globs, and reverse gate dependencies produce separate direct and full semantic gate closures. Generated schemas/views, workspace/toolchain/CI authorities, unknown or relationship-free paths, and crate/gate sets above closed cardinality limits fail broad to `prepush-standard`; no unmatched path can silently become a docs-only no-op. Git collection unions committed divergence, index, unstaged/deleted files, both rename endpoints, and non-ignored untracked files; the current dirty worktree correctly reports 620 paths instead of the former two committed-only paths. Canonical UTF-8 repository paths reject traversal, absolute, dotted, and backslash aliases. Changed-fast executes the machine plan, records its SHA-256 plus affected counts/fallback profile, and retains deterministic override isolation for tests. Ten controls prove root/leaf graph closure, generated-schema and unknown-path escalation, malformed-path rejection, order/duplicate independence, host-path exclusion, complete four-state Git collection, closed schema/policy behavior, and duplicate-key rejection. CI, profile-matrix, and green-front-door entrypoints enforce the selector; the ambiguous two-file governance regression now escalates rather than choosing an arbitrary subset.
- [x] **R0.4.d Add resource telemetry.** Every gate emits duration, peak RSS, bytes read/written, generated disk delta, cache hits, and network attempts in a common schema. done 2026-07-10; evidence: `scripts/check_gate_resource_telemetry.sh`; input: `gate-resource-telemetry-bundle-sha256:3c322976425ddd636a31ded07e0027a5e9311bac77fe7fdf96ade1c796afcb1b`
  Evidence 2026-07-10: all 114 governed checks carry an exact top-of-file wrapper and re-execute under a parent observer; entrypoint-scoped recursion suppression preserves one record for aggregates and one for every nested governed gate. A closed Draft 2020-12 schema requires gate ID, execution identity, platform, exit result, and all seven metrics with explicit units, methods, and `exact`, `instrumented`, `sampled`, `estimated`, or `unavailable` fidelity, preventing numeric zero from masquerading as proof. Monotonic duration is exact; process-tree RSS and Linux procfs I/O are sampled; Darwin I/O is labeled as a block-operation estimate; default disk delta is a constant-time sampled allocation observation with an opt-in exact logical scan of policy-declared generated roots. Parent-created bounded event channels account for content-addressed Cargo cache reuse and declared network attempts without entering semantic logs, replay identity, package identity, or generated artifact hashes. Fourteen controls cover complete preamble placement, nested observation, pass/fail/signal preservation, real cache-hit instrumentation, event count and line bounds, malformed/duplicate input, canonical paths, host-path exclusion, and cleanup after malformed events or launch failure. CI, the profile matrix, and the green front door enforce the local-only, remote-collector-free contract; ordinary checks retain no telemetry file.
- [x] **R0.4.e Add deterministic cleanup.** Provide dry-run and execute modes that classify rebuildable outputs, retained evidence, dependency mirrors, and user-authored data. Never delete untracked user files by pattern alone. done 2026-07-10; evidence: `scripts/check_deterministic_cleanup.sh`; input: `deterministic-cleanup-bundle-sha256:ff0ab8df0a2a8f8dc4d902175d56d211b9764b6611337cf3f7c58ad80f12db5b`
  Evidence 2026-07-10, strengthened 2026-07-14: one closed policy enumerates fourteen exact roots across `rebuildable-output`, `retained-evidence`, `dependency-mirror`, and permanently non-deletable `user-authored`, with four explicit cleanup profiles and unknown `.genesis` children defaulting to user-authored preservation. Four closed Draft 2020-12 schemas govern policy, producer marker, canonical plan, and execution result. Deletion requires both reviewed root identity and a policy-bound producer marker; Cargo and dependency-mirror producers mark only their owned roots, explicit initialization rejects protected, tracked, symlinked, escaped, or cross-filesystem roots, and path/age/size/glob/untracked status alone grants no authority. Dry-run is repository-read-only, timestamp-free, host-path-free, and byte-identical for unchanged trees. Execute binds an exact canonical plan hash, revalidates every class, tree, marker, and tracked path, then atomically quarantines all selected roots before physical deletion with termination signals blocked. Whole-tree deletion retries a bounded eight times when host indexers recreate metadata between traversal and `rmdir`; continuous mutation still fails closed and leaves only quarantined rebuildable state. Forty-one controls include explicit transient metadata recreation, transactional rollback, stale-plan and confirmation binding, marker/symlink/mount protection, generated-state admission and cleanup races, crash recovery, and protected-class preservation. A reviewed real-checkout plan reclaimed 33.5 GB allocated from `.genesis/build` and legacy `target`, preserved mirrors, evidence, package state, unmarked roots, and user data, and returned total `.genesis` state to 114 MB. CI, profile-matrix, and green-front-door gates enforce the contract.
- [x] **R0.4.f Enforce GB-1 through GB-8.** Split or redesign gates that exceed budgets. The no-panic/TCB policy check must not require a multi-minute cold build merely to scan source policy. done 2026-07-10; evidence: `scripts/check_engineering_gate_contract.sh`, `scripts/check_upgrade_plan_health.sh --profile prepush-standard`, `scripts/check_gate_resource_telemetry.sh`, and `scripts/check_cargo_target_dir_policy.sh`; input: `engineering-gate-budget-bundle-sha256:41301241dacee53d6dee1b9926cd91299271eec7304dca2d06c2f7c1d1f13228`
  Evidence 2026-07-10, strengthened 2026-07-14: one closed authority fixes all eight normative budgets and is enforced by ten adversarial controls. All 48 static gates are compiler-free, network-denied, capped at 15 seconds/64 MiB, and the static panic scan completes in about 0.26 seconds while a separate Clippy lane preserves semantic assurance. Changed-impact fallback passed offline in 83.333 seconds with 6,475,776 bytes of positive disk growth; a clean locked/offline workspace build used 2,671,382,528 bytes; and the prebuilt signed-evidence verifier passed offline without a compiler in 6.1 seconds. The final warm `prepush-standard` run passed 67 gates in 365,794 ms against the 480,000 ms/3 GiB GB-3 limits, including workspace Clippy, panic assurance, 27/27 agent workflows, mobile/edge pipelines, and real-device Apple M1 GPU compute. Twelve release-scale gates are now held in a schema-closed `releaseFullOnlyGates` inventory that is proven absent from common/prepush scheduling and present in `release-full`; this includes full generative/performance suites, production CLI parity, domain bootstrap, source differential parity, backend feature matrices, and stress loops. Cargo scheduling consumes transitive compilation declarations, declared build-environment transitions are provenance-guarded with 19 cache-policy controls, and the retained release ceiling remains 2,700 seconds/20 GiB. GB-7 concentration debt is bounded by four reviewed file waivers and two M3-expiring semantic-crate waivers; GB-8 audits 336 Python-bearing files with zero undeclared modules using the vendored licensed Python 3.9 TOML compatibility path. Its pure classifier now proves that the Unix `fcntl` and Windows `msvcrt` lock backends are declared Python 3.9 standard-library dependencies while an injected third-party module still fails closed.
- [x] **R0.4.g Enforce bounded generated-state caches.** Apply GB-5 continuously, not only when a human remembers cleanup: every producer declares owner, content key, last-use/lease, size class, retention class, and safe reclamation order. Enforce configurable soft/hard quotas with concurrency-safe admission and garbage collection; refuse new rebuildable growth before crossing the hard limit; preserve user-authored data, retained evidence, dependency mirrors, active leases, and rollback quarantine. Prove dry-run/execute parity, crash recovery, concurrent builders, low-disk behavior, and steady state across repeated full profiles. done 2026-07-14; evidence: `scripts/check_deterministic_cleanup.sh`, `scripts/check_cargo_target_dir_policy.sh`, and `scripts/check_default_iteration_workflow.sh`; input: `generated-state-lifecycle-bundle-sha256:83b9a652a4e3611a51c9988f805c596877f52f5076e4cba4721736fc92d93407`
  Evidence 2026-07-14: a closed producer authority covers every reviewed generated or retained root with owner, content-key strategy, bounded size class, retention class, process/protected lease mode, and deterministic reclaim priority. Fixed 8 MiB JSON, 64-producer/class, 4,096-entry, 16,384-lease, 6 GiB soft, 8 GiB hard, and 2 GiB free-space ceilings make policy and registry growth structurally bounded; environment overrides may only tighten the GB-5 ceiling. Rebuildable admission reserves before materialization, accounts for the maximum of reservation and observed allocation, evicts inactive entries by reclaim priority/last-use/identity, trims an oversized inactive requested cache, and denies hard-quota or low-disk growth when only requested or active state remains. Random leases bind OS boot/session plus process-start identity, recover PID reuse and abnormal exits, and block intersecting whole-root cleanup; one-shot resolver formats use self-owned transient leases so a persistent CI supervisor cannot accidentally pin a reservation it cannot release. An external Git-control mutex works for linked worktrees and keeps Windows cleanup from renaming an open lock; atomic registry writes and planned/quarantined transactions recover interrupted reclamation. Every deletion still requires deterministic-cleanup class and current producer-marker authority, so retained evidence, mirrors, rollback quarantine, unknown/user-authored state, symlinks, tracked content, and active leases never become quota candidates. Forty-one exact lifecycle controls cover closed schemas, producer closure, clean-checkout provenance initialization, legacy reclamation, deterministic LRU, hard/low-disk/lease/cardinality denial, protected retention, stale-process recovery, crash recovery, eight concurrent same-entry builders, cleanup/admission serialization, duplicate/oversized input, transient metadata recreation, and twenty alternating host/WASI cycles reaching one-entry steady state. Cargo's semantic resolver acquires and releases leases across scope transitions; its 21-control policy gate also rejects a Python-only parent gate reserving the full root-host class around nested verifier builds. Migration transactionally reclaimed about 61 GiB of pre-policy build islands, then a warm full fallback loop passed all core/CLI tests in 109,632 ms with zero active-target growth and a 3.0 GiB managed cache, within GB-2 and GB-5.
- [x] **R0.4.h Consolidate governance machinery before adding more.** Until M1, use a one-in/one-out budget for governed check entrypoints and prefer extending existing schemas, manifests, matrices, and shared libraries. Remove duplicate wrappers/update scripts, collapse checks with the same authority and resource envelope, and publish the before/after inventory plus saved default/CI time and disk. A new entrypoint requires a distinct trust boundary and cannot be justified by documentation convenience alone. done 2026-07-14; evidence: `scripts/check_gate_manifest.sh` and `scripts/check_check_update_boundary.sh`; input: `governance-consolidation-bundle-sha256:d7a87d7398d04c6e0f3f73bb6cb477b4c62fe4b49edfa651fdde39818c27ae75`
  Evidence 2026-07-14: the existing closed gate policy and generated manifest now own a one-in/one-out budget rather than introducing another governance document or check. The renderer independently counts check, update, and render entrypoints; binds baseline, ceiling, current, non-positive delta, retired aliases, canonical replacements, and aggregate declared duration/disk; rejects any surviving compatibility-wrapper marker; and adds a dedicated budget-weakening negative control. The feature-matrix check and updater were pure aliases for capability-ledger authorities, so every caller and agent skill reference moved to the canonical names and both aliases were deleted. Inventory fell from 125 to 124 checks and 79 to 78 updaters, with renderers unchanged at 58. The removed aggregate check carried a 600-second/4,096-MiB declared prepush/CI envelope, so aggregate declared inventory fell from 63,420 to 62,820 seconds and 425,472 to 421,376 MiB; these are scheduling ceilings, not observed consumption claims. The canonical capability-ledger behavior and trust boundary remain intact. Roadmap evidence binds the gate policy, schema, renderer, and checks rather than recursively hashing the generated manifest that itself binds `ROADMAP.md`; `check_gate_manifest.sh` independently proves that derived output exact. The check/update audit passes 124/124, the gate manifest passes with 17 negative controls, and any future entrypoint growth fails unless an existing entrypoint is retired in the same reviewed change for a distinct trust boundary.

### R0.5 Baselines and release hygiene

- [x] **R0.5.a Establish reference host profiles.** Record tier-1 macOS arm64 and Linux x86_64; add Linux arm64 and Windows x86_64 as tier-2 until promoted. Include CPU, memory, filesystem, OS, compiler, and power-mode metadata. done 2026-07-10; evidence: `scripts/check_reference_host_profiles.sh`; input: `reference-host-profile-bundle-sha256:cfbee02aaeda5e3e6446480f683e4846ea8668e8dfa124dfa1908f6ae4065c7a`
  Evidence 2026-07-10: one closed policy and two closed Draft 2020-12 schemas define exactly four prerequisite-aligned host classes: tier-1/reference macOS arm64 and Linux x86_64, and tier-2/candidate Linux arm64 and Windows x86_64. Each profile pins CPU architecture/model/core floors, memory floor, filesystem type/block size/case behavior, OS range, Rust/native compiler family and range, and AC/low-power/governor state; measurement-session controls separately require an exclusive non-virtualized host, nominal thermals, at most 5 percent background load, and are consumed by R0.5.b rather than falsely inferred by this machine probe. Promotion requires signed E3 evidence, independent verification, at least two hosts, and at least 30 samples per workload. The dependency-free Python 3.9 probe emits timestamp-free, hostname-free, username-free, serial-free, absolute-path-free canonical JSON, derives rather than trusts conformance, and binds every portable fact with SHA-256. Nine controls prove conformant fixtures for all four platforms and reject tier drift, identity tampering, path leakage, stale derived conformance, and duplicate keys. Two consecutive real Apple M1 observations were byte-identical and conformant with identity `578df4adfb8b04c2c1cd9910ad195dae7448750afb293a25e3a858c247dc7c8c`; this remains unsigned diagnostic E0 and cannot support a baseline or release claim. The read-only, compiler-free, network-denied gate is registered across all four platforms in CI, the gate manifest, the green front door, the profile matrix, topology/index authority, and all 117 update-boundary checks remain compliant.
- [x] **R0.5.b Normalize benchmark workloads.** Fix exact source terms, input sizes, expected hashes, warmup, sample count, timeout, cache state, and statistical treatment for PB-1 through PB-10. done 2026-07-10; evidence: `scripts/check_roadmap_workloads.sh`; input: `roadmap-workload-bundle-sha256:8210987bea7a76469d9f620142196e1dd0a6aae7b2be4dd14d5e7e8880d7c5f8`
  Evidence 2026-07-10: one closed Draft 2020-12 schema and canonical policy define all ten PB IDs in numeric order, with exact content-addressed source/protocol inputs, canonical expected-outcome descriptors and hashes, input sizes, runner and truthful availability state, target direction/unit/value, cache state, warmup, sample count/unit, and timeout. PB-1/PB-2/PB-3 share one exact `fib(25)` source and result; four currently executable workloads are `active`, five unavailable target runners are fail-closed `roadmap-blocked`, and the optional JIT is `decision-gated`, preventing specification from masquerading as implementation evidence. Timing workloads retain 30 ordered samples after five warmups, report median/p95-nearest-rank/MAD, retain every outlier, invalidate the whole set on timeout or semantic failure, and decide against the directional bound of the exact distribution-free 95 percent median interval at ranks 10/21. PB-8 fixes 1,000 warmups, 100,000 measured requests, 30 quiescent RSS checkpoints, maximum growth plus robust Theil-Sen slope, and a 45-minute hard session ceiling. PB-9 binds five transitive cases spanning values, canonical hashes, effects, sealed/resource errors, and scheduling; PB-10 binds the seed, source manifest, exact stage commands, two rebuilds on each of two tier-1 hosts, and byte-identical stage2/stage3. Twelve adversarial controls reject false availability, source/descriptor tampering, cache/sample/timeout drift, corpus omission, path escape, workload order drift, outlier deletion, fixpoint weakening, and duplicate keys. The 0.28-second, roughly 18.5 MiB read-only gate is compiler-free and network-denied; its cache identity covers twenty direct/transitive inputs and is enforced in CI, the green front door, profile matrix, topology, and all 118 update-boundary checks. Legacy scalar `best_of` output is explicitly non-authoritative E0; raw signed samples and truthful current failures remain R0.5.c work.
- [x] **R0.5.c Capture a signed baseline.** Preserve raw samples and current failures without rewriting history; label local 2026-07-10 numbers E0 until reproduced under the new harness. done 2026-07-10; evidence: `scripts/check_roadmap_baseline.sh`; input: `roadmap-baseline-bundle-sha256:58e13bafd51de7d349098482ce8dc55bfac0a63aa7c45e9de04433054395ab69`
  Evidence 2026-07-10: a closed statement schema, closed DSSE bundle schema, explicit capture producer, independent verifier, and content-addressed retained fixture preserve the complete first normalized run. The release-equivalent sampler accepts only active PB-1/PB-4/PB-5/PB-7 IDs, consumes exact normalized source bytes, prepares declared caches outside the clock, measures one execution with a monotonic nanosecond clock, validates every integer/vector/map/parser result after timing, and lets the controller enforce a hard process timeout. The E0 statement retains all 20 warmups and 120 ordered samples, recomputes exact-rational median/p95/MAD/rank-10/rank-21 statistics, binds workload policy `4fdb7f57fdf68a0ef33dcdf075c3697c8213317958e354c1d55e518784f69dea`, conformant host observation, binary, Rust toolchain, Git revision, and complete dirty tracked/untracked material identity, and has baseline identity `a3d6c7b809f1c1ba403bab0c4e18fce94154cbae4b35b23aa9e96cfb1c02e967`. PB-1 passed with a 210,932,958 ns upper median-confidence bound and PB-4 passed at 6,928,000 ns; PB-5 truthfully failed at 488,535,292 ns versus 300 ms, and PB-7 failed with a 42,804 bytes/s lower bound versus 1,048,576. PB-2/PB-6/PB-8/PB-9/PB-10 remain runner-unavailable and PB-3 remains decision-not-approved; none carries fabricated samples. A generic standalone producer signed canonical DSSE bytes using an ephemeral 0600 Ed25519 seed that was destroyed after capture. The retained raw public key is independently pinned as `sha256:f942d973dd550ff9b95e0a61f47e8cee9580e8bb8d43f0226408d57bde2d113f`; the separate verifier workspace shares no producer code, recomputes payload and baseline identities plus the ten-workload/sample/failure inventory, and deterministically reports E0 with `signatureGrantsAuthority=false`. Nine cryptographic and eight statement controls reject forged signatures, payload/sample tampering, wrong/duplicate keys, authority/class escalation, permissive/short secrets, failure erasure, unavailable-runner fabrication, and overwrite attempts. The read-only warm gate passes offline in about 2.36 seconds from the governed cache, all 119 update-boundary checks remain compliant, and the sole update command uses create-new content-addressed paths, refuses an existing date or identity, rolls back only newly created outputs, and never rewrites retained history.
- [x] **R0.5.d Add release-note automation.** Generate compatibility, migration, known-gap, evidence, dependency, and security sections from canonical inputs; reject unsupported claims. done 2026-07-11; evidence: `scripts/check_release_notes.sh`; input: `release-notes-bundle-sha256:2ab7d8ea3c13d5b67abb76ba703e07b5d09f651bc6c2e6c03b5f4a22451f1f7e`
  Evidence 2026-07-11: one reviewed policy and two recursively closed Draft 2020-12 schemas generate a content-addressed E1 JSON artifact plus a uniquely bounded Unreleased changelog block from exact canonical inputs. The artifact preserves all ten version surfaces, ten reserved v1 compatibility entries, four migration records, forty roadmap gaps mapped to twenty-nine capability claims, per-platform maturity and limitations, six classified fixtures, the signed E0 baseline and its two current budget failures, three Cargo lock inventories, npm integrity and mirror policy, and nine mandatory release/security gates. Static generation cannot assert execution: every gate is `required-but-not-attested`, `passedChecks` is empty, fixture authority remains false, the signed baseline remains observation-only, and an unqualified capability claim requires L5 on every tier-1 platform plus immutable E3/E4 evidence; none currently qualifies. Source hashes, lock hashes, selected gate execution identities, and a canonical content identity make every generated fact reproducible without timestamps or host paths. Seventeen adversarial controls reject compatibility, capability, evidence, or baseline authority escalation; migration, gap, limitation, or security-gate omission; injected gate success; dependency and source tampering; unknown or duplicate keys; absolute host paths; content-identity drift; duplicate changelog markers; and movement of generated content across released history. `scripts/check_release_notes.sh` is compiler-free, network-denied, read-only, deterministic, proves updater idempotence, and is registered in CI, release smoke, the green front door, profile topology, and the 120-gate manifest; only `scripts/update_release_notes.sh` may replace the generated JSON and marked changelog region, leaving released history outside its write boundary.

**R0 exit criteria:** a published clean checkout passes planning/topology/hygiene/complexity, root-lock, version, generated-artifact, warning-denied Clippy, no-user-panic static, and TCB static checks under declared tools on local and GitHub CI profiles; all checks are read-only; replay parity includes denied effects on native and WASI; the evidence verifier rejects every adversarial fixture; default tests neither nest aggregate pipelines nor contain stress/performance loops and remain stable under supported parallel load; default changed gates and generated-state caches meet GB budgets; generated status views exactly match the capability ledger.

---

## R1. Agent Preview: training-ready authoring and iteration

**Goal:** make v0.3 a stable, self-contained target that the user's AIs and other agents can learn, write, repair, test, and operate locally without hidden repository knowledge.

### R1.1 Versioned agent language profile

- [x] **R1.1.a Define `GC-AGENT-v0.3`.** Freeze the supported surface for agent training: lexical grammar, CoreForm mapping, evaluation, values, contracts, modules, effects, packages, errors, resource limits, and compatibility identifiers. done 2026-07-11; evidence: `scripts/check_gc_agent_profile.sh`; input: `gc-agent-profile-bundle-sha256:c48b46cdb66d101d68f3c3524b99425c5478dff1bc4a12337281199f389ba091`
  Evidence 2026-07-11: reviewed policy `policies/gc_agent_profile_v0.3.json`, recursively closed Draft 2020-12 schema `docs/spec/GC_AGENT_PROFILE_v0.3.schema.json`, and generated artifact `docs/spec/GC_AGENT_PROFILE_v0.3.json` freeze `GC-AGENT-v0.3` across all 11 required semantic domains. The profile exposes 51 exact evaluator primitives and 12 runtime value variants, seven parser cases, six evaluator cases, two resource-limit cases, three package cases, and 12 explicit unsupported-behavior boundaries; its canonical identity is `942572431408109b381add432a4a40958c5865c70e6268d0bb29526c0f6074a2`. `scripts/lib/gc_agent_profile.py` independently rediscovers the primitive surface from Rust match arms, verifies special forms, runtime values, authority anchors, conformance paths, version registries, compatibility constants, and all five mandatory unsupported classes, hashes every normative input, and rejects 20 adversarial mutations. `crates/gc_cli/tests/gc_agent_profile_v03.rs` executes the declared parser, evaluator, resource, package, card, unsupported-class, and runtime-identity contracts directly (six tests). The profile is negotiated by the canonical authoring skill and pack, exposed through the agent index and onboarding bundle, uses repository-relative references, and is enforced by the governed check/update boundary, CI, profile matrix, and green front door. Independent `jsonschema` validation, profile check/self-test, Rust conformance, and all focused authoring-bundle gates pass.
- [x] **R1.1.b Generate a compact language card.** Produce a <=4k-token core card from normative schemas/spec anchors, with canonical positive and negative examples. A drift gate proves every symbol/example parses against the current profile. done 2026-07-11; evidence: `scripts/check_gc_agent_core_card.sh`; input: `gc-agent-core-card-bundle-sha256:3279f8832ccb1a61ac699bed4a39d044d98c9f2ad5469eaa26f14e147b7d7883`
  Evidence 2026-07-11: `policies/gc_agent_core_card_v0.3.json` generates `docs/spec/GC_AGENT_CORE_CARD_v0.3.md` and content-addressed manifest `docs/spec/GC_AGENT_CORE_CARD_v0.3.json` solely from the frozen profile and its schema. The card is ASCII-only and 3,916 bytes, establishing a tokenizer-independent upper bound of 3,916 tokens, below the 4,000-token requirement. Its manifest identity is `b4e77412714bee699cdef5127c34a17774a258252bff0485a8928bf3f5fd713e`; it contains all 168 deduplicated profile surface symbols, six canonical examples split between successful semantics and valid-syntax failures at named semantic/resource boundaries, and all five mandatory unsupported classes with complete actionable records in the manifest. `crates/gc_cli/tests/gc_agent_profile_v03.rs` parses every symbol/example and proves class visibility with the production CoreForm integration corpus. The read-only checker proves byte-for-byte regeneration, profile/source identities, complete surface coverage, ASCII and byte ceilings, duplicate-key rejection, host-path exclusion, and 11 adversarial manifest controls. The card is exposed through `agent-index`, placed before the full profile in onboarding retrieval, required by the authoring skill and pack, and enforced by CI, the profile matrix, and green front door.
- [x] **R1.1.c Generate task-specific cards.** Create capability, package, patch, replay, testing, deployment, and troubleshooting cards selected by declared task intent, each with a token budget and source hash. done 2026-07-11; evidence: `scripts/check_gc_agent_task_cards.sh`; input: `gc-agent-task-cards-bundle-sha256:f63be11283eb0f52797601cbb713ceff01976c89f15a060d53baf5fd17237fd3`
  Evidence 2026-07-11: reviewed policy `policies/gc_agent_task_cards_v0.3.json` generates the seven required records in `docs/spec/GC_AGENT_TASK_CARDS_v0.3.json` plus the human compendium `docs/spec/GC_AGENT_TASK_CARDS_v0.3.md`. Every card is ASCII-only, capped at 1,400 bytes, and independently binds reviewed authority content through `sourceHashSha256`; mutable tests, benchmarks, retained evidence, symlinks, repository escapes, and host paths cannot authorize content. All seven cards total 5,263 bytes/tokens, below both AB-2's 12,000 maximum and 8,000 target; registry identity is `4a443803ce35ac1a27eb549e583514abeb64c72c537f4eba9499ae2d0c8de018`. The independent selector validates exact `genesis/agent-intent-v0.1` fields, normalizes goal/domain/workflow/op declarations, emits deterministic reasons and bundle identity, falls back explicitly to troubleshooting, passes six positive fixtures, and rejects ten prompt-authority, schema, shape, budget, source, host-path, and content mutations. Production `genesis --json agent-plan` embeds the registry at compile time, rejects unknown intent fields, returns complete selected card content and identities in `plan.context_cards`, and binds that selection into `plan_hash_blake3`. Five planner tests pass, including byte-for-byte production/Python selector parity and repeated plan-hash stability. CI, profile matrix, green front door, agent index/onboarding, and the canonical authoring skill/pack require the governed card gate; diagnostics and executable skill conformance pass at 100/100.
- [x] **R1.1.d Publish a machine-readable symbol index.** Include signatures, effects, capabilities, contracts, examples, diagnostics, deprecations, and source links. Support exact lookup without embedding the entire doc tree. done 2026-07-11; evidence: `scripts/check_gc_agent_symbol_index.sh`; input: `gc-agent-symbol-index-bundle-sha256:80b63bafe632a3c20035b16929e270eae72367503365276a53659ffd133fd7bb`
  Evidence 2026-07-11: reviewed policy `policies/gc_agent_symbol_index_v0.3.json`, recursively closed Draft 2020-12 schema, and deterministic generator produce `docs/spec/GC_AGENT_SYMBOL_INDEX_v0.3.json` with one uniquely sorted record for every one of the 168 deduplicated `GC-AGENT-v0.3` symbols. Each self-contained record carries a structured callable/form/value/field/identifier signature, explicit purity or deferred runner-effect semantics, required capability declarations, normative contracts, conformance or usage examples, diagnostic families, active/deprecated lifecycle state, and repository-relative authority links. The generator hashes 22 source authorities, verifies every anchor, independently proves exact closure over all 51 runtime primitive match arms, rejects duplicate JSON keys, host paths, symlinks, repository escapes, prompt authority, stale bytes, incomplete records, and ten adversarial artifact mutations, and exercises two lookup-cardinality controls; the generated index identity is `63662d549943908f8018f091b9adeb7a284dd33d62bfb631d8f0e00b8dd1097d`. Production `genesis --json agent-index` exposes only bounded metadata and identities, including the unsupported inventory, while `--symbol <exact-name>` reads the compile-time index and returns at most one `genesis/agent-symbol-v0.3` record without loading the doc tree; padded, unknown, and case-drifted names fail closed. Four production/parity integration tests pass. The gate is governed as read-only, offline, and compiling, and is required by CI, the profile matrix, green front door, onboarding, the authoring bundle, and the canonical authoring skill/pack.
- [x] **R1.1.e Define unsupported behavior.** The card and schema must name experimental syntax, host-only operations, unavailable targets, nondeterministic facilities, and capabilities outside the selected profile. done 2026-07-11; evidence: `scripts/check_gc_agent_profile.sh` and `scripts/check_gc_agent_core_card.sh`; input: `gc-agent-unsupported-behavior-bundle-sha256:c05fb2dcb5fb08f02ceeac6f142d930d7dce748170da9689ad582de0f0bd71b9`
  Evidence 2026-07-11: the frozen profile policy, recursively closed schema, generated profile, and compact card now require the ordered classes `experimental-syntax`, `host-only-operation`, `unavailable-target`, `nondeterministic-facility`, and `out-of-profile-capability`. Draft 2020-12 `contains` rules independently require every class, while the resolver requires exactly one canonical record per class and rejects missing classes, unknown fields, weak status, unknown enforcement, erased safe alternatives, duplicate IDs, host paths, and stale roadmap links. Twelve status-`unsupported` records define exact behavior, enforcement (`reject`, explicit-effect-only, deny-by-default, profile negotiation, bounded opt-in, or claim prohibition), rationale, safe alternative, and change-authorizing roadmap task; stale bytecode and JIT links were corrected to R3.1.a and R3.4.c. The 3,916-byte ASCII core card names all five classes, and its content-addressed manifest carries every complete record; 20 profile and 11 card mutation controls plus the six-test Rust corpus prove card visibility and actionable alternatives. The symbol/agent index exposes bounded class/count metadata and unsupported-inventory identity `e0060de7377c7e5123c651c32baf974527493f72afed8947c1edbe12f9d3fcc5` without embedding the profile tree. Both profile and symbol artifacts pass independent Draft 2020-12 validation; authoring/onboarding rules forbid prompt- or index-derived authority and require safe alternatives without silent policy broadening.

### R1.2 Diagnostics as an agent API

- [x] **R1.2.a Create a stable diagnostic catalog.** Assign versioned IDs, severity, phase, primary span, related spans, structured parameters, likely causes, safe repair actions, and documentation anchors. done 2026-07-11; evidence: `scripts/check_cli_diagnostics_contract.sh`; input: `gc-diagnostic-catalog-bundle-sha256:76372964e4a520a2d1114a9fa16741b77212b8a070472b3bc5316fb1796294ce`
  Evidence 2026-07-11: reviewed `policies/gc_diagnostic_catalog_v0.1.json`, a recursively closed Draft 2020-12 schema, and the independent read-only generator close all 125 production CLI diagnostic codes across 20 explicit lifecycle phases. Generated `docs/spec/GC_DIAGNOSTIC_CATALOG_v0.1.json` has identity `27817269acf0a487e731b39d27788ba41ded0a7d3bc685dea5930dd2097a51ed`; each uniquely sorted record binds a versioned `genesis/diagnostic/v1/<family>/<name>` ID to severity, nullable primary-span and always-present related-span contracts, JSON-typed parameters, nonempty likely causes, guarded safe repairs, anchored documentation, and exact source callsites. Runtime envelopes embed the catalog identity and emit those fields; producer context remains namespaced under the declared `parameters.context` object, and uncataloged routes fail closed as `diagnostic/catalog-miss` while retaining `parameters.reported_code`. `agent-index --diagnostic <exact-code>` returns at most one self-contained record without loading the documentation tree and rejects padding, case drift, unknown codes, and conflicting symbol lookup. Fifteen artifact mutations, two lookup controls, six runtime unit tests, six agent-index integration tests, the all-command-family matrix, independent JSON Schema validation, and source/freshness hashing pass. The whitespace-bearing debugger helper routes were collapsed to stable `debug/bisect`; normative prose was folded into the existing CLI JSON contract to hold the 134-active-doc ratchet; R1.2.b builds the universal closed context contract and typed domain producers on this catalog authority.
- [x] **R1.2.b Eliminate string-only failure contracts.** Parser, typechecker, evaluator, package, policy, replay, patch, build, and deployment errors expose structured JSON while retaining concise human rendering. done 2026-07-11; evidence: `scripts/check_cli_diagnostics_contract.sh`; input: `structured-failure-contract-bundle-sha256:2b00b0fea01b0184e5c9300a12a645716958862c55820eba3a42468320297923`
  Evidence 2026-07-11: the recursively closed Draft 2020-12 `docs/spec/GC_FAILURE_CONTEXT_v0.1.schema.json` defines the only runtime failure-context shape and the exact authority domains `parser`, `typechecker`, `evaluator`, `package`, `policy`, `replay`, `patch`, `build`, and `deployment`. `crates/gc_cli_driver/src/structured_failures.rs` converts typed parser offsets into UTF-8 line/column spans, preserves kernel and effect variants, records stable manifest/patch/build/replay facts, scrubs absolute paths recursively, and normalizes every remaining legacy producer into the same closed schema without treating prose as a routing contract. `gc_types` now creates stable typechecker diagnostic IDs, codes, severities, module identities, and ordinals at the checker authority while retaining legacy error/warning vectors and CoreForm rendering; `gc_obligations` and the CLI preserve those diagnostics end-to-end. Parser, evaluator, package, policy, replay, patch, build, deployment, and self-host protocol boundaries emit typed contexts directly; any failure envelope without a producer error fails closed to a cataloged synthesized error, and every other legacy context is preserved only as path-scrubbed `facts.legacy_context`. The 125-code catalog scanner recognizes plain, anyhow, and structured constructors and rejects scanner drift. `cli_structured_failures` exercises all nine domains, span localization, deterministic path exclusion, structured typechecker payloads, and concise non-JSON rendering; the existing all-command-family matrix now requires the closed context on every failure. Forty-nine driver unit tests, 35 typechecker tests, 18 obligation tests, five typecheck/patch frontend tests, both CLI matrices, the catalog self-test, schema closure check, and the no-user-panic gate pass. The check remains read-only, offline, isolated-cache aware, and completed in 13.39 seconds against a 300-second budget.
- [x] **R1.2.c Add repair hints with guardrails.** Hints may propose syntax/contract/capability changes but never silently broaden policy or suppress an obligation. Capability broadening is a separately reviewable policy diff. done 2026-07-11; evidence: `scripts/check_cli_diagnostics_contract.sh`; input: `guarded-repair-hints-bundle-sha256:0eedc4872a9d6b1355ee5e222e5acbf46fef07ddccf8dccb368a2dca940df253`
  Evidence 2026-07-11: reviewed catalog policy now fixes `genesis/diagnostic-repair-guardrails-v0.1` with no prompt authority, forbidden obligation suppression, catalog-only automatic eligibility, and policy broadening only through a separate reviewed diff. The recursively closed `GC_DIAGNOSTIC_REPAIR_PLAN_v0.1` schema distinguishes inspection, verification commands, source patches, effectful commands, and policy review; every action binds diagnostic/content/policy preconditions, original-command retry and obligation-preservation postconditions, explicit policy and obligation effects, automation eligibility, and review state. The generator enriches all 125 cataloged diagnostics from 20 lifecycle phases: source patches, effectful reruns, and policy changes require review; policy actions are never automation-eligible; obligation-sensitive repairs require reruns; no action can suppress verification. Runtime diagnostics preserve compatibility strings while adding a catalog-bound repair plan whose authorization always denies policy changes and obligation suppression. A concrete capability denial emits only a `genesis/capability-policy-diff-v0.1` proposal with `requires_review = true`, `auto_apply = false`, and an intentionally unresolved base-policy identity; code-derived placeholder capabilities cannot produce proposals. `run` now emits typed `caps/denied` failures carrying the exact blocked operation instead of degrading to `error/unknown`. Fifteen catalog mutation controls reject prompt authority, automatic broadening, obligation suppression, review bypass, stale sources, and malformed metadata; schema closure checks reject mutable authorization or applicable policy diffs. Fifty-two driver unit tests, the all-command-family matrix, and the nine-domain matrix cover automatic-action implications, obligation reruns, concrete and placeholder capabilities, policy proposal non-applicability, concise human compatibility, and deterministic path safety. The read-only diagnostics gate passes in 14.033 seconds against its 300-second budget.
- [x] **R1.2.d Build diagnostic goldens.** Cover malformed syntax, type/effect mismatch, unhandled effects, seal misuse, replay tampering, path normalization, exhausted budgets, invalid packages, stale patches, and incompatible profiles. done 2026-07-11; evidence: `scripts/check_cli_diagnostics_contract.sh`; input: `diagnostic-goldens-bundle-sha256:92a6b7444a3a7d8c1bc024243c2ee0396689bcc08d62adf260e2bedb8c03943d`
  Evidence 2026-07-11: `tests/diagnostics/goldens/v0.1/diagnostics.json` freezes ten uniquely sorted, one-per-class production failure projections under `genesis/diagnostic-goldens-v0.1`; every projection binds the exact catalog identity, diagnostic/error route, command or generic envelope kind, closed failure context, phase, spans, parameters, causes, documentation, and guarded repair plan. The executable Rust harness constructs all fixtures under an unpredictable absolute root and reproduces malformed CoreForm, a strict open effect-row mismatch, an allowed but unknown effect operation, a non-token seal argument, a tampered replay decision/capability fact, an absolute manifest path, hard kernel step exhaustion, an unsupported package schema, a stale semantic node identity, and an incompatible runtime backend profile. Full agent-consumed projections must contain no fixture root; absolute paths retain only safe basenames, preserving actionable deterministic material. Runtime probing corrected two success fallthroughs: sealed protocol ERROR results from `eval` now emit typed evaluator failures, and allowed-but-unhandled runner operations now emit typed policy failures instead of `ok = true`; replay applies the same sealed-ERROR boundary; caps denials remain distinct. `scripts/lib/gc_diagnostic_goldens.py` independently enforces exact class/code/domain/kind routes, one diagnostic per case, sorted uniqueness, closed outer shapes, catalog IDs, immutable repair authorization, and zero host-path leaks, while ten negative controls reject missing/duplicate coverage, reordering, success fallthrough, absent context, policy broadening, obligation suppression, and path leakage. The updater is a separate exact-opt-in command; normal checks are read-only. Fifty-two driver tests and both golden tests pass, and the diagnostics front door now runs catalog/schema checks plus the command-family, nine-domain, and ten-golden matrices in 9.679 seconds against its 300-second budget.
- [x] **R1.2.e Measure repair utility.** Run deterministic mutation families plus pinned reference agents; report exact recovery, over-repair, policy broadening, regression, and token cost. done 2026-07-11; evidence: `scripts/check_cli_diagnostics_contract.sh`; input: `diagnostic-repair-utility-bundle-sha256:bd675f83844637550c134b3f3189a455d939367bcf18a893a4e0d49b29c6a904`
  Evidence 2026-07-11: reviewed `policies/gc_repair_utility_v0.1.json`, a closed Draft 2020-12 report schema, and an 18-case sorted corpus freeze six mutation families: missing and extra delimiters, integer literal type errors, primitive-name edits, unsupported package schemas, and deny-by-default capability policy cases. Fifteen cases are automatically repairable and three require safe abstention; every family has three variants. The hidden exact-source oracle remains in the runner only. The two source-hashed pinned agents run as isolated Python stdio programs with deterministic decoding, null seeds, no filesystem/network/subprocess imports, no case IDs, and no expected fields: the baseline receives only the human message, while the primary receives the complete structured diagnostic and authorization. Every hash-preconditioned patch is confined to declared mutable source or manifest bytes; policy files are immutable. Exact recovery requires expected bytes, the original production CLI command to succeed, and an independent replay to return the identical structured result within two turns. Exact UTF-8 byte-token accounting covers canonical request and response JSON. The retained report records all attempts, failure codes, file identities, outcomes, model/runtime/context/decoding metadata, and cost. Across two byte-identical production runs the catalog-guided agent recovered 15/15 automatic cases (10000 basis points, exceeding AB-6's 8500 M1 threshold) and safely abstained on 3/3 reviewed cases, versus 6/15 recovery for the human-message baseline: nine additional exact recoveries and a 6000-basis-point lift at 41,865 exact byte tokens. Over-repair, policy broadening, regression, and initial diagnostic mismatch are all zero. The independent verifier recomputes every identity, route, hash, attempt bound, rate, token total, comparison, and acceptance decision; it enforces oracle isolation and rejects 15 corpus/report attacks. The diagnostics front door reproduces the report twice and compares exact bytes in read-only mode; intentional refresh is a separate producer. This deterministic evidence establishes current API utility without claiming the later held-out multi-model corpus result.
- [x] **R1.2.f Make human diagnostics independently useful.** Render every stable structured diagnostic as a concise message that includes the failed operation, safe normalized subject, primary cause, and one next action without requiring `--json`. Generate human text from the catalog/context rather than maintaining parallel prose, preserve redaction and deterministic ordering, and test terse terminals, color/no-color, narrow widths, nested causes, and unknown-safe fallback. done 2026-07-14; evidence: `scripts/check_cli_diagnostics_contract.sh`; input: `human-diagnostic-rendering-bundle-sha256:3c13d40af1952172f3ab3dc4f6f000d1fafb72c32e87451735c790aa406430ca`
  Evidence 2026-07-14: both production CLI profiles now render pre-dispatch and command-result non-JSON failures from the same catalog-bound diagnostic object used by JSON mode. Every rendering contains the exact diagnostic code, failed normalized operation, one safe subject, one primary cause, and exactly one catalog-authorized next action; fixed-priority traversal handles nested cause/legacy objects without turning prose into a routing contract. Absolute paths collapse to basenames, control characters become spaces, fields are bounded, and absent or uncataloged facts fail closed to explicit unknown/catalog-miss output. Deterministic wrapping clamps `COLUMNS` to 24-160 columns with a 96-column default. ANSI affects labels only, activates for terminals or `CLICOLOR_FORCE`, and is always disabled by `NO_COLOR`; stripping ANSI reproduces plain output exactly. Eleven focused renderer controls and three production-binary tests cover nested causes, path and terminal-control injection, unknown codes, terse/narrow output, forced color, no-color precedence, command-result routing, and stderr-only behavior. JSON and MCP protocol surfaces remain unchanged.

### R1.3 Warm daemon and generated MCP interface

- [x] **R1.3.a Harden the existing `warm` protocol.** Version framing, request IDs, cancellation, deadlines, bounded queues, per-workspace isolation, graceful restart, crash recovery, idle eviction, and protocol-level typed errors. done 2026-07-11; evidence: `scripts/check_warm_protocol_contract.sh`; input: `warm-protocol-bundle-sha256:ff9102f98ab9b9fa08c121ddc721a56100a331dd93aa4e390a137f3604a3610b`
  Evidence 2026-07-11: `genesis/warm-protocol-v0.2` is a closed newline-delimited JSON protocol with six exact methods, mandatory per-generation initialization, bounded ASCII request IDs, duplicate rejection, immediate execute admission plus exactly one terminal response, and sequence-ordered `genesis/warm-response-v0.2` envelopes carrying closed `genesis/warm-protocol-error-v0.2` failures. The recursively closed Draft 2020-12 schema and independent standard-library verifier bind protocol, response, error, and final session identities; exact method and bound sets; 19 mandatory failure routes; source capabilities; normative documentation; and seven public integration controls. Five schema mutations reject identity drift, open response objects, missing metadata, unbounded argv, and undeclared methods. Transport allocation is bounded before UTF-8 and JSON decoding, oversized frames are drained without desynchronizing the next frame, and malformed, oversized, and invalid-UTF-8 frames consume the same finite session budget as accepted frames. Queue depth, reader capacity, workspace count, frame bytes, argv cardinality/entry bytes, deadline, frame count, and idle lifetime are finite and initialization reports the negotiated limits. Workspace IDs bind once to canonical base-relative roots; parent, absolute, symlink, and rebinding escapes fail closed; active workspaces cannot be evicted; serialized dispatch prevents current-directory overlap; and restoration failure becomes a typed worker error. Native control remains responsive while one worker executes. Queued cancellation removes work; running cancellation and deadlines suppress late results; initialization explicitly advertises `hard_termination = false` and `running_cancellation = cooperative-result-suppression`, preserving R1.3.b as the kill-and-reap authority rather than overclaiming it here. Idle-only restart advances the generation and requires renegotiation; contained worker panic emits a typed crash, discards uncertain queued work, clears workspace/ID state, and requires reinitialization; shutdown and EOF drain accepted work. Five white-box parser, panic-boundary, eviction, and crash-reset tests plus seven shipped-binary tests prove cold/warm semantic parity, two-workspace isolation, strict framing, oversized recovery, path/rebind rejection, nested-command denial, finite malformed admission, queue backpressure, queued/running cancellation, deadlines, and restart negotiation. The dedicated governed gate passes in 21.97 seconds; all 125 checks remain read-only compliant; no-user-panic covers 332 production files; the lifecycle scheduler is decomposed to 690 counted lines under the 700-line no-new-debt target; source/test budgets, gate manifest, engineering contract, planning freshness, and all documentation gates pass. The full green front door passes in 176.53 seconds; changed-impact conservatively covers all 18 crates and 125 gates, its selected full suite completes in 75.91 seconds under the 120-second budget, network remains denied, and generated disk delta is 34.6 MB against the 1 GB allowance.
- [x] **R1.3.b Make cancellation real.** Killing a timed-out host/process/bridge operation must terminate and reap the child or isolate it in a killable worker. Add repeated-hang stress tests and prove no worker/thread leak. done 2026-07-11; evidence: `scripts/check_host_bridge_fault_injection.sh`; input: `hard-cancellation-bundle-sha256:fdfa3e1dc24165394b5235c6572ce681e902d5f213571456a87ad7eeb6f3f7c8`
  Evidence 2026-07-11: timed spawn-per-operation and persistent-stdio host bridges now start in fresh Unix process groups, enforce the deadline across process execution and pipe I/O, kill the complete group on expiry, synchronously reap the child, and join every bridge-owned I/O pump or persistent worker before returning a typed timeout. A timed-out persistent session is evicted, and the uncertain request is never retried; the next request starts a fresh session. Platforms without a supported process-tree primitive reject `timeout_ms` with a deterministic `bridge-policy` error before spawning instead of claiming cooperative cancellation as hard cancellation. The governed fault-injection gate executes 48 repeated hangs across both transports: 32 spawn-per-operation cases at 25 ms and 16 persistent cases at 200 ms. Fixtures record leader and descendant PIDs; tests prove every observed process is dead, child/worker join counters advance, active I/O pumps return to zero, stress completes within its isolated bound, and healthy requests succeed afterward. The same gate retains deterministic replay fault coverage for filesystem, network, process, and plugin bridges. It passes in 14.25 seconds with zero failures and no retained report mutation; the complete nine-test host-bridge unit group passes in normal parallel execution in 3.62 seconds. No-user-panic, source/test size, source decomposition, documentation, check/update-boundary, and the 125-entry gate manifest checks pass. The exact final source tree passes the full green front door in 190.03 seconds; its conservative 811-file fallback covers all 18 crates and all 125 gates in 85.90 seconds under the 120-second changed-impact budget, with zero network attempts and no generated-disk growth.
- [x] **R1.3.c Generate a pinned MCP interface from CLI schemas.** Implement the MCP 2025-11-25 profile over stdio first, including lifecycle, version/capability negotiation, cancellation, progress, errors, roots, resources, and tools. Expose parse, format, check, run, test, explain, search-symbol, get-card, diff, apply-patch, verify, replay, package, build, and the transactional session lifecycle without hand-maintaining a second semantic API. MCP Tasks remain an experimental, explicitly negotiated extension mapped onto Genesis durable-job semantics; core interoperability cannot depend on them. done 2026-07-14; evidence: `scripts/check_warm_protocol_contract.sh` and `scripts/check_cli_diagnostics_contract.sh`; input: `mcp-interface-bundle-sha256:a5d356567503da3de62b9cdd8ac5bc4638fab13ab00ed78aef09dd5c204c1c0b`
  Evidence 2026-07-14: `genesis mcp` implements the pinned MCP `2025-11-25` lifecycle over bounded newline-delimited JSON-RPC 2.0 stdio with protocol/capability negotiation, stdout purity, finite input/output/session/root/queue limits, strictly increasing progress, queued cancellation and active-result suppression, typed transport errors, and native plus sequential WASI execution. Client roots are requested only after initialization, accepted only as bounded canonical local `file://` directories beneath the configured boundary, and selected by exact negotiated URI; non-file, parent, absolute-argument, symlink, duplicate, inaccessible, unadvertised, and multi-root ambiguity cases fail closed. A reviewed exposure table selects 20 canonical CLI routes, including six transaction lifecycle tools, while names, descriptions, required/default/enum/type schemas, parent-option placement, boolean polarity, positional order, and argv spellings are derived and startup-validated against Clap; calls re-enter normal CLI dispatch, so no private semantic API exists. Canonical `parse`, bounded symbol search, exact generated card retrieval, and generated transaction begin/status execution complete the required authority surface. Seven `genesis://` resources expose the generated CLI/MCP profiles and frozen agent authorities. Every core tool declares `taskSupport = forbidden`; Tasks are unadvertised and task methods/augmented calls are rejected until a future explicitly negotiated extension maps them to durable Genesis jobs. Six white-box controls and four shipped-binary tests cover catalog drift, resource parsing, root escape, cancellation state, lifecycle, output purity, malformed/oversized frames, output truncation, task rejection, negotiated parse and transaction execution, and progress ordering. The MCP session remains decomposed below the 700-line ratchet. The governed warm/MCP gate, 179-code diagnostics/golden/repair gate, panic policy, selfhost dashboard, source decomposition, check/update contracts, and gate manifest pass locally; generated diagnostic, profile, card, symbol, golden, and repair identities are fresh and deterministic.
- [x] **R1.3.d Add transactional sessions.** Agents operate on content-addressed workspace snapshots; writes produce semantic patches, tests run against the snapshot, and commit/apply is explicit. done 2026-07-14; evidence: `scripts/check_warm_protocol_contract.sh`, `scripts/check_agent_authoring_bundle.sh`, `scripts/check_cli_diagnostics_contract.sh`, and `scripts/check_write_genesiscode_skill_conformance.sh`; input: `transactional-agent-session-bundle-sha256:c2230a4c45c74284150191a7dc0cdbf801cc348c4decac1eacd3d4ab7d7fc645`
  Evidence 2026-07-14: the closed `genesis/agent-transaction-v0.1` and `genesis/workspace-snapshot-v0.1` contracts define an open/applied/aborted lifecycle, ordered semantic-patch chain, exact-snapshot verification, sorted base-relative file manifest, and bounded content records. `session begin` recursively captures the package manifest, modules, declared capability policy, and local dependency closure as domain-separated BLAKE3 blobs and a snapshot identity; 4096-file and 256 MiB admission limits, regular-file and no-symlink-component checks, root confinement, canonical UTF-8 path material, object rehashing on reuse and before live writes, and chain validation fail closed. `session stage` materializes a fresh candidate, accepts only canonical Genesis semantic patches, applies through the existing patch engine, reruns obligations there, records patch/before/after/acceptance identities, and activates no partially failed candidate. `session test` rejects workspace tampering before binding obligation evidence to the exact current snapshot. `session apply` acquires a package-local lock, requires successful current-snapshot verification, rehashes both isolated and live trees, rejects stale live inputs, writes only managed files, verifies the result, and proves rollback to the base on write, post-write verification, or state-commit failure; `session abort` closes without live mutation. State and protocol results expose no absolute host paths. Twenty generated MCP tools now include all six lifecycle operations and prove begin/status execution through the normal CLI route. Six shipped transaction tests prove isolated staging, exact-snapshot testing, explicit apply, stale-base preservation, unverified, workspace-tampered, state-chain-tampered, and coherently content-addressed path-forgery rejection, absolute-path redaction, and lock reaping; four schema mutations reject open state, lifecycle broadening, missing verification, and unbounded snapshots. The 179-code diagnostic catalog includes every session code, and the generated patch card plus authoring skill mandate transaction use and hard-stop on integrity failure without direct-edit or policy-broadening fallback.
- [x] **R1.3.e Bound and observe sessions.** Enforce CPU, wall, steps, heap, output, effects, processes, and disk. Emit provenance and an audit summary without collecting mandatory remote telemetry. On stdin EOF or client disconnect, stop admission, drain an explicitly bounded set of already accepted responses when transport permits, cancel and reap the rest, and report deterministic completion/cancellation provenance so accepted work is never silently dropped. done 2026-07-14; evidence: `scripts/check_warm_protocol_contract.sh`, `scripts/check_no_user_panics.sh`, and `scripts/check_cli_diagnostics_contract.sh`; input: `agent-session-resources-bundle-sha256:b621bb593ba36a40d6a2168a8453311c97e0f56f658c05043d979ae70e11b727`
  Evidence 2026-07-14: warm and MCP sessions require finite wall, CPU, semantic-step, heap, combined-output, effect, process, workspace-disk-growth, bounded-drain-count, and drain-time limits whose canonical BLAKE3 identity is returned during initialization and in every terminal `genesis/agent-session-audit-v0.1`. Native macOS/Linux execution isolates each accepted command in a fresh worker process, applies kernel CPU/address-space/file-size ceilings where supported, samples the complete recursive process tree for aggregate CPU, resident memory, and process count even when host bridges create separate process groups, meters workspace growth without following symlinks, and kills then reaps the worker on cancellation or any hard resource breach. Unsupported native platforms fail closed before accepting work rather than claiming hard isolation; WASI-inline execution reports its exact enforcement profile. Request argv cannot weaken session ceilings, and effect dispatch uses a thread-local minimum of policy and session limits without cross-session leakage. Output is drained through one bounded channel so stdout/stderr cannot deadlock after overflow. EOF, disconnect, shutdown, deadline, queued cancellation, active cancellation, crash recovery, and oversized-frame fallback all stop admission, drain only the negotiated finite set, terminalize every accepted ID, and attach deterministic completion, cancellation, resource-exceeded, or not-started provenance without mandatory telemetry. The recursively closed session-resource and warm-protocol schemas, independent standard-library contract verifier, 13 schema/source negative controls, five warm unit tests, six MCP unit tests, 11 shipped warm tests, five shipped MCP tests, and six transaction tests prove all eight resource dimensions, descendant escape resistance, repeated hard termination, bounded EOF drain, exact audit propagation, override rejection, and zero silent drops. Strict Clippy, panic policy over 354 production files, source/test/decomposition budgets with a 690-line observed module ceiling, 185-code diagnostic catalog with deterministic replay and perfect 18-case repair utility, all 169 Quarto pages, planning freshness, documentation topology, and the 124-entry governed gate manifest pass on the exact source tree.
- [ ] **R1.3.f Meet AB-3 and AB-4.** Benchmark cold, warm, cache-hit, cache-miss, 100-module, 10k-module, parallel-agent, cancellation, and daemon-restart cases.

### R1.4 Training and evaluation corpus

- [x] **R1.4.a Publish a corpus manifest.** Version source provenance, license, generator identity, language profile, capability requirements, expected hashes, tests, difficulty, and intended train/dev/public-test/held-out role. done 2026-07-14; evidence: `scripts/check_agent_authoring_bundle.sh` and `crates/gc_cli/tests/cli_agent_index.rs`; input: `gc-agent-corpus-bundle-sha256:84eb06c34da775916dd970a52ff38e48f4d5e053a02070bb8654b4c5022d74b1`
  Evidence 2026-07-14: the recursively closed `GC_AGENT_CORPUS_v0.1` schema and content-addressed manifest publish five uniquely sorted corpus entries across train, development, and public-test roles. Eleven artifacts totaling 19,815 exact context bytes bind repository origin, per-file SHA-256, Apache-2.0/MIT policy, authored or deterministic generator identity, frozen `GC-AGENT-v0.3` profile identity, minimum capability operations, sealed executable test argv, four-level difficulty plus concept dimensions, and public versus commitment-only oracle exposure. The split contract reserves held-out as non-distributable and requires oracle separation without claiming R1.4.d's not-yet-built private evaluation set. The read-only validator rejects duplicate keys; unknown fields; host, parent, symlink, and escaped paths; stale profile/artifact/generator hashes; license or provenance rebinding; split broadening; held-out oracle leakage; prompt-selected profiles; shell test injection; context drift; duplicate assignment; and identity drift. Thirteen permanent mutation controls pass. The existing authoring-bundle gate owns validation without adding a governance entrypoint, the bounded `llms.txt` and documentation indexes expose the authority, and production `agent-index` publishes its exact path; all seven shipped-binary index tests pass.
- [ ] **R1.4.b Build canonical language examples.** Include pure functions, persistent collections, contracts, modules, sealed errors, effects, replay, packages, patches, tests, and resource failures. Pair valid examples with minimal invalid counterexamples.
- [ ] **R1.4.c Build task benchmarks.** Cover generation, completion, repair, refactor, policy minimization, replay investigation, performance repair, package migration, and deployment across increasing context sizes.
- [ ] **R1.4.d Protect held-out evaluation.** Keep test inputs or oracle details outside distributed training packs, publish hashes and later disclosure rules, rotate compromised sets, and label any contaminated model result.
- [ ] **R1.4.e Make scoring model-agnostic.** Deterministic tests score semantics, obligations, effects, patch minimality, resource use, and policy scope. Model-specific latency/cost is a separate dimension.
- [ ] **R1.4.f Add benchmark reproducibility.** Record model/runtime, prompt/card hashes, decoding, retries, tool protocol, host, and every candidate artifact. Permit fully local model runners through explicit effects.

### R1.5 Authoring skill vNext

- [ ] **R1.5.a Generate the skill from canonical inputs.** Replace duplicated prose across `.agents/skills/genesiscode-authoring`, `docs/write_genesisCode_skill.md`, and the versioned pack with generated cards plus a small hand-owned workflow policy.
- [ ] **R1.5.b Remove dead planning guidance.** Do not direct agents to an empty `upgrade_plan.md` as their ordinary work queue. Route active defects, roadmap tasks, user tasks, and exploratory work explicitly.
- [ ] **R1.5.c Add profile negotiation.** The skill identifies CLI/language/card versions, supported capabilities, target platform, available resources, and strictness before writing code.
- [ ] **R1.5.d Add compact workflows.** Provide author, repair, refactor, package, replay-debug, optimize, deploy, and self-host migration flows with exact stop/fail conditions.
- [ ] **R1.5.e Add safety invariants to tool calls.** The skill never edits policy, signing roots, generated evidence, bootstrap trust, or format versions incidentally.
- [ ] **R1.5.f Validate distribution.** Test clean install, offline use, stale-card rejection, token budgets, links, examples, and behavior with at least two independent agent/model families.

### R1.6 Agent-safe collaboration

- [ ] **R1.6.a Implement O(1)-logical workspace forks.** Use content-addressed snapshots and copy-on-write overlays so multiple candidates do not duplicate build trees or mutate one another.
- [ ] **R1.6.b Define deterministic candidate comparison.** Rank by required-test pass, obligation strength, capability minimization, semantic patch size, resource delta, and risk; tie-break by canonical patch hash.
- [ ] **R1.6.c Separate proposer, reviewer, and verifier identities.** Provenance records role, tool/model version, context bundle, and evidence. One agent may fill roles in development, but release profiles require independent verification for high-risk changes.
- [ ] **R1.6.d Prevent secret/context leakage.** Cards, diagnostics, logs, model effects, and evidence redact or seal secret material; add canary negative tests.
- [ ] **R1.6.e Separate instructions from untrusted content.** Source comments, package docs, diagnostics, logs, registry metadata, model output, and retrieved files carry provenance/trust labels and cannot silently become agent policy. Add prompt-injection, confused-deputy, and indirect tool-instruction corpora.
- [ ] **R1.6.f Bind tools to explicit intent and authority.** Each mutating or effectful agent call carries workspace identity, requested operation, capability scope, budget, and confirmation policy; stale, cross-workspace, broadened, or replayed requests fail closed.

**R1 exit criteria:** `GC-AGENT-v0.3` is frozen and versioned; compact cards meet AB-1/AB-2; diagnostics are structured across every user boundary; warm/MCP meets AB-3/AB-4 and survives cancellation/leak stress; the corpus and held-out policy are published; the authoring skill passes clean/offline installs; a pinned reference-agent run meets AB-5 through AB-8 without private repository context.

---

## R2. Runtime and resource foundation

**Goal:** finish the reference runtime, make memory/resource behavior part of semantics, and deliver a fast long-lived agent loop before adding aggressive execution tiers.

### R2.1 Complete the compiled interpreter

- [ ] **R2.1.a Implement free-variable analysis and minimal closure capture.** Closures retain only needed bindings; differential tests cover shadowing, recursion, nested patterns, and long-lived closure graphs.
- [ ] **R2.1.b Replace linked lookup on hot paths.** Resolve lexical locals to slots/frames where semantics permit; remove avoidable `Rc` frame walks and per-`let` environment allocation.
- [ ] **R2.1.c Close tail behavior.** Test at least 10 million bounded tail iterations in treewalk and compiled modes with constant host stack and declared step accounting.
- [ ] **R2.1.d Finish primitive call normalization.** Preserve n-ary uncurried paths, partial application behavior, error spans, and hashes without wrapper allocation regressions.
- [ ] **R2.1.e Close persistent collection budgets.** Bring normative map construction under PB-5, specify order/sharing, and test adversarial hash/shape/update workloads.
- [ ] **R2.1.f Keep the reference path readable.** Optimization-specific code remains separated from the semantic evaluator and is continuously differential-tested.
- [ ] **R2.1.g Retire workload-specific recognizers.** Delete `crates/gc_kernel/src/compiled_runtime/patterns.rs` from production dispatch and prohibit replacement recognizers keyed to benchmark source shape, symbol names, literal constants, or expected results. General optimizations must trigger from documented semantic properties, improve a diverse anti-overfit corpus, and preserve exact behavior under source-equivalent rewrites that defeat syntactic recognition.
- [ ] **R2.1.h Make PB-5 and PB-7 the first performance closure targets.** Profile allocation, hashing, shape transitions, parser/frontend routing, and self-host dispatch with normalized workloads; fix general data-structure and parser costs before further `fib` tuning. Meet the signed PB-5 map and PB-7 throughput bounds with confidence intervals, anti-overfit variants, cold/warm separation, and no semantic fast-path bypass.

### R2.2 Heap and lifetime semantics

- [ ] **R2.2.a Specify the value graph.** Document ownership, sharing, closure environments, cycles, finalization prohibition or semantics, weak references if any, and host-handle lifetime.
- [ ] **R2.2.b Choose and implement cycle handling.** Prove structural cycle prevention or add deterministic cycle collection/tracing. Long-lived warm sessions may not leak recursive closures or host handles.
- [ ] **R2.2.c Add allocation and heap metering.** Charge deterministic logical units independent of allocator internals; expose policy limits and sealed exhaustion errors.
- [ ] **R2.2.d Define out-of-memory behavior.** Preflight bounded allocations where possible, catch recoverable host allocation failures, and isolate workloads so an untrusted program cannot terminate the daemon.
- [ ] **R2.2.e Bound persistent sharing.** Add stress tests for adversarial update chains, retained roots, maps/vectors/strings, package graphs, logs, and snapshots.
- [ ] **R2.2.f Prove host-handle cleanup.** Files, sockets, processes, bridges, graphics, GPU, and model sessions close on success, error, cancellation, timeout, and daemon restart.

### R2.3 Startup and incremental execution

- [ ] **R2.3.a Build a deterministic Prelude snapshot.** Version and hash the compiled Prelude/environment; rebuild from source reproducibly; reject wrong profile, platform, or dependency identity.
- [ ] **R2.3.b Meet PB-6 and reverse startup drift.** Attribute process startup, catalog/card/schema loading, Prelude construction, and snapshot validation separately; remove or defer nonessential work before adding caches. Load the deterministic snapshot with bounded validation and no semantic dependence on cache state; preserve a source-bootstrap mode for verification and prove both modes agree.
- [ ] **R2.3.c Content-address frontend stages.** Cache parse, canonicalization, type/effect inference, obligations, compiled AST, and tests by complete semantic inputs.
- [ ] **R2.3.d Add precise invalidation.** Track imports, capability schemas, contracts, profile versions, and generated cards. Negative tests prove stale results cannot be reused.
- [ ] **R2.3.e Optimize large workspaces.** Incremental check/test and symbol/card retrieval meet AB-4 at 100 modules and have explicit SLOs at 10k modules.

### R2.4 Runtime observability without semantic leakage

- [ ] **R2.4.a Add deterministic logical profiles.** Count calls, steps, allocations, collection operations, effects, and cache decisions without embedding host addresses or unstable clocks in artifacts.
- [ ] **R2.4.b Keep wall-clock telemetry out of replay identity.** Performance reports may include host time as metadata, but semantic logs and hashes remain deterministic.
- [ ] **R2.4.c Provide explainable hot paths.** Map profile sites back to stable module/term IDs and generate optimization suggestions that require evidence before application.

**R2 exit criteria:** PB-1, PB-4, PB-5, PB-6, PB-7, and PB-8 pass on normalized workloads; closure/tail/heap/handle stress tests pass; all exhaustion paths return sealed or explicit errors; incremental invalidation negative controls pass; treewalk and compiled modes retain PB-9 semantic identity.

---

## R3. Validated execution tiers

**Goal:** gain predictable speed without enlarging the semantic TCB or fragmenting effects, errors, resource accounting, and canonical identity.

### R3.1 Bytecode contract and verifier

- [ ] **R3.1.a Specify a versioned bytecode.** Define instructions, constants, frames, closures, tail calls, spans, resource charges, serialization, validation, and compatibility/migration.
- [ ] **R3.1.b Implement an independent verifier.** Reject malformed control flow, stack/type errors, invalid constants, out-of-range indices, forged seals, unsupported effects, oversized artifacts, and version confusion before execution.
- [ ] **R3.1.c Build the VM outside TCB-A.** The bytecode producer and optimizer are untrusted; only verified artifacts execute, and authoritative semantics remain comparison oracle.
- [ ] **R3.1.d Match resource semantics.** Step/allocation/effect/heap budgets map to documented logical units; tier switching cannot evade a limit.
- [ ] **R3.1.e Add complete differential coverage.** Generate and mutate pure/effectful programs; compare values, canonical hashes, sealed errors, logs, scheduling, and limits across treewalk, compiled AST, and bytecode.
- [ ] **R3.1.f Meet PB-2.** Publish workload-normalized samples and regression thresholds without compromising PB-9.

### R3.2 WebAssembly/stage2 closure

- [ ] **R3.2.a Audit existing `wasmi` coverage.** Generate a per-form/per-primitive/per-effect matrix and replace unsupported silent fallback with explicit profile negotiation or failure.
- [ ] **R3.2.b Validate translation.** Compare source/CoreForm semantics with emitted module behavior and independently validate embedded source/artifact hashes.
- [ ] **R3.2.c Unify resource/capability enforcement.** WASI imports and host functions use the same deny-by-default policy, path normalization, cancellation, payload, and log contracts.
- [ ] **R3.2.d Fuzz the boundary.** Mutate modules, imports, memories, tables, traps, resource limits, and host responses; no malformed artifact reaches a panic or capability bypass.

### R3.3 Optimizer and tier controller

- [ ] **R3.3.a Give every rewrite an identity and precondition.** Rewrites are pure, versioned, independently testable, and disabled when their proof/precondition cannot be established.
- [ ] **R3.3.b Translation-validate optimized units.** Use equivalence checks, property tests, replay comparison, and bounded proof methods appropriate to the unit; fall back safely on uncertainty.
- [ ] **R3.3.c Make tier decisions reproducible.** Inputs are deterministic logical profiles and versioned thresholds, not hidden wall-clock races. Record decisions outside semantic identity where appropriate.
- [ ] **R3.3.d Add deoptimization/fallback tests.** Unsupported or failed optimization returns to a verified lower tier without duplicated effects or changed errors.

### R3.4 Conditional JIT decision gate

- [ ] **R3.4.a Measure the product gap after R2/R3.** Use flagship and agent workloads, not only `fib`, to determine whether bytecode/snapshot/warm execution misses v1 latency or throughput SLOs.
- [ ] **R3.4.b Write an architecture decision record.** Compare no JIT, Wasmtime/Cranelift, native AOT, and specialization by binary size, startup, platform reach, memory, compile time, trust, maintenance, and measured gain.
- [ ] **R3.4.c Implement only if justified.** A JIT is feature-gated, optional on constrained targets, translation-validated, sandboxed, and excluded from canonical outputs. Bytecode and `wasmi` remain complete fallbacks.
- [ ] **R3.4.d If implemented, meet PB-3.** Include code-cache integrity, W^X, architecture fuzzing, deterministic tier decisions, and safe deoptimization.

**R3 exit criteria:** versioned bytecode and independent verification pass adversarial tests; PB-2 and PB-9 pass; stage2 coverage has no undeclared fallback; resource/capability behavior is identical across tiers. R3.4 may close with a documented "not required for v1" decision if product SLOs are met.

---

## R4. Self-host semantic authority and bootstrap closure

**Goal:** make the toolchain above a minimal host genuinely GenesisCode-authored, independently verifiable, and reproducibly bootstrapped.

### R4.1 Define self-host truth precisely

- [ ] **R4.1.a Publish the stage0 contract.** Keep TCB-A limited to the pure evaluator, immutable values/primitives, and seal machinery. Enumerate bytecode/artifact verification and the effect-host ABI as separate, explicitly layered trust domains; state exactly which host semantics remain trusted and why.
- [ ] **R4.1.b Define closure levels.** `H0 routed`, `H1 GenesisCode implementation`, `H2 GenesisCode production authority`, `H3 reproducible bootstrap fixpoint`, `H4 independently reimplemented/conformant`.
- [ ] **R4.1.c Build a semantic-ownership ledger.** For every command and semantic decision, name spec authority, producing implementation, production authority, host binding, verifier, fallback reachability, tests, and H-level.
- [ ] **R4.1.d Correct status documents.** `SELFHOST_CUTOVER`, module-boundary migration tables, scorecards, and dashboards must never equate a `.gc` wrapper or route with H2/H3 closure.
- [ ] **R4.1.e Enforce the boundary structurally.** Crate dependency rules prevent stage0 from importing CLI, package, registry, optimizer, self-host frontend, or ambient effect semantics.

### R4.2 Migrate semantic authorities

Each component follows the same sequence: normative corpus -> GenesisCode implementation -> differential verifier -> production switch -> strict no-fallback profile -> Rust authority removal/demotion -> ownership-ledger update.

- [ ] **R4.2.a Frontend and canonicalization.** Parser, formatter, CoreForm lowering/printing, canonical hashes, spans, and profile migration become GenesisCode-produced with a minimal independent verifier for identity-critical artifacts.
- [ ] **R4.2.b Type and effect checker.** Constraint generation, rows, contracts, diagnostics, and incremental dependency logic become GenesisCode-authoritative; Rust retains only test oracle or is removed after sustained parity.
- [ ] **R4.2.c Patch and refactor engine.** Semantic diff/apply/merge, preconditions, conflict explanations, and patch minimization become GenesisCode-authoritative.
- [ ] **R4.2.d Obligation and policy decision logic.** Obligation generation/evaluation and policy composition become GenesisCode-authored. Host code enforces already-authorized effects but does not invent policy outcomes.
- [ ] **R4.2.e Package, registry, and VCS logic.** Manifest/lock resolution, graph solving, evidence verification, publish decisions, snapshot/diff/merge, and migration become GenesisCode-authoritative.
- [ ] **R4.2.f Compiler, optimizer, linker, and builder.** Bytecode/Wasm production, translation-validation orchestration, snapshots, target builds, and artifact manifests become GenesisCode-authored.
- [ ] **R4.2.g CLI and agent orchestration.** Command dispatch, warm/MCP schemas, card/docs generation, gate planning, and release assembly run from self-host artifacts without hidden Rust command semantics.

### R4.3 Reduce host complexity

- [ ] **R4.3.a Split oversized host crates.** Decompose `gc_effects` and `gc_cli_driver` by generated capability/command interfaces and shrink them as R4.2 migrates authority; meet GB-7.
- [ ] **R4.3.b Generate repetitive bridges.** One schema produces Rust host bindings, Prelude wrappers, capability docs/cards, MCP schemas, codecs, and parity tests.
- [ ] **R4.3.c Audit all host escapes.** Inventory FFI, process, dynamic library, WASI, GPU/UI, plugin, and model bridges; require capability and deterministic boundary contracts.
- [ ] **R4.3.d Remove legacy authorities.** Delete or archive `old_bootstrap/` and obsolete Rust paths only after the release profile proves no production/test dependency and history preserves recovery instructions.

### R4.4 Bootstrap fixpoint and trusting-trust defense

- [ ] **R4.4.a Define stages.** Stage0 builds stage1 from pinned source; stage1 builds stage2; stage2 builds stage3. Compare canonical artifacts and explain every intentionally host-specific envelope field.
- [ ] **R4.4.b Make builds hermetic.** Pin source, dependencies, profiles, environment, locale, paths, ordering, numeric behavior, and timestamps. Normalize or exclude nondeterministic container metadata.
- [ ] **R4.4.c Require cross-host fixpoint.** macOS arm64 and Linux x86_64 are tier-1 minimum; add a second independent builder implementation and tier-2 hosts before v1.
- [ ] **R4.4.d Add diverse double compilation.** Rebuild critical toolchain artifacts through independently built stage0/verifier paths and investigate any mismatch before release.
- [ ] **R4.4.e Publish bootstrap witnesses.** Include source tree hash, stage hashes, build graph, toolchain identity, commands, environment, and independent verification result.

### R4.5 Replaceability and conformance

- [ ] **R4.5.a Publish a kernel conformance suite.** Cover evaluation, seals, errors, canonical values/hashes, resource limits, and malformed/adversarial inputs independent of Rust implementation details.
- [ ] **R4.5.b Publish effect-host conformance.** Cover capability decisions, normalized paths, logs/replay, cancellation/kill/reap, payload limits, scheduling, and host errors.
- [ ] **R4.5.c Validate an independent implementation.** At least one independently authored stage0 or verifier passes the normative suite before the strongest replaceability claim.

**R4 exit criteria:** every production semantic decision has an ownership-ledger row; all required components reach H2; stage0 is contractually minimal; stage2/stage3 are byte-identical on tier-1 hosts; DDC and negative controls pass; legacy authorities are unreachable or removed; an independent verifier/conformance implementation validates release artifacts.

---

## R5. Stable language and platform contract

**Goal:** freeze a coherent v1 language profile and complete the semantics needed by real agent-authored systems without uncontrolled surface growth.

### R5.1 Audit and freeze existing semantics

- [ ] **R5.1.a Reconcile paper, handoff, specs, parser, Prelude, types, and all execution tiers.** Generate a normative-form matrix and reject undocumented behavior.
- [ ] **R5.1.b Promote effect rows deliberately.** Audit existing `gc_types` row inference, polymorphism, strictness, diagnostics, and module boundaries; close unsoundness/completeness gaps before declaring GA.
- [ ] **R5.1.c Freeze error and pattern behavior.** Exhaustiveness, duplicate bindings, guards if supported, sealed errors, and span provenance must match across tiers.
- [ ] **R5.1.d Specify numeric profiles.** Integers, overflow, division, floats/NaN, decimal/bigint if included, serialization, hashing, and GPU/backend differences must be deterministic or explicitly profile-scoped.
- [ ] **R5.1.e Specify text/path behavior.** Unicode normalization, byte strings, graphemes, path separators, case sensitivity, locale independence, and base-relative error payloads are explicit.

### R5.2 Deterministic structured concurrency

- [ ] **R5.2.a Audit the existing concurrency contract/runtime.** Reconcile `CONCURRENCY_v0.1.md`, schedule logs, task IDs, cancellation, joins, errors, and resource accounting.
- [ ] **R5.2.b Add structured scopes.** No orphan tasks; parent cancellation and failure propagation are deterministic and replayable.
- [ ] **R5.2.c Specify communication.** Channels/select/races, backpressure, fairness, ordering, deadlines, and closed-channel behavior have deterministic schedule facts.
- [ ] **R5.2.d Enforce global and per-scope bounds.** Task count, queue depth, messages, bytes, steps, effects, processes, and host workers cannot bypass parent policy.
- [ ] **R5.2.e Differential/model-check schedules.** Explore bounded interleavings and replay every accepted trace across tiers/hosts.

### R5.3 Modules, contracts, and compatibility

- [ ] **R5.3.a Freeze module resolution.** Content identity, imports/exports, visibility, cycles, profile constraints, workspace overrides, and package boundaries are deterministic.
- [ ] **R5.3.b Complete contract composition.** Define blame, refinement/shape identity, effect interaction, generics/parametricity where supported, and optimization preconditions.
- [ ] **R5.3.c Add profile negotiation.** Packages declare minimum/exact compatible language, capability, artifact, and target profiles; unsupported combinations fail before execution.
- [ ] **R5.3.d Ship migration tooling.** Canonical semantic patches upgrade syntax/APIs/formats; dry-run explains identity/effect changes and preserves provenance.
- [ ] **R5.3.e Publish support policy.** v1.x compatibility, deprecation windows, security exceptions, format readers, and end-of-life are explicit.

### R5.4 Standard library and capability surface

- [ ] **R5.4.a Define the v1 stdlib matrix.** Pure data/text/encoding/math/testing plus capability-backed filesystem, process, network, time, randomness, crypto, data, service, and model operations.
- [ ] **R5.4.b Keep effect APIs capability-shaped.** No ambient global handles; resources use scoped ownership, bounded streams, explicit closure, and sealed host errors.
- [ ] **R5.4.c Finish robust codecs/protocols.** Canonical JSON/CBOR or selected formats, HTTP, WebSocket, streaming, TLS policy, and package/evidence codecs have fuzzed incremental parsers.
- [ ] **R5.4.d Define crypto correctly.** Use audited host/provider primitives behind explicit capabilities; never invent cryptography in the kernel; expose algorithm/profile/version and deterministic test vectors.
- [ ] **R5.4.e Treat model inference as an effect.** Version provider/model/weights identity, prompt/input hash, sampling, token/resource budget, secret policy, output provenance, and replay mode. Fully local providers are first-class.

### R5.5 Safe FFI and component extension

- [ ] **R5.5.a Adopt a component boundary.** Use the WebAssembly Component Model and WIT with an explicitly pinned WASI 0.3 profile for native async, streams, futures, and cancellation, plus a tested WASI 0.2 compatibility/virtualization path where ecosystem reach requires it. Define the exact Genesis value/effect/resource mapping and treat native escape hatches as higher-risk profiles.
- [ ] **R5.5.b Generate bindings and capability manifests.** Types, ownership, errors, resources, limits, and evidence derive from one schema.
- [ ] **R5.5.c Sandbox extensions.** Memory, CPU, calls, effects, filesystem/network, cancellation, and lifecycle are bounded; malformed components cannot panic the host.
- [ ] **R5.5.d Make extension behavior replayable.** Pure components are content-addressed; effectful calls log canonical request/response or an explicit non-replayable policy decision.

**R5 exit criteria:** the v1 profile has no spec/implementation matrix gaps; effect rows and concurrency meet L3; numeric/text/path behavior is cross-host deterministic; stdlib APIs satisfy ownership/resource rules; compatibility and migrations work on the full corpus; extension negative controls pass.

---

## R6. Packages, registry, builds, and deployment

**Goal:** let agents ship verifiable systems to the declared target matrix using infrastructure that can run entirely on the user's own machines.

### R6.1 Package and workspace completion

- [ ] **R6.1.a Freeze manifest/lock schemas.** Canonical dependency identities, features/profiles, capabilities, targets, evidence requirements, source substitutions, and migrations are versioned.
- [ ] **R6.1.b Make resolution deterministic.** Same repository snapshot and policy produces the same graph or exact conflict proof across hosts; no unlogged ambient registry state.
- [ ] **R6.1.c Add workspace-scale operations.** Incremental resolution, shared caches, semantic diffs, atomic updates, offline mode, vendoring, and policy review meet large-workspace budgets.
- [ ] **R6.1.d Verify lifecycle scripts by construction.** Prefer declarative GenesisCode build actions; any host process hook requires explicit capability, sandbox, provenance, and noninteractive behavior.

### R6.2 Self-hosted registry trust

- [ ] **R6.2.a Ship a local registry distribution.** Single-node/offline use is complete; replication/mirroring is optional. No mandatory external account or hosted control plane.
- [ ] **R6.2.b Add transparency and immutability.** Append-only publish records, inclusion/consistency proofs, content-addressed blobs, namespace policy, and mirrored verification.
- [ ] **R6.2.c Implement signing and rotation.** Profile TUF-compatible threshold roles, delegated scopes, offline roots, freshness, rollback/freeze protection, compromise recovery, expiry, and reproducible key-policy tests; bind package evidence/transparency data without weakening TUF client rules.
- [ ] **R6.2.d Enforce evidence policy.** Registry acceptance checks source/artifact identity, compatibility, capabilities, obligations, reproducibility level, SBOM, provenance, and malware/policy scans without trusting publisher prose.
- [ ] **R6.2.e Support air-gapped promotion.** Export/import bundles preserve proofs, policy, revocations, dependency closure, and audit history.

### R6.3 Build target matrix

- [ ] **R6.3.a Define target profiles.** Native CLI/service, WASI, browser worker, edge/serverless, OCI image, and portable component are v1 candidates. Mobile app embedding is promoted only if flagship proof meets L4.
- [ ] **R6.3.b Implement `genesis build`.** One self-hosted build graph emits target artifacts, capability manifests, runtime requirements, a profiled SPDX SBOM, in-toto/SLSA provenance, and Genesis evidence pointers.
- [ ] **R6.3.c Make builds reproducible.** Normalize paths/timestamps/order/archives; pin base images and SDK identities; compare independent outputs.
- [ ] **R6.3.d Minimize artifacts.** Tree-shake unused Prelude/capabilities, strip evidence duplication without losing verification, and report size/startup/memory budgets per target.
- [ ] **R6.3.e Validate target parity.** Normative programs produce the same semantic result/effects or an explicitly declared target limitation.

### R6.4 Operations and lifecycle

- [ ] **R6.4.a Generate deploy plans, never hidden imperative magic.** Agents receive a reviewable diff of artifacts, capabilities, secrets, network exposure, storage, rollback, and evidence.
- [ ] **R6.4.b Add health, migration, rollback, and recovery contracts.** Service upgrades and data migrations are bounded, idempotent where required, observable, and reversible or explicitly irreversible.
- [ ] **R6.4.c Handle secrets safely.** Secret values never enter canonical source, logs, diagnostics, model contexts, or evidence; only references/policy and redacted attestations do.
- [ ] **R6.4.d Verify deployment provenance.** Running instances can report the exact package/artifact/evidence/profile identity without exposing secrets.

**R6 exit criteria:** deterministic offline resolution and package builds pass tier-1 hosts; a self-hosted registry supports publish/mirror/revoke/recover; target builds meet reproducibility and semantic-parity gates; at least native, WASI, browser, and OCI profiles reach L4; deploy/rollback/secret negative controls pass.

---

## R7. Assurance, formalization, and security

**Goal:** turn the trust story into independently checkable evidence focused on the smallest and highest-impact boundaries first.

### R7.1 Layered test strategy

- [ ] **R7.1.a Classify the suite.** Unit, semantic golden, negative boundary, property, differential, mutation, fuzz, model-check, integration, host-matrix, benchmark, proof, and release tests have explicit profiles and owners.
- [ ] **R7.1.b Add grammar/AST/CoreForm generators and shrinkers.** Generate valid and near-valid terms with effect/resource/profile annotations; preserve failure while minimizing counterexamples.
- [ ] **R7.1.c Fuzz every parser and decoder.** Source, CoreForm/artifacts, GCLOG, evidence, packages, locks, patches, bytecode, Wasm/component, protocols, and registry inputs must be bounded and panic-free.
- [ ] **R7.1.d Add fault injection.** Partial reads/writes, ENOSPC, permission races, process hangs, crashes, corrupt caches, lost network, clock anomalies, cancellation, and concurrent updates fail closed.
- [ ] **R7.1.e Use mutation testing on trust checks.** Demonstrate that tests fail when capability, seal, replay, signature, hash, bootstrap, and evidence validation is intentionally weakened.

### R7.2 Formal models and proofs

- [ ] **R7.2.a Mechanize the pure core.** Define syntax, values, evaluation, determinism, substitution/environment relation, errors, and canonicalization in Lean or the chosen proof system.
- [ ] **R7.2.b Prove seal properties.** User terms cannot forge protected variants; boundary opening preserves the intended abstraction.
- [ ] **R7.2.c Model effect dispatch/replay.** Prove state-machine determinism, complete fact checking, fail-closed unknowns, and replay equivalence under the specified host assumptions.
- [ ] **R7.2.d Prove bytecode safety/refinement.** Verified bytecode cannot violate stack/frame/seal invariants and refines authoritative evaluation for the covered subset; track proof coverage explicitly.
- [ ] **R7.2.e Model deterministic concurrency.** Prove schedule-log/replay properties for the core task/channel calculus and connect executable tests to extracted/model traces.
- [ ] **R7.2.f Verify critical canonical codecs.** Prove or independently cross-check injectivity/canonicality assumptions used for content identity.
- [ ] **R7.2.g Establish type/effect soundness.** Prove progress/preservation or an equivalent executable safety theorem for the declared v1 fragment, including row/effect dispatch and sealed boundary behavior; publish machine-checked coverage and every excluded feature.

### R7.3 Host and supply-chain hardening

- [ ] **R7.3.a Threat-model every boundary.** Kernel, Prelude, bridges, processes, network/server, plugins/components, model providers, package sources, registry, build workers, CI, signing, and update channels.
- [ ] **R7.3.b Sandbox host execution.** Least privilege, hard kill/reap, syscall/filesystem/network restrictions where available, bounded IPC, and platform-specific degradation policy.
- [ ] **R7.3.c Pin and audit dependencies.** Provenance, licenses, advisories, minimal features, checksums, vendoring/offline mirror, and update policy are part of release evidence.
- [ ] **R7.3.d Harden release keys and builders.** Isolated signing, threshold/recovery policy, reproducible unsigned artifacts, attestations, builder identity, and independent mirrors.
- [ ] **R7.3.e Run external review.** Commission or invite independent review of TCB, effects/replay, package/registry trust, bytecode verifier, and bootstrap process; track findings in `upgrade_plan.md`.

### R7.4 Reliability and scale

- [ ] **R7.4.a Soak long-lived daemons/services.** 100k+ requests, cancellation storms, malformed clients, package churn, cache corruption, and constrained memory/disk.
- [ ] **R7.4.b Test adversarial complexity.** Parser/typechecker/collections/solver/log/registry inputs have documented worst-case or enforced budgets.
- [ ] **R7.4.c Validate disaster recovery.** Registry restore, key compromise, corrupt release mirror, bad migration, failed bootstrap, and incompatible package release have rehearsed runbooks and automated tests.

**R7 exit criteria:** all trust boundaries have threat models and mutation-tested negative controls; fuzz/sanitizer/property soak has 30 clean days with no unresolved P0/P1; required formal statements are checked and coverage limits are published; independent review findings are closed or explicitly release-blocking; supply-chain/recovery drills pass.

---

## R8. Product proof and adoption

**Goal:** prove GenesisCode is useful rather than merely feature-rich, especially for the user's own AI systems.

### R8.1 Reference agent integration

- [ ] **R8.1.a Integrate at least two agent stacks.** One may be the user's AI system; one must be independently implemented. Both use the versioned cards/diagnostics/MCP protocol rather than private repo prompts.
- [ ] **R8.1.b Exercise the complete loop.** Plan, author, check, run, inspect effects, repair, test, minimize capability policy, package, build, deploy, replay, and explain provenance.
- [ ] **R8.1.c Publish failure taxonomy.** Attribute failures to language ambiguity, card retrieval, diagnostics, model reasoning, tool protocol, runtime, policy, performance, or missing domain support.
- [ ] **R8.1.d Ratchet from evidence.** Roadmap priority responds to repeated measured failure classes, not isolated demo success or benchmark gaming.

### R8.2 Ten-archetype gauntlet

- [ ] **R8.2.a Pure library and CLI.** Parsing/transformation, tests, package, reproducible native/WASI builds.
- [ ] **R8.2.b HTTP/WebSocket service.** Structured concurrency, TLS policy, persistence, cancellation, replayable tests, OCI deploy/rollback.
- [ ] **R8.2.c Data pipeline.** Streaming, bounded memory, deterministic transforms, checkpoint/recovery, provenance.
- [ ] **R8.2.d MCP/tool server.** Generated schemas, authentication policy, bounded tools, audit/replay.
- [ ] **R8.2.e Browser application.** Worker/runtime boundary, deterministic state, package/build evidence, sandboxed effects.
- [ ] **R8.2.f Package and registry workflow.** Author, resolve, sign, publish, mirror, revoke, migrate, offline verify.
- [ ] **R8.2.g Parallel agent refactor.** Candidate forks, semantic merge/conflict, independent verifier, minimal patch selection.
- [ ] **R8.2.h GPU or numeric workload.** Pure kernel semantics, deterministic tolerance/profile, backend evidence, CPU fallback.
- [ ] **R8.2.i Mobile embedding pilot.** One iOS and one Android host integration using preserved platform toolchains; promote only if reproducibility, lifecycle, size, and debugging meet L4.
- [ ] **R8.2.j Self-host toolchain change.** Agent updates a non-TCB compiler/tooling component, bootstraps, differentially verifies, and produces a reviewable evidence bundle.

### R8.3 Flagship systems

- [ ] **R8.3.a Ship at least five maintained programs.** Required set: replay-audited data service, MCP tool server, static site/build tool, package registry deployment, and one compute/visual/mobile application.
- [ ] **R8.3.b Treat examples as products.** Each has tests, threat model, capability policy, budgets, packages, deploy/rollback, operations guide, and pinned evidence.
- [ ] **R8.3.c Measure maintenance.** Perform seeded upgrades, dependency changes, defect repairs, policy tightening, and profile migrations through agents; report patch quality and human review time.

### R8.4 Minimal human surface

- [ ] **R8.4.a REPL:** structured values/effects, multiline editing, history privacy, resource controls, profile selection, and replay inspection.
- [ ] **R8.4.b LSP:** parser/type/effect diagnostics, semantic navigation, format, safe actions, capability/policy insights, and incremental performance from shared schemas.
- [ ] **R8.4.c Playground:** locally hostable, browser-sandboxed, capability-minimal, version-pinned, shareable by content hash, and incapable of implying unsupported host effects.
- [ ] **R8.4.d Documentation spine:** one getting-started path, language reference, agent SDK, capability reference, operations/security, package/registry, self-host/bootstrap, and contribution guide generated from canonical sources.
- [ ] **R8.4.e Ten-minute proof:** a clean user evaluates a pure program, runs an effect under policy, inspects the log, and replays it in <=10 minutes without hidden setup.

**R8 exit criteria:** AB-5 through AB-8 meet v1 targets on held-out tasks; at least 9/10 archetypes pass without human code edits; five flagship systems reach L4; seeded maintenance exercises succeed; human quickstart and tooling meet their SLOs; failures and unsupported domains are published.

---

## R9. v1 Trust Release

**Goal:** freeze, reproduce, attest, publish, operate, and support GenesisCode v1 without overstating any claim.

### R9.1 Freeze and release candidates

- [ ] **R9.1.a Freeze v1 profiles.** Language, CoreForm, hashes, logs/replay, evidence, package/lock, patch, bytecode, snapshot, component ABI, bootstrap, and target profiles receive final IDs.
- [ ] **R9.1.b Enforce release branches by evidence.** Only reviewed semantic patches with required gates and compatibility decisions enter release candidates.
- [ ] **R9.1.c Run at least two candidates.** Each receives full host matrix, 30-day soak overlap where possible, agent gauntlet, bootstrap, reproducibility, migration, rollback, security, and performance review.

### R9.2 Reproducible distribution

- [ ] **R9.2.a Produce signed source and binary bundles.** Include installers/packages for supported hosts, self-host artifacts, offline dependency/registry options, profiled SPDX inventory, in-toto/SLSA provenance, licenses, symbols, and independent verification instructions.
- [ ] **R9.2.b Reproduce independently.** Two builders reproduce every tier-1 artifact; discrepancies block release and are preserved as evidence.
- [ ] **R9.2.c Publish E4 attestations and mirrors.** Users can verify source-to-artifact, bootstrap witness, package graph, target profile, and evidence offline.
- [ ] **R9.2.d Test install/upgrade/uninstall.** Clean, existing v0.x, air-gapped, low-disk, and rollback paths preserve user data and report exact compatibility issues.

### R9.3 Operations and governance

- [ ] **R9.3.a Publish security policy.** Scope, reporting, supported versions, severity, disclosure, patch/signing procedure, and compromise recovery.
- [ ] **R9.3.b Publish compatibility governance.** Decision process, profile evolution, deprecation, emergency exceptions, and independent implementation input.
- [ ] **R9.3.c Establish incident response.** Rehearse bad release, compromised key, malicious package, registry split, verifier defect, replay defect, and bootstrap mismatch.
- [ ] **R9.3.d Make telemetry optional.** The project works fully offline; any opt-in metrics are transparent, minimal, redacted, erasable, and never required for evidence.
- [ ] **R9.3.e Audit public claims.** Every performance, self-host, security, agent-success, domain-support, and reproducibility statement links to an E3/E4 evidence ID and states limitations.

### R9.4 Final acceptance

- [ ] **R9.4.a No open release blockers.** `upgrade_plan.md` has no open P0/P1, all required ledger rows are L5 on tier-1 platforms, and accepted lower-level items are explicitly excluded from v1 claims.
- [ ] **R9.4.b Independent verification succeeds.** A clean verifier environment checks the release, conformance corpus, bootstrap witness, registry/package proofs, and signatures without the producing CLI.
- [ ] **R9.4.c User AI pilot succeeds.** The user's target AI systems complete the declared held-out workflows with the published SDK and no private migration prompt.
- [ ] **R9.4.d Publish v1.0 and preserve it.** Tag, sign, mirror, archive source/dependencies/evidence, publish migration/support dates, and create the post-v1 baseline.

**R9 exit criteria:** every R9 task is complete; all M6 claims have E4 attestations; independent reproduction and verification pass; operational drills pass; the release is installable and usable offline; public claims exactly match the capability ledger.

---

## F. Post-v1 frontier program

Post-v1 work is intentionally outside the v1 critical path. It may be researched earlier, but it cannot weaken compatibility, trust, or maintenance of the released profile.

### F1 Bounded self-improvement

- [ ] **F1.a Roadmap/task compiler.** Extend R0.1.f's reviewed static manifest into a proposal compiler that can derive candidate dependencies, risk, context, tests, evidence, rollback, and acceptance policy, but requires independent validation before changing the approved manifest and never treats prose or model output as authority.
- [ ] **F1.b Supervised improvement loop.** Retrieve cards, fork bounded candidates, propose semantic patches, run focused/full gates by risk, minimize/ablate, compare deterministically, and stop for human acceptance.
- [ ] **F1.c Protected governance roots.** Agents cannot unilaterally alter capability policy, signing roots, evidence verification, release rules, bootstrap trust, or their own approval constraints.
- [ ] **F1.d Independent roles.** High-risk changes require independently provisioned proposer, reviewer, verifier, and release authority with conflict-of-interest records.
- [ ] **F1.e Longitudinal evaluation.** Track accepted/rejected patches, regressions, repair time, evidence cost, human review load, and benchmark overfitting.

### F2 Verified optimization research

- [ ] **F2.a Proof-carrying pure rewrites.** Synthesize/enumerate CoreForm rewrites with machine-checked preconditions or translation validation.
- [ ] **F2.b Whole-program specialization.** Use deterministic profiles to specialize contracts, closures, effects-free paths, data layouts, and target backends while preserving fallback semantics.
- [ ] **F2.c Validated JIT/AOT expansion.** Add architectures/engines only where measured product value exceeds binary, startup, memory, security, and maintenance cost.
- [ ] **F2.d Verified incremental/distributed cache.** Share content-addressed frontend/build/evidence artifacts across machines with corruption detection and no semantic dependence on hits.

### F3 Distributed and hardware-aware execution

- [ ] **F3.a Deterministic distributed task graphs.** Explicit partitioning, retries, idempotence, data identity, capability delegation, and replay envelopes.
- [ ] **F3.b Portable accelerator profiles.** SIMD/GPU/NPU backends specify numeric/order semantics, translation validation, resource evidence, and CPU fallback.
- [ ] **F3.c Federated self-hosted registries/builders.** Transparency consistency, policy federation, offline roots, reproducible builders, and compromise containment.

### F4 Ecosystem governance and replaceability

- [ ] **F4.a Multiple conformant implementations.** Encourage independent stage0, verifier, VM, and tooling implementations; resolve spec ambiguities through tests and versioned decisions.
- [ ] **F4.b Evidence-weighted standards process.** Language/profile proposals include compatibility, agent-context cost, implementation/proof burden, migration, and measured user value.
- [ ] **F4.c Preserve a small language.** Track semantic/syntax/context complexity budgets and require deletion/simplification opportunities alongside additions.
- [ ] **F4.d Research ledger.** Every frontier claim records hypothesis, baselines, raw data, machine/tool versions, negative results, reproduction, and limitations.

---

## 10. Sequencing and parallelization

```text
                         +---------------- formal models ----------------+
                         |                                               v
R0 -> R1 -> R2 -> R3 -> R5 core freeze -> R4 -> R5 completion -> R6 -> R7 -> R8 -> R9
 |     |     |                    |       |                              |
 |     |     +-- profiling/JIT ADR+       +-- ownership/bootstrap -------+-> v1.0
 |     +-- human UX prototypes ----------------------------------------->|
 +-- registry/build prototypes ---------------------------------------->|

After v1: F1 bounded self-improvement, F2 validated optimization,
          F3 distributed/hardware execution, F4 governance/replaceability
```

Practical order:

1. Close the 2026-07-14 stabilization tranche first: publish a checkpoint, run remote CI, fix denied-effect replay parity, remove load flakiness and nested default pipelines, clear warnings, and bound generated state.
2. Ship R1 as soon as its gates pass, concentrating on transactional sessions, corpus/held-out evaluation, generated skill vNext, and instruction/content separation. This is the point where training and integrating the user's agents becomes responsible.
3. Finish R2 before relying on a long-lived agent daemon or advertising robust services.
4. Build R3 bytecode after the resource contract is fixed. Decide JIT only from measured product gaps.
5. Start R4.1's ownership/TCB ledger early, but freeze each relevant R5.1/R5.3 core contract before switching its R4.2 production authority. Sequence migration by semantic risk and build leverage, not easiest wrapper count.
6. Complete the remaining R5 platform contract before claiming package/target compatibility in R6.
7. Run R7 continuously, but require its soak/formal/external-review gates for release candidates.
8. Let R8 flagship failures drive final priorities. Do not add new domains to hide weaknesses in existing ones.
9. Treat R9 as a release engineering phase with independent authority, not a documentation ceremony.

Workstream dependency matrix:

| Workstream | Hard prerequisites | May proceed in parallel | Hard-blocks |
|---|---|---|---|
| R0.1 truth sources | none | R0.3 prerequisite discovery, R7 formal-model scoping | every generated status/release claim |
| R0.2 evidence lifecycle | R0.1 authority decisions | R0.3-R0.5 | M0, every L3-L5 promotion |
| R0.3 hermetic versions | R0.1 source-of-truth rules | R0.2, R0.4 | reproducible checks, profiles, R1 card/version negotiation |
| R0.4 gate/resource architecture | R0.1 ledger and R0.3 tool identities | R0.2, R0.5 | affordable continuous evidence and release execution |
| R0.5 normalized baselines | R0.3 host/tool profiles and R0.4 telemetry schema | late R0.2 verifier work | every performance claim and JIT decision |
| R1.1 profile/cards | R0 version/compatibility registry | R1.2 diagnostic catalog | R1.3 schema generation, R1.4 corpus, R1.5 skill |
| R1.2 diagnostics | R1.1 profile IDs and R0 evidence schemas | R1.1 cards | R1.4 repair benchmark and M1 |
| R1.3 warm/MCP | R1.1 schemas; R2.2 semantics required before final daemon sign-off | R1.4-R1.6 prototypes | agent product loop, AB-3/AB-4 |
| R1.4 corpus | R1.1 profile and R1.2 diagnostic IDs | R1.3 implementation | R1.5 validation, R8 held-out proof |
| R1.5 skill | generated R1.1/R1.2/R1.4 inputs | R1.6 | M1 training readiness |
| R1.6 collaboration | R0 evidence/identity; R1.3 transactional API for completion | R1.4-R1.5 | safe parallel-agent proof |
| R2.1 interpreter | R0 normalized semantic/perf corpus | R2.2 heap specification | R2.3 snapshots, R3 execution tiers |
| R2.2 heap/resources | R0 evidence and resource schemas | R2.1 | final R1.3 daemon sign-off, all R3 tiers |
| R2.3 startup/incremental | R2.1 stable compiled representation and R2.2 accounting | R2.4 | AB-3/AB-4, R3 artifact caches |
| R3 bytecode/Wasm/tiering | R2 semantic and resource contracts | R4.1 ledger, R5.1 semantic audit | R4.2 compiler authority, M2 |
| R5.1 and R5.3 core freeze | R0 compatibility registry; R2/R3 discrepancies resolved | R4.1 ownership mapping | every corresponding R4.2 authority switch |
| R4.1 ownership/TCB | R0 truthful ledgers | R2-R3 and R5 core audit | all R4.2 migration acceptance |
| R4.2-R4.5 self-host closure | relevant R5.1/R5.3 contract frozen; R3 verified artifact path | remaining R5.2/R5.4/R5.5 implementation | M3 and trusted self-hosted R6 orchestration |
| R5.2/R5.4/R5.5 completion | R5 core profile plus R2/R3 resource semantics | late R4 migration | R6 compatibility and target claims |
| R6 ecosystem/deployment | R5 compatibility/ABI freeze; self-host build authority for GA | R7 continuous assurance | M4 and flagship deployment |
| R7 assurance | models/scaffolding may start immediately; release claims require frozen subjects | all phases | M5/M6 |
| R8 product proof | M1 interfaces; relevant R2-R7 features at least L3 | late R6/R7 hardening | v1 scope and release-candidate acceptance |
| R9 release | R0-R8 exit criteria and zero open release blockers | no unbounded feature work | v1.0 |
| F1-F4 frontier | preserved v1 compatibility/governance baseline | post-v1 research lanes | no v1 milestone |

---

## 11. Migration from the 2026-07-02 roadmap

No substantive objective from the prior R0-R11 plan is intentionally discarded. It is reordered and given stricter acceptance semantics:

| Prior area | New home | Change |
|---|---|---|
| Prior R0 foundation repair | R0 | Reopened and expanded because root-lock hermeticity, evidence mutation, status accuracy, and gate cost remain unresolved. |
| Prior R1 interpreter overhaul | R2 | Retains completed inline-int/vector/parser/primitive work as baseline; focuses open work on closures, frames, maps, tails, heap, and startup. |
| Prior R2 bytecode/JIT | R3 | Bytecode remains required; JIT is conditional on measured need rather than assumed critical path. |
| Prior R3 warm/MCP/agent loop | R1-R2 | Moved before bytecode because it is the shortest path to useful AI adoption; warm exists and is hardened rather than rebuilt. |
| Prior R4 self-host closure | R4 | Strengthened with H-levels, semantic authority, DDC, independent conformance, and explicit stage0 truth. |
| Prior R5 AI completeness | R1 and R8 | Compact SDK/training readiness moves early; broad gauntlet and flagship proof remain late product gates. |
| Prior R6 language completeness | R5 | Existing effect rows/concurrency are audited/promoted instead of described as absent. |
| Prior R7 deployment | R6 | Adds offline/self-hosted infrastructure, profile compatibility, reproducibility, and operations. |
| Prior R8 ecosystem | R6 and R9 | Registry/package GA and release governance are separated. |
| Prior R9 verification | R7 | Adds mutation evidence, threat models, fault injection, DDC, and independent review. |
| Prior R10 human UX | R8 | Kept minimal and schema-generated, tied to real flagship workflows. |
| Prior R11 self-improvement | F1-F4 | Moved post-v1 so recursive automation cannot outrun evidence, governance, and release stability. |

Completed work from the prior plan is preserved in git history and the audited baseline. It must be reclassified in the capability ledger rather than copied as unchecked green claims. If a prior task has durable evidence and satisfies the stronger definition of done, it may be marked at the corresponding new maturity level without reimplementation.

---

## 12. Milestone acceptance summary

| Milestone | Required phases | Headline acceptance |
|---|---|---|
| M0 Truthful Green Door | R0 | Published clean clone and remote CI; clean, hermetic, read-only checks; replay parity; load-stable default suite; warning-free build; bounded gate/cache disk and time; independent evidence verifier |
| M1 Agent Preview | R1 | Versioned compact SDK; structured diagnostics; warm/MCP; protected corpus; 8/10 core held-out tasks |
| M2 Runtime Beta | R2-R3 core | Resource-bounded runtime; deterministic snapshot/incremental loop; verified bytecode; semantic tier parity |
| M3 Self-Host Authority Beta | R4 | H2 toolchain authority; H3 cross-host fixpoint; DDC; independently verifiable stage0 contract |
| M4 Platform Beta | R5-R6 | Stable language/profile; deterministic concurrency; safe FFI; offline registry; reproducible target matrix |
| M5 Trust Release Candidate | R7-R8 | Formal/fuzz/security gates; 9/10 gauntlet; five maintained flagship systems; external review |
| M6 Trust Release | R9 | Reproduced signed release, E4 evidence, offline verification/install, compatibility/security operations |

---

## 13. Risk register

| Risk | Why it matters | Mitigation and trigger |
|---|---|---|
| Status theater | Broad routing/tests can look complete while semantic ownership or cross-host proof is absent | L0-L5 and H0-H4 ledgers; generated matrices; independent verifier |
| Local-only accumulation | Large dirty worktrees are neither bisectable nor protected from accidental loss, and CI changes remain unexercised | Task-scoped green commits, remote draft checkpoints, clean-clone reconstruction, required GitHub CI |
| Flaky assurance | Load races and recursively nested suites turn trust checks into timing lotteries or encourage ignored failures | Hermetic lane ownership, explicit fixture readiness, repeat-under-load controls, no retry-to-green acceptance |
| Benchmark-shaped execution | Workload recognizers can manufacture impressive numbers without improving the language runtime | Remove production recognizers, anti-overfit source variants, property-based optimization preconditions, differential semantics |
| Roadmap scope overwhelms delivery | A single maintainer can spend years on breadth before agents can use the language | M1 is the first product; critical path blocks speculative breadth; each milestone is independently usable |
| Optimizations change identity/effects | Breaks the core trust model | Reference semantics, differential corpus, translation validation, versioned formats, mutation tests |
| JIT dependency and security cost | Can harm startup, platform reach, build size, and TCB without helping agent workloads | R3.4 decision gate; bytecode/wasmi completeness; optional feature |
| Memory leaks or host OOM | Warm agents and services become unsafe despite step/effect limits | R2 heap semantics, logical metering, isolation, 100k-request stress, hard cleanup |
| Self-hosting duplicates rather than removes trust | Two implementations increase attack and maintenance surface | Ownership ledger; production authority switch; strict no-fallback; Rust demotion/removal |
| Self-host migration targets moving semantics | Switching authority before a profile freeze causes duplicate churn and makes parity evidence ambiguous | Freeze the relevant R5.1/R5.3 contract before each R4.2 production switch; version any later semantic change |
| Bootstrap trusting-trust attack | A fixpoint alone can reproduce a malicious compiler | DDC, independent builders/verifiers, signed source/evidence, conformance implementations |
| Agent benchmark contamination | Inflates quality claims and trains to tests | Split manifests, hidden oracles, hashes, rotation, contamination labels, model-agnostic scoring |
| Context bundle drift | Agents learn APIs that no longer match runtime behavior | Generated cards/symbol index, parse/run goldens, profile negotiation, stale-card rejection |
| Gate explosion and disk growth | Slow checks discourage use and can consume tens of gigabytes | Gate manifest, impact selection, shared caches, GB budgets, deterministic cleanup |
| Capability convenience erodes security | Agents may solve errors by asking for broad authority | Structured repair rules, policy diffs, minimization score, negative controls, independent review |
| Cross-platform nondeterminism | Breaks hashes, replay, builds, and bootstrap | Numeric/text/path specs, tier-1 host matrix, normalized archives, exact mismatch reports |
| Registry/signing compromise | Invalidates ecosystem trust | Offline roots, threshold/rotation/revocation, transparency, mirrors, rehearsed recovery |
| Formal work becomes decorative | Proofs may cover a toy subset while claims imply full runtime | Coverage ledger tied to executable forms; published assumptions/gaps; proofs hard-gate only their stated claims |
| Public claims outrun product proof | Damages credibility and user trust | E4-linked claims, explicit unsupported domains, flagship maintenance exercises |

---

## 14. Maintenance rules

- Keep `Last audited` current whenever priorities, facts, budgets, or release criteria change.
- Never check a task without the full done annotation and durable evidence described in section 2.2.
- Never delete completed work to make the queue look smaller. Move historical detail into a versioned evidence/changelog view when this file becomes unwieldy.
- Generate status matrices from the capability/evidence ledger after R0; do not manually synchronize multiple truth sources.
- Ratchet budgets downward. A loosening requires a dated decision record and evidence.
- Promote repeated active failures to `upgrade_plan.md` as P0/P1; close them there only when their regression gate passes.
- Keep governed check entrypoints one-in/one-out until M1. Prefer extending an existing authority and record consolidation savings; never add a gate merely to assert that too many gates exist.
- Keep a published remote checkpoint for every coherent green tranche. An unpushed local worktree is not durable project state.
- Add a new capability family only with a spec owner, capability schema, resource model, threat model, host/platform scope, negative controls, generated bindings/cards, and at least one product workload.
- Change canonical hashes, logs, package/patch/evidence formats, bytecode, snapshots, or bootstrap envelopes only through a versioned compatibility proposal and migration corpus.
- Review this roadmap at every milestone and at least monthly while active. Reordering must explain dependency or evidence changes, not preference alone.
- The final test of the roadmap is not how advanced it sounds. It is whether an independent user and their agents can reproduce the claims, understand failures, remain inside explicit authority, and ship useful software without trusting hidden machinery.
