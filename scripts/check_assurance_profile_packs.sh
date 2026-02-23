#!/usr/bin/env bash
set -euo pipefail

ROOT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
cd "$ROOT_DIR"

PROFILE_FILE="policies/assurance/profile_packs.toml"
DOC_FILE="docs/spec/ASSURANCE_PROFILE_PACKS_v0.1.md"
BUNDLE_FILE="docs/spec/GCPM_BUNDLE_v0.1.md"

[[ -f "$PROFILE_FILE" ]] || {
  echo "assurance-profile-packs: missing profile file: $PROFILE_FILE" >&2
  exit 1
}
[[ -f "$DOC_FILE" ]] || {
  echo "assurance-profile-packs: missing doc file: $DOC_FILE" >&2
  exit 1
}
[[ -f "$BUNDLE_FILE" ]] || {
  echo "assurance-profile-packs: missing bundle file: $BUNDLE_FILE" >&2
  exit 1
}

python3 - "$PROFILE_FILE" "$DOC_FILE" "$BUNDLE_FILE" <<'PY'
import pathlib
import sys

try:
    import tomllib  # py311+
except ModuleNotFoundError:
    import tomli as tomllib  # type: ignore

profile_path = pathlib.Path(sys.argv[1])
doc_path = pathlib.Path(sys.argv[2])
bundle_path = pathlib.Path(sys.argv[3])

profiles = tomllib.loads(profile_path.read_text(encoding="utf-8"))
if profiles.get("version") != 1:
    raise SystemExit("assurance-profile-packs: profile_packs.toml must set version = 1")
if profiles.get("schema") != "genesis/assurance-profile-pack-v0.1":
    raise SystemExit(
        "assurance-profile-packs: profile_packs.toml must set schema = genesis/assurance-profile-pack-v0.1"
    )

profile_table = profiles.get("profile")
if not isinstance(profile_table, dict):
    raise SystemExit("assurance-profile-packs: missing [profile.*] table entries")

expected = {
    "custom": ("none", False, ":custom", False, False),
    "do178c-dal-a": ("mcdc", True, ":do178c-dal-a", True, True),
    "do178c-dal-b": ("decision", True, ":do178c-dal-b", True, True),
    "nasa-class-a": ("mcdc", True, ":nasa-class-a", True, True),
    "nasa-class-b": ("decision", True, ":nasa-class-b", True, True),
    "iec62304-class-c": ("symbol", False, ":iec62304-class-c", True, True),
}

actual_keys = set(profile_table.keys())
expected_keys = set(expected.keys())
missing = sorted(expected_keys - actual_keys)
extra = sorted(actual_keys - expected_keys)
if missing:
    raise SystemExit(
        "assurance-profile-packs: missing profile(s): " + ", ".join(missing)
    )
if extra:
    raise SystemExit(
        "assurance-profile-packs: unexpected profile(s): " + ", ".join(extra)
    )

for name, (coverage, independence, symbol, object_equivalence, independent_runs) in expected.items():
    entry = profile_table[name]
    if not isinstance(entry, dict):
        raise SystemExit(f"assurance-profile-packs: profile `{name}` must be a table")
    if entry.get("minimum_coverage_profile") != coverage:
        raise SystemExit(
            f"assurance-profile-packs: profile `{name}` minimum_coverage_profile must be `{coverage}`"
        )
    if entry.get("require_independence_attestations") is not independence:
        raise SystemExit(
            "assurance-profile-packs: profile "
            f"`{name}` require_independence_attestations must be {str(independence).lower()}"
        )
    if entry.get("target_profile_symbol") != symbol:
        raise SystemExit(
            f"assurance-profile-packs: profile `{name}` target_profile_symbol must be `{symbol}`"
        )
    if entry.get("require_requirements_trace") is not True:
        raise SystemExit(
            f"assurance-profile-packs: profile `{name}` must require requirements trace evidence"
        )
    if entry.get("require_tool_qualification") is not True:
        raise SystemExit(
            f"assurance-profile-packs: profile `{name}` must require tool qualification evidence"
        )
    if entry.get("require_object_equivalence") is not object_equivalence:
        raise SystemExit(
            "assurance-profile-packs: profile "
            f"`{name}` require_object_equivalence must be {str(object_equivalence).lower()}"
        )
    if entry.get("require_independent_verifier_runs") is not independent_runs:
        raise SystemExit(
            "assurance-profile-packs: profile "
            f"`{name}` require_independent_verifier_runs must be {str(independent_runs).lower()}"
        )

doc_text = doc_path.read_text(encoding="utf-8")
for name in sorted(expected.keys()):
    if f"`{name}`" not in doc_text:
        raise SystemExit(
            f"assurance-profile-packs: {doc_path.as_posix()} missing profile mention `{name}`"
        )
if profile_path.as_posix() not in doc_text:
    raise SystemExit(
        f"assurance-profile-packs: {doc_path.as_posix()} must reference {profile_path.as_posix()}"
    )

bundle_text = bundle_path.read_text(encoding="utf-8")
if doc_path.as_posix() not in bundle_text:
    raise SystemExit(
        f"assurance-profile-packs: {bundle_path.as_posix()} must include {doc_path.as_posix()}"
    )

print(
    "assurance-profile-packs: ok "
    f"(profiles={len(expected)} crosswalk={doc_path.as_posix()})"
)
PY
