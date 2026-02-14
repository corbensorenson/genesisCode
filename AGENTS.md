# GenesisCode v0.2 — Codex Working Agreement

## Mission
Implement GenesisCode v0.2 per `docs/PAPER_v0.2.md` and `docs/TECH_HANDOFF.md`.

## Non-negotiable invariants
- Kernel (Gλ) must be pure and deterministic. No filesystem/time/network/LLM inside the evaluator.
- UNHANDLED/EFFECT/ERROR must be unforgeable using seal tokens created by Prelude.
- Never panic on user input. Convert errors to sealed ERROR values at boundaries, or explicit Rust `Result` internally.
- Effect runner must be deny-by-default and must produce deterministic effect logs + replay checker.

## Repo conventions
- Language: Rust workspace in `crates/`.
- Keep TCB-A minimal: evaluator + immutable primitives + seals only.
- Prefer adding tests/goldens for every semantic rule.

## Development flow
- Make small commits (if/when you use git) with passing tests.
- Prefer implementing behavior in libraries over adding kernel special forms.
