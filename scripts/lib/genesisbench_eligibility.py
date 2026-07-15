#!/usr/bin/env python3
"""Validate closed GenesisBench eligibility reports against bound inputs."""

from __future__ import annotations

import copy
import hashlib
import json
import re
from typing import Any


HOST_PATH_RE = re.compile(r"(?:/Users/|/home/|[A-Za-z]:\\\\Users\\\\)")


class EligibilityError(ValueError):
    pass


def require(condition: bool, message: str) -> None:
    if not condition:
        raise EligibilityError(message)


def closed(value: Any, keys: set[str], label: str) -> dict[str, Any]:
    require(isinstance(value, dict) and set(value) == keys, f"{label} fields are not closed")
    return value


def canonical_bytes(value: Any) -> bytes:
    return (
        json.dumps(value, sort_keys=True, separators=(",", ":"), ensure_ascii=True)
        + "\n"
    ).encode("ascii")


def content_identity(document: dict[str, Any]) -> str:
    unsigned = copy.deepcopy(document)
    unsigned["contentIdentitySha256"] = ""
    return hashlib.sha256(canonical_bytes(unsigned)).hexdigest()


def string_set(values: Any, label: str) -> None:
    require(
        isinstance(values, list)
        and len(values) <= 64
        and all(isinstance(value, str) and 1 <= len(value) <= 256 for value in values)
        and values == sorted(set(values)),
        f"{label} must be a bounded sorted string set",
    )


def validate_report(
    report: Any,
    profile: dict[str, Any],
    run: dict[str, Any],
    expected_case: dict[str, Any],
) -> dict[str, Any]:
    doc = closed(
        report,
        {
            "kind", "version", "protocol", "snapshot", "run", "case",
            "validation", "contamination", "eligibility", "contentIdentitySha256",
        },
        "eligibility report",
    )
    require(
        doc["kind"] == "genesis/genesisbench-eligibility-v0.1"
        and doc["version"] == "0.1.0",
        "eligibility report version drift",
    )
    closed(doc["protocol"], {"id", "version", "identitySha256"}, "report protocol")
    require(
        doc["protocol"]
        == {
            "id": profile["protocolId"],
            "version": profile["version"],
            "identitySha256": profile["contentIdentitySha256"],
        },
        "eligibility protocol binding drift",
    )
    closed(doc["snapshot"], {"commitSha1", "treeSha1", "manifestIdentitySha256"}, "report snapshot")
    require(
        doc["snapshot"]
        == {
            "commitSha1": profile["sourceSnapshot"]["commitSha1"],
            "treeSha1": profile["sourceSnapshot"]["treeSha1"],
            "manifestIdentitySha256": profile["sourceSnapshot"]["manifestIdentitySha256"],
        },
        "eligibility snapshot binding drift",
    )
    closed(
        doc["run"],
        {"id", "identitySha256", "modelId", "modelRevision", "attempts", "qualityScoreBasisPoints"},
        "report run",
    )
    require(
        doc["run"]
        == {
            "id": run["runId"],
            "identitySha256": run["contentIdentitySha256"],
            "modelId": run["model"]["modelId"],
            "modelRevision": run["model"]["modelRevision"],
            "attempts": len(run["invocation"]["attempts"]),
            "qualityScoreBasisPoints": run["score"]["qualityScoreBasisPoints"],
        },
        "eligibility run binding drift",
    )
    closed(
        doc["case"],
        {"id", "taskClass", "contextTier", "contextMode", "interactionMode", "split", "visibilityClass", "heldOutEpochId"},
        "report case",
    )
    require(doc["case"] == expected_case, "eligibility case binding drift")
    validation = closed(
        doc["validation"],
        {
            "profileValid", "snapshotValid", "runRecordValid", "scoreRecordValid",
            "independentRescoreRequired", "independentRescoreObserved", "judgeModelUsed",
        },
        "report validation",
    )
    require(
        validation["profileValid"] is True
        and validation["snapshotValid"] is True
        and validation["runRecordValid"] is True
        and validation["scoreRecordValid"] is True,
        "eligibility validation overclaim",
    )
    require(
        validation["independentRescoreRequired"] is True
        and isinstance(validation["independentRescoreObserved"], bool)
        and validation["judgeModelUsed"] is False,
        "eligibility scoring policy drift",
    )
    contamination = closed(
        doc["contamination"],
        {"claimedLabel", "strongestSupportedLabel", "evidenceCodes"},
        "report contamination",
    )
    require(
        contamination["claimedLabel"] == run["benchmark"]["contamination"],
        "eligibility contamination claim binding drift",
    )
    string_set(contamination["evidenceCodes"], "contamination evidence codes")
    eligibility = closed(
        doc["eligibility"],
        {"decision", "rankingCohort", "reasonCodes"},
        "report eligibility",
    )
    string_set(eligibility["reasonCodes"], "eligibility reason codes")
    decision = eligibility["decision"]
    require(decision in {"invalid", "ranked", "unranked"}, "invalid eligibility decision")
    require((decision == "ranked") == (not eligibility["reasonCodes"]), "eligibility reasons and decision disagree")
    reason_class = "invalidReasonCodes" if decision == "invalid" else "unrankedReasonCodes"
    allowed_reasons = set(profile["eligibilityPolicy"][reason_class]) if decision != "ranked" else set()
    require(set(eligibility["reasonCodes"]) <= allowed_reasons, "eligibility reason class drift")
    overclaim = contamination["claimedLabel"] != contamination["strongestSupportedLabel"]
    require(
        not overclaim
        or (decision == "invalid" and "contamination/overclaim" in eligibility["reasonCodes"]),
        "eligibility contamination decision drift",
    )
    require(
        overclaim or "contamination/overclaim" not in eligibility["reasonCodes"],
        "eligibility contamination reason drift",
    )
    require(decision != "ranked" or validation["independentRescoreObserved"], "ranked report lacks independent rescore")
    expected_cohort = (
        f"{profile['protocolId'].lower()}-{expected_case['contextMode']}-"
        f"{expected_case['interactionMode']}-{expected_case['visibilityClass']}-"
        f"{contamination['strongestSupportedLabel']}"
    )
    require(eligibility["rankingCohort"] == expected_cohort, "eligibility cohort binding drift")
    require(HOST_PATH_RE.search(canonical_bytes(doc).decode("ascii")) is None, "eligibility report leaks a host path")
    require(doc["contentIdentitySha256"] == content_identity(doc), "eligibility report identity drift")
    return doc
