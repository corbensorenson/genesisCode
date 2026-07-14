#!/usr/bin/env bash
set -euo pipefail

ROOT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
cd "$ROOT_DIR"

if [[ "$#" -ne 7 ]]; then
  echo "usage: $0 <metrics-output> <metrics-history-output> <runtime-report-output> <runtime-history-output> <metrics-history-input> <runtime-history-input> <runtime-baseline-input>" >&2
  exit 2
fi

REPORT_OUT="$1"
METRICS_HISTORY="$2"
RUNTIME_REPORT="$3"
RUNTIME_HISTORY="$4"
METRICS_HISTORY_INPUT="$5"
RUNTIME_HISTORY_INPUT="$6"
RUNTIME_BASELINE_HISTORY="$7"

source "$ROOT_DIR/scripts/lib/cargo_target_dir.sh"
genesis_configure_cargo_target_dir \
  "$ROOT_DIR" \
  "check-large-workspace-agent-perf" \
  root-host

source "$ROOT_DIR/scripts/lib/measure.sh"
source "$ROOT_DIR/scripts/lib/gcpm_caps_fixture.sh"
source "$ROOT_DIR/scripts/lib/perf_disk_mode.sh"
source "$ROOT_DIR/scripts/lib/profile_gate_timing.sh"

MODULE_COUNT="${GENESIS_LARGE_WORKSPACE_MODULE_COUNT:-10000}"
BUILD_TARGET="${GENESIS_LARGE_WORKSPACE_BUILD_TARGET:-service-runtime}"
STEP_LIMIT="${GENESIS_LARGE_WORKSPACE_STEP_LIMIT:-1000000000}"
MEASURE_WARMUPS="${GENESIS_LARGE_WORKSPACE_WARMUPS:-0}"
MEASURE_REPEATS="${GENESIS_LARGE_WORKSPACE_REPEATS:-1}"
CARGO_PROFILE="${GENESIS_PERF_CARGO_PROFILE:-selfhost-strict}"
DISK_STRICT_MODE="$(genesis_resolve_perf_disk_strict_mode)"
DISK_MIN_FREE_KB="${GENESIS_LARGE_WORKSPACE_MIN_FREE_KB:-3145728}"

BUDGET_GCPM_LOCK_MS="${GENESIS_LARGE_WORKSPACE_BUDGET_GCPM_LOCK_MS:-90000}"
BUDGET_GCPM_BUILD_MS="${GENESIS_LARGE_WORKSPACE_BUDGET_GCPM_BUILD_MS:-300000}"
BUDGET_GCPM_TEST_MS="${GENESIS_LARGE_WORKSPACE_BUDGET_GCPM_TEST_MS:-240000}"
BUDGET_SELFHOST_REFRESH_MS="${GENESIS_LARGE_WORKSPACE_BUDGET_SELFHOST_REFRESH_MS:-300000}"

RUNTIME_BUDGET_MS="${GENESIS_LARGE_WORKSPACE_RUNTIME_BUDGET_MS:-900000}"
RUNTIME_MIN_HISTORY="${GENESIS_LARGE_WORKSPACE_RUNTIME_MIN_HISTORY:-5}"
RUNTIME_REQUIRE_MIN_HISTORY="${GENESIS_LARGE_WORKSPACE_RUNTIME_REQUIRE_MIN_HISTORY:-0}"
RUNTIME_HISTORY_SCOPE_KEY="${GENESIS_LARGE_WORKSPACE_RUNTIME_HISTORY_SCOPE_KEY:-large-workspace-v1}"
METRICS_MIN_HISTORY="${GENESIS_LARGE_WORKSPACE_METRICS_MIN_HISTORY:-5}"

fail() {
  echo "large-workspace-agent-perf: $*" >&2
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

profile_target_dir() {
  case "$1" in
    release) echo "release" ;;
    dev|test) echo "debug" ;;
    *) echo "$1" ;;
  esac
}

require_positive_int "GENESIS_LARGE_WORKSPACE_MODULE_COUNT" "$MODULE_COUNT"
require_positive_int "GENESIS_LARGE_WORKSPACE_STEP_LIMIT" "$STEP_LIMIT"
require_non_negative_int "GENESIS_LARGE_WORKSPACE_WARMUPS" "$MEASURE_WARMUPS"
require_positive_int "GENESIS_LARGE_WORKSPACE_REPEATS" "$MEASURE_REPEATS"
require_positive_int "GENESIS_LARGE_WORKSPACE_BUDGET_GCPM_LOCK_MS" "$BUDGET_GCPM_LOCK_MS"
require_positive_int "GENESIS_LARGE_WORKSPACE_BUDGET_GCPM_BUILD_MS" "$BUDGET_GCPM_BUILD_MS"
require_positive_int "GENESIS_LARGE_WORKSPACE_BUDGET_GCPM_TEST_MS" "$BUDGET_GCPM_TEST_MS"
require_positive_int "GENESIS_LARGE_WORKSPACE_BUDGET_SELFHOST_REFRESH_MS" "$BUDGET_SELFHOST_REFRESH_MS"
require_positive_int "GENESIS_LARGE_WORKSPACE_RUNTIME_BUDGET_MS" "$RUNTIME_BUDGET_MS"
require_positive_int "GENESIS_LARGE_WORKSPACE_RUNTIME_MIN_HISTORY" "$RUNTIME_MIN_HISTORY"
require_positive_int "GENESIS_LARGE_WORKSPACE_METRICS_MIN_HISTORY" "$METRICS_MIN_HISTORY"
if [[ "$RUNTIME_REQUIRE_MIN_HISTORY" != "0" && "$RUNTIME_REQUIRE_MIN_HISTORY" != "1" ]]; then
  fail "GENESIS_LARGE_WORKSPACE_RUNTIME_REQUIRE_MIN_HISTORY must be 0 or 1"
fi

bash scripts/check_disk_headroom.sh \
  --path "$ROOT_DIR" \
  --context "large-workspace-agent-perf" \
  --min-kb "$DISK_MIN_FREE_KB" \
  --strict "$DISK_STRICT_MODE"

TARGET_PROFILE_DIR="$(profile_target_dir "$CARGO_PROFILE")"
GENESIS_BIN="$CARGO_TARGET_DIR/$TARGET_PROFILE_DIR/genesis"

echo "large-workspace-agent-perf: preparing genesis binary"
cargo build -p gc_cli --profile "$CARGO_PROFILE" >/dev/null
[[ -x "$GENESIS_BIN" ]] || fail "unable to locate genesis binary at $GENESIS_BIN"

TMP_DIR="$(mktemp -d)"
cleanup() {
  rm -rf "$TMP_DIR"
}
trap cleanup EXIT

WORKSPACE="$TMP_DIR/large-workspace"
mkdir -p "$WORKSPACE"
write_gcpm_low_caps_fixture "$WORKSPACE/caps.toml"

echo "large-workspace-agent-perf: generating workspace modules=$MODULE_COUNT"
python3 - "$WORKSPACE" "$MODULE_COUNT" "$STEP_LIMIT" <<'PY'
import pathlib
import sys

workspace = pathlib.Path(sys.argv[1])
module_count = int(sys.argv[2])
step_limit = int(sys.argv[3])
src = workspace / "src"
src.mkdir(parents=True, exist_ok=True)

pkg_lines = [
    'name = "large_workspace"',
    'version = "0.1.0"',
    "obligations = []",
    "dependencies = []",
    "",
    "[limits]",
    "allow_unlimited = true",
    f"step_limit = {step_limit}",
    "",
]

for idx in range(module_count):
    mod_name = f"m{idx:05d}"
    rel = f"src/{mod_name}.gc"
    file_path = workspace / rel
    file_path.write_text(
        f"(def large/{mod_name} {idx})\nlarge/{mod_name}\n",
        encoding="utf-8",
    )
    pkg_lines.append("[[modules]]")
    pkg_lines.append(f'path = "{rel}"')
    pkg_lines.append("")

(workspace / "package.toml").write_text("\n".join(pkg_lines), encoding="utf-8")
PY

BASE_TOOLCHAIN="$TMP_DIR/toolchain_base.gc"
echo "large-workspace-agent-perf: building baseline selfhost artifact"
"$GENESIS_BIN" selfhost-artifact --out "$BASE_TOOLCHAIN" >/dev/null

run_gcpm() {
  (
    cd "$WORKSPACE"
    "$GENESIS_BIN" --step-limit "$STEP_LIMIT" --selfhost-artifact "$BASE_TOOLCHAIN" gcpm --caps "$WORKSPACE/caps.toml" "$@"
  )
}

run_gcpm new --workspace "large-workspace" --policy "policy:default-v0.1" --registry-default "gen://registry" >/dev/null
"$GENESIS_BIN" --step-limit "$STEP_LIMIT" --selfhost-artifact "$BASE_TOOLCHAIN" pack --pkg "$WORKSPACE/package.toml" >/dev/null

LANE_START_MS="$(genesis_profile_gate_now_ms)"

echo "large-workspace-agent-perf: measuring gcpm lock"
genesis_measure_best_of_ms lock_ms "$MEASURE_WARMUPS" "$MEASURE_REPEATS" run_gcpm lock --strict
GCPM_LOCK_MS="$MEASURE_LAST_MS"

echo "large-workspace-agent-perf: measuring gcpm build target=$BUILD_TARGET"
genesis_measure_best_of_ms \
  build_ms \
  "$MEASURE_WARMUPS" \
  "$MEASURE_REPEATS" \
  run_gcpm build --pkg "$WORKSPACE/package.toml" --target "$BUILD_TARGET" --out-dir "$WORKSPACE/.genesis/build-targets"
GCPM_BUILD_MS="$MEASURE_LAST_MS"

echo "large-workspace-agent-perf: measuring gcpm test"
genesis_measure_best_of_ms test_ms "$MEASURE_WARMUPS" "$MEASURE_REPEATS" run_gcpm test --pkg "$WORKSPACE/package.toml"
GCPM_TEST_MS="$MEASURE_LAST_MS"

REFRESH_ARTIFACT="$TMP_DIR/toolchain_refresh.gc"
echo "large-workspace-agent-perf: measuring selfhost-artifact refresh"
genesis_measure_best_of_ms \
  selfhost_refresh_ms \
  "$MEASURE_WARMUPS" \
  "$MEASURE_REPEATS" \
  "$GENESIS_BIN" selfhost-artifact --out "$REFRESH_ARTIFACT"
SELFHOST_REFRESH_MS="$MEASURE_LAST_MS"

[[ -f "$REFRESH_ARTIFACT" ]] || fail "selfhost artifact refresh did not produce $REFRESH_ARTIFACT"

python3 - \
  "$REPORT_OUT" \
  "$METRICS_HISTORY" \
  "$METRICS_HISTORY_INPUT" \
  "$MODULE_COUNT" \
  "$BUILD_TARGET" \
  "$CARGO_PROFILE" \
  "$DISK_STRICT_MODE" \
  "$MEASURE_WARMUPS" \
  "$MEASURE_REPEATS" \
  "$GCPM_LOCK_MS" \
  "$GCPM_BUILD_MS" \
  "$GCPM_TEST_MS" \
  "$SELFHOST_REFRESH_MS" \
  "$BUDGET_GCPM_LOCK_MS" \
  "$BUDGET_GCPM_BUILD_MS" \
  "$BUDGET_GCPM_TEST_MS" \
  "$BUDGET_SELFHOST_REFRESH_MS" \
  "$METRICS_MIN_HISTORY" <<'PY'
import datetime as dt
import json
import math
import pathlib
import sys

(
    report_out,
    metrics_history,
    metrics_history_input,
    module_count,
    build_target,
    build_profile,
    disk_strict_mode,
    measure_warmups,
    measure_repeats,
    gcpm_lock_ms,
    gcpm_build_ms,
    gcpm_test_ms,
    selfhost_refresh_ms,
    budget_lock_ms,
    budget_build_ms,
    budget_test_ms,
    budget_refresh_ms,
    metrics_min_history,
) = sys.argv[1:]

module_count = int(module_count)
measure_warmups = int(measure_warmups)
measure_repeats = int(measure_repeats)
gcpm_lock_ms = int(gcpm_lock_ms)
gcpm_build_ms = int(gcpm_build_ms)
gcpm_test_ms = int(gcpm_test_ms)
selfhost_refresh_ms = int(selfhost_refresh_ms)
budget_lock_ms = int(budget_lock_ms)
budget_build_ms = int(budget_build_ms)
budget_test_ms = int(budget_test_ms)
budget_refresh_ms = int(budget_refresh_ms)
metrics_min_history = int(metrics_min_history)

report_path = pathlib.Path(report_out)
history_path = pathlib.Path(metrics_history)
history_input_path = pathlib.Path(metrics_history_input)
kind = "genesis/large-workspace-agent-perf-v0.1"
elapsed_total_ms = gcpm_lock_ms + gcpm_build_ms + gcpm_test_ms + selfhost_refresh_ms
budget_total_ms = budget_lock_ms + budget_build_ms + budget_test_ms + budget_refresh_ms
timestamp_utc = dt.datetime.now(dt.timezone.utc).replace(microsecond=0).isoformat()

checks = {
    "gcpm_lock_ms": gcpm_lock_ms <= budget_lock_ms,
    "gcpm_build_ms": gcpm_build_ms <= budget_build_ms,
    "gcpm_test_ms": gcpm_test_ms <= budget_test_ms,
    "selfhost_artifact_refresh_ms": selfhost_refresh_ms <= budget_refresh_ms,
}
total_checks = len(checks)
passed_checks = sum(1 for ok in checks.values() if ok)
score_percent = round((passed_checks / total_checks) * 100.0, 2)

history_rows = []
if history_input_path.is_file():
    for raw in history_input_path.read_text(encoding="utf-8").splitlines():
        line = raw.strip()
        if not line:
            continue
        try:
            row = json.loads(line)
        except json.JSONDecodeError:
            continue
        if (
            isinstance(row, dict)
            and row.get("kind") == kind
            and row.get("build_target") == build_target
            and row.get("build_profile") == build_profile
            and int(row.get("module_count", -1)) == module_count
            and isinstance(row.get("elapsed_total_ms"), int)
            and int(row.get("budget_total_ms", -1)) == budget_total_ms
        ):
            history_rows.append(row)

history_path.parent.mkdir(parents=True, exist_ok=True)
history_entry = {
    "kind": kind,
    "timestamp_utc": timestamp_utc,
    "module_count": module_count,
    "build_target": build_target,
    "build_profile": build_profile,
    "elapsed_total_ms": elapsed_total_ms,
    "budget_total_ms": budget_total_ms,
    "score_percent": score_percent,
    "ok": passed_checks == total_checks,
}
with history_path.open("a", encoding="utf-8") as fh:
    fh.write(json.dumps(history_entry, sort_keys=True) + "\n")

elapsed_samples = sorted([int(row["elapsed_total_ms"]) for row in history_rows] + [elapsed_total_ms])
history_samples = len(elapsed_samples)
p95_idx = max(0, math.ceil(0.95 * history_samples) - 1)
history_p95_ms = elapsed_samples[p95_idx]
history_p95_enforced = history_samples >= metrics_min_history
history_p95_ok = (not history_p95_enforced) or (history_p95_ms <= budget_total_ms)

fail_reasons = []
for metric, ok in checks.items():
    if not ok:
        fail_reasons.append(f"{metric}-budget")
if not history_p95_ok:
    fail_reasons.append("history-p95-budget")

doc = {
    "kind": kind,
    "ok": (passed_checks == total_checks) and history_p95_ok,
    "score_percent": score_percent,
    "fail_reasons": fail_reasons,
    "timestamp_utc": timestamp_utc,
    "module_count": module_count,
    "build_target": build_target,
    "build_profile": build_profile,
    "disk_strict_mode": disk_strict_mode,
    "measure_warmups": measure_warmups,
    "measure_repeats": measure_repeats,
    "gcpm_lock_ms": gcpm_lock_ms,
    "gcpm_build_ms": gcpm_build_ms,
    "gcpm_test_ms": gcpm_test_ms,
    "selfhost_artifact_refresh_ms": selfhost_refresh_ms,
    "elapsed_total_ms": elapsed_total_ms,
    "budget_total_ms": budget_total_ms,
    "history_samples": history_samples,
    "history_p95_ms": history_p95_ms,
    "history_p95_enforced": history_p95_enforced,
    "history_p95_ok": history_p95_ok,
    "history_file": "large-workspace-agent-perf-history",
    "budgets": {
        "gcpm_lock_ms": budget_lock_ms,
        "gcpm_build_ms": budget_build_ms,
        "gcpm_test_ms": budget_test_ms,
        "selfhost_artifact_refresh_ms": budget_refresh_ms,
    },
}
if history_rows:
    previous_elapsed = int(history_rows[-1]["elapsed_total_ms"])
    doc["previous_elapsed_total_ms"] = previous_elapsed
    doc["elapsed_total_delta_ms"] = elapsed_total_ms - previous_elapsed
    doc["wall_time_trend_ms"] = doc["elapsed_total_delta_ms"]

import hashlib
doc["history_sha256"] = hashlib.sha256(history_path.read_bytes()).hexdigest()
if history_input_path.is_file():
    doc["history_input_sha256"] = hashlib.sha256(history_input_path.read_bytes()).hexdigest()

report_path.parent.mkdir(parents=True, exist_ok=True)
report_path.write_text(json.dumps(doc, indent=2, sort_keys=True) + "\n", encoding="utf-8")
PY

echo "large-workspace-agent-perf: metrics"
echo "  module_count=$MODULE_COUNT"
echo "  gcpm_lock_ms=$GCPM_LOCK_MS budget=$BUDGET_GCPM_LOCK_MS"
echo "  gcpm_build_ms=$GCPM_BUILD_MS budget=$BUDGET_GCPM_BUILD_MS"
echo "  gcpm_test_ms=$GCPM_TEST_MS budget=$BUDGET_GCPM_TEST_MS"
echo "  selfhost_artifact_refresh_ms=$SELFHOST_REFRESH_MS budget=$BUDGET_SELFHOST_REFRESH_MS"

[[ "$GCPM_LOCK_MS" -le "$BUDGET_GCPM_LOCK_MS" ]] || fail "gcpm lock regression: $GCPM_LOCK_MS > $BUDGET_GCPM_LOCK_MS"
[[ "$GCPM_BUILD_MS" -le "$BUDGET_GCPM_BUILD_MS" ]] || fail "gcpm build regression: $GCPM_BUILD_MS > $BUDGET_GCPM_BUILD_MS"
[[ "$GCPM_TEST_MS" -le "$BUDGET_GCPM_TEST_MS" ]] || fail "gcpm test regression: $GCPM_TEST_MS > $BUDGET_GCPM_TEST_MS"
[[ "$SELFHOST_REFRESH_MS" -le "$BUDGET_SELFHOST_REFRESH_MS" ]] || fail "selfhost-artifact refresh regression: $SELFHOST_REFRESH_MS > $BUDGET_SELFHOST_REFRESH_MS"

METRICS_REPORT_SHA256="$(python3 - "$REPORT_OUT" <<'PY'
import hashlib
import pathlib
import sys
print(hashlib.sha256(pathlib.Path(sys.argv[1]).read_bytes()).hexdigest())
PY
)"

EFFECTIVE_RUNTIME_BASELINE="$RUNTIME_BASELINE_HISTORY"
if [[ "$RUNTIME_HISTORY_INPUT" != "$RUNTIME_HISTORY" && -f "$RUNTIME_HISTORY_INPUT" ]]; then
  EFFECTIVE_RUNTIME_BASELINE="$TMP_DIR/runtime_history_baseline.jsonl"
  if [[ -n "$RUNTIME_BASELINE_HISTORY" && "$RUNTIME_BASELINE_HISTORY" != "$RUNTIME_HISTORY_INPUT" ]]; then
    cat "$RUNTIME_BASELINE_HISTORY" "$RUNTIME_HISTORY_INPUT" >"$EFFECTIVE_RUNTIME_BASELINE"
  else
    cp "$RUNTIME_HISTORY_INPUT" "$EFFECTIVE_RUNTIME_BASELINE"
  fi
fi

set +e
genesis_profile_gate_emit_runtime_report \
  "large-workspace-agent-perf" \
  "genesis/large-workspace-agent-runtime-v0.1" \
  "$RUNTIME_REPORT" \
  "$RUNTIME_HISTORY" \
  "$LANE_START_MS" \
  "$RUNTIME_BUDGET_MS" \
  "$RUNTIME_MIN_HISTORY" \
  "{\"metrics_report\":\"large-workspace-agent-perf\",\"metrics_report_sha256\":\"$METRICS_REPORT_SHA256\",\"module_count\":$MODULE_COUNT,\"build_target\":\"$BUILD_TARGET\"}" \
  "$RUNTIME_HISTORY_SCOPE_KEY" \
  "$EFFECTIVE_RUNTIME_BASELINE" \
  "$RUNTIME_REQUIRE_MIN_HISTORY"
runtime_status=$?
set -e

if [[ -f "$RUNTIME_REPORT" ]]; then
  python3 - "$RUNTIME_REPORT" "$RUNTIME_HISTORY" "$EFFECTIVE_RUNTIME_BASELINE" <<'PY'
import hashlib
import json
import pathlib
import sys

report_path = pathlib.Path(sys.argv[1])
history_path = pathlib.Path(sys.argv[2])
baseline_raw = sys.argv[3]
doc = json.loads(report_path.read_text(encoding="utf-8"))
doc["history_file"] = "large-workspace-agent-runtime-history"
doc["history_sha256"] = hashlib.sha256(history_path.read_bytes()).hexdigest()
if baseline_raw:
    baseline_path = pathlib.Path(baseline_raw)
    doc["baseline_history_file"] = "large-workspace-agent-runtime-baseline"
    doc["baseline_history_sha256"] = hashlib.sha256(baseline_path.read_bytes()).hexdigest()
report_path.write_text(json.dumps(doc, indent=2, sort_keys=True) + "\n", encoding="utf-8")
PY
fi

if [[ "$runtime_status" -ne 0 ]]; then
  exit "$runtime_status"
fi

echo "large-workspace-agent-perf: ok"
