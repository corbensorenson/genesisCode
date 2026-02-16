# GenesisCode Authoring Skill (v0.2)

Use this skill when implementing or modifying GenesisCode language/runtime/tooling behavior, or when writing `.gc` modules meant to survive long-term self-hosted evolution.

## Mission

Deliver deterministic, obligation-gated changes that move the system toward fully self-hosted GenesisCode workflows, minimizing Rust bootstrap dependence without sacrificing correctness.

## Ground rules (non-negotiable)

- Kernel purity: never introduce ambient filesystem/time/network/LLM into evaluator semantics.
- Protocol integrity: preserve unforgeable `UNHANDLED`, `EFFECT`, and `ERROR` behavior.
- No panic on user input: return explicit errors (`Result`) in Rust internals and sealed protocol errors at language boundaries.
- Determinism first: every effectful workflow must remain replayable and auditable.
- No mock/stub behavior in production paths.

## Canonical workflow (agent prompt protocol)

Always execute this loop:

1. `Plan`
- Identify which `upgrade_plan.md` item(s) are the highest-impact unchecked targets.
- State acceptance criteria before edits.

2. `Patch`
- Implement smallest coherent vertical slice.
- Prefer `.gc` prelude/runtime/editor layers when functionality belongs in language space.

3. `Evidence`
- Run focused tests first, then relevant broader checks.
- Required baseline after meaningful changes:
  - `cargo run -q -p gc_cli -- fmt <changed .gc module>`
  - `bash scripts/assemble_prelude.sh` (if prelude modules changed)
  - target crate tests
  - `cargo clippy -p <changed crates> --all-targets -- -D warnings`

4. `Accept`
- Update `upgrade_plan.md` checkboxes only when acceptance criteria are objectively met.
- Record exactly what was completed and which tests prove it.

## Language authoring conventions

- Prefer existing Level 1/2 APIs before adding new primitives.
- Preserve canonical CoreForm style:
  - explicit curried forms where applicable
  - stable map/vector data shapes
  - deterministic ordering and hashing behavior
- When adding editor/VCS/package/graphics actions:
  - return stable panel/report maps with explicit `:kind`
  - keep payload schema strict and total
  - avoid ambient assumptions; explicit inputs only

## Effects, capabilities, and policies

- Deny-by-default capability posture for new effect ops.
- When introducing a new host capability wrapper:
  - add pure constructor wrapper in prelude
  - add request-shape coverage in prelude tests
  - document payload/response in spec docs
- Keep replay correctness: include enough logged data to deterministically reproduce decisions.

## GenesisGraph / GenesisPkg expectations

- Patch-first changes:
  - propose structural patch artifacts
  - apply and re-run obligations
- Obligation-first acceptance:
  - unit tests
  - replayability
  - capability declaration compliance
  - additional stack obligations when enabled (typecheck, translation validation, gfx gates)

## Self-hosting strategy

- Prefer implementing new behavior in `.gc` modules (especially prelude/editor/tooling layers).
- Treat Rust as host/bootstrap boundary only.
- Before moving/removing bootstrap artifacts:
  - confirm equivalent self-hosted path exists
  - confirm CI and local smoke parity
  - archive legacy artifacts instead of deleting irreversibly

## Performance and safety heuristics

- Keep recursive `.gc` helpers total and shape-checked.
- Avoid quadratic concatenation patterns; use vector/bytes join helpers.
- For large structural passes:
  - split into helper defs with deterministic data contracts
  - add tests for error-path and success-path outputs

## Required output quality in reviews/PR notes

- Findings first when reviewing.
- Explicit file paths and semantics changed.
- Tests executed and current status.
- Clear statement of what remains blocked or unchecked in `upgrade_plan.md`.
