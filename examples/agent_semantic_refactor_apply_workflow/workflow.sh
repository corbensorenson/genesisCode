#!/usr/bin/env bash
set -euo pipefail

ROOT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")/../.." && pwd)"
DEFAULT_DEBUG_DIR="${CARGO_TARGET_DIR:-$ROOT_DIR/target}/debug"
GENESIS_BIN="${GENESIS_BIN:-$DEFAULT_DEBUG_DIR/genesis_parity}"

if [[ ! -x "$GENESIS_BIN" ]]; then
  cargo build -p gc_cli --bin genesis_parity >/dev/null
fi

TMP_DIR="$(mktemp -d)"
cleanup() {
  rm -rf "$TMP_DIR"
}
trap cleanup EXIT

cat >"$TMP_DIR/package.toml" <<'TOML'
name = "semantic-apply-workflow"
version = "0.0.1"
dependencies = []
obligations = ["core/obligation::unit-tests"]
modules = [
  { path = "a.gc", hash = "" },
  { path = "b.gc", hash = "" }
]
tests = ["my/pkg::tests"]
TOML

cat >"$TMP_DIR/a.gc" <<'GC'
(def my/pkg::foo 41)

(def my/pkg::tests
  {
    "t1" { :body (fn (_) my/pkg::foo) :expect 41 }
  })
GC

cat >"$TMP_DIR/b.gc" <<'GC'
(def my/pkg::use-foo (fn (_) my/pkg::foo))
GC

PLAN_JSON="$TMP_DIR/plan.json"
APPLY_JSON="$TMP_DIR/apply.json"
VERIFY_JSON="$TMP_DIR/verify.json"

"$GENESIS_BIN" \
  --json \
  --coreform-frontend rust \
  semantic-edit refactor-plan \
  --pkg "$TMP_DIR/package.toml" \
  --kind rename \
  --from my/pkg::foo \
  --to my/pkg::foo_v2 >"$PLAN_JSON"

python3 - "$PLAN_JSON" <<'PY'
import json
import pathlib
import sys

doc = json.loads(pathlib.Path(sys.argv[1]).read_text(encoding="utf-8"))
if doc.get("kind") != "genesis/semantic-edit-refactor-plan-v0.1":
    raise SystemExit("semantic workflow: unexpected refactor-plan kind")
if doc.get("ok") is not True:
    raise SystemExit("semantic workflow: refactor-plan must be safe before apply")
PY

"$GENESIS_BIN" \
  --json \
  --coreform-frontend rust \
  semantic-edit apply-plan \
  --pkg "$TMP_DIR/package.toml" \
  --kind rename \
  --from my/pkg::foo \
  --to my/pkg::foo_v2 >"$APPLY_JSON"

python3 - "$APPLY_JSON" <<'PY'
import json
import pathlib
import sys

doc = json.loads(pathlib.Path(sys.argv[1]).read_text(encoding="utf-8"))
if doc.get("kind") != "genesis/semantic-edit-apply-plan-v0.1":
    raise SystemExit("semantic workflow: unexpected apply-plan kind")
if doc.get("ok") is not True:
    raise SystemExit("semantic workflow: apply-plan failed")
PY

"$GENESIS_BIN" \
  --json \
  --coreform-frontend rust \
  verify --pkg "$TMP_DIR/package.toml" >"$VERIFY_JSON"

python3 - "$VERIFY_JSON" <<'PY'
import json
import pathlib
import sys

doc = json.loads(pathlib.Path(sys.argv[1]).read_text(encoding="utf-8"))
if doc.get("kind") != "genesis/verify-v0.2":
    raise SystemExit("semantic workflow: unexpected verify kind")
if doc.get("ok") is not True:
    raise SystemExit("semantic workflow: verify failed")
PY

echo "agent-semantic-refactor-apply-workflow: ok"
