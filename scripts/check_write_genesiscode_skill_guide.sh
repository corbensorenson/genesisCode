#!/usr/bin/env bash
set -euo pipefail

ROOT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
cd "$ROOT_DIR"

GUIDE="docs/write_genesisCode_skill.md"
[[ -f "$GUIDE" ]] || {
  echo "write-genesiscode-skill-guide: missing guide: $GUIDE" >&2
  exit 1
}

python3 - "$GUIDE" <<'PY'
import pathlib
import re
import sys

path = pathlib.Path(sys.argv[1])
text = path.read_text(encoding="utf-8")

required_headers = [
    "## Canonical Sources",
    "## Objective",
    "## Architecture Pattern",
    "## Contract Pattern",
    "## Testing Pattern",
    "## Debugging Pattern",
    "## Performance Pattern",
    "## Assurance Pattern",
    "## Determinism and Replay Pattern",
    "## Package and Deployment Pattern",
    "## Selfhost Evolution Pattern",
    "## Anti-Patterns",
    "## Output Contract for Agent Runs",
]
missing = [h for h in required_headers if h not in text]
if missing:
    raise SystemExit(
        "write-genesiscode-skill-guide: missing required section(s): " + ", ".join(missing)
    )

required_refs = [
    "/Users/corbensorenson/Documents/genesisCode/docs/spec/AGENT_AUTHORING_BUNDLE_v0.1.md",
    "/Users/corbensorenson/Documents/genesisCode/docs/spec/WRITE_GENESISCODE_SKILL_v0.1.json",
    "/Users/corbensorenson/Documents/genesisCode/docs/spec/WRITE_GENESISCODE_SKILL_PACK_v0.1.md",
    "/Users/corbensorenson/Documents/genesisCode/docs/spec/WRITE_GENESISCODE_SKILL_DISTRIBUTION_v1.md",
]
missing_refs = [r for r in required_refs if r not in text]
if missing_refs:
    raise SystemExit(
        "write-genesiscode-skill-guide: missing canonical reference(s): " + ", ".join(missing_refs)
    )

numbered = re.findall(r"^\d+\. ", text, flags=re.MULTILINE)
if len(numbered) < 5:
    raise SystemExit("write-genesiscode-skill-guide: expected at least 5 numbered output contract lines")

print(
    "write-genesiscode-skill-guide: ok "
    f"(sections={len(required_headers)} refs={len(required_refs)})"
)
PY
