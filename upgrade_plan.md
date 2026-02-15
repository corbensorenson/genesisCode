# GenesisCode: Next-Phase Upgrade Plan (WASM Bootstrap + Self-Host Roadmap)

Date: 2026-02-15

This file tracks the next phase now that the GenesisCode v0.2 production plan is complete.

Primary goals:
- Make GenesisCode **fully usable** in WASM-hosted environments (Node + browser) while keeping the kernel pure.
- Make the Rust implementation a **bootstrap** layer that can be replaced by a self-hosted toolchain later.
- Keep specs, docs, and the CLI surface aligned and heavily tested.

Non-negotiables:
- No mock/simulated outputs in product behavior.
- Fix root causes of warnings/errors; keep `cargo fmt`, `cargo test --workspace`, `cargo clippy --workspace --all-targets -- -D warnings` green.
- Determinism and replayability remain first-class invariants across hosts (native and WASM).

---

## P0: Docs/Spec <-> Implementation Alignment (Must-Have)

### CLI spec conformance gaps (discovered from `docs/CLI_SPEC_GENESISPKG_GENESISGRAPH_v0.1.md`)
- [x] Implement `genesis vcs hash --in <file>` (pure; no store mutation). Add `--json` output.
- [ ] Implement `genesis vcs blame ...` and `genesis vcs why ...`, or update the CLI spec to match a corrected, deterministic interface.
  - Acceptance: deterministic results; no dependence on filesystem iteration order; includes evidence pointers where available.
  - Tests: golden CLI tests validating output structure and stable exit codes.
- [ ] Implement `genesis commit new` and `genesis commit show` (as specified), or update the CLI spec and provide equivalent commands.
  - Acceptance: commit creation is policy-gated only at ref-advance time, not at commit creation; commit is content-addressed and stored.
- [ ] Implement `genesis policy list/show/set-default` as described in the CLI spec (or update the spec and provide an equivalent workflow).
- [ ] Add missing CLI flags required by the CLI spec (or update the spec):
   - `genesis pkg import --set-ref <ref>=<hash>` (set local refs after importing a bundle).
   - `genesis vcs merge3 --out <file>` (write merged snapshot/conflict to a file deterministically).
   - `genesis vcs diff/apply` parity with spec (`--store/--no-store` and output file options).

### CLI flag/name compatibility polish (spec vs current CLI)
- [x] Add `genesis store put --in` as an alias for `--input` (spec-name compatibility).
- [ ] Audit remaining CLI spec-name flags and add backwards-compatible aliases where the CLI diverges.
- [ ] Add optional `--quiet` and `--verbose` global flags (retain stable JSON envelope in `--json` mode).
- [ ] Decide and enforce a single canonical hash string format for CLI output (hex or base32).
   - Acceptance: CLI accepts both; prints only the canonical format; docs and tests match.

### Spec audit harness
- [ ] Add a “spec surface” test that asserts every CLI-spec command has a corresponding CLI parser entry (or is explicitly marked “deviates” with a doc link).
  - Acceptance: changes to CLI surface must update either code or the spec mapping.

---

## P0: WASM Bootstrap (Make It Usable in Node/Browser)

### WASM runtime model (effects without breaking kernel purity)
- [x] Define a normative WASM host interface doc:
  - `docs/spec/WASM_HOST_BRIDGE.md`
  - Includes: step/resume API, effect request hashing inputs, continuation handles, response hashing, and log equivalence requirements.
- [ ] Extend the WASM build beyond pure `eval` so effectful programs can run with a host runner:
  - Preferred architecture: WASM exports a stateful “runtime” object:
    - runs until `done` or `effect-request`
    - returns `{op, payload, req_h, cont_h, payload_h, state_handle}`
    - resumes from `{state_handle, response}` deterministically
  - Acceptance: host can implement capability policy + deterministic `.gclog` entirely outside the kernel while still using kernel-owned continuation hashing.

### WASM artifact verification & cross-host determinism
- [ ] Add cross-host determinism tests:
  - Same `.gc` module must produce identical:
    - canonical formatting bytes
    - module hash
    - pure eval result term bytes
    - effect request/response hashes (for effectful stepping)
  - Hosts: native Rust CLI and Node WASM (CI via `node`).

### Distribution: “Rust bootstrap on top of WASM”
- [ ] Provide a Node distribution path:
  - Build a minimal `genesis-wasm` package in `./packages/` (workspace-local) that wraps WASM exports.
  - Include a `genesis` Node CLI that supports at least: `fmt`, `hash`, `eval`, and effect stepping/replay using host capabilities.
- [ ] Add CI jobs to build the WASM package and run a smoke test in Node.

---

## P1: WASI/Wasmtime CLI (WASM-First Tooling)

Goal: a `genesis.wasm` (WASI) build that can run in `wasmtime`, so the toolchain runs “on top of wasm” on all platforms.

- [ ] Create a `wasm32-wasi` target build that provides a CLI entrypoint (initially `fmt/hash/eval`, then expand).
- [ ] Add `wasmtime`-backed CI smoke tests verifying deterministic behavior and compatible exit codes.
- [ ] Define capability bridging rules for WASI:
  - filesystem sandbox behavior
  - network policy (deny by default)
  - deterministic time behavior via effect logs

---

## P2: Self-Host Roadmap (Remove Rust Eventually)

This is intentionally staged; correctness and auditability stay central.

- [ ] Define the “self-host boundary” spec:
  - what subset of GenesisCode is required to implement the compiler/tooling
  - how obligations validate the self-hosted artifacts
  - how translation validation is used to de-risk the bootstrap replacement
- [ ] Implement a self-hostable frontend in GenesisCode:
  - CoreForm libraries for parsing/printing/canonicalization (or a definitional encoding that produces identical output).
  - A minimal module loader/package resolver that uses GenesisGraph objects.
- [ ] Implement a compiler pipeline (initially CoreForm -> CoreForm transforms; later CoreForm -> WASM):
  - Obligations must validate equivalence against the Rust implementation (translation validation + replayable tests).
- [ ] Cut-over plan:
  - stage 1: Rust builds the self-hosted toolchain artifact, but runtime uses it
  - stage 2: self-hosted toolchain builds itself (bootstrapping) under obligations
  - stage 3: Rust becomes optional tooling, not required
