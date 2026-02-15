# Limits and Resource Safety (v0.2)

GenesisCode v0.2 is designed to run on untrusted inputs, but the initial toolchain is intentionally conservative: it provides practical safety valves (step limits, deny-by-default effects, deterministic logs) while leaving room for stronger sandboxing and proof-carrying constraints in later versions.

## Kernel Step Limit

- The kernel maintains a step counter (`EvalCtx.steps`) and enforces a configurable step limit.
- Default (v0.2 toolchain): `5_000_000` evaluation ticks.
- CLI controls:
  - `--step-limit N`: set an explicit limit
  - `--no-step-limit`: disable the step limit (trusted inputs only)

The step limit is intended to prevent accidental infinite loops and to bound worst-case evaluation time under adversarial inputs. It is not a semantic feature of the language.

## Recursion Depth and Stack Safety

CoreForm parsing, canonical printing, term ordering (for map keys), and kernel evaluation are structurally recursive.

To mitigate stack overflows on deep inputs, the v0.2 toolchain uses stack growth via the `stacker` crate at the recursive entrypoints. This keeps behavior deterministic while preventing process aborts from typical deep-but-finite structures.

Notes:
- This is a mitigation, not a full resource model.
- Extremely deep terms can still consume large amounts of memory and time (both for evaluation and for canonical printing/hashing).
- For CI and untrusted inputs, prefer enabling a conservative `--step-limit` and running under OS-level memory limits.

## Deterministic Memory Limits (Kernel)

In addition to step limits, the kernel supports optional, deterministic memory safety valves.

These limits are *not* an exact accounting of process RSS. They are stable, semantic measures based on observed sizes of CoreForm values during evaluation:

- `max_pair_cells`: total number of `pair/cons` cells allocated.
- `max_vec_len`: maximum observed vector length.
- `max_map_len`: maximum observed map length.
- `max_bytes_len`: maximum observed bytes length.
- `max_string_len`: maximum observed string length in UTF-8 bytes.

When a limit is exceeded, evaluation fails with a kernel error of kind `memory limit exceeded` and a message that includes the observed value and the configured limit.

Limits can be set:
- via CLI global flags (see `docs/spec/CLI.md`), or
- via `package.toml` `[limits]` keys (see `docs/spec/PACKAGE_TOML.md`), where package policy is always enforced as an upper bound.
