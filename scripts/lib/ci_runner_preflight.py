#!/usr/bin/env python3
"""Classify exact self-hosted runner readiness before workflow dispatch."""

from __future__ import annotations

import argparse
import copy
import json
import tempfile
from pathlib import Path
from typing import Any


KIND = "genesis/ci-runner-preflight-v0.1"
POLICY_KIND = "genesis/ci-control-plane-policy-v0.1"
STATUSES = {"ready", "unsupported-profile", "infrastructure-failure"}
POLICY_KEYS = {
    "kind",
    "version",
    "repository",
    "branch",
    "workflows",
    "limitsSeconds",
    "historicalIncident",
    "selectionProfiles",
    "runnerLanes",
}


class PreflightError(ValueError):
    pass


def require(condition: bool, message: str) -> None:
    if not condition:
        raise PreflightError(message)


def read_json(path: Path) -> Any:
    try:
        return json.loads(path.read_text(encoding="utf-8"))
    except (OSError, json.JSONDecodeError) as exc:
        raise PreflightError(f"cannot read JSON {path.name}: {exc}") from exc


def validate_policy(policy: Any) -> tuple[dict[str, list[str]], list[dict[str, Any]]]:
    require(isinstance(policy, dict), "policy must be an object")
    require(set(policy) == POLICY_KEYS, "policy fields are not closed")
    require(policy["kind"] == POLICY_KIND and policy["version"] == "0.1", "policy identity mismatch")
    require(isinstance(policy["repository"], str) and policy["repository"], "repository is required")
    require(policy["branch"] == "main", "canonical branch must be main")
    require(
        policy["workflows"] == {
            "ci": ".github/workflows/ci.yml",
            "watchdog": ".github/workflows/ci-watchdog.yml",
        },
        "workflow authority mismatch",
    )
    limits = policy["limitsSeconds"]
    require(
        limits
        == {
            "latestMainDisposition": 7200,
            "fullRunTermination": 3600,
            "successfulFullFreshness": 172800,
            "scheduledFullCadence": 93600,
            "runnerPreflight": 300,
        },
        "control-plane limits mismatch",
    )
    require(
        policy["historicalIncident"]
        == {
            "path": "docs/program/incidents/CI_LIVENESS_2026-07-18_2026-08-04.json",
            "incidentId": "ci-liveness-2026-07-18_2026-08-04",
            "lastSuccessfulRunId": 29664738972,
            "firstAffectedRunId": 29682310988,
            "lastAffectedRunId": 30939372612,
            "failedCount": 15,
            "cancelledCount": 16,
            "recordsSha256": "8651a34f4c44d6859eebaa75166a315751a0a1a66602bd1bb91c31cf2f463d0e",
        },
        "historical incident authority mismatch",
    )
    profiles = policy["selectionProfiles"]
    require(isinstance(profiles, dict) and set(profiles) == {"none", "primary", "matrix"}, "selection profiles mismatch")
    lanes = policy["runnerLanes"]
    require(isinstance(lanes, list) and lanes, "runner lanes must be non-empty")
    lane_ids: list[str] = []
    for lane in lanes:
        require(isinstance(lane, dict) and set(lane) == {"id", "requiredLabels"}, "runner lane fields are not closed")
        lane_id = lane["id"]
        labels = lane["requiredLabels"]
        require(isinstance(lane_id, str) and lane_id and lane_id not in lane_ids, "runner lane id is invalid")
        require(isinstance(labels, list) and len(labels) >= 4, f"{lane_id}: labels are incomplete")
        require(all(isinstance(label, str) and label for label in labels), f"{lane_id}: label is invalid")
        require(len(labels) == len(set(labels)) and "self-hosted" in labels, f"{lane_id}: labels are not exact")
        lane_ids.append(lane_id)
    require(profiles["none"] == [], "none profile must request no runner")
    require(profiles["primary"] == ["primary-linux"], "primary profile mismatch")
    require(profiles["matrix"] == lane_ids, "matrix profile must bind every lane in policy order")
    return profiles, lanes


def validate_inventory(inventory: Any) -> list[dict[str, Any]]:
    require(isinstance(inventory, dict), "runner inventory must be an object")
    require(isinstance(inventory.get("total_count"), int), "runner inventory total_count is invalid")
    runners = inventory.get("runners")
    require(isinstance(runners, list), "runner inventory runners must be an array")
    require(inventory["total_count"] >= len(runners), "runner inventory count is inconsistent")
    normalized: list[dict[str, Any]] = []
    for runner in runners:
        require(isinstance(runner, dict), "runner row must be an object")
        require(isinstance(runner.get("id"), int), "runner id is invalid")
        require(runner.get("status") in {"online", "offline"}, "runner status is invalid")
        require(isinstance(runner.get("busy"), bool), "runner busy state is invalid")
        labels = runner.get("labels")
        require(isinstance(labels, list), "runner labels must be an array")
        names: list[str] = []
        for label in labels:
            require(isinstance(label, dict) and isinstance(label.get("name"), str), "runner label is invalid")
            names.append(label["name"])
        require(len(names) == len(set(names)), "runner labels contain duplicates")
        normalized.append({"id": runner["id"], "status": runner["status"], "busy": runner["busy"], "labels": set(names)})
    return normalized


def classify(
    policy: Any,
    inventory: Any,
    selection: str,
    inventory_status: str,
    repository: str,
    revision: str,
    run_id: str,
    run_attempt: str,
) -> dict[str, Any]:
    profiles, lanes = validate_policy(policy)
    require(selection in profiles, f"unknown runner selection profile: {selection}")
    require(inventory_status in {"ok", "not-requested", "api-unavailable"}, "inventory status is invalid")
    requested = set(profiles[selection])
    require(repository == policy["repository"], "repository identity mismatch")
    require(len(revision) == 40 and all(ch in "0123456789abcdef" for ch in revision), "revision must be lowercase SHA-1")
    require(run_id.isdigit() and int(run_id) > 0, "workflow run id is invalid")
    require(run_attempt.isdigit() and int(run_attempt) > 0, "workflow run attempt is invalid")
    runners: list[dict[str, Any]] = []
    if requested and inventory_status == "ok":
        runners = validate_inventory(inventory)
    elif inventory_status == "ok":
        validate_inventory(inventory)

    lane_reports: list[dict[str, Any]] = []
    for lane in lanes:
        lane_id = lane["id"]
        required_labels = lane["requiredLabels"]
        is_requested = lane_id in requested
        matching = [runner for runner in runners if set(required_labels).issubset(runner["labels"])]
        online = [runner for runner in matching if runner["status"] == "online"]
        if not is_requested:
            status = "unsupported-profile"
            reason = "runner-pack-not-requested"
        elif inventory_status != "ok":
            status = "infrastructure-failure"
            reason = "runner-inventory-unavailable"
        elif not online:
            status = "infrastructure-failure"
            reason = "no-online-exact-label-match"
        else:
            status = "ready"
            reason = "online-exact-label-match"
        lane_reports.append(
            {
                "id": lane_id,
                "requested": is_requested,
                "requiredLabels": required_labels,
                "status": status,
                "reason": reason,
                "matchingCount": len(matching),
                "onlineMatchingCount": len(online),
                "busyOnlineMatchingCount": sum(1 for runner in online if runner["busy"]),
                "dispatch": status == "ready",
                "releaseQualified": False,
            }
        )
    if any(row["status"] == "infrastructure-failure" for row in lane_reports):
        overall = "infrastructure-failure"
    elif requested:
        overall = "ready"
    else:
        overall = "unsupported-profile"
    require(overall in STATUSES, "internal status error")
    return {
        "kind": KIND,
        "version": "0.1",
        "status": overall,
        "selectionProfile": selection,
        "inventoryStatus": inventory_status,
        "source": {
            "repository": repository,
            "revision": revision,
            "workflowRunId": int(run_id),
            "workflowRunAttempt": int(run_attempt),
        },
        "lanes": lane_reports,
        "releaseQualified": False,
    }


def write_github_output(path: Path, report: dict[str, Any]) -> None:
    lines = [f"overall_status={report['status']}"]
    for lane in report["lanes"]:
        key = lane["id"].replace("-", "_")
        lines.append(f"{key}_status={lane['status']}")
        lines.append(f"{key}_dispatch={'true' if lane['dispatch'] else 'false'}")
    with path.open("a", encoding="utf-8") as handle:
        handle.write("\n".join(lines) + "\n")


def verify_report(report: Any) -> None:
    require(isinstance(report, dict) and report.get("kind") == KIND, "preflight report identity mismatch")
    require(report.get("status") in STATUSES, "preflight report status is invalid")
    require(report.get("releaseQualified") is False, "preflight cannot qualify a release")
    lanes = report.get("lanes")
    require(isinstance(lanes, list) and lanes, "preflight lanes are missing")
    for lane in lanes:
        require(lane.get("status") in STATUSES, "lane status is invalid")
        require(lane.get("dispatch") is (lane["status"] == "ready"), "lane dispatch/status mismatch")
        require(lane.get("releaseQualified") is False, "runner readiness cannot qualify a release")
    require(report["status"] != "infrastructure-failure", "requested runner profile is unavailable")


def fixture_runner(runner_id: int, labels: list[str], status: str = "online", busy: bool = False) -> dict[str, Any]:
    return {"id": runner_id, "name": f"fixture-{runner_id}", "status": status, "busy": busy, "labels": [{"name": label} for label in labels]}


def self_test(policy_path: Path) -> None:
    policy = read_json(policy_path)
    _, lanes = validate_policy(policy)
    empty = {"total_count": 0, "runners": []}
    base = dict(repository=policy["repository"], revision="a" * 40, run_id="7", run_attempt="1")
    none = classify(policy, empty, "none", "not-requested", **base)
    require(none["status"] == "unsupported-profile" and not any(row["dispatch"] for row in none["lanes"]), "none control failed")
    absent = classify(policy, empty, "primary", "ok", **base)
    require(absent["status"] == "infrastructure-failure", "absent-runner control failed")
    partial_labels = lanes[0]["requiredLabels"][:-1]
    partial = {"total_count": 1, "runners": [fixture_runner(1, partial_labels)]}
    require(classify(policy, partial, "primary", "ok", **base)["status"] == "infrastructure-failure", "partial-label control failed")
    offline = {"total_count": 1, "runners": [fixture_runner(1, lanes[0]["requiredLabels"], status="offline")]}
    require(classify(policy, offline, "primary", "ok", **base)["status"] == "infrastructure-failure", "offline-runner control failed")
    busy = {"total_count": 1, "runners": [fixture_runner(1, lanes[0]["requiredLabels"], busy=True)]}
    require(classify(policy, busy, "primary", "ok", **base)["status"] == "ready", "busy-online control failed")
    all_runners = {"total_count": len(lanes), "runners": [fixture_runner(index + 1, lane["requiredLabels"]) for index, lane in enumerate(lanes)]}
    matrix = classify(policy, all_runners, "matrix", "ok", **base)
    require(matrix["status"] == "ready" and all(row["dispatch"] for row in matrix["lanes"]), "matrix-ready control failed")
    unavailable = classify(policy, empty, "matrix", "api-unavailable", **base)
    require(unavailable["status"] == "infrastructure-failure", "inventory-unavailable control failed")
    bad_policy = copy.deepcopy(policy)
    bad_policy["unknown"] = True
    try:
        classify(bad_policy, empty, "none", "not-requested", **base)
    except PreflightError:
        pass
    else:
        raise PreflightError("unknown-policy-field negative control failed")
    try:
        classify(policy, empty, "unknown", "ok", **base)
    except PreflightError:
        pass
    else:
        raise PreflightError("unknown-selection negative control failed")
    try:
        validate_inventory({"total_count": 0, "runners": [{}]})
    except PreflightError:
        pass
    else:
        raise PreflightError("malformed-inventory negative control failed")
    with tempfile.TemporaryDirectory(prefix="genesis-ci-preflight-") as directory:
        output = Path(directory) / "github-output"
        write_github_output(output, matrix)
        rendered = output.read_text(encoding="utf-8")
        require("primary_linux_dispatch=true" in rendered and "overall_status=ready" in rendered, "GitHub output control failed")
    print("ci-runner-preflight: self-test ok (10 controls)")


def main() -> int:
    parser = argparse.ArgumentParser()
    subparsers = parser.add_subparsers(dest="command", required=True)
    classify_parser = subparsers.add_parser("classify")
    classify_parser.add_argument("--policy", type=Path, required=True)
    classify_parser.add_argument("--inventory", type=Path, required=True)
    classify_parser.add_argument("--inventory-status", required=True)
    classify_parser.add_argument("--selection", required=True)
    classify_parser.add_argument("--repository", required=True)
    classify_parser.add_argument("--revision", required=True)
    classify_parser.add_argument("--run-id", required=True)
    classify_parser.add_argument("--run-attempt", required=True)
    classify_parser.add_argument("--out", type=Path, required=True)
    classify_parser.add_argument("--github-output", type=Path)
    verify_parser = subparsers.add_parser("verify")
    verify_parser.add_argument("--report", type=Path, required=True)
    self_parser = subparsers.add_parser("self-test")
    self_parser.add_argument("--policy", type=Path, required=True)
    args = parser.parse_args()
    try:
        if args.command == "classify":
            report = classify(
                read_json(args.policy),
                read_json(args.inventory),
                args.selection,
                args.inventory_status,
                args.repository,
                args.revision,
                args.run_id,
                args.run_attempt,
            )
            args.out.parent.mkdir(parents=True, exist_ok=True)
            args.out.write_text(json.dumps(report, indent=2, sort_keys=True) + "\n", encoding="utf-8")
            if args.github_output:
                write_github_output(args.github_output, report)
            print(f"ci-runner-preflight: status={report['status']} selection={report['selectionProfile']}")
        elif args.command == "verify":
            verify_report(read_json(args.report))
            print("ci-runner-preflight: verified")
        else:
            self_test(args.policy)
    except PreflightError as exc:
        print(f"ci-runner-preflight: {exc}", flush=True)
        return 1
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
