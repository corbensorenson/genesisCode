#!/usr/bin/env bash
set -euo pipefail

ROOT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
cd "$ROOT_DIR"

python3 - "$ROOT_DIR" <<'PY'
import datetime as dt
import pathlib
import re
import sys

root = pathlib.Path(sys.argv[1])
upgrade_plan = root / "upgrade_plan.md"
feature_matrix = root / "feature_matrix.md"
docs_index = root / "docs" / "INDEX.md"
agent_onboarding = root / "docs" / "AGENT_ONBOARDING_v0.1.md"
deprecation_map = root / "docs" / "DEPRECATION_MAP_v0.1.md"

for p in (upgrade_plan, feature_matrix, docs_index, agent_onboarding, deprecation_map):
    if not p.is_file():
        raise SystemExit(f"planning-docs-fresh: missing file: {p}")

upgrade_text = upgrade_plan.read_text(encoding="utf-8")
feature_text = feature_matrix.read_text(encoding="utf-8")
index_text = docs_index.read_text(encoding="utf-8")
agent_onboarding_text = agent_onboarding.read_text(encoding="utf-8")

upgrade_match = re.search(r"^Last updated:\s+(\d{4}-\d{2}-\d{2})$", upgrade_text, re.MULTILINE)
if not upgrade_match:
    raise SystemExit("planning-docs-fresh: upgrade_plan.md must include `Last updated: YYYY-MM-DD`")
upgrade_date = dt.date.fromisoformat(upgrade_match.group(1))

feature_match = re.search(
    r"^#\s+GenesisCode Feature Matrix\s+\(Audit Date:\s+(\d{4}-\d{2}-\d{2})\)$",
    feature_text,
    re.MULTILINE,
)
if not feature_match:
    raise SystemExit("planning-docs-fresh: feature_matrix.md must include `Audit Date: YYYY-MM-DD` in title")
feature_date = dt.date.fromisoformat(feature_match.group(1))

index_match = re.search(r"^Last updated:\s+(\d{4}-\d{2}-\d{2})$", index_text, re.MULTILINE)
if not index_match:
    raise SystemExit("planning-docs-fresh: docs/INDEX.md must include `Last updated: YYYY-MM-DD`")
index_date = dt.date.fromisoformat(index_match.group(1))

if feature_date < upgrade_date:
    raise SystemExit(
        "planning-docs-fresh: feature_matrix.md audit date is older than upgrade_plan.md last updated"
    )
if index_date < upgrade_date:
    raise SystemExit(
        "planning-docs-fresh: docs/INDEX.md last updated is older than upgrade_plan.md last updated"
    )

if "upgrade_plan.md" not in index_text or "feature_matrix.md" not in index_text:
    raise SystemExit("planning-docs-fresh: docs/INDEX.md must reference upgrade_plan.md and feature_matrix.md")
if "AGENT_ONBOARDING_v0.1.md" not in index_text:
    raise SystemExit("planning-docs-fresh: docs/INDEX.md must reference docs/AGENT_ONBOARDING_v0.1.md")
if "DEPRECATION_MAP_v0.1.md" not in index_text:
    raise SystemExit("planning-docs-fresh: docs/INDEX.md must reference docs/DEPRECATION_MAP_v0.1.md")

if "/upgrade_plan.md" not in feature_text:
    raise SystemExit("planning-docs-fresh: feature_matrix.md evidence list must reference upgrade_plan.md")

for required in (
    "docs/spec/CLI_TOOLING_BUNDLE_v0.1.md",
    "docs/spec/GCPM_BUNDLE_v0.1.md",
    "docs/spec/HOST_RUNTIME_BUNDLE_v0.1.md",
    "docs/spec/TESTING_BUNDLE_v0.1.md",
):
    if required not in agent_onboarding_text:
        raise SystemExit(
            "planning-docs-fresh: docs/AGENT_ONBOARDING_v0.1.md missing canonical bundle reference: "
            + required
        )

print(
    "planning-docs-fresh: ok "
    f"(upgrade_plan={upgrade_date.isoformat()} feature_matrix={feature_date.isoformat()} docs_index={index_date.isoformat()})"
)
PY
