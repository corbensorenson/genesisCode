#!/usr/bin/env bash
set -euo pipefail

source "$(dirname "${BASH_SOURCE[0]}")/lib/gate_telemetry.sh"
genesis_gate_telemetry_reexec "$0" "$@"

ROOT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
cd "$ROOT_DIR"

CONTRACT_FILE="docs/spec/WRITE_GENESISCODE_SKILL_v0.1.json"
SKILL_FILE=".agents/skills/genesiscode-authoring/SKILL.md"
POINTER_FILE="docs/write_genesisCode_skill.md"
BUNDLE_FILE="docs/spec/AGENT_AUTHORING_BUNDLE_v0.1.md"

[[ -f "$CONTRACT_FILE" ]] || {
  echo "genesiscode-authoring-skill: missing contract file: $CONTRACT_FILE" >&2
  exit 1
}
[[ -f "$SKILL_FILE" ]] || {
  echo "genesiscode-authoring-skill: missing skill file: $SKILL_FILE" >&2
  exit 1
}
[[ -f "$POINTER_FILE" ]] || {
  echo "genesiscode-authoring-skill: missing pointer file: $POINTER_FILE" >&2
  exit 1
}
[[ -f "$BUNDLE_FILE" ]] || {
  echo "genesiscode-authoring-skill: missing bundle file: $BUNDLE_FILE" >&2
  exit 1
}

python3 - "$CONTRACT_FILE" "$SKILL_FILE" "$POINTER_FILE" "$BUNDLE_FILE" <<'PY'
import json
import pathlib
import sys

contract_path = pathlib.Path(sys.argv[1])
skill_path = pathlib.Path(sys.argv[2])
pointer_path = pathlib.Path(sys.argv[3])
bundle_path = pathlib.Path(sys.argv[4])
root = pathlib.Path.cwd()

contract = json.loads(contract_path.read_text(encoding="utf-8"))
skill = skill_path.read_text(encoding="utf-8")
pointer = pointer_path.read_text(encoding="utf-8")
bundle = bundle_path.read_text(encoding="utf-8")

if contract.get("kind") != "genesis/write-genesiscode-skill-contract-v0.1":
    raise SystemExit(
        "genesiscode-authoring-skill: invalid contract kind in docs/spec/WRITE_GENESISCODE_SKILL_v0.1.json"
    )

contract_bundle = contract.get("bundle_entrypoint")
contract_pointer = contract.get("pointer_doc")
contract_skill = contract.get("skill_file")

if contract_bundle != bundle_path.as_posix():
    raise SystemExit(
        "genesiscode-authoring-skill: contract bundle_entrypoint must match docs/spec/AGENT_AUTHORING_BUNDLE_v0.1.md"
    )
if contract_pointer != pointer_path.as_posix():
    raise SystemExit(
        "genesiscode-authoring-skill: contract pointer_doc must match docs/write_genesisCode_skill.md"
    )
if contract_skill != skill_path.as_posix():
    raise SystemExit(
        "genesiscode-authoring-skill: contract skill_file must match .agents/skills/genesiscode-authoring/SKILL.md"
    )

required_sections = contract.get("required_skill_sections", [])
required_refs = contract.get("required_spec_refs", [])
required_contract_ids = contract.get("required_contract_ids", [])
required_indices = contract.get("required_capability_indices", [])
required_schema_docs = contract.get("required_schema_docs", [])

missing_sections = [s for s in required_sections if s not in skill]
if missing_sections:
    raise SystemExit(
        "genesiscode-authoring-skill: missing required section(s): "
        + ", ".join(missing_sections)
    )

missing_ref_files = [r for r in required_refs if not (root / r).is_file()]
if missing_ref_files:
    raise SystemExit(
        "genesiscode-authoring-skill: missing required reference file(s): "
        + ", ".join(missing_ref_files)
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

for item in required_indices:
    path = item.get("path")
    kind = item.get("kind")
    if not path or not kind:
        raise SystemExit(
            "genesiscode-authoring-skill: required_capability_indices entries must include path + kind"
        )
    p = root / path
    if not p.is_file():
        raise SystemExit(
            "genesiscode-authoring-skill: missing capability index file: "
            + path
        )
    obj = json.loads(p.read_text(encoding="utf-8"))
    if obj.get("kind") != kind:
        raise SystemExit(
            "genesiscode-authoring-skill: capability index kind mismatch for "
            + path
            + f" (expected {kind}, got {obj.get('kind')})"
        )
    if path not in skill:
        raise SystemExit(
            "genesiscode-authoring-skill: skill must explicitly reference capability index path: "
            + path
        )

for entry in required_schema_docs:
    path = entry.get("path")
    kinds = entry.get("kinds", [])
    if not path or not kinds:
        raise SystemExit(
            "genesiscode-authoring-skill: required_schema_docs entries must include path + kinds"
        )
    p = root / path
    if not p.is_file():
        raise SystemExit(
            "genesiscode-authoring-skill: missing schema doc file: "
            + path
        )
    text = p.read_text(encoding="utf-8")
    missing_kinds = [k for k in kinds if k not in text]
    if missing_kinds:
        raise SystemExit(
            "genesiscode-authoring-skill: schema doc missing required kind(s) "
            + ", ".join(missing_kinds)
            + " in "
            + path
        )

if skill_path.as_posix() not in pointer:
    raise SystemExit(
        "genesiscode-authoring-skill: docs/write_genesisCode_skill.md must point to canonical SKILL.md path"
    )
if contract_path.as_posix() not in pointer:
    raise SystemExit(
        "genesiscode-authoring-skill: docs/write_genesisCode_skill.md must reference machine-consumable contract JSON"
    )
if bundle_path.as_posix() not in pointer:
    raise SystemExit(
        "genesiscode-authoring-skill: docs/write_genesisCode_skill.md must reference canonical bundle path"
    )

if pointer_path.as_posix() not in bundle:
    raise SystemExit(
        "genesiscode-authoring-skill: AGENT_AUTHORING_BUNDLE must include docs/write_genesisCode_skill.md"
    )
if "docs/spec/WRITE_GENESISCODE_SKILL_v0.1.md" not in bundle:
    raise SystemExit(
        "genesiscode-authoring-skill: AGENT_AUTHORING_BUNDLE must include docs/spec/WRITE_GENESISCODE_SKILL_v0.1.md"
    )

print(
    "genesiscode-authoring-skill: ok "
    f"(sections={len(required_sections)} refs={len(required_refs)} "
    f"contract_ids={len(required_contract_ids)} indices={len(required_indices)})"
)
PY
