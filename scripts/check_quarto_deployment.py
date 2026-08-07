#!/usr/bin/env python3
"""Attest the public GenesisCode Pages deployment after publication."""

from __future__ import annotations

import argparse
import json
import tempfile
import time
from pathlib import Path
from urllib.error import HTTPError
from urllib.parse import urljoin, urlsplit
from urllib.request import Request, urlopen
from xml.etree import ElementTree

from lib.quarto_site_contract import TOP_LEVEL_MARKDOWN_SOURCES, verify_html_inventory


ROOT = Path(__file__).resolve().parents[1]


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


def validate_metadata(metadata: object, expected_commit: str, root: Path) -> int:
    if not isinstance(metadata, dict):
        raise ValueError("build metadata must be an object")
    actual_commit = metadata.get("source", {}).get("commit")
    if actual_commit != expected_commit:
        raise ValueError(f"deployed commit {actual_commit!r} != expected {expected_commit!r}")
    if metadata.get("source", {}).get("treeState") != "clean":
        raise ValueError("deployed artifact was not produced from a clean source tree")
    html_files = metadata.get("artifact", {}).get("htmlFiles")
    verify_html_inventory(html_files, root)
    return html_files


def validate_sitemap_inventory(sitemap: bytes, base_url: str, html_files: int) -> None:
    document = ElementTree.fromstring(sitemap)
    namespace = {"sm": "http://www.sitemaps.org/schemas/sitemap/0.9"}
    locations = [element.text for element in document.findall(".//sm:loc", namespace)]
    if any(not isinstance(location, str) or not location.startswith(base_url) for location in locations):
        raise ValueError("sitemap contains a missing or non-canonical location")
    if len(set(locations)) != len(locations):
        raise ValueError("sitemap contains duplicate locations")
    # The custom 404 page is intentionally excluded from the sitemap.
    if len(locations) != html_files - 1:
        raise ValueError(
            f"sitemap inventory {len(locations)} does not match stamped HTML inventory {html_files}"
        )


def attest(base_url: str, expected_commit: str, root: Path = ROOT) -> None:
    html_pages = {
        "index.html": "A language agents can reason about",
        "learn/documentation-map.html": "Choose the smallest trustworthy path",
        "learn/quickstart.html": "From checkout to verified output",
        "reference/index.html": "Exhaustive reference",
    }
    for path, needle in html_pages.items():
        body = fetch(base_url, path).decode("utf-8")
        if needle not in body:
            raise ValueError(f"{path} is missing {needle!r}")
        canonical = base_url + ("" if path == "index.html" else path)
        if f'<link rel="canonical" href="{canonical}">' not in body:
            raise ValueError(f"{path} has no deployment-correct canonical URL")

    llms = fetch(base_url, "llms.txt").decode("utf-8")
    if "GenesisCode documentation index for language models" not in llms:
        raise ValueError("llms.txt is missing its machine-readable title")
    if base_url not in llms:
        raise ValueError("llms.txt does not advertise the canonical deployment base URL")

    sitemap = fetch(base_url, "sitemap.xml")
    canonical_symbol = f"<loc>{base_url}reference/symbols.html</loc>"
    if canonical_symbol not in sitemap.decode("utf-8"):
        raise ValueError("sitemap.xml has no deployment-correct symbol URL")

    social_card = fetch(base_url, "site_assets/genesis-social-card.png")
    if not social_card.startswith(b"\x89PNG\r\n\x1a\n") or len(social_card) < 10_000:
        raise ValueError("social card is missing or malformed")

    reference = json.loads(fetch(base_url, "reference/generated/reference-index.json"))
    if reference.get("counts", {}).get("symbols", 0) < 150:
        raise ValueError("deployed reference index is incomplete")

    metadata = json.loads(fetch(base_url, "build-metadata.json"))
    html_files = validate_metadata(metadata, expected_commit, root)
    validate_sitemap_inventory(sitemap, base_url, html_files)

    missing = fetch(base_url, "__genesiscode_missing_page_attestation__", expected_status=404).decode("utf-8")
    if "This path is not part of the current language map" not in missing:
        raise ValueError("custom 404 recovery page is not active")


def self_test() -> None:
    commit = "a" * 40
    with tempfile.TemporaryDirectory(prefix="genesis-quarto-contract-") as directory:
        root = Path(directory)
        (root / "docs" / "nested").mkdir(parents=True)
        for source_directory in ("learn", "guides", "reference"):
            (root / source_directory).mkdir()
            (root / source_directory / "index.qmd").write_text("index\n", encoding="utf-8")
        (root / "docs" / "one.md").write_text("one\n", encoding="utf-8")
        (root / "docs" / "nested" / "two.md").write_text("two\n", encoding="utf-8")
        for source in ("index.qmd", "404.qmd", *TOP_LEVEL_MARKDOWN_SOURCES):
            (root / source).write_text("source\n", encoding="utf-8")
        expected = 14
        metadata = {
            "source": {"commit": commit, "treeState": "clean"},
            "artifact": {"htmlFiles": expected},
        }
        if validate_metadata(metadata, commit, root) != expected:
            raise ValueError("valid metadata control failed")
        for value in (expected - 1, expected + 1, True):
            invalid = json.loads(json.dumps(metadata))
            invalid["artifact"]["htmlFiles"] = value
            try:
                validate_metadata(invalid, commit, root)
            except ValueError:
                pass
            else:
                raise ValueError("invalid HTML inventory was accepted")
        for source_field, value in (("commit", "b" * 40), ("treeState", "dirty")):
            invalid = json.loads(json.dumps(metadata))
            invalid["source"][source_field] = value
            try:
                validate_metadata(invalid, commit, root)
            except ValueError:
                pass
            else:
                raise ValueError(f"invalid {source_field} was accepted")
        sitemap = (
            '<?xml version="1.0"?><urlset xmlns="http://www.sitemaps.org/schemas/sitemap/0.9">'
            + "".join(
                f"<url><loc>https://example.test/{index}.html</loc></url>"
                for index in range(expected - 1)
            )
            + "</urlset>"
        ).encode("utf-8")
        validate_sitemap_inventory(sitemap, "https://example.test/", expected)
        try:
            validate_sitemap_inventory(
                sitemap.replace(b"1.html", b"0.html", 1),
                "https://example.test/",
                expected,
            )
        except ValueError:
            pass
        else:
            raise ValueError("duplicate sitemap location was accepted")
    print("quarto-deployment: self-test ok (8 controls)")


def main() -> int:
    parser = argparse.ArgumentParser()
    parser.add_argument("--url")
    parser.add_argument("--expected-commit")
    parser.add_argument("--attempts", type=int, default=12)
    parser.add_argument("--retry-delay", type=float, default=10.0)
    parser.add_argument("--self-test", action="store_true")
    args = parser.parse_args()

    if args.self_test:
        if args.url or args.expected_commit:
            parser.error("--self-test does not accept deployment arguments")
        self_test()
        return 0
    if not args.url or not args.expected_commit:
        parser.error("--url and --expected-commit are required")
    if len(args.expected_commit) != 40:
        raise SystemExit("quarto-deployment: expected commit must be a 40-character SHA")
    if args.attempts < 1 or args.retry_delay < 0:
        parser.error("--attempts must be positive and --retry-delay must be non-negative")

    base_url = args.url.rstrip("/") + "/"
    parsed_base = urlsplit(base_url)
    if parsed_base.scheme not in {"http", "https"} or not parsed_base.netloc:
        parser.error("--url must be an absolute HTTP(S) URL")
    for attempt in range(1, args.attempts + 1):
        try:
            attest(base_url, args.expected_commit.lower())
            print(f"quarto-deployment: ok (url={base_url} commit={args.expected_commit[:12]})")
            return 0
        except Exception as error:
            if attempt == args.attempts:
                raise SystemExit(f"quarto-deployment: failed after {attempt} attempts: {error}") from error
            print(f"quarto-deployment: attempt {attempt}/{args.attempts} failed: {error}")
            time.sleep(args.retry_delay)
    return 1


if __name__ == "__main__":
    raise SystemExit(main())
