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

