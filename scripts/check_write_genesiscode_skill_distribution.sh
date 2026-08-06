#!/usr/bin/env bash
set -euo pipefail

source "$(dirname "${BASH_SOURCE[0]}")/lib/gate_telemetry.sh"
genesis_gate_telemetry_reexec "$0" "$@"

ROOT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
cd "$ROOT_DIR"
source "$ROOT_DIR/scripts/lib/health_profile_evidence.sh"

KIT_ROOT="${GENESIS_WRITE_SKILL_DIST_ROOT:-docs/skill_pack/write_genesiscode_v1}"
MANIFEST_PATH="${GENESIS_WRITE_SKILL_DIST_MANIFEST:-$KIT_ROOT/manifest.json}"
VERIFY_RUNTIME="${GENESIS_WRITE_SKILL_DIST_VERIFY_RUNTIME:-0}"
CONFORMANCE_PROFILE="${GENESIS_WRITE_SKILL_CONFORMANCE_PROFILE:-${GENESIS_AGENT_GAUNTLET_PROFILE:-dev-fast}}"
GAUNTLET_INPUT="${GENESIS_WRITE_SKILL_GAUNTLET_REPORT:-.genesis/perf/agent_capability_gauntlet_report.json}"
GENERATIVE_INPUT="${GENESIS_WRITE_SKILL_GENERATIVE_REPORT:-.genesis/perf/agent_generative_workloads_report.json}"
RUNTIME_BACKEND_INPUT="${GENESIS_WRITE_SKILL_RUNTIME_BACKEND_REPORT:-.genesis/perf/runtime_backend_feature_matrix_report.json}"
HOST_BRIDGE_INPUT="${GENESIS_WRITE_SKILL_HOST_BRIDGE_REPORT:-.genesis/perf/host_bridge_fault_injection_report.json}"
GPU_XR_INPUT="${GENESIS_WRITE_SKILL_GPU_XR_REPORT:-.genesis/perf/gpu_xr_productization_kits_report.json}"
ASSURANCE_INPUT="${GENESIS_WRITE_SKILL_ASSURANCE_REPORT:-.genesis/perf/assurance_profile_packs_report.json}"

[[ -f "$MANIFEST_PATH" ]] || {
  echo "write-genesiscode-skill-distribution: missing manifest: $MANIFEST_PATH" >&2
  exit 1
}

python3 scripts/lib/genesiscode_authoring_skill.py --check --self-test

python3 - "$MANIFEST_PATH" "$ROOT_DIR" "$KIT_ROOT" <<'PY'
from hashlib import sha256
import json
import pathlib
import sys

manifest_path = pathlib.Path(sys.argv[1])
root = pathlib.Path(sys.argv[2])
kit_root = pathlib.Path(sys.argv[3])
manifest = json.loads(manifest_path.read_text(encoding="utf-8"))

def fail(message):
    raise SystemExit("write-genesiscode-skill-distribution: " + message)

def load_cards(key, expected_kind):
    relative = manifest.get(key)
    if not isinstance(relative, str) or not relative:
        fail(f"{key} must identify a card registry")
    path = kit_root / relative
    if not path.is_file():
        fail(f"missing card registry: {path.as_posix()}")
    document = json.loads(path.read_text(encoding="utf-8"))
    if document.get("kind") != expected_kind or str(document.get("version")) != "1":
        fail(f"invalid card registry identity: {path.as_posix()}")
    if document.get("source") != manifest.get("source") or document.get("source_sha256") != manifest.get("source_sha256"):
        fail(f"card registry source identity drift: {path.as_posix()}")
    cards = document.get("cards")
    if not isinstance(cards, list) or not cards:
        fail(f"empty card registry: {path.as_posix()}")
    return cards

if manifest.get("kind") != "genesis/write-genesiscode-skill-distribution-v1":
    raise SystemExit(
        "write-genesiscode-skill-distribution: invalid manifest kind"
    )
if str(manifest.get("version")) != "1":
    raise SystemExit(
        "write-genesiscode-skill-distribution: manifest version must be '1'"
    )

source = manifest.get("source")
if not isinstance(source, str) or not source or not (root / source).is_file():
    fail("manifest source is missing")
actual_source_sha = sha256((root / source).read_bytes()).hexdigest()
if manifest.get("source_sha256") != actual_source_sha:
    fail("manifest source identity is stale")

prompt_cards = load_cards("prompt_cards", "genesis/write-genesiscode-prompt-cards-v1")
recipe_cards = load_cards("recipe_cards", "genesis/write-genesiscode-recipe-cards-v1")
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

prompt_ids = [item.get("id") for item in prompts if isinstance(item, dict)]
card_prompt_ids = [item.get("id") for item in prompt_cards if isinstance(item, dict)]
if prompt_ids != card_prompt_ids or len(prompt_ids) != len(set(prompt_ids)):
    fail("prompt inventory and generated cards disagree")

seen_domains = set()
fault_injection_count = 0

for item in recipes:
    if not isinstance(item, dict):
        raise SystemExit("write-genesiscode-skill-distribution: recipe entry must be an object")
    workflow_path = item.get("workflow")
    domain = item.get("domain")
    mode = item.get("mode", "standard")
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
    workflow_full = root / workflow_path
    if not workflow_full.is_file():
        raise SystemExit(
            f"write-genesiscode-skill-distribution: missing workflow script: {workflow_full.as_posix()}"
        )

recipe_ids = [item.get("id") for item in recipes if isinstance(item, dict)]
card_recipe_ids = [item.get("id") for item in recipe_cards if isinstance(item, dict)]
if recipe_ids != card_recipe_ids or len(recipe_ids) != len(set(recipe_ids)):
    fail("recipe inventory and generated cards disagree")

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
    kind = item.get("kind")
    report_path = item.get("path")
    if not isinstance(kind, str) or not kind:
        raise SystemExit("write-genesiscode-skill-distribution: expected report kind must be a non-empty string")
    if not isinstance(report_path, str) or not report_path:
        raise SystemExit("write-genesiscode-skill-distribution: expected report path must be a non-empty string")
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
  if [[ "$CONFORMANCE_PROFILE" == "release-full" ]]; then
    [[ "${GENESIS_HEALTH_EVIDENCE_REQUIRED:-0}" == "1" ]] || {
      echo "write-genesiscode-skill-distribution: release-full runtime verification requires private evidence" >&2
      exit 1
    }
    [[ -n "${GENESIS_HEALTH_EVIDENCE_MANIFEST:-}" ]] || {
      echo "write-genesiscode-skill-distribution: release-full runtime verification requires an evidence manifest" >&2
      exit 1
    }
    for binding in \
      GENESIS_WRITE_SKILL_GAUNTLET_REPORT \
      GENESIS_WRITE_SKILL_GENERATIVE_REPORT \
      GENESIS_WRITE_SKILL_RUNTIME_BACKEND_REPORT \
      GENESIS_WRITE_SKILL_HOST_BRIDGE_REPORT \
      GENESIS_WRITE_SKILL_GPU_XR_REPORT \
      GENESIS_WRITE_SKILL_ASSURANCE_REPORT; do
      [[ -n "${!binding:-}" ]] || {
        echo "write-genesiscode-skill-distribution: release-full runtime verification requires $binding" >&2
        exit 1
      }
    done
  fi
  if [[ -n "${GENESIS_HEALTH_EVIDENCE_MANIFEST:-}" ]]; then
    genesis_verify_health_profile_evidence \
      "write-skill-distribution" \
      "scripts/check_write_genesiscode_skill_distribution.sh" \
      "$GAUNTLET_INPUT" \
      "$GENERATIVE_INPUT" \
      "$ASSURANCE_INPUT" \
      "$GPU_XR_INPUT" \
      "$HOST_BRIDGE_INPUT" \
      "$RUNTIME_BACKEND_INPUT"
  fi
  GENESIS_WRITE_SKILL_CONFORMANCE_PROFILE="$CONFORMANCE_PROFILE" \
    bash scripts/check_write_genesiscode_skill_conformance.sh
  if [[ "$CONFORMANCE_PROFILE" == "release-full" ]]; then
    echo "write-genesiscode-skill-distribution: runtime verification ok"
  else
    python3 - "$MANIFEST_PATH" <<'PY'
import json
import pathlib
import sys

manifest_path = pathlib.Path(sys.argv[1])
manifest = json.loads(manifest_path.read_text(encoding="utf-8"))
for item in manifest.get("expected_reports", []):
    report_path = pathlib.Path(item["path"])
    if not report_path.is_file():
        raise SystemExit(
            f"write-genesiscode-skill-distribution: expected report missing: {report_path}; "
            "produce it with: bash scripts/update_write_genesiscode_skill_conformance_report.sh"
        )
    report = json.loads(report_path.read_text(encoding="utf-8"))
    if report.get("kind") != item["kind"]:
        raise SystemExit(
            f"write-genesiscode-skill-distribution: expected kind {item['kind']!r}, "
            f"got {report.get('kind')!r}"
        )
    if int(report.get("score", 0)) < int(item["min_score"]):
        raise SystemExit(
            f"write-genesiscode-skill-distribution: score below minimum for {report_path}: "
            f"{report.get('score')} < {item['min_score']}"
        )
    if report.get("ok") is not True:
        raise SystemExit(
            f"write-genesiscode-skill-distribution: retained report is not ok: {report_path}"
        )

print("write-genesiscode-skill-distribution: runtime verification ok")
PY
  fi
fi

echo "write-genesiscode-skill-distribution: ok"
