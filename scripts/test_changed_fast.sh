#!/usr/bin/env bash
set -euo pipefail

ROOT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
cd "$ROOT_DIR"

BASE_REF="${GENESIS_CHANGED_BASE:-}"
RUNNER="${GENESIS_TEST_CHANGED_RUNNER:-auto}" # auto|cargo|nextest
REPORT_PATH="${GENESIS_TEST_CHANGED_REPORT:-.genesis/perf/test_changed_fast_metrics.json}"
HISTORY_PATH="${GENESIS_TEST_CHANGED_HISTORY:-.genesis/perf/test_changed_fast_history.jsonl}"
BUDGET_MS="${GENESIS_TEST_CHANGED_BUDGET_MS:-300000}" # 5 minutes
MIN_HISTORY="${GENESIS_TEST_CHANGED_MIN_HISTORY:-5}"
FULL_MODE_THRESHOLD="${GENESIS_TEST_CHANGED_FULL_THRESHOLD:-120}"
STRICT_DISK_MODE="${GENESIS_TEST_CHANGED_STRICT_DISK:-auto}"
DRY_RUN=0

usage() {
  cat <<'EOF'
Usage: scripts/test_changed_fast.sh [options]

Options:
  --base <rev>         diff base revision (default: merge-base with origin/main or HEAD~1)
  --runner <name>      auto|cargo|nextest (default: auto)
  --report <path>      metrics report path (default: .genesis/perf/test_changed_fast_metrics.json)
  --history <path>     metrics history jsonl (default: .genesis/perf/test_changed_fast_history.jsonl)
  --budget-ms <N>      max allowed elapsed ms for this run (default: 300000)
  --min-history <N>    samples required before enforcing history P95 (default: 5)
  --strict-disk <mode> pass through to check_disk_headroom strict mode (auto|0|1)
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
[[ "$MIN_HISTORY" =~ ^[0-9]+$ ]] || { echo "test-changed-fast: --min-history must be numeric" >&2; exit 2; }
[[ "$FULL_MODE_THRESHOLD" =~ ^[0-9]+$ ]] || { echo "test-changed-fast: GENESIS_TEST_CHANGED_FULL_THRESHOLD must be numeric" >&2; exit 2; }

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
declare -a CHANGED_FILES=()
while IFS= read -r line; do
  [[ -n "$line" ]] || continue
  CHANGED_FILES+=("$line")
done < <(git diff --name-only "$BASE"...HEAD | sed '/^$/d')
CHANGED_COUNT="${#CHANGED_FILES[@]}"

MODE="targeted"
if (( CHANGED_COUNT == 0 )); then
  MODE="clean-tree"
fi
if (( CHANGED_COUNT > FULL_MODE_THRESHOLD )); then
  MODE="full-threshold"
fi

declare -a CHANGED_CRATES=()
declare -a GC_CLI_TESTS=()
declare -a GC_WASI_TESTS=()
NEEDS_SELFHOST_CACHE=0
NEEDS_FULL_FAST=0

for f in "${CHANGED_FILES[@]-}"; do
  case "$f" in
    crates/*/*)
      crate="$(cut -d'/' -f2 <<<"$f")"
      add_unique CHANGED_CRATES "$crate"
      ;;
  esac

  case "$f" in
    crates/gc_cli/tests/*.rs)
      test_name="$(basename "$f" .rs)"
      add_unique GC_CLI_TESTS "$test_name"
      ;;
    crates/gc_wasi_cli/tests/*.rs)
      test_name="$(basename "$f" .rs)"
      add_unique GC_WASI_TESTS "$test_name"
      ;;
  esac

  case "$f" in
    prelude/*|selfhost/*|tests/spec/*|docs/spec/*)
      NEEDS_SELFHOST_CACHE=1
      ;;
  esac

  case "$f" in
    Cargo.toml|Cargo.lock|rust-toolchain*|.github/workflows/*)
      NEEDS_FULL_FAST=1
      ;;
  esac
done

if (( NEEDS_FULL_FAST == 1 )); then
  MODE="full-global-change"
fi

declare -a TARGET_CRATES=()
if [[ "$MODE" != "clean-tree" && "$MODE" != "full-threshold" && "$MODE" != "full-global-change" ]]; then
  for c in "${CHANGED_CRATES[@]-}"; do
    add_unique TARGET_CRATES "$c"
    case "$c" in
      gc_coreform|gc_kernel|gc_prelude|gc_effects|gc_obligations|gc_patches|gc_types|gc_opt|gc_vcs|gc_registry|gc_pkg|gc_cli_driver)
        add_unique TARGET_CRATES "gc_cli"
        NEEDS_SELFHOST_CACHE=1
        ;;
      gc_cli|gc_wasi_cli)
        NEEDS_SELFHOST_CACHE=1
        ;;
    esac
  done
fi

COMMANDS=()
if [[ "$MODE" == "clean-tree" ]]; then
  COMMANDS+=("cargo test -p gc_coreform -p gc_kernel --lib --quiet")
elif [[ "$MODE" == "full-threshold" || "$MODE" == "full-global-change" ]]; then
  COMMANDS+=("bash scripts/test_fast_full.sh")
else
  # WASI integration suites are valuable but expensive; include them only when their crate or
  # WASI-facing specs changed. Full CI lanes still run complete WASI coverage.
  INCLUDE_WASI=0
  if contains "gc_wasi_cli" "${TARGET_CRATES[@]-}"; then
    INCLUDE_WASI=1
  elif printf '%s\n' "${CHANGED_FILES[@]-}" | grep -q '^docs/spec/WASI\.md$'; then
    INCLUDE_WASI=1
  fi
  if (( INCLUDE_WASI == 0 )); then
    declare -a FILTERED=()
    for c in "${TARGET_CRATES[@]-}"; do
      if [[ "$c" != "gc_wasi_cli" ]]; then
        FILTERED+=("$c")
      fi
    done
    TARGET_CRATES=("${FILTERED[@]}")
  fi

  if (( NEEDS_SELFHOST_CACHE == 1 )); then
    COMMANDS+=("bash scripts/warm_selfhost_cache.sh")
  fi

  if [[ "$RUNNER" == "nextest" ]]; then
    for c in "${TARGET_CRATES[@]-}"; do
      if [[ "$c" == "gc_cli" && "${#GC_CLI_TESTS[@]}" -gt 0 ]]; then
        for t in "${GC_CLI_TESTS[@]-}"; do
          COMMANDS+=("cargo nextest run -p gc_cli --test ${t} --profile ci")
        done
      elif [[ "$c" == "gc_cli" ]] && ! contains "gc_cli" "${CHANGED_CRATES[@]-}"; then
        COMMANDS+=("cargo nextest run -p gc_cli --test cli_smoke --test cli_selfhost_only --test cli_store --profile ci")
      elif [[ "$c" == "gc_wasi_cli" && "${#GC_WASI_TESTS[@]}" -gt 0 ]]; then
        for t in "${GC_WASI_TESTS[@]-}"; do
          COMMANDS+=("cargo nextest run -p gc_wasi_cli --test ${t} --profile ci")
        done
      elif [[ "$c" == "gc_wasi_cli" ]] && ! contains "gc_wasi_cli" "${CHANGED_CRATES[@]-}"; then
        COMMANDS+=("cargo nextest run -p gc_wasi_cli --test cli_eval_engine --test cli_store_engine --profile ci")
      else
        COMMANDS+=("cargo nextest run -p ${c} --profile ci")
      fi
    done
  else
    for c in "${TARGET_CRATES[@]-}"; do
      if [[ "$c" == "gc_cli" && "${#GC_CLI_TESTS[@]}" -gt 0 ]]; then
        for t in "${GC_CLI_TESTS[@]-}"; do
          COMMANDS+=("cargo test -p gc_cli --test ${t}")
        done
      elif [[ "$c" == "gc_cli" ]] && ! contains "gc_cli" "${CHANGED_CRATES[@]-}"; then
        COMMANDS+=("cargo test -p gc_cli --test cli_smoke")
        COMMANDS+=("cargo test -p gc_cli --test cli_selfhost_only")
        COMMANDS+=("cargo test -p gc_cli --test cli_store")
      elif [[ "$c" == "gc_wasi_cli" && "${#GC_WASI_TESTS[@]}" -gt 0 ]]; then
        for t in "${GC_WASI_TESTS[@]-}"; do
          COMMANDS+=("cargo test -p gc_wasi_cli --test ${t}")
        done
      elif [[ "$c" == "gc_wasi_cli" ]] && ! contains "gc_wasi_cli" "${CHANGED_CRATES[@]-}"; then
        COMMANDS+=("cargo test -p gc_wasi_cli --test cli_eval_engine")
        COMMANDS+=("cargo test -p gc_wasi_cli --test cli_store_engine")
      else
        COMMANDS+=("cargo test -p ${c}")
      fi
    done
  fi
fi

if [[ "${#COMMANDS[@]}" -eq 0 ]]; then
  COMMANDS+=("cargo test -p gc_coreform --lib --quiet")
fi

echo "test-changed-fast: base=$BASE"
echo "test-changed-fast: mode=$MODE runner=$RUNNER changed_files=$CHANGED_COUNT commands=${#COMMANDS[@]}"

mkdir -p "$(dirname "$REPORT_PATH")"
mkdir -p "$(dirname "$HISTORY_PATH")"

if (( DRY_RUN == 1 )); then
  printf 'test-changed-fast: changed files:\n'
  printf '  %s\n' "${CHANGED_FILES[@]:-<none>}"
  printf 'test-changed-fast: commands:\n'
  printf '  %s\n' "${COMMANDS[@]}"
  exit 0
fi

START_NS="$(python3 - <<'PY'
import time
print(time.time_ns())
PY
)"

for cmd in "${COMMANDS[@]-}"; do
  echo ">> $cmd"
  # Execute in-process to avoid one shell startup per command in the hot loop.
  eval "$cmd"
done

END_NS="$(python3 - <<'PY'
import time
print(time.time_ns())
PY
)"
ELAPSED_MS="$(( (END_NS - START_NS) / 1000000 ))"

python3 - "$REPORT_PATH" "$HISTORY_PATH" "$BASE" "$MODE" "$RUNNER" "$CHANGED_COUNT" "$ELAPSED_MS" "$BUDGET_MS" "$MIN_HISTORY" "$STRICT_DISK_MODE" <<'PY'
import json, os, statistics, sys, time
report_path, history_path, base, mode, runner, changed_count_s, elapsed_ms_s, budget_ms_s, min_hist_s, strict_disk_mode = sys.argv[1:]
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
    "disk_strict_mode": strict_disk_mode,
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
if len(elapsed) >= min_history and p95 > budget_ms:
    raise SystemExit(
        f"test-changed-fast: history p95 {p95} exceeds budget {budget_ms} with {len(elapsed)} samples"
    )
PY

echo "test-changed-fast: ok elapsed_ms=$ELAPSED_MS budget_ms=$BUDGET_MS report=$REPORT_PATH"
