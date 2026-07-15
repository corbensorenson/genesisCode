#!/usr/bin/env python3
"""Validate scaled public commitments and locally held private custody packs."""

from __future__ import annotations

import argparse
import copy
from collections import Counter
from datetime import date, datetime, timezone
import hashlib
import json
import os
import re
import stat
import sys
from pathlib import Path
from typing import Any

ROOT = Path(__file__).resolve().parents[2]
MANIFEST = ROOT / "docs/spec/GC_AGENT_HELD_OUT_EVALUATION_v0.1.json"
SCHEMA = ROOT / "docs/spec/GC_AGENT_HELD_OUT_EVALUATION_v0.1.schema.json"
PRIVATE_SCHEMA = ROOT / "docs/spec/GC_AGENT_HELD_OUT_PRIVATE_PACK_v0.1.schema.json"
PRIVATE_ROOT = ROOT / ".genesis/private/agent-evaluation"
AUDIT = ROOT / "docs/program/GENESISBENCH_TEMPORAL_EPOCH_AUDIT_v0.1.json"
SHA_RE = re.compile(r"^[0-9a-f]{64}$")
DATE_RE = re.compile(r"^20[0-9]{2}-[0-9]{2}-[0-9]{2}$")
TIMESTAMP_RE = re.compile(r"^20[0-9]{2}-[0-9]{2}-[0-9]{2}T[0-9]{2}:[0-9]{2}:[0-9]{2}Z$")
HOST_PATH_RE = re.compile(r"(?:/Users/|/home/|[A-Za-z]:\\\\Users\\\\)")
TASK_CLASSES = [
    "completion", "deployment", "generation", "package-migration",
    "performance-repair", "policy-minimization", "refactor", "repair",
    "replay-investigation",
]
DIFFICULTIES = ["foundation", "engineering", "frontier"]
DOMAIN = b"genesis/agent-held-out-case/v0.1\0"
PILOT_EPOCH = "epoch-2026-07-a"
PILOT_SNAPSHOT = "77cfe60ca3959c06badfda1b8ae77b71c4bda2eecb776d4becbcf07cc55fe3cf"
SCALED_CASE_KEYS = {
    "id", "lineageId", "taskClass", "difficultyBand", "authorGeneratorId",
    "authorGeneratorFamily", "domain", "acceptanceShape", "rankingWeightUnits",
    "newlyPrecommitted", "overlayFamilyId", "oracleExposure",
    "metadataIdentitySha256", "commitmentSha256",
}


class HeldOutError(ValueError):
    pass


def require(condition: bool, message: str) -> None:
    if not condition:
        raise HeldOutError(message)


def load_json(path: Path) -> Any:
    def pairs(rows: list[tuple[str, Any]]) -> dict[str, Any]:
        result: dict[str, Any] = {}
        for key, value in rows:
            require(key not in result, f"duplicate JSON key: {key}")
            result[key] = value
        return result
    return json.loads(path.read_text(encoding="utf-8"), object_pairs_hook=pairs)


def canonical_bytes(value: Any) -> bytes:
    return (
        json.dumps(value, sort_keys=True, separators=(",", ":"), ensure_ascii=True)
        + "\n"
    ).encode("ascii")


def sha256_value(value: Any) -> str:
    return hashlib.sha256(canonical_bytes(value)).hexdigest()


def signed_identity(document: dict[str, Any], field: str = "contentIdentitySha256") -> str:
    unsigned = copy.deepcopy(document)
    unsigned[field] = ""
    return sha256_value(unsigned)


def content_identity(document: dict[str, Any]) -> str:
    return signed_identity(document)


def commitment(salt_hex: str, payload: Any) -> str:
    require(re.fullmatch(r"[0-9a-f]{64}", salt_hex) is not None, "private salt must be exactly 32 bytes")
    return hashlib.sha256(DOMAIN + bytes.fromhex(salt_hex) + canonical_bytes(payload)).hexdigest()


def closed(value: Any, keys: set[str], label: str) -> dict[str, Any]:
    require(isinstance(value, dict) and set(value) == keys, f"{label} fields are not closed")
    return value


def timestamp(value: Any, label: str) -> datetime:
    require(isinstance(value, str) and TIMESTAMP_RE.fullmatch(value) is not None, f"invalid {label}")
    try:
        return datetime.strptime(value, "%Y-%m-%dT%H:%M:%SZ").replace(tzinfo=timezone.utc)
    except ValueError as exc:
        raise HeldOutError(f"invalid {label}") from exc


def validate_schema() -> None:
    schema = load_json(SCHEMA)
    private = load_json(PRIVATE_SCHEMA)
    require(schema.get("$schema") == "https://json-schema.org/draft/2020-12/schema", "schema draft drift")
    require(schema.get("$id") == "https://genesiscode.dev/schemas/gc-agent-held-out-evaluation-v0.1.json", "schema id drift")
    require(schema.get("additionalProperties") is False, "schema root must be closed")
    for name in (
        "protocol", "storage", "lifecycle", "contamination", "quality", "profile",
        "pilotCase", "scaledCase", "pilotEpoch", "custody", "precommitment",
        "balance", "overlay", "scaledEpoch", "disclosure",
    ):
        require(schema.get("$defs", {}).get(name, {}).get("additionalProperties") is False, f"schema {name} must be closed")
    require(private.get("additionalProperties") is False, "private schema root must be closed")
    require(private.get("$defs", {}).get("case", {}).get("additionalProperties") is False, "private case schema must be closed")


def expected_quality_policy() -> dict[str, Any]:
    return {
        "previewMinimumLineages": 45,
        "matureMinimumLineages": 90,
        "previewMinimumPerTaskClass": 5,
        "matureMinimumPerTaskClass": 10,
        "difficultyBands": DIFFICULTIES,
        "minimumAuthorGeneratorIdentities": 2,
        "maximumAuthorGeneratorFamilyWeightBp": 2500,
        "minimumFreshRankingWeightBp": 2500,
        "maximumActiveDays": 90,
        "requiredBalanceDimensions": ["taskClass", "difficultyBand", "authorGenerator", "authorGeneratorFamily", "domain", "acceptanceShape"],
        "eligibleOverlayKinds": ["capability-protocol", "data-structure", "package", "semantic-patch-operation"],
        "benchmarkOnlyOverlayForbidden": True,
        "maintainOverlayAfterRankingRetirement": True,
    }


def metadata_identity(case: dict[str, Any]) -> str:
    metadata = {key: value for key, value in case.items() if key not in {"metadataIdentitySha256", "commitmentSha256"}}
    return sha256_value(metadata)


def count_weights(cases: list[dict[str, Any]], field: str) -> dict[str, int]:
    counts: Counter[str] = Counter()
    for case in cases:
        counts[str(case[field])] += case["rankingWeightUnits"]
    return dict(sorted(counts.items()))


def balance_summary(cases: list[dict[str, Any]]) -> dict[str, Any]:
    total = sum(case["rankingWeightUnits"] for case in cases)
    fresh = sum(case["rankingWeightUnits"] for case in cases if case["newlyPrecommitted"])
    families = count_weights(cases, "authorGeneratorFamily")
    result = {
        "lineageCount": len(cases),
        "rankingWeightUnits": total,
        "freshRankingWeightUnits": fresh,
        "freshRankingWeightBp": fresh * 10000 // total,
        "perTaskClass": count_weights(cases, "taskClass"),
        "perDifficultyBand": count_weights(cases, "difficultyBand"),
        "perAuthorGenerator": count_weights(cases, "authorGeneratorId"),
        "perAuthorGeneratorFamily": families,
        "perDomain": count_weights(cases, "domain"),
        "perAcceptanceShape": count_weights(cases, "acceptanceShape"),
        "maxAuthorGeneratorFamilyWeightBp": max(families.values()) * 10000 // total,
        "balanceIdentitySha256": "",
    }
    result["balanceIdentitySha256"] = signed_identity(result, "balanceIdentitySha256")
    return result


def validate_profile(profile: Any, epoch_id: str) -> None:
    closed(profile, {"id", "path", "sha256"}, f"{epoch_id} profile")
    require(profile["id"] == "GC-AGENT-v0.3" and profile["path"] == "docs/spec/GC_AGENT_PROFILE_v0.3.json", "profile authority drift")
    path = ROOT / profile["path"]
    require(path.is_file() and hashlib.sha256(path.read_bytes()).hexdigest() == profile["sha256"], "profile hash drift")


def validate_pilot(epoch: dict[str, Any], epoch_ids: list[str], all_commitments: set[str]) -> set[str]:
    closed(epoch, {"id", "status", "activatedOn", "retiredOn", "replacementEpochId", "profile", "cases", "commitmentSnapshotIdentity"}, "pilot epoch")
    require(epoch["id"] == PILOT_EPOCH and epoch["commitmentSnapshotIdentity"] == PILOT_SNAPSHOT, "pilot epoch identity drift")
    require(epoch["status"] == "retired" and epoch["replacementEpochId"] in epoch_ids and epoch["replacementEpochId"] != epoch["id"], "pilot replacement drift")
    require(DATE_RE.fullmatch(epoch["activatedOn"]) is not None and DATE_RE.fullmatch(epoch["retiredOn"]) is not None, "pilot lifecycle date drift")
    require(date.fromisoformat(epoch["retiredOn"]) >= date.fromisoformat(epoch["activatedOn"]), "pilot retirement predates activation")
    validate_profile(epoch["profile"], epoch["id"])
    cases = epoch["cases"]
    require(isinstance(cases, list) and len(cases) == len(TASK_CLASSES), "pilot task coverage drift")
    ids = [case.get("id") for case in cases if isinstance(case, dict)]
    require(ids == sorted(set(ids)), "pilot case ids must be sorted and unique")
    rows = []
    classes = []
    for case in cases:
        closed(case, {"id", "taskClass", "commitmentSha256", "oracleExposure"}, "pilot case")
        require(case["taskClass"] in TASK_CLASSES and case["oracleExposure"] == "commitment-only", "pilot case metadata drift")
        require(SHA_RE.fullmatch(case["commitmentSha256"]) is not None and case["commitmentSha256"] not in all_commitments, "pilot commitment drift or reuse")
        all_commitments.add(case["commitmentSha256"])
        classes.append(case["taskClass"])
        rows.append({"id": case["id"], "taskClass": case["taskClass"], "commitmentSha256": case["commitmentSha256"]})
    require(sorted(classes) == sorted(TASK_CLASSES), "pilot task classes incomplete")
    require(sha256_value(rows) == PILOT_SNAPSHOT, "immutable pilot commitments changed")
    return set(ids)


def validate_scaled(epoch: dict[str, Any], quality: dict[str, Any], all_commitments: set[str]) -> set[str]:
    keys = {
        "id", "status", "scaleTarget", "activatedOn", "precommittedAt", "rotationDueOn",
        "retiredOn", "replacementEpochId", "rotationTriggers", "profile", "custody",
        "precommitment", "balanceSummary", "overlays", "cases", "commitmentSnapshotIdentity",
    }
    closed(epoch, keys, "scaled epoch")
    require(re.fullmatch(r"epoch-20[0-9]{2}-[0-9]{2}-[a-z]", epoch["id"]) is not None, "invalid scaled epoch id")
    require(epoch["status"] in {"active", "compromised", "retired"}, "scaled epoch state drift")
    require(epoch["scaleTarget"] in {"preview", "mature"}, "scaled epoch target drift")
    require(DATE_RE.fullmatch(epoch["activatedOn"]) is not None and DATE_RE.fullmatch(epoch["rotationDueOn"]) is not None, "scaled epoch date drift")
    precommitted_at = timestamp(epoch["precommittedAt"], "epoch precommit timestamp")
    activated = date.fromisoformat(epoch["activatedOn"])
    due = date.fromisoformat(epoch["rotationDueOn"])
    require(precommitted_at.date() <= activated and 0 <= (due - activated).days <= quality["maximumActiveDays"], "scaled epoch chronology or rotation deadline drift")
    require(epoch["rotationTriggers"] == ["leakage", "saturation", "schedule-90-days"], "rotation triggers drift")
    if epoch["status"] == "active":
        require(epoch["retiredOn"] is None and epoch["replacementEpochId"] is None, "active scaled epoch has retirement metadata")
    else:
        require(isinstance(epoch["retiredOn"], str) and DATE_RE.fullmatch(epoch["retiredOn"]) is not None, "inactive scaled epoch lacks retirement date")
        require(isinstance(epoch["replacementEpochId"], str) and epoch["replacementEpochId"] != epoch["id"], "inactive scaled epoch lacks replacement")
    validate_profile(epoch["profile"], epoch["id"])

    custody = closed(epoch["custody"], {"privateRoot", "privatePackMode", "separateCapabilityRoles", "trainingBuilderCanReadActivePack", "evaluatorCanWriteTrainingStore", "publicArtifactsContainPrivatePayloads", "crossRoleTransferPolicy", "attestationIdentitySha256"}, "custody attestation")
    require(custody["privateRoot"] == f".genesis/private/agent-evaluation/{epoch['id']}" and custody["privatePackMode"] == "0600", "custody root or mode drift")
    require(custody["separateCapabilityRoles"] == ["author", "custodian", "evaluator", "training-builder"], "custody role separation drift")
    require(custody["trainingBuilderCanReadActivePack"] is False and custody["evaluatorCanWriteTrainingStore"] is False and custody["publicArtifactsContainPrivatePayloads"] is False, "custody firewall weakened")
    require(custody["crossRoleTransferPolicy"] == "explicit-reviewed-content-addressed-record", "custody transfer policy drift")
    require(custody["attestationIdentitySha256"] == signed_identity(custody, "attestationIdentitySha256"), "custody attestation identity drift")

    cases = epoch["cases"]
    minimum = quality[f"{epoch['scaleTarget']}MinimumLineages"]
    per_class_min = quality[f"{epoch['scaleTarget']}MinimumPerTaskClass"]
    require(isinstance(cases, list) and len(cases) >= minimum, f"{epoch['id']}: BQ-11 lineage scale not met")
    ids = [case.get("id") for case in cases if isinstance(case, dict)]
    lineage_ids = [case.get("lineageId") for case in cases if isinstance(case, dict)]
    require(ids == sorted(set(ids)) and len(set(lineage_ids)) == len(cases), "scaled case or lineage ids are not unique and sorted")
    for case in cases:
        closed(case, SCALED_CASE_KEYS, "scaled case")
        require(re.fullmatch(r"ho-[a-z0-9-]+-[0-9]{2}", case["id"]) is not None and re.fullmatch(r"gb-[a-z0-9-]+", case["lineageId"]) is not None, "invalid scaled case identity")
        require(case["taskClass"] in TASK_CLASSES and case["difficultyBand"] in DIFFICULTIES, "scaled case class or difficulty drift")
        for field in ("authorGeneratorId", "authorGeneratorFamily", "domain", "acceptanceShape"):
            require(isinstance(case[field], str) and re.fullmatch(r"[a-z][a-z0-9-]{1,63}", case[field]) is not None, f"{case['id']}: invalid {field}")
        require(isinstance(case["rankingWeightUnits"], int) and not isinstance(case["rankingWeightUnits"], bool) and case["rankingWeightUnits"] > 0, "ranking weight must be positive")
        require(isinstance(case["newlyPrecommitted"], bool), "freshness marker must be boolean")
        require(case["overlayFamilyId"] is None or isinstance(case["overlayFamilyId"], str), "invalid overlay family id")
        require(case["oracleExposure"] == "commitment-only", "private oracle exposure")
        require(case["metadataIdentitySha256"] == metadata_identity(case), f"{case['id']}: public metadata identity drift")
        require(SHA_RE.fullmatch(case["commitmentSha256"]) is not None and case["commitmentSha256"] not in all_commitments, f"{case['id']}: commitment reuse")
        all_commitments.add(case["commitmentSha256"])
    snapshot = sha256_value(cases)
    require(epoch["commitmentSnapshotIdentity"] == snapshot, "scaled commitment snapshot drift")

    summary = balance_summary(cases)
    require(epoch["balanceSummary"] == summary, "scaled balance summary drift")
    require(set(summary["perTaskClass"]) == set(TASK_CLASSES) and min(summary["perTaskClass"].values()) >= per_class_min, "BQ-11 per-class minimum not met")
    require(set(summary["perDifficultyBand"]) == set(DIFFICULTIES), "difficulty balance is incomplete")
    require(len(summary["perAuthorGenerator"]) >= quality["minimumAuthorGeneratorIdentities"], "author/generator diversity not met")
    require(summary["maxAuthorGeneratorFamilyWeightBp"] <= quality["maximumAuthorGeneratorFamilyWeightBp"], "author/generator family controls too much ranking weight")
    require(summary["freshRankingWeightBp"] >= quality["minimumFreshRankingWeightBp"], "BQ-4 fresh ranking weight not met")
    for field in ("perDifficultyBand", "perAuthorGenerator", "perDomain", "perAcceptanceShape"):
        values = list(summary[field].values())
        require(len(values) >= 3 and max(values) - min(values) <= 1, f"{field} is not balanced")

    precommit = closed(epoch["precommitment"], {"committedAt", "modelReleaseRule", "eligibleModelReleaseBefore", "chronologyEvidenceRequiredPerRun", "commitmentSnapshotIdentity", "attestationIdentitySha256"}, "precommit attestation")
    require(precommit["committedAt"] == epoch["precommittedAt"] == precommit["eligibleModelReleaseBefore"], "precommit chronology binding drift")
    require(precommit["modelReleaseRule"] == "immutable-model-release-must-precede-committedAt" and precommit["chronologyEvidenceRequiredPerRun"] is True, "temporal-clean chronology rule weakened")
    require(precommit["commitmentSnapshotIdentity"] == snapshot and precommit["attestationIdentitySha256"] == signed_identity(precommit, "attestationIdentitySha256"), "precommit attestation identity drift")

    overlays = epoch["overlays"]
    require(isinstance(overlays, list) and overlays, "scaled epoch requires a useful post-release overlay")
    overlay_ids: set[str] = set()
    overlay_lineages: set[str] = set()
    for overlay in overlays:
        closed(overlay, {"id", "kind", "introducedOn", "authorityPath", "authoritySha256", "benchmarkOnly", "maintenanceStatus", "rankingRetirementPolicy", "maintenanceGate", "challengeLineageIds", "contentIdentitySha256"}, "overlay")
        require(overlay["id"] not in overlay_ids and overlay["kind"] in quality["eligibleOverlayKinds"], "overlay identity or kind drift")
        overlay_ids.add(overlay["id"])
        require(overlay["benchmarkOnly"] is False and overlay["maintenanceStatus"] == "active" and overlay["rankingRetirementPolicy"] == "maintain-after-ranking-retirement", "overlay is benchmark-only or not durably maintained")
        require(DATE_RE.fullmatch(overlay["introducedOn"]) is not None and date.fromisoformat(overlay["introducedOn"]) >= precommitted_at.date(), "overlay does not postdate the release boundary")
        require(re.fullmatch(r"docs/spec/[A-Za-z0-9_.-]+", overlay["authorityPath"]) is not None, "overlay authority path is not canonical")
        authority = ROOT / overlay["authorityPath"]
        require(authority.is_file() and not authority.is_symlink() and hashlib.sha256(authority.read_bytes()).hexdigest() == overlay["authoritySha256"], "overlay authority hash drift")
        require(overlay["maintenanceGate"] == "python3 scripts/lib/gc_capability_lease.py --check --self-test", "overlay maintenance gate drift")
        lineages = overlay["challengeLineageIds"]
        require(isinstance(lineages, list) and lineages == sorted(set(lineages)) and set(lineages).issubset(set(lineage_ids)), "overlay challenge lineage binding drift")
        overlay_lineages.update(lineages)
        require(overlay["contentIdentitySha256"] == signed_identity(overlay), "overlay content identity drift")
    declared_overlay_lineages = {case["lineageId"] for case in cases if case["overlayFamilyId"] is not None}
    require(declared_overlay_lineages == overlay_lineages, "overlay cases and challenge family inventory disagree")
    require(all(case["overlayFamilyId"] in overlay_ids for case in cases if case["overlayFamilyId"] is not None), "case references unknown overlay")
    return set(ids)


def validate_disclosures(document: dict[str, Any], cases_by_epoch: dict[str, set[str]]) -> None:
    disclosures = document["disclosures"]
    require(isinstance(disclosures, list), "disclosures must be an array")
    ids = [row.get("id") for row in disclosures if isinstance(row, dict)]
    require(ids == sorted(set(ids)), "disclosure ids must be sorted and unique")
    for row in disclosures:
        closed(row, {"id", "epochId", "caseId", "disclosedOn", "reason", "payloadPath", "saltHex", "verifiedCommitmentSha256"}, "disclosure")
        require(row["epochId"] in cases_by_epoch and row["caseId"] in cases_by_epoch[row["epochId"]], "disclosure target is unknown")
        epoch = next(item for item in document["epochs"] if item["id"] == row["epochId"])
        require(epoch["status"] != "active", "active epoch cannot be disclosed")
        require(DATE_RE.fullmatch(row["disclosedOn"]) is not None, "invalid disclosure date")
        if row["reason"] == "scheduled-release":
            delay = (date.fromisoformat(row["disclosedOn"]) - date.fromisoformat(epoch["retiredOn"])).days
            require(epoch["status"] == "retired" and delay >= document["lifecyclePolicy"]["minimumDisclosureDelayDays"], "scheduled disclosure is too early")
        else:
            require(row["reason"] == "compromise-forensics" and epoch["status"] == "compromised", "invalid forensic disclosure")
        require(re.fullmatch(r"docs/program/held-out-disclosures/[A-Za-z0-9._/-]+", row["payloadPath"]) is not None, "disclosure path must be public and canonical")
        payload_path = ROOT / row["payloadPath"]
        require(payload_path.is_file() and not payload_path.is_symlink() and payload_path.resolve().is_relative_to(ROOT.resolve()), "disclosure payload is missing or escaped")
        case = next(item for item in epoch["cases"] if item["id"] == row["caseId"])
        require(commitment(row["saltHex"], load_json(payload_path)) == case["commitmentSha256"] == row["verifiedCommitmentSha256"], "disclosure does not open the commitment")


def validate(document: Any, *, check_identity: bool = True) -> dict[str, Any]:
    doc = closed(document, {"kind", "version", "evaluationId", "commitmentProtocol", "storagePolicy", "lifecyclePolicy", "contaminationPolicy", "benchmarkQualityPolicy", "epochs", "disclosures", "contentIdentitySha256"}, "manifest")
    require(doc["kind"] == "genesis/agent-held-out-evaluation-v0.1" and doc["version"] == "0.1.0" and doc["evaluationId"] == "GC-AGENT-HELD-OUT-v0.1", "manifest identity drift")
    require(doc["commitmentProtocol"] == {"algorithm": "sha256", "canonicalization": "genesis/canonical-json-v0.1", "caseDomain": "genesis/agent-held-out-case/v0.1\\0", "formula": "sha256(domain || 32-byte-secret-salt || canonical-case-bytes)", "saltBytes": 32, "saltVisibility": "withheld-until-disclosure"}, "commitment protocol drift")
    require(doc["storagePolicy"] == {"privateRoot": ".genesis/private/agent-evaluation", "publicContent": ["commitments", "disclosure-records", "lifecycle-metadata"], "forbiddenDistribution": ["case-payloads", "oracles", "salts"], "trainingPackExclusionRequired": True, "custodyRoles": ["author", "custodian", "evaluator", "training-builder"], "crossRoleTransferRequiresReviewedRecord": True}, "storage policy drift")
    require(doc["lifecyclePolicy"] == {"appendOnlyEpochs": True, "compromiseRequiresReplacement": True, "minimumDisclosureDelayDays": 30, "retiredCommitmentsRemainPublished": True, "rotationStates": ["active", "compromised", "retired"], "maximumActiveDays": 90, "rotationTriggers": ["leakage", "saturation", "schedule-90-days"], "minimumFreshRankingWeightBp": 2500, "overlayMaintenanceAfterRankingRetirement": True}, "lifecycle policy drift")
    require(doc["contaminationPolicy"] == {"resultLabels": ["declared-contaminated", "declared-uncontaminated", "temporal-clean", "unknown"], "defaultWhenTrainingProvenanceMissing": "unknown", "disclosedCaseRequiresContaminatedLabel": True, "mixedEpochAggregationForbidden": True, "requiredResultBindings": ["commitmentSnapshotIdentity", "epochId", "modelIdentity", "trainingCutoff"]}, "contamination policy drift")
    quality = expected_quality_policy()
    require(doc["benchmarkQualityPolicy"] == quality, "benchmark quality policy drift")
    epochs = doc["epochs"]
    require(isinstance(epochs, list) and len(epochs) >= 2, "pilot and scaled epochs are required")
    epoch_ids = [row.get("id") for row in epochs if isinstance(row, dict)]
    require(epoch_ids == sorted(set(epoch_ids)), "epoch ids must be sorted and unique")
    active = [row for row in epochs if row.get("status") == "active"]
    require(len(active) == 1 and active[0].get("id") != PILOT_EPOCH, "exactly one scaled active epoch is required")
    all_commitments: set[str] = set()
    cases_by_epoch: dict[str, set[str]] = {}
    for epoch in epochs:
        if epoch.get("id") == PILOT_EPOCH:
            cases_by_epoch[epoch["id"]] = validate_pilot(epoch, epoch_ids, all_commitments)
        else:
            cases_by_epoch[epoch["id"]] = validate_scaled(epoch, quality, all_commitments)
    validate_disclosures(doc, cases_by_epoch)
    require(HOST_PATH_RE.search(canonical_bytes(doc).decode("ascii")) is None, "public manifest leaks a host path")
    if check_identity:
        require(SHA_RE.fullmatch(doc["contentIdentitySha256"]) is not None and content_identity(doc) == doc["contentIdentitySha256"], "manifest content identity drift")
    return doc


def private_payload_keys() -> set[str]:
    return {"caseVersion", "lineageId", "taskClass", "title", "prompt", "difficultyBand", "authorGeneratorId", "authorGeneratorFamily", "domain", "acceptanceShape", "overlayFamilyId", "overlayAuthoritySha256", "inputWorkspace", "editablePaths", "referenceWorkspace", "verification", "followOnRequirement", "noveltyStatement", "publicMetadataIdentitySha256", "nonce"}


def verify_private(document: dict[str, Any], path: Path) -> dict[str, Any]:
    resolved = path.resolve(strict=True)
    require(resolved.is_relative_to(PRIVATE_ROOT.resolve()), "private pack must remain under the ignored custody root")
    require(path.is_file() and not path.is_symlink(), "private pack must be a regular non-symlink file")
    mode = stat.S_IMODE(path.stat().st_mode)
    require(mode == 0o600, "private pack mode must be 0600")
    pack = closed(load_json(path), {"kind", "version", "evaluationId", "epochId", "commitmentSnapshotIdentity", "cases"}, "private pack")
    require(pack["kind"] == "genesis/agent-held-out-private-pack-v0.1" and pack["version"] == "0.2.0", "private pack identity drift")
    require(pack["evaluationId"] == document["evaluationId"], "private pack evaluation mismatch")
    epoch = next((row for row in document["epochs"] if row["id"] == pack["epochId"]), None)
    require(epoch is not None and epoch["id"] != PILOT_EPOCH, "private pack epoch is not a scaled public epoch")
    require(pack["commitmentSnapshotIdentity"] == epoch["commitmentSnapshotIdentity"], "private pack snapshot mismatch")
    public = {row["id"]: row for row in epoch["cases"]}
    require([row.get("id") for row in pack["cases"]] == sorted(public), "private pack case inventory drift")
    salts: set[str] = set()
    nonces: set[str] = set()
    prompts: set[str] = set()
    overlay_authorities = {row["id"]: row["authoritySha256"] for row in epoch["overlays"]}
    for row in pack["cases"]:
        closed(row, {"id", "lineageId", "taskClass", "saltHex", "payload"}, "private case")
        expected = public[row["id"]]
        require(row["lineageId"] == expected["lineageId"] and row["taskClass"] == expected["taskClass"], f"{row['id']}: private lineage metadata drift")
        require(row["saltHex"] not in salts and re.fullmatch(r"[0-9a-f]{64}", row["saltHex"]) is not None, f"{row['id']}: weak or reused salt")
        salts.add(row["saltHex"])
        payload = closed(row["payload"], private_payload_keys(), "private payload")
        for field in ("lineageId", "taskClass", "difficultyBand", "authorGeneratorId", "authorGeneratorFamily", "domain", "acceptanceShape", "overlayFamilyId"):
            require(payload[field] == expected[field], f"{row['id']}: private {field} drift")
        require(payload["caseVersion"] == "0.2.0" and payload["publicMetadataIdentitySha256"] == expected["metadataIdentitySha256"], f"{row['id']}: payload version or public binding drift")
        require(commitment(row["saltHex"], payload) == expected["commitmentSha256"], f"{row['id']}: commitment mismatch")
        require(isinstance(payload["title"], str) and len(payload["title"]) >= 8 and isinstance(payload["prompt"], str) and len(payload["prompt"]) >= 100, f"{row['id']}: task statement is underspecified")
        lower = (payload["prompt"] + " " + payload["noveltyStatement"]).lower()
        require("riddle" not in lower.replace("not a riddle", "") and "benchmark-only" not in lower.replace("not a riddle or benchmark-only api", ""), f"{row['id']}: ineligible arbitrary or benchmark-only task")
        require(payload["prompt"] not in prompts, f"{row['id']}: duplicate task prompt")
        prompts.add(payload["prompt"])
        require(re.fullmatch(r"[0-9a-f]{64}", payload["nonce"]) is not None and payload["nonce"] not in nonces, f"{row['id']}: nonce drift or reuse")
        nonces.add(payload["nonce"])
        inputs = payload["inputWorkspace"]
        reference = payload["referenceWorkspace"]
        require(isinstance(inputs, dict) and inputs and isinstance(reference, dict) and reference and inputs != reference, f"{row['id']}: task lacks distinct input and reference workspaces")
        for workspace in (inputs, reference):
            require(all(isinstance(key, str) and key and not key.startswith("/") and ".." not in Path(key).parts and isinstance(value, str) for key, value in workspace.items()), f"{row['id']}: unsafe or non-text workspace material")
        require(isinstance(payload["editablePaths"], list) and payload["editablePaths"] == sorted(set(payload["editablePaths"])) and set(payload["editablePaths"]) == set(reference), f"{row['id']}: editable/reference surface drift")
        verification = closed(payload["verification"], {"deterministic", "modelJudgeForbidden", "referencePatchEqualityRequired", "checks", "failureIsNonSolve"}, "private verification")
        require(verification == {"deterministic": True, "modelJudgeForbidden": True, "referencePatchEqualityRequired": False, "checks": ["parse-and-type", "semantic-behavior", "declared-acceptance-envelope"], "failureIsNonSolve": True}, f"{row['id']}: deterministic acceptance contract drift")
        require(isinstance(payload["followOnRequirement"], str) and len(payload["followOnRequirement"]) >= 80, f"{row['id']}: missing maintenance follow-on")
        overlay_id = payload["overlayFamilyId"]
        if overlay_id is None:
            require(payload["overlayAuthoritySha256"] is None, f"{row['id']}: undeclared overlay authority")
        else:
            require(payload["overlayAuthoritySha256"] == overlay_authorities.get(overlay_id), f"{row['id']}: overlay authority binding drift")
    pack_identity = hashlib.sha256(path.read_bytes()).hexdigest()
    return {
        "epochId": epoch["id"],
        "commitmentSnapshotIdentity": epoch["commitmentSnapshotIdentity"],
        "privatePackIdentitySha256": pack_identity,
        "verifiedLineages": len(pack["cases"]),
        "commitmentOpeningsVerified": len(pack["cases"]),
        "metadataBindingsVerified": len(pack["cases"]),
        "qualityContractsVerified": len(pack["cases"]),
        "custodyAttestationIdentitySha256": epoch["custody"]["attestationIdentitySha256"],
    }


def write_audit(result: dict[str, Any], output: Path) -> None:
    require(output.resolve().is_relative_to(ROOT.resolve()), "audit output must remain in the repository")
    audit = {
        "kind": "genesis/genesisbench-temporal-epoch-audit-v0.1",
        "version": "0.1.0",
        **result,
        "verifierPath": "scripts/lib/gc_held_out_evaluation.py",
        "verifierSha256": hashlib.sha256(Path(__file__).read_bytes()).hexdigest(),
        "privateMaterialPublished": False,
        "verificationMode": "local-custody-openings-plus-public-independent-controls",
        "contentIdentitySha256": "",
    }
    audit["contentIdentitySha256"] = content_identity(audit)
    output.parent.mkdir(parents=True, exist_ok=True)
    output.write_text(json.dumps(audit, indent=2, sort_keys=True) + "\n", encoding="utf-8")


def validate_audit(document: dict[str, Any], path: Path = AUDIT) -> dict[str, Any]:
    audit = closed(load_json(path), {"kind", "version", "epochId", "commitmentSnapshotIdentity", "privatePackIdentitySha256", "verifiedLineages", "commitmentOpeningsVerified", "metadataBindingsVerified", "qualityContractsVerified", "custodyAttestationIdentitySha256", "verifierPath", "verifierSha256", "privateMaterialPublished", "verificationMode", "contentIdentitySha256"}, "temporal epoch audit")
    require(audit["kind"] == "genesis/genesisbench-temporal-epoch-audit-v0.1" and audit["version"] == "0.1.0", "temporal audit version drift")
    epoch = next((row for row in document["epochs"] if row["id"] == audit["epochId"] and row["status"] == "active"), None)
    require(epoch is not None, "temporal audit does not bind the active epoch")
    count = len(epoch["cases"])
    require(audit["commitmentSnapshotIdentity"] == epoch["commitmentSnapshotIdentity"] and audit["custodyAttestationIdentitySha256"] == epoch["custody"]["attestationIdentitySha256"], "temporal audit epoch binding drift")
    require(audit["verifiedLineages"] == audit["commitmentOpeningsVerified"] == audit["metadataBindingsVerified"] == audit["qualityContractsVerified"] == count, "temporal audit coverage drift")
    require(audit["privateMaterialPublished"] is False and audit["verificationMode"] == "local-custody-openings-plus-public-independent-controls", "temporal audit privacy or verification mode drift")
    verifier = ROOT / audit["verifierPath"]
    require(audit["verifierPath"] == "scripts/lib/gc_held_out_evaluation.py" and verifier.is_file() and hashlib.sha256(verifier.read_bytes()).hexdigest() == audit["verifierSha256"], "temporal audit verifier drift")
    require(SHA_RE.fullmatch(audit["privatePackIdentitySha256"]) is not None and audit["contentIdentitySha256"] == content_identity(audit), "temporal audit identity drift")
    return audit


def resign_document(document: dict[str, Any]) -> None:
    for epoch in document.get("epochs", []):
        if epoch.get("id") == PILOT_EPOCH or "cases" not in epoch:
            continue
        for case in epoch["cases"]:
            if isinstance(case, dict) and SCALED_CASE_KEYS.issubset(case):
                case["metadataIdentitySha256"] = metadata_identity(case)
        epoch["commitmentSnapshotIdentity"] = sha256_value(epoch["cases"])
        if "balanceSummary" in epoch:
            epoch["balanceSummary"] = balance_summary(epoch["cases"])
        if "custody" in epoch and "attestationIdentitySha256" in epoch["custody"]:
            epoch["custody"]["attestationIdentitySha256"] = signed_identity(epoch["custody"], "attestationIdentitySha256")
        for overlay in epoch.get("overlays", []):
            overlay["contentIdentitySha256"] = signed_identity(overlay)
        if "precommitment" in epoch:
            epoch["precommitment"]["commitmentSnapshotIdentity"] = epoch["commitmentSnapshotIdentity"]
            epoch["precommitment"]["attestationIdentitySha256"] = signed_identity(epoch["precommitment"], "attestationIdentitySha256")
    document["contentIdentitySha256"] = content_identity(document)


def self_test(document: dict[str, Any]) -> int:
    active_index = next(i for i, row in enumerate(document["epochs"]) if row["status"] == "active")
    controls: list[tuple[str, Any, bool]] = [
        ("unknown-field", lambda d: d.update({"oracle": True}), False),
        ("weak-salt", lambda d: d["commitmentProtocol"].__setitem__("saltBytes", 8), True),
        ("training-leak", lambda d: d["storagePolicy"].__setitem__("trainingPackExclusionRequired", False), True),
        ("mutable-history", lambda d: d["lifecyclePolicy"].__setitem__("appendOnlyEpochs", False), True),
        ("late-rotation", lambda d: d["epochs"][active_index].__setitem__("rotationDueOn", "2026-10-14"), True),
        ("missing-leak-trigger", lambda d: d["epochs"][active_index]["rotationTriggers"].remove("leakage"), True),
        ("pilot-commitment-rewrite", lambda d: d["epochs"][0]["cases"][0].__setitem__("commitmentSha256", "0" * 64), True),
        ("pilot-case-omission", lambda d: d["epochs"][0]["cases"].pop(), True),
        ("scaled-below-preview", lambda d: d["epochs"][active_index].__setitem__("cases", d["epochs"][active_index]["cases"][:44]), True),
        ("class-imbalance", lambda d: [row.__setitem__("taskClass", "generation") for row in d["epochs"][active_index]["cases"] if row["taskClass"] == "completion"], True),
        ("difficulty-omission", lambda d: [row.__setitem__("difficultyBand", "engineering") for row in d["epochs"][active_index]["cases"]], True),
        ("author-collapse", lambda d: [row.__setitem__("authorGeneratorId", "single-generator") for row in d["epochs"][active_index]["cases"]], True),
        ("family-domination", lambda d: [row.__setitem__("authorGeneratorFamily", "dominant-generator") for row in d["epochs"][active_index]["cases"][:30]], True),
        ("fresh-weight-loss", lambda d: [row.__setitem__("newlyPrecommitted", False) for row in d["epochs"][active_index]["cases"]], True),
        ("zero-weight", lambda d: d["epochs"][active_index]["cases"][0].__setitem__("rankingWeightUnits", 0), True),
        ("duplicate-lineage", lambda d: d["epochs"][active_index]["cases"][1].__setitem__("lineageId", d["epochs"][active_index]["cases"][0]["lineageId"]), True),
        ("commitment-reuse", lambda d: d["epochs"][active_index]["cases"][1].__setitem__("commitmentSha256", d["epochs"][active_index]["cases"][0]["commitmentSha256"]), True),
        ("oracle-leak", lambda d: d["epochs"][active_index]["cases"][0].__setitem__("oracleExposure", "public"), True),
        ("overlay-omission", lambda d: d["epochs"][active_index].__setitem__("overlays", []), True),
        ("benchmark-only-overlay", lambda d: d["epochs"][active_index]["overlays"][0].__setitem__("benchmarkOnly", True), True),
        ("retired-overlay", lambda d: d["epochs"][active_index]["overlays"][0].__setitem__("maintenanceStatus", "retired"), True),
        ("overlay-lineage-drift", lambda d: d["epochs"][active_index]["overlays"][0]["challengeLineageIds"].pop(), True),
        ("overlay-authority-drift", lambda d: d["epochs"][active_index]["overlays"][0].__setitem__("authoritySha256", "0" * 64), True),
        ("temporal-rule-weakened", lambda d: d["epochs"][active_index]["precommitment"].__setitem__("modelReleaseRule", "model-release-may-follow-precommit"), True),
        ("snapshot-drift", lambda d: d["epochs"][active_index].__setitem__("commitmentSnapshotIdentity", "0" * 64), False),
        ("custody-access", lambda d: d["epochs"][active_index]["custody"].__setitem__("trainingBuilderCanReadActivePack", True), True),
        ("public-private-material", lambda d: d["epochs"][active_index]["custody"].__setitem__("publicArtifactsContainPrivatePayloads", True), True),
        ("active-disclosure", lambda d: d["disclosures"].append({"id": "bad", "epochId": d["epochs"][active_index]["id"], "caseId": d["epochs"][active_index]["cases"][0]["id"], "disclosedOn": "2026-08-15", "reason": "scheduled-release", "payloadPath": "docs/program/held-out-disclosures/bad.json", "saltHex": "0" * 64, "verifiedCommitmentSha256": d["epochs"][active_index]["cases"][0]["commitmentSha256"]}), True),
        ("host-path", lambda d: d["epochs"][active_index]["custody"].__setitem__("privateRoot", "/Users/example/private"), True),
        ("identity-drift", lambda d: d.__setitem__("contentIdentitySha256", "0" * 64), False),
    ]
    for name, mutate, resign in controls:
        candidate = copy.deepcopy(document)
        mutate(candidate)
        if resign:
            resign_document(candidate)
        try:
            validate(candidate)
        except (HeldOutError, KeyError, ZeroDivisionError):
            continue
        raise HeldOutError(f"negative control accepted: {name}")
    return len(controls)


def main() -> int:
    parser = argparse.ArgumentParser()
    parser.add_argument("--check", action="store_true")
    parser.add_argument("--self-test", action="store_true")
    parser.add_argument("--verify-private", type=Path)
    parser.add_argument("--audit-out", type=Path)
    args = parser.parse_args()
    require(args.check or args.verify_private is not None, "select --check or --verify-private")
    require(args.audit_out is None or args.verify_private is not None, "--audit-out requires --verify-private")
    validate_schema()
    document = validate(load_json(MANIFEST))
    private_result = None
    if args.verify_private is not None:
        private_result = verify_private(document, args.verify_private)
        if args.audit_out is not None:
            write_audit(private_result, args.audit_out)
    controls = self_test(document) if args.self_test else 0
    audit = validate_audit(document) if AUDIT.is_file() else None
    active = next(row for row in document["epochs"] if row["status"] == "active")
    print(
        "gc-held-out-evaluation: ok "
        f"(epochs={len(document['epochs'])} active={active['id']} "
        f"lineages={len(active['cases'])} controls={controls} "
        f"private_verified={private_result is not None} audit_verified={audit is not None} "
        f"identity={document['contentIdentitySha256']})"
    )
    return 0


if __name__ == "__main__":
    try:
        raise SystemExit(main())
    except (HeldOutError, FileNotFoundError, json.JSONDecodeError, OSError) as exc:
        print(f"gc-held-out-evaluation: {exc}", file=sys.stderr)
        raise SystemExit(1)
