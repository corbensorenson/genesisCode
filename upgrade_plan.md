# GenesisCode v0.2 Production Upgrade Plan

Date: 2026-02-14

## Current State (What Exists Today)

- Rust workspace with crates: CoreForm, kernel (Gλ), prelude (protocol + contracts), effects (runner + log + replay), obligations (packages + evidence store), patches, types (gradual), optimizer, CLI.
- CLI commands implemented: `fmt`, `eval`, `explain`, `run`, `replay`, `test`, `pack`, `typecheck`, `optimize`, `apply-patch`.
- Deterministic effect logs + replay are implemented for built-in caps:
  - `sys/time::now`
  - `io/fs::read`
  - `io/fs::write`
- Obligations implemented (baseline + optional hooks): unit tests, determinism, capabilities-declared, replayable-tests, typecheck, translation-validation.
- CI workflow exists (fmt/clippy/tests), and the workspace is currently green under `cargo test --workspace` and `cargo clippy -- -D warnings`.

## Definition: “Production Ready” For v0.2

For this plan, “production ready” means:
- The v0.2 conformance surface is stable, specified, and heavily tested (goldens + negative tests).
- The CLI and file formats are stable enough for tooling: deterministic outputs, clear exit codes, machine-readable reports.
- The effect runner is secure-by-default and robust under adversarial inputs (policy + sandbox + replay).
- Evidence artifacts are durable, race-safe, and verifiable.
- The system is operable: docs, examples, and failure modes are clear.

This plan does not require implementing refinement proofs, a registry server, a JIT, or a fully self-hosted compiler (those are listed as post-production “advanced stacks”).

## Remaining Work (Prioritized)

### P0: Spec Closure + Correctness Harness (Must-Have)

- [x] Write a normative spec for CoreForm canonical printing and hashing (version tags, map key ordering, application printing, width/indent rules) and link it from `docs/spec/`. (`docs/spec/COREFORM_CANON_HASH.md`)
- [x] Write a normative spec for value hashing used in effect logs (`value_hash`) and for effect request hashing (`req_h`), including what data is included/excluded. (`docs/spec/VALUE_EFFECT_HASH.md`)
- [ ] Expand golden/spec tests to cover every conformance checklist item:
  - [x] `seal/unseal` edge cases and spoof resistance (baseline + non-token mismatch errors + protocol spoof tests).
  - [x] contract dispatch/extend precedence and `explain` trace stability.
  - [x] effect log schema roundtrip + replay mismatch matrix (wrong op/payload/cont/resp).
  - [x] patch schema validation matrix (bad schema, bad paths, obligation rerun failures).
- [x] Add “failure fixture” packages under `tests/spec/` that intentionally fail each obligation, and assert stable error reporting.

### P0: CLI Stability + Machine Interfaces (Must-Have)

- [x] Define and document stable CLI exit codes for each command class (parse error, eval error, obligation failure, replay mismatch, denied capability, internal error). (`docs/spec/CLI.md`)
- [x] Add `--json` output mode for `test`, `pack`, `apply-patch`, `typecheck`, `optimize` (and consistently across other commands), so CI/tooling can consume results robustly. (`docs/spec/CLI.md`)
- [x] Add `genesis verify --pkg package.toml` to validate:
  - pinned module hashes match disk
  - dependency hashes match
  - evidence store artifacts referenced by reports exist and hash-check
- [x] Improve error payload conventions (align with style guide: `:error/code`, `:error/message`, `:error/context`) and ensure they are emitted consistently across kernel/prelude/effects.

### P0: Runner + Store Hardening (Must-Have)

- [x] Evidence store hardening:
  - [x] make `put_bytes` race-safe (handle concurrent writers without spurious failures)
  - [x] optionally verify existing artifact contents match the hash (detect corruption)
  - [x] optionally fsync temp + directory for stronger durability (document semantics). (`docs/spec/EVIDENCE_STORE.md`)
- [x] Effect runner hardening:
  - [x] document the filesystem sandbox model and remaining TOCTOU limitations. (`docs/spec/FS_SANDBOX.md`)
  - [x] add policy knobs for maximum response size logged inline, with artifact-store fallback for large responses (`[log].inline_max_bytes` + store). (`docs/spec/CAPS_TOML.md`)
  - [x] add per-op timeouts (runner-side) for capabilities that can block (`timeout_ms`, supported for non-mutating ops like `io/fs::read`). (`docs/spec/CAPS_TOML.md`)

### P0: Prelude / “Core Stdlib” (Must-Have For Usability)

Style guide expects a stable set of names and helpers. Today, many helpers exist as `prim` ops rather than `core/*` functions.

- [x] Implement a real `prelude/prelude.gc` with CoreForm-level wrappers and helpers:
  - message helpers (already exist as native fns; expose stable wrappers if desired)
  - `core/error::*` helpers (aliases + standardized payload constructor)
  - convenience wrappers for common primitives (map, vec, list, int, etc.)
- [x] Add a minimal test DSL in CoreForm (stable `core/test::case` and `core/test::case0`) so style-guide examples are representable without verbose boilerplate.

### P1: Reliability (Fuzzing, Limits, Portability)

- [x] Add fuzz/property-test harnesses (as suggested in `docs/TECH_HANDOFF.md`), using `proptest`:
  - parser: parse/print/parse invariants
  - canonicalization: idempotence
  - log parser: malformed `.gclog` inputs must not panic
  - patch parser/validator: malformed `.gcpatch` must not panic
- [ ] Address recursion depth risk in evaluator and printer:
  - either implement a trampoline/CEK-style evaluator or document hard limits and provide mitigation.
- [ ] Add configurable step limits and (optional) memory limits via CLI flags and package policy.
- [ ] Cross-platform determinism review (path normalization in logs, line endings, OS-specific capability behavior).

### P1: Obligation Stack Expansion (From Paper/Style Guide)

- [ ] Add `core/obligation::property-tests` with recorded seeds as evidence artifacts.
- [ ] Add `core/obligation::coverage` (tooling obligation; define what coverage means for GenesisCode code and how it is measured).
- [ ] Add `core/obligation::budgets` (time/memory budget evidence for selected tests/benchmarks).

### P2: Supply Chain + Registry Policy (Paper Direction)

- [ ] Add signature support for acceptance artifacts (package-level signing).
- [ ] Define a minimal “registry policy” spec (local policy format first) and implement a verifier that enforces it.
- [ ] Optional: transparency log integration (append-only log of published package hashes + signatures).

### P2: Advanced Stacks (Not Required For Initial Production, But v0.2-Vision Complete)

- [ ] Type stack completion:
  - row-polymorphic contract typing with effect rows (current checker is partial/lightweight).
- [ ] Optimizer completion:
  - real e-graph (`egg`) optimizer for the pure subset, with rewrite statistics and stronger translation-validation evidence.
- [ ] WASM target:
  - compile kernel + pure evaluator to WASM; keep runner as host bridge; keep logs identical.

## Rough Effort Estimate (Single Engineer, Full-Time)

Assumptions:
- Scope is “production ready v0.2 toolchain for local + CI use” (no registry server, no refinement proofs, no JIT).
- Includes P0 and most of P1, excludes P2 “advanced stacks”.

Estimate:
- P0 (spec closure + CLI stability + store/runner hardening + prelude usability): 4-6 weeks
- P1 (fuzzing, evaluator limits, portability review, property tests obligation): 4-8 weeks

Total: 8-14 weeks

If you include P2 supply-chain signing + policy enforcement: +3-6 weeks.
If you include “advanced stacks” (row-polymorphic typing + real e-graph optimizer + WASM): +8-16+ weeks.
