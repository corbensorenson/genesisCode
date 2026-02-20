#!/usr/bin/env bash
set -euo pipefail

ROOT_DIR="$(cd "$(dirname "$0")/.." && pwd)"
cd "$ROOT_DIR"

source "$ROOT_DIR/scripts/lib/gcpm_caps_fixture.sh"

BUDGET_INCREMENTAL_WARM_MS="${GENESIS_BUDGET_INCREMENTAL_WARM_MS:-60000}"
BUDGET_CORE_SUITE_MS="${GENESIS_BUDGET_CORE_SUITE_MS:-300000}"
BUDGET_CHANGED_FAST_MS="${GENESIS_BUDGET_CHANGED_FAST_MS:-300000}"
BUDGET_GCPM_LOCK_MS="${GENESIS_BUDGET_GCPM_LOCK_MS:-20000}"
BUDGET_GCPM_ENV_MS="${GENESIS_BUDGET_GCPM_ENV_MS:-15000}"
CARGO_PROFILE="${GENESIS_PERF_CARGO_PROFILE:-selfhost-strict}"
DISK_STRICT_MODE="${GENESIS_PERF_DISK_STRICT_MODE:-1}"
REPORT_OUT="${GENESIS_AI_ITERATION_SLO_OUT:-.genesis/perf/ai_iteration_slo_metrics.json}"

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
  if ! "$@" >/dev/null; then
    echo "ai-iteration-slo: measurement command failed for ${label}: $*" >&2
    return 1
  fi
  end_ns="$(now_ns)"
  elapsed_ms="$(( (end_ns - start_ns) / 1000000 ))"
  MEASURE_LAST_LABEL="$label"
  MEASURE_LAST_MS="$elapsed_ms"
}

fail() {
  echo "ai-iteration-slo: $*" >&2
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

bash scripts/check_disk_headroom.sh --path "$ROOT_DIR" --context "ai-iteration-slo" --strict "$DISK_STRICT_MODE"

TMP_DIR="$(mktemp -d)"
cleanup() {
  rm -rf "$TMP_DIR"
}
trap cleanup EXIT

echo "ai-iteration-slo: preparing genesis binary"
cargo build -p gc_cli --profile "$CARGO_PROFILE" >/dev/null
GENESIS_BIN="$ROOT_DIR/target/$TARGET_PROFILE_DIR/genesis"
[[ -x "$GENESIS_BIN" ]] || fail "unable to locate genesis binary at $GENESIS_BIN"

for name in basic.gc caps.toml package.toml pure.gcpatch; do
  cp "tests/spec/pkg_basic/$name" "$TMP_DIR/$name"
done

TOOLCHAIN="$TMP_DIR/toolchain.gc"
"$GENESIS_BIN" selfhost-artifact --out "$TOOLCHAIN" >/dev/null

run_incremental_loop() {
  "$GENESIS_BIN" \
    --selfhost-artifact "$TOOLCHAIN" \
    fmt "$TMP_DIR/basic.gc" --engine selfhost

  "$GENESIS_BIN" \
    --selfhost-artifact "$TOOLCHAIN" \
    eval "$TMP_DIR/basic.gc" --engine selfhost

  "$GENESIS_BIN" \
    --selfhost-artifact "$TOOLCHAIN" \
    typecheck --pkg "$TMP_DIR/package.toml"

  "$GENESIS_BIN" \
    --selfhost-artifact "$TOOLCHAIN" \
    test --pkg "$TMP_DIR/package.toml"
}

write_gcpm_low_caps_fixture "$TMP_DIR/gcpm_caps.toml"

run_gcpm_tmp() {
  (
    cd "$TMP_DIR"
    "$GENESIS_BIN" --selfhost-artifact "$TOOLCHAIN" gcpm --caps "$TMP_DIR/gcpm_caps.toml" "$@"
  )
}

run_changed_fast_loop() {
  bash scripts/test_changed_fast.sh \
    --base HEAD \
    --runner cargo \
    --budget-ms "$BUDGET_CHANGED_FAST_MS" \
    --min-history 1 \
    --strict-disk "$DISK_STRICT_MODE" \
    --report "$TMP_DIR/test_changed_fast_metrics.json" \
    --history "$TMP_DIR/test_changed_fast_history.jsonl"
}

# One warm-up pass to amortize startup and artifact load effects.
run_incremental_loop >/dev/null

echo "ai-iteration-slo: measuring warm incremental loop"
measure_ms incremental_warm_ms run_incremental_loop
INCREMENTAL_WARM_MS="$MEASURE_LAST_MS"

echo "ai-iteration-slo: measuring default changed-file fast loop"
measure_ms changed_fast_ms run_changed_fast_loop
CHANGED_FAST_MS="$MEASURE_LAST_MS"

echo "ai-iteration-slo: measuring core suite wall-time"
measure_ms core_suite_ms cargo test -p gc_coreform -p gc_kernel -p gc_prelude -p gc_cli --test cli_smoke --quiet --profile "$CARGO_PROFILE"
CORE_SUITE_MS="$MEASURE_LAST_MS"

echo "ai-iteration-slo: measuring gcpm lock/env iteration path"
run_gcpm_tmp new --workspace "slo" --policy "policy:default-v0.1" --registry-default "gen://registry" >/dev/null
measure_ms gcpm_lock_ms run_gcpm_tmp lock --strict
GCPM_LOCK_MS="$MEASURE_LAST_MS"
measure_ms gcpm_env_ms run_gcpm_tmp env --profile dev
GCPM_ENV_MS="$MEASURE_LAST_MS"

echo "ai-iteration-slo: metrics"
echo "  incremental_warm_ms=$INCREMENTAL_WARM_MS (budget=$BUDGET_INCREMENTAL_WARM_MS)"
echo "  changed_fast_ms=$CHANGED_FAST_MS (budget=$BUDGET_CHANGED_FAST_MS)"
echo "  core_suite_ms=$CORE_SUITE_MS (budget=$BUDGET_CORE_SUITE_MS)"
echo "  gcpm_lock_ms=$GCPM_LOCK_MS (budget=$BUDGET_GCPM_LOCK_MS)"
echo "  gcpm_env_ms=$GCPM_ENV_MS (budget=$BUDGET_GCPM_ENV_MS)"

mkdir -p "$(dirname "$REPORT_OUT")"
cat > "$REPORT_OUT" <<EOF
{
  "kind": "genesis/ai-iteration-slo-v0.1",
  "build_profile": "$CARGO_PROFILE",
  "build_mode": "release-equivalent",
  "build_target_dir": "$TARGET_PROFILE_DIR",
  "disk_strict_mode": "$DISK_STRICT_MODE",
  "metrics": {
    "incremental_warm_ms": $INCREMENTAL_WARM_MS,
    "changed_fast_ms": $CHANGED_FAST_MS,
    "core_suite_ms": $CORE_SUITE_MS,
    "gcpm_lock_ms": $GCPM_LOCK_MS,
    "gcpm_env_ms": $GCPM_ENV_MS
  },
  "budgets": {
    "incremental_warm_ms": $BUDGET_INCREMENTAL_WARM_MS,
    "changed_fast_ms": $BUDGET_CHANGED_FAST_MS,
    "core_suite_ms": $BUDGET_CORE_SUITE_MS,
    "gcpm_lock_ms": $BUDGET_GCPM_LOCK_MS,
    "gcpm_env_ms": $BUDGET_GCPM_ENV_MS
  }
}
EOF
echo "ai-iteration-slo: wrote report $REPORT_OUT"

[[ "$INCREMENTAL_WARM_MS" -le "$BUDGET_INCREMENTAL_WARM_MS" ]] || fail "warm incremental loop regression: $INCREMENTAL_WARM_MS > $BUDGET_INCREMENTAL_WARM_MS"
[[ "$CHANGED_FAST_MS" -le "$BUDGET_CHANGED_FAST_MS" ]] || fail "changed fast loop regression: $CHANGED_FAST_MS > $BUDGET_CHANGED_FAST_MS"
[[ "$CORE_SUITE_MS" -le "$BUDGET_CORE_SUITE_MS" ]] || fail "core suite regression: $CORE_SUITE_MS > $BUDGET_CORE_SUITE_MS"
[[ "$GCPM_LOCK_MS" -le "$BUDGET_GCPM_LOCK_MS" ]] || fail "gcpm lock regression: $GCPM_LOCK_MS > $BUDGET_GCPM_LOCK_MS"
[[ "$GCPM_ENV_MS" -le "$BUDGET_GCPM_ENV_MS" ]] || fail "gcpm env regression: $GCPM_ENV_MS > $BUDGET_GCPM_ENV_MS"

echo "ai-iteration-slo: ok"
