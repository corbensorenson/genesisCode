#!/usr/bin/env python3
"""Write deterministic provenance for a rendered GenesisCode documentation site."""

from __future__ import annotations

import hashlib
import json
import os
import re
import shutil
import subprocess
from pathlib import Path

ROOT = Path(__file__).resolve().parents[1]
SITE = ROOT / "_site"
OUTPUT = SITE / "build-metadata.json"
FORBIDDEN_DIRECTORIES = {".genesis", ".quarto", ".tmp", "__pycache__", "node_modules", "target"}
SITE_URL = "https://corbensorenson.github.io/genesisCode/"


def fail(message: str) -> None:
    raise SystemExit(f"quarto-stamp: {message}")


def file_sha256(path: Path) -> str:
    digest = hashlib.sha256()
    with path.open("rb") as handle:
        for chunk in iter(lambda: handle.read(1024 * 1024), b""):
            digest.update(chunk)
    return digest.hexdigest()


def artifact_summary() -> tuple[int, int, str]:
    files = sorted(
        path for path in SITE.rglob("*")
        if path.is_file() and path != OUTPUT and path.name != ".DS_Store"
    )
    digest = hashlib.sha256()
    total_bytes = 0
    for path in files:
        relative = path.relative_to(SITE).as_posix()
        size = path.stat().st_size
        sha256 = file_sha256(path)
        digest.update(relative.encode("utf-8"))
        digest.update(b"\0")
        digest.update(str(size).encode("ascii"))
        digest.update(b"\0")
        digest.update(sha256.encode("ascii"))
        digest.update(b"\n")
        total_bytes += size
    return len(files), total_bytes, digest.hexdigest()


def git(*args: str) -> str:
    result = subprocess.run(
        ["git", *args], cwd=ROOT, check=True, text=True,
        stdout=subprocess.PIPE, stderr=subprocess.PIPE,
    )
    return result.stdout.strip()


def stamp_canonical_urls() -> None:
    canonical_re = re.compile(
        r'<link\b(?=[^>]*\brel=["\']canonical["\'])[^>]*>', re.IGNORECASE,
    )
    skip_link = '<a class="gc-skip-link" href="#quarto-document-content">Skip to main content</a>'
    body_re = re.compile(r"(<body\b[^>]*>)", re.IGNORECASE)
    for page in sorted(SITE.rglob("*.html")):
        relative = page.relative_to(SITE).as_posix()
        canonical = SITE_URL if relative == "index.html" else SITE_URL + relative
        tag = f'<link rel="canonical" href="{canonical}">'
        text = page.read_text(encoding="utf-8")
        if canonical_re.search(text):
            updated = canonical_re.sub(tag, text, count=1)
        elif "</head>" in text:
            updated = text.replace("</head>", f"{tag}\n</head>", 1)
        else:
            fail(f"rendered page has no head element: {relative}")
        if updated.count(skip_link) != 1:
            fail(f"rendered page must contain exactly one skip link: {relative}")
        updated = updated.replace(skip_link, "", 1)
        if not body_re.search(updated):
            fail(f"rendered page has no body element: {relative}")
        updated = body_re.sub(rf"\1\n{skip_link}", updated, count=1)
        if updated != text:
            page.write_text(updated, encoding="utf-8")


if not SITE.is_dir():
    fail("missing _site; run quarto render")

commit = os.environ.get("GITHUB_SHA") or git("rev-parse", "HEAD")
if len(commit) != 40 or any(char not in "0123456789abcdefABCDEF" for char in commit):
    fail(f"invalid source commit: {commit!r}")
commit = commit.lower()
tree_state = "clean" if not git("status", "--porcelain", "--untracked-files=all") else "dirty"

# Resource discovery must never publish local build/cache state alongside source examples.
for directory in sorted(
    (path for path in SITE.rglob("*") if path.is_dir() and path.name in FORBIDDEN_DIRECTORIES),
    key=lambda path: len(path.parts),
    reverse=True,
):
    shutil.rmtree(directory)
for garbage in SITE.rglob("*"):
    if garbage.is_file() and (garbage.name == ".DS_Store" or garbage.suffix == ".pyc"):
        garbage.unlink()

stamp_canonical_urls()

reference_path = SITE / "reference/generated/reference-index.json"
if not reference_path.is_file():
    fail("missing rendered reference index")
reference_index = json.loads(reference_path.read_text(encoding="utf-8"))
files, total_bytes, artifact_sha256 = artifact_summary()

metadata = {
    "schemaVersion": 1,
    "site": SITE_URL,
    "source": {"commit": commit, "treeState": tree_state},
    "referenceIndex": {
        "path": "reference/generated/reference-index.json",
        "sha256": file_sha256(reference_path),
        "counts": reference_index.get("counts", {}),
    },
    "artifact": {
        "files": files,
        "bytes": total_bytes,
        "sha256": artifact_sha256,
        "htmlFiles": len(list(SITE.rglob("*.html"))),
    },
}
OUTPUT.write_text(json.dumps(metadata, indent=2, sort_keys=True) + "\n", encoding="utf-8")
print(
    "quarto-stamp: wrote build-metadata.json "
    f"(commit={commit[:12]} tree={tree_state} files={files} sha256={artifact_sha256[:12]})"
)
