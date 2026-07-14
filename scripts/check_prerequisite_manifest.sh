#!/usr/bin/env bash
set -euo pipefail

source "$(dirname "${BASH_SOURCE[0]}")/lib/gate_telemetry.sh"
genesis_gate_telemetry_reexec "$0" "$@"

ROOT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
cd "$ROOT_DIR"

MANIFEST="genesis.prerequisites.json"
SCHEMA="docs/spec/PREREQUISITES_v0.1.schema.json"
IMPLEMENTATION="scripts/lib/prerequisite_manifest.py"
TMP_DIR="$(mktemp -d "${TMPDIR:-/tmp}/genesis-prerequisites.XXXXXX")"
trap 'rm -rf "$TMP_DIR"' EXIT

snapshot_contract() {
  python3 - "$MANIFEST" "$SCHEMA" "$IMPLEMENTATION" \
    rust-toolchain.toml package.json package-lock.json .github/workflows/ci.yml <<'PY'
from hashlib import sha256
from pathlib import Path
import json
import sys

print(json.dumps({path: sha256(Path(path).read_bytes()).hexdigest() for path in sys.argv[1:]}, sort_keys=True))
PY
}

expect_fail() {
  local label="$1"
  shift
  if "$@" >"$TMP_DIR/$label.stdout" 2>"$TMP_DIR/$label.stderr"; then
    echo "prerequisite-manifest: negative control was accepted: $label" >&2
    exit 1
  fi
}

before="$(snapshot_contract)"

python3 "$IMPLEMENTATION" validate
python3 "$IMPLEMENTATION" self-test
python3 "$IMPLEMENTATION" list-profiles >"$TMP_DIR/profiles-a.txt"
python3 "$IMPLEMENTATION" list-profiles >"$TMP_DIR/profiles-b.txt"
cmp -s "$TMP_DIR/profiles-a.txt" "$TMP_DIR/profiles-b.txt" || {
  echo "prerequisite-manifest: profile listing is not deterministic" >&2
  exit 1
}

bash scripts/genesis_prerequisites.sh --profile core --format json >"$TMP_DIR/core-a.json"
bash scripts/genesis_prerequisites.sh --profile core --format json >"$TMP_DIR/core-b.json"
cmp -s "$TMP_DIR/core-a.json" "$TMP_DIR/core-b.json" || {
  echo "prerequisite-manifest: repeated core diagnostics differ" >&2
  exit 1
}
python3 - "$TMP_DIR/core-a.json" <<'PY'
from pathlib import Path
import json
import sys

report = json.loads(Path(sys.argv[1]).read_text(encoding="utf-8"))
if report.get("kind") != "genesis/prerequisite-diagnostic-v0.1" or report.get("profile") != "core":
    raise SystemExit("prerequisite-manifest: core diagnostic identity mismatch")
if not report.get("ok") or report.get("summary", {}).get("requiredFailures") != 0:
    raise SystemExit("prerequisite-manifest: core diagnostic has required failures")
if not isinstance(report.get("checks"), list) or not report["checks"]:
    raise SystemExit("prerequisite-manifest: core diagnostic contains no checks")
PY

python3 - "$MANIFEST" "$TMP_DIR/duplicate.json" "$TMP_DIR/unsafe.json" \
  "$TMP_DIR/drift.json" "$TMP_DIR/incomplete.json" "$TMP_DIR/target-drift.json" <<'PY'
from pathlib import Path
import json
import sys

source = Path(sys.argv[1]).read_text(encoding="utf-8")
needle = '  "version": "0.1"'
if source.count(needle) != 1:
    raise SystemExit("prerequisite-manifest: duplicate-key anchor drift")
Path(sys.argv[2]).write_text(source.replace(needle, needle + ',\n  "version": "0.1"', 1), encoding="utf-8")
manifest = json.loads(source)
unsafe = json.loads(source)
next(tool for tool in unsafe["tools"] if tool["id"] == "git")["probe"]["argv"] = ["git", "pu" + "ll"]
Path(sys.argv[3]).write_text(json.dumps(unsafe, sort_keys=True), encoding="utf-8")
drift = json.loads(source)
next(tool for tool in drift["tools"] if tool["id"] == "rustc")["constraint"]["exact"] = "9.9.9"
Path(sys.argv[4]).write_text(json.dumps(drift, sort_keys=True), encoding="utf-8")
incomplete = json.loads(source)
next(profile for profile in incomplete["profiles"] if profile["id"] == "core")["requires"].remove("python")
Path(sys.argv[5]).write_text(json.dumps(incomplete, sort_keys=True), encoding="utf-8")
target_drift = json.loads(source)
next(tool for tool in target_drift["tools"] if tool["id"] == "rust-target-wasm32-wasip1")["probe"]["target"] = "wasm32-wasip2"
Path(sys.argv[6]).write_text(json.dumps(target_drift, sort_keys=True), encoding="utf-8")
PY

expect_fail duplicate-key python3 "$IMPLEMENTATION" --manifest "$TMP_DIR/duplicate.json" validate
expect_fail unsafe-command python3 "$IMPLEMENTATION" --manifest "$TMP_DIR/unsafe.json" validate
expect_fail source-drift python3 "$IMPLEMENTATION" --manifest "$TMP_DIR/drift.json" validate
expect_fail incomplete-profile python3 "$IMPLEMENTATION" --manifest "$TMP_DIR/incomplete.json" validate
expect_fail target-mirror-drift python3 "$IMPLEMENTATION" --manifest "$TMP_DIR/target-drift.json" validate
expect_fail unknown-profile python3 "$IMPLEMENTATION" diagnose --profile does-not-exist
expect_fail wrong-platform python3 "$IMPLEMENTATION" diagnose --profile apple-device --platform linux-x86-64

after="$(snapshot_contract)"
[[ "$before" == "$after" ]] || {
  echo "prerequisite-manifest: check mutated retained contract inputs" >&2
  exit 1
}

echo "prerequisite-manifest-contract: ok (profiles=9 platforms=4 tools=27 negative_controls=7 check_mode=read_only)"
