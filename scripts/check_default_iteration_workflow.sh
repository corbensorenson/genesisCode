#!/usr/bin/env bash
set -euo pipefail

ROOT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
cd "$ROOT_DIR"

BUDGET_CHANGED_FAST_MS="${GENESIS_BUDGET_CHANGED_FAST_MS:-300000}"
TMP_DIR="$(mktemp -d)"
cleanup() {
  rm -rf "$TMP_DIR"
}
trap cleanup EXIT

echo "default-iteration-workflow: measuring changed-file default loop"
bash scripts/test_changed_fast.sh \
  --base HEAD \
  --runner auto \
  --budget-ms "$BUDGET_CHANGED_FAST_MS" \
  --min-history 1 \
  --report "$TMP_DIR/changed_fast_report.json" \
  --history "$TMP_DIR/changed_fast_history.jsonl"

python3 - "$TMP_DIR/changed_fast_report.json" <<'PY'
import json
import sys

report = json.load(open(sys.argv[1], "r", encoding="utf-8"))
if report.get("kind") != "genesis/test-changed-fast-metrics-v0.1":
    raise SystemExit("default-iteration-workflow: unexpected changed-fast report kind")
if report.get("elapsed_ms", 0) <= 0:
    raise SystemExit("default-iteration-workflow: changed-fast elapsed_ms must be > 0")
mode = report.get("mode")
if mode not in {"clean-tree", "targeted", "full-threshold", "full-global-change"}:
    raise SystemExit(f"default-iteration-workflow: unexpected changed-fast mode: {mode}")
PY

echo "default-iteration-workflow: validating deterministic shard selection (dry-run)"
SEED="${GENESIS_TEST_SHARD_SEED:-default-iteration-workflow-seed}"
bash scripts/test_shard_workspace.sh \
  --total 3 \
  --index 1 \
  --runner cargo \
  --seed "$SEED" \
  --dry-run \
  --out "$TMP_DIR/shard_a"
bash scripts/test_shard_workspace.sh \
  --total 3 \
  --index 1 \
  --runner cargo \
  --seed "$SEED" \
  --dry-run \
  --out "$TMP_DIR/shard_b"

python3 - "$TMP_DIR/shard_a/shard_1_of_3.json" "$TMP_DIR/shard_b/shard_1_of_3.json" <<'PY'
import json
import sys

a = json.load(open(sys.argv[1], "r", encoding="utf-8"))
b = json.load(open(sys.argv[2], "r", encoding="utf-8"))
if a.get("kind") != "genesis/test-shard-report-v0.1":
    raise SystemExit("default-iteration-workflow: unexpected shard report kind (A)")
if b.get("kind") != "genesis/test-shard-report-v0.1":
    raise SystemExit("default-iteration-workflow: unexpected shard report kind (B)")
crates_a = [row.get("crate") for row in a.get("commands", [])]
crates_b = [row.get("crate") for row in b.get("commands", [])]
if crates_a != crates_b:
    raise SystemExit("default-iteration-workflow: shard selection is not deterministic")
if not crates_a:
    raise SystemExit("default-iteration-workflow: shard selection produced zero crates")
PY

echo "default-iteration-workflow: ok"
