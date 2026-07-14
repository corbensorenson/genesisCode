#!/usr/bin/env python3
"""Compiler-free production panic policy and workspace coverage audit."""

from __future__ import annotations

import argparse
from hashlib import sha256
import json
from pathlib import Path, PurePosixPath
import re
import sys
from typing import Any, Dict, Iterable, List, Optional, Sequence, Set, Tuple


class PanicPolicyError(ValueError):
    pass


ARRAY_RE = re.compile(r'^([A-Za-z0-9_]+)\s*=\s*\[(.*)\]\s*(?:#.*)?$')
STRING_RE = re.compile(r'"([^"\\]*(?:\\.[^"\\]*)*)"')
PACKAGE_RE = re.compile(r'^name\s*=\s*"([A-Za-z0-9_-]+)"\s*(?:#.*)?$', re.MULTILINE)
VERSION_RE = re.compile(r'^version\s*=\s*([0-9]+)\s*(?:#.*)?$', re.MULTILINE)
FORBIDDEN_RE = re.compile(r'\b(?:unreachable|todo|unimplemented)!\s*\(')


def canonical_path(raw: str, field: str) -> str:
    path = PurePosixPath(raw)
    if not raw or path.is_absolute() or path.as_posix() != raw or ".." in path.parts or "." in path.parts or "\\" in raw:
        raise PanicPolicyError(f"{field} is not canonical repository-relative: {raw!r}")
    return raw


def parse_policy(path: Path) -> Dict[str, Any]:
    try:
        source = path.read_text(encoding="utf-8")
    except OSError as exc:
        raise PanicPolicyError(f"cannot read panic policy: {exc}") from exc
    version = VERSION_RE.search(source)
    if version is None or int(version.group(1)) != 1:
        raise PanicPolicyError("panic policy version must be 1")
    arrays: Dict[str, List[str]] = {}
    for line in source.splitlines():
        match = ARRAY_RE.match(line.strip())
        if match is None:
            continue
        key, body = match.groups()
        arrays[key] = [bytes(value, "utf-8").decode("unicode_escape") for value in STRING_RE.findall(body)]
    expected = {"exclude_packages", "lib_exempt_packages", "bin_exempt_targets"}
    if set(arrays) != expected:
        raise PanicPolicyError(f"panic policy array fields mismatch: expected {sorted(expected)}, got {sorted(arrays)}")
    for key, values in arrays.items():
        if values != sorted(set(values)):
            raise PanicPolicyError(f"panic policy {key} must be sorted and unique")
    return {"version": 1, **arrays}


def workspace_members(root: Path) -> List[str]:
    source = (root / "Cargo.toml").read_text(encoding="utf-8")
    match = re.search(r'(?ms)^members\s*=\s*\[(.*?)\]', source)
    if match is None:
        raise PanicPolicyError("root Cargo.toml has no workspace members array")
    members = STRING_RE.findall(match.group(1))
    if not members or members != sorted(set(members)):
        # Root order is semantically meaningful for humans; only uniqueness is required.
        if len(members) != len(set(members)):
            raise PanicPolicyError("workspace member paths are duplicated")
    return [canonical_path(value, "workspace member") for value in members]


def package_inventory(root: Path) -> Dict[str, Dict[str, Any]]:
    packages: Dict[str, Dict[str, Any]] = {}
    for member in workspace_members(root):
        manifest = root / member / "Cargo.toml"
        if not manifest.is_file():
            raise PanicPolicyError(f"workspace member manifest is missing: {member}/Cargo.toml")
        source = manifest.read_text(encoding="utf-8")
        package_section = re.search(r'(?ms)^\[package\]\s*(.*?)(?=^\[|\Z)', source)
        if package_section is None:
            raise PanicPolicyError(f"workspace member has no [package] table: {member}")
        name_match = PACKAGE_RE.search(package_section.group(1))
        if name_match is None:
            raise PanicPolicyError(f"workspace package has no literal name: {member}")
        name = name_match.group(1)
        if name in packages:
            raise PanicPolicyError(f"duplicate workspace package name: {name}")
        root_dir = manifest.parent
        explicit_bins = re.findall(r'(?ms)^\[\[bin\]\]\s*(.*?)(?=^\[|\Z)', source)
        bins: List[str] = []
        for table in explicit_bins:
            bin_name = PACKAGE_RE.search(table)
            if bin_name is None:
                raise PanicPolicyError(f"binary target has no literal name: {member}")
            bins.append(bin_name.group(1))
        if (root_dir / "src/main.rs").is_file() and name not in bins:
            bins.append(name)
        packages[name] = {
            "member": member,
            "hasLib": (root_dir / "src/lib.rs").is_file() or "[lib]" in source,
            "bins": sorted(set(bins)),
        }
    return packages


def production_sources(root: Path, packages: Dict[str, Dict[str, Any]], excluded: Set[str]) -> Iterable[Tuple[str, Path]]:
    for name in sorted(packages):
        if name in excluded:
            continue
        package_root = root / packages[name]["member"]
        for path in sorted((package_root / "src").rglob("*.rs")):
            rel = path.relative_to(root).as_posix()
            if any(part in {"tests", "benches"} for part in path.relative_to(package_root).parts):
                continue
            yield rel, path


def audit(root: Path, policy_path: Path) -> Dict[str, Any]:
    policy = parse_policy(policy_path)
    packages = package_inventory(root)
    package_names = set(packages)
    excluded = set(policy["exclude_packages"])
    lib_exempt = set(policy["lib_exempt_packages"])
    bin_exempt = set(policy["bin_exempt_targets"])
    unknown_packages = sorted((excluded | lib_exempt) - package_names)
    if unknown_packages:
        raise PanicPolicyError("policy references unknown workspace packages: " + ", ".join(unknown_packages))
    all_bins = {target for package in packages.values() for target in package["bins"]}
    unknown_bins = sorted(bin_exempt - all_bins)
    if unknown_bins:
        raise PanicPolicyError("policy references unknown binary targets: " + ", ".join(unknown_bins))

    production = sorted(package_names - excluded)
    covered: Set[str] = set()
    libraries = []
    binaries = []
    for name in production:
        package = packages[name]
        if package["hasLib"] and name not in lib_exempt:
            covered.add(name)
            libraries.append(name)
        for target in package["bins"]:
            if target not in bin_exempt:
                covered.add(name)
                binaries.append({"package": name, "target": target})
    uncovered = sorted(set(production) - covered)
    if uncovered:
        raise PanicPolicyError("production packages lack an assured lib/bin target: " + ", ".join(uncovered))

    forbidden = []
    scanned_files = 0
    for rel, path in production_sources(root, packages, excluded):
        scanned_files += 1
        for number, line in enumerate(path.read_text(encoding="utf-8").splitlines(), 1):
            if FORBIDDEN_RE.search(line):
                forbidden.append(f"{rel}:{number}")
    if forbidden:
        raise PanicPolicyError("forbidden panic macro in production source: " + ", ".join(forbidden))

    policy_rel = policy_path.relative_to(root).as_posix()
    compiler_gate = root / "scripts/check_no_user_panics_compiler.sh"
    renderer = root / "scripts/render_no_user_panics_report.sh"
    ci = (root / ".github/workflows/ci.yml").read_text(encoding="utf-8")
    profile = (root / "scripts/render_upgrade_plan_health_report.sh").read_text(encoding="utf-8")
    if not compiler_gate.is_file() or not renderer.is_file():
        raise PanicPolicyError("compiler-backed panic assurance lane is missing")
    renderer_text = renderer.read_text(encoding="utf-8")
    required_lints = ("clippy::unwrap_used", "clippy::expect_used", "clippy::panic")
    if any(lint not in renderer_text for lint in required_lints) or "cargo clippy" not in renderer_text:
        raise PanicPolicyError("compiler-backed panic assurance lost required Clippy lints")
    if "bash scripts/check_no_user_panics_compiler.sh" not in ci or "bash scripts/check_no_user_panics_compiler.sh" not in profile:
        raise PanicPolicyError("compiler-backed panic assurance is not wired into CI and profile execution")

    identity = sha256()
    for rel in (policy_rel, "scripts/check_no_user_panics_compiler.sh", "scripts/render_no_user_panics_report.sh"):
        data = (root / rel).read_bytes()
        identity.update(len(rel.encode()).to_bytes(8, "big"))
        identity.update(rel.encode())
        identity.update(len(data).to_bytes(8, "big"))
        identity.update(data)
    return {
        "compilerAssuranceIdentitySha256": identity.hexdigest(),
        "excludedPackages": sorted(excluded),
        "kind": "genesis/no-user-panics-static-v0.1",
        "productionBinaryTargets": binaries,
        "productionLibraries": libraries,
        "productionPackages": production,
        "scannedSourceFiles": scanned_files,
        "version": "0.1",
    }


def main(argv: Optional[Sequence[str]] = None) -> int:
    parser = argparse.ArgumentParser()
    parser.add_argument("--root", type=Path, default=Path.cwd())
    parser.add_argument("--policy", default="policies/panic_guard.toml")
    parser.add_argument("--out", type=Path)
    args = parser.parse_args(argv)
    root = args.root.resolve()
    try:
        report = audit(root, root / canonical_path(args.policy, "policy"))
        rendered = json.dumps(report, indent=2, sort_keys=True) + "\n"
        if args.out:
            args.out.parent.mkdir(parents=True, exist_ok=True)
            args.out.write_text(rendered, encoding="utf-8")
        print(
            "panic-policy: ok "
            f"(packages={len(report['productionPackages'])} files={report['scannedSourceFiles']} "
            f"compiler_assurance={report['compilerAssuranceIdentitySha256']})"
        )
        return 0
    except (OSError, UnicodeError, PanicPolicyError) as exc:
        print(f"panic-policy: {exc}", file=sys.stderr)
        return 1


if __name__ == "__main__":
    raise SystemExit(main())
