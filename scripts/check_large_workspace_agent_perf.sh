#!/usr/bin/env bash
set -euo pipefail

ROOT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
cd "$ROOT_DIR"

source "$ROOT_DIR/scripts/lib/cargo_target_dir.sh"
genesis_configure_cargo_target_dir \
  "$ROOT_DIR" \
  "check-large-workspace-agent-perf" \
  ".genesis/build/cargo" \
  "GENESIS_CHECK_LARGE_WORKSPACE_AGENT_PERF_CARGO_TARGET_DIR"

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

REPORT_OUT="${GENESIS_LARGE_WORKSPACE_REPORT_OUT:-.genesis/perf/large_workspace_agent_perf_report.json}"
RUNTIME_REPORT="${GENESIS_LARGE_WORKSPACE_RUNTIME_REPORT:-.genesis/perf/large_workspace_agent_runtime_report.json}"
RUNTIME_HISTORY="${GENESIS_LARGE_WORKSPACE_RUNTIME_HISTORY:-.genesis/perf/large_workspace_agent_runtime_history.jsonl}"
RUNTIME_BASELINE_HISTORY="${GENESIS_LARGE_WORKSPACE_RUNTIME_BASELINE_HISTORY:-}"
RUNTIME_BUDGET_MS="${GENESIS_LARGE_WORKSPACE_RUNTIME_BUDGET_MS:-900000}"
RUNTIME_MIN_HISTORY="${GENESIS_LARGE_WORKSPACE_RUNTIME_MIN_HISTORY:-5}"
RUNTIME_REQUIRE_MIN_HISTORY="${GENESIS_LARGE_WORKSPACE_RUNTIME_REQUIRE_MIN_HISTORY:-0}"
RUNTIME_HISTORY_SCOPE_KEY="${GENESIS_LARGE_WORKSPACE_RUNTIME_HISTORY_SCOPE_KEY:-large-workspace-v1}"

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

mkdir -p "$ROOT_DIR/.genesis/perf"
cat > "$REPORT_OUT" <<EOF
{
  "kind": "genesis/large-workspace-agent-perf-v0.1",
  "ok": true,
  "module_count": $MODULE_COUNT,
  "build_target": "$BUILD_TARGET",
  "build_profile": "$CARGO_PROFILE",
  "disk_strict_mode": "$DISK_STRICT_MODE",
  "measure_warmups": $MEASURE_WARMUPS,
  "measure_repeats": $MEASURE_REPEATS,
  "gcpm_lock_ms": $GCPM_LOCK_MS,
  "gcpm_build_ms": $GCPM_BUILD_MS,
  "gcpm_test_ms": $GCPM_TEST_MS,
  "selfhost_artifact_refresh_ms": $SELFHOST_REFRESH_MS,
  "budgets": {
    "gcpm_lock_ms": $BUDGET_GCPM_LOCK_MS,
    "gcpm_build_ms": $BUDGET_GCPM_BUILD_MS,
    "gcpm_test_ms": $BUDGET_GCPM_TEST_MS,
    "selfhost_artifact_refresh_ms": $BUDGET_SELFHOST_REFRESH_MS
  }
}
EOF

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

genesis_profile_gate_emit_runtime_report \
  "large-workspace-agent-perf" \
  "genesis/large-workspace-agent-runtime-v0.1" \
  "$RUNTIME_REPORT" \
  "$RUNTIME_HISTORY" \
  "$LANE_START_MS" \
  "$RUNTIME_BUDGET_MS" \
  "$RUNTIME_MIN_HISTORY" \
  "{\"metrics_report\":\"$REPORT_OUT\",\"module_count\":$MODULE_COUNT,\"build_target\":\"$BUILD_TARGET\"}" \
  "$RUNTIME_HISTORY_SCOPE_KEY" \
  "$RUNTIME_BASELINE_HISTORY" \
  "$RUNTIME_REQUIRE_MIN_HISTORY"

echo "large-workspace-agent-perf: ok"
