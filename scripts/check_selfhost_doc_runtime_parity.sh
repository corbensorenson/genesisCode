#!/usr/bin/env bash
set -euo pipefail

source "$(dirname "${BASH_SOURCE[0]}")/lib/gate_telemetry.sh"
genesis_gate_telemetry_reexec "$0" "$@"

ROOT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
cd "$ROOT_DIR"

DASHBOARD_MD="$ROOT_DIR/docs/status/SELFHOST_CUTOVER.md"
DOCS=(
  "$ROOT_DIR/docs/spec/CLI.md"
  "$ROOT_DIR/docs/spec/WASI.md"
  "$ROOT_DIR/docs/spec/SELF_HOST_BOUNDARY.md"
)
SOURCE_OF_TRUTH_REF='docs/status/SELFHOST_CUTOVER.md'
SEMANTIC_AUTHORITY_REF='docs/status/SELFHOST_AUTHORITY_v0.1.md'

[[ -f "$DASHBOARD_MD" ]] || {
  echo "selfhost-doc-runtime-parity: missing dashboard markdown: $DASHBOARD_MD" >&2
  exit 1
}
[[ -f "$ROOT_DIR/$SEMANTIC_AUTHORITY_REF" ]] || {
  echo "selfhost-doc-runtime-parity: missing semantic authority view: $SEMANTIC_AUTHORITY_REF" >&2
  exit 1
}

for doc in "${DOCS[@]}"; do
  [[ -f "$doc" ]] || {
    echo "selfhost-doc-runtime-parity: missing doc: $doc" >&2
    exit 1
  }
done

python3 - "$DASHBOARD_MD" "${DOCS[@]}" "$SOURCE_OF_TRUTH_REF" "$SEMANTIC_AUTHORITY_REF" <<'PY'
import pathlib
import re
import sys

dashboard_path = pathlib.Path(sys.argv[1])
docs = [pathlib.Path(p) for p in sys.argv[2:-2]]
source_ref = sys.argv[-2]
semantic_ref = sys.argv[-1]

dashboard_text = dashboard_path.read_text(encoding="utf-8")
for required in (
    "Scope: command routing only",
    "docs/status/SELFHOST_AUTHORITY_v0.1.md",
):
    if required not in dashboard_text:
        raise SystemExit(
            "selfhost-doc-runtime-parity: routing dashboard missing semantic-scope boundary: "
            + required
        )
match = re.search(r"\|\s*Selfhost-routed coverage\s*\|\s*([0-9]+\.[0-9]+)%\s*\|", dashboard_text)
if match is None:
    raise SystemExit(
        "selfhost-doc-runtime-parity: could not parse Selfhost-routed coverage from "
        f"{dashboard_path}"
    )
routed_percent = float(match.group(1))

stale_patterns = [
    re.compile(r"not yet selfhost-routed", re.IGNORECASE),
    re.compile(r"not yet selfhost routed", re.IGNORECASE),
    re.compile(r"not yet routed through selfhost frontend", re.IGNORECASE),
]

errors = []
for doc_path in docs:
    text = doc_path.read_text(encoding="utf-8")
    if source_ref not in text:
        errors.append(
            f"selfhost-doc-runtime-parity: {doc_path} missing canonical source reference "
            f"'{source_ref}'"
        )
    if semantic_ref not in text:
        errors.append(
            f"selfhost-doc-runtime-parity: {doc_path} missing semantic authority reference "
            f"'{semantic_ref}'"
        )
    if routed_percent >= 100.0:
        for pattern in stale_patterns:
            if pattern.search(text):
                errors.append(
                    "selfhost-doc-runtime-parity: stale routing caveat present in "
                    f"{doc_path} while dashboard reports {routed_percent:.2f}% routed coverage"
                )
                break

if errors:
    raise SystemExit("\n".join(errors))

print(
    "selfhost-doc-runtime-parity: ok "
    f"(selfhost_routed_coverage={routed_percent:.2f}% source={dashboard_path})"
)
PY
