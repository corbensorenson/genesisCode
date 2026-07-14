#!/usr/bin/env python3
"""Attest the public GenesisCode Pages deployment after publication."""

from __future__ import annotations

import argparse
import json
import time
from urllib.error import HTTPError
from urllib.parse import urljoin
from urllib.request import Request, urlopen


def fetch(base_url: str, path: str, expected_status: int = 200) -> bytes:
    request = Request(urljoin(base_url, path), headers={"User-Agent": "GenesisCode-Pages-Attestor/1"})
    try:
        with urlopen(request, timeout=20) as response:
            status = response.status
            body = response.read()
    except HTTPError as error:
        status = error.code
        body = error.read()
    if status != expected_status:
        raise ValueError(f"{path} returned HTTP {status}, expected {expected_status}")
    return body


def attest(base_url: str, expected_commit: str) -> None:
    pages = {
        "index.html": "A language agents can reason about",
        "learn/quickstart.html": "From checkout to verified output",
        "reference/index.html": "Exhaustive reference",
        "llms.txt": "GenesisCode documentation index for language models",
        "sitemap.xml": "/genesisCode/reference/symbols.html",
    }
    for path, needle in pages.items():
        body = fetch(base_url, path).decode("utf-8")
        if needle not in body:
            raise ValueError(f"{path} is missing {needle!r}")

    reference = json.loads(fetch(base_url, "reference/generated/reference-index.json"))
    if reference.get("counts", {}).get("symbols", 0) < 150:
        raise ValueError("deployed reference index is incomplete")

    metadata = json.loads(fetch(base_url, "build-metadata.json"))
    actual_commit = metadata.get("source", {}).get("commit")
    if actual_commit != expected_commit:
        raise ValueError(f"deployed commit {actual_commit!r} != expected {expected_commit!r}")
    if metadata.get("source", {}).get("treeState") != "clean":
        raise ValueError("deployed artifact was not produced from a clean source tree")

    missing = fetch(base_url, "__genesiscode_missing_page_attestation__", expected_status=404).decode("utf-8")
    if "This path is not part of the current language map" not in missing:
        raise ValueError("custom 404 recovery page is not active")


parser = argparse.ArgumentParser()
parser.add_argument("--url", required=True)
parser.add_argument("--expected-commit", required=True)
parser.add_argument("--attempts", type=int, default=12)
parser.add_argument("--retry-delay", type=float, default=10.0)
args = parser.parse_args()

if len(args.expected_commit) != 40:
    raise SystemExit("quarto-deployment: expected commit must be a 40-character SHA")

base_url = args.url.rstrip("/") + "/"
for attempt in range(1, args.attempts + 1):
    try:
        attest(base_url, args.expected_commit.lower())
        print(f"quarto-deployment: ok (url={base_url} commit={args.expected_commit[:12]})")
        break
    except Exception as error:
        if attempt == args.attempts:
            raise SystemExit(f"quarto-deployment: failed after {attempt} attempts: {error}") from error
        print(f"quarto-deployment: attempt {attempt}/{args.attempts} failed: {error}")
        time.sleep(args.retry_delay)
