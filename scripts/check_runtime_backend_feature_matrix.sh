#!/usr/bin/env bash
set -euo pipefail

ROOT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
cd "$ROOT_DIR"

source "$ROOT_DIR/scripts/lib/cargo_target_dir.sh"

DISK_MIN_FREE_KB="${GENESIS_RUNTIME_BACKEND_MATRIX_MIN_FREE_KB:-1572864}"
DISK_STRICT_MODE="${GENESIS_RUNTIME_BACKEND_MATRIX_DISK_STRICT_MODE:-1}"
REPORT_OUT="${GENESIS_RUNTIME_BACKEND_MATRIX_REPORT_OUT:-.genesis/perf/runtime_backend_feature_matrix_report.json}"
HISTORY_OUT="${GENESIS_RUNTIME_BACKEND_MATRIX_HISTORY_OUT:-.genesis/perf/runtime_backend_feature_matrix_history.jsonl}"
BUDGET_MS="${GENESIS_RUNTIME_BACKEND_MATRIX_BUDGET_MS:-0}"
TMP_DIR="$(mktemp -d)"
STAGE_FILE="$TMP_DIR/stages.tsv"
trap 'rm -rf "$TMP_DIR"' EXIT

if [[ ! "$BUDGET_MS" =~ ^[0-9]+$ ]]; then
  echo "runtime-backend-feature-matrix: GENESIS_RUNTIME_BACKEND_MATRIX_BUDGET_MS must be numeric" >&2
  exit 2
fi

bash scripts/check_disk_headroom.sh \
  --path "$ROOT_DIR" \
  --context "runtime-backend-feature-matrix" \
  --min-kb "$DISK_MIN_FREE_KB" \
  --strict "$DISK_STRICT_MODE"
genesis_configure_cargo_target_dir \
  "$ROOT_DIR" \
  "runtime-backend-feature-matrix" \
  ".genesis/build/runtime_backend_feature_matrix" \
  "GENESIS_RUNTIME_BACKEND_MATRIX_CARGO_TARGET_DIR"

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
  start_ms="$(now_ms)"
  "$@"
  end_ms="$(now_ms)"
  elapsed_ms=$((end_ms - start_ms))
  printf '%s\t%s\n' "$label" "$elapsed_ms" >>"$STAGE_FILE"
  echo "runtime-backend-feature-matrix: stage='${label}' elapsed_ms=${elapsed_ms}"
}

TOTAL_START_MS="$(now_ms)"

echo "runtime-backend-feature-matrix: checking gc_effects feature combinations"
run_stage "gc_effects default" cargo check -p gc_effects --quiet
run_stage "gc_effects gpu-device-backend" cargo check -p gc_effects --no-default-features --features gpu-device-backend --quiet
run_stage "gc_effects gfx-desktop-backend" cargo check -p gc_effects --no-default-features --features gfx-desktop-backend --quiet
run_stage "gc_effects gpu+gfx" cargo check -p gc_effects --no-default-features --features gpu-device-backend,gfx-desktop-backend --quiet

echo "runtime-backend-feature-matrix: checking gc_cli build profiles"
run_stage "gc_cli default" cargo check -p gc_cli --quiet

for profile in profile-headless profile-gpu profile-gfx profile-backend; do
  run_stage "gc_cli ${profile}" cargo check -p gc_cli --no-default-features --features "${profile}" --quiet
done

echo "runtime-backend-feature-matrix: checking gc_cli_driver backend profile tests"
for profile in profile-headless profile-gpu profile-gfx profile-backend; do
  run_stage "gc_cli_driver ${profile}" \
    cargo test -p gc_cli_driver --no-default-features --features "${profile}" \
      backend_feature_flags_match_active_profile --quiet
done

echo "runtime-backend-feature-matrix: checking gcpm env runtime backend mapping end-to-end"
for profile in profile-headless profile-gpu profile-gfx profile-backend; do
  run_stage "gc_cli gcpm env ${profile}" \
    cargo test -p gc_cli --no-default-features --features "${profile}" --test cli_pkg_workspace \
      gcpm_env_runtime_backend_profile_contract_is_machine_readable --quiet
done

TOTAL_END_MS="$(now_ms)"
TOTAL_ELAPSED_MS=$((TOTAL_END_MS - TOTAL_START_MS))

python3 - "$REPORT_OUT" "$HISTORY_OUT" "$STAGE_FILE" "$TOTAL_ELAPSED_MS" "$BUDGET_MS" <<'PY'
import json
import pathlib
import sys
import time

report_path = pathlib.Path(sys.argv[1])
history_path = pathlib.Path(sys.argv[2])
stages_path = pathlib.Path(sys.argv[3])
elapsed_ms = int(sys.argv[4])
budget_ms = int(sys.argv[5])

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
    "budget_ms": budget_ms if budget_ms > 0 else None,
    "ok": (elapsed_ms <= budget_ms) if budget_ms > 0 else True,
    "stage_count": len(stages),
    "stages": stages,
}

if report_path.is_file():
    try:
        prev = json.loads(report_path.read_text(encoding="utf-8"))
        if (
            isinstance(prev, dict)
            and prev.get("kind") == "genesis/runtime-backend-feature-matrix-v0.1"
            and isinstance(prev.get("elapsed_ms"), int)
        ):
            doc["previous_elapsed_ms"] = prev["elapsed_ms"]
            doc["elapsed_delta_ms"] = elapsed_ms - prev["elapsed_ms"]
    except Exception:
        pass

report_path.parent.mkdir(parents=True, exist_ok=True)
history_path.parent.mkdir(parents=True, exist_ok=True)
report_path.write_text(json.dumps(doc, indent=2, sort_keys=True) + "\n", encoding="utf-8")
with history_path.open("a", encoding="utf-8") as fh:
    fh.write(json.dumps(doc, sort_keys=True) + "\n")

print(f"runtime-backend-feature-matrix: wrote report {report_path}")
if budget_ms > 0 and elapsed_ms > budget_ms:
    raise SystemExit(
        f"runtime-backend-feature-matrix: budget exceeded ({elapsed_ms}ms > {budget_ms}ms)"
    )
PY

echo "runtime-backend-feature-matrix: ok elapsed_ms=${TOTAL_ELAPSED_MS}"
