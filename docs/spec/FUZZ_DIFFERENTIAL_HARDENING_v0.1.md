# Fuzz + Differential Hardening v0.1

Status: normative gate for parser/toolchain robustness against malformed and adversarial inputs.

## Scope

This gate enforces two classes of hardening checks:

- Fuzz-style invariant suites over parser/canonicalizer, patch schemas, effect logs, and optimizer rewrites.
- Differential corpus checks that compare Rust vs selfhost frontend behavior on malformed/adversarial inputs.

## Gate Script

- `scripts/check_fuzz_differential_hardening.sh`

## Required Suites

The gate runs these suites:

- `cargo test -p gc_coreform --test fuzz_parse_print --quiet`
- `cargo test -p gc_patches --test fuzz_patch --quiet`
- `cargo test -p gc_effects --test fuzz_log --quiet`
- `cargo test -p gc_opt --test fuzz_optimizer --quiet`
- `cargo test -p gc_cli --test cli_differential_adversarial --quiet`

## Differential Corpus

Corpus fixtures live under:

- `tests/spec/adversarial_coreform/`

Current required corpus cases:

- malformed module with unterminated form
- malformed module with odd map arity
- adversarial deeply-unbalanced parse input
- malformed patch schema input

For each case, parity harness enforces:

- Rust and selfhost frontends return the same process exit code
- failure envelope shape/code parity:
  - `kind`
  - `diagnostics_schema`
  - `error.code`
  - first diagnostic `code` and `exit_code`

## CI + Health Integration

The hardening gate is required by:

- `.github/workflows/ci.yml`
- `scripts/check_upgrade_plan_health.sh`

This keeps malformed/adversarial differential behavior from regressing silently.
