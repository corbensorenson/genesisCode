#!/usr/bin/env bash
set -euo pipefail

source "$(dirname "${BASH_SOURCE[0]}")/lib/gate_telemetry.sh"
genesis_gate_telemetry_reexec "$0" "$@"

ROOT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
cd "$ROOT_DIR"

MANIFEST="tools/genesis-evidence-verifier/Cargo.toml"
POLICY="policies/evidence_verifier_trust_v0.1.json"
POLICY_SHA256="6c11d747540c71887a23074f7d30b1f8eecd79b695eae9af79553d11a8011220"
BUNDLE="docs/program/evidence/GENESIS_EVIDENCE_BUNDLE_v0.1.json"
TREE="docs/program/evidence/GENESIS_EVIDENCE_ARTIFACT_TREE_v0.1.json"
ARTIFACT_ROOT="docs/program/evidence"
CATALOG="docs/program/evidence/GENESIS_EVIDENCE_VERIFIER_NEGATIVE_VECTORS_v0.1.json"

source "$ROOT_DIR/scripts/lib/cargo_target_dir.sh"
genesis_configure_cargo_target_dir \
  "$ROOT_DIR" \
  "genesis-evidence-verifier" \
  evidence-verifier-host

TMP_DIR="$(mktemp -d "${TMPDIR:-/tmp}/genesis-evidence-verifier.XXXXXX")"
trap 'rm -rf "$TMP_DIR"' EXIT

snapshot_retained() {
  python3 - "$POLICY" "$BUNDLE" "$TREE" "$CATALOG" \
    "$ARTIFACT_ROOT/artifact/genesis-example.bin" <<'PY'
from hashlib import sha256
from pathlib import Path
import json
import sys

print(json.dumps({
    path: sha256(Path(path).read_bytes()).hexdigest()
    for path in sys.argv[1:]
}, sort_keys=True))
PY
}

before="$(snapshot_retained)"

python3 scripts/lib/genesis_evidence_verifier_vectors.py --check

root_metadata="$TMP_DIR/root-metadata.json"
tool_metadata="$TMP_DIR/tool-metadata.json"
cargo metadata --locked --offline --no-deps --format-version 1 >"$root_metadata"
cargo metadata --manifest-path "$MANIFEST" --locked --offline --no-deps --format-version 1 \
  >"$tool_metadata"
python3 - "$root_metadata" "$tool_metadata" <<'PY'
import json
from pathlib import Path
import sys

root = json.loads(Path(sys.argv[1]).read_text(encoding="utf-8"))
tool = json.loads(Path(sys.argv[2]).read_text(encoding="utf-8"))
root_names = {package["name"] for package in root["packages"]}
tool_names = {package["name"] for package in tool["packages"]}
if "genesis-evidence-verifier" in root_names:
    raise SystemExit("genesis-evidence-verifier: verifier entered the root workspace graph")
if tool_names != {"genesis-evidence-verifier"}:
    raise SystemExit(
        "genesis-evidence-verifier: standalone metadata has unexpected local packages: "
        + repr(sorted(tool_names))
    )
PY

production_source="$TMP_DIR/production-source.rs"
while IFS= read -r source; do
  awk '/^#\[cfg\(test\)\]/{exit} {print}' "$source" >>"$production_source"
done < <(find tools/genesis-evidence-verifier/src -type f -name '*.rs' -print | LC_ALL=C sort)
if grep -Eq 'SigningKey|(^|[^[:alnum:]_])(fs::write|File::create|OpenOptions)([^[:alnum:]_]|$)' \
  "$production_source"; then
  echo "genesis-evidence-verifier: production verifier contains a signing or write path" >&2
  exit 1
fi

cargo fmt --manifest-path "$MANIFEST" -- --check
cargo clippy --manifest-path "$MANIFEST" --locked --offline --all-targets -- -D warnings
cargo test --manifest-path "$MANIFEST" --locked --offline
cargo build --manifest-path "$MANIFEST" --locked --offline

VERIFY=(
  "$CARGO_TARGET_DIR/debug/genesis-evidence-verifier"
  --bundle "$BUNDLE"
  --policy "$POLICY"
  --policy-sha256 "$POLICY_SHA256"
  --artifact-tree "$TREE"
  --artifact-root "$ARTIFACT_ROOT"
)
"${VERIFY[@]}" >"$TMP_DIR/result-a.json"
"${VERIFY[@]}" >"$TMP_DIR/result-b.json"
cmp -s "$TMP_DIR/result-a.json" "$TMP_DIR/result-b.json" || {
  echo "genesis-evidence-verifier: repeated verification output is not deterministic" >&2
  exit 1
}

python3 - "$TMP_DIR/result-a.json" "$CATALOG" <<'PY'
import json
from pathlib import Path
import sys

result = json.loads(Path(sys.argv[1]).read_text(encoding="utf-8"))
catalog = json.loads(Path(sys.argv[2]).read_text(encoding="utf-8"))
expected = {
    "kind": "genesis/evidence-verification-result-v0.1",
    "ok": True,
    "verifierVersion": "0.1.0",
    "compatibilityProfile": "genesis-evidence-v0.1+slsa-v1+dsse-v1",
    "verifiedAttestations": 2,
    "verifiedSignatures": 2,
    "verifiedArtifacts": 1,
    "verifiedNegativeControls": 1,
}
for key, value in expected.items():
    if result.get(key) != value:
        raise SystemExit(
            f"genesis-evidence-verifier: result field {key} expected {value!r}, "
            f"observed {result.get(key)!r}"
        )
for key in ("bundleSha256", "policySha256", "artifactTreeSha256", "artifactTreeRoot"):
    value = result.get(key)
    if not isinstance(value, str) or len(value) != 64 or any(ch not in "0123456789abcdef" for ch in value):
        raise SystemExit(f"genesis-evidence-verifier: result field {key} is not SHA-256")
case_ids = [case["id"] for case in catalog.get("cases", [])]
if len(case_ids) != 30 or case_ids != sorted(set(case_ids)):
    raise SystemExit("genesis-evidence-verifier: negative vector catalog must contain 30 sorted unique cases")
PY

after="$(snapshot_retained)"
[[ "$before" == "$after" ]] || {
  echo "genesis-evidence-verifier: check mutated retained evidence" >&2
  exit 1
}

echo "genesis-evidence-verifier-contract: ok (standalone=true read_only=true signatures=2 artifacts=1 negative_controls=30)"
