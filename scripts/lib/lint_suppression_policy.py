#!/usr/bin/env python3
"""Reject broad or unreviewable lint suppression in shipped Rust/build policy."""

from __future__ import annotations

import argparse
import re
import sys
from pathlib import Path


ATTRIBUTE = re.compile(r"(?P<inner>#!|#)\s*\[\s*(?P<kind>allow|expect)\s*\((?P<body>.*?)\)\s*\]", re.DOTALL)
BROAD_BUILD_FLAGS = (
    re.compile(r"(?:^|\s)-A\s+warnings(?:\s|$)"),
    re.compile(r"--allow(?:=|\s+)warnings(?:\s|$)"),
    re.compile(r"--cap-lints(?:=|\s+)allow(?:\s|$)"),
)


def rust_violations(path: Path, text: str) -> list[str]:
    violations: list[str] = []
    for match in ATTRIBUTE.finditer(text):
        body = match.group("body")
        if "clippy::" not in body and not re.search(r"(?:^|,)\s*warnings\b", body):
            continue
        line = text.count("\n", 0, match.start()) + 1
        location = f"{path}:{line}"
        if match.group("inner") == "#!":
            violations.append(f"{location}: module-level lint suppression is forbidden")
        elif match.group("kind") == "allow":
            violations.append(
                f"{location}: use a narrow #[expect(..., reason = \"...\")] after review"
            )
        elif "clippy::all" in body or re.search(r"(?:^|,)\s*warnings\b", body):
            violations.append(f"{location}: broad warning suppression is forbidden")
        elif not re.search(r"\breason\s*=\s*\"[^\"]+\"", body):
            violations.append(f"{location}: lint expectation requires a non-empty reason")
    return violations


def build_policy_violations(path: Path, text: str) -> list[str]:
    violations: list[str] = []
    for pattern in BROAD_BUILD_FLAGS:
        for match in pattern.finditer(text):
            line = text.count("\n", 0, match.start()) + 1
            violations.append(f"{path}:{line}: build-wide warning suppression is forbidden")
    if re.search(r"(?ms)^\s*warnings\s*=\s*(?:\"allow\"|\{[^}]*level\s*=\s*\"allow\")", text):
        violations.append(f"{path}: Cargo warning lint level may not be allow")
    if re.search(r"(?ms)^\s*clippy::all\s*=\s*(?:\"allow\"|\{[^}]*level\s*=\s*\"allow\")", text):
        violations.append(f"{path}: Cargo clippy::all lint level may not be allow")
    return violations


def scan(root: Path) -> list[str]:
    violations: list[str] = []
    rust_roots = [root / "crates", root / "tools", root / "build.rs"]
    rust_paths: list[Path] = []
    for candidate in rust_roots:
        if candidate.is_file():
            rust_paths.append(candidate)
        elif candidate.is_dir():
            rust_paths.extend(
                path for path in candidate.rglob("*.rs") if "target" not in path.parts
            )
    for path in sorted(rust_paths):
        violations.extend(rust_violations(path.relative_to(root), path.read_text()))

    build_paths = [root / "Cargo.toml"]
    build_paths.extend((root / "crates").glob("*/Cargo.toml"))
    build_paths.extend((root / "tools").glob("*/Cargo.toml"))
    build_paths.extend((root / ".cargo").glob("*.toml"))
    build_paths.extend((root / ".github" / "workflows").glob("*.yml"))
    build_paths.extend((root / ".github" / "workflows").glob("*.yaml"))
    for path in sorted({path for path in build_paths if path.is_file()}):
        violations.extend(
            build_policy_violations(path.relative_to(root), path.read_text())
        )
    return violations


def self_test() -> None:
    rejected = {
        "module": "#![allow(clippy::all)]\n",
        "item_allow": "#[allow(clippy::too_many_arguments)]\nfn f() {}\n",
        "missing_reason": "#[expect(clippy::too_many_arguments)]\nfn f() {}\n",
        "broad_expect": '#[expect(clippy::all, reason = "temporary")]\nfn f() {}\n',
    }
    for name, fixture in rejected.items():
        if not rust_violations(Path(f"negative-{name}.rs"), fixture):
            raise RuntimeError(f"negative control unexpectedly accepted: {name}")
    accepted = '#[expect(clippy::too_many_arguments, reason = "protocol ABI is fixed")]\nfn f() {}\n'
    if rust_violations(Path("positive-reviewed.rs"), accepted):
        raise RuntimeError("reviewed narrow expectation was rejected")
    for name, fixture in {
        "allow-warning": "cargo clippy -A warnings\n",
        "cap-lints": "RUSTFLAGS=--cap-lints=allow\n",
        "cargo-level": '[workspace.lints.rust]\nwarnings = "allow"\n',
    }.items():
        if not build_policy_violations(Path(f"negative-{name}.toml"), fixture):
            raise RuntimeError(f"build-policy negative control unexpectedly accepted: {name}")


def main() -> int:
    parser = argparse.ArgumentParser()
    parser.add_argument("--root", type=Path, default=Path.cwd())
    args = parser.parse_args()
    self_test()
    violations = scan(args.root.resolve())
    if violations:
        print("lint-suppression-policy: rejected", file=sys.stderr)
        for violation in violations:
            print(f"- {violation}", file=sys.stderr)
        return 1
    print("lint-suppression-policy: ok (7 negative controls)")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
