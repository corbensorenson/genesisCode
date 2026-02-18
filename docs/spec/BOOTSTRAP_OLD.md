# Bootstrap Archive And Active Tooling (v0.2)

This document records which bootstrap-era artifacts have been archived and which
ones remain required for reproducible host/runtime verification.

## Archived (`old_bootstrap/`)

- `old_bootstrap/scripts/build_wasi.sh`
  - reason: legacy convenience wrapper only
  - replacement:
    - direct build command:
      - `rustup target add wasm32-wasip1`
      - `cargo build -p gc_wasi_cli --target wasm32-wasip1 --release`
    - `scripts/wasi_smoke.sh` now self-builds WASI when no wasm path is supplied

- `old_bootstrap/scripts/host_abi_conformance.sh`
  - reason: heavy manual parity harness, superseded by focused CI guards
  - replacement:
    - `scripts/check_host_abi_conformance.sh`
    - `scripts/check_prelude_capability_coverage.sh`
    - `scripts/selfhost_default_profile_guard.sh`

- `old_bootstrap/rust_semantics/`
  - reason: isolate bootstrap-only Rust semantic program builders from production CLI wiring
  - replacement:
    - selfhost contract programs in `selfhost/cli_coreform_v1.gc`
    - low-level host capability execution in `crates/gc_effects/src/runner.rs`

## Still required (active)

- `scripts/assemble_prelude.sh`
  - deterministic prelude module assembly (`prelude/modules/*.gc -> prelude/prelude.gc`)
- `scripts/wasi_smoke.sh`
  - native-vs-WASI deterministic equivalence checks for CLI behavior
- `scripts/wasm_bindgen_node.sh`
  - build wasm-bindgen node target for host-bridge and selfhost API checks
- `scripts/wasm_bindgen_web.sh`
  - build wasm-bindgen web target for browser parity checks
- `scripts/wasm_node_smoke.mjs`
  - node wasm smoke for core/selfhost APIs
- `scripts/wasm_cross_host_determinism.mjs`
  - native-vs-node cross-host determinism checks
- `scripts/wasm_web_smoke.mjs`
  - headless browser wasm smoke/parity checks

## Policy for future archival

Move artifacts to `old_bootstrap/` only when all are true:

1. The self-hosted or direct command path is available and tested.
2. CI no longer references the archived path.
3. Specs/docs point to the replacement path.
4. Determinism and obligation coverage remains intact after removal.
