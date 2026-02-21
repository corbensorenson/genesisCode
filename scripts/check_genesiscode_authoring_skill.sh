#!/usr/bin/env bash
set -euo pipefail

ROOT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
cd "$ROOT_DIR"

SKILL_FILE=".agents/skills/genesiscode-authoring/SKILL.md"
POINTER_FILE="docs/write_genesisCode_skill.md"

[[ -f "$SKILL_FILE" ]] || {
  echo "genesiscode-authoring-skill: missing skill file: $SKILL_FILE" >&2
  exit 1
}
[[ -f "$POINTER_FILE" ]] || {
  echo "genesiscode-authoring-skill: missing pointer file: $POINTER_FILE" >&2
  exit 1
}

python3 - "$SKILL_FILE" "$POINTER_FILE" <<'PY'
import pathlib
import re
import sys

skill_path = pathlib.Path(sys.argv[1])
pointer_path = pathlib.Path(sys.argv[2])
skill = skill_path.read_text(encoding="utf-8")
pointer = pointer_path.read_text(encoding="utf-8")

required_sections = [
    "## Mission",
    "## Required references (must stay synchronized)",
    "## Required contract IDs (must stay present)",
    "## Ground rules (non-negotiable)",
    "## Canonical workflow (agent prompt protocol)",
    "## Effects, capabilities, and policies",
    "## GenesisGraph / GenesisPkg expectations",
    "## Self-hosting strategy",
    "## Required output quality in reviews/PR notes",
]

required_refs = [
    "docs/spec/CLI.md",
    "docs/spec/CLI_SCHEMA_v0.1.md",
    "docs/spec/CLI_JSON_SCHEMAS_v0.1.md",
    "docs/spec/GCPM_JSON_SCHEMAS_v0.1.md",
    "docs/spec/GCPM_WORKFLOW_REPORTS_v0.1.md",
    "docs/spec/HOST_ABI_INDEX_v0.1.json",
    "docs/spec/PRELUDE_CAPABILITY_INDEX_v0.1.json",
    "docs/spec/SELF_HOST_BOUNDARY.md",
    "docs/spec/TEST_EXECUTION_PROFILES_v0.1.md",
    "upgrade_plan.md",
]

required_contract_ids = [
    "genesis/cli-schema-v0.1",
    "genesis/error-v0.2",
    "genesis/pkg-lock-v0.1",
    "genesis/pkg-update-v0.1",
    "genesis/pkg-publish-v0.1",
]

missing_sections = [s for s in required_sections if s not in skill]
if missing_sections:
    raise SystemExit(
        "genesiscode-authoring-skill: missing required section(s): "
        + ", ".join(missing_sections)
    )

missing_refs = [r for r in required_refs if r not in skill]
if missing_refs:
    raise SystemExit(
        "genesiscode-authoring-skill: missing required reference(s): "
        + ", ".join(missing_refs)
    )

missing_ids = [cid for cid in required_contract_ids if cid not in skill]
if missing_ids:
    raise SystemExit(
        "genesiscode-authoring-skill: missing required contract ID(s): "
        + ", ".join(missing_ids)
    )

if str(skill_path) not in pointer and skill_path.as_posix() not in pointer:
    raise SystemExit(
        "genesiscode-authoring-skill: docs/write_genesisCode_skill.md must point to canonical SKILL.md path"
    )

if re.search(r"^\s*-\s+`docs/spec/.*`\s*$", skill, flags=re.MULTILINE) is None:
    raise SystemExit(
        "genesiscode-authoring-skill: required references list must include explicit docs/spec links"
    )

print("genesiscode-authoring-skill: ok")
PY
