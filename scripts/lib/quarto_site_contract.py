#!/usr/bin/env python3
"""Shared source-bound inventory rules for the GenesisCode documentation site."""

from __future__ import annotations

from pathlib import Path


TOP_LEVEL_MARKDOWN_SOURCES = (
    "README.md",
    "CHANGELOG.md",
    "ROADMAP.md",
    "feature_matrix.md",
    "upgrade_plan.md",
    "genesisCode.md",
    "AGENTS.md",
)
QUARTO_SOURCE_DIRECTORIES = ("learn", "guides", "reference")


def canonical_markdown_sources(root: Path) -> list[Path]:
    docs = root / "docs"
    return sorted(path for path in docs.rglob("*.md") if path.name != ".DS_Store")


def canonical_render_sources(root: Path) -> list[Path]:
    sources = set(canonical_markdown_sources(root))
    sources.update(root / path for path in TOP_LEVEL_MARKDOWN_SOURCES)
    sources.update(root / path for path in ("index.qmd", "404.qmd"))
    for directory in QUARTO_SOURCE_DIRECTORIES:
        sources.update((root / directory).rglob("*.qmd"))
    missing = sorted(path.relative_to(root) for path in sources if not path.is_file())
    if missing:
        raise ValueError(f"declared Quarto source is missing: {missing[0]}")
    return sorted(sources)


def verify_html_inventory(actual: object, root: Path) -> int:
    expected = len(canonical_render_sources(root))
    if isinstance(actual, bool) or not isinstance(actual, int):
        raise ValueError("documentation HTML inventory must be an integer")
    if actual != expected:
        raise ValueError(
            f"documentation HTML inventory {actual} does not match render-source inventory {expected}"
        )
    return expected
