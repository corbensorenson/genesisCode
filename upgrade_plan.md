# GenesisCode Upgrade Plan (Post-v0.2): WASM Bootstrap -> WASM-First -> Self-Host

Date: 2026-02-15

This plan is derived from the current `docs/` set (v0.2 + GenesisGraph/GenesisPkg addendum docs).
It tracks work required to make GenesisCode usable in WASM hosts (Node/browser), then move the
toolchain onto WASM (WASI/wasmtime), and finally self-host (removing Rust from the steady-state).

Non-negotiables:
- Keep the kernel pure and deterministic (effects only via runner + `.gclog` + replay).
- Keep `cargo fmt`, `cargo test --workspace`, and `cargo clippy --workspace --all-targets -- -D warnings` green.
- No mock/simulated product behavior.

---

## P0: WASM Bootstrap (Node/Browser Usability)

- [x] Define normative WASM host stepping interface: `docs/spec/WASM_HOST_BRIDGE.md`.
- [x] WASM kernel exports for canonical fmt/hash/eval and effectful step/resume runtime (`crates/gc_wasm`).
- [x] Node wasm-bindgen build + smoke (repo-local):
  - `scripts/wasm_bindgen_node.sh`
  - `scripts/wasm_node_smoke.mjs`
  - CI runs Node smoke in `.github/workflows/ci.yml`
- [x] Cross-host determinism: native runner vs Node(WASM) must match for:
  - canonical formatting bytes (module + term)
  - module hash
  - effect request/response hashes for a deterministic deny-by-default run
  - Acceptance: CI fails on any mismatch.
- [ ] Browser build support:
  - produce `wasm-bindgen --target web` artifacts
  - add a minimal browser harness that can run `Runtime.step/respond_*` with a host policy
  - Acceptance: deterministic golden tests run in headless browser in CI.

---

## P0: CLI/Spec Hygiene (Docs Must Remain Trustworthy)

Focus docs:
- `docs/spec/CLI.md` (exit codes + JSON envelope)
- `docs/CLI_SPEC_GENESISPKG_GENESISGRAPH_v0.1.md` (GenesisGraph/GenesisPkg commands)

- [x] `genesis vcs hash --in <file>` implemented (pure).
- [x] `genesis store put --in` flag compatibility alias.
- [ ] Align `docs/CLI_SPEC_GENESISPKG_GENESISGRAPH_v0.1.md` with the current CLI surface:
  - either implement missing spec commands as aliases, or update the spec to point at the actual commands
  - Required outcomes: a single authoritative command surface and stable flags.
- [x] Implement `genesis pkg import --set-ref <ref>=<hash>` (spec wants local ref updates post-import).
- [ ] Add `genesis vcs merge3 --out <file>` (deterministic file output for snapshot or conflict).
- [ ] Add a “spec surface” test that asserts the CLI parser exposes the documented commands/flags (or lists explicit deviations with doc links).

---

## P1: WASM-First Toolchain (WASI/wasmtime)

Goal: “Rust bootstrap on top of WASM” in practice: run the toolchain on WASI everywhere.

- [ ] Add a WASI CLI target that runs `fmt/hash/eval` on top of the WASM kernel.
  - Preferred: compile a small WASI wrapper that hosts the kernel and prints results exactly like native `genesis`.
- [ ] Add `wasmtime` CI smoke tests to prove “tooling runs on wasm”.
- [ ] Specify/implement WASI capability bridging policy:
  - filesystem sandboxing (`docs/spec/FS_SANDBOX.md`)
  - deterministic time via effect logs (no ambient time in kernel)
  - network deny-by-default

---

## P2: Self-Host Roadmap (Remove Rust in Steady-State)

- [ ] Write a “self-host boundary” spec:
  - what subset of GenesisCode is required to implement parsing/printing/canonicalization and a compiler pipeline
  - how obligations and translation validation (`docs/spec/TRANSLATION_VALIDATION.md`) de-risk bootstrap replacement
- [ ] Implement a self-hostable frontend in GenesisCode:
  - CoreForm printer/canonicalizer equivalence tests against Rust implementation
  - module loader + package resolver on GenesisGraph objects
- [ ] Implement a compiler pipeline suitable for WASM-first execution:
  - stage 1: CoreForm -> CoreForm transforms (optimized, validated)
  - stage 2: CoreForm -> WASM (behind translation validation obligation)
- [ ] Cutover plan:
  - Rust builds the self-host toolchain artifact, then runtime uses it
  - self-hosted toolchain builds itself under obligations
  - Rust becomes optional tooling only

---

## Notes

If this file reaches “all checked”, we should delete it and replace with a release checklist plus a long-term roadmap doc.
