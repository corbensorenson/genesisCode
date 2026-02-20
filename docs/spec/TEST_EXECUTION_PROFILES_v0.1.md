# Test Execution Profiles v0.1

Deterministic test execution policy for local iteration and CI.

## Goals

- Keep local feedback loops fast (`<10m` target for default developer loop).
- Preserve full suite coverage in CI profiles.
- Keep shard assignment deterministic across runs.

## Runners

- Preferred runner: `cargo nextest` (configured by `/.config/nextest.toml`).
- Fallback runner: `cargo test` when nextest is unavailable.

## Local

- Default fast loop: `scripts/test_changed_fast.sh`
  - changed-file aware selection (or clean-tree fallback)
  - warms selfhost artifact cache when relevant paths change
  - emits deterministic metrics report `kind = genesis/test-changed-fast-metrics-v0.1`
- Full fast fallback: `scripts/test_fast.sh`
  - auto-detects nextest
  - runs high-signal core libs + selected CLI integration tests
- Full/sharded loop: `scripts/test_shard_workspace.sh --total N --index I --runner auto|nextest|cargo`
  - deterministic shard assignment by `(seed, crate)` hash
  - emits report `kind = genesis/test-shard-report-v0.1`

## CI Profiles

- `fast`: runs `scripts/test_changed_fast.sh` (default local/CI fast path)
- `standard|full`:
  - installs nextest
  - uses deterministic shard execution when `GENESIS_TEST_SHARDS_TOTAL > 1`
  - otherwise runs full workspace tests with nextest (`--cargo-profile selfhost-strict`)
  - preserves existing strict/smoke/golden gates as separate steps
  - runs `scripts/check_ai_stress_suite.sh` to enforce deterministic high-throughput stress
    coverage for tasks + bridge + gpu/compute + replay integrity.
- Iteration conformance check:
  - `scripts/check_default_iteration_workflow.sh` validates measurable fast-path execution and
    deterministic shard selection.

## Determinism

- Shard selection is deterministic from:
  - shard total/index
  - seed (`GENESIS_TEST_SHARD_SEED` or `GITHUB_SHA` in CI)
  - stable sorted crate list
- Runner selection is explicit in reports (`runner = cargo|nextest`).
