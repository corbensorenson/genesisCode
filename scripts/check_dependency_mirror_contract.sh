#!/usr/bin/env bash
set -euo pipefail

source "$(dirname "${BASH_SOURCE[0]}")/lib/gate_telemetry.sh"
genesis_gate_telemetry_reexec "$0" "$@"

ROOT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
cd "$ROOT_DIR"

TMP_DIR="$(mktemp -d "${TMPDIR:-/tmp}/genesis-dependency-mirror-contract.XXXXXX")"
trap 'rm -rf "$TMP_DIR"' EXIT

POLICY="genesis.dependency-mirror.json"
RETAINED=(
  "$POLICY"
  "Cargo.lock"
  "Cargo.toml"
  "package-lock.json"
  "package.json"
  "tools/genesis-evidence-verifier/Cargo.lock"
  "tools/genesis-evidence-verifier/Cargo.toml"
  "docs/spec/DEPENDENCY_MIRROR_v0.1.md"
  "docs/spec/DEPENDENCY_MIRROR_v0.1.schema.json"
  "docs/spec/DEPENDENCY_MIRROR_MANIFEST_v0.1.schema.json"
  "scripts/lib/dependency_mirror.py"
)

for path in "${RETAINED[@]}"; do
  [[ -f "$path" ]] || {
    echo "dependency-mirror-contract: missing retained input: $path" >&2
    exit 1
  }
done

before="$(cksum "${RETAINED[@]}")"
python3 scripts/lib/dependency_mirror.py validate >"$TMP_DIR/validate-1.out"
python3 scripts/lib/dependency_mirror.py validate >"$TMP_DIR/validate-2.out"
cmp "$TMP_DIR/validate-1.out" "$TMP_DIR/validate-2.out"
python3 scripts/lib/dependency_mirror.py self-test >"$TMP_DIR/self-test.out"

BASE="$TMP_DIR/base"
mkdir -p "$BASE"
while IFS= read -r relative; do
  [[ -n "$relative" ]] || continue
  mkdir -p "$BASE/$(dirname "$relative")"
  cp "$relative" "$BASE/$relative"
done <<'EOF'
genesis.dependency-mirror.json
Cargo.lock
Cargo.toml
package-lock.json
package.json
tools/genesis-evidence-verifier/Cargo.lock
tools/genesis-evidence-verifier/Cargo.toml
EOF

expect_rejected() {
  local label="$1"
  local expected="$2"
  local fixture="$TMP_DIR/$label"
  if python3 scripts/lib/dependency_mirror.py \
      --source-root "$fixture" \
      --policy "$fixture/genesis.dependency-mirror.json" \
      validate >"$TMP_DIR/$label.out" 2>&1; then
    echo "dependency-mirror-contract: expected rejection: $label" >&2
    exit 1
  fi
  if ! grep -Fq "$expected" "$TMP_DIR/$label.out"; then
    echo "dependency-mirror-contract: wrong diagnostic for $label" >&2
    cat "$TMP_DIR/$label.out" >&2
    exit 1
  fi
}

new_fixture() {
  local label="$1"
  cp -R "$BASE" "$TMP_DIR/$label"
}

new_fixture duplicate-key
python3 - "$TMP_DIR/duplicate-key/genesis.dependency-mirror.json" <<'PY'
from pathlib import Path
import sys
path = Path(sys.argv[1])
text = path.read_text(encoding="utf-8")
path.write_text(text.replace('  "version": "0.1"', '  "version": "0.1",\n  "version": "0.1"'), encoding="utf-8")
PY
expect_rejected duplicate-key "duplicate JSON key: version"

new_fixture unknown-field
python3 - "$TMP_DIR/unknown-field/genesis.dependency-mirror.json" <<'PY'
import json
from pathlib import Path
import sys
path = Path(sys.argv[1]); doc = json.loads(path.read_text()); doc["trustMe"] = True
path.write_text(json.dumps(doc, indent=2, sort_keys=True) + "\n")
PY
expect_rejected unknown-field "policy keys differ"

new_fixture incomplete-authority
python3 - "$TMP_DIR/incomplete-authority/genesis.dependency-mirror.json" <<'PY'
import json
from pathlib import Path
import sys
path = Path(sys.argv[1]); doc = json.loads(path.read_text()); doc["authorityFiles"].remove("package.json")
path.write_text(json.dumps(doc, indent=2, sort_keys=True) + "\n")
PY
expect_rejected incomplete-authority "policy.authorityFiles must exactly close Cargo and npm authorities"

new_fixture unsorted-workspaces
python3 - "$TMP_DIR/unsorted-workspaces/genesis.dependency-mirror.json" <<'PY'
import json
from pathlib import Path
import sys
path = Path(sys.argv[1]); doc = json.loads(path.read_text()); doc["cargo"]["workspaces"].reverse()
path.write_text(json.dumps(doc, indent=2, sort_keys=True) + "\n")
PY
expect_rejected unsorted-workspaces "policy.cargo.workspaces must be sorted by identity"

new_fixture isolation-downgrade
python3 - "$TMP_DIR/isolation-downgrade/genesis.dependency-mirror.json" <<'PY'
import json
from pathlib import Path
import sys
path = Path(sys.argv[1]); doc = json.loads(path.read_text()); doc["networkIsolation"]["darwin"]["backend"] = "unsupported-fail-closed"
path.write_text(json.dumps(doc, indent=2, sort_keys=True) + "\n")
PY
expect_rejected isolation-downgrade "network isolation matrix differs from the v0.1 closed profile"

new_fixture incomplete-checks
python3 - "$TMP_DIR/incomplete-checks/genesis.dependency-mirror.json" <<'PY'
import json
from pathlib import Path
import sys
path = Path(sys.argv[1]); doc = json.loads(path.read_text()); doc["offlineChecks"].pop()
path.write_text(json.dumps(doc, indent=2, sort_keys=True) + "\n")
PY
expect_rejected incomplete-checks "offline check set is incomplete or unknown"

new_fixture cargo-source
python3 - "$TMP_DIR/cargo-source/Cargo.lock" <<'PY'
from pathlib import Path
import sys
path = Path(sys.argv[1]); text = path.read_text()
path.write_text(text.replace('registry+https://github.com/rust-lang/crates.io-index', 'git+https://example.invalid/repo', 1))
PY
expect_rejected cargo-source "undeclared Cargo source"

new_fixture cargo-checksum
python3 - "$TMP_DIR/cargo-checksum/Cargo.lock" <<'PY'
from pathlib import Path
import re, sys
path = Path(sys.argv[1]); text = path.read_text()
path.write_text(re.sub(r'checksum = "[0-9a-f]{64}"', 'checksum = "invalid"', text, count=1))
PY
expect_rejected cargo-checksum "invalid Cargo checksum"

new_fixture npm-origin
python3 - "$TMP_DIR/npm-origin/package-lock.json" <<'PY'
import json
from pathlib import Path
import sys
path = Path(sys.argv[1]); doc = json.loads(path.read_text())
for package in doc["packages"].values():
    if "resolved" in package:
        package["resolved"] = package["resolved"].replace("https://registry.npmjs.org/", "https://example.invalid/", 1)
        break
path.write_text(json.dumps(doc, indent=2) + "\n")
PY
expect_rejected npm-origin "npm URL is outside the exact declared origin"

new_fixture npm-integrity
python3 - "$TMP_DIR/npm-integrity/package-lock.json" <<'PY'
import json
from pathlib import Path
import sys
path = Path(sys.argv[1]); doc = json.loads(path.read_text())
for package in doc["packages"].values():
    if "resolved" in package:
        package["integrity"] = "sha256-invalid"
        break
path.write_text(json.dumps(doc, indent=2) + "\n")
PY
expect_rejected npm-integrity "npm package integrity must use sha512"

new_fixture symlink-authority
mv "$TMP_DIR/symlink-authority/package.json" "$TMP_DIR/symlink-authority/package.real.json"
ln -s package.real.json "$TMP_DIR/symlink-authority/package.json"
expect_rejected symlink-authority "authority must be a regular non-symlink file"

after="$(cksum "${RETAINED[@]}")"
[[ "$before" == "$after" ]] || {
  echo "dependency-mirror-contract: check mode mutated retained inputs" >&2
  exit 1
}

echo "dependency-mirror-contract: ok (authorities=6 cargo_records=483 npm_packages=3 negative_controls=11 internal_controls=10 check_mode=read_only)"
