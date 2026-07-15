#!/usr/bin/env python3
"""Validate the rendered GenesisCode Quarto site without network access."""

from __future__ import annotations

import argparse
import hashlib
import json
import re
import sys
from html import unescape
from pathlib import Path
from urllib.parse import unquote, urlsplit
from xml.etree import ElementTree

ROOT = Path(__file__).resolve().parents[1]
SITE = ROOT / "_site"
SITE_URL = "https://corbensorenson.github.io/genesisCode/"
FORBIDDEN_DIRECTORIES = {".genesis", ".quarto", ".tmp", "__pycache__", "node_modules", "target"}
REQUIRED = [
    ".nojekyll", "404.html", "index.html", "learn/documentation-map.html",
    "learn/quickstart.html", "learn/agent-loop.html",
    "guides/agent-authoring.html", "reference/index.html", "reference/symbols.html",
    "reference/capabilities.html", "reference/diagnostics.html", "reference/examples.html",
    "reference/source-catalog.html", "llms.txt", "robots.txt", "sitemap.xml",
    "build-metadata.json", "reference/generated/reference-index.json",
    "site_assets/genesis-social-card.png",
]


def fail(message: str) -> None:
    raise SystemExit(f"quarto-site: {message}")


def file_sha256(path: Path) -> str:
    digest = hashlib.sha256()
    with path.open("rb") as handle:
        for chunk in iter(lambda: handle.read(1024 * 1024), b""):
            digest.update(chunk)
    return digest.hexdigest()


def artifact_summary() -> tuple[int, int, str]:
    metadata_path = SITE / "build-metadata.json"
    files = sorted(
        path for path in SITE.rglob("*")
        if path.is_file() and path != metadata_path and path.name != ".DS_Store"
    )
    digest = hashlib.sha256()
    total_bytes = 0
    for path in files:
        relative = path.relative_to(SITE).as_posix()
        size = path.stat().st_size
        digest.update(relative.encode("utf-8"))
        digest.update(b"\0")
        digest.update(str(size).encode("ascii"))
        digest.update(b"\0")
        digest.update(file_sha256(path).encode("ascii"))
        digest.update(b"\n")
        total_bytes += size
    return len(files), total_bytes, digest.hexdigest()


parser = argparse.ArgumentParser()
parser.add_argument("--expected-commit")
parser.add_argument("--require-clean", action="store_true")
args = parser.parse_args()

if not SITE.is_dir():
    fail("missing _site; run quarto render")
for rel in REQUIRED:
    if not (SITE / rel).is_file():
        fail(f"missing required rendered artifact: {rel}")

index = json.loads((SITE / "reference/generated/reference-index.json").read_text())
counts = index.get("counts", {})
minimums = {
    "symbols": 150,
    "hostOperations": 180,
    "diagnostics": 170,
    "exampleFiles": 120,
    "schemas": 40,
    "canonicalSources": 200,
}
for key, minimum in minimums.items():
    value = counts.get(key)
    if not isinstance(value, int) or value < minimum:
        fail(f"reference index {key}={value!r} below completeness floor {minimum}")

canonical_md = sorted(p for p in (ROOT / "docs").rglob("*.md") if p.name != ".DS_Store")
for source in canonical_md:
    rel = source.relative_to(ROOT).with_suffix(".html")
    if not (SITE / rel).is_file():
        fail(f"canonical Markdown was not rendered: {source.relative_to(ROOT)}")

html_files = sorted(SITE.rglob("*.html"))
if len(html_files) < len(canonical_md) + 20:
    fail(f"rendered HTML inventory unexpectedly small: {len(html_files)}")

forbidden_paths = sorted(
    path.relative_to(SITE) for path in SITE.rglob("*")
    if path.is_dir() and path.name in FORBIDDEN_DIRECTORIES
)
if forbidden_paths:
    fail(f"rendered artifact contains build/cache directory: {forbidden_paths[0]}")

metadata = json.loads((SITE / "build-metadata.json").read_text(encoding="utf-8"))
if metadata.get("schemaVersion") != 1:
    fail("unsupported build metadata schema")
source = metadata.get("source", {})
if args.expected_commit and source.get("commit") != args.expected_commit.lower():
    fail(f"build metadata commit {source.get('commit')!r} != expected {args.expected_commit!r}")
if args.require_clean and source.get("treeState") != "clean":
    fail(f"build metadata tree state is {source.get('treeState')!r}, expected clean")
reference_path = SITE / "reference/generated/reference-index.json"
if metadata.get("referenceIndex", {}).get("sha256") != file_sha256(reference_path):
    fail("build metadata reference-index digest mismatch")
files, total_bytes, artifact_sha256 = artifact_summary()
artifact = metadata.get("artifact", {})
if (artifact.get("files"), artifact.get("bytes"), artifact.get("sha256")) != (files, total_bytes, artifact_sha256):
    fail("build metadata artifact summary mismatch")
if artifact.get("htmlFiles") != len(html_files):
    fail("build metadata HTML inventory mismatch")
if total_bytes > 32 * 1024 * 1024:
    fail(f"rendered artifact is unexpectedly large: {total_bytes} bytes")

href_re = re.compile(r'''\bhref=["']([^"']+)["']''', re.IGNORECASE)
id_re = re.compile(r'''\bid=["']([^"']+)["']''', re.IGNORECASE)
ids_by_page: dict[Path, set[str]] = {}
missing: list[str] = []
missing_fragments: list[str] = []
for page in html_files:
    text = page.read_text(encoding="utf-8")
    ids_by_page[page.resolve()] = {unescape(value) for value in id_re.findall(text)}
    required_structure = [
        "<main", "<title>", 'lang="en-US"', 'name="viewport"',
        'class="gc-skip-link"', 'href="#quarto-document-content"',
    ]
    if any(needle not in text for needle in required_structure):
        fail(f"missing title/main/lang/skip-link structure: {page.relative_to(SITE)}")
    relative = page.relative_to(SITE).as_posix()
    canonical_url = SITE_URL if relative == "index.html" else SITE_URL + relative
    canonical_tag = f'<link rel="canonical" href="{canonical_url}">'
    if text.count(canonical_tag) != 1:
        fail(f"missing or duplicate canonical URL: {relative}")
    for raw in href_re.findall(text):
        href = unescape(raw)
        parsed = urlsplit(href)
        if parsed.scheme or parsed.netloc or href.startswith(("mailto:", "tel:", "javascript:")):
            continue
        path_text = unquote(parsed.path)
        if not path_text:
            target = page
        elif path_text.startswith("/genesisCode/"):
            target = SITE / path_text.removeprefix("/genesisCode/")
        elif path_text.startswith("/"):
            continue
        else:
            target = page.parent / path_text
        if path_text.endswith("/"):
            target = target / "index.html"
        target = target.resolve()
        try:
            target.relative_to(SITE.resolve())
        except ValueError:
            # Repository-source links outside the published corpus are intentionally left as source references.
            continue
        if not target.exists():
            missing.append(f"{page.relative_to(SITE)} -> {href}")
            continue
        if parsed.fragment and target.suffix == ".html":
            fragment = unquote(parsed.fragment)
            target_ids = ids_by_page.get(target)
            if target_ids is None:
                target_text = target.read_text(encoding="utf-8")
                target_ids = {unescape(value) for value in id_re.findall(target_text)}
                ids_by_page[target] = target_ids
            if fragment not in target_ids:
                missing_fragments.append(f"{page.relative_to(SITE)} -> {href}")

if missing:
    sample = " | ".join(missing[:20])
    fail(f"{len(missing)} broken internal links: {sample}")
if missing_fragments:
    sample = " | ".join(missing_fragments[:20])
    fail(f"{len(missing_fragments)} broken internal fragments: {sample}")

sitemap = ElementTree.parse(SITE / "sitemap.xml")
namespace = {"sm": "http://www.sitemaps.org/schemas/sitemap/0.9"}
sitemap_urls = {element.text for element in sitemap.findall(".//sm:loc", namespace)}
expected_sitemap_urls = {
    SITE_URL + page.relative_to(SITE).as_posix()
    for page in html_files if page.name != "404.html"
}
missing_sitemap_urls = sorted(expected_sitemap_urls - sitemap_urls)
if missing_sitemap_urls:
    fail(f"sitemap omits {len(missing_sitemap_urls)} rendered pages: {missing_sitemap_urls[0]}")

robots = (SITE / "robots.txt").read_text(encoding="utf-8")
if f"Sitemap: {SITE_URL}sitemap.xml" not in robots:
    fail("robots.txt does not advertise the canonical sitemap")

home = (SITE / "index.html").read_text(encoding="utf-8")
for needle in [
    "A language agents can reason about", "frozen symbols", "host operations",
    "Start in fifteen minutes", "Know what is ready", "Documentation is part of the product",
    'property="og:title"', 'name="twitter:card"', "genesis-social-card.png",
]:
    if needle not in home:
        fail(f"home page missing required content: {needle!r}")

print(
    "quarto-site: ok "
    f"(html={len(html_files)} canonical_md={len(canonical_md)} "
    f"symbols={counts['symbols']} host_ops={counts['hostOperations']} "
    f"diagnostics={counts['diagnostics']})"
)
