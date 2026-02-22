#!/usr/bin/env bash
set -euo pipefail

ROOT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
cd "$ROOT_DIR"

MAP_FILE="docs/DEPRECATION_MAP_v0.1.md"
if [[ ! -f "$MAP_FILE" ]]; then
  echo "doc-hygiene: missing deprecation map: $MAP_FILE" >&2
  exit 1
fi

python3 - "$ROOT_DIR" "$MAP_FILE" <<'PY'
import pathlib
import re
import sys

root = pathlib.Path(sys.argv[1]).resolve()
map_arg = pathlib.Path(sys.argv[2])
map_rel = map_arg.as_posix()
map_path = (root / map_arg).resolve() if not map_arg.is_absolute() else map_arg.resolve()
map_text = map_path.read_text(encoding="utf-8")

row_pattern = re.compile(
    r"^\|\s*`(?P<deprecated>docs/[^`]+\.md)`\s*\|\s*(?P<replacements>.+?)\s*\|\s*(?P<status>[^|]+)\|$",
    re.MULTILINE,
)
rows = list(row_pattern.finditer(map_text))
deprecated_docs = {}
for row in rows:
    deprecated_rel = row.group("deprecated")
    replacements_cell = row.group("replacements")
    replacement_paths = [
        value
        for value in re.findall(r"`([^`]+)`", replacements_cell)
        if value.startswith("docs/")
    ]
    if not replacement_paths:
        raise SystemExit(
            "doc-hygiene: deprecation row has no backticked replacement paths: "
            + deprecated_rel
        )
    deprecated_docs[deprecated_rel] = replacement_paths

required_headers = {
    "## Status",
    "## Canonical Replacements",
    "## Migration Guidance",
}
max_stub_lines = 80

for deprecated_rel, replacement_paths in deprecated_docs.items():
    deprecated_path = root / deprecated_rel
    if not deprecated_path.is_file():
        raise SystemExit(f"doc-hygiene: deprecated doc missing: {deprecated_rel}")
    text = deprecated_path.read_text(encoding="utf-8")

    if "Deprecated Top-Level Doc:" not in text:
        raise SystemExit(
            f"doc-hygiene: deprecated doc missing deprecation banner: {deprecated_rel}"
        )
    if "Bundle Entry:" not in text:
        raise SystemExit(
            f"doc-hygiene: deprecated doc missing Bundle Entry marker: {deprecated_rel}"
        )
    if "Legacy Split Doc:" not in text:
        raise SystemExit(
            f"doc-hygiene: deprecated doc missing Legacy Split Doc marker: {deprecated_rel}"
        )

    lines = text.splitlines()
    if len(lines) > max_stub_lines:
        raise SystemExit(
            f"doc-hygiene: deprecated doc exceeds stub size budget ({len(lines)} > {max_stub_lines}): "
            + deprecated_rel
        )

    headings = set(re.findall(r"^##\s+.+$", text, flags=re.MULTILINE))
    missing_required_headers = sorted(required_headers - headings)
    if missing_required_headers:
        raise SystemExit(
            "doc-hygiene: deprecated doc missing required stub section(s): "
            + deprecated_rel
            + " -> "
            + ", ".join(missing_required_headers)
        )
    extra_headers = sorted(h for h in headings if h not in required_headers)
    if extra_headers:
        raise SystemExit(
            "doc-hygiene: deprecated doc contains non-stub section(s): "
            + deprecated_rel
            + " -> "
            + ", ".join(extra_headers)
        )

    for replacement_rel in replacement_paths:
        replacement_path = root / replacement_rel
        if not replacement_path.is_file():
            raise SystemExit(
                "doc-hygiene: replacement path in deprecation map does not exist: "
                + replacement_rel
            )
        if replacement_rel not in text:
            raise SystemExit(
                "doc-hygiene: deprecated stub must explicitly list replacement path "
                + replacement_rel
                + " in "
                + deprecated_rel
            )

deprecated_names = [pathlib.Path(p).name for p in deprecated_docs]
docs_root = root / "docs"
for candidate in docs_root.rglob("*.md"):
    rel = candidate.relative_to(root).as_posix()
    if rel == map_rel:
        continue
    if rel in deprecated_docs:
        continue
    body = candidate.read_text(encoding="utf-8")
    for deprecated_name in deprecated_names:
        if deprecated_name in body:
            raise SystemExit(
                "doc-hygiene: stale reference to deprecated doc "
                + deprecated_name
                + " found in "
                + rel
            )

print(
    "doc-hygiene: ok "
    f"(deprecated_docs={len(deprecated_docs)} stale_reference_scan=passed)"
)
PY
