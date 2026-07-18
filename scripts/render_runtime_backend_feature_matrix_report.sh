#!/usr/bin/env bash
set -euo pipefail

ROOT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
cd "$ROOT_DIR"

if [[ "$#" -ne 3 ]]; then
  echo "usage: $0 <report-output> <history-output> <history-input>" >&2
  exit 2
fi

source "$ROOT_DIR/scripts/lib/cargo_target_dir.sh"

REPORT_OUT="$1"
HISTORY_OUT="$2"
HISTORY_INPUT="$3"
EPHEMERAL_TARGET_REQUEST="${GENESIS_RUNTIME_BACKEND_MATRIX_EPHEMERAL_TARGET_DIR:-}"
EPHEMERAL_TARGET_DIR=""
DISK_MIN_FREE_KB="${GENESIS_RUNTIME_BACKEND_MATRIX_MIN_FREE_KB:-2097152}"
DISK_STRICT_MODE="${GENESIS_RUNTIME_BACKEND_MATRIX_DISK_STRICT_MODE:-auto}"
AUTO_RECLAIM="${GENESIS_RUNTIME_BACKEND_MATRIX_AUTO_RECLAIM:-0}"
BUDGET_MS="${GENESIS_RUNTIME_BACKEND_MATRIX_BUDGET_MS:-360000}"
MATRIX_DEV_DEBUG="${GENESIS_RUNTIME_BACKEND_MATRIX_CARGO_PROFILE_DEV_DEBUG:-0}"
if [[ "${CI:-}" == "true" ]]; then
  MATRIX_INCREMENTAL_DEFAULT="0"
else
  MATRIX_INCREMENTAL_DEFAULT="1"
fi
MATRIX_INCREMENTAL="${GENESIS_RUNTIME_BACKEND_MATRIX_CARGO_INCREMENTAL:-$MATRIX_INCREMENTAL_DEFAULT}"
STAGE_AUTO_RECLAIM="${GENESIS_RUNTIME_BACKEND_MATRIX_STAGE_AUTO_RECLAIM:-0}"
TMP_DIR="$(mktemp -d)"
STAGE_FILE="$TMP_DIR/stages.tsv"
cleanup() {
  if [[ -n "$EPHEMERAL_TARGET_DIR" ]]; then
    rm -rf "$EPHEMERAL_TARGET_DIR"
  fi
  rm -rf "$TMP_DIR"
}
trap cleanup EXIT

if [[ ! "$BUDGET_MS" =~ ^[0-9]+$ || "$BUDGET_MS" -le 0 ]]; then
  echo "runtime-backend-feature-matrix: GENESIS_RUNTIME_BACKEND_MATRIX_BUDGET_MS must be a positive integer" >&2
  exit 2
fi
if [[ "$AUTO_RECLAIM" != "0" && "$AUTO_RECLAIM" != "1" ]]; then
  echo "runtime-backend-feature-matrix: GENESIS_RUNTIME_BACKEND_MATRIX_AUTO_RECLAIM must be 0 or 1" >&2
  exit 2
fi
if [[ "$AUTO_RECLAIM" == "1" ]]; then
  echo "runtime-backend-feature-matrix: checks are read-only outside their declared compilation outputs; dry-run, review, and execute a confirmed dev-clean plan with scripts/reclaim_build_space.sh, then set GENESIS_RUNTIME_BACKEND_MATRIX_AUTO_RECLAIM=0" >&2
  exit 2
fi
if [[ "$DISK_STRICT_MODE" != "auto" && "$DISK_STRICT_MODE" != "0" && "$DISK_STRICT_MODE" != "1" ]]; then
  echo "runtime-backend-feature-matrix: GENESIS_RUNTIME_BACKEND_MATRIX_DISK_STRICT_MODE must be auto, 0, or 1" >&2
  exit 2
fi
if [[ ! "$MATRIX_DEV_DEBUG" =~ ^[0-9]+$ ]]; then
  echo "runtime-backend-feature-matrix: GENESIS_RUNTIME_BACKEND_MATRIX_CARGO_PROFILE_DEV_DEBUG must be numeric" >&2
  exit 2
fi
if [[ "$MATRIX_INCREMENTAL" != "0" && "$MATRIX_INCREMENTAL" != "1" ]]; then
  echo "runtime-backend-feature-matrix: GENESIS_RUNTIME_BACKEND_MATRIX_CARGO_INCREMENTAL must be 0 or 1" >&2
  exit 2
fi
if [[ "$STAGE_AUTO_RECLAIM" != "0" && "$STAGE_AUTO_RECLAIM" != "1" ]]; then
  echo "runtime-backend-feature-matrix: GENESIS_RUNTIME_BACKEND_MATRIX_STAGE_AUTO_RECLAIM must be 0 or 1" >&2
  exit 2
fi
if [[ "$STAGE_AUTO_RECLAIM" == "1" ]]; then
  echo "runtime-backend-feature-matrix: checks cannot reclaim build caches; dry-run, review, and execute a confirmed dev-clean plan with scripts/reclaim_build_space.sh, then set GENESIS_RUNTIME_BACKEND_MATRIX_STAGE_AUTO_RECLAIM=0" >&2
  exit 2
fi

bash scripts/check_disk_headroom.sh \
  --path "$ROOT_DIR" \
  --context "runtime-backend-feature-matrix" \
  --min-kb "$DISK_MIN_FREE_KB" \
  --auto-reclaim 0 \
  --strict "$DISK_STRICT_MODE"
export CARGO_PROFILE_DEV_DEBUG="$MATRIX_DEV_DEBUG"
export CARGO_INCREMENTAL="$MATRIX_INCREMENTAL"
if [[ -n "$EPHEMERAL_TARGET_REQUEST" ]]; then
  mkdir -p "$(dirname "$REPORT_OUT")"
  genesis_configure_ephemeral_cargo_target_dir \
    "runtime-backend-feature-matrix" \
    "$EPHEMERAL_TARGET_REQUEST" \
    "$(dirname "$REPORT_OUT")"
  EPHEMERAL_TARGET_DIR="$CARGO_TARGET_DIR"
else
  genesis_clear_resolved_cargo_target_dir "runtime-backend-feature-matrix"
  genesis_configure_cargo_target_dir \
    "$ROOT_DIR" \
    "runtime-backend-feature-matrix" \
    root-host
fi

now_ms() {
  python3 - <<'PY'
import time
print(int(time.time() * 1000))
PY
}

run_stage() {
  local label="$1"
  shift
  local start_ms
  local end_ms
  local elapsed_ms
  if ! bash scripts/check_disk_headroom.sh \
    --path "$ROOT_DIR" \
    --context "runtime-backend-feature-matrix:${label}" \
    --min-kb "$DISK_MIN_FREE_KB" \
    --auto-reclaim 0 \
    --strict "$DISK_STRICT_MODE"; then
    return 1
  fi
  start_ms="$(now_ms)"
  "$@"
  end_ms="$(now_ms)"
  elapsed_ms=$((end_ms - start_ms))
  printf '%s\t%s\n' "$label" "$elapsed_ms" >>"$STAGE_FILE"
  echo "runtime-backend-feature-matrix: stage='${label}' elapsed_ms=${elapsed_ms}"
}

TOTAL_START_MS="$(now_ms)"

echo "runtime-backend-feature-matrix: checking gc_effects feature combinations"
run_stage "gc_effects default" cargo clippy -p gc_effects --all-targets --locked --offline --quiet -- -D warnings
run_stage "gc_effects gpu-device-backend" cargo clippy -p gc_effects --all-targets --locked --offline --no-default-features --features gpu-device-backend --quiet -- -D warnings
run_stage "gc_effects gfx-desktop-backend" cargo clippy -p gc_effects --all-targets --locked --offline --no-default-features --features gfx-desktop-backend --quiet -- -D warnings
run_stage "gc_effects gpu+gfx" cargo clippy -p gc_effects --all-targets --locked --offline --no-default-features --features gpu-device-backend,gfx-desktop-backend --quiet -- -D warnings

echo "runtime-backend-feature-matrix: checking gc_cli build profiles"
run_stage "gc_cli default" cargo clippy -p gc_cli --all-targets --locked --offline --quiet -- -D warnings

for profile in profile-headless profile-gpu profile-gfx profile-backend; do
  run_stage "gc_cli ${profile}" cargo clippy -p gc_cli --all-targets --locked --offline --no-default-features --features "${profile}" --quiet -- -D warnings
done

echo "runtime-backend-feature-matrix: checking gcpm env runtime backend mapping end-to-end"
for profile in profile-headless profile-gpu profile-gfx profile-backend; do
  run_stage "gc_cli gcpm env ${profile}" \
    cargo test -p gc_cli --no-default-features --features "${profile}" --test cli_pkg_workspace \
      gcpm_env_runtime_backend_profile_contract_is_machine_readable --quiet
done

TOTAL_END_MS="$(now_ms)"
TOTAL_ELAPSED_MS=$((TOTAL_END_MS - TOTAL_START_MS))

python3 - "$REPORT_OUT" "$HISTORY_OUT" "$HISTORY_INPUT" "$STAGE_FILE" "$TOTAL_ELAPSED_MS" "$BUDGET_MS" <<'PY'
import json
import pathlib
import sys
import time

report_path = pathlib.Path(sys.argv[1])
history_path = pathlib.Path(sys.argv[2])
history_input_path = pathlib.Path(sys.argv[3])
stages_path = pathlib.Path(sys.argv[4])
elapsed_ms = int(sys.argv[5])
budget_ms = int(sys.argv[6])

stages = []
for raw in stages_path.read_text(encoding="utf-8").splitlines():
    if not raw.strip():
        continue
    name, elapsed = raw.split("\t", 1)
    stages.append({"name": name, "elapsed_ms": int(elapsed)})

doc = {
    "kind": "genesis/runtime-backend-feature-matrix-v0.1",
    "timestamp_unix_s": int(time.time()),
    "elapsed_ms": elapsed_ms,
    "budget_ms": budget_ms,
    "ok": elapsed_ms <= budget_ms,
    "stage_count": len(stages),
    "stages": stages,
}

if history_input_path.is_file():
    for raw in reversed(history_input_path.read_text(encoding="utf-8").splitlines()):
        try:
            prev = json.loads(raw)
        except json.JSONDecodeError:
            continue
        if (
            isinstance(prev, dict)
            and prev.get("kind") == "genesis/runtime-backend-feature-matrix-v0.1"
            and isinstance(prev.get("elapsed_ms"), int)
        ):
            doc["previous_elapsed_ms"] = prev["elapsed_ms"]
            doc["elapsed_delta_ms"] = elapsed_ms - prev["elapsed_ms"]
            break

report_path.parent.mkdir(parents=True, exist_ok=True)
history_path.parent.mkdir(parents=True, exist_ok=True)
report_path.write_text(json.dumps(doc, indent=2, sort_keys=True) + "\n", encoding="utf-8")
with history_path.open("a", encoding="utf-8") as fh:
    fh.write(json.dumps(doc, sort_keys=True) + "\n")

print(f"runtime-backend-feature-matrix: wrote report {report_path}")
if elapsed_ms > budget_ms:
    raise SystemExit(
        f"runtime-backend-feature-matrix: budget exceeded ({elapsed_ms}ms > {budget_ms}ms)"
    )
PY

echo "runtime-backend-feature-matrix: ok elapsed_ms=${TOTAL_ELAPSED_MS}"
