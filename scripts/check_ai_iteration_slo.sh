#!/usr/bin/env bash
set -euo pipefail

ROOT_DIR="$(cd "$(dirname "$0")/.." && pwd)"
cd "$ROOT_DIR"

source "$ROOT_DIR/scripts/lib/gcpm_caps_fixture.sh"

BUDGET_INCREMENTAL_WARM_MS="${GENESIS_BUDGET_INCREMENTAL_WARM_MS:-5000}"
BUDGET_CORE_SUITE_MS="${GENESIS_BUDGET_CORE_SUITE_MS:-45000}"
BUDGET_CHANGED_FAST_MS="${GENESIS_BUDGET_CHANGED_FAST_MS:-15000}"
BUDGET_GCPM_LOCK_MS="${GENESIS_BUDGET_GCPM_LOCK_MS:-5000}"
BUDGET_GCPM_ENV_MS="${GENESIS_BUDGET_GCPM_ENV_MS:-1000}"
CARGO_PROFILE="${GENESIS_PERF_CARGO_PROFILE:-selfhost-strict}"
DISK_STRICT_MODE="${GENESIS_PERF_DISK_STRICT_MODE:-1}"
REPORT_OUT="${GENESIS_AI_ITERATION_SLO_OUT:-.genesis/perf/ai_iteration_slo_metrics.json}"
HISTORY_OUT="${GENESIS_AI_ITERATION_SLO_HISTORY:-.genesis/perf/ai_iteration_slo_history.jsonl}"
HISTORY_MIN_SAMPLES="${GENESIS_AI_ITERATION_SLO_MIN_HISTORY:-5}"
REGRESSION_PERCENT="${GENESIS_AI_ITERATION_SLO_REGRESSION_PERCENT:-20}"

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

run_core_suite() {
  cargo test -p gc_coreform -p gc_kernel -p gc_prelude -p gc_cli --test cli_smoke --quiet --profile "$CARGO_PROFILE" "$@"
}

# One warm-up pass to amortize startup and artifact load effects.
run_incremental_loop >/dev/null

echo "ai-iteration-slo: measuring warm incremental loop"
measure_ms incremental_warm_ms run_incremental_loop
INCREMENTAL_WARM_MS="$MEASURE_LAST_MS"

echo "ai-iteration-slo: measuring default changed-file fast loop"
measure_ms changed_fast_ms run_changed_fast_loop
CHANGED_FAST_MS="$MEASURE_LAST_MS"

echo "ai-iteration-slo: warming core suite build graph"
run_core_suite --no-run >/dev/null

echo "ai-iteration-slo: measuring core suite wall-time"
measure_ms core_suite_ms run_core_suite
CORE_SUITE_MS="$MEASURE_LAST_MS"

echo "ai-iteration-slo: measuring gcpm lock/env iteration path"
run_gcpm_tmp new --workspace "slo" --policy "policy:default-v0.1" --registry-default "gen://registry" >/dev/null
measure_ms gcpm_lock_ms run_gcpm_tmp lock --strict
GCPM_LOCK_MS="$MEASURE_LAST_MS"
measure_ms gcpm_env_ms run_gcpm_tmp env --profile dev
GCPM_ENV_MS="$MEASURE_LAST_MS"

mkdir -p "$(dirname "$REPORT_OUT")"
mkdir -p "$(dirname "$HISTORY_OUT")"

python3 - "$REPORT_OUT" "$HISTORY_OUT" "$CARGO_PROFILE" "$TARGET_PROFILE_DIR" "$DISK_STRICT_MODE" "$HISTORY_MIN_SAMPLES" "$REGRESSION_PERCENT" "$INCREMENTAL_WARM_MS" "$CHANGED_FAST_MS" "$CORE_SUITE_MS" "$GCPM_LOCK_MS" "$GCPM_ENV_MS" "$BUDGET_INCREMENTAL_WARM_MS" "$BUDGET_CHANGED_FAST_MS" "$BUDGET_CORE_SUITE_MS" "$BUDGET_GCPM_LOCK_MS" "$BUDGET_GCPM_ENV_MS" <<'PY'
import json
import math
import os
import sys
import time

(
    report_path,
    history_path,
    profile,
    target_profile_dir,
    disk_strict_mode,
    history_min_samples_s,
    regression_percent_s,
    incremental_warm_ms_s,
    changed_fast_ms_s,
    core_suite_ms_s,
    gcpm_lock_ms_s,
    gcpm_env_ms_s,
    budget_incremental_warm_ms_s,
    budget_changed_fast_ms_s,
    budget_core_suite_ms_s,
    budget_gcpm_lock_ms_s,
    budget_gcpm_env_ms_s,
) = sys.argv[1:]

history_min_samples = int(history_min_samples_s)
regression_percent = float(regression_percent_s)
metrics = {
    "incremental_warm_ms": int(incremental_warm_ms_s),
    "changed_fast_ms": int(changed_fast_ms_s),
    "core_suite_ms": int(core_suite_ms_s),
    "gcpm_lock_ms": int(gcpm_lock_ms_s),
    "gcpm_env_ms": int(gcpm_env_ms_s),
}
budgets = {
    "incremental_warm_ms": int(budget_incremental_warm_ms_s),
    "changed_fast_ms": int(budget_changed_fast_ms_s),
    "core_suite_ms": int(budget_core_suite_ms_s),
    "gcpm_lock_ms": int(budget_gcpm_lock_ms_s),
    "gcpm_env_ms": int(budget_gcpm_env_ms_s),
}


def p95(values):
    if not values:
        return None
    ordered = sorted(values)
    idx = max(0, min(len(ordered) - 1, int(round(0.95 * (len(ordered) - 1)))))
    return ordered[idx]


history = []
if os.path.exists(history_path):
    with open(history_path, "r", encoding="utf-8") as f:
        for line in f:
            line = line.strip()
            if not line:
                continue
            try:
                obj = json.loads(line)
            except Exception:
                continue
            if isinstance(obj, dict) and isinstance(obj.get("metrics"), dict):
                history.append(obj)

baseline_stats = {}
for key in metrics:
    values = []
    for row in history:
        value = row.get("metrics", {}).get(key)
        if isinstance(value, int):
            values.append(value)
    baseline_stats[key] = {
        "samples": len(values),
        "p95_ms": p95(values),
    }

entry = {
    "kind": "genesis/ai-iteration-slo-v0.1",
    "timestamp_unix_s": int(time.time()),
    "build_profile": profile,
    "build_mode": "release-equivalent",
    "build_target_dir": target_profile_dir,
    "disk_strict_mode": disk_strict_mode,
    "metrics": metrics,
    "budgets": budgets,
}

history.append(entry)
history = history[-200:]
with open(history_path, "w", encoding="utf-8") as f:
    for row in history:
        f.write(json.dumps(row, sort_keys=True))
        f.write("\n")

thresholds = {}
regressions = []
for key, value in metrics.items():
    p95_ms = baseline_stats[key]["p95_ms"]
    samples = baseline_stats[key]["samples"]
    regression_budget = None
    if p95_ms is not None and samples >= history_min_samples:
        regression_budget = int(math.ceil(p95_ms * (1.0 + regression_percent / 100.0)))
        if value > regression_budget:
            regressions.append(
                f"{key} regression: {value}ms exceeds baseline-p95-regression budget {regression_budget}ms "
                f"(baseline p95 {p95_ms}ms, samples={samples}, regression={regression_percent:.1f}%)"
            )
    thresholds[key] = {
        "baseline_samples": samples,
        "baseline_p95_ms": p95_ms,
        "regression_budget_ms": regression_budget,
        "regression_enforced": p95_ms is not None and samples >= history_min_samples,
    }

report = {
    **entry,
    "history_path": history_path,
    "history_min_samples": history_min_samples,
    "regression_percent": regression_percent,
    "thresholds": thresholds,
}
with open(report_path, "w", encoding="utf-8") as f:
    json.dump(report, f, indent=2, sort_keys=True)
    f.write("\n")

print(json.dumps(report, sort_keys=True))

for key, value in metrics.items():
    if value > budgets[key]:
        raise SystemExit(
            f"ai-iteration-slo: {key} regression: {value} > {budgets[key]}"
        )
if regressions:
    raise SystemExit("ai-iteration-slo: " + "; ".join(regressions))
PY

echo "ai-iteration-slo: wrote report $REPORT_OUT"

echo "ai-iteration-slo: metrics"
echo "  incremental_warm_ms=$INCREMENTAL_WARM_MS (budget=$BUDGET_INCREMENTAL_WARM_MS)"
echo "  changed_fast_ms=$CHANGED_FAST_MS (budget=$BUDGET_CHANGED_FAST_MS)"
echo "  core_suite_ms=$CORE_SUITE_MS (budget=$BUDGET_CORE_SUITE_MS)"
echo "  gcpm_lock_ms=$GCPM_LOCK_MS (budget=$BUDGET_GCPM_LOCK_MS)"
echo "  gcpm_env_ms=$GCPM_ENV_MS (budget=$BUDGET_GCPM_ENV_MS)"

[[ "$INCREMENTAL_WARM_MS" -le "$BUDGET_INCREMENTAL_WARM_MS" ]] || fail "warm incremental loop regression: $INCREMENTAL_WARM_MS > $BUDGET_INCREMENTAL_WARM_MS"
[[ "$CHANGED_FAST_MS" -le "$BUDGET_CHANGED_FAST_MS" ]] || fail "changed fast loop regression: $CHANGED_FAST_MS > $BUDGET_CHANGED_FAST_MS"
[[ "$CORE_SUITE_MS" -le "$BUDGET_CORE_SUITE_MS" ]] || fail "core suite regression: $CORE_SUITE_MS > $BUDGET_CORE_SUITE_MS"
[[ "$GCPM_LOCK_MS" -le "$BUDGET_GCPM_LOCK_MS" ]] || fail "gcpm lock regression: $GCPM_LOCK_MS > $BUDGET_GCPM_LOCK_MS"
[[ "$GCPM_ENV_MS" -le "$BUDGET_GCPM_ENV_MS" ]] || fail "gcpm env regression: $GCPM_ENV_MS > $BUDGET_GCPM_ENV_MS"

echo "ai-iteration-slo: ok"
