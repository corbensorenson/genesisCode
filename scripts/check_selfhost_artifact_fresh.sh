#!/usr/bin/env bash
set -euo pipefail

source "$(dirname "${BASH_SOURCE[0]}")/lib/gate_telemetry.sh"
genesis_gate_telemetry_reexec "$0" "$@"

ROOT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
TMP_DIR="$(mktemp -d)"
trap 'rm -rf "$TMP_DIR"' EXIT

python3 - "$ROOT_DIR/scripts/update_selfhost_toolchain_review.sh" <<'PY'
import pathlib
import sys

source = pathlib.Path(sys.argv[1]).read_text(encoding="utf-8")
required = (
    'recovery_input="$recovery_seed.missing"',
    '[[ ! -e "$recovery_input" ]]',
    '--selfhost-artifact "$recovery_input"',
    '--recover-missing-artifact',
)
missing = [token for token in required if source.count(token) != 1]
if missing:
    raise SystemExit(
        "selfhost-artifact-fresh: stale-artifact recovery route drift: "
        + ", ".join(missing)
    )
PY

bash "$ROOT_DIR/scripts/render_selfhost_artifact_fresh_report.sh"   "$TMP_DIR/selfhost_artifact_fresh_report.json"   "$TMP_DIR/selfhost_artifact_fresh_history.jsonl"
