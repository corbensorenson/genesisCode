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
UNSUPPORTED_LUNA = "The 'luna' model is not supported when using Codex with a ChatGPT account."


def provider_messages(events: Path) -> list[str]:
    messages: list[str] = []
    with events.open("r", encoding="utf-8") as stream:
        for raw in stream:
            event = json.loads(raw)
            if event.get("type") != "error":
                continue
            message = event.get("message")
            if isinstance(message, str):
                messages.append(message)
    return messages


def classify(run_root: Path) -> list[str]:
    codes: list[str] = []
    if any(UNSUPPORTED_LUNA in message for message in provider_messages(run_root / "events.jsonl")):
        codes.append("model/unavailable-for-account")
    stderr = (run_root / "stderr.txt").read_text(encoding="utf-8")
    if "failed to load skill" in stderr:
        codes.append("harness/ambient-skill-discovery")
    return sorted(codes)


def build(campaign_path: Path, runs: Path, genesis_bin: Path, selfhost_artifact: Path) -> dict[str, Any]:
    campaign = open_agent.validate_campaign(open_agent.load_json(campaign_path))
    attempts = []
    for case in campaign["cases"]:
        root = runs / case["id"]
        run = open_agent.validate_run(root / "run.json", check_files=True)
        replay = open_agent.replay_run(root / "run.json", genesis_bin, selfhost_artifact)
        open_agent.require(run["case"] == case, "campaign report case binding drift")
        attempts.append({
            "caseId": case["id"],
            "runIdentitySha256": run["contentIdentitySha256"],
            "outcome": run["outcome"],
            "elapsedMs": run["attempt"]["elapsedMs"],
            "validationPassed": True,
            "replayPassed": replay["allFieldsValidated"] is True,
            "independentRescoreMatched": replay["independentRescoreMatched"],
            "failureCodes": classify(root),
        })
    complete = len(attempts) == campaign["publication"]["expectedAttemptCount"]
    provider_unavailable = complete and all("model/unavailable-for-account" in row["failureCodes"] for row in attempts)
    ambient_discovery = any("harness/ambient-skill-discovery" in row["failureCodes"] for row in attempts)
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
        },
        "expansion": {
            "allowed": complete and all(row["outcome"] in {"verified", "failed"} for row in attempts) and not ambient_discovery,
            "reasonCodes": sorted(
                (["campaign/model-unavailable"] if provider_unavailable else [])
                + (["campaign/harness-defect"] if ambient_discovery else [])
            ),
        },
        "nonClaims": [
            "No GenesisCode capability score was observed because the requested model never began task execution.",
            "The mutable luna alias is not an immutable model identity and this campaign is unranked.",
            "This Open Agent campaign is not a fixed-scaffold Cold Acquisition result.",
        ],
        "contentIdentitySha256": "",
    }
    open_agent.require(report["matrix"]["complete"], "campaign attempt matrix is incomplete")
    open_agent.require(not report["expansion"]["allowed"], "failed reality gate incorrectly permits expansion")
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
