# GenesisCode Canonical Language Examples v0.1

This directory is the executable teaching authority for the frozen
`GC-AGENT-v0.3` profile. It is designed for agents first: every lesson has a
successful fixture, a minimal failing counterexample, a declared one-site
mutation, machine-readable expectations, and a deterministic repair.

The suite covers pure functions, persistent collections, contracts, modules,
sealed errors, effects, replay, packages, semantic patches, tests, and resource
failures. `suite.json` binds every source byte by SHA-256 and binds the complete
suite by a canonical content identity.

## Use

Inspect a pair under `pairs/<concept>/{valid,invalid}`. Run the argv recorded in
`suite.json` from that side's directory. The production test harness injects
the repository-pinned self-host artifact but does not change the declared
command, policy, source, or expected result.

The invalid side is not approximate. The validator proves it equals the valid
side after exactly one `replace-once` mutation in one named file; every other
file must be byte-identical. This makes each repair local, auditable, and safe
for training or deterministic evaluation.

## Authority

- Schema: `docs/spec/GC_CANONICAL_EXAMPLES_v0.1.schema.json`
- Manifest: `examples/canonical_language/v0.1/suite.json`
- Static and adversarial validator: `scripts/lib/gc_canonical_examples.py`
- Shipped-CLI conformance: `crates/gc_cli/tests/cli_canonical_language_examples.rs`
- Governed gate: `scripts/check_agent_authoring_bundle.sh`

Do not broaden capabilities, disable limits, select the Rust compatibility
frontend, or rewrite expected failures to make an invalid case pass. Repair the
single declared mutation and rerun the exact command instead.
