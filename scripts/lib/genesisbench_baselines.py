#!/usr/bin/env python3
"""Predeclare, validate, and publish reproducible GenesisBench model baselines."""

from __future__ import annotations

import argparse
import copy
import hashlib
import json
import re
import sys
import tempfile
from pathlib import Path
from typing import Any, Callable

import genesisbench_front_door as front_door
import genesisbench_registry as registry


ROOT = Path(__file__).resolve().parents[2]
SUITE_PATH = ROOT / "benchmarks/agent_tasks/v0.1/suite.json"
PROTOCOL_PATH = ROOT / "docs/spec/GENESISBENCH_BASELINE_PROTOCOL_v0.1.json"
PROTOCOL_SCHEMA_PATH = ROOT / "docs/spec/GENESISBENCH_BASELINE_PROTOCOL_v0.1.schema.json"
PREDECLARATION_SCHEMA_PATH = ROOT / "docs/spec/GENESISBENCH_BASELINE_PREDECLARATION_v0.1.schema.json"
EVIDENCE_SCHEMA_PATH = ROOT / "docs/spec/GENESISBENCH_BASELINE_EVIDENCE_v0.1.schema.json"
PUBLICATION_SCHEMA_PATH = ROOT / "docs/spec/GENESISBENCH_BASELINE_PUBLICATION_v0.1.schema.json"
CARD_PATH = ROOT / "docs/spec/GENESISBENCH_BENCHMARK_CARD_v0.1.json"
FAILURES_PATH = ROOT / "docs/spec/GENESISBENCH_FAILURE_TAXONOMY_v0.1.json"
FIXTURE_EVIDENCE_PATH = ROOT / "benchmarks/genesisbench/v0.1/baselines/conformance/evidence.fixture.json"
FIXTURE_REPORT_PATH = ROOT / "benchmarks/genesisbench/v0.1/baselines/conformance/publication.fixture.json"

SHA_RE = re.compile(r"^[0-9a-f]{64}$")
ID_RE = re.compile(r"^[a-z0-9][a-z0-9._-]{0,127}$")
UTC_RE = re.compile(r"^20[0-9]{2}-[01][0-9]-[0-3][0-9]T[0-2][0-9]:[0-5][0-9]:[0-5][0-9]Z$")
SYSTEM_CLASSES = [
    "code-specialized", "frontier-general", "open-agent", "open-weight", "small-local",
]
OUTCOMES = ["abstained", "invalid", "missing", "solved", "unsolved"]
FAILURE_CODES = [
    "agent/abstained", "adapter/cancelled", "adapter/failed", "adapter/timed-out",
    "artifact/invalid", "authority/broadened", "budget/exhausted", "candidate/incomplete",
    "candidate/invalid", "capability/requested", "harness/invalid", "model/refusal",
    "obligation/failed", "policy/excess", "replay/mismatch", "resource/exceeded",
    "scope/escaped", "semantic/incorrect", "transport/invalid", "unknown/failure",
]
MUTABLE_ALIASES = {"auto", "current", "default", "latest", "stable"}


class BaselineError(ValueError):
    pass


def require(condition: bool, message: str) -> None:
    if not condition:
        raise BaselineError(message)


def unique_object(pairs: list[tuple[str, Any]]) -> dict[str, Any]:
    result: dict[str, Any] = {}
    for key, value in pairs:
        require(key not in result, f"duplicate JSON key: {key}")
        result[key] = value
    return result


def load(path: Path) -> Any:
    try:
        return json.loads(path.read_text(encoding="utf-8"), object_pairs_hook=unique_object)
    except (OSError, json.JSONDecodeError) as error:
        raise BaselineError(f"cannot load {path}: {error}") from error


def canonical(value: Any) -> bytes:
    return json.dumps(value, sort_keys=True, separators=(",", ":"), ensure_ascii=True).encode("ascii")


def identity(value: dict[str, Any]) -> str:
    candidate = copy.deepcopy(value)
    candidate["contentIdentitySha256"] = ""
    return hashlib.sha256(canonical(candidate)).hexdigest()


def seal(value: dict[str, Any]) -> dict[str, Any]:
    value["contentIdentitySha256"] = identity(value)
    return value


def closed(value: Any, fields: set[str], label: str) -> dict[str, Any]:
    require(isinstance(value, dict), f"{label} must be an object")
    require(set(value) == fields, f"{label} fields drift: missing={sorted(fields-set(value))} extra={sorted(set(value)-fields)}")
    return value


def valid_id(value: Any, label: str) -> str:
    require(isinstance(value, str) and ID_RE.fullmatch(value) is not None, f"invalid {label}")
    return value


def valid_sha(value: Any, label: str) -> str:
    require(isinstance(value, str) and SHA_RE.fullmatch(value) is not None, f"invalid {label}")
    return value


def validate_identity(value: dict[str, Any], label: str) -> None:
    valid_sha(value.get("contentIdentitySha256"), f"{label} identity")
    require(identity(value) == value["contentIdentitySha256"], f"{label} identity drift")


def suite() -> dict[str, Any]:
    return load(SUITE_PATH)


def authority_identity(path: str) -> str:
    value = load(ROOT / path)
    valid_sha(value.get("contentIdentitySha256"), path)
    return value["contentIdentitySha256"]


def expected_protocol() -> dict[str, Any]:
    tasks = suite()
    small = sorted(row["id"] for row in tasks["cases"] if row["contextTier"] == "small")
    all_cases = sorted(row["id"] for row in tasks["cases"])
    return seal({
        "kind": "genesis/genesisbench-baseline-protocol-v0.1",
        "version": "0.1.0",
        "status": "project-controlled-preview",
        "authorities": {
            "analysisPlanIdentitySha256": authority_identity("docs/spec/GENESISBENCH_ANALYSIS_PLAN_v0.1.json"),
            "constructValidityIdentitySha256": load(ROOT / "benchmarks/genesisbench/v0.1/construct-validity/report.json")["contentIdentitySha256"],
            "protocolIdentitySha256": authority_identity("docs/spec/GENESISBENCH_PROTOCOL_v0.1.json"),
            "referenceAgentIdentitySha256": authority_identity("docs/spec/GENESISBENCH_REFERENCE_AGENT_v0.1.json"),
            "scoringIdentitySha256": authority_identity("docs/spec/GC_AGENT_BENCHMARK_SCORING_v0.1.json"),
            "suiteIdentitySha256": tasks["contentIdentitySha256"],
        },
        "phases": {
            "realityGate": {
                "attemptsPerCell": 1,
                "caseIds": small,
                "immutableModelRequired": True,
                "minimumSystems": 1,
                "requiredSystemClass": "frontier-general",
                "sameScaffoldAcrossClasses": True,
                "syntheticEvidenceAllowed": False,
            },
            "publicMatrix": {
                "attemptsPerCell": 3,
                "caseIds": all_cases,
                "requiredSystemClasses": SYSTEM_CLASSES,
                "startRequiresPassedRealityGate": True,
                "syntheticEvidenceAllowed": False,
            },
        },
        "attemptPolicy": {
            "boundedPassK": 3,
            "hiddenRetriesAllowed": False,
            "passAtOneAttempt": 0,
            "preserveEveryAttempt": True,
            "selectionAfterOutcomeAllowed": False,
        },
        "analysis": {
            "clusterUnit": "lineageId",
            "costAndLatencyAffectRank": False,
            "missingness": "explicit-and-in-denominator",
            "primaryContextTier": "small",
            "teacherSelection": "per-task-class-verified-trajectory",
            "uncertainty": "lineage-cluster-bootstrap-and-wilson-95",
        },
        "custody": {
            "closedBundlesRequired": True,
            "failedAttemptsRetained": True,
            "providerSecretsRecorded": False,
            "rawRequestsAndResponsesRetained": True,
            "replayWithoutModelAccess": True,
        },
        "publication": {
            "artifacts": [
                "accepted-closed-bundles", "benchmark-card", "complete-attempt-table",
                "failure-taxonomy", "methods-paper", "model-cards", "reproduction-manifest",
            ],
            "economics": "reported-non-ranking",
            "invalidPartialQuality": "never-ranked",
            "nonClaimsRequired": True,
            "temporalCleanRequiresReleasePrecommitProof": True,
        },
        "contentIdentitySha256": "",
    })


def schema_header(identifier: str, title: str) -> dict[str, Any]:
    return {
        "$schema": "https://json-schema.org/draft/2020-12/schema",
        "$id": f"https://genesiscode.dev/schemas/{identifier}.json",
        "title": title,
        "type": "object",
        "additionalProperties": False,
    }


def common_defs() -> dict[str, Any]:
    return {
        "sha256": {"type": "string", "pattern": "^[0-9a-f]{64}$"},
        "id": {"type": "string", "pattern": "^[a-z0-9][a-z0-9._-]{0,127}$"},
        "utc": {"type": "string", "pattern": UTC_RE.pattern},
    }


def protocol_schema() -> dict[str, Any]:
    document = schema_header("genesisbench-baseline-protocol-v0.1", "GenesisBench baseline protocol v0.1")
    expected = expected_protocol()
    document.update({
        "required": sorted(expected),
        "properties": {key: {"const": value} for key, value in expected.items()},
    })
    return document


def predeclaration_schema() -> dict[str, Any]:
    document = schema_header("genesisbench-baseline-predeclaration-v0.1", "GenesisBench baseline predeclaration v0.1")
    system = {
        "type": "object", "additionalProperties": False,
        "required": ["id", "systemClass", "familyId", "providerId", "modelId", "revision", "immutable", "adapterClass", "adapterIdentitySha256", "trackId", "trainingProvenance", "releaseEvidenceIdentitySha256"],
        "properties": {
            "id": {"$ref": "#/$defs/id"}, "systemClass": {"enum": SYSTEM_CLASSES},
            "familyId": {"$ref": "#/$defs/id"}, "providerId": {"$ref": "#/$defs/id"},
            "modelId": {"$ref": "#/$defs/id"}, "revision": {"$ref": "#/$defs/id"},
            "immutable": {"const": True}, "adapterClass": {"enum": ["command-plugin", "direct-local-runtime", "hosted-api", "local-openai-compatible"]}, "adapterIdentitySha256": {"$ref": "#/$defs/sha256"},
            "trackId": {"enum": ["cold-acquisition", "embedded-local", "genesis-adapted", "open-agent"]},
            "trainingProvenance": {"enum": ["declared-public", "none", "unknown"]},
            "releaseEvidenceIdentitySha256": {"anyOf": [{"$ref": "#/$defs/sha256"}, {"type": "null"}]},
        },
    }
    document.update({
        "required": ["kind", "version", "studyId", "lockedAtUtc", "protocolIdentitySha256", "phase", "realityGatePublicationIdentitySha256", "systems", "stopBudget", "operator", "nonClaims", "contentIdentitySha256"],
        "properties": {
            "kind": {"const": "genesis/genesisbench-baseline-predeclaration-v0.1"}, "version": {"const": "0.1.0"},
            "studyId": {"$ref": "#/$defs/id"}, "lockedAtUtc": {"$ref": "#/$defs/utc"},
            "protocolIdentitySha256": {"$ref": "#/$defs/sha256"}, "phase": {"enum": ["reality-gate", "public-matrix"]},
            "realityGatePublicationIdentitySha256": {"anyOf": [{"$ref": "#/$defs/sha256"}, {"type": "null"}]},
            "systems": {"type": "array", "minItems": 1, "maxItems": 64, "items": system},
            "stopBudget": {"type": "object", "additionalProperties": False, "required": ["maximumModelCalls", "maximumCostMicrounits", "currency", "maximumWallTimeMs", "stopOnBudgetExhaustion"], "properties": {"maximumModelCalls": {"type": "integer", "minimum": 1}, "maximumCostMicrounits": {"type": "integer", "minimum": 0}, "currency": {"type": "string", "pattern": "^[A-Z]{3}$"}, "maximumWallTimeMs": {"type": "integer", "minimum": 1}, "stopOnBudgetExhaustion": {"const": True}}},
            "operator": {"type": "object", "additionalProperties": False, "required": ["id", "custodyStatement", "conflicts"], "properties": {"id": {"$ref": "#/$defs/id"}, "custodyStatement": {"type": "string", "minLength": 1, "maxLength": 2048}, "conflicts": {"type": "array", "items": {"type": "string", "maxLength": 512}, "uniqueItems": True}}},
            "nonClaims": {"type": "array", "minItems": 1, "items": {"type": "string", "minLength": 1, "maxLength": 512}, "uniqueItems": True},
            "contentIdentitySha256": {"$ref": "#/$defs/sha256"},
        }, "$defs": common_defs(),
    })
    return document


def evidence_schema() -> dict[str, Any]:
    document = schema_header("genesisbench-baseline-evidence-v0.1", "GenesisBench baseline evidence v0.1")
    row = {
        "type": "object", "additionalProperties": False,
        "required": ["systemId", "caseId", "attempt", "evidenceClass", "bundlePath", "bundleSha256", "runIdentitySha256", "outcome", "qualityScoreBasisPoints", "failureCodes", "inputTokens", "outputTokens", "latencyMs", "costMicrounits", "capabilityRequests", "resourceFacts"],
        "properties": {
            "systemId": {"$ref": "#/$defs/id"}, "caseId": {"$ref": "#/$defs/id"},
            "attempt": {"type": "integer", "minimum": 0, "maximum": 15},
            "evidenceClass": {"enum": ["authentic-closed-bundle", "declared-missing", "synthetic-conformance"]},
            "bundlePath": {"anyOf": [{"type": "string", "pattern": "^[^/].*\\.gcbundle$"}, {"type": "null"}]},
            "bundleSha256": {"anyOf": [{"$ref": "#/$defs/sha256"}, {"type": "null"}]},
            "runIdentitySha256": {"anyOf": [{"$ref": "#/$defs/sha256"}, {"type": "null"}]},
            "outcome": {"enum": OUTCOMES}, "qualityScoreBasisPoints": {"anyOf": [{"type": "integer", "minimum": 0, "maximum": 10000}, {"type": "null"}]},
            "failureCodes": {"type": "array", "items": {"enum": FAILURE_CODES}, "uniqueItems": True},
            "inputTokens": {"anyOf": [{"type": "integer", "minimum": 0}, {"type": "null"}]},
            "outputTokens": {"anyOf": [{"type": "integer", "minimum": 0}, {"type": "null"}]},
            "latencyMs": {"anyOf": [{"type": "integer", "minimum": 0}, {"type": "null"}]},
            "costMicrounits": {"anyOf": [{"type": "integer", "minimum": 0}, {"type": "null"}]},
            "capabilityRequests": {"type": "array", "items": {"type": "string", "maxLength": 256}, "uniqueItems": True},
            "resourceFacts": {"type": "object", "additionalProperties": {"type": ["integer", "string", "boolean", "null"]}},
        },
    }
    document.update({
        "required": ["kind", "version", "predeclarationIdentitySha256", "publicationMode", "attempts", "contentIdentitySha256"],
        "properties": {
            "kind": {"const": "genesis/genesisbench-baseline-evidence-v0.1"}, "version": {"const": "0.1.0"},
            "predeclarationIdentitySha256": {"$ref": "#/$defs/sha256"},
            "publicationMode": {"enum": ["authentic-study", "synthetic-conformance"]},
            "attempts": {"type": "array", "minItems": 1, "items": row},
            "contentIdentitySha256": {"$ref": "#/$defs/sha256"},
        }, "$defs": common_defs(),
    })
    return document


def publication_schema() -> dict[str, Any]:
    document = schema_header("genesisbench-baseline-publication-v0.1", "GenesisBench baseline publication v0.1")
    summary_properties = {
        "independentLineages": {"type": "integer", "minimum": 1}, "conditionCells": {"type": "integer", "minimum": 1},
        "attempts": {"type": "integer", "minimum": 1}, "clusteredBy": {"const": "lineageId"},
        "passAtOneSolved": {"type": "integer", "minimum": 0}, "passAtOneBasisPoints": {"type": "integer", "minimum": 0, "maximum": 10000},
        "boundedPassAtK": {"type": "integer", "minimum": 1, "maximum": 3}, "boundedPassAtKSolved": {"type": "integer", "minimum": 0},
        "boundedPassAtKBasisPoints": {"type": "integer", "minimum": 0, "maximum": 10000}, "conditionalQualityDenominator": {"type": "integer", "minimum": 0},
        "conditionalQualityMeanBasisPoints": {"anyOf": [{"type": "integer", "minimum": 0, "maximum": 10000}, {"type": "null"}]},
        "systemId": {"$ref": "#/$defs/id"},
    }
    system_summary = {"type": "object", "additionalProperties": False, "required": sorted(summary_properties), "properties": summary_properties}
    class_properties = {**summary_properties, "taskClass": {"$ref": "#/$defs/id"}}
    class_summary = {"type": "object", "additionalProperties": False, "required": sorted(class_properties), "properties": class_properties}
    document.update({
        "required": ["kind", "version", "predeclarationIdentitySha256", "evidenceIdentitySha256", "publicationMode", "realityGate", "systemSummaries", "taskClassSummaries", "teacherCandidates", "failureCounts", "economics", "nonClaims", "contentIdentitySha256"],
        "properties": {
            "kind": {"const": "genesis/genesisbench-baseline-publication-v0.1"}, "version": {"const": "0.1.0"},
            "predeclarationIdentitySha256": {"$ref": "#/$defs/sha256"}, "evidenceIdentitySha256": {"$ref": "#/$defs/sha256"},
            "publicationMode": {"enum": ["authentic-study", "synthetic-conformance"]},
            "realityGate": {"type": "object", "additionalProperties": False, "required": ["passed", "authentic", "requiredCases", "observedCases", "mockCanSatisfy", "expansionAuthorized"], "properties": {"passed": {"type": "boolean"}, "authentic": {"type": "boolean"}, "requiredCases": {"const": 9}, "observedCases": {"type": "integer", "minimum": 0, "maximum": 9}, "mockCanSatisfy": {"const": False}, "expansionAuthorized": {"type": "boolean"}}},
            "systemSummaries": {"type": "array", "minItems": 1, "items": system_summary},
            "taskClassSummaries": {"type": "array", "minItems": 1, "items": class_summary},
            "teacherCandidates": {"type": "array", "minItems": 9, "maxItems": 9, "items": {"type": "object", "additionalProperties": False, "required": ["taskClass", "trajectories", "selection"], "properties": {"taskClass": {"$ref": "#/$defs/id"}, "trajectories": {"type": "array", "items": {"type": "object", "additionalProperties": False, "required": ["systemId", "caseId", "attempt", "bundleSha256", "runIdentitySha256", "qualityScoreBasisPoints"], "properties": {"systemId": {"$ref": "#/$defs/id"}, "caseId": {"$ref": "#/$defs/id"}, "attempt": {"type": "integer", "minimum": 0}, "bundleSha256": {"$ref": "#/$defs/sha256"}, "runIdentitySha256": {"$ref": "#/$defs/sha256"}, "qualityScoreBasisPoints": {"type": "integer", "minimum": 0, "maximum": 10000}}}}, "selection": {"const": "verified-trajectory-per-class-not-aggregate-rank"}}}},
            "failureCounts": {"type": "object", "additionalProperties": False, "required": FAILURE_CODES, "properties": {code: {"type": "integer", "minimum": 0} for code in FAILURE_CODES}},
            "economics": {"type": "object", "additionalProperties": False, "required": ["currency", "totalCostMicrounits", "totalLatencyMs", "affectsRank"], "properties": {"currency": {"type": "string", "pattern": "^[A-Z]{3}$"}, "totalCostMicrounits": {"type": "integer", "minimum": 0}, "totalLatencyMs": {"type": "integer", "minimum": 0}, "affectsRank": {"const": False}}},
            "nonClaims": {"type": "array", "items": {"type": "string"}},
            "contentIdentitySha256": {"$ref": "#/$defs/sha256"},
        }, "$defs": common_defs(),
    })
    return document


def benchmark_card(protocol: dict[str, Any]) -> dict[str, Any]:
    return seal({
        "kind": "genesis/genesisbench-benchmark-card-v0.1", "version": "0.1.0",
        "name": "GenesisBench", "status": protocol["status"],
        "protocolIdentitySha256": protocol["contentIdentitySha256"],
        "purpose": "Measure how reliably an agent learns and engineers GenesisCode under frozen authority and bounded resources.",
        "unitOfIndependence": "task-lineage",
        "coreTaskClasses": sorted({row["taskClass"] for row in suite()["cases"]}),
        "primaryMetrics": ["verified-pass-at-one", "bounded-pass-at-k", "conditional-quality-among-valid-solves"],
        "safetyMetrics": ["authority-excess", "capability-requests", "invalid-attempts", "policy-scope", "resource-excess"],
        "requiredDisclosures": ["adapter", "all-attempts", "contamination", "cost", "hardware", "model-revision", "runtime", "scaffold"],
        "nonClaims": [
            "Public cases are not held-out or temporal-clean.",
            "Benchmark score is not a general intelligence measure.",
            "Cost and latency do not improve substantive rank.",
            "A deterministic mock is conformance evidence only.",
            "Unknown training provenance is not clean provenance.",
        ],
        "contentIdentitySha256": "",
    })


def failure_taxonomy(protocol: dict[str, Any]) -> dict[str, Any]:
    groups: dict[str, list[str]] = {}
    for code in FAILURE_CODES:
        groups.setdefault(code.split("/", 1)[0], []).append(code)
    return seal({
        "kind": "genesis/genesisbench-failure-taxonomy-v0.1", "version": "0.1.0",
        "protocolIdentitySha256": protocol["contentIdentitySha256"],
        "closedCodes": FAILURE_CODES, "groups": groups,
        "rules": {"solvedHasNoFailureCodes": True, "nonSolvedHasAtLeastOneFailureCode": True, "unknownMapsOnlyToUnknownFailure": True},
        "contentIdentitySha256": "",
    })


def validate_predeclaration(
    value: Any,
    protocol: dict[str, Any],
    *,
    reality_gate_publication: dict[str, Any] | None = None,
) -> dict[str, Any]:
    fields = {"kind", "version", "studyId", "lockedAtUtc", "protocolIdentitySha256", "phase", "realityGatePublicationIdentitySha256", "systems", "stopBudget", "operator", "nonClaims", "contentIdentitySha256"}
    doc = closed(value, fields, "predeclaration")
    require(doc["kind"] == "genesis/genesisbench-baseline-predeclaration-v0.1" and doc["version"] == "0.1.0", "predeclaration version drift")
    valid_id(doc["studyId"], "study id")
    require(isinstance(doc["lockedAtUtc"], str) and UTC_RE.fullmatch(doc["lockedAtUtc"]) is not None, "invalid lock timestamp")
    require(doc["protocolIdentitySha256"] == protocol["contentIdentitySha256"], "predeclaration protocol drift")
    require(doc["phase"] in {"reality-gate", "public-matrix"}, "invalid study phase")
    systems = doc["systems"]
    require(isinstance(systems, list) and systems, "predeclaration requires systems")
    ids: list[str] = []
    for row in systems:
        closed(row, {"id", "systemClass", "familyId", "providerId", "modelId", "revision", "immutable", "adapterClass", "adapterIdentitySha256", "trackId", "trainingProvenance", "releaseEvidenceIdentitySha256"}, "system")
        ids.append(valid_id(row["id"], "system id"))
        require(row["systemClass"] in SYSTEM_CLASSES and row["immutable"] is True, "system is mutable or unclassified")
        for key in ("familyId", "providerId", "modelId", "revision"):
            valid_id(row[key], f"system {key}")
        require(row["modelId"] not in MUTABLE_ALIASES and row["revision"] not in MUTABLE_ALIASES, "mutable model alias is forbidden")
        require(row["adapterClass"] in {"command-plugin", "direct-local-runtime", "hosted-api", "local-openai-compatible"}, "mock or unknown adapters cannot enter a real-model study")
        valid_sha(row["adapterIdentitySha256"], "adapter identity")
        require(row["trackId"] in {"cold-acquisition", "embedded-local", "genesis-adapted", "open-agent"}, "invalid benchmark track")
        require(row["trainingProvenance"] in {"declared-public", "none", "unknown"}, "invalid training provenance")
        if row["releaseEvidenceIdentitySha256"] is not None:
            valid_sha(row["releaseEvidenceIdentitySha256"], "release evidence")
    require(ids == sorted(set(ids)), "systems must be sorted and unique")
    classes = {row["systemClass"] for row in systems}
    if doc["phase"] == "reality-gate":
        require(classes == {"frontier-general"} and len(systems) == 1, "reality gate requires exactly one frontier-general system")
        require(doc["realityGatePublicationIdentitySha256"] is None, "reality gate cannot cite itself as prior evidence")
    else:
        require(set(SYSTEM_CLASSES) <= classes, "public matrix omits a required system class")
        valid_sha(doc["realityGatePublicationIdentitySha256"], "prior reality-gate publication")
        require(reality_gate_publication is not None, "public matrix validation requires --reality-gate-publication")
        validate_identity(reality_gate_publication, "prior reality-gate publication")
        require(reality_gate_publication["contentIdentitySha256"] == doc["realityGatePublicationIdentitySha256"], "public matrix cites a different reality-gate publication")
        gate = reality_gate_publication.get("realityGate")
        require(isinstance(gate, dict) and gate.get("passed") is True and gate.get("authentic") is True and gate.get("expansionAuthorized") is True, "prior reality gate did not authorize expansion")
    budget = closed(doc["stopBudget"], {"maximumModelCalls", "maximumCostMicrounits", "currency", "maximumWallTimeMs", "stopOnBudgetExhaustion"}, "stop budget")
    phase = protocol["phases"]["realityGate" if doc["phase"] == "reality-gate" else "publicMatrix"]
    required_calls = len(phase["caseIds"]) * phase["attemptsPerCell"] * len(systems)
    require(budget["maximumModelCalls"] == required_calls, "model-call stop budget must equal the complete declared matrix")
    require(isinstance(budget["maximumCostMicrounits"], int) and not isinstance(budget["maximumCostMicrounits"], bool) and budget["maximumCostMicrounits"] >= 0 and budget["stopOnBudgetExhaustion"] is True, "invalid financial stop budget")
    require(isinstance(budget["currency"], str) and re.fullmatch(r"[A-Z]{3}", budget["currency"]) is not None, "invalid budget currency")
    require(isinstance(budget["maximumWallTimeMs"], int) and not isinstance(budget["maximumWallTimeMs"], bool) and budget["maximumWallTimeMs"] > 0, "invalid wall-time stop budget")
    operator = closed(doc["operator"], {"id", "custodyStatement", "conflicts"}, "operator")
    valid_id(operator["id"], "operator id")
    require(isinstance(operator["custodyStatement"], str) and 1 <= len(operator["custodyStatement"]) <= 2048, "invalid custody statement")
    require(isinstance(operator["conflicts"], list) and operator["conflicts"] == sorted(set(operator["conflicts"])) and all(isinstance(item, str) and len(item) <= 512 for item in operator["conflicts"]), "conflicts must be sorted, unique strings")
    require(isinstance(doc["nonClaims"], list) and doc["nonClaims"] == sorted(set(doc["nonClaims"])) and doc["nonClaims"], "non-claims must be sorted, unique, and non-empty")
    validate_identity(doc, "predeclaration")
    return doc


def expected_cells(predeclaration: dict[str, Any], protocol: dict[str, Any]) -> list[tuple[str, str, int]]:
    phase = protocol["phases"]["realityGate" if predeclaration["phase"] == "reality-gate" else "publicMatrix"]
    return sorted((system["id"], case, attempt) for system in predeclaration["systems"] for case in phase["caseIds"] for attempt in range(phase["attemptsPerCell"]))


def verify_bundle_row(
    row: dict[str, Any], system: dict[str, Any], bundle_root: Path,
    genesis_bin: Path, selfhost_artifact: Path,
) -> None:
    try:
        relative = Path(row["bundlePath"])
        require(not relative.is_absolute() and ".." not in relative.parts, "bundle path must be root-relative")
        root = bundle_root.resolve(strict=True)
        candidate = root / relative
        require(not candidate.is_symlink(), "bundle cannot be a symlink")
        bundle = candidate.resolve(strict=True)
        require(bundle.is_relative_to(root) and bundle.is_file(), "bundle escapes the declared evidence root")
        manifest, digest = front_door.validate_bundle(bundle)
        require(digest == row["bundleSha256"] and manifest["runIdentitySha256"] == row["runIdentitySha256"], "bundle identity disagrees with evidence")
        with tempfile.TemporaryDirectory(prefix="genesisbench-baseline-bundle-") as temporary:
            extracted = Path(temporary) / "run"
            registry.extract_bundle(bundle, extracted)
            run = front_door.validate_run(extracted / "run.json", check_files=True)
            adapter = front_door.validate_adapter(front_door.load_json(extracted / "adapter.json"))
            require(run["case"]["id"] == row["caseId"], "bundle case disagrees with evidence")
            require(adapter["contentIdentitySha256"] == system["adapterIdentitySha256"], "bundle adapter disagrees with predeclared system")
            require(adapter["class"] == system["adapterClass"] and adapter["class"] != "deterministic-mock", "bundle adapter class disagrees with real-model predeclaration")
            require(adapter["model"]["id"] == system["modelId"] and adapter["model"]["revision"] == system["revision"] and adapter["model"]["immutable"] is True, "bundle model disagrees with predeclaration")
            score = front_door.load_json(extracted / "score.json") if run["scoreIdentitySha256"] else None
            derived_outcome = {"verified": "solved", "failed": "unsolved", "invalid": "invalid", "abstained": "abstained"}[run["outcome"]]
            require(row["outcome"] == derived_outcome, "bundle outcome disagrees with evidence")
            expected_quality = score["qualityScoreBasisPoints"] if derived_outcome == "solved" else None
            require(row["qualityScoreBasisPoints"] == expected_quality, "bundle quality disagrees with evidence")
            responses = [front_door.load_json(extracted / attempt["responseArtifact"]) for attempt in run["attempts"]]
            token_values = [(response["usage"].get("inputTokens"), response["usage"].get("outputTokens")) for response in responses if response["status"] == "succeeded"]
            expected_input = sum(value[0] for value in token_values) if token_values else None
            expected_output = sum(value[1] for value in token_values) if token_values else None
            require(row["inputTokens"] == expected_input and row["outputTokens"] == expected_output, "bundle token facts disagree with evidence")
            require(row["latencyMs"] == sum(attempt["elapsedMs"] for attempt in run["attempts"]), "bundle latency disagrees with evidence")
            replay = front_door.replay_run(extracted / "run.json", genesis_bin, selfhost_artifact)
            require(replay["adapterInvoked"] is False and replay["modelAccessed"] is False and replay["allFieldsValidated"] is True and (run["scoreIdentitySha256"] is None or replay["independentRescoreMatched"] is True), "bundle replay or independent rescore failed")
    except BaselineError:
        raise
    except (OSError, KeyError, TypeError, ValueError, front_door.FrontDoorError, registry.RegistryError) as error:
        raise BaselineError(f"bundle validation failed: {error}") from error


def validate_evidence(
    value: Any,
    predeclaration: dict[str, Any],
    protocol: dict[str, Any],
    *,
    bundle_root: Path | None = None,
    genesis_bin: Path | None = None,
    selfhost_artifact: Path | None = None,
) -> dict[str, Any]:
    doc = closed(value, {"kind", "version", "predeclarationIdentitySha256", "publicationMode", "attempts", "contentIdentitySha256"}, "evidence")
    require(doc["kind"] == "genesis/genesisbench-baseline-evidence-v0.1" and doc["version"] == "0.1.0", "evidence version drift")
    require(doc["predeclarationIdentitySha256"] == predeclaration["contentIdentitySha256"], "evidence predeclaration drift")
    require(doc["publicationMode"] in {"authentic-study", "synthetic-conformance"}, "invalid publication mode")
    rows = doc["attempts"]
    require(isinstance(rows, list), "attempts must be an array")
    keys: list[tuple[str, str, int]] = []
    case_ids = {row["id"] for row in suite()["cases"]}
    systems = {row["id"]: row for row in predeclaration["systems"]}
    for row in rows:
        fields = {"systemId", "caseId", "attempt", "evidenceClass", "bundlePath", "bundleSha256", "runIdentitySha256", "outcome", "qualityScoreBasisPoints", "failureCodes", "inputTokens", "outputTokens", "latencyMs", "costMicrounits", "capabilityRequests", "resourceFacts"}
        closed(row, fields, "attempt")
        require(row["systemId"] in systems, "attempt has unknown system")
        require(isinstance(row["attempt"], int) and not isinstance(row["attempt"], bool) and 0 <= row["attempt"] <= 15, "invalid attempt index")
        keys.append((row["systemId"], row["caseId"], row["attempt"]))
        require(row["caseId"] in case_ids and row["outcome"] in OUTCOMES, "attempt has unknown case or outcome")
        require(row["failureCodes"] == sorted(set(row["failureCodes"])) and set(row["failureCodes"]) <= set(FAILURE_CODES), "invalid failure taxonomy")
        require((row["outcome"] == "solved") == (isinstance(row["qualityScoreBasisPoints"], int) and not isinstance(row["qualityScoreBasisPoints"], bool)), "quality exists exactly for solved attempts")
        require((row["outcome"] == "solved" and row["failureCodes"] == []) or (row["outcome"] != "solved" and row["failureCodes"]), "failure codes disagree with outcome")
        for key in ("inputTokens", "outputTokens", "latencyMs", "costMicrounits"):
            require(row[key] is None or isinstance(row[key], int) and not isinstance(row[key], bool) and row[key] >= 0, f"invalid {key}")
        require(isinstance(row["capabilityRequests"], list) and row["capabilityRequests"] == sorted(set(row["capabilityRequests"])) and all(isinstance(item, str) and len(item) <= 256 for item in row["capabilityRequests"]), "invalid capability requests")
        require(isinstance(row["resourceFacts"], dict) and all(isinstance(key, str) and isinstance(value, (str, int, bool, type(None))) for key, value in row["resourceFacts"].items()), "invalid resource facts")
        if row["evidenceClass"] == "authentic-closed-bundle":
            require(doc["publicationMode"] == "authentic-study" and all(row[key] is not None for key in ("bundlePath", "bundleSha256", "runIdentitySha256")), "authentic evidence requires a closed bundle")
            require(bundle_root is not None and genesis_bin is not None and selfhost_artifact is not None, "authentic evidence validation requires --bundle-root, --genesis-bin, and --selfhost-artifact")
            verify_bundle_row(row, systems[row["systemId"]], bundle_root, genesis_bin, selfhost_artifact)
        elif row["evidenceClass"] == "declared-missing":
            require(doc["publicationMode"] == "authentic-study" and row["outcome"] == "missing", "declared-missing is authentic-study missingness only")
            require(all(row[key] is None for key in ("bundlePath", "bundleSha256", "runIdentitySha256", "inputTokens", "outputTokens", "latencyMs", "costMicrounits")), "missing evidence cannot invent run facts")
        else:
            require(doc["publicationMode"] == "synthetic-conformance" and all(row[key] is None for key in ("bundlePath", "bundleSha256", "runIdentitySha256")), "synthetic evidence cannot claim bundle provenance")
    require(keys == expected_cells(predeclaration, protocol), "evidence is incomplete, duplicated, or noncanonical")
    require(sum((row["costMicrounits"] or 0) for row in rows) <= predeclaration["stopBudget"]["maximumCostMicrounits"], "evidence exceeds predeclared cost budget")
    validate_identity(doc, "evidence")
    return doc


def basis_points(numerator: int, denominator: int) -> int:
    return (numerator * 10_000 + denominator // 2) // denominator


def summarize_rows(rows: list[dict[str, Any]], k: int, cases: dict[str, dict[str, Any]]) -> dict[str, Any]:
    by_case: dict[str, list[dict[str, Any]]] = {}
    for row in rows:
        by_case.setdefault(row["caseId"], []).append(row)
    case_ids = sorted(by_case)
    observed_k = min(k, max(len(rows) for rows in by_case.values()))
    pass1 = sum(by_case[case][0]["outcome"] == "solved" for case in case_ids)
    passk = sum(any(row["outcome"] == "solved" for row in by_case[case][:observed_k]) for case in case_ids)
    solved_rows = [row for row in rows if row["outcome"] == "solved"]
    return {
        "independentLineages": len({cases[case]["lineageId"] for case in by_case}),
        "conditionCells": len(by_case), "attempts": len(rows), "clusteredBy": "lineageId",
        "passAtOneSolved": pass1, "passAtOneBasisPoints": basis_points(pass1, len(case_ids)),
        "boundedPassAtK": observed_k, "boundedPassAtKSolved": passk, "boundedPassAtKBasisPoints": basis_points(passk, len(case_ids)),
        "conditionalQualityDenominator": len(solved_rows),
        "conditionalQualityMeanBasisPoints": None if not solved_rows else sum(row["qualityScoreBasisPoints"] for row in solved_rows) // len(solved_rows),
    }


def build_publication(predeclaration: dict[str, Any], evidence: dict[str, Any], protocol: dict[str, Any]) -> dict[str, Any]:
    cases = {row["id"]: row for row in suite()["cases"]}
    systems = [row["id"] for row in predeclaration["systems"]]
    attempts = evidence["attempts"]
    k = protocol["attemptPolicy"]["boundedPassK"]
    system_summaries = []
    class_summaries = []
    for system in systems:
        primary = [row for row in attempts if row["systemId"] == system and cases[row["caseId"]]["contextTier"] == protocol["analysis"]["primaryContextTier"]]
        summary = summarize_rows(primary, k, cases)
        summary["systemId"] = system
        system_summaries.append(summary)
        for task_class in sorted({case["taskClass"] for case in cases.values()}):
            selected = [row for row in attempts if row["systemId"] == system and cases[row["caseId"]]["taskClass"] == task_class]
            if selected:
                item = summarize_rows(selected, k, cases)
                item.update({"systemId": system, "taskClass": task_class})
                class_summaries.append(item)
    teachers = []
    for task_class in sorted({row["taskClass"] for row in cases.values()}):
        trajectories = [
            {
                "systemId": row["systemId"], "caseId": row["caseId"], "attempt": row["attempt"],
                "bundleSha256": row["bundleSha256"], "runIdentitySha256": row["runIdentitySha256"],
                "qualityScoreBasisPoints": row["qualityScoreBasisPoints"],
            }
            for row in attempts
            if cases[row["caseId"]]["taskClass"] == task_class
            and row["outcome"] == "solved"
            and row["evidenceClass"] == "authentic-closed-bundle"
        ]
        trajectories.sort(key=lambda row: (-row["qualityScoreBasisPoints"], row["systemId"], row["caseId"], row["attempt"]))
        teachers.append({"taskClass": task_class, "trajectories": trajectories, "selection": "verified-trajectory-per-class-not-aggregate-rank"})
    authentic = evidence["publicationMode"] == "authentic-study"
    reality_cases = set(protocol["phases"]["realityGate"]["caseIds"])
    frontier_ids = {row["id"] for row in predeclaration["systems"] if row["systemClass"] == "frontier-general"}
    gate_rows = [row for row in attempts if row["systemId"] in frontier_ids and row["caseId"] in reality_cases and row["attempt"] == 0]
    gate_passed = authentic and predeclaration["phase"] == "reality-gate" and len(gate_rows) == len(reality_cases) and all(row["evidenceClass"] == "authentic-closed-bundle" for row in gate_rows)
    failures = {code: sum(code in row["failureCodes"] for row in attempts) for code in FAILURE_CODES}
    return seal({
        "kind": "genesis/genesisbench-baseline-publication-v0.1", "version": "0.1.0",
        "predeclarationIdentitySha256": predeclaration["contentIdentitySha256"],
        "evidenceIdentitySha256": evidence["contentIdentitySha256"], "publicationMode": evidence["publicationMode"],
        "realityGate": {"passed": gate_passed, "authentic": authentic, "requiredCases": len(reality_cases), "observedCases": len(gate_rows), "mockCanSatisfy": False, "expansionAuthorized": gate_passed},
        "systemSummaries": system_summaries, "taskClassSummaries": class_summaries,
        "teacherCandidates": teachers, "failureCounts": failures,
        "economics": {"currency": predeclaration["stopBudget"]["currency"], "totalCostMicrounits": sum((row["costMicrounits"] or 0) for row in attempts), "totalLatencyMs": sum((row["latencyMs"] or 0) for row in attempts), "affectsRank": False},
        "nonClaims": predeclaration["nonClaims"], "contentIdentitySha256": "",
    })


def validate_publication(value: Any, predeclaration: dict[str, Any], evidence: dict[str, Any], protocol: dict[str, Any]) -> dict[str, Any]:
    expected = build_publication(predeclaration, evidence, protocol)
    require(value == expected, "publication differs from deterministic derivation")
    validate_identity(value, "publication")
    return value


def conformance_documents(protocol: dict[str, Any]) -> tuple[dict[str, Any], dict[str, Any], dict[str, Any]]:
    system = {"id": "synthetic-frontier", "systemClass": "frontier-general", "familyId": "synthetic-family", "providerId": "synthetic-provider", "modelId": "synthetic-model", "revision": "fixture-v0.1", "immutable": True, "adapterClass": "hosted-api", "adapterIdentitySha256": "1" * 64, "trackId": "cold-acquisition", "trainingProvenance": "unknown", "releaseEvidenceIdentitySha256": None}
    pre = seal({"kind": "genesis/genesisbench-baseline-predeclaration-v0.1", "version": "0.1.0", "studyId": "synthetic-conformance", "lockedAtUtc": "2026-01-01T00:00:00Z", "protocolIdentitySha256": protocol["contentIdentitySha256"], "phase": "reality-gate", "realityGatePublicationIdentitySha256": None, "systems": [system], "stopBudget": {"maximumModelCalls": 9, "maximumCostMicrounits": 0, "currency": "USD", "maximumWallTimeMs": 5400000, "stopOnBudgetExhaustion": True}, "operator": {"id": "fixture-operator", "custodyStatement": "Synthetic validator conformance only; no model was invoked.", "conflicts": []}, "nonClaims": ["No model capability is measured.", "No rank or reality-gate claim is allowed.", "No temporal-clean claim is made."], "contentIdentitySha256": ""})
    rows = []
    for index, (_, case, attempt) in enumerate(expected_cells(pre, protocol)):
        solved = index % 3 == 0
        rows.append({"systemId": system["id"], "caseId": case, "attempt": attempt, "evidenceClass": "synthetic-conformance", "bundlePath": None, "bundleSha256": None, "runIdentitySha256": None, "outcome": "solved" if solved else "unsolved", "qualityScoreBasisPoints": 9000 - index if solved else None, "failureCodes": [] if solved else ["semantic/incorrect"], "inputTokens": 1000 + index, "outputTokens": 100 + index, "latencyMs": index, "costMicrounits": 0, "capabilityRequests": [], "resourceFacts": {"synthetic": True}})
    evidence = seal({"kind": "genesis/genesisbench-baseline-evidence-v0.1", "version": "0.1.0", "predeclarationIdentitySha256": pre["contentIdentitySha256"], "publicationMode": "synthetic-conformance", "attempts": rows, "contentIdentitySha256": ""})
    return pre, evidence, build_publication(pre, evidence, protocol)


def validate_static() -> tuple[dict[str, Any], dict[str, Any], dict[str, Any]]:
    protocol = expected_protocol()
    require(load(PROTOCOL_PATH) == protocol, "baseline protocol drift; run --write")
    require(load(PROTOCOL_SCHEMA_PATH) == protocol_schema(), "baseline protocol schema drift; run --write")
    require(load(PREDECLARATION_SCHEMA_PATH) == predeclaration_schema(), "baseline predeclaration schema drift; run --write")
    require(load(EVIDENCE_SCHEMA_PATH) == evidence_schema(), "baseline evidence schema drift; run --write")
    require(load(PUBLICATION_SCHEMA_PATH) == publication_schema(), "baseline publication schema drift; run --write")
    require(load(CARD_PATH) == benchmark_card(protocol), "benchmark card drift; run --write")
    require(load(FAILURES_PATH) == failure_taxonomy(protocol), "failure taxonomy drift; run --write")
    pre, evidence, report = conformance_documents(protocol)
    validate_predeclaration(pre, protocol)
    validate_evidence(evidence, pre, protocol)
    require(load(FIXTURE_EVIDENCE_PATH) == evidence, "baseline evidence fixture drift; run --write")
    require(load(FIXTURE_REPORT_PATH) == report, "baseline publication fixture drift; run --write")
    validate_publication(report, pre, evidence, protocol)
    return pre, evidence, report


def self_test(protocol: dict[str, Any], pre: dict[str, Any], evidence: dict[str, Any], report: dict[str, Any]) -> int:
    controls: list[tuple[str, str, Callable[[dict[str, Any]], None]]] = [
        ("pre", "mutable-model", lambda d: d["systems"][0].__setitem__("immutable", False)),
        ("pre", "wrong-class", lambda d: d["systems"][0].__setitem__("systemClass", "small-local")),
        ("pre", "call-budget", lambda d: d["stopBudget"].__setitem__("maximumModelCalls", 10)),
        ("pre", "stale-protocol", lambda d: d.__setitem__("protocolIdentitySha256", "0" * 64)),
        ("evidence", "missing-attempt", lambda d: d["attempts"].pop()),
        ("evidence", "duplicate-attempt", lambda d: d["attempts"].append(copy.deepcopy(d["attempts"][0]))),
        ("evidence", "mock-bundle-claim", lambda d: d["attempts"][0].__setitem__("bundleSha256", "0" * 64)),
        ("evidence", "silent-failure", lambda d: d["attempts"][1].__setitem__("failureCodes", [])),
        ("evidence", "quality-on-failure", lambda d: d["attempts"][1].__setitem__("qualityScoreBasisPoints", 1)),
        ("evidence", "unknown-failure", lambda d: d["attempts"][1].__setitem__("failureCodes", ["provider/magic"])),
        ("report", "mock-passes-gate", lambda d: d["realityGate"].__setitem__("passed", True)),
        ("report", "cost-ranks", lambda d: d["economics"].__setitem__("affectsRank", True)),
        ("report", "teacher-aggregate", lambda d: d["teacherCandidates"][0].__setitem__("selection", "aggregate-rank")),
        ("report", "pass-inflation", lambda d: d["systemSummaries"][0].__setitem__("passAtOneSolved", 9)),
    ]
    passed = 0
    for target, name, mutate in controls:
        candidate = copy.deepcopy({"pre": pre, "evidence": evidence, "report": report}[target])
        mutate(candidate)
        candidate["contentIdentitySha256"] = identity(candidate)
        try:
            if target == "pre": validate_predeclaration(candidate, protocol)
            elif target == "evidence": validate_evidence(candidate, pre, protocol)
            else: validate_publication(candidate, pre, evidence, protocol)
        except BaselineError:
            passed += 1
        else:
            raise BaselineError(f"negative control accepted: {name}")
    return passed


def write_all() -> None:
    protocol = expected_protocol()
    pre, evidence, report = conformance_documents(protocol)
    outputs = {
        PROTOCOL_PATH: protocol, PROTOCOL_SCHEMA_PATH: protocol_schema(),
        PREDECLARATION_SCHEMA_PATH: predeclaration_schema(), EVIDENCE_SCHEMA_PATH: evidence_schema(),
        PUBLICATION_SCHEMA_PATH: publication_schema(), CARD_PATH: benchmark_card(protocol),
        FAILURES_PATH: failure_taxonomy(protocol), FIXTURE_EVIDENCE_PATH: evidence,
        FIXTURE_REPORT_PATH: report,
    }
    for path, value in outputs.items():
        path.parent.mkdir(parents=True, exist_ok=True)
        path.write_text(json.dumps(value, indent=2, sort_keys=True) + "\n", encoding="utf-8")


def main() -> int:
    parser = argparse.ArgumentParser()
    parser.add_argument("--check", action="store_true")
    parser.add_argument("--self-test", action="store_true")
    parser.add_argument("--write", action="store_true")
    parser.add_argument("--predeclaration", type=Path)
    parser.add_argument("--evidence", type=Path)
    parser.add_argument("--publication", type=Path)
    parser.add_argument("--bundle-root", type=Path)
    parser.add_argument("--genesis-bin", type=Path)
    parser.add_argument("--selfhost-artifact", type=Path)
    parser.add_argument("--reality-gate-publication", type=Path)
    parser.add_argument("--render-publication", action="store_true")
    args = parser.parse_args()
    if args.write:
        write_all()
    protocol = expected_protocol()
    if args.predeclaration:
        prior_gate = load(args.reality_gate_publication) if args.reality_gate_publication else None
        pre = validate_predeclaration(load(args.predeclaration), protocol, reality_gate_publication=prior_gate)
        if args.evidence:
            evidence = validate_evidence(load(args.evidence), pre, protocol, bundle_root=args.bundle_root, genesis_bin=args.genesis_bin, selfhost_artifact=args.selfhost_artifact)
            report = build_publication(pre, evidence, protocol)
            if args.publication:
                validate_publication(load(args.publication), pre, evidence, protocol)
            if args.render_publication:
                json.dump(report, sys.stdout, indent=2, sort_keys=True); sys.stdout.write("\n")
        return 0
    pre, evidence, report = validate_static()
    controls = self_test(protocol, pre, evidence, report) if args.self_test else 0
    print(f"genesisbench-baselines: ok (controls={controls} reality_gate={str(report['realityGate']['passed']).lower()} mode={report['publicationMode']})")
    return 0


if __name__ == "__main__":
    try:
        raise SystemExit(main())
    except BaselineError as error:
        print(f"genesisbench-baselines: {error}", file=sys.stderr)
        raise SystemExit(1)
