#!/usr/bin/env bash
set -euo pipefail

ROOT_DIR="$(cd "$(dirname "$0")/.." && pwd)"
cd "$ROOT_DIR"

if [[ "$#" -ne 7 ]]; then
  echo "usage: $0 <metrics-output> <metrics-history-output> <runtime-report-output> <runtime-history-output> <metrics-input> <metrics-history-input> <runtime-history-input>" >&2
  exit 2
fi

OUT="$1"
HISTORY="$2"
RUNTIME_REPORT="$3"
RUNTIME_HISTORY="$4"
METRICS_INPUT="$5"
METRICS_HISTORY_INPUT="$6"
RUNTIME_HISTORY_INPUT="$7"

source "$ROOT_DIR/scripts/lib/cargo_target_dir.sh"
genesis_configure_cargo_target_dir \
  "$ROOT_DIR" \
  "check-runtime-workload-budgets" \
  root-host

source "$ROOT_DIR/scripts/lib/perf_disk_mode.sh"
source "$ROOT_DIR/scripts/lib/profile_gate_timing.sh"

START_MS="$(genesis_profile_gate_now_ms)"

SKIP_RUN="${GENESIS_RUNTIME_WORKLOAD_SKIP_RUN:-0}"
CARGO_PROFILE="${GENESIS_PERF_CARGO_PROFILE:-selfhost-strict}"
WORKLOAD_PROFILE="${GENESIS_RUNTIME_WORKLOAD_PROFILE:-smoke}"
REQUIRE_ROADMAP_SIZES="${GENESIS_RUNTIME_WORKLOAD_REQUIRE_ROADMAP_SIZES:-0}"
DISK_STRICT_MODE="$(genesis_resolve_perf_disk_strict_mode)"
DISK_MIN_FREE_KB="${GENESIS_RUNTIME_WORKLOAD_MIN_FREE_KB:-2097152}"
RUNTIME_BASELINE_HISTORY="${GENESIS_RUNTIME_WORKLOAD_RUNTIME_BASELINE_HISTORY_OUT:-policies/perf/runtime_workload_bench_runtime_seed_history.jsonl}"
RUNTIME_BUDGET_MS="${GENESIS_RUNTIME_WORKLOAD_RUNTIME_BUDGET_MS:-300000}"
RUNTIME_MIN_HISTORY="${GENESIS_RUNTIME_WORKLOAD_RUNTIME_MIN_HISTORY:-5}"
RUNTIME_REQUIRE_MIN_HISTORY="${GENESIS_RUNTIME_WORKLOAD_RUNTIME_REQUIRE_MIN_HISTORY:-1}"

if [[ "$SKIP_RUN" != "0" && "$SKIP_RUN" != "1" ]]; then
  echo "runtime-workload-bench: GENESIS_RUNTIME_WORKLOAD_SKIP_RUN must be 0 or 1" >&2
  exit 2
fi
if [[ "$REQUIRE_ROADMAP_SIZES" != "0" && "$REQUIRE_ROADMAP_SIZES" != "1" ]]; then
  echo "runtime-workload-bench: GENESIS_RUNTIME_WORKLOAD_REQUIRE_ROADMAP_SIZES must be 0 or 1" >&2
  exit 2
fi
if [[ ! "$RUNTIME_MIN_HISTORY" =~ ^[0-9]+$ || "$RUNTIME_MIN_HISTORY" -le 0 ]]; then
  echo "runtime-workload-bench: GENESIS_RUNTIME_WORKLOAD_RUNTIME_MIN_HISTORY must be a positive integer" >&2
  exit 2
fi
if [[ "$RUNTIME_REQUIRE_MIN_HISTORY" != "0" && "$RUNTIME_REQUIRE_MIN_HISTORY" != "1" ]]; then
  echo "runtime-workload-bench: GENESIS_RUNTIME_WORKLOAD_RUNTIME_REQUIRE_MIN_HISTORY must be 0 or 1" >&2
  exit 2
fi

bash scripts/check_disk_headroom.sh \
  --path "$ROOT_DIR" \
  --context "runtime-workload-bench" \
  --min-kb "$DISK_MIN_FREE_KB" \
  --strict "$DISK_STRICT_MODE"

mkdir -p "$(dirname "$OUT")" "$(dirname "$HISTORY")"

export GENESIS_MICROBENCH_WARMUPS="${GENESIS_RUNTIME_WORKLOAD_WARMUPS:-${GENESIS_MICROBENCH_WARMUPS:-0}}"
export GENESIS_MICROBENCH_REPEATS="${GENESIS_RUNTIME_WORKLOAD_REPEATS:-${GENESIS_MICROBENCH_REPEATS:-1}}"

if [[ "$SKIP_RUN" == "1" ]]; then
  if [[ ! -f "$METRICS_INPUT" ]]; then
    echo "runtime-workload-bench: GENESIS_RUNTIME_WORKLOAD_SKIP_RUN=1 requires existing report: $METRICS_INPUT" >&2
    exit 2
  fi
  if [[ "$METRICS_INPUT" != "$OUT" ]]; then
    cp "$METRICS_INPUT" "$OUT"
  fi
  echo "runtime-workload-bench: skipping benchmark execution (GENESIS_RUNTIME_WORKLOAD_SKIP_RUN=1)"
else
  echo "runtime-workload-bench: running profile=$WORKLOAD_PROFILE warmups=$GENESIS_MICROBENCH_WARMUPS repeats=$GENESIS_MICROBENCH_REPEATS"
  GENESIS_RUNTIME_WORKLOAD_PROFILE="$WORKLOAD_PROFILE" \
    GENESIS_RUNTIME_MICROBENCH_PROFILE="$CARGO_PROFILE" \
    GENESIS_RUNTIME_MICROBENCH_BUILD_MODE="release-equivalent" \
    cargo run --profile "$CARGO_PROFILE" -p gc_runtime_bench -- --mode workloads --out "$OUT"
fi

echo "runtime-workload-bench: metrics report=$OUT"

if [[ "$METRICS_HISTORY_INPUT" != "$HISTORY" ]]; then
  if [[ -f "$METRICS_HISTORY_INPUT" ]]; then
    cp "$METRICS_HISTORY_INPUT" "$HISTORY"
  else
    : >"$HISTORY"
  fi
fi

python3 - "$OUT" "$HISTORY" "$REQUIRE_ROADMAP_SIZES" <<'PY'
import datetime as dt
import json
import pathlib
import sys

report_path = pathlib.Path(sys.argv[1])
history_path = pathlib.Path(sys.argv[2])
require_roadmap_sizes = sys.argv[3] == "1"

doc = json.loads(report_path.read_text(encoding="utf-8"))
if doc.get("kind") != "genesis/runtime-workload-bench-v0.1":
    raise SystemExit("runtime-workload-bench: unexpected report kind")
if doc.get("bench_mode") != "workloads":
    raise SystemExit("runtime-workload-bench: report bench_mode must be workloads")

required_metrics = [
    "fib_ms",
    "vec_build_ms",
    "map_build_ms",
    "str_concat_ms",
    "selfhost_parse_ms",
    "dispatch_ms",
]
metrics = doc.get("metrics")
budgets = doc.get("budgets")
if not isinstance(metrics, dict) or not isinstance(budgets, dict):
    raise SystemExit("runtime-workload-bench: report missing metrics/budgets maps")
for key in required_metrics:
    if key not in metrics:
        raise SystemExit(f"runtime-workload-bench: missing metrics.{key}")
    if key not in budgets:
        raise SystemExit(f"runtime-workload-bench: missing budgets.{key}")
    if int(metrics[key]) > int(budgets[key]):
        raise SystemExit(
            f"runtime-workload-bench: budget failure {key}={metrics[key]} > {budgets[key]}"
        )

sizes = doc.get("sizes")
roadmap_sizes = doc.get("roadmap_sizes")
corpus = doc.get("selfhost_parse_corpus")
roadmap_corpus = doc.get("roadmap_selfhost_parse_corpus")
if not isinstance(sizes, dict) or not isinstance(roadmap_sizes, dict):
    raise SystemExit("runtime-workload-bench: missing sizes/roadmap_sizes")
if not isinstance(corpus, list) or not corpus:
    raise SystemExit("runtime-workload-bench: missing selfhost_parse_corpus")
if not isinstance(roadmap_corpus, list) or not roadmap_corpus:
    raise SystemExit("runtime-workload-bench: missing roadmap_selfhost_parse_corpus")
if any(str(item).startswith("/") or ".." in pathlib.PurePosixPath(str(item)).parts for item in corpus):
    raise SystemExit("runtime-workload-bench: corpus paths must be workspace-relative")
if require_roadmap_sizes:
    if sizes != roadmap_sizes:
        raise SystemExit("runtime-workload-bench: roadmap size requirement failed")
    if corpus != roadmap_corpus:
        raise SystemExit("runtime-workload-bench: roadmap corpus requirement failed")

row = dict(doc)
row["timestamp_utc"] = dt.datetime.now(dt.timezone.utc).isoformat(timespec="seconds")
history_path.parent.mkdir(parents=True, exist_ok=True)
with history_path.open("a", encoding="utf-8") as fh:
    fh.write(json.dumps(row, sort_keys=True) + "\n")
print(f"runtime-workload-bench: appended history {history_path}")
PY

EFFECTIVE_BASELINE_HISTORY="$RUNTIME_BASELINE_HISTORY"
MERGED_BASELINE_HISTORY=""
if [[ "$RUNTIME_HISTORY_INPUT" != "$RUNTIME_HISTORY" && -f "$RUNTIME_HISTORY_INPUT" ]]; then
  MERGED_BASELINE_HISTORY="$(mktemp)"
  cat "$RUNTIME_BASELINE_HISTORY" "$RUNTIME_HISTORY_INPUT" >"$MERGED_BASELINE_HISTORY"
  EFFECTIVE_BASELINE_HISTORY="$MERGED_BASELINE_HISTORY"
fi
cleanup_baseline() {
  [[ -z "$MERGED_BASELINE_HISTORY" ]] || rm -f "$MERGED_BASELINE_HISTORY"
}
trap cleanup_baseline EXIT

genesis_profile_gate_emit_runtime_report \
  "runtime-workload-bench" \
  "genesis/runtime-workload-bench-runtime-v0.1" \
  "$RUNTIME_REPORT" \
  "$RUNTIME_HISTORY" \
  "$START_MS" \
  "$RUNTIME_BUDGET_MS" \
  "$RUNTIME_MIN_HISTORY" \
  "{\"metrics_report\":\"$OUT\",\"metrics_history\":\"$HISTORY\",\"build_profile\":\"$CARGO_PROFILE\",\"workload_profile\":\"$WORKLOAD_PROFILE\"}" \
  "$WORKLOAD_PROFILE" \
  "$EFFECTIVE_BASELINE_HISTORY" \
  "$RUNTIME_REQUIRE_MIN_HISTORY"
