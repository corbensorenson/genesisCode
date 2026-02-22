#!/usr/bin/env bash
set -euo pipefail

ROOT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
cd "$ROOT_DIR"

PLAN_FILE="upgrade_plan.md"
MATRIX_FILE="feature_matrix.md"
READINESS_REPORT="${GENESIS_SELFHOST_READINESS_REPORT:-.genesis/perf/selfhost_readiness_report.json}"
REFRESH_READINESS="${GENESIS_PLANNING_REFRESH_READINESS:-1}"

[[ -f "$PLAN_FILE" ]] || {
  echo "feature-matrix-gap-hygiene: missing plan file: $PLAN_FILE" >&2
  exit 1
}
[[ -f "$MATRIX_FILE" ]] || {
  echo "feature-matrix-gap-hygiene: missing matrix file: $MATRIX_FILE" >&2
  exit 1
}
if [[ "$REFRESH_READINESS" != "0" && "$REFRESH_READINESS" != "1" ]]; then
  echo "feature-matrix-gap-hygiene: GENESIS_PLANNING_REFRESH_READINESS must be 0 or 1" >&2
  exit 2
fi
if [[ "$REFRESH_READINESS" == "1" ]]; then
  echo "feature-matrix-gap-hygiene: refreshing readiness report via check_selfhost_readiness_scorecard.sh"
  bash scripts/check_selfhost_readiness_scorecard.sh >/dev/null
elif [[ ! -f "$READINESS_REPORT" ]]; then
  echo "feature-matrix-gap-hygiene: readiness report missing; generating via check_selfhost_readiness_scorecard.sh"
  bash scripts/check_selfhost_readiness_scorecard.sh >/dev/null
fi
[[ -f "$READINESS_REPORT" ]] || {
  echo "feature-matrix-gap-hygiene: missing readiness report after generation: $READINESS_REPORT" >&2
  exit 1
}

python3 - "$PLAN_FILE" "$MATRIX_FILE" "$READINESS_REPORT" <<'PY'
import pathlib
import re
import sys
import json

plan_path = pathlib.Path(sys.argv[1])
matrix_path = pathlib.Path(sys.argv[2])
readiness_path = pathlib.Path(sys.argv[3])
plan_text = plan_path.read_text(encoding="utf-8")
matrix_text = matrix_path.read_text(encoding="utf-8")
readiness_doc = json.loads(readiness_path.read_text(encoding="utf-8"))

if readiness_doc.get("kind") != "genesis/selfhost-readiness-v0.1":
    raise SystemExit(
        "feature-matrix-gap-hygiene: readiness report kind mismatch (expected genesis/selfhost-readiness-v0.1)"
    )

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
readiness_ids_sorted = sorted(
    set(
        str(x)
        for x in readiness_doc.get("unresolved_upgrade_plan_ids", [])
        if re.fullmatch(r"P\d+\.\d+", str(x))
    )
)
if open_ids_sorted != readiness_ids_sorted:
    raise SystemExit(
        "feature-matrix-gap-hygiene: unresolved upgrade IDs must match readiness report: "
        f"plan={open_ids_sorted} readiness={readiness_ids_sorted}"
    )

if open_ids_sorted and re.search(r"^-\s+none\b", section_text, flags=re.MULTILINE):
    raise SystemExit(
        "feature-matrix-gap-hygiene: '- none' is forbidden while unresolved upgrade plan IDs exist"
    )

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
    # When declaring zero gaps, every non-✅ GenesisCode capability row must carry an inline rationale.
    # This prevents silent `none` declarations while partial states still exist.
    for line in lines:
        if (
            not line.startswith("|")
            or line.startswith("|---")
            or line.startswith("| Capability")
        ):
            continue
        cols = [c.strip() for c in line.strip().strip("|").split("|")]
        if len(cols) < 2:
            continue
        capability = cols[0]
        genesis_cell = cols[1]
        if genesis_cell.startswith("✅"):
            continue
        if not (genesis_cell.startswith("⚠️") or genesis_cell.startswith("❌")):
            continue
        if re.search(r"\(.+\)", genesis_cell) is None:
            raise SystemExit(
                "feature-matrix-gap-hygiene: zero-gap state requires explicit rationale for non-✅ row: "
                + capability
            )

print(
    "feature-matrix-gap-hygiene: ok "
    f"(open_ids={len(open_ids_sorted)} mapped_ids={len(section_ids)} readiness_ids={len(readiness_ids_sorted)})"
)
PY
