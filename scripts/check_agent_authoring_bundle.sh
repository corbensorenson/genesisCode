#!/usr/bin/env bash
set -euo pipefail

ROOT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
cd "$ROOT_DIR"

BUNDLE="docs/spec/AGENT_AUTHORING_BUNDLE_v0.1.md"
AGENT_INDEX_SPEC="docs/spec/AGENT_INDEX_v0.1.md"
AGENT_INDEX_CMD="crates/gc_cli_driver/src/cmd_agent_index.rs"

[[ -f "$BUNDLE" ]] || {
  echo "agent-authoring-bundle: missing bundle doc: $BUNDLE" >&2
  exit 1
}
[[ -f "$AGENT_INDEX_SPEC" ]] || {
  echo "agent-authoring-bundle: missing agent index spec: $AGENT_INDEX_SPEC" >&2
  exit 1
}
[[ -f "$AGENT_INDEX_CMD" ]] || {
  echo "agent-authoring-bundle: missing agent index command source: $AGENT_INDEX_CMD" >&2
  exit 1
}

python3 - "$BUNDLE" "$AGENT_INDEX_SPEC" "$AGENT_INDEX_CMD" <<'PY'
import pathlib
import re
import sys

bundle_path = pathlib.Path(sys.argv[1])
agent_index_spec_path = pathlib.Path(sys.argv[2])
agent_index_cmd_path = pathlib.Path(sys.argv[3])

bundle = bundle_path.read_text(encoding="utf-8")
agent_index_spec = agent_index_spec_path.read_text(encoding="utf-8")
agent_index_cmd = agent_index_cmd_path.read_text(encoding="utf-8")

include_re = re.compile(r"^- `([^`]+)`\s*$", re.MULTILINE)
included_paths = include_re.findall(bundle)
if not included_paths:
    raise SystemExit("agent-authoring-bundle: no included specs found in bundle")

required_included = [
    "docs/spec/CLI_TOOLING_BUNDLE_v0.1.md",
    "docs/spec/GCPM_BUNDLE_v0.1.md",
    "docs/spec/HOST_RUNTIME_BUNDLE_v0.1.md",
    "docs/spec/TESTING_BUNDLE_v0.1.md",
    "docs/spec/AGENT_INDEX_v0.1.md",
    "docs/spec/AGENT_CAPABILITY_GAUNTLET_v0.1.md",
    "docs/spec/WRITE_GENESISCODE_SKILL_PACK_v0.1.md",
    "docs/spec/WRITE_GENESISCODE_SKILL_PACK_v0.1.json",
    "docs/spec/WRITE_GENESISCODE_SKILL_DISTRIBUTION_v1.md",
    "docs/skill_pack/write_genesiscode_v1/manifest.json",
    "docs/write_genesisCode_skill.md",
]
missing_required = [p for p in required_included if p not in included_paths]
if missing_required:
    raise SystemExit(
        "agent-authoring-bundle: missing required included spec path(s): "
        + ", ".join(missing_required)
    )

for p in included_paths:
    if not pathlib.Path(p).is_file():
        raise SystemExit(f"agent-authoring-bundle: listed path does not exist: {p}")

legacy_header = "## Legacy Split Docs (must stay marked)"
if legacy_header not in bundle:
    raise SystemExit("agent-authoring-bundle: missing legacy split docs section")

legacy_block = bundle.split(legacy_header, 1)[1]
legacy_paths = include_re.findall(legacy_block)
if not legacy_paths:
    raise SystemExit("agent-authoring-bundle: legacy split docs section has no paths")

for p in legacy_paths:
    doc = pathlib.Path(p)
    if not doc.is_file():
        raise SystemExit(f"agent-authoring-bundle: legacy split doc missing: {p}")
    src = doc.read_text(encoding="utf-8")
    if "Bundle Entry:" not in src or "Legacy Split Doc:" not in src:
        raise SystemExit(
            f"agent-authoring-bundle: legacy split doc is not clearly marked: {p}"
        )

bundle_rel = bundle_path.as_posix()
if bundle_rel not in agent_index_spec:
    raise SystemExit(
        "agent-authoring-bundle: AGENT_INDEX spec must reference the authoring bundle path"
    )
if bundle_rel not in agent_index_cmd:
    raise SystemExit(
        "agent-authoring-bundle: cmd_agent_index must expose authoring bundle in docs map"
    )

print(
    "agent-authoring-bundle: ok "
    f"(included={len(included_paths)} legacy_marked={len(legacy_paths)})"
)
PY
