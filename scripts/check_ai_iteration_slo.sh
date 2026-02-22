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
BASELINE_HISTORY="${GENESIS_AI_ITERATION_SLO_BASELINE:-policies/perf/ai_iteration_slo_seed_history.jsonl}"
HISTORY_MIN_SAMPLES="${GENESIS_AI_ITERATION_SLO_MIN_HISTORY:-5}"
REGRESSION_PERCENT="${GENESIS_AI_ITERATION_SLO_REGRESSION_PERCENT:-100}"
SAMPLES_INCREMENTAL_WARM="${GENESIS_AI_ITERATION_SLO_SAMPLES_INCREMENTAL_WARM:-3}"
SAMPLES_CHANGED_FAST="${GENESIS_AI_ITERATION_SLO_SAMPLES_CHANGED_FAST:-2}"
SAMPLES_CORE_SUITE="${GENESIS_AI_ITERATION_SLO_SAMPLES_CORE_SUITE:-2}"
SAMPLES_GCPM_LOCK="${GENESIS_AI_ITERATION_SLO_SAMPLES_GCPM_LOCK:-2}"
SAMPLES_GCPM_ENV="${GENESIS_AI_ITERATION_SLO_SAMPLES_GCPM_ENV:-2}"
WARMUP_GCPM_LOCK="${GENESIS_AI_ITERATION_SLO_WARMUP_GCPM_LOCK:-1}"
WARMUP_GCPM_ENV="${GENESIS_AI_ITERATION_SLO_WARMUP_GCPM_ENV:-1}"
STABILIZE_RETRIES_GCPM_LOCK="${GENESIS_AI_ITERATION_SLO_STABILIZE_RETRIES_GCPM_LOCK:-3}"
STABILIZE_RETRIES_GCPM_ENV="${GENESIS_AI_ITERATION_SLO_STABILIZE_RETRIES_GCPM_ENV:-3}"
CONTENTION_WARN_PERCENT="${GENESIS_AI_ITERATION_SLO_CONTENTION_WARN_PERCENT:-60}"

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

require_positive_int() {
  local name="$1"
  local value="$2"
  [[ "$value" =~ ^[1-9][0-9]*$ ]] || fail "$name must be a positive integer (got '$value')"
}

require_non_negative_int() {
  local name="$1"
  local value="$2"
  [[ "$value" =~ ^[0-9]+$ ]] || fail "$name must be a non-negative integer (got '$value')"
}

measure_ms_samples() {
  local label="$1"
  local sample_count="$2"
  shift 2
  local -a cmd=("$@")
  local -a samples=()
  local i csv stats

  for ((i = 1; i <= sample_count; i += 1)); do
    measure_ms "${label}" "${cmd[@]}"
    samples+=("$MEASURE_LAST_MS")
  done
  csv="$(IFS=,; echo "${samples[*]}")"
  stats="$(python3 - "$csv" <<'PY'
import math
import sys

values = [int(x) for x in sys.argv[1].split(",") if x]
ordered = sorted(values)
n = len(ordered)
mid = n // 2
if n % 2 == 1:
    median = ordered[mid]
else:
    median = int((ordered[mid - 1] + ordered[mid]) / 2.0)
idx = max(0, min(n - 1, int(round(0.95 * (n - 1)))))
p95 = ordered[idx]
print(f"{median},{p95}")
PY
)"
  MEASURE_LAST_SAMPLES_CSV="$csv"
  MEASURE_LAST_MEDIAN_MS="${stats%,*}"
  MEASURE_LAST_P95_MS="${stats#*,}"
  MEASURE_LAST_SPREAD_PCT="$(python3 - "$csv" <<'PY'
import sys

values = [int(x) for x in sys.argv[1].split(",") if x]
ordered = sorted(values)
n = len(ordered)
mid = n // 2
if n % 2 == 1:
    med = ordered[mid]
else:
    med = int((ordered[mid - 1] + ordered[mid]) / 2.0)
if med <= 0:
    spread = 0.0
else:
    spread = ((ordered[-1] - ordered[0]) * 100.0) / med
print(f"{spread:.2f}")
PY
)"
}

measure_ms_samples_stabilized() {
  local label="$1"
  local sample_count="$2"
  local warmup_runs="$3"
  local max_retries="$4"
  local spread_threshold="$5"
  shift 5
  local -a cmd=("$@")
  local i retries
  local -a all_samples=()
  local -a window_samples=()
  local csv

  for ((i = 1; i <= warmup_runs; i += 1)); do
    measure_ms "${label}:warmup" "${cmd[@]}"
  done

  for ((i = 1; i <= sample_count; i += 1)); do
    measure_ms "${label}" "${cmd[@]}"
    all_samples+=("$MEASURE_LAST_MS")
    window_samples+=("$MEASURE_LAST_MS")
  done

  retries=0
  while true; do
    csv="$(IFS=,; echo "${window_samples[*]}")"
    # Compute stats from the stabilized window only; do not rerun the measured command here.
    MEASURE_LAST_SAMPLES_CSV="$csv"
    MEASURE_LAST_MEDIAN_MS="$(python3 - "$csv" <<'PY'
import sys
values = sorted(int(x) for x in sys.argv[1].split(",") if x)
n = len(values)
mid = n // 2
if n % 2 == 1:
    print(values[mid])
else:
    print(int((values[mid - 1] + values[mid]) / 2.0))
PY
)"
    MEASURE_LAST_P95_MS="$(python3 - "$csv" <<'PY'
import sys
values = sorted(int(x) for x in sys.argv[1].split(",") if x)
idx = max(0, min(len(values) - 1, int(round(0.95 * (len(values) - 1)))))
print(values[idx])
PY
)"
    MEASURE_LAST_SPREAD_PCT="$(python3 - "$csv" <<'PY'
import sys
values = sorted(int(x) for x in sys.argv[1].split(",") if x)
n = len(values)
mid = n // 2
if n % 2 == 1:
    med = values[mid]
else:
    med = int((values[mid - 1] + values[mid]) / 2.0)
spread = 0.0 if med <= 0 else ((values[-1] - values[0]) * 100.0) / med
print(f"{spread:.2f}")
PY
)"
    if (( retries >= max_retries )); then
      break
    fi
    if python3 - "$MEASURE_LAST_SPREAD_PCT" "$spread_threshold" <<'PY'
import sys
spread = float(sys.argv[1])
threshold = float(sys.argv[2])
raise SystemExit(0 if spread <= threshold else 1)
PY
    then
      break
    fi

    measure_ms "${label}:stabilize" "${cmd[@]}"
    all_samples+=("$MEASURE_LAST_MS")
    window_samples=("${window_samples[@]:1}" "$MEASURE_LAST_MS")
    retries=$((retries + 1))
  done

  MEASURE_STABILIZE_TOTAL_RUNS="${#all_samples[@]}"
  MEASURE_STABILIZE_EXTRA_RETRIES="$retries"
}

profile_target_dir() {
  case "$1" in
    release) echo "release" ;;
    dev|test) echo "debug" ;;
    *) echo "$1" ;;
  esac
}

TARGET_PROFILE_DIR="$(profile_target_dir "$CARGO_PROFILE")"

require_positive_int "GENESIS_AI_ITERATION_SLO_MIN_HISTORY" "$HISTORY_MIN_SAMPLES"
require_positive_int "GENESIS_AI_ITERATION_SLO_SAMPLES_INCREMENTAL_WARM" "$SAMPLES_INCREMENTAL_WARM"
require_positive_int "GENESIS_AI_ITERATION_SLO_SAMPLES_CHANGED_FAST" "$SAMPLES_CHANGED_FAST"
require_positive_int "GENESIS_AI_ITERATION_SLO_SAMPLES_CORE_SUITE" "$SAMPLES_CORE_SUITE"
require_positive_int "GENESIS_AI_ITERATION_SLO_SAMPLES_GCPM_LOCK" "$SAMPLES_GCPM_LOCK"
require_positive_int "GENESIS_AI_ITERATION_SLO_SAMPLES_GCPM_ENV" "$SAMPLES_GCPM_ENV"
require_non_negative_int "GENESIS_AI_ITERATION_SLO_WARMUP_GCPM_LOCK" "$WARMUP_GCPM_LOCK"
require_non_negative_int "GENESIS_AI_ITERATION_SLO_WARMUP_GCPM_ENV" "$WARMUP_GCPM_ENV"
require_non_negative_int "GENESIS_AI_ITERATION_SLO_STABILIZE_RETRIES_GCPM_LOCK" "$STABILIZE_RETRIES_GCPM_LOCK"
require_non_negative_int "GENESIS_AI_ITERATION_SLO_STABILIZE_RETRIES_GCPM_ENV" "$STABILIZE_RETRIES_GCPM_ENV"
require_non_negative_int "GENESIS_AI_ITERATION_SLO_CONTENTION_WARN_PERCENT" "$CONTENTION_WARN_PERCENT"

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

echo "ai-iteration-slo: measuring warm incremental loop (samples=$SAMPLES_INCREMENTAL_WARM, statistic=median)"
measure_ms_samples incremental_warm_ms "$SAMPLES_INCREMENTAL_WARM" run_incremental_loop
INCREMENTAL_WARM_MS="$MEASURE_LAST_MEDIAN_MS"
INCREMENTAL_WARM_SAMPLES="$MEASURE_LAST_SAMPLES_CSV"
INCREMENTAL_WARM_SAMPLE_P95_MS="$MEASURE_LAST_P95_MS"

echo "ai-iteration-slo: measuring default changed-file fast loop (samples=$SAMPLES_CHANGED_FAST, statistic=median)"
measure_ms_samples changed_fast_ms "$SAMPLES_CHANGED_FAST" run_changed_fast_loop
CHANGED_FAST_MS="$MEASURE_LAST_MEDIAN_MS"
CHANGED_FAST_SAMPLES="$MEASURE_LAST_SAMPLES_CSV"
CHANGED_FAST_SAMPLE_P95_MS="$MEASURE_LAST_P95_MS"

echo "ai-iteration-slo: warming core suite build graph"
run_core_suite --no-run >/dev/null

echo "ai-iteration-slo: measuring core suite wall-time (samples=$SAMPLES_CORE_SUITE, statistic=median)"
measure_ms_samples core_suite_ms "$SAMPLES_CORE_SUITE" run_core_suite
CORE_SUITE_MS="$MEASURE_LAST_MEDIAN_MS"
CORE_SUITE_SAMPLES="$MEASURE_LAST_SAMPLES_CSV"
CORE_SUITE_SAMPLE_P95_MS="$MEASURE_LAST_P95_MS"

echo "ai-iteration-slo: measuring gcpm lock/env iteration path"
run_gcpm_tmp new --workspace "slo" --policy "policy:default-v0.1" --registry-default "gen://registry" >/dev/null
measure_ms_samples_stabilized gcpm_lock_ms "$SAMPLES_GCPM_LOCK" "$WARMUP_GCPM_LOCK" "$STABILIZE_RETRIES_GCPM_LOCK" "$CONTENTION_WARN_PERCENT" run_gcpm_tmp lock --strict
GCPM_LOCK_MS="$MEASURE_LAST_MEDIAN_MS"
GCPM_LOCK_SAMPLES="$MEASURE_LAST_SAMPLES_CSV"
GCPM_LOCK_SAMPLE_P95_MS="$MEASURE_LAST_P95_MS"
GCPM_LOCK_SAMPLE_SPREAD_PCT="$MEASURE_LAST_SPREAD_PCT"
GCPM_LOCK_TOTAL_RUNS="$MEASURE_STABILIZE_TOTAL_RUNS"
GCPM_LOCK_STABILIZE_RETRIES="$MEASURE_STABILIZE_EXTRA_RETRIES"
measure_ms_samples_stabilized gcpm_env_ms "$SAMPLES_GCPM_ENV" "$WARMUP_GCPM_ENV" "$STABILIZE_RETRIES_GCPM_ENV" "$CONTENTION_WARN_PERCENT" run_gcpm_tmp env --profile dev
GCPM_ENV_MS="$MEASURE_LAST_MEDIAN_MS"
GCPM_ENV_SAMPLES="$MEASURE_LAST_SAMPLES_CSV"
GCPM_ENV_SAMPLE_P95_MS="$MEASURE_LAST_P95_MS"
GCPM_ENV_SAMPLE_SPREAD_PCT="$MEASURE_LAST_SPREAD_PCT"
GCPM_ENV_TOTAL_RUNS="$MEASURE_STABILIZE_TOTAL_RUNS"
GCPM_ENV_STABILIZE_RETRIES="$MEASURE_STABILIZE_EXTRA_RETRIES"

mkdir -p "$(dirname "$REPORT_OUT")"
mkdir -p "$(dirname "$HISTORY_OUT")"
mkdir -p "$(dirname "$BASELINE_HISTORY")"

python3 - "$REPORT_OUT" "$HISTORY_OUT" "$BASELINE_HISTORY" "$CARGO_PROFILE" "$TARGET_PROFILE_DIR" "$DISK_STRICT_MODE" "$HISTORY_MIN_SAMPLES" "$REGRESSION_PERCENT" "$CONTENTION_WARN_PERCENT" "$INCREMENTAL_WARM_SAMPLES" "$CHANGED_FAST_SAMPLES" "$CORE_SUITE_SAMPLES" "$GCPM_LOCK_SAMPLES" "$GCPM_ENV_SAMPLES" "$BUDGET_INCREMENTAL_WARM_MS" "$BUDGET_CHANGED_FAST_MS" "$BUDGET_CORE_SUITE_MS" "$BUDGET_GCPM_LOCK_MS" "$BUDGET_GCPM_ENV_MS" <<'PY'
import json
import math
import os
import sys
import time

(
    report_path,
    history_path,
    baseline_history_path,
    profile,
    target_profile_dir,
    disk_strict_mode,
    history_min_samples_s,
    regression_percent_s,
    contention_warn_percent_s,
    incremental_warm_samples_csv,
    changed_fast_samples_csv,
    core_suite_samples_csv,
    gcpm_lock_samples_csv,
    gcpm_env_samples_csv,
    budget_incremental_warm_ms_s,
    budget_changed_fast_ms_s,
    budget_core_suite_ms_s,
    budget_gcpm_lock_ms_s,
    budget_gcpm_env_ms_s,
) = sys.argv[1:]

history_min_samples = int(history_min_samples_s)
regression_percent = float(regression_percent_s)
contention_warn_percent = int(contention_warn_percent_s)
sample_csv = {
    "incremental_warm_ms": incremental_warm_samples_csv,
    "changed_fast_ms": changed_fast_samples_csv,
    "core_suite_ms": core_suite_samples_csv,
    "gcpm_lock_ms": gcpm_lock_samples_csv,
    "gcpm_env_ms": gcpm_env_samples_csv,
}

def parse_samples(raw):
    parts = [p for p in raw.split(",") if p]
    if not parts:
        raise SystemExit("ai-iteration-slo: empty measurement sample set")
    values = []
    for p in parts:
        try:
            v = int(p)
        except Exception as exc:
            raise SystemExit(f"ai-iteration-slo: invalid sample value {p!r}: {exc}") from exc
        if v < 0:
            raise SystemExit(f"ai-iteration-slo: negative sample value {v}")
        values.append(v)
    return values

measurement_samples = {key: parse_samples(raw) for key, raw in sample_csv.items()}

def median(values):
    ordered = sorted(values)
    n = len(ordered)
    mid = n // 2
    if n % 2 == 1:
        return ordered[mid]
    return int((ordered[mid - 1] + ordered[mid]) / 2.0)

metrics = {
    key: median(values) for key, values in measurement_samples.items()
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

sample_stats = {}
contention_warnings = []
for key, values in measurement_samples.items():
    ordered = sorted(values)
    min_v = ordered[0]
    max_v = ordered[-1]
    med_v = median(values)
    spread_pct = 0.0
    if med_v > 0:
        spread_pct = ((max_v - min_v) * 100.0) / med_v
    sample_stats[key] = {
        "samples": values,
        "count": len(values),
        "min_ms": min_v,
        "max_ms": max_v,
        "median_ms": med_v,
        "p95_ms": p95(values),
        "spread_pct": round(spread_pct, 2),
    }
    if len(values) > 1 and spread_pct > contention_warn_percent:
        contention_warnings.append(
            f"{key} sample spread {spread_pct:.2f}% exceeded contention warn threshold "
            f"{contention_warn_percent}% (samples={values})"
        )


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

baseline_history = []
if os.path.exists(baseline_history_path):
    with open(baseline_history_path, "r", encoding="utf-8") as f:
        for line in f:
            line = line.strip()
            if not line:
                continue
            try:
                obj = json.loads(line)
            except Exception:
                continue
            if isinstance(obj, dict) and isinstance(obj.get("metrics"), dict):
                baseline_history.append(obj)

history_for_baseline = baseline_history + history

baseline_stats = {}
for key in metrics:
    values = []
    for row in history_for_baseline:
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
    "sampling": {
        "statistic": "median",
        "contention_warn_percent": contention_warn_percent,
        "warnings": contention_warnings,
    },
    "measurement_samples": sample_stats,
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
    "baseline_history_path": baseline_history_path,
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
echo "  incremental_warm_ms=$INCREMENTAL_WARM_MS samples=[$INCREMENTAL_WARM_SAMPLES] sample_p95=$INCREMENTAL_WARM_SAMPLE_P95_MS (budget=$BUDGET_INCREMENTAL_WARM_MS)"
echo "  changed_fast_ms=$CHANGED_FAST_MS samples=[$CHANGED_FAST_SAMPLES] sample_p95=$CHANGED_FAST_SAMPLE_P95_MS (budget=$BUDGET_CHANGED_FAST_MS)"
echo "  core_suite_ms=$CORE_SUITE_MS samples=[$CORE_SUITE_SAMPLES] sample_p95=$CORE_SUITE_SAMPLE_P95_MS (budget=$BUDGET_CORE_SUITE_MS)"
echo "  gcpm_lock_ms=$GCPM_LOCK_MS samples=[$GCPM_LOCK_SAMPLES] sample_p95=$GCPM_LOCK_SAMPLE_P95_MS (budget=$BUDGET_GCPM_LOCK_MS)"
echo "  gcpm_env_ms=$GCPM_ENV_MS samples=[$GCPM_ENV_SAMPLES] sample_p95=$GCPM_ENV_SAMPLE_P95_MS (budget=$BUDGET_GCPM_ENV_MS)"
echo "  gcpm_lock_stabilization spread_pct=$GCPM_LOCK_SAMPLE_SPREAD_PCT total_runs=$GCPM_LOCK_TOTAL_RUNS retries=$GCPM_LOCK_STABILIZE_RETRIES"
echo "  gcpm_env_stabilization spread_pct=$GCPM_ENV_SAMPLE_SPREAD_PCT total_runs=$GCPM_ENV_TOTAL_RUNS retries=$GCPM_ENV_STABILIZE_RETRIES"

[[ "$INCREMENTAL_WARM_MS" -le "$BUDGET_INCREMENTAL_WARM_MS" ]] || fail "warm incremental loop regression: $INCREMENTAL_WARM_MS > $BUDGET_INCREMENTAL_WARM_MS"
[[ "$CHANGED_FAST_MS" -le "$BUDGET_CHANGED_FAST_MS" ]] || fail "changed fast loop regression: $CHANGED_FAST_MS > $BUDGET_CHANGED_FAST_MS"
[[ "$CORE_SUITE_MS" -le "$BUDGET_CORE_SUITE_MS" ]] || fail "core suite regression: $CORE_SUITE_MS > $BUDGET_CORE_SUITE_MS"
[[ "$GCPM_LOCK_MS" -le "$BUDGET_GCPM_LOCK_MS" ]] || fail "gcpm lock regression: $GCPM_LOCK_MS > $BUDGET_GCPM_LOCK_MS"
[[ "$GCPM_ENV_MS" -le "$BUDGET_GCPM_ENV_MS" ]] || fail "gcpm env regression: $GCPM_ENV_MS > $BUDGET_GCPM_ENV_MS"

echo "ai-iteration-slo: ok"
