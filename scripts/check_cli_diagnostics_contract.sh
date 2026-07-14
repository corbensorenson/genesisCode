#!/usr/bin/env bash
set -euo pipefail

source "$(dirname "${BASH_SOURCE[0]}")/lib/gate_telemetry.sh"
genesis_gate_telemetry_reexec "$0" "$@"

ROOT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
cd "$ROOT_DIR"

python3 scripts/lib/gc_diagnostic_catalog.py --check
python3 scripts/lib/gc_diagnostic_catalog.py --self-test
python3 scripts/lib/gc_diagnostic_goldens.py --check
python3 scripts/lib/gc_diagnostic_goldens.py --self-test
python3 scripts/lib/gc_repair_utility.py --check
python3 scripts/lib/gc_repair_utility.py --self-test
python3 - <<'PY'
import json
from pathlib import Path

schema_path = Path("docs/spec/GC_FAILURE_CONTEXT_v0.1.schema.json")
schema = json.loads(schema_path.read_text(encoding="utf-8"))
expected_domains = {
    "parser", "typechecker", "evaluator", "package", "policy",
    "replay", "patch", "build", "deployment",
}
required = {
    "schema", "domain", "kind", "operation", "facts",
    "primary_span", "related_spans",
}
actual_domains = set(schema["properties"]["domain"]["enum"])
actual_required = set(schema["required"])
if actual_domains != expected_domains:
    raise SystemExit(
        f"structured failure domain drift: expected {sorted(expected_domains)}, "
        f"got {sorted(actual_domains)}"
    )
if actual_required != required:
    raise SystemExit(
        f"structured failure required-field drift: expected {sorted(required)}, "
        f"got {sorted(actual_required)}"
    )
if schema.get("additionalProperties") is not False:
    raise SystemExit("structured failure schema must remain closed")
print("structured failure schema contract: ok")
PY

python3 - <<'PY'
import json
from pathlib import Path

schema = json.loads(
    Path("docs/spec/GC_DIAGNOSTIC_REPAIR_PLAN_v0.1.schema.json").read_text(encoding="utf-8")
)
required = {
    "schema", "diagnostic_id", "catalog_identity_sha256", "action",
    "guardrails", "authorization", "policy_diff",
}
if set(schema["required"]) != required or schema.get("additionalProperties") is not False:
    raise SystemExit("diagnostic repair-plan root contract drift")
authorization = schema["$defs"]["authorization"]
if authorization.get("additionalProperties") is not False:
    raise SystemExit("repair authorization must remain closed")
for field in ("policy_change_allowed", "obligation_suppression_allowed"):
    if authorization["properties"][field].get("const") is not False:
        raise SystemExit(f"repair authorization must forbid {field}")
policy_diff = schema["$defs"]["policyDiff"]["properties"]
if policy_diff["requires_review"].get("const") is not True:
    raise SystemExit("capability policy diff must require review")
if policy_diff["auto_apply"].get("const") is not False:
    raise SystemExit("capability policy diff must forbid auto apply")
print("diagnostic repair-plan schema contract: ok")
PY

BASELINE_INPUT_FILE="${GENESIS_CLI_DIAGNOSTICS_CONTRACT_HISTORY:-.genesis/perf/cli_diagnostics_contract_history.jsonl}"
TMP_DIR="$(mktemp -d)"
cleanup() {
  rm -rf "$TMP_DIR"
}
trap cleanup EXIT

bash scripts/render_gc_repair_utility_report.sh \
  "$TMP_DIR/gc_repair_utility_report.json"
cmp -s \
  benchmarks/diagnostics/repair_utility/v0.1/report.json \
  "$TMP_DIR/gc_repair_utility_report.json" || {
  echo "gc-repair-utility: checked-in report is stale; inspect and run scripts/update_gc_repair_utility_report.sh" >&2
  exit 1
}

bash scripts/render_cli_diagnostics_contract_report.sh \
  "$TMP_DIR/cli_diagnostics_contract_report.json" \
  "$TMP_DIR/cli_diagnostics_contract_history.jsonl" \
  "$BASELINE_INPUT_FILE"
