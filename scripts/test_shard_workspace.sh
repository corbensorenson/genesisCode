#!/usr/bin/env bash
set -euo pipefail

ROOT_DIR="$(cd "$(dirname "$0")/.." && pwd)"
cd "$ROOT_DIR"

source "$ROOT_DIR/scripts/lib/cargo_target_dir.sh"
genesis_configure_cargo_target_dir \
  "$ROOT_DIR" \
  "test-shard-workspace" \
  root-host

TOTAL=""
INDEX=""
SEED="${GENESIS_TEST_SHARD_SEED:-genesis-v1}"
OUT_DIR="${GENESIS_TEST_SHARD_OUT_DIR:-.genesis/ci-shards}"
DRY_RUN=0
EXCLUDE_CRATES=()
RUNNER="auto"

usage() {
  cat <<'EOF'
Usage: scripts/test_shard_workspace.sh --total N --index I [options]

Options:
  --total N        total shard count (N >= 1)
  --index I        shard index (0 <= I < N)
  --seed S         deterministic seed for ordering (default: GENESIS_TEST_SHARD_SEED or genesis-v1)
  --out DIR        output directory for shard artifacts (default: .genesis/ci-shards)
  --exclude-crate  omit a workspace crate from shard execution (repeatable)
  --runner NAME    test runner: auto|cargo|nextest (default: auto)
  --dry-run        print selected crates/commands without running tests
EOF
}

while [[ $# -gt 0 ]]; do
  case "$1" in
    --total)
      TOTAL="${2:-}"
      shift 2
      ;;
    --index)
      INDEX="${2:-}"
      shift 2
      ;;
    --seed)
      SEED="${2:-}"
      shift 2
      ;;
    --out)
      OUT_DIR="${2:-}"
      shift 2
      ;;
    --exclude-crate)
      EXCLUDE_CRATES+=("${2:-}")
      shift 2
      ;;
    --runner)
      RUNNER="${2:-}"
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
      echo "test-shard-workspace: unknown argument: $1" >&2
      usage >&2
      exit 2
      ;;
  esac
done

[[ -n "$TOTAL" ]] || { echo "test-shard-workspace: --total is required" >&2; exit 2; }
[[ -n "$INDEX" ]] || { echo "test-shard-workspace: --index is required" >&2; exit 2; }
[[ "$TOTAL" =~ ^[0-9]+$ ]] || { echo "test-shard-workspace: --total must be integer" >&2; exit 2; }
[[ "$INDEX" =~ ^[0-9]+$ ]] || { echo "test-shard-workspace: --index must be integer" >&2; exit 2; }
(( TOTAL >= 1 )) || { echo "test-shard-workspace: --total must be >= 1" >&2; exit 2; }
(( INDEX < TOTAL )) || { echo "test-shard-workspace: --index must be < --total" >&2; exit 2; }
[[ "$RUNNER" == "auto" || "$RUNNER" == "cargo" || "$RUNNER" == "nextest" ]] || {
  echo "test-shard-workspace: --runner must be one of auto|cargo|nextest" >&2
  exit 2
}

now_ns() {
  python3 - <<'PY'
import time
print(time.time_ns())
PY
}

mkdir -p "$OUT_DIR"
LOG_FILE="$OUT_DIR/shard_${INDEX}_of_${TOTAL}.log"
RESULTS_TSV="$OUT_DIR/shard_${INDEX}_of_${TOTAL}.tsv"
REPORT_JSON="$OUT_DIR/shard_${INDEX}_of_${TOTAL}.json"

: >"$LOG_FILE"
: >"$RESULTS_TSV"

WORKSPACE_CRATES=()
while IFS= read -r crate; do
  [[ -z "$crate" ]] && continue
  WORKSPACE_CRATES+=("$crate")
done < <(
  cargo metadata --no-deps --format-version 1 | python3 -c '
import json, sys
m = json.load(sys.stdin)
members = set(m["workspace_members"])
for p in m["packages"]:
    if p["id"] in members:
        print(p["name"])
'
)

if [[ "${#WORKSPACE_CRATES[@]}" -eq 0 ]]; then
  echo "test-shard-workspace: no workspace crates found" >&2
  exit 1
fi

if [[ "${#EXCLUDE_CRATES[@]}" -gt 0 ]]; then
  FILTERED_CRATES=()
  for crate in "${WORKSPACE_CRATES[@]}"; do
    skip=0
    for excluded in "${EXCLUDE_CRATES[@]}"; do
      if [[ "$crate" == "$excluded" ]]; then
        skip=1
        break
      fi
    done
    if (( skip == 0 )); then
      FILTERED_CRATES+=("$crate")
    fi
  done
  WORKSPACE_CRATES=("${FILTERED_CRATES[@]}")
fi

if [[ "${#WORKSPACE_CRATES[@]}" -eq 0 ]]; then
  echo "test-shard-workspace: no workspace crates remain after exclusions" >&2
  exit 1
fi

ORDERED=()
for crate in "${WORKSPACE_CRATES[@]}"; do
  key="$(
    python3 - "$SEED" "$crate" <<'PY'
import hashlib, sys
seed = sys.argv[1]
crate = sys.argv[2]
print(hashlib.sha256(f"{seed}|{crate}".encode("utf-8")).hexdigest())
PY
  )"
  ORDERED+=("${key}"$'\t'"${crate}")
done
ORDERED_SORTED=()
while IFS= read -r line; do
  [[ -z "$line" ]] && continue
  ORDERED_SORTED+=("$line")
done < <(printf "%s\n" "${ORDERED[@]}" | LC_ALL=C sort)

SELECTED=()
for i in "${!ORDERED_SORTED[@]}"; do
  crate="${ORDERED_SORTED[$i]#*$'\t'}"
  if (( i % TOTAL == INDEX )); then
    SELECTED+=("$crate")
  fi
done

echo "test-shard-workspace: shard=${INDEX}/${TOTAL} seed=${SEED}" | tee -a "$LOG_FILE"
echo "test-shard-workspace: selected crates=${#SELECTED[@]}" | tee -a "$LOG_FILE"
if [[ "${#EXCLUDE_CRATES[@]}" -gt 0 ]]; then
  echo "test-shard-workspace: excluded crates=${EXCLUDE_CRATES[*]}" | tee -a "$LOG_FILE"
fi

NEXTTEST_AVAILABLE=0
if cargo nextest --version >/dev/null 2>&1; then
  NEXTTEST_AVAILABLE=1
fi

if [[ "$RUNNER" == "auto" ]]; then
  if (( NEXTTEST_AVAILABLE == 1 )); then
    RESOLVED_RUNNER="nextest"
  else
    RESOLVED_RUNNER="cargo"
  fi
else
  RESOLVED_RUNNER="$RUNNER"
fi

if [[ "$RESOLVED_RUNNER" == "nextest" && "$NEXTTEST_AVAILABLE" -ne 1 && "$DRY_RUN" -ne 1 ]]; then
  echo "test-shard-workspace: nextest runner requested but cargo-nextest is not installed" >&2
  exit 2
fi

echo "test-shard-workspace: runner=${RESOLVED_RUNNER}" | tee -a "$LOG_FILE"

if (( DRY_RUN == 1 )); then
  for crate in "${SELECTED[@]}"; do
    if [[ "$RESOLVED_RUNNER" == "nextest" ]]; then
      echo "DRY-RUN cargo nextest run -p ${crate} --cargo-profile selfhost-strict --profile ci" | tee -a "$LOG_FILE"
    else
      echo "DRY-RUN cargo test -p ${crate} --profile selfhost-strict" | tee -a "$LOG_FILE"
    fi
    printf "%s\t%s\t%s\t%s\n" "$crate" "0" "0" "dry-run" >>"$RESULTS_TSV"
  done
else
  for crate in "${SELECTED[@]}"; do
    if [[ "$RESOLVED_RUNNER" == "nextest" ]]; then
      cmd=(cargo nextest run -p "$crate" --cargo-profile selfhost-strict --profile ci)
    else
      cmd=(cargo test -p "$crate" --profile selfhost-strict)
    fi
    start_ns="$(now_ns)"
    status=0
    {
      echo ">> ${cmd[*]}"
      "${cmd[@]}"
    } >>"$LOG_FILE" 2>&1 || status=$?
    end_ns="$(now_ns)"
    elapsed_ms="$(( (end_ns - start_ns) / 1000000 ))"
    verdict="ok"
    if (( status != 0 )); then
      verdict="fail"
    fi
    printf "%s\t%s\t%s\t%s\n" "$crate" "$status" "$elapsed_ms" "$verdict" >>"$RESULTS_TSV"
  done
fi

python3 - "$RESULTS_TSV" "$REPORT_JSON" "$INDEX" "$TOTAL" "$SEED" "$DRY_RUN" "$RESOLVED_RUNNER" <<'PY'
import json, sys
tsv_path, out_path, index, total, seed, dry_run, runner = sys.argv[1:]
rows = []
failed = []
total_ms = 0
with open(tsv_path, "r", encoding="utf-8") as f:
    for line in f:
        line = line.rstrip("\n")
        if not line:
            continue
        crate, status_s, elapsed_s, verdict = line.split("\t")
        status = int(status_s)
        elapsed_ms = int(elapsed_s)
        total_ms += elapsed_ms
        row = {
            "crate": crate,
            "status": status,
            "elapsed_ms": elapsed_ms,
            "verdict": verdict,
        }
        rows.append(row)
        if status != 0:
            failed.append(crate)
report = {
    "kind": "genesis/test-shard-report-v0.1",
    "shard_index": int(index),
    "shard_total": int(total),
    "seed": seed,
    "dry_run": dry_run == "1",
    "runner": runner,
    "commands": rows,
    "summary": {
        "count": len(rows),
        "failed": len(failed),
        "total_elapsed_ms": total_ms,
        "failed_crates": failed,
    },
}
with open(out_path, "w", encoding="utf-8") as f:
    json.dump(report, f, indent=2, sort_keys=True)
    f.write("\n")
print(json.dumps(report["summary"], sort_keys=True))
PY

if (( DRY_RUN == 1 )); then
  echo "test-shard-workspace: dry-run complete (report: $REPORT_JSON)"
  exit 0
fi

if awk -F '\t' '{ if ($2 != 0) { exit 1 } } END { exit 0 }' "$RESULTS_TSV"; then
  echo "test-shard-workspace: ok (report: $REPORT_JSON)"
  exit 0
fi

echo "test-shard-workspace: failures detected (report: $REPORT_JSON)" >&2
exit 1
