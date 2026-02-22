#!/usr/bin/env bash
set -euo pipefail

ROOT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
cd "$ROOT_DIR"

PACK_JSON="docs/spec/WRITE_GENESISCODE_SKILL_PACK_v0.1.json"
PACK_MD="docs/spec/WRITE_GENESISCODE_SKILL_PACK_v0.1.md"
SKILL_FILE=".agents/skills/genesiscode-authoring/SKILL.md"
POINTER_FILE="docs/write_genesisCode_skill.md"
BUNDLE_FILE="docs/spec/AGENT_AUTHORING_BUNDLE_v0.1.md"
ONBOARDING_FILE="docs/AGENT_ONBOARDING_v0.1.md"
INDEX_FILE="docs/INDEX.md"

[[ -f "$PACK_JSON" ]] || {
  echo "write-genesiscode-skill-pack: missing pack contract json: $PACK_JSON" >&2
  exit 1
}
[[ -f "$PACK_MD" ]] || {
  echo "write-genesiscode-skill-pack: missing pack markdown: $PACK_MD" >&2
  exit 1
}
[[ -f "$SKILL_FILE" ]] || {
  echo "write-genesiscode-skill-pack: missing canonical skill file: $SKILL_FILE" >&2
  exit 1
}
[[ -f "$POINTER_FILE" ]] || {
  echo "write-genesiscode-skill-pack: missing pointer doc: $POINTER_FILE" >&2
  exit 1
}
[[ -f "$BUNDLE_FILE" ]] || {
  echo "write-genesiscode-skill-pack: missing agent bundle doc: $BUNDLE_FILE" >&2
  exit 1
}
[[ -f "$ONBOARDING_FILE" ]] || {
  echo "write-genesiscode-skill-pack: missing onboarding doc: $ONBOARDING_FILE" >&2
  exit 1
}
[[ -f "$INDEX_FILE" ]] || {
  echo "write-genesiscode-skill-pack: missing docs index: $INDEX_FILE" >&2
  exit 1
}

python3 - "$PACK_JSON" "$PACK_MD" "$SKILL_FILE" "$POINTER_FILE" "$BUNDLE_FILE" "$ONBOARDING_FILE" "$INDEX_FILE" <<'PY'
import json
import pathlib
import sys

pack_json_path = pathlib.Path(sys.argv[1])
pack_md_path = pathlib.Path(sys.argv[2])
skill_path = pathlib.Path(sys.argv[3])
pointer_path = pathlib.Path(sys.argv[4])
bundle_path = pathlib.Path(sys.argv[5])
onboarding_path = pathlib.Path(sys.argv[6])
index_path = pathlib.Path(sys.argv[7])
root = pathlib.Path.cwd()

pack = json.loads(pack_json_path.read_text(encoding="utf-8"))
pack_md = pack_md_path.read_text(encoding="utf-8")
skill = skill_path.read_text(encoding="utf-8")
pointer = pointer_path.read_text(encoding="utf-8")
bundle = bundle_path.read_text(encoding="utf-8")
onboarding = onboarding_path.read_text(encoding="utf-8")
index = index_path.read_text(encoding="utf-8")

if pack.get("kind") != "genesis/write-genesiscode-skill-pack-v0.1":
    raise SystemExit(
        "write-genesiscode-skill-pack: invalid kind in docs/spec/WRITE_GENESISCODE_SKILL_PACK_v0.1.json"
    )

if pack.get("version") != "0.1":
    raise SystemExit(
        "write-genesiscode-skill-pack: contract version must be 0.1"
    )

if pack.get("pack_doc") != pack_md_path.as_posix():
    raise SystemExit(
        "write-genesiscode-skill-pack: pack_doc must match docs/spec/WRITE_GENESISCODE_SKILL_PACK_v0.1.md"
    )
if pack.get("pointer_doc") != pointer_path.as_posix():
    raise SystemExit(
        "write-genesiscode-skill-pack: pointer_doc must match docs/write_genesisCode_skill.md"
    )
if pack.get("skill_file") != skill_path.as_posix():
    raise SystemExit(
        "write-genesiscode-skill-pack: skill_file must match .agents/skills/genesiscode-authoring/SKILL.md"
    )
if pack.get("bundle_entrypoint") != bundle_path.as_posix():
    raise SystemExit(
        "write-genesiscode-skill-pack: bundle_entrypoint must match docs/spec/AGENT_AUTHORING_BUNDLE_v0.1.md"
    )

required_sections = pack.get("required_pack_sections", [])
missing_sections = [s for s in required_sections if s not in pack_md]
if missing_sections:
    raise SystemExit(
        "write-genesiscode-skill-pack: missing required section(s): "
        + ", ".join(missing_sections)
    )

required_gates = pack.get("required_gate_scripts", [])
missing_gate_files = [p for p in required_gates if not (root / p).is_file()]
if missing_gate_files:
    raise SystemExit(
        "write-genesiscode-skill-pack: missing required gate script(s): "
        + ", ".join(missing_gate_files)
    )
missing_gate_refs = [p for p in required_gates if p not in pack_md]
if missing_gate_refs:
    raise SystemExit(
        "write-genesiscode-skill-pack: pack markdown missing required gate reference(s): "
        + ", ".join(missing_gate_refs)
    )

required_spec_refs = pack.get("required_spec_refs", [])
missing_spec_files = [p for p in required_spec_refs if not (root / p).is_file()]
if missing_spec_files:
    raise SystemExit(
        "write-genesiscode-skill-pack: missing required spec/reference file(s): "
        + ", ".join(missing_spec_files)
    )
missing_spec_refs = [p for p in required_spec_refs if p not in pack_md]
if missing_spec_refs:
    raise SystemExit(
        "write-genesiscode-skill-pack: pack markdown missing required spec reference(s): "
        + ", ".join(missing_spec_refs)
    )

if skill_path.as_posix() not in pack_md:
    raise SystemExit(
        "write-genesiscode-skill-pack: pack markdown must reference canonical skill path"
    )
if pack_md_path.as_posix() not in bundle:
    raise SystemExit(
        "write-genesiscode-skill-pack: agent authoring bundle must include pack markdown path"
    )
if pack_json_path.as_posix() not in bundle:
    raise SystemExit(
        "write-genesiscode-skill-pack: agent authoring bundle must include pack contract json path"
    )

if pack_md_path.as_posix() not in pointer:
    raise SystemExit(
        "write-genesiscode-skill-pack: pointer doc must reference pack markdown path"
    )
if pack_json_path.as_posix() not in pointer:
    raise SystemExit(
        "write-genesiscode-skill-pack: pointer doc must reference pack contract json path"
    )

if "docs/spec/AGENT_AUTHORING_BUNDLE_v0.1.md" not in onboarding:
    raise SystemExit(
        "write-genesiscode-skill-pack: onboarding must reference AGENT_AUTHORING_BUNDLE_v0.1.md"
    )
if pack_md_path.as_posix() not in onboarding:
    raise SystemExit(
        "write-genesiscode-skill-pack: onboarding must reference WRITE_GENESISCODE_SKILL_PACK_v0.1.md"
    )

if pack_md_path.as_posix() not in index:
    raise SystemExit(
        "write-genesiscode-skill-pack: docs index must reference WRITE_GENESISCODE_SKILL_PACK_v0.1.md"
    )

dist_spec_path = "docs/spec/WRITE_GENESISCODE_SKILL_DISTRIBUTION_v1.md"
if dist_spec_path in required_spec_refs:
    if dist_spec_path not in bundle:
        raise SystemExit(
            "write-genesiscode-skill-pack: AGENT_AUTHORING_BUNDLE must include WRITE_GENESISCODE_SKILL_DISTRIBUTION_v1.md"
        )
    if dist_spec_path not in onboarding:
        raise SystemExit(
            "write-genesiscode-skill-pack: onboarding must reference WRITE_GENESISCODE_SKILL_DISTRIBUTION_v1.md"
        )
    if dist_spec_path not in index:
        raise SystemExit(
            "write-genesiscode-skill-pack: docs index must reference WRITE_GENESISCODE_SKILL_DISTRIBUTION_v1.md"
        )

print(
    "write-genesiscode-skill-pack: ok "
    f"(sections={len(required_sections)} gates={len(required_gates)} refs={len(required_spec_refs)})"
)
PY
