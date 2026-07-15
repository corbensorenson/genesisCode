#!/usr/bin/env bash
set -euo pipefail

ROOT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
cd "$ROOT_DIR"

POLICY_FILE="${GENESIS_DOC_COMPLEXITY_POLICY:-policies/docs/doc_complexity_budget.toml}"
FEATURE_MATRIX_FILE="feature_matrix.md"
DEPRECATION_MAP_FILE="docs/DEPRECATION_MAP_v0.1.md"
OWNERSHIP_FILE="docs/spec/DOC_LEAF_OWNERSHIP_v0.1.md"
REPORT_FILE="${1:?usage: scripts/render_doc_complexity_report.sh <output.json>}"

for required in \
  "$POLICY_FILE" \
  "$FEATURE_MATRIX_FILE" \
  "$DEPRECATION_MAP_FILE" \
  "$OWNERSHIP_FILE"
do
  [[ -f "$required" ]] || {
    echo "doc-complexity-budget: missing required file: $required" >&2
    exit 1
  }
done

python3 - "$ROOT_DIR" "$POLICY_FILE" "$FEATURE_MATRIX_FILE" "$DEPRECATION_MAP_FILE" "$OWNERSHIP_FILE" "$REPORT_FILE" <<'PY'
import json
import pathlib
import re
import subprocess
import sys
root = pathlib.Path(sys.argv[1]).resolve()
sys.path.insert(0, str(root / "scripts/lib"))
from toml_compat import tomllib
policy_path = root / sys.argv[2]
matrix_path = root / sys.argv[3]
deprecation_map_path = root / sys.argv[4]
ownership_path = root / sys.argv[5]
report_path = root / sys.argv[6]

policy = tomllib.loads(policy_path.read_text(encoding="utf-8"))
version = policy.get("version")
if version != 1:
    raise SystemExit("doc-complexity-budget: policy version must equal 1")

def require_positive_int(key: str) -> int:
    value = policy.get(key)
    if not isinstance(value, int) or value <= 0:
        raise SystemExit(f"doc-complexity-budget: {key} must be a positive integer")
    return value

def require_positive_float(key: str) -> float:
    value = policy.get(key)
    if not isinstance(value, (int, float)) or float(value) <= 0:
        raise SystemExit(f"doc-complexity-budget: {key} must be a positive number")
    return float(value)

max_active_docs_md = require_positive_int("max_active_docs_md")
max_active_top_level_docs_md = require_positive_int("max_active_top_level_docs_md")
max_capability_retrieval_fanout = require_positive_float("max_capability_retrieval_fanout")

inventory = subprocess.run(
    [
        "git",
        "ls-files",
        "-z",
        "--cached",
        "--others",
        "--exclude-standard",
        "--",
        "*.md",
    ],
    cwd=root,
    check=True,
    stdout=subprocess.PIPE,
).stdout
relative_md_files = [
    pathlib.Path(raw.decode("utf-8"))
    for raw in inventory.split(b"\0")
    if raw
]
all_md_files = sorted(
    root / relative
    for relative in relative_md_files
    if not relative.is_absolute()
    and ".." not in relative.parts
    and (root / relative).is_file()
)
docs_md_files = [
    path
    for path in all_md_files
    if path.relative_to(root).parts[0] == "docs"
]

def is_deprecated_stub(path: pathlib.Path) -> bool:
    return "Deprecated Top-Level Doc:" in path.read_text(encoding="utf-8")

active_docs = [path for path in docs_md_files if not is_deprecated_stub(path)]
active_docs_md = len(active_docs)
docs_md_total = len(docs_md_files)
total_md = len(all_md_files)

infra_top_level = {"INDEX.md", "DEPRECATION_MAP_v0.1.md"}
top_level_docs = sorted((root / "docs").glob("*.md"))
active_top_level = [
    path
    for path in top_level_docs
    if path.name not in infra_top_level and not is_deprecated_stub(path)
]
active_top_level_rel = sorted(path.relative_to(root).as_posix() for path in active_top_level)

deprecation_map = deprecation_map_path.read_text(encoding="utf-8")
active_section_match = re.search(
    r"## Active Top-Level References \(Not Deprecated\)\n(?P<body>.*?)(?:\n## |\Z)",
    deprecation_map,
    flags=re.DOTALL,
)
if not active_section_match:
    raise SystemExit(
        "doc-complexity-budget: docs/DEPRECATION_MAP_v0.1.md missing "
        "'Active Top-Level References (Not Deprecated)' section"
    )
active_top_level_from_map = sorted(
    set(re.findall(r"- `([^`]+)`", active_section_match.group("body")))
)

if active_top_level_rel != active_top_level_from_map:
    missing_in_map = sorted(set(active_top_level_rel) - set(active_top_level_from_map))
    stale_in_map = sorted(set(active_top_level_from_map) - set(active_top_level_rel))
    details = []
    if missing_in_map:
        details.append("missing_in_map=" + ",".join(missing_in_map))
    if stale_in_map:
        details.append("stale_in_map=" + ",".join(stale_in_map))
    raise SystemExit(
        "doc-complexity-budget: deprecation map active top-level set drifted: "
        + " ".join(details)
    )

ownership_text = ownership_path.read_text(encoding="utf-8")
row_pattern = re.compile(
    r"^\|\s*`(?P<leaf>docs/[^`]+\.md)`\s*\|\s*(?P<owner>[^|]+?)\s*\|\s*(?P<sources>.+?)\s*\|$",
    re.MULTILINE,
)
ownership_rows = list(row_pattern.finditer(ownership_text))
if not ownership_rows:
    raise SystemExit("doc-complexity-budget: ownership table has no rows")

ownership_entries = {}
for row in ownership_rows:
    leaf = row.group("leaf")
    owner = row.group("owner").strip()
    sources = row.group("sources")
    if not owner:
        raise SystemExit(
            f"doc-complexity-budget: ownership row has empty owner for leaf: {leaf}"
        )
    source_paths = [value for value in re.findall(r"`([^`]+)`", sources) if value.startswith("docs/")]
    if not source_paths:
        raise SystemExit(
            "doc-complexity-budget: ownership row must include at least one canonical "
            f"source path for leaf: {leaf}"
        )
    for source in source_paths:
        if not (root / source).is_file():
            raise SystemExit(
                "doc-complexity-budget: ownership canonical source path does not exist: "
                + source
            )
    if leaf in ownership_entries:
        raise SystemExit(f"doc-complexity-budget: duplicate ownership row for leaf: {leaf}")
    ownership_entries[leaf] = {
        "owner": owner,
        "sources": source_paths,
    }

ownership_leafs = sorted(ownership_entries.keys())
if ownership_leafs != active_top_level_rel:
    missing_in_ownership = sorted(set(active_top_level_rel) - set(ownership_leafs))
    stale_in_ownership = sorted(set(ownership_leafs) - set(active_top_level_rel))
    details = []
    if missing_in_ownership:
        details.append("missing_in_ownership=" + ",".join(missing_in_ownership))
    if stale_in_ownership:
        details.append("stale_in_ownership=" + ",".join(stale_in_ownership))
    raise SystemExit(
        "doc-complexity-budget: ownership registry top-level leaf set drifted: "
        + " ".join(details)
    )

feature_matrix = matrix_path.read_text(encoding="utf-8")
feature_lines = feature_matrix.splitlines()
capability_rows = 0
for line in feature_lines:
    stripped = line.strip()
    if not stripped.startswith("|"):
        continue
    if stripped.startswith("|---"):
        continue
    if "Capability | GenesisCode" in stripped:
        continue
    capability_rows += 1

if capability_rows == 0:
    raise SystemExit("doc-complexity-budget: feature matrix capability table has zero rows")

try:
    evidence_start = feature_lines.index("Primary evidence paths:") + 1
except ValueError:
    raise SystemExit("doc-complexity-budget: feature_matrix.md missing 'Primary evidence paths:' section")

evidence_paths = [
    line[2:].strip()
    for line in feature_lines[evidence_start:]
    if line.startswith("- ")
]
if not evidence_paths:
    raise SystemExit("doc-complexity-budget: feature_matrix.md has zero primary evidence paths")

capability_retrieval_fanout = len(evidence_paths) / capability_rows

errors = []
if active_docs_md > max_active_docs_md:
    errors.append(
        f"active_docs_md {active_docs_md} exceeds budget {max_active_docs_md}"
    )
if len(active_top_level_rel) > max_active_top_level_docs_md:
    errors.append(
        "active_top_level_docs_md "
        f"{len(active_top_level_rel)} exceeds budget {max_active_top_level_docs_md}"
    )
if capability_retrieval_fanout > max_capability_retrieval_fanout:
    errors.append(
        "capability_retrieval_fanout "
        f"{capability_retrieval_fanout:.6f} exceeds budget {max_capability_retrieval_fanout:.6f}"
    )

report = {
    "kind": "genesis/doc-complexity-v0.1",
    "policy_path": policy_path.relative_to(root).as_posix(),
    "docs_md_total": docs_md_total,
    "total_md": total_md,
    "deprecated_stub_docs": docs_md_total - active_docs_md,
    "active_docs_md": active_docs_md,
    "active_top_level_leaf_docs": len(active_top_level_rel),
    "active_top_level_leaf_paths": active_top_level_rel,
    "capability_rows": capability_rows,
    "primary_evidence_paths": len(evidence_paths),
    "capability_retrieval_fanout": capability_retrieval_fanout,
    "budgets": {
        "max_active_docs_md": max_active_docs_md,
        "max_active_top_level_docs_md": max_active_top_level_docs_md,
        "max_capability_retrieval_fanout": max_capability_retrieval_fanout,
    },
    "ok": len(errors) == 0,
    "errors": errors,
}
report_path.parent.mkdir(parents=True, exist_ok=True)
report_path.write_text(json.dumps(report, indent=2, sort_keys=True) + "\n", encoding="utf-8")

if errors:
    raise SystemExit("doc-complexity-budget: " + " | ".join(errors))

print(
    "doc-complexity-budget: ok "
    f"(active_docs_md={active_docs_md} active_top_level_leaf_docs={len(active_top_level_rel)} "
    f"fanout={capability_retrieval_fanout:.6f})"
)
PY
