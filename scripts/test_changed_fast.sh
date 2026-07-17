#!/usr/bin/env bash
set -euo pipefail

ROOT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
cd "$ROOT_DIR"

source "$ROOT_DIR/scripts/lib/cargo_target_dir.sh"
genesis_configure_cargo_target_dir \
  "$ROOT_DIR" \
  "test-changed-fast" \
  root-host

BASE_REF="${GENESIS_CHANGED_BASE:-}"
RUNNER="${GENESIS_TEST_CHANGED_RUNNER:-auto}" # auto|cargo|nextest
REPORT_PATH=""
HISTORY_PATH=""
BUDGET_MS="${GENESIS_TEST_CHANGED_BUDGET_MS:-120000}" # 2 minutes
FALLBACK_BUDGET_MS="${GENESIS_TEST_CHANGED_FALLBACK_BUDGET_MS:-480000}" # GB-3: 8 minutes
MIN_HISTORY="${GENESIS_TEST_CHANGED_MIN_HISTORY:-5}"
FULL_MODE_THRESHOLD="${GENESIS_TEST_CHANGED_FULL_THRESHOLD:-120}"
STRICT_DISK_MODE="${GENESIS_TEST_CHANGED_STRICT_DISK:-auto}"
CHANGED_FILES_OVERRIDE="${GENESIS_TEST_CHANGED_FILES_OVERRIDE:-}"
DRY_RUN=0
if [[ "${GENESIS_TEST_CHANGED_BUDGET_MS+x}" == "x" ]]; then
  BUDGET_EXPLICIT=1
else
  BUDGET_EXPLICIT=0
fi
OUTPUT_TMP_DIR=""
IMPACT_TMP_DIR=""

cleanup() {
  if [[ -n "$OUTPUT_TMP_DIR" ]]; then
    rm -rf "$OUTPUT_TMP_DIR"
  fi
  if [[ -n "$IMPACT_TMP_DIR" ]]; then
    rm -rf "$IMPACT_TMP_DIR"
  fi
}
trap cleanup EXIT

usage() {
  cat <<'EOF'
Usage: scripts/test_changed_fast.sh [options]

Options:
  --base <rev>         diff base revision (default: merge-base with origin/main or HEAD~1)
  --runner <name>      auto|cargo|nextest (default: auto)
  --report <path>      explicit metrics report path (must be paired with --history)
  --history <path>     explicit metrics history path (must be paired with --report)
  --budget-ms <N>      max allowed elapsed ms for this run (default: 120000;
                       profile fallback: 480000 unless explicitly overridden)
  --min-history <N>    samples required before enforcing history P95 (default: 5)
  --strict-disk <mode> pass through to check_disk_headroom strict mode (auto|0|1)
  --changed-files-from <path>
                       override changed-file detection using newline-delimited file list
                       (test-only override; equivalent env: GENESIS_TEST_CHANGED_FILES_OVERRIDE)
  --dry-run            print selected commands without executing
  -h, --help           show this help
EOF
}

while [[ $# -gt 0 ]]; do
  case "$1" in
    --base)
      BASE_REF="${2:-}"
      shift 2
      ;;
    --runner)
      RUNNER="${2:-}"
      shift 2
      ;;
    --report)
      REPORT_PATH="${2:-}"
      shift 2
      ;;
    --history)
      HISTORY_PATH="${2:-}"
      shift 2
      ;;
    --budget-ms)
      BUDGET_MS="${2:-}"
      BUDGET_EXPLICIT=1
      shift 2
      ;;
    --min-history)
      MIN_HISTORY="${2:-}"
      shift 2
      ;;
    --strict-disk)
      STRICT_DISK_MODE="${2:-}"
      shift 2
      ;;
    --changed-files-from)
      [[ -f "${2:-}" ]] || {
        echo "test-changed-fast: --changed-files-from path not found: ${2:-}" >&2
        exit 2
      }
      CHANGED_FILES_OVERRIDE="$(cat "${2:-}")"
      shift 2
      ;;
    --dry-run)
      DRY_RUN=1
      shift
      ;;
    -h|--help)
      usage
      exit 0
      ;;
    *)
      echo "test-changed-fast: unknown argument: $1" >&2
      usage >&2
      exit 2
      ;;
  esac
done

[[ "$RUNNER" == "auto" || "$RUNNER" == "cargo" || "$RUNNER" == "nextest" ]] || {
  echo "test-changed-fast: --runner must be one of auto|cargo|nextest" >&2
  exit 2
}
[[ "$STRICT_DISK_MODE" == "auto" || "$STRICT_DISK_MODE" == "0" || "$STRICT_DISK_MODE" == "1" ]] || {
  echo "test-changed-fast: --strict-disk must be auto, 0, or 1" >&2
  exit 2
}
[[ "$BUDGET_MS" =~ ^[0-9]+$ ]] || { echo "test-changed-fast: --budget-ms must be numeric" >&2; exit 2; }
[[ "$FALLBACK_BUDGET_MS" =~ ^[0-9]+$ ]] || { echo "test-changed-fast: GENESIS_TEST_CHANGED_FALLBACK_BUDGET_MS must be numeric" >&2; exit 2; }
[[ "$MIN_HISTORY" =~ ^[0-9]+$ ]] || { echo "test-changed-fast: --min-history must be numeric" >&2; exit 2; }
[[ "$FULL_MODE_THRESHOLD" =~ ^[0-9]+$ ]] || { echo "test-changed-fast: GENESIS_TEST_CHANGED_FULL_THRESHOLD must be numeric" >&2; exit 2; }

if [[ -n "$REPORT_PATH" && -z "$HISTORY_PATH" ]] || [[ -z "$REPORT_PATH" && -n "$HISTORY_PATH" ]]; then
  echo "test-changed-fast: --report and --history must be provided together" >&2
  exit 2
fi
if [[ -z "$REPORT_PATH" ]]; then
  OUTPUT_TMP_DIR="$(mktemp -d)"
  REPORT_PATH="$OUTPUT_TMP_DIR/test_changed_fast_metrics.json"
  HISTORY_PATH="$OUTPUT_TMP_DIR/test_changed_fast_history.jsonl"
  REPORT_DISPLAY="temporary"
else
  REPORT_DISPLAY="$REPORT_PATH"
fi

# GB-2 covers the complete changed-file gate, including impact planning and disk preflight.
GENESIS_CHANGED_GATE_START_NS="$(python3 - <<'PY'
import time
print(time.time_ns())
PY
)"
generated_target_bytes() {
  if [[ ! -e "$CARGO_TARGET_DIR" ]]; then
    printf '0\n'
    return
  fi
  # `du` reports allocated KiB and avoids a slow Python stat walk over Cargo's
  # high-cardinality cache. This loop owns exactly one content-addressed target.
  du -sk "$CARGO_TARGET_DIR" | awk '{ print $1 * 1024 }'
}
GENESIS_CHANGED_GATE_START_GENERATED_BYTES="$(generated_target_bytes)"
GENESIS_CHANGED_GATE_DISK_BUDGET_BYTES=1073741824
GENESIS_CHANGED_GATE_FALLBACK_DISK_BUDGET_BYTES=3221225472
export CARGO_NET_OFFLINE=true

bash scripts/check_disk_headroom.sh --path "$ROOT_DIR" --context "test-changed-fast" --strict "$STRICT_DISK_MODE"

resolve_base_ref() {
  if [[ -n "$BASE_REF" ]]; then
    echo "$BASE_REF"
    return 0
  fi
  if git rev-parse --verify origin/main >/dev/null 2>&1; then
    git merge-base HEAD origin/main
    return 0
  fi
  if git rev-parse --verify HEAD~1 >/dev/null 2>&1; then
    git rev-parse HEAD~1
    return 0
  fi
  git rev-parse HEAD
}

contains() {
  local needle="$1"
  shift
  local x
  for x in "$@"; do
    if [[ "$x" == "$needle" ]]; then
      return 0
    fi
  done
  return 1
}

add_unique() {
  local arr_name="$1"
  local val="$2"
  local existing=()
  eval "existing=(\"\${${arr_name}[@]-}\")"
  if ! contains "$val" "${existing[@]}"; then
    eval "${arr_name}+=(\"\$val\")"
  fi
}

NEXTTEST_AVAILABLE=0
if cargo nextest --version >/dev/null 2>&1; then
  NEXTTEST_AVAILABLE=1
fi
if [[ "$RUNNER" == "auto" ]]; then
  if (( NEXTTEST_AVAILABLE == 1 )); then
    RUNNER="nextest"
  else
    RUNNER="cargo"
  fi
fi
if [[ "$RUNNER" == "nextest" && "$NEXTTEST_AVAILABLE" -ne 1 ]]; then
  echo "test-changed-fast: cargo-nextest requested but not installed" >&2
  exit 2
fi

BASE="$(resolve_base_ref)"
IMPACT_TMP_DIR="$(mktemp -d)"
IMPACT_PLAN="$IMPACT_TMP_DIR/plan.json"
if [[ -n "$CHANGED_FILES_OVERRIDE" ]]; then
  printf '%s\n' "$CHANGED_FILES_OVERRIDE" >"$IMPACT_TMP_DIR/changed.txt"
  python3 scripts/lib/changed_impact.py \
    --root "$ROOT_DIR" \
    --changed-files "$IMPACT_TMP_DIR/changed.txt" \
    --runner "$RUNNER" \
    --out "$IMPACT_PLAN"
else
  python3 scripts/lib/changed_impact.py \
    --root "$ROOT_DIR" \
    --git-base "$BASE" \
    --runner "$RUNNER" \
    --out "$IMPACT_PLAN"
fi

MODE="$(python3 -c 'import json,sys; print(json.load(open(sys.argv[1]))["mode"])' "$IMPACT_PLAN")"
CHANGED_COUNT="$(python3 -c 'import json,sys; print(len(json.load(open(sys.argv[1]))["changedFiles"]))' "$IMPACT_PLAN")"
FALLBACK_PROFILE="$(python3 -c 'import json,sys; print(json.load(open(sys.argv[1]))["fallbackProfile"] or "")' "$IMPACT_PLAN")"
IMPACT_SHA256="$(python3 -c 'from hashlib import sha256; import pathlib,sys; print(sha256(pathlib.Path(sys.argv[1]).read_bytes()).hexdigest())' "$IMPACT_PLAN")"
declare -a CHANGED_FILES=()
declare -a TARGET_CRATES=()
declare -a IMPACT_GATES=()
declare -a COMMANDS=()
while IFS= read -r value; do [[ -n "$value" ]] && CHANGED_FILES+=("$value"); done < <(
  python3 -c 'import json,sys; print("\n".join(json.load(open(sys.argv[1]))["changedFiles"]))' "$IMPACT_PLAN"
)
while IFS= read -r value; do [[ -n "$value" ]] && TARGET_CRATES+=("$value"); done < <(
  python3 -c 'import json,sys; print("\n".join(json.load(open(sys.argv[1]))["affectedCrates"]))' "$IMPACT_PLAN"
)
while IFS= read -r value; do [[ -n "$value" ]] && IMPACT_GATES+=("$value"); done < <(
  python3 -c 'import json,sys; print("\n".join(json.load(open(sys.argv[1]))["affectedGates"]))' "$IMPACT_PLAN"
)
while IFS= read -r value; do [[ -n "$value" ]] && COMMANDS+=("$value"); done < <(
  python3 -c 'import json,sys; print("\n".join(json.load(open(sys.argv[1]))["commands"]))' "$IMPACT_PLAN"
)

echo "test-changed-fast: base=$BASE"
echo "test-changed-fast: mode=$MODE runner=$RUNNER changed_files=$CHANGED_COUNT commands=${#COMMANDS[@]}"

BUDGET_SUBJECT="changed-file-gate"
if [[ "$MODE" == "profile-fallback" && "$BUDGET_EXPLICIT" == "0" ]]; then
  BUDGET_MS="$FALLBACK_BUDGET_MS"
  GENESIS_CHANGED_GATE_DISK_BUDGET_BYTES="$GENESIS_CHANGED_GATE_FALLBACK_DISK_BUDGET_BYTES"
  BUDGET_SUBJECT="${FALLBACK_PROFILE:-prepush-standard}"
fi
echo "test-changed-fast: budget_subject=$BUDGET_SUBJECT budget_ms=$BUDGET_MS disk_budget_bytes=$GENESIS_CHANGED_GATE_DISK_BUDGET_BYTES"

if (( DRY_RUN == 1 )); then
  printf 'test-changed-fast: changed files:\n'
  printf '  %s\n' "${CHANGED_FILES[@]:-<none>}"
  printf 'test-changed-fast: commands:\n'
  printf '  %s\n' "${COMMANDS[@]}"
  exit 0
fi

mkdir -p "$(dirname "$REPORT_PATH")"
mkdir -p "$(dirname "$HISTORY_PATH")"

# Generated authorities are resolved in an external staging worktree before any
# selected gate can validate or publish a stale derived view. Its path-specific
# Cargo products remain stage-local and are reclaimed with that worktree rather
# than contaminating this loop's reusable target and residual disk budget.
if (( ${#CHANGED_FILES[@]} > 0 )); then
  declare -a GENERATED_AUTHORITY_ARGS=(--freshness)
  for path in "${CHANGED_FILES[@]}"; do
    GENERATED_AUTHORITY_ARGS+=(--path "$path")
  done
  python3 scripts/lib/generated_authority.py "${GENERATED_AUTHORITY_ARGS[@]}"
else
  echo "test-changed-fast: generated-authority skipped (clean tree)"
fi

for cmd in "${COMMANDS[@]}"; do
  echo ">> $cmd"
  # Execute in-process to avoid one shell startup per command in the hot loop.
  eval "$cmd"
done

GENESIS_CHANGED_GATE_END_NS="$(python3 - <<'PY'
import time
print(time.time_ns())
PY
)"
ELAPSED_MS="$(( (GENESIS_CHANGED_GATE_END_NS - GENESIS_CHANGED_GATE_START_NS) / 1000000 ))"
GENESIS_CHANGED_GATE_END_GENERATED_BYTES="$(generated_target_bytes)"
GENERATED_DISK_DELTA_BYTES="$(( GENESIS_CHANGED_GATE_END_GENERATED_BYTES > GENESIS_CHANGED_GATE_START_GENERATED_BYTES ? GENESIS_CHANGED_GATE_END_GENERATED_BYTES - GENESIS_CHANGED_GATE_START_GENERATED_BYTES : 0 ))"

python3 - "$REPORT_PATH" "$HISTORY_PATH" "$BASE" "$MODE" "$RUNNER" "$CHANGED_COUNT" "$ELAPSED_MS" "$BUDGET_MS" "$MIN_HISTORY" "$STRICT_DISK_MODE" "$IMPACT_SHA256" "${#TARGET_CRATES[@]}" "${#IMPACT_GATES[@]}" "$FALLBACK_PROFILE" "$GENERATED_DISK_DELTA_BYTES" "$GENESIS_CHANGED_GATE_DISK_BUDGET_BYTES" "$BUDGET_SUBJECT" <<'PY'
import json, os, statistics, sys, time
report_path, history_path, base, mode, runner, changed_count_s, elapsed_ms_s, budget_ms_s, min_hist_s, strict_disk_mode, impact_sha256, affected_crates_s, affected_gates_s, fallback_profile, disk_delta_s, disk_budget_s, budget_subject = sys.argv[1:]
changed_count = int(changed_count_s)
elapsed_ms = int(elapsed_ms_s)
budget_ms = int(budget_ms_s)
min_history = int(min_hist_s)

entry = {
    "kind": "genesis/test-changed-fast-metrics-v0.1",
    "timestamp_unix_s": int(time.time()),
    "base": base,
    "mode": mode,
    "runner": runner,
    "changed_file_count": changed_count,
    "elapsed_ms": elapsed_ms,
    "budget_ms": budget_ms,
    "budget_subject": budget_subject,
    "disk_strict_mode": strict_disk_mode,
    "generated_disk_delta_bytes": int(disk_delta_s),
    "generated_disk_budget_bytes": int(disk_budget_s),
    "generated_disk_measurement": "allocated-blocks-active-cargo-target",
    "network_mode": "deny",
    "impact_plan_sha256": impact_sha256,
    "affected_crate_count": int(affected_crates_s),
    "affected_gate_count": int(affected_gates_s),
    "fallback_profile": fallback_profile or None,
}

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
            if isinstance(obj, dict) and "elapsed_ms" in obj:
                history.append(obj)
history.append(entry)
history = history[-200:]

with open(history_path, "w", encoding="utf-8") as f:
    for row in history:
        f.write(json.dumps(row, sort_keys=True))
        f.write("\n")

comparable = [
    row for row in history
    if isinstance(row, dict)
    and "elapsed_ms" in row
    and row.get("mode") == mode
    and row.get("runner") == runner
    and int(row.get("budget_ms", -1)) == budget_ms
]
elapsed = sorted(int(row["elapsed_ms"]) for row in comparable)
if not elapsed:
    elapsed = sorted(int(row["elapsed_ms"]) for row in history if isinstance(row, dict) and "elapsed_ms" in row)
if elapsed:
    idx = max(0, min(len(elapsed)-1, int(round(0.95 * (len(elapsed)-1)))))
    p95 = elapsed[idx]
else:
    p95 = elapsed_ms

report = {
    **entry,
    "history_samples": len(elapsed),
    "history_p95_ms": p95,
    "history_enforced": len(elapsed) >= min_history,
}

with open(report_path, "w", encoding="utf-8") as f:
    json.dump(report, f, indent=2, sort_keys=True)
    f.write("\n")

print(json.dumps(report, sort_keys=True))

if elapsed_ms > budget_ms:
    raise SystemExit(f"test-changed-fast: elapsed_ms {elapsed_ms} exceeds budget {budget_ms}")
if int(disk_delta_s) > int(disk_budget_s):
    raise SystemExit(
        f"test-changed-fast: generated disk delta {disk_delta_s} exceeds budget {disk_budget_s}"
    )
if len(elapsed) >= min_history and p95 > budget_ms:
    raise SystemExit(
        f"test-changed-fast: history p95 {p95} exceeds budget {budget_ms} with {len(elapsed)} samples"
    )
PY

echo "test-changed-fast: ok elapsed_ms=$ELAPSED_MS budget_ms=$BUDGET_MS report=$REPORT_DISPLAY"
