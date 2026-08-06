#!/usr/bin/env python3
"""Validate and query the closed release-full evidence execution partition."""

from __future__ import annotations

import argparse
import copy
import hashlib
import json
from pathlib import Path
import re
import sys
from typing import Any, Mapping, Optional, Sequence


KIND = "genesis/release-evidence-dag-v0.2"
VERSION = "0.2.0"
POLICY = Path("policies/release_evidence_dag_v0.2.json")
SCHEMA = Path("docs/spec/RELEASE_EVIDENCE_DAG_v0.2.schema.json")
SCHEMA_ID = "https://genesiscode.dev/schemas/release-evidence-dag-v0.2.schema.json"
CLASSES = ("cache-sensitive", "invariant", "stress-performance")
GROUPS = ("common", "profile", "setup")


class DagError(ValueError):
    pass


def fail(message: str) -> None:
    raise DagError(message)


def canonical(value: Any) -> bytes:
    return (json.dumps(value, sort_keys=True, separators=(",", ":")) + "\n").encode()


def sha256(value: Any) -> str:
    return hashlib.sha256(canonical(value)).hexdigest()


def exact_keys(value: Any, expected: set[str], label: str) -> Mapping[str, Any]:
    if not isinstance(value, dict) or set(value) != expected:
        observed = sorted(value) if isinstance(value, dict) else type(value).__name__
        fail(f"{label} fields mismatch: expected={sorted(expected)!r} observed={observed!r}")
    return value


def load_policy(root: Path, path: Optional[Path] = None) -> dict[str, Any]:
    source = path or root / POLICY
    try:
        value = json.loads(source.read_text(encoding="utf-8"))
    except (OSError, UnicodeError, json.JSONDecodeError) as exc:
        fail(f"cannot read release evidence DAG policy: {exc}")
    if not isinstance(value, dict):
        fail("release evidence DAG policy root must be an object")
    return value


def validate_schema(root: Path) -> None:
    try:
        schema = json.loads((root / SCHEMA).read_text(encoding="utf-8"))
    except (OSError, UnicodeError, json.JSONDecodeError) as exc:
        fail(f"cannot read release evidence DAG schema: {exc}")
    if not isinstance(schema, dict) or schema.get("$id") != SCHEMA_ID:
        fail("release evidence DAG schema identity mismatch")
    if schema.get("$schema") != "https://json-schema.org/draft/2020-12/schema":
        fail("release evidence DAG schema dialect mismatch")


def extract_health_commands(source: str) -> list[tuple[str, str]]:
    try:
        common = source.split("COMMON_GATES=(\n", 1)[1].split(
            '\n)\n\nif [[ "$PROFILE"', 1
        )[0]
        release = source.rsplit("  release-full)\n", 1)[1].split(
            "  full-selfhost-cutover)\n", 1
        )[0]
    except IndexError as exc:
        fail("release health command topology is not parseable")
    rows = [("common", command) for command in re.findall(r'^  "(.+)"$', common, re.M)]
    gate_pattern = re.compile(r'PROFILE_(SETUP_)?GATES\+=\(\s*"(.*?)"\s*\)', re.S)
    rows.extend(
        ("setup" if setup else "profile", command)
        for setup, command in gate_pattern.findall(release)
    )
    gpu_match = re.search(
        r'PROFILE_GATES\+=\(\s*"(bash scripts/render_gpu_compute_device_conformance_report\.sh.*?)"\s*\)',
        source,
        re.S,
    )
    if gpu_match is None:
        fail("release GPU-device command is not parseable")
    rows.append(("profile", gpu_match.group(1)))
    return rows


def validate(policy: Mapping[str, Any], health_source: str) -> dict[str, Any]:
    exact_keys(
        policy,
        {
            "$schema", "aggregate", "budgets", "commands", "executionClasses",
            "fanout", "kind", "profile", "version", "warmPrecondition", "watchdog",
        },
        "release evidence DAG",
    )
    if policy["kind"] != KIND or policy["version"] != VERSION or policy["profile"] != "release-full":
        fail("release evidence DAG identity mismatch")
    if policy["$schema"] != "../docs/spec/RELEASE_EVIDENCE_DAG_v0.2.schema.json":
        fail("release evidence DAG schema binding mismatch")
    if policy["budgets"] != {
        "aggregateWallMs": 300_000,
        "artifactBytesPerWorker": 20 * 1024 * 1024 * 1024,
        "diagnosticTailBytes": 4096,
        "diagnosticTailLines": 40,
        "measuredWorkerWallMs": 2_700_000,
        "workflowWallMs": 3_600_000,
    }:
        fail("release evidence DAG budget contract mismatch")

    classes = policy["executionClasses"]
    if not isinstance(classes, list) or [row.get("id") for row in classes] != list(CLASSES):
        fail("release evidence execution classes must be complete and ordered")
    expected_classes = {
        "cache-sensitive": (["cold", "warm"], 3, "independent-matched-cohorts"),
        "invariant": (["not-measured"], 1, "exactly-once"),
        "stress-performance": (["not-measured"], 3, "independent-odd-cohort"),
    }
    for row in classes:
        exact_keys(
            row,
            {
                "cacheStates", "dependencies", "id", "isolation",
                "minimumSamplesPerState", "oddSamplesRequired", "repeatPolicy",
            },
            f"execution class {row.get('id')}",
        )
        states, samples, repeat = expected_classes[row["id"]]
        if (
            row["cacheStates"] != states
            or row["minimumSamplesPerState"] != samples
            or row["oddSamplesRequired"] is not True
            or row["repeatPolicy"] != repeat
        ):
            fail(f"execution class contract mismatch: {row['id']}")
        if samples % 2 != 1:
            fail(f"execution class sample count is not odd: {row['id']}")

    warm = exact_keys(
        policy["warmPrecondition"],
        {
            "artifactInventoryMustMatch", "cacheKeyMustMatch", "featureSetMustMatch",
            "measured", "network", "ownedEmptyRootRequired", "sourceMustMatch",
            "toolchainMustMatch",
        },
        "warm precondition",
    )
    if (
        warm["measured"] is not False
        or warm["network"] != "deny"
        or any(warm[key] is not True for key in warm if key not in {"measured", "network"})
    ):
        fail("warm precondition is not deterministic and fail closed")

    commands = policy["commands"]
    if not isinstance(commands, list) or not commands:
        fail("release evidence DAG has no commands")
    by_id: dict[str, Mapping[str, Any]] = {}
    selectors: set[tuple[str, str]] = set()
    for index, raw in enumerate(commands):
        expected = {"dependencies", "evidenceClass", "group", "id", "selector"}
        if isinstance(raw, dict) and raw.get("evidenceClass") == "superseded":
            expected.add("replacement")
        if isinstance(raw, dict) and "condition" in raw:
            expected.add("condition")
        row = exact_keys(raw, expected, f"command {index}")
        command_id = row["id"]
        group = row["group"]
        evidence_class = row["evidenceClass"]
        selector = row["selector"]
        if not isinstance(command_id, str) or not command_id or command_id in by_id:
            fail(f"release command id is invalid or duplicated: {command_id!r}")
        if group not in GROUPS or evidence_class not in (*CLASSES, "superseded"):
            fail(f"release command class or group is invalid: {command_id}")
        if "condition" in row and row["condition"] != "agent-gpu-strict":
            fail(f"release command condition is invalid: {command_id}")
        if not isinstance(selector, str) or len(selector) < 4 or (group, selector) in selectors:
            fail(f"release command selector is invalid or duplicated: {command_id}")
        if not isinstance(row["dependencies"], list) or len(set(row["dependencies"])) != len(row["dependencies"]):
            fail(f"release command dependencies are invalid: {command_id}")
        by_id[command_id] = row
        selectors.add((group, selector))
    for command_id, row in by_id.items():
        for dependency in row["dependencies"]:
            if dependency == command_id or dependency not in by_id:
                fail(f"release command dependency is invalid: {command_id} -> {dependency}")
        if row["evidenceClass"] == "superseded":
            replacement = row["replacement"]
            if replacement not in by_id or by_id[replacement]["evidenceClass"] == "superseded":
                fail(f"superseded release command lacks a live replacement: {command_id}")

    observed = extract_health_commands(health_source)
    matched: set[str] = set()
    for group, command in observed:
        candidates = [
            row for row in commands
            if row["group"] == group and row["selector"] in command
        ]
        if len(candidates) != 1:
            fail(
                "release health command must match exactly one DAG selector: "
                f"group={group} matches={[row['id'] for row in candidates]!r} command={command!r}"
            )
        matched.add(candidates[0]["id"])
    missing = sorted(set(by_id) - matched)
    if missing:
        fail(f"release evidence DAG declares commands absent from the health runner: {missing!r}")

    fanout = exact_keys(
        policy["fanout"],
        {"consumers", "crossRunReuseAllowed", "producer", "sameRunOnly", "sourceSample"},
        "release evidence fanout",
    )
    if (
        fanout["producer"] != "setup/evidence-bundle"
        or fanout["sourceSample"] != {"class": "cold", "index": 1}
        or fanout["sameRunOnly"] is not True
        or fanout["crossRunReuseAllowed"] is not False
        or fanout["consumers"] != ["invariant", "stress-performance"]
    ):
        fail("release evidence fanout authority mismatch")
    if by_id[fanout["producer"]]["evidenceClass"] != "cache-sensitive":
        fail("release evidence fanout producer is not cache-sensitive")

    aggregate = exact_keys(
        policy["aggregate"], {"readOnly", "reject", "requiredTargetDispositions"},
        "release aggregate",
    )
    required_rejections = {
        "cross-class-reuse", "duplicate-execution", "forged-cache-state",
        "identity-drift", "incomplete-cleanup", "missing-node",
        "overlapping-exclusive-lanes", "producer-authored-verdict", "stale-evidence",
        "undeclared-command",
    }
    if (
        aggregate["readOnly"] is not True
        or set(aggregate["reject"]) != required_rejections
        or aggregate["requiredTargetDispositions"]
        != ["android", "edge", "ios", "service-runtime"]
    ):
        fail("release aggregate fail-closed contract mismatch")
    watchdog = exact_keys(
        policy["watchdog"],
        {
            "authority", "freshnessSeconds", "fullRunDeadlineSeconds",
            "latestMainStandardDeadlineSeconds", "selfReportingAllowed",
        },
        "release watchdog",
    )
    if watchdog != {
        "authority": "independent",
        "freshnessSeconds": 172_800,
        "fullRunDeadlineSeconds": 3_600,
        "latestMainStandardDeadlineSeconds": 7_200,
        "selfReportingAllowed": False,
    }:
        fail("release watchdog authority mismatch")
    return {
        "commands": len(commands),
        "activeCommands": sum(row["evidenceClass"] != "superseded" for row in commands),
        "supersededCommands": sum(row["evidenceClass"] == "superseded" for row in commands),
        "identitySha256": sha256(policy),
    }


def classify(policy: Mapping[str, Any], group: str, command: str) -> Mapping[str, Any]:
    candidates = [
        row for row in policy["commands"]
        if row["group"] == group and row["selector"] in command
    ]
    if len(candidates) != 1:
        fail(f"command classification is not unique: group={group} matches={len(candidates)}")
    return candidates[0]


def self_test(policy: dict[str, Any], health_source: str) -> int:
    controls = 0
    superseded_index = next(
        index
        for index, row in enumerate(policy["commands"])
        if row["evidenceClass"] == "superseded"
    )
    mutations = [
        lambda doc: doc["budgets"].__setitem__("measuredWorkerWallMs", 2_700_001),
        lambda doc: doc["executionClasses"][0].__setitem__("minimumSamplesPerState", 2),
        lambda doc: doc["warmPrecondition"].__setitem__("measured", True),
        lambda doc: doc["fanout"].__setitem__("crossRunReuseAllowed", True),
        lambda doc: doc["aggregate"].__setitem__("readOnly", False),
        lambda doc: doc["watchdog"].__setitem__("selfReportingAllowed", True),
        lambda doc: doc["commands"].pop(),
        lambda doc: doc["commands"][0].__setitem__("selector", doc["commands"][1]["selector"]),
        lambda doc: doc["commands"][0].__setitem__("dependencies", [doc["commands"][0]["id"]]),
        lambda doc: doc["commands"][superseded_index].__setitem__("replacement", "missing/node"),
    ]
    for mutate in mutations:
        candidate = copy.deepcopy(policy)
        mutate(candidate)
        try:
            validate(candidate, health_source)
        except DagError:
            controls += 1
        else:
            fail(f"release evidence DAG self-test accepted mutation {controls + 1}")
    return controls


def main(argv: Optional[Sequence[str]] = None) -> int:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--root", type=Path, default=Path.cwd())
    parser.add_argument("--policy", type=Path)
    sub = parser.add_subparsers(dest="action", required=True)
    sub.add_parser("check")
    sub.add_parser("self-test")
    classify_parser = sub.add_parser("classify")
    classify_parser.add_argument("--group", choices=GROUPS, required=True)
    classify_parser.add_argument("--command", dest="command_text", required=True)
    classify_parser.add_argument("--format", choices=("json", "tsv"), default="json")
    select_parser = sub.add_parser("select")
    select_parser.add_argument("--group", choices=GROUPS, required=True)
    select_parser.add_argument("--evidence-class", choices=CLASSES, required=True)
    select_parser.add_argument("--command", dest="command_texts", action="append", default=[])
    args = parser.parse_args(argv)
    root = args.root.resolve(strict=True)
    try:
        validate_schema(root)
        policy = load_policy(root, args.policy)
        health_source = (root / "scripts/render_upgrade_plan_health_report.sh").read_text(
            encoding="utf-8"
        )
        result = validate(policy, health_source)
        if args.action == "classify":
            row = classify(policy, args.group, args.command_text)
            if args.format == "tsv":
                print(f"{row['evidenceClass']}\t{row['id']}")
            else:
                print(json.dumps(row, sort_keys=True))
        elif args.action == "select":
            for index, command_text in enumerate(args.command_texts):
                row = classify(policy, args.group, command_text)
                if row["evidenceClass"] == args.evidence_class:
                    print(f"{index}\t{row['id']}")
        elif args.action == "self-test":
            controls = self_test(policy, health_source)
            print(f"release-evidence-dag: self-test ok (negative_controls={controls})")
        else:
            print(
                "release-evidence-dag: ok "
                f"(commands={result['commands']} active={result['activeCommands']} "
                f"superseded={result['supersededCommands']} identity={result['identitySha256']})"
            )
    except (DagError, OSError, UnicodeError, json.JSONDecodeError) as exc:
        print(f"release-evidence-dag: {exc}", file=sys.stderr)
        return 1
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
