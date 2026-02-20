# GenesisCode Red-Team Report

Last updated: 2026-02-20

Scope:
- Track unresolved `P0` and `P1` risks from `/Users/corbensorenson/Documents/genesisCode/upgrade_plan.md`.
- Keep entries actionable for self-hosted, AI-first v1 cutover.

## Active Risks (P0/P1)

- `P0.1` Interactive gfx terminal adapter writes control bytes to stdout and breaks deterministic agent workflow outputs.
  - Next action: isolate terminal side effects from command output channels and re-stabilize workflow hashes.
- `P0.2` First-party GPU backend remains deterministic in-memory simulation; production device-backed runtime is not integrated in `gc_effects`.
  - Next action: integrate in-repo device-backed backend path with replay-safe contract.
- `P0.3` First-party interactive gfx backend is terminal-host only (no production desktop window/audio backend).
  - Next action: add non-terminal window/input/audio adapter path while preserving deterministic headless mode.
- `P0.4` Host ABI lacks network/process capability domains needed for general service/tool workloads.
  - Next action: define and implement capability-gated `io/net::*` and `sys/process::*` surfaces.
- `P1.1` Editor watch polling rescans full trees each poll (non-incremental).
  - Next action: switch to incremental watcher/indexed-delta strategy with deterministic replay semantics.
- `P1.2` Local `prepush-standard` health profile omits agent reference workflow gate present in CI.
  - Next action: align local and CI gating for agent workflow regressions.
- `P1.3` Selfhost cutover dashboard freshness contract is currently broken (`SELFHOST_CUTOVER.md` stale).
  - Next action: regenerate and keep dashboard synchronized in standard update flow.
- `P1.4` `gc_effects` currently fails strict clippy `-D warnings`.
  - Next action: clear current warnings and keep lint-clean profile enforced.
- `P1.5` WASI profile currently rejects HTTP(S) remotes, limiting registry workflows.
  - Next action: add constrained policy-gated WASI networking profile for registry use.
- `P1.6` Deterministic resource model remains intentionally incomplete for adversarial workloads.
  - Next action: add stronger deterministic resource controls beyond current conservative limits.
