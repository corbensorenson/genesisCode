#!/usr/bin/env bash
set -euo pipefail

ROOT_DIR="$(cd "$(dirname "$0")/.." && pwd)"
cd "$ROOT_DIR"

if [[ "$#" -ne 4 ]]; then
  echo "usage: $0 <metrics-output> <runtime-report-output> <runtime-history-output> <runtime-history-input>" >&2
  exit 2
fi

REPORT_OUT="$1"
RUNTIME_REPORT="$2"
HISTORY_OUT="$3"
RUNTIME_HISTORY_INPUT="$4"

source "$ROOT_DIR/scripts/lib/cargo_target_dir.sh"
genesis_configure_cargo_target_dir \
  "$ROOT_DIR" \
  "check-perf-budgets" \
  root-host

source "$ROOT_DIR/scripts/lib/measure.sh"
source "$ROOT_DIR/scripts/lib/perf_disk_mode.sh"
source "$ROOT_DIR/scripts/lib/profile_gate_timing.sh"

START_MS="$(genesis_profile_gate_now_ms)"

# Default budgets are intentionally conservative for shared CI runners.
# Override with env vars to tighten over time.
BUDGET_TEST_WALL_MS="${GENESIS_BUDGET_TEST_WALL_MS:-120000}"
BUDGET_SELFHOST_BOOTSTRAP_MS="${GENESIS_BUDGET_SELFHOST_BOOTSTRAP_MS:-15000}"
BUDGET_OBLIGATION_RUNTIME_MS="${GENESIS_BUDGET_OBLIGATION_RUNTIME_MS:-30000}"
MEASURE_WARMUPS="${GENESIS_BUDGET_WARMUPS:-1}"
MEASURE_REPEATS="${GENESIS_BUDGET_REPEATS:-3}"
CARGO_PROFILE="${GENESIS_PERF_CARGO_PROFILE:-selfhost-strict}"
DISK_STRICT_MODE="$(genesis_resolve_perf_disk_strict_mode)"
DISK_MIN_FREE_KB="${GENESIS_PERF_BUDGET_MIN_FREE_KB:-3145728}"
RUNTIME_BUDGET_MS="${GENESIS_PERF_BUDGET_RUNTIME_BUDGET_MS:-900000}"
RUNTIME_BASELINE_HISTORY="${GENESIS_PERF_BUDGET_RUNTIME_BASELINE_HISTORY_OUT:-policies/perf/perf_budget_runtime_seed_history.jsonl}"
RUNTIME_MIN_HISTORY="${GENESIS_PERF_BUDGET_RUNTIME_MIN_HISTORY:-5}"
RUNTIME_REQUIRE_MIN_HISTORY="${GENESIS_PERF_BUDGET_RUNTIME_REQUIRE_MIN_HISTORY:-1}"

if [[ ! "$RUNTIME_MIN_HISTORY" =~ ^[0-9]+$ || "$RUNTIME_MIN_HISTORY" -le 0 ]]; then
  echo "perf-budgets: GENESIS_PERF_BUDGET_RUNTIME_MIN_HISTORY must be a positive integer" >&2
  exit 2
fi
if [[ "$RUNTIME_REQUIRE_MIN_HISTORY" != "0" && "$RUNTIME_REQUIRE_MIN_HISTORY" != "1" ]]; then
  echo "perf-budgets: GENESIS_PERF_BUDGET_RUNTIME_REQUIRE_MIN_HISTORY must be 0 or 1" >&2
  exit 2
fi

fail() {
  echo "perf-budgets: $*" >&2
  exit 1
}

profile_target_dir() {
  case "$1" in
    release) echo "release" ;;
    dev|test) echo "debug" ;;
    *) echo "$1" ;;
  esac
}

TARGET_PROFILE_DIR="$(profile_target_dir "$CARGO_PROFILE")"
GENESIS_BIN="$CARGO_TARGET_DIR/$TARGET_PROFILE_DIR/genesis"

bash scripts/check_disk_headroom.sh \
  --path "$ROOT_DIR" \
  --context "perf-budgets" \
  --min-kb "$DISK_MIN_FREE_KB" \
  --strict "$DISK_STRICT_MODE"

echo "perf-budgets: preparing genesis binary"
cargo build -p gc_cli --profile "$CARGO_PROFILE" >/dev/null
cargo test -p gc_cli --test cli_smoke --no-run --quiet --profile "$CARGO_PROFILE" >/dev/null

CLI_SMOKE_BIN="$(
  find "$CARGO_TARGET_DIR/$TARGET_PROFILE_DIR/deps" -maxdepth 1 -type f -name 'cli_smoke-*' -perm -u+x \
    | sort \
    | tail -n 1
)"
[[ -x "${CLI_SMOKE_BIN:-}" ]] || fail "unable to locate compiled cli_smoke test binary"
[[ -x "$GENESIS_BIN" ]] || fail "unable to locate genesis binary at $GENESIS_BIN"

TMP_DIR="$(mktemp -d)"
cleanup() {
  rm -rf "$TMP_DIR"
}
trap cleanup EXIT

echo "perf-budgets: measuring selfhost bootstrap artifact build time"
genesis_measure_best_of_ms \
  selfhost_bootstrap_ms \
  "$MEASURE_WARMUPS" \
  "$MEASURE_REPEATS" \
  "$GENESIS_BIN" selfhost-artifact --out "$TMP_DIR/toolchain.gc"
SELFHOST_BOOTSTRAP_MS="$MEASURE_LAST_MS"

echo "perf-budgets: measuring wall-time budget via prebuilt cli_smoke runtime"
genesis_measure_best_of_ms \
  test_wall_ms \
  "$MEASURE_WARMUPS" \
  "$MEASURE_REPEATS" \
  env GENESIS_SELFHOST_TOOLCHAIN_ARTIFACT="$TMP_DIR/toolchain.gc" \
  "$CLI_SMOKE_BIN" --quiet
TEST_WALL_MS="$MEASURE_LAST_MS"

echo "perf-budgets: measuring obligation runtime on hello_pkg"
genesis_measure_best_of_ms \
  obligation_runtime_ms \
  "$MEASURE_WARMUPS" \
  "$MEASURE_REPEATS" \
  "$GENESIS_BIN" \
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

mkdir -p "$(dirname "$REPORT_OUT")"
cat > "$REPORT_OUT" <<EOF
{
  "kind": "genesis/perf-budgets-v0.1",
  "build_profile": "$CARGO_PROFILE",
  "build_mode": "release-equivalent",
  "build_target_dir": "$TARGET_PROFILE_DIR",
  "disk_strict_mode": "$DISK_STRICT_MODE",
  "measure_warmups": $MEASURE_WARMUPS,
  "measure_repeats": $MEASURE_REPEATS,
  "metrics": {
    "test_wall_ms": $TEST_WALL_MS,
    "selfhost_bootstrap_ms": $SELFHOST_BOOTSTRAP_MS,
    "obligation_runtime_ms": $OBLIGATION_RUNTIME_MS
  },
  "budgets": {
    "test_wall_ms": $BUDGET_TEST_WALL_MS,
    "selfhost_bootstrap_ms": $BUDGET_SELFHOST_BOOTSTRAP_MS,
    "obligation_runtime_ms": $BUDGET_OBLIGATION_RUNTIME_MS
  }
}
EOF
echo "perf-budgets: wrote report $REPORT_OUT"

[[ "$TEST_WALL_MS" -le "$BUDGET_TEST_WALL_MS" ]] || fail "test wall-time regression: $TEST_WALL_MS > $BUDGET_TEST_WALL_MS"
[[ "$SELFHOST_BOOTSTRAP_MS" -le "$BUDGET_SELFHOST_BOOTSTRAP_MS" ]] || fail "selfhost bootstrap regression: $SELFHOST_BOOTSTRAP_MS > $BUDGET_SELFHOST_BOOTSTRAP_MS"
[[ "$OBLIGATION_RUNTIME_MS" -le "$BUDGET_OBLIGATION_RUNTIME_MS" ]] || fail "obligation runtime regression: $OBLIGATION_RUNTIME_MS > $BUDGET_OBLIGATION_RUNTIME_MS"

EFFECTIVE_BASELINE_HISTORY="$RUNTIME_BASELINE_HISTORY"
MERGED_BASELINE_HISTORY=""
if [[ "$RUNTIME_HISTORY_INPUT" != "$HISTORY_OUT" && -f "$RUNTIME_HISTORY_INPUT" ]]; then
  MERGED_BASELINE_HISTORY="$(mktemp)"
  cat "$RUNTIME_BASELINE_HISTORY" "$RUNTIME_HISTORY_INPUT" >"$MERGED_BASELINE_HISTORY"
  EFFECTIVE_BASELINE_HISTORY="$MERGED_BASELINE_HISTORY"
fi
cleanup_baseline() {
  [[ -z "$MERGED_BASELINE_HISTORY" ]] || rm -f "$MERGED_BASELINE_HISTORY"
}
trap 'cleanup_baseline; cleanup' EXIT

genesis_profile_gate_emit_runtime_report \
  "perf-budgets" \
  "genesis/perf-budgets-runtime-v0.1" \
  "$RUNTIME_REPORT" \
  "$HISTORY_OUT" \
  "$START_MS" \
  "$RUNTIME_BUDGET_MS" \
  "$RUNTIME_MIN_HISTORY" \
  "{\"metrics_report\":\"$REPORT_OUT\",\"build_profile\":\"$CARGO_PROFILE\"}" \
  "" \
  "$EFFECTIVE_BASELINE_HISTORY" \
  "$RUNTIME_REQUIRE_MIN_HISTORY"

echo "perf-budgets: ok"
