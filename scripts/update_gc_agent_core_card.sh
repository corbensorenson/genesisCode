#!/usr/bin/env bash
set -euo pipefail

ROOT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
cd "$ROOT_DIR"

tmp="$(mktemp "${TMPDIR:-/tmp}/genesis-agent-card.XXXXXX")"
trap 'rm -f "$tmp"' EXIT
python3 scripts/lib/gc_agent_core_card.py --render >"$tmp"
python3 - "$tmp" <<'PY'
import json
from pathlib import Path
import sys

doc = json.loads(Path(sys.argv[1]).read_text(encoding="utf-8"))
Path("docs/spec/GC_AGENT_CORE_CARD_v0.3.md").write_text(doc["card"], encoding="ascii")
Path("docs/spec/GC_AGENT_CORE_CARD_v0.3.json").write_text(
    json.dumps(doc["manifest"], indent=2, sort_keys=True) + "\n", encoding="ascii"
)
PY
bash scripts/check_gc_agent_core_card.sh
