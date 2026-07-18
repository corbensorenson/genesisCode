#!/usr/bin/env python3
"""Derive a closed Open Agent campaign report from retained attempts."""

from __future__ import annotations

import argparse
import json
import sys
from pathlib import Path
from typing import Any

import genesisbench_open_agent as open_agent


KIND = "genesis/genesisbench-open-agent-campaign-report-v0.1"
MAX_EVENT_LINE_BYTES = 1024 * 1024


def retained_events(path: Path) -> list[dict[str, Any]]:
    events: list[dict[str, Any]] = []
    with path.open("rb") as stream:
        for raw in stream:
            try:
                event = json.loads(raw)
            except (UnicodeDecodeError, json.JSONDecodeError):
                continue
            if isinstance(event, dict):
                events.append(event)
    return events


def provider_messages(events: list[dict[str, Any]]) -> list[str]:
    return [
        event["message"]
        for event in events
        if event.get("type") == "error" and isinstance(event.get("message"), str)
    ]


def model_execution_observed(events: list[dict[str, Any]]) -> bool:
    for event in events:
        if event.get("type") == "turn.completed":
            return True
        item = event.get("item")
        if isinstance(item, dict) and item.get("type") in {
            "agent_message", "command_execution", "file_change", "reasoning",
        }:
            return True
    return False


def has_oversized_event_line(path: Path) -> bool:
    with path.open("rb") as stream:
        return any(len(raw) > MAX_EVENT_LINE_BYTES for raw in stream)


def classify(run_root: Path, run: dict[str, Any], events: list[dict[str, Any]]) -> list[str]:
    codes: list[str] = []
    campaign = open_agent.validate_campaign(open_agent.load_json(run_root / "campaign.json"))
    harness = open_agent.authority(campaign["authorities"]["harnessIdentitySha256"])
    current_harness = open_agent.is_v3_or_later_harness(harness)
    messages = provider_messages(events)
    unavailable = any(
        "not supported when using Codex" in message
        or "model is not supported" in message.lower()
        for message in messages
    )
    if unavailable:
        codes.append("model/unavailable-for-account")
    stderr = (run_root / "stderr.txt").read_text(encoding="utf-8")
    if "failed to load skill" in stderr:
        codes.append("harness/ambient-skill-discovery")
    violations = set(run["workspace"]["violations"])
    if "workspace-path-drift" in violations:
        before = {row["path"] for row in run["workspace"]["beforeInventory"]}
        after = {row["path"] for row in run["workspace"]["afterInventory"]}
        extras = after - before
        declared_outputs = extras & set(run["case"]["editablePaths"])
        undeclared_extras = extras - declared_outputs
        if declared_outputs:
            codes.append("harness/declared-editable-output-rejected")
        if undeclared_extras and all(path.startswith(".genesis/cache/") for path in undeclared_extras):
            codes.append("harness/tool-cache-path-contamination")
        elif undeclared_extras or not declared_outputs:
            codes.append(
                "model/undeclared-output"
                if current_harness and undeclared_extras
                else "harness/workspace-path-drift"
            )
    if "malformed-event-transcript" in violations:
        codes.append(
            "harness/event-line-limit-exceeded"
            if has_oversized_event_line(run_root / "events.jsonl")
            else "harness/malformed-event-transcript"
        )
    if "repository-invalid" in violations:
        codes.append("harness/repository-integrity")
    if "noneditable-input-drift" in violations:
        codes.append(
            "model/noneditable-input-drift"
            if current_harness
            else "harness/noneditable-input-drift"
        )
    if "workspace-invalid" in violations:
        codes.append("harness/workspace-invalid")
    if "timeout" in violations:
        codes.append("resource/timeout")
    if "capture-limit" in violations:
        codes.append("resource/capture-limit")
    if "nonzero-exit" in violations and not unavailable:
        codes.append("model/nonzero-exit")
    if run["outcome"] == "failed":
        codes.append("semantic/incorrect")
    if run["outcome"] == "invalid" and not codes:
        codes.append("model/invalid-unclassified")
    return sorted(codes)


def build(campaign_path: Path, runs: Path, genesis_bin: Path, selfhost_artifact: Path) -> dict[str, Any]:
    campaign = open_agent.validate_campaign(open_agent.load_json(campaign_path))
    harness = open_agent.authority(campaign["authorities"]["harnessIdentitySha256"])
    if open_agent.is_v3_or_later_harness(harness):
        open_agent.validate_tool_archive(campaign_path.parent / "tools" / "archive.json", campaign)
    attempts = []
    execution_by_case: dict[str, bool] = {}
    for case in campaign["cases"]:
        root = runs / case["id"]
        run = open_agent.validate_run(root / "run.json", check_files=True)
        replay = open_agent.replay_run(root / "run.json", genesis_bin, selfhost_artifact)
        events = retained_events(root / "events.jsonl")
        open_agent.require(run["case"] == case, "campaign report case binding drift")
        execution_by_case[case["id"]] = model_execution_observed(events)
        attempts.append({
            "caseId": case["id"],
            "runIdentitySha256": run["contentIdentitySha256"],
            "outcome": run["outcome"],
            "elapsedMs": run["attempt"]["elapsedMs"],
            "validationPassed": True,
            "replayPassed": replay["allFieldsValidated"] is True,
            "independentRescoreMatched": replay["independentRescoreMatched"],
            "failureCodes": classify(root, run, events),
        })
    complete = len(attempts) == campaign["publication"]["expectedAttemptCount"]
    provider_unavailable = complete and all("model/unavailable-for-account" in row["failureCodes"] for row in attempts)
    ambient_discovery = any("harness/ambient-skill-discovery" in row["failureCodes"] for row in attempts)
    tool_cache_contamination = any("harness/tool-cache-path-contamination" in row["failureCodes"] for row in attempts)
    declared_output_rejection = any("harness/declared-editable-output-rejected" in row["failureCodes"] for row in attempts)
    event_line_limit = any("harness/event-line-limit-exceeded" in row["failureCodes"] for row in attempts)
    harness_defect = any(code.startswith("harness/") for row in attempts for code in row["failureCodes"])
    invalid_count = sum(row["outcome"] == "invalid" for row in attempts)
    expansion_allowed = (
        complete
        and invalid_count == 0
        and all(row["outcome"] in {"verified", "failed"} for row in attempts)
        and all(row["validationPassed"] and row["replayPassed"] for row in attempts)
    )
    reason_codes = []
    if not complete:
        reason_codes.append("campaign/incomplete-matrix")
    if invalid_count:
        reason_codes.append("campaign/invalid-attempts")
    if provider_unavailable:
        reason_codes.append("campaign/model-unavailable")
    if harness_defect:
        reason_codes.append("campaign/harness-defect")
    non_claims = ["This Open Agent campaign is not a fixed-scaffold Cold Acquisition result."]
    if not campaign["model"]["immutableRevision"]:
        non_claims.append("The provider-observed model revision is not immutable, so this campaign remains unranked.")
    if provider_unavailable:
        non_claims.append("No GenesisCode capability score was observed because the requested model never began task execution.")
    elif harness_defect:
        non_claims.append("Harness-invalid attempts are excluded from GenesisCode capability scoring and are not model failures.")
    if invalid_count and any(row["outcome"] == "verified" for row in attempts):
        non_claims.append("Verified per-case outcomes do not establish aggregate capability while any predeclared attempt remains invalid.")
    report = {
        "kind": KIND,
        "version": "0.1.0",
        "campaignId": campaign["campaignId"],
        "campaignIdentitySha256": campaign["contentIdentitySha256"],
        "publicationClass": campaign["publication"]["class"],
        "matrix": {
            "expectedAttempts": campaign["publication"]["expectedAttemptCount"],
            "observedAttempts": len(attempts),
            "complete": complete,
        },
        "attempts": attempts,
        "summary": {
            "verified": sum(row["outcome"] == "verified" for row in attempts),
            "failed": sum(row["outcome"] == "failed" for row in attempts),
            "invalid": sum(row["outcome"] == "invalid" for row in attempts),
            "providerUnavailableForAccount": provider_unavailable,
            "ambientSkillDiscoveryObserved": ambient_discovery,
            "modelExecutionObserved": any(execution_by_case.values()),
            "toolCacheContaminationObserved": tool_cache_contamination,
            "declaredEditableOutputRejectionObserved": declared_output_rejection,
            "eventLineLimitObserved": event_line_limit,
        },
        "expansion": {
            "allowed": expansion_allowed,
            "reasonCodes": sorted(reason_codes),
        },
        "nonClaims": non_claims,
        "contentIdentitySha256": "",
    }
    open_agent.require(report["matrix"]["complete"], "campaign attempt matrix is incomplete")
    return open_agent.identified(report)


def main() -> int:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--campaign", required=True, type=Path)
    parser.add_argument("--runs", required=True, type=Path)
    parser.add_argument("--genesis-bin", required=True, type=Path)
    parser.add_argument("--selfhost-artifact", required=True, type=Path)
    parser.add_argument("--out", required=True, type=Path)
    args = parser.parse_args()
    open_agent.require(not args.out.exists(), "campaign report output already exists")
    report = build(args.campaign, args.runs, args.genesis_bin, args.selfhost_artifact)
    open_agent.write_json(args.out, report)
    sys.stdout.buffer.write(open_agent.pretty_bytes(report))
    return 0


if __name__ == "__main__":
    try:
        raise SystemExit(main())
    except (open_agent.OpenAgentError, OSError, UnicodeError, json.JSONDecodeError, KeyError, ValueError) as exc:
        sys.stderr.buffer.write(open_agent.pretty_bytes({
            "kind": "genesis/genesisbench-open-agent-campaign-report-error-v0.1",
            "code": "bench/open-agent-campaign-report-failed",
            "message": str(exc),
        }))
        raise SystemExit(1)
