#!/usr/bin/env bash
set -euo pipefail

ROOT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
cd "$ROOT_DIR"

DASHBOARD_MD="$ROOT_DIR/docs/status/SELFHOST_CUTOVER.md"
DOCS=(
  "$ROOT_DIR/docs/spec/CLI.md"
  "$ROOT_DIR/docs/spec/WASI.md"
  "$ROOT_DIR/docs/spec/SELF_HOST_BOUNDARY.md"
)
SOURCE_OF_TRUTH_REF='docs/status/SELFHOST_CUTOVER.md'

[[ -f "$DASHBOARD_MD" ]] || {
  echo "selfhost-doc-runtime-parity: missing dashboard markdown: $DASHBOARD_MD" >&2
  exit 1
}

for doc in "${DOCS[@]}"; do
  [[ -f "$doc" ]] || {
    echo "selfhost-doc-runtime-parity: missing doc: $doc" >&2
    exit 1
  }
done

python3 - "$DASHBOARD_MD" "${DOCS[@]}" "$SOURCE_OF_TRUTH_REF" <<'PY'
import pathlib
import re
import sys

dashboard_path = pathlib.Path(sys.argv[1])
docs = [pathlib.Path(p) for p in sys.argv[2:-1]]
source_ref = sys.argv[-1]

dashboard_text = dashboard_path.read_text(encoding="utf-8")
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
