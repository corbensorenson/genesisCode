#!/usr/bin/env bash
set -euo pipefail

ROOT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
cd "$ROOT_DIR"

PROFILE_FILE="policies/assurance/profile_packs.toml"
CROSSWALK_JSON="docs/spec/ASSURANCE_STANDARDS_CROSSWALK_v0.1.json"
CROSSWALK_MD="docs/spec/ASSURANCE_STANDARDS_CROSSWALK_v0.1.md"
PROFILE_DOC="docs/spec/ASSURANCE_PROFILE_PACKS_v0.1.md"
BUNDLE_DOC="docs/spec/GCPM_BUNDLE_v0.1.md"

for path in "$PROFILE_FILE" "$CROSSWALK_JSON" "$CROSSWALK_MD" "$PROFILE_DOC" "$BUNDLE_DOC"; do
  [[ -f "$path" ]] || {
    echo "assurance-standards-crosswalk: missing required file: $path" >&2
    exit 1
  }
done

python3 - "$PROFILE_FILE" "$CROSSWALK_JSON" "$CROSSWALK_MD" "$PROFILE_DOC" "$BUNDLE_DOC" <<'PY'
import json
import pathlib
import sys

try:
    import tomllib  # py311+
except ModuleNotFoundError:
    import tomli as tomllib  # type: ignore

profile_path = pathlib.Path(sys.argv[1])
crosswalk_json_path = pathlib.Path(sys.argv[2])
crosswalk_md_path = pathlib.Path(sys.argv[3])
profile_doc_path = pathlib.Path(sys.argv[4])
bundle_doc_path = pathlib.Path(sys.argv[5])
root = pathlib.Path.cwd()

profiles = tomllib.loads(profile_path.read_text(encoding="utf-8"))
profile_table = profiles.get("profile")
if not isinstance(profile_table, dict):
    raise SystemExit("assurance-standards-crosswalk: profile_packs.toml missing [profile.*] table")

regulated_profiles = {
    key for key in profile_table.keys()
    if key != "custom"
}

crosswalk = json.loads(crosswalk_json_path.read_text(encoding="utf-8"))
if crosswalk.get("kind") != "genesis/assurance-standards-crosswalk-v0.1":
    raise SystemExit(
        "assurance-standards-crosswalk: invalid kind in docs/spec/ASSURANCE_STANDARDS_CROSSWALK_v0.1.json"
    )
if crosswalk.get("version") != "0.1":
    raise SystemExit(
        "assurance-standards-crosswalk: version must be 0.1"
    )

sources = crosswalk.get("sources")
if not isinstance(sources, dict):
    raise SystemExit("assurance-standards-crosswalk: sources must be an object")
for required_key in (
    "profile_pack_policy",
    "assurance_profile_packs_doc",
    "assurance_artifacts_doc",
    "cli_contract_doc",
):
    if required_key not in sources:
        raise SystemExit(
            f"assurance-standards-crosswalk: missing sources.{required_key}"
        )
    ref = sources[required_key]
    if not isinstance(ref, str) or not (root / ref).is_file():
        raise SystemExit(
            f"assurance-standards-crosswalk: sources.{required_key} must reference an existing file: {ref}"
        )

outputs = crosswalk.get("profile_pack_outputs")
if not isinstance(outputs, list) or not outputs:
    raise SystemExit(
        "assurance-standards-crosswalk: profile_pack_outputs must be a non-empty list"
    )
expected_outputs = {
    "assurance_pack.gc",
    "requirements_trace.gc",
    "tool_qualification.gc",
    "coverage/*.gc",
    "object_equivalence.gc",
    "independent_verifier/*.gc",
    "bundle_manifest.gc",
}
if set(outputs) != expected_outputs:
    raise SystemExit(
        "assurance-standards-crosswalk: profile_pack_outputs must exactly match expected deterministic bundle members"
    )

entries = crosswalk.get("profiles")
if not isinstance(entries, list) or not entries:
    raise SystemExit("assurance-standards-crosswalk: profiles must be a non-empty list")

seen_profiles = set()
unresolved_count = 0
allowed_status = {"covered-by-toolchain", "partial", "external"}
allowed_unresolved_status = {"open", "program-backlog"}
for entry in entries:
    if not isinstance(entry, dict):
        raise SystemExit("assurance-standards-crosswalk: each profiles entry must be an object")
    profile = entry.get("assurance_profile")
    if not isinstance(profile, str):
        raise SystemExit("assurance-standards-crosswalk: each profile entry needs assurance_profile string")
    if profile in seen_profiles:
        raise SystemExit(f"assurance-standards-crosswalk: duplicate assurance_profile entry: {profile}")
    seen_profiles.add(profile)

    objectives = entry.get("objectives")
    if not isinstance(objectives, list) or not objectives:
        raise SystemExit(
            f"assurance-standards-crosswalk: profile `{profile}` must declare non-empty objectives"
        )
    for obj in objectives:
        if not isinstance(obj, dict):
            raise SystemExit(
                f"assurance-standards-crosswalk: profile `{profile}` has non-object objective"
            )
        for key in ("objective_id", "summary", "status", "evidence_refs"):
            if key not in obj:
                raise SystemExit(
                    f"assurance-standards-crosswalk: profile `{profile}` objective missing `{key}`"
                )
        if obj["status"] not in allowed_status:
            raise SystemExit(
                f"assurance-standards-crosswalk: profile `{profile}` objective `{obj.get('objective_id')}` has invalid status `{obj['status']}`"
            )
        refs = obj["evidence_refs"]
        if not isinstance(refs, list) or not refs:
            raise SystemExit(
                f"assurance-standards-crosswalk: profile `{profile}` objective `{obj.get('objective_id')}` must provide evidence_refs"
            )
        for ref in refs:
            if not isinstance(ref, str):
                raise SystemExit(
                    f"assurance-standards-crosswalk: profile `{profile}` objective `{obj.get('objective_id')}` has non-string evidence ref"
                )
            path_part = ref.split("#", 1)[0]
            if path_part and not (root / path_part).is_file():
                raise SystemExit(
                    f"assurance-standards-crosswalk: missing evidence ref path `{path_part}` for profile `{profile}` objective `{obj.get('objective_id')}`"
                )

    unresolved = entry.get("unresolved_controls")
    if not isinstance(unresolved, list) or not unresolved:
        raise SystemExit(
            f"assurance-standards-crosswalk: profile `{profile}` must declare unresolved_controls (explicit non-claims)"
        )
    for control in unresolved:
        if not isinstance(control, dict):
            raise SystemExit(
                f"assurance-standards-crosswalk: profile `{profile}` has non-object unresolved control"
            )
        for key in ("control_id", "summary", "status", "owner", "tracked_in"):
            if key not in control:
                raise SystemExit(
                    f"assurance-standards-crosswalk: unresolved control in `{profile}` missing `{key}`"
                )
        if control["status"] not in allowed_unresolved_status:
            raise SystemExit(
                f"assurance-standards-crosswalk: unresolved control `{control.get('control_id')}` in `{profile}` has invalid status `{control['status']}`"
            )
        tracked_in = str(control.get("tracked_in", ""))
        tracked_path = tracked_in.split("#", 1)[0]
        if not tracked_path or not (root / tracked_path).is_file():
            raise SystemExit(
                f"assurance-standards-crosswalk: unresolved control `{control.get('control_id')}` in `{profile}` must track to an existing document via tracked_in"
            )
        if control["status"] == "program-backlog":
            if tracked_path != "docs/program/ASSURANCE_PROGRAM_BACKLOG_v0.1.md":
                raise SystemExit(
                    f"assurance-standards-crosswalk: program-backlog control `{control.get('control_id')}` in `{profile}` must track to docs/program/ASSURANCE_PROGRAM_BACKLOG_v0.1.md"
                )
        unresolved_count += 1

missing_profiles = sorted(regulated_profiles - seen_profiles)
extra_profiles = sorted(seen_profiles - regulated_profiles)
if missing_profiles:
    raise SystemExit(
        "assurance-standards-crosswalk: missing regulated profile entries: "
        + ", ".join(missing_profiles)
    )
if extra_profiles:
    raise SystemExit(
        "assurance-standards-crosswalk: crosswalk has profiles not present in policy pack: "
        + ", ".join(extra_profiles)
    )

global_non_claims = crosswalk.get("global_non_claims")
if not isinstance(global_non_claims, list) or len(global_non_claims) < 2:
    raise SystemExit(
        "assurance-standards-crosswalk: global_non_claims must contain explicit non-claim statements"
    )

md = crosswalk_md_path.read_text(encoding="utf-8")
for token in (
    "Objective Matrix (Toolchain Posture)",
    "Unresolved Controls (Explicit Non-Claims)",
    "Not a Certification Claim",
    crosswalk_json_path.as_posix(),
):
    if token not in md:
        raise SystemExit(
            f"assurance-standards-crosswalk: markdown doc missing required token: {token}"
        )
for output in expected_outputs:
    if output not in md:
        raise SystemExit(
            f"assurance-standards-crosswalk: markdown doc must enumerate profile-pack output `{output}`"
        )
for profile in sorted(regulated_profiles):
    if f"`{profile}`" not in md:
        raise SystemExit(
            f"assurance-standards-crosswalk: markdown doc missing regulated profile mention `{profile}`"
        )

profile_doc = profile_doc_path.read_text(encoding="utf-8")
if crosswalk_md_path.as_posix() not in profile_doc:
    raise SystemExit(
        "assurance-standards-crosswalk: ASSURANCE_PROFILE_PACKS doc must reference standards crosswalk markdown"
    )
if crosswalk_json_path.as_posix() not in profile_doc:
    raise SystemExit(
        "assurance-standards-crosswalk: ASSURANCE_PROFILE_PACKS doc must reference standards crosswalk schema json"
    )

bundle_doc = bundle_doc_path.read_text(encoding="utf-8")
for path in (crosswalk_md_path.as_posix(), crosswalk_json_path.as_posix()):
    if path not in bundle_doc:
        raise SystemExit(
            f"assurance-standards-crosswalk: GCPM bundle must include {path}"
        )

print(
    "assurance-standards-crosswalk: ok "
    f"(profiles={len(seen_profiles)} unresolved_controls={unresolved_count})"
)
PY
