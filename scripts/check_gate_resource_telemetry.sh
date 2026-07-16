#!/usr/bin/env bash
set -euo pipefail

source "$(dirname "${BASH_SOURCE[0]}")/lib/gate_telemetry.sh"
genesis_gate_telemetry_reexec "$0" "$@"

ROOT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
cd "$ROOT_DIR"

TMP_DIR="$(mktemp -d "${TMPDIR:-/tmp}/genesis-gate-telemetry.XXXXXX")"
trap 'rm -rf "$TMP_DIR"' EXIT

PYTHONDONTWRITEBYTECODE=1 GENESIS_GATE_TELEMETRY_DISABLE=1 python3 - "$ROOT_DIR" "$TMP_DIR" <<'PY'
import copy
from hashlib import sha256
import json
import os
from pathlib import Path
import subprocess
import sys
import time

root = Path(sys.argv[1]).resolve()
temp = Path(sys.argv[2])
sys.path.insert(0, str(root / "scripts/lib"))
import gate_telemetry as telemetry

controls = []

def require(value, message):
    if not value:
        raise SystemExit(f"gate-resource-telemetry: {message}")

policy = telemetry.load_policy(root)
require(
    policy["exactDiskEntrypoints"]
    == [
        "scripts/check_agent_authoring_bundle.sh",
        "scripts/check_cargo_target_dir_policy.sh",
        "scripts/check_check_update_boundary.sh",
        "scripts/check_docs_quickstart.sh",
        "scripts/check_gate_manifest.sh",
        "scripts/check_gate_resource_telemetry.sh",
    ]
    and telemetry.exact_disk_enabled(policy, "scripts/check_agent_authoring_bundle.sh", None)
    and telemetry.exact_disk_enabled(policy, "scripts/check_cargo_target_dir_policy.sh", None)
    and telemetry.exact_disk_enabled(policy, "scripts/check_check_update_boundary.sh", None)
    and telemetry.exact_disk_enabled(policy, "scripts/check_docs_quickstart.sh", None)
    and telemetry.exact_disk_enabled(policy, "scripts/check_gate_resource_telemetry.sh", None)
    and telemetry.exact_disk_enabled(policy, "scripts/check_gate_manifest.sh", None)
    and not telemetry.exact_disk_enabled(policy, "scripts/check_doc_hygiene.sh", None)
    and telemetry.exact_disk_enabled(policy, "scripts/check_doc_hygiene.sh", "1"),
    "exact logical-disk policy drift",
)
try:
    telemetry.exact_disk_enabled(policy, "scripts/check_doc_hygiene.sh", "yes")
except telemetry.TelemetryError:
    pass
else:
    raise SystemExit("gate-resource-telemetry: malformed exact-disk override accepted")
controls.append("entrypoint-scoped-exact-disk-accounting")
require(
    telemetry.sample_interval_ms(policy, {"boundaryClass": "static"}) == policy["sampleIntervalMs"]
    and telemetry.sample_interval_ms(policy, {"boundaryClass": "aggregate"}) == policy["aggregateSampleIntervalMs"]
    and policy["aggregateSampleIntervalMs"] >= 10 * policy["sampleIntervalMs"],
    "aggregate observer cadence is not low-perturbation",
)
controls.append("aggregate-low-perturbation-sampling")
schema_path = root / "docs/spec/GATE_RESOURCE_TELEMETRY_v0.1.schema.json"
schema = telemetry.load_json(schema_path)
require(
    schema.get("$schema") == "https://json-schema.org/draft/2020-12/schema"
    and schema.get("$id") == "https://genesiscode.dev/schemas/gate-resource-telemetry-v0.1.json"
    and schema.get("additionalProperties") is False,
    "schema identity/closure drift",
)
controls.append("schema-policy-closure")

report = temp / "pass.json"
start = time.monotonic()
event_env = dict(os.environ, GENESIS_GATE_BUDGET_ENFORCE="0")
proc = subprocess.run([
    sys.executable, str(root / "scripts/lib/gate_telemetry.py"),
    "--root", str(root), "--entrypoint", "scripts/check_doc_hygiene.sh",
    "--out", str(report), "--emit", "none", "--",
    "bash", "scripts/check_doc_hygiene.sh",
], cwd=root, env=event_env)
require(proc.returncode == 0 and time.monotonic() - start < 5, "passing gate measurement failed or exceeded observer envelope")
doc = telemetry.load_json(report)
require(set(doc) == {"gate", "kind", "metrics", "platform", "result", "version"}, "telemetry top-level fields drift")
required_metrics = {"durationNs", "peakRssBytes", "ioReadBytes", "ioWriteBytes", "generatedDiskDeltaBytes", "cacheHits", "networkAttempts"}
require(set(doc["metrics"]) == required_metrics, "metric coverage drift")
require(doc["result"] == {"exitCode": 0, "status": "passed"}, "pass status drift")
require(doc["metrics"]["durationNs"]["value"] > 0, "duration was not measured")
require(doc["metrics"]["peakRssBytes"]["value"] > 0, "peak RSS was not measured")
require(str(root) not in json.dumps(doc, sort_keys=True), "telemetry leaked checkout path")
expected_units = {
    "durationNs": "nanoseconds",
    "peakRssBytes": "bytes",
    "ioReadBytes": "bytes",
    "ioWriteBytes": "bytes",
    "generatedDiskDeltaBytes": "bytes",
    "cacheHits": "count",
    "networkAttempts": "count",
}
qualities = {"exact", "instrumented", "sampled", "estimated", "unavailable"}
for name, value in doc["metrics"].items():
    require(set(value) == {"completeness", "method", "unit", "value"}, f"metric fields drift: {name}")
    require(isinstance(value["value"], int) and not isinstance(value["value"], bool), f"metric value type drift: {name}")
    require(value["unit"] == expected_units[name] and value["completeness"] in qualities and value["method"], f"metric contract drift: {name}")
require(doc["metrics"]["durationNs"]["completeness"] == "exact", "duration fidelity drift")
for name in ("cacheHits", "networkAttempts"):
    require(doc["metrics"][name]["completeness"] == "instrumented", f"event fidelity drift: {name}")
controls.append("passing-gate-complete-shape")

nested = subprocess.run([
    sys.executable, str(root / "scripts/lib/gate_telemetry.py"),
    "--root", str(root), "--entrypoint", "scripts/check_gate_resource_telemetry.sh",
    "--emit", "none", "--", "bash", "scripts/check_doc_hygiene.sh",
], cwd=root, env={key: value for key, value in os.environ.items() if key != "GENESIS_GATE_TELEMETRY_DISABLE"}, text=True, stdout=subprocess.PIPE, stderr=subprocess.PIPE)
nested_lines = [line for line in nested.stderr.splitlines() if line.startswith("gate-telemetry: ")]
require(nested.returncode == 0 and len(nested_lines) == 1, "nested governed gate did not emit exactly one observation")
nested_doc = json.loads(nested_lines[0].removeprefix("gate-telemetry: "), object_pairs_hook=telemetry.unique_pairs)
require(nested_doc["gate"]["entrypoint"] == "scripts/check_doc_hygiene.sh", "nested observation bound the wrong gate")
controls.append("nested-gate-observation")

event_report = temp / "events.json"
event_command = 'printf \'{"count":2,"kind":"cache-hit"}\\n{"count":3,"kind":"network-attempt"}\\n\' >>"$GENESIS_GATE_TELEMETRY_EVENT_FILE"'
proc = subprocess.run([
    sys.executable, str(root / "scripts/lib/gate_telemetry.py"),
    "--root", str(root), "--entrypoint", "scripts/check_doc_hygiene.sh",
    "--out", str(event_report), "--emit", "none", "--", "bash", "-c", event_command,
], cwd=root, env=event_env)
events = telemetry.load_json(event_report)
require(proc.returncode == 0 and events["metrics"]["cacheHits"]["value"] == 2 and events["metrics"]["networkAttempts"]["value"] == 3, "event channel count drift")
controls.append("explicit-event-accounting")

network_budget_report = temp / "network-budget.json"
proc = subprocess.run([
    sys.executable, str(root / "scripts/lib/gate_telemetry.py"),
    "--root", str(root), "--entrypoint", "scripts/check_doc_hygiene.sh",
    "--out", str(network_budget_report), "--emit", "none", "--", "bash", "-c",
    'printf \'{"count":1,"kind":"network-attempt"}\\n\' >>"$GENESIS_GATE_TELEMETRY_EVENT_FILE"',
], cwd=root, env={key: value for key, value in os.environ.items() if key != "GENESIS_GATE_BUDGET_ENFORCE"}, stdout=subprocess.DEVNULL, stderr=subprocess.DEVNULL)
network_budget = telemetry.load_json(network_budget_report)
require(proc.returncode == 3 and network_budget["result"] == {"exitCode": 3, "status": "failed"}, "deny-network budget was not enforced")
controls.append("deny-network-budget-enforcement")

failure_report = temp / "failure.json"
proc = subprocess.run([
    sys.executable, str(root / "scripts/lib/gate_telemetry.py"),
    "--root", str(root), "--entrypoint", "scripts/check_doc_hygiene.sh",
    "--out", str(failure_report), "--emit", "none", "--", "bash", "-c", "exit 7",
], cwd=root)
failure = telemetry.load_json(failure_report)
require(proc.returncode == 7 and failure["result"] == {"exitCode": 7, "status": "failed"}, "failure status was not preserved")
controls.append("failure-exit-preservation")

signal_report = temp / "signal.json"
proc = subprocess.run([
    sys.executable, str(root / "scripts/lib/gate_telemetry.py"),
    "--root", str(root), "--entrypoint", "scripts/check_doc_hygiene.sh",
    "--out", str(signal_report), "--emit", "none", "--", "bash", "-c", "kill -TERM $$",
], cwd=root)
signaled = telemetry.load_json(signal_report)
require(proc.returncode == 143 and signaled["result"] == {"exitCode": 143, "status": "signaled"}, "signaled exit was not preserved")
controls.append("signaled-exit-preservation")

cache_report = temp / "cache.json"
cache_command = "source scripts/lib/gate_telemetry.sh; source scripts/lib/cargo_target_dir.sh; genesis_configure_cargo_target_dir \"$PWD\" telemetry-cache root-host >/dev/null; genesis_configure_cargo_target_dir \"$PWD\" telemetry-cache root-host >/dev/null"
proc = subprocess.run([
    sys.executable, str(root / "scripts/lib/gate_telemetry.py"),
    "--root", str(root), "--entrypoint", "scripts/check_doc_hygiene.sh",
    "--out", str(cache_report), "--emit", "none", "--", "bash", "-c", cache_command,
], cwd=root, env=event_env)
cache = telemetry.load_json(cache_report)
require(proc.returncode == 0 and cache["metrics"]["cacheHits"]["value"] >= 1, "Cargo cache reuse event was not observed")
controls.append("cargo-cache-hit-instrumentation")

disk_root = temp / "disk"
disk_root.mkdir()
before = telemetry.logical_size(disk_root)
(disk_root / "payload").write_bytes(b"x" * 4096)
after = telemetry.logical_size(disk_root)
require(after - before == 4096, "logical generated-disk delta drift")
controls.append("exact-logical-disk-delta")

bad_event = temp / "bad-events.jsonl"
bad_event.write_text('{"count":0,"kind":"cache-hit"}\n', encoding="utf-8")
try:
    telemetry.event_counts(bad_event, policy)
except telemetry.TelemetryError:
    pass
else:
    raise SystemExit("gate-resource-telemetry: invalid event accepted")
controls.append("invalid-event-rejection")

malformed_temp = temp / "malformed-channel"
malformed_temp.mkdir()
malformed_env = dict(os.environ, TMPDIR=str(malformed_temp))
proc = subprocess.run([
    sys.executable, str(root / "scripts/lib/gate_telemetry.py"),
    "--root", str(root), "--entrypoint", "scripts/check_doc_hygiene.sh",
    "--emit", "none", "--", "bash", "-c", 'printf \'{"count":0,"kind":"cache-hit"}\\n\' >>"$GENESIS_GATE_TELEMETRY_EVENT_FILE"',
], cwd=root, env=malformed_env, stdout=subprocess.DEVNULL, stderr=subprocess.DEVNULL)
require(proc.returncode == 2 and not list(malformed_temp.glob("genesis-gate-events.*")), "malformed event did not fail closed and clean up")
controls.append("malformed-channel-fail-closed")

launch_temp = temp / "launch-failure"
launch_temp.mkdir()
launch_env = dict(os.environ, TMPDIR=str(launch_temp))
proc = subprocess.run([
    sys.executable, str(root / "scripts/lib/gate_telemetry.py"),
    "--root", str(root), "--entrypoint", "scripts/check_doc_hygiene.sh",
    "--emit", "none", "--", "genesis-telemetry-command-that-does-not-exist",
], cwd=root, env=launch_env, stdout=subprocess.DEVNULL, stderr=subprocess.DEVNULL)
require(proc.returncode == 2 and not list(launch_temp.glob("genesis-gate-events.*")), "launch failure leaked event channel")
controls.append("launch-failure-cleanup")

duplicate = temp / "duplicate.json"
duplicate.write_text('{"kind":"a","kind":"b"}\n', encoding="utf-8")
try:
    telemetry.load_json(duplicate)
except telemetry.TelemetryError:
    pass
else:
    raise SystemExit("gate-resource-telemetry: duplicate key accepted")
controls.append("duplicate-key-rejection")

for bad in ("../escape", "/tmp/escape", "./scripts/check_doc_hygiene.sh", "scripts\\bad.sh"):
    try:
        telemetry.repo_path(bad, "fixture")
    except telemetry.TelemetryError:
        pass
    else:
        raise SystemExit(f"gate-resource-telemetry: noncanonical path accepted: {bad}")
controls.append("noncanonical-path-rejection")

check_scripts = sorted((root / "scripts").glob("check_*.sh"))
missing = []
for path in check_scripts:
    lines = path.read_text(encoding="utf-8").splitlines()
    if lines[3:5] != [
        'source "$(dirname "${BASH_SOURCE[0]}")/lib/gate_telemetry.sh"',
        'genesis_gate_telemetry_reexec "$0" "$@"',
    ]:
        missing.append(path.name)
require(not missing, f"governed checks missing telemetry wrapper: {missing}")
controls.append("complete-gate-wrapper-coverage")

require(len(controls) == 17 and len(set(controls)) == 17, "control coverage drift")
authorities = [
    "policies/gate_telemetry_v0.1.json",
    "docs/spec/GATE_RESOURCE_TELEMETRY_v0.1.schema.json",
    "scripts/lib/gate_telemetry.py",
    "scripts/lib/gate_telemetry.sh",
    "scripts/check_gate_resource_telemetry.sh",
]
digest = sha256()
for path in authorities:
    path_bytes = path.encode("utf-8")
    content = (root / path).read_bytes()
    digest.update(len(path_bytes).to_bytes(8, "big"))
    digest.update(path_bytes)
    digest.update(len(content).to_bytes(8, "big"))
    digest.update(content)
bundle = digest.hexdigest()
print(f"gate-resource-telemetry: ok (gates={len(check_scripts)} metrics=7 controls={len(controls)} bundle={bundle})")
PY
