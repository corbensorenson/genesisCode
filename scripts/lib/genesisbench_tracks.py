#!/usr/bin/env python3
"""Normative GenesisBench track, scaffold, hardware, and cohort authority."""

from __future__ import annotations

import copy
import hashlib
import json
import re
from pathlib import Path
from typing import Any


SHA256_RE = re.compile(r"^[0-9a-f]{64}$")
ID_RE = re.compile(r"^[A-Za-z0-9][A-Za-z0-9._-]{0,127}$")
ROOT = Path(__file__).resolve().parents[2]
REFERENCE_AGENT = ROOT / "docs/spec/GENESISBENCH_REFERENCE_AGENT_v0.1.json"


TRACK_IDS = [
    "cold-acquisition", "embedded-local", "genesis-adapted", "open-agent",
]
HARDWARE_CLASSES = [
    {"id": "embedded-s", "maxCombinedResidentBytes": 4 * 1024**3},
    {"id": "embedded-m", "maxCombinedResidentBytes": 16 * 1024**3},
    {"id": "embedded-l", "maxCombinedResidentBytes": 64 * 1024**3},
]
COHORT_KEYS = [
    "attempt-policy-identity", "contamination-label", "context-mode",
    "hardware-class", "interaction-mode", "language-profile-artifact",
    "protocol-identity", "scaffold-identity", "task-epoch", "task-visibility",
    "track",
]

TRACK_POLICY = {
    "kind": "genesis/genesisbench-track-policy-v0.1",
    "version": "0.1.0",
    "crossTrackRankingAllowed": False,
    "comparisonRule": "same-track-and-content-addressed-cohort-only",
    "cohortIdentityAlgorithm": "sha256-canonical-json-v0.1",
    "cohortKeys": COHORT_KEYS,
    "tracks": [
        {
            "id": "cold-acquisition",
            "purpose": "headline-model-comparison",
            "genesisSpecificTraining": ["none"],
            "scaffoldClasses": ["fixed-reference"],
            "inferenceModes": ["local-offline", "remote-disclosed"],
            "rankedAdmissionOpen": True,
        },
        {
            "id": "embedded-local",
            "purpose": "self-hosted-local-appliance",
            "genesisSpecificTraining": ["declared-public", "none", "unknown"],
            "scaffoldClasses": ["disclosed-custom", "fixed-reference"],
            "inferenceModes": ["local-offline"],
            "rankedAdmissionOpen": True,
        },
        {
            "id": "genesis-adapted",
            "purpose": "lineage-audited-public-adaptation",
            "genesisSpecificTraining": ["declared-public"],
            "scaffoldClasses": ["disclosed-custom", "fixed-reference"],
            "inferenceModes": ["local-offline", "remote-disclosed"],
            "rankedAdmissionOpen": True,
        },
        {
            "id": "open-agent",
            "purpose": "disclosed-retrieval-and-orchestration",
            "genesisSpecificTraining": ["none", "unknown"],
            "scaffoldClasses": ["disclosed-custom", "fixed-reference"],
            "inferenceModes": ["local-offline", "remote-disclosed"],
            "rankedAdmissionOpen": True,
        },
    ],
    "hardwareClasses": HARDWARE_CLASSES,
    "hardwareMembershipRule": "smallest-class-whose-bound-covers-combined-resident-bytes",
    "hardwareFootprintBasis": "peak-combined-model-and-runtime-resident-bytes",
    "hardwareEvidenceRequiredForRanking": True,
    "adaptationManifestSchemaAuthorityId": "genesisbench-adaptation-manifest-schema",
    "hardwareEvidenceSchemaAuthorityId": "genesisbench-hardware-evidence-schema",
    "scaffoldManifestSchemaAuthorityId": "genesisbench-scaffold-manifest-schema",
    "fixedReferenceProfileAuthorityId": "genesisbench-reference-agent",
    "fixedReferenceProfileSchemaAuthorityId": "genesisbench-reference-agent-schema",
    "fixedReferenceAblationAuthorityId": "genesisbench-reference-agent-ablations",
    "fixedReferenceTraceSchemaAuthorityId": "genesisbench-reference-agent-trace-schema",
    "resourceCeilingsSharedAcrossTracks": True,
    "rawScaffoldedAdaptedAndLocalAggregatesMayMix": False,
}


class TrackError(ValueError):
    pass


def require(condition: bool, message: str) -> None:
    if not condition:
        raise TrackError(message)


def canonical_bytes(value: Any) -> bytes:
    return (
        json.dumps(value, sort_keys=True, separators=(",", ":"), ensure_ascii=True)
        + "\n"
    ).encode("ascii")


def sha256_object(value: Any) -> str:
    return hashlib.sha256(canonical_bytes(value)).hexdigest()


def object_identity(value: dict[str, Any]) -> str:
    material = copy.deepcopy(value)
    material["contentIdentitySha256"] = ""
    return sha256_object(material)


def closed(value: Any, keys: set[str], label: str) -> dict[str, Any]:
    require(isinstance(value, dict) and set(value) == keys, f"{label} fields are not closed")
    return value


def validate_track_policy(policy: Any) -> dict[str, Any]:
    require(policy == TRACK_POLICY, "track policy drift")
    return policy


def validate_adaptation_manifest(document: Any) -> dict[str, Any]:
    doc = closed(
        document,
        {"kind", "version", "publiclyRetrievable", "licenseAuditComplete", "lineageAuditComplete", "materials", "contentIdentitySha256"},
        "adaptation manifest",
    )
    require(doc["kind"] == "genesis/genesisbench-adaptation-manifest-v0.1" and doc["version"] == "0.1.0", "adaptation manifest version drift")
    require(doc["publiclyRetrievable"] is True and doc["licenseAuditComplete"] is True and doc["lineageAuditComplete"] is True, "adaptation manifest audit incomplete")
    require(isinstance(doc["materials"], list) and 1 <= len(doc["materials"]) <= 65536, "invalid adaptation materials")
    ids: list[str] = []
    for row in doc["materials"]:
        closed(row, {"id", "publicUri", "sha256", "license", "role"}, "adaptation material")
        ids.append(row["id"])
        require(isinstance(row["id"], str) and ID_RE.fullmatch(row["id"]) is not None, "invalid adaptation material id")
        require(isinstance(row["publicUri"], str) and row["publicUri"].startswith("https://") and not any(ch.isspace() for ch in row["publicUri"]), "adaptation material is not publicly addressable")
        require(isinstance(row["sha256"], str) and SHA256_RE.fullmatch(row["sha256"]) is not None, "invalid adaptation material identity")
        require(isinstance(row["license"], str) and re.fullmatch(r"[A-Za-z0-9.+-]{1,128}", row["license"]) is not None, "invalid adaptation material license")
        require(row["role"] in {"evaluation-exclusion", "instruction", "pretraining", "reinforcement", "synthetic-seed", "tool-trajectory"}, "invalid adaptation material role")
    require(ids == sorted(set(ids)), "adaptation materials must be sorted and unique")
    require(doc["contentIdentitySha256"] == object_identity(doc), "adaptation manifest identity drift")
    return doc


def validate_hardware_evidence(document: Any, hardware: dict[str, Any]) -> dict[str, Any]:
    doc = closed(
        document,
        {"kind", "version", "measurementMethod", "metric", "modelResidentBytes", "runtimeResidentBytes", "combinedResidentBytes", "measurementTool", "startedAt", "endedAt", "contentIdentitySha256"},
        "hardware evidence",
    )
    require(doc["kind"] == "genesis/genesisbench-hardware-evidence-v0.1" and doc["version"] == "0.1.0", "hardware evidence version drift")
    require(doc["measurementMethod"] in {"measured-peak-rss", "enforced-hard-ceiling"} and doc["measurementMethod"] == hardware["measurementMethod"], "hardware measurement method drift")
    require(doc["metric"] == "peak-combined-model-and-runtime-resident-bytes", "hardware evidence metric drift")
    for key in ("modelResidentBytes", "runtimeResidentBytes", "combinedResidentBytes"):
        require(isinstance(doc[key], int) and 0 <= doc[key] <= 64 * 1024**3, "invalid hardware resident bytes")
    require(doc["combinedResidentBytes"] == doc["modelResidentBytes"] + doc["runtimeResidentBytes"] == hardware["combinedResidentBytes"], "hardware resident-byte accounting drift")
    require(isinstance(doc["measurementTool"], str) and ID_RE.fullmatch(doc["measurementTool"]) is not None, "invalid hardware measurement tool")
    timestamp = re.compile(r"^20[0-9]{2}-[0-9]{2}-[0-9]{2}T[0-9]{2}:[0-9]{2}:[0-9]{2}Z$")
    require(timestamp.fullmatch(doc["startedAt"]) is not None and timestamp.fullmatch(doc["endedAt"]) is not None and doc["startedAt"] <= doc["endedAt"], "invalid hardware measurement interval")
    require(doc["contentIdentitySha256"] == object_identity(doc), "hardware evidence identity drift")
    return doc


def expected_scaffold_manifest(run: dict[str, Any]) -> dict[str, Any]:
    prompt = run["invocation"]["promptAssembly"]
    system_rows = [row for row in prompt["assemblyOrder"] if row["role"] == "system-policy"]
    require(len(system_rows) == 1, "scaffold requires one system policy")
    has_retrieval = any(row["role"] == "retrieval-transcript" for row in prompt["assemblyOrder"])
    return {
        "kind": "genesis/genesisbench-scaffold-manifest-v0.1",
        "version": "0.1.0",
        "class": run["track"]["scaffold"]["class"],
        "systemPromptArtifact": system_rows[0]["artifact"],
        "promptAssemblyAlgorithm": prompt["algorithm"],
        "retrieval": {
            "mode": "logged-exact-artifact" if has_retrieval else "none",
            "algorithmId": "recorded-retrieval-v0.1" if has_retrieval else None,
            "configIdentitySha256": prompt["identitySha256"] if has_retrieval else None,
            "completeTranscriptRequired": True,
        },
        "orchestration": {
            "agentCount": 1, "subagentsAllowed": False,
            "plannerId": None, "repairLoopId": None,
            "stopPolicyId": "first-complete-response",
        },
        "tool": {
            "catalogArtifact": run["toolProtocol"]["catalogArtifact"],
            "protocolId": run["toolProtocol"]["protocolId"],
            "operations": run["toolProtocol"]["operations"],
        },
        "externalAuthorityPolicy": "protocol-and-case-exact-no-broadening",
        "resourceCeilingPolicy": "protocol-and-case-exact-no-broadening",
        "contentIdentitySha256": "",
    }


def validate_scaffold_manifest(document: Any, run: dict[str, Any]) -> dict[str, Any]:
    scaffold = run["track"]["scaffold"]
    if scaffold["class"] == "fixed-reference":
        require(scaffold["id"] == "genesisbench-reference-agent-v0.1", "fixed reference scaffold id drift")
        expected = json.loads(REFERENCE_AGENT.read_text(encoding="utf-8"))
        require(document == expected, "fixed reference profile binding drift")
        require(document["contentIdentitySha256"] == object_identity(document), "fixed reference profile identity drift")
        return document
    expected = expected_scaffold_manifest(run)
    expected["contentIdentitySha256"] = object_identity(expected)
    require(document == expected, "scaffold manifest binding drift")
    return document


def artifact_hashes(run: dict[str, Any]) -> dict[str, str]:
    return {row["key"]: row["sha256"] for row in run["artifactInventory"]}


def expected_hardware_class(combined_resident_bytes: int) -> str | None:
    for row in HARDWARE_CLASSES:
        if combined_resident_bytes <= row["maxCombinedResidentBytes"]:
            return row["id"]
    return None


def validate_track_declaration(track: Any, run: dict[str, Any]) -> dict[str, Any]:
    doc = closed(
        track,
        {"trackId", "scaffold", "training", "inference", "hardware", "contentIdentitySha256"},
        "benchmark track",
    )
    require(doc["trackId"] in TRACK_IDS, "unknown benchmark track")
    scaffold = closed(doc["scaffold"], {"class", "id", "manifestArtifact", "identitySha256"}, "track scaffold")
    require(scaffold["class"] in {"disclosed-custom", "fixed-reference"}, "invalid scaffold class")
    require(isinstance(scaffold["id"], str) and ID_RE.fullmatch(scaffold["id"]) is not None, "invalid scaffold id")
    require(scaffold["manifestArtifact"] in artifact_hashes(run), "missing scaffold manifest artifact")
    require(scaffold["identitySha256"] == artifact_hashes(run)[scaffold["manifestArtifact"]], "track scaffold binding drift")
    training = closed(
        doc["training"], {"genesisSpecificTraining", "manifestArtifact", "manifestIdentitySha256"},
        "track training",
    )
    require(training["genesisSpecificTraining"] in {"declared-public", "none", "unknown"}, "invalid training declaration")
    require(
        training["manifestIdentitySha256"] is None
        or (isinstance(training["manifestIdentitySha256"], str) and SHA256_RE.fullmatch(training["manifestIdentitySha256"]) is not None),
        "invalid training manifest identity",
    )
    require(training["manifestArtifact"] is None or isinstance(training["manifestArtifact"], str) and training["manifestArtifact"] in artifact_hashes(run), "invalid training manifest artifact")
    require((training["manifestArtifact"] is None) == (training["manifestIdentitySha256"] is None), "incomplete training manifest binding")
    if training["manifestArtifact"] is not None:
        require(artifact_hashes(run)[training["manifestArtifact"]] == training["manifestIdentitySha256"], "training manifest artifact identity drift")
    inference = closed(doc["inference"], {"mode", "networkMode"}, "track inference")
    require(inference["mode"] in {"local-offline", "remote-disclosed"}, "invalid inference mode")
    require(inference["networkMode"] in {"deny", "provider-only"}, "invalid inference network mode")
    hardware = closed(
        doc["hardware"],
        {"classId", "combinedResidentBytes", "evidenceArtifact", "evidenceIdentitySha256", "measurementMethod"},
        "track hardware",
    )
    require(hardware["classId"] is None or hardware["classId"] in {row["id"] for row in HARDWARE_CLASSES}, "invalid hardware class")
    require(hardware["combinedResidentBytes"] is None or isinstance(hardware["combinedResidentBytes"], int) and hardware["combinedResidentBytes"] >= 0, "invalid resident footprint")
    require(hardware["evidenceIdentitySha256"] is None or isinstance(hardware["evidenceIdentitySha256"], str) and SHA256_RE.fullmatch(hardware["evidenceIdentitySha256"]) is not None, "invalid hardware evidence identity")
    require(hardware["evidenceArtifact"] is None or isinstance(hardware["evidenceArtifact"], str) and hardware["evidenceArtifact"] in artifact_hashes(run), "invalid hardware evidence artifact")
    require((hardware["evidenceArtifact"] is None) == (hardware["evidenceIdentitySha256"] is None), "incomplete hardware evidence binding")
    if hardware["evidenceArtifact"] is not None:
        require(artifact_hashes(run)[hardware["evidenceArtifact"]] == hardware["evidenceIdentitySha256"], "hardware evidence artifact identity drift")
    require(hardware["measurementMethod"] in {"measured-peak-rss", "enforced-hard-ceiling", "not-claimed"}, "invalid hardware measurement method")
    require(doc["contentIdentitySha256"] == object_identity(doc), "track declaration identity drift")
    return doc


def classify_track(track: dict[str, Any], run: dict[str, Any]) -> tuple[list[str], list[str]]:
    policy = next(row for row in TRACK_POLICY["tracks"] if row["id"] == track["trackId"])
    unranked: list[str] = []
    invalid: list[str] = []
    training = track["training"]
    inference = track["inference"]
    hardware = track["hardware"]
    if track["scaffold"]["class"] not in policy["scaffoldClasses"]:
        invalid.append("track/scaffold-mismatch")
    if track["trackId"] == "cold-acquisition" and (
        track["scaffold"]["class"] != "fixed-reference"
        or track["scaffold"]["id"] != "genesisbench-reference-agent-v0.1"
    ):
        invalid.append("track/scaffold-mismatch")
    if training["genesisSpecificTraining"] not in policy["genesisSpecificTraining"]:
        invalid.append("track/declaration-mismatch")
    if training["genesisSpecificTraining"] == "unknown":
        unranked.append("track/training-provenance-incomplete")
    if training["genesisSpecificTraining"] == "declared-public" and training["manifestIdentitySha256"] is None:
        invalid.append("track/declaration-mismatch")
    if inference["mode"] not in policy["inferenceModes"]:
        invalid.append("track/declaration-mismatch")
    if track["trackId"] == "embedded-local":
        if inference != {"mode": "local-offline", "networkMode": "deny"} or run["model"]["providerKind"] != "local" or run["host"]["environment"]["networkMode"] != "deny":
            invalid.append("track/offline-violation")
        complete = all(hardware[key] is not None for key in ("classId", "combinedResidentBytes", "evidenceArtifact", "evidenceIdentitySha256")) and hardware["measurementMethod"] != "not-claimed"
        if not complete:
            unranked.append("track/hardware-evidence-incomplete")
        elif expected_hardware_class(hardware["combinedResidentBytes"]) != hardware["classId"]:
            invalid.append("track/hardware-class-mismatch")
    elif any(hardware[key] is not None for key in ("classId", "combinedResidentBytes", "evidenceArtifact", "evidenceIdentitySha256")) or hardware["measurementMethod"] != "not-claimed":
        invalid.append("track/hardware-class-mismatch")
    if not policy["rankedAdmissionOpen"]:
        unranked.append("track/admission-not-open")
    return sorted(set(unranked)), sorted(set(invalid))


def build_cohort(
    profile: dict[str, Any], run: dict[str, Any], *, context_mode: str,
    interaction_mode: str, visibility: str, contamination_label: str,
) -> dict[str, Any]:
    authorities = {row["id"]: row for row in profile["authorities"]}
    attempt_material = {
        "profileAttemptPolicy": profile["attemptPolicy"],
        "runRetryPolicy": run["invocation"]["retryPolicy"],
    }
    cohort = {
        "kind": "genesis/genesisbench-cohort-v0.1",
        "version": "0.1.0",
        "trackId": run["track"]["trackId"],
        "protocolIdentitySha256": profile["contentIdentitySha256"],
        "languageProfileArtifactSha256": authorities["agent-profile"]["sha256"],
        "scaffoldIdentitySha256": run["track"]["scaffold"]["identitySha256"],
        "taskEpochId": run["benchmark"]["heldOutEpochId"],
        "contextMode": context_mode,
        "interactionMode": interaction_mode,
        "attemptPolicyIdentitySha256": sha256_object(attempt_material),
        "hardwareClassId": run["track"]["hardware"]["classId"],
        "contaminationLabel": contamination_label,
        "taskVisibilityClass": visibility,
        "contentIdentitySha256": "",
    }
    cohort["contentIdentitySha256"] = object_identity(cohort)
    return cohort


def cohort_id(cohort: dict[str, Any]) -> str:
    return f"genesisbench-cohort-v0.1/{cohort['contentIdentitySha256']}"


def self_test(run: dict[str, Any]) -> int:
    """Prove track predicates reject or downgrade representative overclaims."""
    controls = 0

    def candidate(track_id: str) -> dict[str, Any]:
        value = copy.deepcopy(run["track"])
        value["trackId"] = track_id
        return value

    cold = candidate("cold-acquisition")
    cold["training"]["genesisSpecificTraining"] = "none"
    cold_unranked, cold_invalid = classify_track(cold, run)
    require("track/admission-not-open" not in cold_unranked, "cold admission did not open")
    require("track/scaffold-mismatch" in cold_invalid, "cold scaffold control failed")
    controls += 2

    fixed_cold = copy.deepcopy(cold)
    fixed_cold["scaffold"]["class"] = "fixed-reference"
    fixed_cold["scaffold"]["id"] = "genesisbench-reference-agent-v0.1"
    require(classify_track(fixed_cold, run) == ([], []), "fixed cold-acquisition control failed")
    controls += 1

    embedded = candidate("embedded-local")
    embedded_unranked, embedded_invalid = classify_track(embedded, run)
    require("track/hardware-evidence-incomplete" in embedded_unranked and not embedded_invalid, "embedded evidence control failed")
    controls += 1

    networked = copy.deepcopy(embedded)
    networked["inference"]["networkMode"] = "provider-only"
    require("track/offline-violation" in classify_track(networked, run)[1], "embedded offline control failed")
    controls += 1

    misclassified = copy.deepcopy(embedded)
    misclassified["hardware"] = {
        "classId": "embedded-l", "combinedResidentBytes": 1024**3,
        "evidenceArtifact": "bundle:models/weights.fixture",
        "evidenceIdentitySha256": "1" * 64,
        "measurementMethod": "measured-peak-rss",
    }
    require("track/hardware-class-mismatch" in classify_track(misclassified, run)[1], "hardware class control failed")
    controls += 1

    adapted = candidate("genesis-adapted")
    require("track/declaration-mismatch" in classify_track(adapted, run)[1], "adaptation lineage control failed")
    adapted["training"]["genesisSpecificTraining"] = "declared-public"
    require("track/declaration-mismatch" in classify_track(adapted, run)[1], "adaptation manifest control failed")
    controls += 2

    adapted["training"]["manifestArtifact"] = "bundle:models/weights.fixture"
    adapted["training"]["manifestIdentitySha256"] = artifact_hashes(run)["bundle:models/weights.fixture"]
    require(classify_track(adapted, run) == ([], []), "valid adaptation control failed")
    controls += 1

    hardware_on_open = candidate("open-agent")
    hardware_on_open["hardware"] = misclassified["hardware"]
    require("track/hardware-class-mismatch" in classify_track(hardware_on_open, run)[1], "cross-track hardware control failed")
    controls += 1

    rebound = copy.deepcopy(run["track"])
    rebound["scaffold"]["identitySha256"] = "0" * 64
    rebound["contentIdentitySha256"] = object_identity(rebound)
    try:
        validate_track_declaration(rebound, run)
    except TrackError:
        controls += 1
    else:
        raise TrackError("scaffold rebinding control failed")

    scaffold_manifest = expected_scaffold_manifest(run)
    scaffold_manifest["contentIdentitySha256"] = object_identity(scaffold_manifest)
    validate_scaffold_manifest(scaffold_manifest, run)
    controls += 1
    hidden_subagent = copy.deepcopy(scaffold_manifest)
    hidden_subagent["orchestration"]["agentCount"] = 2
    hidden_subagent["orchestration"]["subagentsAllowed"] = True
    hidden_subagent["contentIdentitySha256"] = object_identity(hidden_subagent)
    try:
        validate_scaffold_manifest(hidden_subagent, run)
    except TrackError:
        controls += 1
    else:
        raise TrackError("hidden subagent control failed")

    manifest = {
        "kind": "genesis/genesisbench-adaptation-manifest-v0.1",
        "version": "0.1.0", "publiclyRetrievable": True,
        "licenseAuditComplete": True, "lineageAuditComplete": True,
        "materials": [{
            "id": "public-seed", "publicUri": "https://example.invalid/seed.jsonl",
            "sha256": "3" * 64, "license": "CC0-1.0", "role": "synthetic-seed",
        }],
        "contentIdentitySha256": "",
    }
    manifest["contentIdentitySha256"] = object_identity(manifest)
    validate_adaptation_manifest(manifest)
    controls += 1
    false_public = copy.deepcopy(manifest)
    false_public["publiclyRetrievable"] = False
    false_public["contentIdentitySha256"] = object_identity(false_public)
    try:
        validate_adaptation_manifest(false_public)
    except TrackError:
        controls += 1
    else:
        raise TrackError("adaptation public-material control failed")

    hardware = {
        "kind": "genesis/genesisbench-hardware-evidence-v0.1",
        "version": "0.1.0", "measurementMethod": "measured-peak-rss",
        "metric": "peak-combined-model-and-runtime-resident-bytes",
        "modelResidentBytes": 3 * 1024**3, "runtimeResidentBytes": 512 * 1024**2,
        "combinedResidentBytes": 7 * 512 * 1024**2,
        "measurementTool": "genesisbench-rss-sampler-v0.1",
        "startedAt": "2026-07-15T00:00:00Z", "endedAt": "2026-07-15T00:01:00Z",
        "contentIdentitySha256": "",
    }
    hardware["contentIdentitySha256"] = object_identity(hardware)
    hardware_binding = {"measurementMethod": "measured-peak-rss", "combinedResidentBytes": hardware["combinedResidentBytes"]}
    validate_hardware_evidence(hardware, hardware_binding)
    controls += 1
    wrong_sum = copy.deepcopy(hardware)
    wrong_sum["combinedResidentBytes"] += 1
    wrong_sum["contentIdentitySha256"] = object_identity(wrong_sum)
    try:
        validate_hardware_evidence(wrong_sum, {**hardware_binding, "combinedResidentBytes": wrong_sum["combinedResidentBytes"]})
    except TrackError:
        controls += 1
    else:
        raise TrackError("hardware accounting control failed")
    return controls
