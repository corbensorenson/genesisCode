#!/usr/bin/env bash
set -euo pipefail

ROOT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
cd "$ROOT_DIR"

HISTORY_IN="${GENESIS_AI_ITERATION_SLO_HISTORY:-.genesis/perf/ai_iteration_slo_history.jsonl}"
BASELINE_OUT="${GENESIS_AI_ITERATION_SLO_BASELINE:-policies/perf/ai_iteration_slo_seed_history.jsonl}"
MIN_SAMPLES="${GENESIS_AI_ITERATION_SLO_MIN_HISTORY:-5}"

if [[ ! -f "$HISTORY_IN" ]]; then
  echo "ai-iteration-slo-baseline: missing history file: $HISTORY_IN" >&2
  exit 1
fi
if [[ ! "$MIN_SAMPLES" =~ ^[0-9]+$ ]] || [[ "$MIN_SAMPLES" -lt 1 ]]; then
  echo "ai-iteration-slo-baseline: MIN_SAMPLES must be a positive integer" >&2
  exit 1
fi

mkdir -p "$(dirname "$BASELINE_OUT")"

python3 - "$HISTORY_IN" "$BASELINE_OUT" "$MIN_SAMPLES" <<'PY'
import json
import pathlib
import sys

history_in = pathlib.Path(sys.argv[1])
baseline_out = pathlib.Path(sys.argv[2])
min_samples = int(sys.argv[3])

rows = []
for line in history_in.read_text(encoding="utf-8").splitlines():
    line = line.strip()
    if not line:
        continue
    try:
        obj = json.loads(line)
    except Exception:
        continue
    if isinstance(obj, dict) and isinstance(obj.get("metrics"), dict):
        rows.append(obj)

if len(rows) < min_samples:
    raise SystemExit(
        f"ai-iteration-slo-baseline: need at least {min_samples} valid history rows, found {len(rows)}"
    )

rows = rows[-min_samples:]
baseline_out.write_text(
    "\n".join(json.dumps(r, sort_keys=True) for r in rows) + "\n",
    encoding="utf-8",
)
print(f"ai-iteration-slo-baseline: wrote {len(rows)} seeded rows to {baseline_out}")
PY
