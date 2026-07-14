#!/usr/bin/env python3
"""Compute conservative crate and gate impact for repository changes."""

from __future__ import annotations

import argparse
from collections import defaultdict, deque
from hashlib import sha256
import fnmatch
import json
from pathlib import Path, PurePosixPath
import subprocess
import sys
from typing import Any, Sequence

POLICY_REL = "policies/changed_impact_v0.1.json"
GATES_REL = "genesis.gates.json"
POLICY_FIELDS = {"kind", "version", "fallbackProfile", "profileRanks", "fullFastPatterns", "generatedClasses", "maxTargetedCrates", "maxTargetedGates"}


class ImpactError(ValueError):
    pass


def unique_pairs(pairs):
    out = {}
    for key, value in pairs:
        if key in out:
            raise ImpactError(f"duplicate JSON key: {key}")
        out[key] = value
    return out


def load_json(path: Path) -> Any:
    try:
        return json.loads(path.read_text(encoding="utf-8"), object_pairs_hook=unique_pairs)
    except (OSError, json.JSONDecodeError) as exc:
        raise ImpactError(f"cannot load {path}: {exc}") from exc


def digest(path: Path) -> str:
    return sha256(path.read_bytes()).hexdigest()


def canonical_path(raw: str) -> str:
    if not raw or raw != raw.strip() or "\\" in raw or raw.startswith("./"):
        raise ImpactError(f"noncanonical changed path: {raw!r}")
    path = PurePosixPath(raw)
    if path.is_absolute() or ".." in path.parts or "." in path.parts:
        raise ImpactError(f"changed path escapes repository: {raw!r}")
    return path.as_posix()


def matches(path: str, pattern: str) -> bool:
    return fnmatch.fnmatchcase(path, pattern) or (pattern.endswith("/**") and path == pattern[:-3])


def load_policy(root: Path) -> dict:
    policy = load_json(root / POLICY_REL)
    if not isinstance(policy, dict) or set(policy) != POLICY_FIELDS:
        raise ImpactError("changed-impact policy fields mismatch")
    if policy["kind"] != "genesis/changed-impact-policy-v0.1" or policy["version"] != "0.1":
        raise ImpactError("changed-impact policy identity mismatch")
    if policy["fallbackProfile"] != "prepush-standard":
        raise ImpactError("fallback profile must remain prepush-standard")
    expected_ranks = {"local-fast": 0, "agent-inner-loop": 1, "prepush-standard": 2, "release-full": 3}
    if policy["profileRanks"] != expected_ranks:
        raise ImpactError("profile rank lattice drift")
    for field in ("fullFastPatterns", "generatedClasses"):
        if not isinstance(policy[field], list) or not policy[field]:
            raise ImpactError(f"{field} must be non-empty")
    if policy["fullFastPatterns"] != sorted(set(policy["fullFastPatterns"])):
        raise ImpactError("fullFastPatterns must be sorted and unique")
    ids = []
    for item in policy["generatedClasses"]:
        if not isinstance(item, dict) or set(item) != {"id", "patterns", "profile", "requireGateMatch"}:
            raise ImpactError("generated class fields mismatch")
        if item["profile"] not in expected_ranks or item["requireGateMatch"] is not True:
            raise ImpactError("generated class fallback contract drift")
        if item["patterns"] != sorted(set(item["patterns"])) or not item["patterns"]:
            raise ImpactError("generated class patterns must be sorted and unique")
        ids.append(item["id"])
    if ids != sorted(set(ids)):
        raise ImpactError("generated class ids must be sorted and unique")
    for field in ("maxTargetedCrates", "maxTargetedGates"):
        if not isinstance(policy[field], int) or isinstance(policy[field], bool) or policy[field] < 1:
            raise ImpactError(f"{field} must be positive")
    return policy


def cargo_metadata(root: Path, metadata_path: Path | None) -> dict:
    if metadata_path:
        return load_json(metadata_path)
    proc = subprocess.run(["cargo", "metadata", "--locked", "--offline", "--no-deps", "--format-version", "1"], cwd=root, text=True, stdout=subprocess.PIPE, stderr=subprocess.PIPE)
    if proc.returncode:
        raise ImpactError(f"cargo metadata failed: {proc.stderr.strip()}")
    return json.loads(proc.stdout, object_pairs_hook=unique_pairs)


def crate_graph(root: Path, metadata: dict):
    workspace_ids = set(metadata.get("workspace_members", []))
    packages = {p["id"]: p for p in metadata.get("packages", []) if p.get("id") in workspace_ids}
    by_manifest = {}
    by_name = {}
    for package in packages.values():
        manifest = Path(package["manifest_path"]).resolve()
        try:
            rel = manifest.relative_to(root).as_posix()
        except ValueError as exc:
            raise ImpactError("workspace manifest escaped repository") from exc
        name = package["name"]
        if name in by_name:
            raise ImpactError(f"duplicate workspace package name: {name}")
        by_name[name] = package
        by_manifest[rel] = name
    names = set(by_name)
    reverse = defaultdict(set)
    for name, package in by_name.items():
        for dep in package.get("dependencies", []):
            dep_name = dep.get("name")
            if dep.get("path") and dep_name in names:
                reverse[dep_name].add(name)
    return by_name, reverse


def reverse_closure(seeds, reverse):
    seen = set(seeds)
    queue = deque(sorted(seeds))
    while queue:
        current = queue.popleft()
        for dependent in sorted(reverse.get(current, ())):
            if dependent not in seen:
                seen.add(dependent)
                queue.append(dependent)
    return sorted(seen)


def resolve(root: Path, changed: Sequence[str], runner: str, metadata_path: Path | None = None) -> dict:
    root = root.resolve()
    policy = load_policy(root)
    gates_doc = load_json(root / GATES_REL)
    files = sorted(set(canonical_path(path) for path in changed))
    metadata = cargo_metadata(root, metadata_path)
    packages, reverse = crate_graph(root, metadata)
    direct_crates = sorted({parts[1] for path in files if len((parts := path.split("/"))) >= 3 and parts[0] == "crates" and parts[1] in packages})
    affected_crates = reverse_closure(direct_crates, reverse)

    input_sets = {item["id"]: item["globs"] for item in gates_doc.get("inputSets", [])}
    direct_gates, affected_gates = set(), set()
    gates = {gate["entrypoint"]: gate for gate in gates_doc.get("gates", [])}
    for entrypoint, gate in gates.items():
        exact = set(gate.get("inputs", {}).get("paths", []))
        if any(path in exact for path in files):
            direct_gates.add(entrypoint)
        if any(path in exact or any(matches(path, pattern) for set_id in gate.get("inputs", {}).get("sets", []) for pattern in input_sets.get(set_id, [])) for path in files):
            affected_gates.add(entrypoint)
    reverse_gates = defaultdict(set)
    for entrypoint, gate in gates.items():
        for dependency in gate.get("dependencies", []):
            reverse_gates[dependency].add(entrypoint)
    affected_gates = set(reverse_closure(affected_gates, reverse_gates))

    reasons = []
    generated = []
    full_fast = any(matches(path, pattern) for path in files for pattern in policy["fullFastPatterns"])
    if full_fast:
        reasons.append("workspace-configuration-change")
    for item in policy["generatedClasses"]:
        matched = sorted(path for path in files if any(matches(path, pattern) for pattern in item["patterns"]))
        if matched:
            generated.append({"id": item["id"], "paths": matched, "profile": item["profile"]})
            if item["requireGateMatch"] and not any(path in set().union(*(set(g.get("inputs", {}).get("paths", [])) for g in gates.values())) for path in matched):
                reasons.append(f"generated-without-gate:{item['id']}")
            full_fast = True
    known = set(direct_crates)
    unknown = [path for path in files if not (path.startswith("crates/") or path.startswith(("docs/", "scripts/", "policies/", "prelude/", "selfhost/", "tests/", "examples/", "tools/", ".github/")) or path in {"Cargo.toml", "Cargo.lock", "package.json", "package-lock.json", "rust-toolchain.toml", "README.md", "ROADMAP.md", "CHANGELOG.md"})]
    if unknown:
        reasons.append("unclassified-path")
        full_fast = True
    if files and not direct_crates and not direct_gates and not full_fast:
        reasons.append("no-exact-impact-authority")
        full_fast = True
    if len(affected_crates) > policy["maxTargetedCrates"] or len(direct_gates) > policy["maxTargetedGates"]:
        reasons.append("targeted-cardinality-limit")
        full_fast = True

    mode = "clean-tree" if not files else ("profile-fallback" if full_fast else "targeted")
    commands = []
    if mode == "clean-tree":
        commands = ["cargo test -p gc_coreform -p gc_kernel --lib --quiet"]
    elif mode == "profile-fallback":
        commands = ["bash scripts/test_fast_full.sh"]
    else:
        local_gates = sorted(g for g in direct_gates if gates[g]["profile"] == "local-fast" and gates[g]["kind"] == "static")
        commands.extend(f"bash {gate}" for gate in local_gates)
        for crate in affected_crates:
            commands.append(("cargo nextest run" if runner == "nextest" else "cargo test") + f" -p {crate}" + (" --profile ci" if runner == "nextest" else ""))
    return {
        "affectedCrates": affected_crates, "affectedGates": sorted(affected_gates), "changedFiles": files,
        "commands": commands, "directCrates": direct_crates, "directGates": sorted(direct_gates),
        "fallbackProfile": policy["fallbackProfile"] if full_fast else None, "fallbackReasons": sorted(set(reasons)),
        "gateManifestSha256": digest(root / GATES_REL), "generatedImpacts": generated,
        "kind": "genesis/changed-impact-plan-v0.1", "mode": mode,
        "policySha256": digest(root / POLICY_REL), "runner": runner, "unknownPaths": unknown, "version": "0.1"
    }


def git_changed_files(root: Path, base: str) -> list[str]:
    commands = [
        ["git", "diff", "--no-renames", "--name-only", "-z", f"{base}...HEAD"],
        ["git", "diff", "--no-renames", "--name-only", "-z"],
        ["git", "diff", "--cached", "--no-renames", "--name-only", "-z"],
        ["git", "ls-files", "--others", "--exclude-standard", "-z"],
    ]
    changed = set()
    for command in commands:
        proc = subprocess.run(command, cwd=root, stdout=subprocess.PIPE, stderr=subprocess.PIPE)
        if proc.returncode:
            raise ImpactError(f"{' '.join(command)} failed: {proc.stderr.decode(errors='replace').strip()}")
        for raw in proc.stdout.split(b"\0"):
            if raw:
                changed.add(raw.decode("utf-8", errors="strict"))
    return sorted(changed)


def main(argv=None):
    parser = argparse.ArgumentParser()
    parser.add_argument("--root", type=Path, required=True)
    source = parser.add_mutually_exclusive_group(required=True)
    source.add_argument("--changed-files", type=Path)
    source.add_argument("--git-base")
    parser.add_argument("--runner", choices=("cargo", "nextest"), default="cargo")
    parser.add_argument("--metadata", type=Path)
    parser.add_argument("--out", type=Path)
    args = parser.parse_args(argv)
    try:
        changed = (
            args.changed_files.read_text(encoding="utf-8").splitlines()
            if args.changed_files
            else git_changed_files(args.root.resolve(), args.git_base)
        )
        plan = resolve(args.root, changed, args.runner, args.metadata)
        rendered = json.dumps(plan, indent=2, sort_keys=True) + "\n"
        if args.out:
            args.out.parent.mkdir(parents=True, exist_ok=True)
            args.out.write_text(rendered, encoding="utf-8")
        else:
            print(rendered, end="")
    except (ImpactError, OSError, json.JSONDecodeError) as exc:
        print(f"changed-impact: {exc}", file=sys.stderr)
        return 2
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
