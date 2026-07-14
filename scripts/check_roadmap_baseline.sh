#!/usr/bin/env bash
set -euo pipefail

source "$(dirname "${BASH_SOURCE[0]}")/lib/gate_telemetry.sh"
genesis_gate_telemetry_reexec "$0" "$@"

ROOT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
cd "$ROOT_DIR"

BUNDLE="docs/program/evidence/roadmap-baselines/roadmap-baseline-e0-2026-07-10-sha256-a3d6c7b809f1c1ba403bab0c4e18fce94154cbae4b35b23aa9e96cfb1c02e967.json"
PUBLIC_KEY="docs/program/evidence/roadmap-baselines/roadmap-baseline-fixture-key-sha256-f942d973dd550ff9b95e0a61f47e8cee9580e8bb8d43f0226408d57bde2d113f.pub"
EXPECTED_KEYID="sha256:f942d973dd550ff9b95e0a61f47e8cee9580e8bb8d43f0226408d57bde2d113f"
PRODUCER_MANIFEST="tools/genesis-evidence-producer/Cargo.toml"
VERIFIER_MANIFEST="tools/genesis-evidence-verifier/Cargo.toml"

source "$ROOT_DIR/scripts/lib/cargo_target_dir.sh"
genesis_configure_cargo_target_dir \
  "$ROOT_DIR" \
  "roadmap-baseline" \
  evidence-verifier-host

TMP_DIR="$(mktemp -d "${TMPDIR:-/tmp}/genesis-roadmap-baseline.XXXXXX")"
trap 'chmod 600 "$TMP_DIR/secret.key" 2>/dev/null || true; rm -rf "$TMP_DIR"' EXIT

snapshot() {
  python3 - "$BUNDLE" "$PUBLIC_KEY" \
    docs/spec/ROADMAP_BASELINE_STATEMENT_v0.1.schema.json \
    docs/spec/ROADMAP_BASELINE_BUNDLE_v0.1.schema.json \
    policies/perf/roadmap_workloads_v0.1.json \
    policies/reference_host_profiles_v0.1.json \
    scripts/lib/roadmap_baseline.py \
    tools/genesis-evidence-producer/Cargo.toml \
    tools/genesis-evidence-producer/Cargo.lock \
    tools/genesis-evidence-producer/src/main.rs \
    tools/genesis-evidence-verifier/Cargo.toml \
    tools/genesis-evidence-verifier/Cargo.lock \
    tools/genesis-evidence-verifier/src/json.rs \
    tools/genesis-evidence-verifier/src/bin/genesis-roadmap-baseline-verifier.rs <<'PY'
from hashlib import sha256
from pathlib import Path
import sys
for raw in sys.argv[1:]:
    path = Path(raw)
    print(f"{sha256(path.read_bytes()).hexdigest()}  {path.as_posix()}")
PY
}

before="$(snapshot)"
python3 scripts/lib/roadmap_baseline.py check-bundle --statement "$BUNDLE"
python3 - "$BUNDLE" "$TMP_DIR/statement.json" <<'PY'
import json
from pathlib import Path
import sys
bundle = json.loads(Path(sys.argv[1]).read_text(encoding="utf-8"))
Path(sys.argv[2]).write_text(json.dumps(bundle["statement"], indent=2, sort_keys=True) + "\n", encoding="ascii")
PY
python3 scripts/lib/roadmap_baseline.py check --statement "$TMP_DIR/statement.json"
python3 scripts/lib/roadmap_baseline.py self-test --statement "$TMP_DIR/statement.json"

python3 - "$BUNDLE" "$PUBLIC_KEY" <<'PY'
from hashlib import sha256
import json
from pathlib import Path
import re
import sys

bundle_path = Path(sys.argv[1])
public_path = Path(sys.argv[2])
bundle = json.loads(bundle_path.read_text(encoding="utf-8"))
statement = bundle["statement"]
identity = statement["baselineIdentitySha256"]
public_hash = sha256(public_path.read_bytes()).hexdigest()
if bundle_path.name != f"roadmap-baseline-e0-2026-07-10-sha256-{identity}.json":
    raise SystemExit("roadmap-baseline: bundle filename is not content-addressed")
if public_path.name != f"roadmap-baseline-fixture-key-sha256-{public_hash}.pub":
    raise SystemExit("roadmap-baseline: public-key filename is not content-addressed")
if len(public_path.read_bytes()) != 32:
    raise SystemExit("roadmap-baseline: public key must contain exactly 32 raw bytes")
expected_failures = {
    "PB-2": "runner-unavailable",
    "PB-3": "decision-not-approved",
    "PB-5": "budget-miss",
    "PB-6": "runner-unavailable",
    "PB-7": "budget-miss",
    "PB-8": "runner-unavailable",
    "PB-9": "runner-unavailable",
    "PB-10": "runner-unavailable",
}
observed = {row["id"]: row for row in statement["workloads"]}
if {key: value["failures"][0]["code"] for key, value in observed.items() if value["failures"]} != expected_failures:
    raise SystemExit("roadmap-baseline: current failure inventory drift")
for workload_id in ("PB-1", "PB-4", "PB-5", "PB-7"):
    row = observed[workload_id]
    if len(row["warmupSamples"]) != 5 or len(row["samples"]) != 30:
        raise SystemExit(f"roadmap-baseline: raw sample count drift for {workload_id}")
if statement["overall"] != {
    "budgetFailing": 2,
    "budgetPassing": 2,
    "decisionGated": 1,
    "observed": 4,
    "runnerUnavailable": 5,
    "status": "observed-with-failures",
}:
    raise SystemExit("roadmap-baseline: truthful overall failure summary drift")
if statement["evidenceClass"] != "E0" or statement["authoritative"] is not False:
    raise SystemExit("roadmap-baseline: local observation attempted authority escalation")
PY

cargo fmt --manifest-path "$PRODUCER_MANIFEST" -- --check
cargo fmt --manifest-path "$VERIFIER_MANIFEST" -- --check
cargo build --manifest-path "$PRODUCER_MANIFEST" --locked --offline
cargo build --manifest-path "$VERIFIER_MANIFEST" --locked --offline --bin genesis-roadmap-baseline-verifier
PRODUCER="$CARGO_TARGET_DIR/debug/genesis-evidence-producer"
VERIFIER="$CARGO_TARGET_DIR/debug/genesis-roadmap-baseline-verifier"

VERIFY=("$VERIFIER" --bundle "$BUNDLE" --public-key "$PUBLIC_KEY" --expected-keyid "$EXPECTED_KEYID")
"${VERIFY[@]}" >"$TMP_DIR/verify-a.json"
"${VERIFY[@]}" >"$TMP_DIR/verify-b.json"
cmp -s "$TMP_DIR/verify-a.json" "$TMP_DIR/verify-b.json" || {
  echo "roadmap-baseline: independent verifier output is not deterministic" >&2
  exit 1
}

python3 - "$TMP_DIR/secret.key" <<'PY'
import os
from pathlib import Path
import sys
path = Path(sys.argv[1])
fd = os.open(path, os.O_WRONLY | os.O_CREAT | os.O_EXCL, 0o600)
with os.fdopen(fd, "wb") as handle:
    handle.write(bytes(range(32)))
PY
"$PRODUCER" --statement "$TMP_DIR/statement.json" --secret-key "$TMP_DIR/secret.key" >"$TMP_DIR/sign-a.json"
"$PRODUCER" --statement "$TMP_DIR/statement.json" --secret-key "$TMP_DIR/secret.key" >"$TMP_DIR/sign-b.json"
cmp -s "$TMP_DIR/sign-a.json" "$TMP_DIR/sign-b.json" || {
  echo "roadmap-baseline: producer output is not deterministic for fixed inputs" >&2
  exit 1
}

expect_verify_rejected() {
  local name="$1" candidate="$2" key="${3:-$PUBLIC_KEY}" keyid="${4:-$EXPECTED_KEYID}"
  if "$VERIFIER" --bundle "$candidate" --public-key "$key" --expected-keyid "$keyid" \
      >"$TMP_DIR/$name.out" 2>"$TMP_DIR/$name.err"; then
    echo "roadmap-baseline: negative control accepted: $name" >&2
    exit 1
  fi
}

python3 - "$BUNDLE" "$TMP_DIR" <<'PY'
import base64
import copy
import json
from pathlib import Path
import sys

source = json.loads(Path(sys.argv[1]).read_text(encoding="utf-8"))
root = Path(sys.argv[2])

candidate = copy.deepcopy(source)
raw = bytearray(base64.b64decode(candidate["envelope"]["signatures"][0]["sig"]))
raw[0] ^= 1
candidate["envelope"]["signatures"][0]["sig"] = base64.b64encode(raw).decode("ascii")
(root / "forged-signature.json").write_text(json.dumps(candidate, sort_keys=True), encoding="ascii")

candidate = copy.deepcopy(source)
candidate["authority"] = "release"
(root / "authority-escalation.json").write_text(json.dumps(candidate, sort_keys=True), encoding="ascii")

candidate = copy.deepcopy(source)
candidate["statement"]["workloads"][0]["samples"][0]["durationNs"] += 1
payload = (json.dumps(candidate["statement"], sort_keys=True, separators=(",", ":")) + "\n").encode("ascii")
candidate["envelope"]["payload"] = base64.b64encode(payload).decode("ascii")
(root / "sample-tamper.json").write_text(json.dumps(candidate, sort_keys=True), encoding="ascii")

text = Path(sys.argv[1]).read_text(encoding="utf-8")
text = text.replace('"authority": "observation"', '"authority": "observation", "authority": "observation"', 1)
(root / "duplicate-key.json").write_text(text, encoding="utf-8")
(root / "wrong-key.pub").write_bytes(bytes([1]) * 32)
PY

expect_verify_rejected forged-signature "$TMP_DIR/forged-signature.json"
expect_verify_rejected authority-escalation "$TMP_DIR/authority-escalation.json"
expect_verify_rejected sample-tamper "$TMP_DIR/sample-tamper.json"
expect_verify_rejected duplicate-key "$TMP_DIR/duplicate-key.json"
expect_verify_rejected wrong-key "$BUNDLE" "$TMP_DIR/wrong-key.pub"

cp "$TMP_DIR/secret.key" "$TMP_DIR/permissive.key"
chmod 0644 "$TMP_DIR/permissive.key"
if "$PRODUCER" --statement "$TMP_DIR/statement.json" --secret-key "$TMP_DIR/permissive.key" >/dev/null 2>&1; then
  echo "roadmap-baseline: producer accepted permissive secret-key permissions" >&2
  exit 1
fi
printf 'short' >"$TMP_DIR/short.key"
chmod 0600 "$TMP_DIR/short.key"
if "$PRODUCER" --statement "$TMP_DIR/statement.json" --secret-key "$TMP_DIR/short.key" >/dev/null 2>&1; then
  echo "roadmap-baseline: producer accepted a short secret key" >&2
  exit 1
fi
python3 - "$TMP_DIR/statement.json" "$TMP_DIR/escalated-statement.json" <<'PY'
import json
from pathlib import Path
import sys
value = json.loads(Path(sys.argv[1]).read_text(encoding="utf-8"))
value["authoritative"] = True
Path(sys.argv[2]).write_text(json.dumps(value), encoding="ascii")
PY
if "$PRODUCER" --statement "$TMP_DIR/escalated-statement.json" --secret-key "$TMP_DIR/secret.key" >/dev/null 2>&1; then
  echo "roadmap-baseline: producer signed an authority-escalated statement" >&2
  exit 1
fi

if python3 scripts/lib/roadmap_baseline.py assemble \
    --statement "$TMP_DIR/statement.json" \
    --signature "$TMP_DIR/sign-a.json" \
    --output "$BUNDLE" \
    --public-key-output "$TMP_DIR/unused.pub" >/dev/null 2>&1; then
  echo "roadmap-baseline: append-only assembler overwrote retained history" >&2
  exit 1
fi

after="$(snapshot)"
[[ "$before" == "$after" ]] || {
  echo "roadmap-baseline: check mutated retained baseline evidence" >&2
  exit 1
}

echo "roadmap-baseline-contract: ok (class=E0 authoritative=false workloads=10 raw_samples=120 warmups=20 current_failures=8 crypto_controls=9 statement_controls=8 check_mode=read_only)"
