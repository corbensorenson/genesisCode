#!/usr/bin/env bash
set -euo pipefail

ROOT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
cd "$ROOT_DIR"

KIT_ROOT="${GENESIS_WRITE_SKILL_DIST_ROOT:-docs/skill_pack/write_genesiscode_v1}"
MANIFEST_PATH="${GENESIS_WRITE_SKILL_DIST_MANIFEST:-$KIT_ROOT/manifest.json}"
VERIFY_RUNTIME="${GENESIS_WRITE_SKILL_DIST_VERIFY_RUNTIME:-0}"
CONFORMANCE_AUTO_RUN="${GENESIS_WRITE_SKILL_DIST_CONFORMANCE_AUTO_RUN:-1}"

[[ -f "$MANIFEST_PATH" ]] || {
  echo "write-genesiscode-skill-distribution: missing manifest: $MANIFEST_PATH" >&2
  exit 1
}

python3 - "$MANIFEST_PATH" "$ROOT_DIR" "$KIT_ROOT" <<'PY'
import json
import pathlib
import sys

manifest_path = pathlib.Path(sys.argv[1])
root = pathlib.Path(sys.argv[2])
kit_root = pathlib.Path(sys.argv[3])
manifest = json.loads(manifest_path.read_text(encoding="utf-8"))

if manifest.get("kind") != "genesis/write-genesiscode-skill-distribution-v1":
    raise SystemExit(
        "write-genesiscode-skill-distribution: invalid manifest kind"
    )
if str(manifest.get("version")) != "1":
    raise SystemExit(
        "write-genesiscode-skill-distribution: manifest version must be '1'"
    )

prompts = manifest.get("prompts")
recipes = manifest.get("recipes")
expected_reports = manifest.get("expected_reports")
verification_scripts = manifest.get("verification_scripts")
requirements = manifest.get("distribution_requirements")

if not isinstance(requirements, dict):
    raise SystemExit("write-genesiscode-skill-distribution: distribution_requirements must be an object")
min_prompts = int(requirements.get("min_prompts", 1))
min_recipes = int(requirements.get("min_recipes", 1))
required_domains = requirements.get("required_recipe_domains", [])
require_fault_injection = bool(requirements.get("require_fault_injection_recipe", False))
min_report_score = int(requirements.get("min_report_score", 0))

if min_prompts <= 0:
    raise SystemExit("write-genesiscode-skill-distribution: min_prompts must be > 0")
if min_recipes <= 0:
    raise SystemExit("write-genesiscode-skill-distribution: min_recipes must be > 0")
if min_report_score < 0:
    raise SystemExit("write-genesiscode-skill-distribution: min_report_score must be >= 0")
if not isinstance(required_domains, list):
    raise SystemExit("write-genesiscode-skill-distribution: required_recipe_domains must be a list")
for d in required_domains:
    if not isinstance(d, str) or not d.strip():
        raise SystemExit("write-genesiscode-skill-distribution: required_recipe_domains entries must be non-empty strings")

if not isinstance(prompts, list) or not prompts:
    raise SystemExit("write-genesiscode-skill-distribution: prompts must be a non-empty list")
if not isinstance(recipes, list) or not recipes:
    raise SystemExit("write-genesiscode-skill-distribution: recipes must be a non-empty list")
if not isinstance(expected_reports, list) or not expected_reports:
    raise SystemExit("write-genesiscode-skill-distribution: expected_reports must be a non-empty list")
if not isinstance(verification_scripts, list) or not verification_scripts:
    raise SystemExit("write-genesiscode-skill-distribution: verification_scripts must be a non-empty list")

if len(prompts) < min_prompts:
    raise SystemExit(
        "write-genesiscode-skill-distribution: prompts below minimum: "
        f"{len(prompts)} < {min_prompts}"
    )
if len(recipes) < min_recipes:
    raise SystemExit(
        "write-genesiscode-skill-distribution: recipes below minimum: "
        f"{len(recipes)} < {min_recipes}"
    )

for item in prompts:
    if not isinstance(item, dict):
        raise SystemExit("write-genesiscode-skill-distribution: prompt entry must be an object")
    prompt_path = item.get("path")
    if not isinstance(prompt_path, str) or not prompt_path:
        raise SystemExit("write-genesiscode-skill-distribution: prompt path must be a non-empty string")
    full = kit_root / prompt_path
    if not full.is_file():
        raise SystemExit(
            f"write-genesiscode-skill-distribution: missing prompt file: {full.as_posix()}"
        )

seen_domains = set()
fault_injection_count = 0

for item in recipes:
    if not isinstance(item, dict):
        raise SystemExit("write-genesiscode-skill-distribution: recipe entry must be an object")
    recipe_path = item.get("path")
    workflow_path = item.get("workflow")
    domain = item.get("domain")
    mode = item.get("mode", "standard")
    if not isinstance(recipe_path, str) or not recipe_path:
        raise SystemExit("write-genesiscode-skill-distribution: recipe path must be a non-empty string")
    if not isinstance(workflow_path, str) or not workflow_path:
        raise SystemExit("write-genesiscode-skill-distribution: recipe workflow must be a non-empty string")
    if not isinstance(domain, str) or not domain:
        raise SystemExit("write-genesiscode-skill-distribution: recipe domain must be a non-empty string")
    if not isinstance(mode, str) or mode not in {"standard", "fault-injection"}:
        raise SystemExit(
            "write-genesiscode-skill-distribution: recipe mode must be 'standard' or 'fault-injection'"
        )
    seen_domains.add(domain)
    if mode == "fault-injection":
        fault_injection_count += 1
    recipe_full = kit_root / recipe_path
    if not recipe_full.is_file():
        raise SystemExit(
            f"write-genesiscode-skill-distribution: missing recipe file: {recipe_full.as_posix()}"
        )
    workflow_full = root / workflow_path
    if not workflow_full.is_file():
        raise SystemExit(
            f"write-genesiscode-skill-distribution: missing workflow script: {workflow_full.as_posix()}"
        )

missing_domains = [d for d in required_domains if d not in seen_domains]
if missing_domains:
    raise SystemExit(
        "write-genesiscode-skill-distribution: missing required recipe domains: "
        + ", ".join(missing_domains)
    )
if require_fault_injection and fault_injection_count == 0:
    raise SystemExit(
        "write-genesiscode-skill-distribution: require_fault_injection_recipe=true but no fault-injection recipe found"
    )

for script_path in verification_scripts:
    if not isinstance(script_path, str) or not script_path:
        raise SystemExit("write-genesiscode-skill-distribution: verification script path must be a non-empty string")
    full = root / script_path
    if not full.is_file():
        raise SystemExit(
            f"write-genesiscode-skill-distribution: missing verification script: {full.as_posix()}"
        )

for item in expected_reports:
    if not isinstance(item, dict):
        raise SystemExit("write-genesiscode-skill-distribution: expected_report entry must be an object")
    if "min_score" not in item:
        raise SystemExit("write-genesiscode-skill-distribution: expected_report entry must include min_score")
    report_min = int(item.get("min_score", 0))
    if report_min < min_report_score:
        raise SystemExit(
            "write-genesiscode-skill-distribution: expected_report min_score below distribution threshold: "
            f"{report_min} < {min_report_score}"
        )

print(
    "write-genesiscode-skill-distribution: manifest ok "
    f"(prompts={len(prompts)} recipes={len(recipes)} reports={len(expected_reports)})"
)
PY

if [[ "$VERIFY_RUNTIME" == "1" ]]; then
  GENESIS_WRITE_SKILL_CONFORMANCE_AUTO_RUN="$CONFORMANCE_AUTO_RUN" \
    bash scripts/check_write_genesiscode_skill_conformance.sh
  python3 - "$MANIFEST_PATH" <<'PY'
import json
import pathlib
import sys

manifest_path = pathlib.Path(sys.argv[1])
manifest = json.loads(manifest_path.read_text(encoding="utf-8"))
reports = manifest.get("expected_reports", [])

for item in reports:
    if not isinstance(item, dict):
        raise SystemExit("write-genesiscode-skill-distribution: expected_report entry must be an object")
    kind = item.get("kind")
    report_path_raw = item.get("path")
    min_score = int(item.get("min_score", 0))
    if not isinstance(kind, str) or not kind:
        raise SystemExit("write-genesiscode-skill-distribution: expected report kind must be a non-empty string")
    if not isinstance(report_path_raw, str) or not report_path_raw:
        raise SystemExit("write-genesiscode-skill-distribution: expected report path must be a non-empty string")
    report_path = pathlib.Path(report_path_raw)
    if not report_path.is_file():
        raise SystemExit(f"write-genesiscode-skill-distribution: expected report missing: {report_path}")
    report = json.loads(report_path.read_text(encoding="utf-8"))
    if report.get("kind") != kind:
        raise SystemExit(
            f"write-genesiscode-skill-distribution: expected kind {kind!r}, got {report.get('kind')!r}"
        )
    if int(report.get("score", 0)) < min_score:
        raise SystemExit(
            f"write-genesiscode-skill-distribution: score below minimum for {report_path}: "
            f"{report.get('score')} < {min_score}"
        )

print("write-genesiscode-skill-distribution: runtime verification ok")
PY
fi

echo "write-genesiscode-skill-distribution: ok"
