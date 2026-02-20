#!/usr/bin/env bash
set -euo pipefail

ROOT_DIR="$(cd "$(dirname "$0")/.." && pwd)"
cd "$ROOT_DIR"

source "$ROOT_DIR/scripts/lib/measure.sh"

# Default budgets are intentionally conservative for shared CI runners.
# Override with env vars to tighten over time.
BUDGET_TEST_WALL_MS="${GENESIS_BUDGET_TEST_WALL_MS:-120000}"
BUDGET_SELFHOST_BOOTSTRAP_MS="${GENESIS_BUDGET_SELFHOST_BOOTSTRAP_MS:-15000}"
BUDGET_OBLIGATION_RUNTIME_MS="${GENESIS_BUDGET_OBLIGATION_RUNTIME_MS:-30000}"
MEASURE_WARMUPS="${GENESIS_BUDGET_WARMUPS:-1}"
MEASURE_REPEATS="${GENESIS_BUDGET_REPEATS:-3}"

fail() {
  echo "perf-budgets: $*" >&2
  exit 1
}

echo "perf-budgets: preparing genesis binary"
cargo build -p gc_cli >/dev/null
cargo test -p gc_cli --test cli_smoke --no-run --quiet >/dev/null

CLI_SMOKE_BIN="$(
  find "$ROOT_DIR/target/debug/deps" -maxdepth 1 -type f -name 'cli_smoke-*' -perm -u+x \
    | sort \
    | tail -n 1
)"
[[ -x "${CLI_SMOKE_BIN:-}" ]] || fail "unable to locate compiled cli_smoke test binary"

TMP_DIR="$(mktemp -d)"
cleanup() {
  rm -rf "$TMP_DIR"
}
trap cleanup EXIT

echo "perf-budgets: measuring wall-time budget via prebuilt cli_smoke runtime"
genesis_measure_best_of_ms test_wall_ms "$MEASURE_WARMUPS" "$MEASURE_REPEATS" "$CLI_SMOKE_BIN" --quiet
TEST_WALL_MS="$MEASURE_LAST_MS"

echo "perf-budgets: measuring selfhost bootstrap artifact build time"
genesis_measure_best_of_ms \
  selfhost_bootstrap_ms \
  "$MEASURE_WARMUPS" \
  "$MEASURE_REPEATS" \
  target/debug/genesis selfhost-artifact --out "$TMP_DIR/toolchain.gc"
SELFHOST_BOOTSTRAP_MS="$MEASURE_LAST_MS"

echo "perf-budgets: measuring obligation runtime on hello_pkg"
genesis_measure_best_of_ms \
  obligation_runtime_ms \
  "$MEASURE_WARMUPS" \
  "$MEASURE_REPEATS" \
  target/debug/genesis \
  --selfhost-only \
  --coreform-frontend selfhost \
  --selfhost-artifact "$TMP_DIR/toolchain.gc" \
  test --pkg examples/hello_pkg/package.toml
OBLIGATION_RUNTIME_MS="$MEASURE_LAST_MS"

echo "perf-budgets: metrics"
echo "  test_wall_ms=$TEST_WALL_MS (budget=$BUDGET_TEST_WALL_MS)"
echo "  selfhost_bootstrap_ms=$SELFHOST_BOOTSTRAP_MS (budget=$BUDGET_SELFHOST_BOOTSTRAP_MS)"
echo "  obligation_runtime_ms=$OBLIGATION_RUNTIME_MS (budget=$BUDGET_OBLIGATION_RUNTIME_MS)"
echo "  warmups=$MEASURE_WARMUPS"
echo "  repeats=$MEASURE_REPEATS"

[[ "$TEST_WALL_MS" -le "$BUDGET_TEST_WALL_MS" ]] || fail "test wall-time regression: $TEST_WALL_MS > $BUDGET_TEST_WALL_MS"
[[ "$SELFHOST_BOOTSTRAP_MS" -le "$BUDGET_SELFHOST_BOOTSTRAP_MS" ]] || fail "selfhost bootstrap regression: $SELFHOST_BOOTSTRAP_MS > $BUDGET_SELFHOST_BOOTSTRAP_MS"
[[ "$OBLIGATION_RUNTIME_MS" -le "$BUDGET_OBLIGATION_RUNTIME_MS" ]] || fail "obligation runtime regression: $OBLIGATION_RUNTIME_MS > $BUDGET_OBLIGATION_RUNTIME_MS"

echo "perf-budgets: ok"
