#!/usr/bin/env bash
set -euo pipefail

ROOT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
cd "$ROOT_DIR"

PLAN_FILE="upgrade_plan.md"
MATRIX_FILE="feature_matrix.md"

[[ -f "$PLAN_FILE" ]] || {
  echo "feature-matrix-gap-hygiene: missing plan file: $PLAN_FILE" >&2
  exit 1
}
[[ -f "$MATRIX_FILE" ]] || {
  echo "feature-matrix-gap-hygiene: missing matrix file: $MATRIX_FILE" >&2
  exit 1
}

python3 - "$PLAN_FILE" "$MATRIX_FILE" <<'PY'
import pathlib
import re
import sys

plan_path = pathlib.Path(sys.argv[1])
matrix_path = pathlib.Path(sys.argv[2])
plan_text = plan_path.read_text(encoding="utf-8")
matrix_text = matrix_path.read_text(encoding="utf-8")

open_ids = []
for line in plan_text.splitlines():
    m = re.match(r"^- \[ \] (P\d+\.\d+)\b", line)
    if m:
        open_ids.append(m.group(1))

lines = matrix_text.splitlines()
start = None
end = None
for i, line in enumerate(lines):
    if line.startswith("Known GenesisCode gaps"):
        start = i + 1
        continue
    if start is not None and line.startswith("Primary evidence paths"):
        end = i
        break

if start is None or end is None or end <= start:
    raise SystemExit(
        "feature-matrix-gap-hygiene: feature_matrix.md must contain a 'Known GenesisCode gaps' section followed by 'Primary evidence paths'"
    )

section_lines = lines[start:end]
section_text = "\n".join(section_lines)
section_ids = sorted(set(re.findall(r"\bP\d+\.\d+\b", section_text)))
open_ids_sorted = sorted(set(open_ids))

missing = sorted(set(open_ids_sorted) - set(section_ids))
extra = sorted(set(section_ids) - set(open_ids_sorted))
if missing:
    raise SystemExit(
        "feature-matrix-gap-hygiene: known gaps missing unresolved upgrade plan IDs: "
        + ", ".join(missing)
    )
if extra:
    raise SystemExit(
        "feature-matrix-gap-hygiene: known gaps reference resolved/non-open IDs: "
        + ", ".join(extra)
    )

if not open_ids_sorted:
    if re.search(r"^-\s+none\b", section_text, flags=re.MULTILINE) is None:
        raise SystemExit(
            "feature-matrix-gap-hygiene: when no unresolved upgrade plan IDs exist, known gaps must include '- none'"
        )

print(
    f"feature-matrix-gap-hygiene: ok (open_ids={len(open_ids_sorted)} mapped_ids={len(section_ids)})"
)
PY
