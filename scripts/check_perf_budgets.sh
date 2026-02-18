#!/usr/bin/env bash
set -euo pipefail

ROOT_DIR="$(cd "$(dirname "$0")/.." && pwd)"
cd "$ROOT_DIR"

# Default budgets are intentionally conservative for shared CI runners.
# Override with env vars to tighten over time.
BUDGET_TEST_WALL_MS="${GENESIS_BUDGET_TEST_WALL_MS:-120000}"
BUDGET_SELFHOST_BOOTSTRAP_MS="${GENESIS_BUDGET_SELFHOST_BOOTSTRAP_MS:-15000}"
BUDGET_OBLIGATION_RUNTIME_MS="${GENESIS_BUDGET_OBLIGATION_RUNTIME_MS:-30000}"

now_ns() {
  python3 - <<'PY'
import time
print(time.time_ns())
PY
}

measure_ms() {
  local label="$1"
  shift
  local start_ns end_ns elapsed_ms
  start_ns="$(now_ns)"
  "$@" >/dev/null
  end_ns="$(now_ns)"
  elapsed_ms="$(( (end_ns - start_ns) / 1000000 ))"
  echo "$label=$elapsed_ms"
}

fail() {
  echo "perf-budgets: $*" >&2
  exit 1
}

echo "perf-budgets: preparing genesis binary"
cargo build -p gc_cli >/dev/null

TMP_DIR="$(mktemp -d)"
cleanup() {
  rm -rf "$TMP_DIR"
}
trap cleanup EXIT

echo "perf-budgets: measuring wall-time budget via cli_smoke integration test"
TEST_WALL_LINE="$(measure_ms test_wall_ms cargo test -p gc_cli --test cli_smoke --quiet)"
TEST_WALL_MS="${TEST_WALL_LINE#*=}"

echo "perf-budgets: measuring selfhost bootstrap artifact build time"
BOOTSTRAP_LINE="$(
  measure_ms selfhost_bootstrap_ms \
    target/debug/genesis selfhost-artifact --out "$TMP_DIR/toolchain.gc"
)"
SELFHOST_BOOTSTRAP_MS="${BOOTSTRAP_LINE#*=}"

echo "perf-budgets: measuring obligation runtime on hello_pkg"
OBLIGATION_LINE="$(
  measure_ms obligation_runtime_ms \
    target/debug/genesis \
    --selfhost-only \
    --coreform-frontend selfhost \
    --selfhost-artifact "$TMP_DIR/toolchain.gc" \
    test --pkg examples/hello_pkg/package.toml
)"
OBLIGATION_RUNTIME_MS="${OBLIGATION_LINE#*=}"

echo "perf-budgets: metrics"
echo "  test_wall_ms=$TEST_WALL_MS (budget=$BUDGET_TEST_WALL_MS)"
echo "  selfhost_bootstrap_ms=$SELFHOST_BOOTSTRAP_MS (budget=$BUDGET_SELFHOST_BOOTSTRAP_MS)"
echo "  obligation_runtime_ms=$OBLIGATION_RUNTIME_MS (budget=$BUDGET_OBLIGATION_RUNTIME_MS)"

[[ "$TEST_WALL_MS" -le "$BUDGET_TEST_WALL_MS" ]] || fail "test wall-time regression: $TEST_WALL_MS > $BUDGET_TEST_WALL_MS"
[[ "$SELFHOST_BOOTSTRAP_MS" -le "$BUDGET_SELFHOST_BOOTSTRAP_MS" ]] || fail "selfhost bootstrap regression: $SELFHOST_BOOTSTRAP_MS > $BUDGET_SELFHOST_BOOTSTRAP_MS"
[[ "$OBLIGATION_RUNTIME_MS" -le "$BUDGET_OBLIGATION_RUNTIME_MS" ]] || fail "obligation runtime regression: $OBLIGATION_RUNTIME_MS > $BUDGET_OBLIGATION_RUNTIME_MS"

echo "perf-budgets: ok"
