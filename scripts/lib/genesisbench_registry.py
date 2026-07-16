#!/usr/bin/env python3
"""Signed, append-only GenesisBench result registry and static leaderboard."""

from __future__ import annotations

import argparse
import base64
import copy
import hashlib
import json
import os
import shutil
import subprocess
import sys
import tarfile
import tempfile
from decimal import Decimal, ROUND_HALF_EVEN
from pathlib import Path
from typing import Any, Callable

try:
    import fcntl
except ImportError:  # Windows uses msvcrt for the same one-byte advisory lock.
    fcntl = None  # type: ignore[assignment]
    import msvcrt

import genesisbench_analysis
import genesisbench_front_door as front_door


ROOT = Path(__file__).resolve().parents[2]
PROFILE_PATH = ROOT / "docs/spec/GENESISBENCH_PROTOCOL_v0.1.json"
REFERENCE_PATH = ROOT / "docs/spec/GENESISBENCH_REFERENCE_AGENT_v0.1.json"
SUITE_PATH = ROOT / "benchmarks/agent_tasks/v0.1/suite.json"
AUTHORITY_PATH = ROOT / "docs/spec/GENESISBENCH_REGISTRY_v0.1.json"
CLAIM_SCHEMA_PATH = ROOT / "docs/spec/GENESISBENCH_SUBMISSION_CLAIM_v0.1.schema.json"
POLICY_SCHEMA_PATH = ROOT / "docs/spec/GENESISBENCH_REGISTRY_POLICY_v0.1.schema.json"
SUBMISSION_SCHEMA_PATH = ROOT / "docs/spec/GENESISBENCH_SIGNED_SUBMISSION_v0.1.schema.json"
RESULT_SCHEMA_PATH = ROOT / "docs/spec/GENESISBENCH_REGISTRY_RESULT_v0.1.schema.json"
EVENT_SCHEMA_PATH = ROOT / "docs/spec/GENESISBENCH_REGISTRY_EVENT_v0.1.schema.json"
CHECKPOINT_SCHEMA_PATH = ROOT / "docs/spec/GENESISBENCH_REGISTRY_CHECKPOINT_v0.1.schema.json"
PUBLICATION_SCHEMA_PATH = ROOT / "docs/spec/GENESISBENCH_LEADERBOARD_v0.1.schema.json"

SUBMISSION_PAYLOAD_TYPE = "application/vnd.genesiscode.genesisbench-submission.v0.1+json"
EVENT_PAYLOAD_TYPE = "application/vnd.genesiscode.genesisbench-registry-event.v0.1+json"
CHECKPOINT_PAYLOAD_TYPE = "application/vnd.genesiscode.genesisbench-registry-checkpoint.v0.1+json"
HASH = "0" * 64
MAX_JSON_BYTES = 16 * 1024 * 1024
MAX_BUNDLE_BYTES = 64 * 1024 * 1024
TRACKS = {"cold-acquisition", "embedded-local", "genesis-adapted", "open-agent"}
CONTAMINATION = {"declared-contaminated", "declared-uncontaminated", "temporal-clean", "unknown"}
OUTCOMES = {"verified", "failed", "invalid", "abstained"}


class RegistryError(RuntimeError):
    pass


def require(condition: bool, message: str) -> None:
    if not condition:
        raise RegistryError(message)


def unique_object(rows: list[tuple[str, Any]]) -> dict[str, Any]:
    out: dict[str, Any] = {}
    for key, value in rows:
        require(key not in out, f"duplicate JSON key: {key}")
        out[key] = value
    return out


def load_json(path: Path, label: str = "JSON") -> Any:
    metadata = path.lstat()
    require(path.is_file() and not path.is_symlink(), f"{label} must be a regular non-symlink file")
    require(metadata.st_size <= MAX_JSON_BYTES, f"{label} exceeds finite size limit")
    with path.open("r", encoding="ascii") as stream:
        return json.load(stream, object_pairs_hook=unique_object, parse_float=lambda _: (_ for _ in ()).throw(RegistryError(f"{label} forbids floating-point JSON")))


def load_stored_json(path: Path, label: str) -> Any:
    value = load_json(path, label)
    require(path.read_bytes() == pretty(value), f"{label} bytes are not the canonical immutable encoding")
    return value


def canonical(value: Any) -> bytes:
    return (json.dumps(value, sort_keys=True, separators=(",", ":"), ensure_ascii=True) + "\n").encode("ascii")


def pretty(value: Any) -> bytes:
    return (json.dumps(value, sort_keys=True, indent=2, ensure_ascii=True) + "\n").encode("ascii")


def sha256_bytes(value: bytes) -> str:
    return hashlib.sha256(value).hexdigest()


def sha256_file(path: Path) -> str:
    digest = hashlib.sha256()
    with path.open("rb") as stream:
        while chunk := stream.read(1024 * 1024):
            digest.update(chunk)
    return digest.hexdigest()


def identified(value: dict[str, Any], field: str = "contentIdentitySha256") -> dict[str, Any]:
    out = copy.deepcopy(value)
    out[field] = ""
    out[field] = sha256_bytes(canonical(out))
    return out


def validate_identity(value: dict[str, Any], field: str = "contentIdentitySha256") -> None:
    require(isinstance(value.get(field), str) and len(value[field]) == 64 and set(value[field]) <= set("0123456789abcdef"), f"invalid {field}")
    require(identified(value, field) == value, f"{field} mismatch")


def closed(value: Any, fields: set[str], label: str) -> dict[str, Any]:
    require(isinstance(value, dict) and set(value) == fields, f"{label} fields are not closed")
    return value


def bounded_id(value: Any, label: str) -> str:
    require(isinstance(value, str) and 1 <= len(value) <= 128 and value[0].isalnum() and all(ch.isalnum() or ch in "._-" for ch in value), f"invalid {label}")
    return value


def hash_value(value: Any, label: str, *, nullable: bool = False) -> str | None:
    if nullable and value is None:
        return None
    require(isinstance(value, str) and len(value) == 64 and set(value) <= set("0123456789abcdef"), f"invalid {label}")
    return value


def sorted_strings(values: Any, label: str, *, maximum: int = 256) -> list[str]:
    require(isinstance(values, list) and len(values) <= maximum and all(isinstance(row, str) and 1 <= len(row) <= 256 for row in values), f"invalid {label}")
    require(values == sorted(set(values)), f"{label} must be sorted and unique")
    return values


def validate_public_key(row: Any, label: str) -> dict[str, Any]:
    item = closed(row, {"id", "keyId", "publicKeyBase64", "provenance"}, label)
    bounded_id(item["id"], f"{label} id")
    require(isinstance(item["provenance"], str) and 1 <= len(item["provenance"]) <= 512, f"invalid {label} provenance")
    try:
        public = base64.b64decode(item["publicKeyBase64"], validate=True)
    except (ValueError, TypeError) as error:
        raise RegistryError(f"invalid {label} public key: {error}") from error
    require(len(public) == 32, f"{label} public key must contain 32 bytes")
    require(item["keyId"] == f"sha256:{sha256_bytes(public)}", f"{label} key identity mismatch")
    return item


def render_authority() -> dict[str, Any]:
    return identified({
        "kind": "genesis/genesisbench-registry-authority-v0.1",
        "version": "0.1.0",
        "registryId": "GenesisBench-Signed-Registry-v0.1",
        "cryptography": {
            "algorithm": "ed25519",
            "envelope": "dsse-v1",
            "canonicalPayload": "ascii-sorted-integer-json-newline-v0.1",
            "submitterAndOperatorKeysSeparated": True,
            "signatureGrantsScoreAuthority": False,
        },
        "admission": {
            "bundleValidation": "strict-all-fields-and-bytes",
            "independentRescore": "required-before-result-derivation",
            "adapterOrRunMaySelfRank": False,
            "invalidPartialQuality": 0,
            "closedTrackAndProfileRequired": True,
            "allAttemptsAndFailuresRetained": True,
        },
        "history": {
            "model": "single-writer-signed-hash-chain",
            "eventSequenceStartsAt": 1,
            "appendOnlyByResultIdentity": True,
            "deleteOrRewriteCommands": False,
            "everyPrefixCheckpointed": True,
            "silentSuppressionAllowed": False,
        },
        "ranking": {
            "cohortIsolation": "exact-content-addressed-cohort-only",
            "independentUnit": "lineage-id",
            "completeEvaluationSetRequired": True,
            "lexicographicKeys": [
                "verified-solve-rate-desc",
                "conditional-quality-desc",
                "capability-excess-asc",
                "context-bytes-asc",
                "tool-calls-asc",
                "repair-calls-asc",
            ],
            "costLatencyAffectRank": False,
            "tiesRemainTies": True,
        },
        "publication": {
            "format": "deterministic-static-json-html",
            "derivedOnlyFromSignedHistory": True,
            "perLineageAndClass": True,
            "invalidMissingAbstainedAndAttempts": True,
            "confidence": "wilson-95-lineage-binomial",
            "replayInstructions": True,
            "historicalCheckpointsPublished": True,
        },
        "contentIdentitySha256": "",
    })


def schema(title: str, schema_id: str, required: list[str], properties: dict[str, Any]) -> dict[str, Any]:
    return {
        "$schema": "https://json-schema.org/draft/2020-12/schema",
        "$id": f"https://genesiscode.dev/schemas/{schema_id}.json",
        "title": title,
        "type": "object",
        "additionalProperties": False,
        "required": required,
        "properties": properties,
        "$defs": {
            "hash": {"type": "string", "pattern": "^[0-9a-f]{64}$"},
            "id": {"type": "string", "pattern": "^[A-Za-z0-9][A-Za-z0-9._-]{0,127}$"},
            "nullableHash": {"oneOf": [{"$ref": "#/$defs/hash"}, {"type": "null"}]},
        },
    }


def render_schemas() -> dict[Path, dict[str, Any]]:
    identity = {"$ref": "#/$defs/hash"}
    identifier = {"$ref": "#/$defs/id"}
    nullable_hash = {"$ref": "#/$defs/nullableHash"}
    nonnegative = {"type": "integer", "minimum": 0}
    nullable_nonnegative = {"oneOf": [nonnegative, {"type": "null"}]}
    nullable_string = {"oneOf": [{"type": "string", "minLength": 1, "maxLength": 512}, {"type": "null"}]}
    string_set = {"type": "array", "maxItems": 1024, "uniqueItems": True, "items": {"type": "string", "minLength": 1, "maxLength": 256}}

    def closed_schema(required: list[str], properties: dict[str, Any]) -> dict[str, Any]:
        return {"type": "object", "additionalProperties": False, "required": required, "properties": properties}

    def envelope_schema(payload_type: str) -> dict[str, Any]:
        signature = closed_schema(["keyid", "sig"], {"keyid": {"type": "string", "pattern": "^sha256:[0-9a-f]{64}$"}, "sig": {"type": "string", "minLength": 88, "maxLength": 88}})
        return closed_schema(["payloadType", "payload", "signatures"], {"payloadType": {"const": payload_type}, "payload": {"type": "string", "minLength": 1, "maxLength": MAX_JSON_BYTES * 2}, "signatures": {"type": "array", "minItems": 1, "maxItems": 1, "items": signature}})

    evaluation = closed_schema(
        ["id", "taskEpochId", "contextMode", "interactionMode", "expectedLineageIds", "expectedLineagesIdentitySha256"],
        {"id": identifier, "taskEpochId": identifier, "contextMode": {"enum": ["compact-small", "extended-large", "standard-medium"]}, "interactionMode": {"enum": ["artifact-response-v0.1", "genesis-mcp-2025-11-25"]}, "expectedLineageIds": {"type": "array", "minItems": 1, "maxItems": 1024, "uniqueItems": True, "items": identifier}, "expectedLineagesIdentitySha256": identity},
    )
    track = closed_schema(
        ["id", "scaffoldClass", "scaffoldIdentitySha256", "genesisSpecificTraining", "adaptationIdentitySha256", "inferenceMode", "networkMode"],
        {"id": {"enum": sorted(TRACKS)}, "scaffoldClass": {"enum": ["disclosed-custom", "fixed-reference"]}, "scaffoldIdentitySha256": identity, "genesisSpecificTraining": {"enum": ["declared-public", "none", "unknown"]}, "adaptationIdentitySha256": nullable_hash, "inferenceMode": {"enum": ["local-offline", "remote-disclosed"]}, "networkMode": {"enum": ["deny", "provider-only"]}},
    )
    model = closed_schema(
        ["familyId", "providerId", "id", "revision", "runtimeId", "runtimeVersion", "runtimeArtifactSha256"],
        {"familyId": identifier, "providerId": identifier, "id": identifier, "revision": identifier, "runtimeId": identifier, "runtimeVersion": identifier, "runtimeArtifactSha256": nullable_hash},
    )
    contamination = closed_schema(
        ["label", "evidenceCodes", "evidenceIdentitySha256"],
        {"label": {"enum": sorted(CONTAMINATION)}, "evidenceCodes": string_set, "evidenceIdentitySha256": nullable_hash},
    )
    hardware = closed_schema(
        ["classId", "combinedResidentBytes", "evidenceIdentitySha256", "measurementMethod"],
        {"classId": {"oneOf": [{"enum": ["embedded-l", "embedded-m", "embedded-s"]}, {"type": "null"}]}, "combinedResidentBytes": nullable_nonnegative, "evidenceIdentitySha256": nullable_hash, "measurementMethod": {"enum": ["enforced-hard-ceiling", "measured-peak-rss", "not-claimed"]}},
    )
    economics = closed_schema(
        ["currency", "costMicrounits", "latencyMs", "energyMillijoules"],
        {"currency": {"oneOf": [{"type": "string", "pattern": "^[A-Z]{3}$"}, {"type": "null"}]}, "costMicrounits": nullable_nonnegative, "latencyMs": nullable_nonnegative, "energyMillijoules": nullable_nonnegative},
    )
    claim = closed_schema(
        ["kind", "version", "registryPolicyIdentitySha256", "evaluation", "track", "model", "contamination", "hardware", "economics", "contentIdentitySha256"],
        {"kind": {"const": "genesis/genesisbench-submission-claim-v0.1"}, "version": {"const": "0.1.0"}, "registryPolicyIdentitySha256": identity, "evaluation": evaluation, "track": track, "model": model, "contamination": contamination, "hardware": hardware, "economics": economics, "contentIdentitySha256": identity},
    )
    public_key = closed_schema(
        ["id", "keyId", "publicKeyBase64", "provenance"],
        {"id": identifier, "keyId": {"type": "string", "pattern": "^sha256:[0-9a-f]{64}$"}, "publicKeyBase64": {"type": "string", "minLength": 44, "maxLength": 44}, "provenance": {"type": "string", "minLength": 1, "maxLength": 512}},
    )
    protocol = closed_schema(["id", "version", "identitySha256"], {"id": identifier, "version": {"type": "string", "pattern": "^[0-9]+\\.[0-9]+\\.[0-9]+$"}, "identitySha256": identity})
    ranking = closed_schema(
        ["lexicographicKeys", "costLatencyAffectRank", "completeEvaluationSetRequired", "tiesRemainTies"],
        {"lexicographicKeys": {"const": render_authority()["ranking"]["lexicographicKeys"]}, "costLatencyAffectRank": {"const": False}, "completeEvaluationSetRequired": {"const": True}, "tiesRemainTies": {"const": True}},
    )
    policy = closed_schema(
        ["kind", "version", "registryId", "protocol", "operator", "submitters", "admission", "ranking", "contentIdentitySha256"],
        {"kind": {"const": "genesis/genesisbench-registry-policy-v0.1"}, "version": {"const": "0.1.0"}, "registryId": identifier, "protocol": protocol, "operator": public_key, "submitters": {"type": "array", "minItems": 1, "maxItems": 1024, "items": public_key}, "admission": closed_schema(["allowedTracks", "maxBundleBytes", "rankPublicReferences"], {"allowedTracks": {"type": "array", "minItems": 1, "maxItems": 4, "uniqueItems": True, "items": {"enum": sorted(TRACKS)}}, "maxBundleBytes": {"type": "integer", "minimum": 1, "maximum": MAX_BUNDLE_BYTES}, "rankPublicReferences": {"const": False}}), "ranking": ranking, "contentIdentitySha256": identity},
    )
    bundle_binding = closed_schema(["sha256", "bytes", "manifestIdentitySha256", "runIdentitySha256"], {"sha256": identity, "bytes": {"type": "integer", "minimum": 1, "maximum": MAX_BUNDLE_BYTES}, "manifestIdentitySha256": identity, "runIdentitySha256": identity})
    submission_statement = closed_schema(
        ["kind", "version", "submitterId", "bundle", "claim", "contentIdentitySha256"],
        {"kind": {"const": "genesis/genesisbench-submission-statement-v0.1"}, "version": {"const": "0.1.0"}, "submitterId": identifier, "bundle": bundle_binding, "claim": claim, "contentIdentitySha256": identity},
    )
    signed_submission = closed_schema(
        ["kind", "version", "statement", "envelope", "contentIdentitySha256"],
        {"kind": {"const": "genesis/genesisbench-signed-submission-v0.1"}, "version": {"const": "0.1.0"}, "statement": submission_statement, "envelope": envelope_schema(SUBMISSION_PAYLOAD_TYPE), "contentIdentitySha256": identity},
    )
    registry_binding = closed_schema(["id", "policyIdentitySha256", "protocolIdentitySha256"], {"id": identifier, "policyIdentitySha256": identity, "protocolIdentitySha256": identity})
    result_submission = closed_schema(["identitySha256", "submitterId", "submitterKeyId", "submitterProvenance", "bundleSha256", "runIdentitySha256"], {"identitySha256": identity, "submitterId": identifier, "submitterKeyId": {"type": "string", "pattern": "^sha256:[0-9a-f]{64}$"}, "submitterProvenance": {"type": "string", "minLength": 1, "maxLength": 512}, "bundleSha256": identity, "runIdentitySha256": identity})
    system = closed_schema(["id", "evaluationId", "expectedLineageIds", "expectedLineagesIdentitySha256"], {"id": identity, "evaluationId": identifier, "expectedLineageIds": evaluation["properties"]["expectedLineageIds"], "expectedLineagesIdentitySha256": identity})
    cohort = closed_schema(
        ["protocolIdentitySha256", "trackId", "scaffoldIdentitySha256", "taskEpochId", "contextMode", "interactionMode", "hardwareClassId", "contaminationLabel", "visibilityClass", "expectedLineagesIdentitySha256", "contentIdentitySha256"],
        {"protocolIdentitySha256": identity, "trackId": {"enum": sorted(TRACKS)}, "scaffoldIdentitySha256": identity, "taskEpochId": identifier, "contextMode": evaluation["properties"]["contextMode"], "interactionMode": evaluation["properties"]["interactionMode"], "hardwareClassId": hardware["properties"]["classId"], "contaminationLabel": {"enum": sorted(CONTAMINATION)}, "visibilityClass": {"enum": ["held-out-commitment", "public-development-reference"]}, "expectedLineagesIdentitySha256": identity, "contentIdentitySha256": identity},
    )
    case = closed_schema(["id", "lineageId", "lineageIdentitySha256", "conditionId", "conditionIdentitySha256", "taskClass", "contextTier", "visibilityClass"], {"id": identifier, "lineageId": identifier, "lineageIdentitySha256": identity, "conditionId": identifier, "conditionIdentitySha256": identity, "taskClass": identifier, "contextTier": {"enum": ["large", "medium", "small"]}, "visibilityClass": cohort["properties"]["visibilityClass"]})
    result_contamination = closed_schema(["claimedLabel", "strongestSupportedLabel", "evidenceCodes", "evidenceIdentitySha256"], {"claimedLabel": {"enum": sorted(CONTAMINATION)}, "strongestSupportedLabel": {"enum": sorted(CONTAMINATION)}, "evidenceCodes": string_set, "evidenceIdentitySha256": nullable_hash})
    outcome = closed_schema(["runOutcome", "verifiedSolve", "admissionDecision", "reasonCodes", "validityFailures"], {"runOutcome": {"enum": sorted(OUTCOMES)}, "verifiedSolve": {"type": "boolean"}, "admissionDecision": {"enum": ["invalid", "ranked", "unranked"]}, "reasonCodes": string_set, "validityFailures": string_set})
    dimension = closed_schema(["applicable", "id", "scoreBasisPoints", "weightBasisPoints"], {"applicable": {"type": "boolean"}, "id": identifier, "scoreBasisPoints": {"type": "integer", "minimum": 0, "maximum": 10000}, "weightBasisPoints": {"type": "integer", "minimum": 0, "maximum": 10000}})
    score = closed_schema(["identitySha256", "qualityScoreBasisPoints", "dimensions", "invalidPartialQualityBasisPoints"], {"identitySha256": nullable_hash, "qualityScoreBasisPoints": {"oneOf": [{"type": "integer", "minimum": 0, "maximum": 10000}, {"type": "null"}]}, "dimensions": {"type": "array", "maxItems": 16, "items": dimension}, "invalidPartialQualityBasisPoints": {"oneOf": [{"const": 0}, {"type": "null"}]}})
    efficiency = closed_schema(["capabilityExcessUnits", "contextBytes", "toolCalls", "repairCalls", "attempts"], {key: nonnegative for key in ["capabilityExcessUnits", "contextBytes", "toolCalls", "repairCalls", "attempts"]})
    secret_disclosure = closed_schema(["valuesRecorded", "declaredNames", "presentNames", "presenceRecorded"], {"valuesRecorded": {"const": False}, "declaredNames": {"type": "array", "maxItems": 1, "uniqueItems": True, "items": identifier}, "presentNames": {"type": "array", "maxItems": 1, "uniqueItems": True, "items": identifier}, "presenceRecorded": {"const": True}})
    attempt = closed_schema(["index", "requestArtifact", "requestIdentitySha256", "responseArtifact", "responseIdentitySha256", "status", "elapsedMs", "secretDisclosure"], {"index": {"type": "integer", "minimum": 0, "maximum": 15}, "requestArtifact": {"type": "string", "pattern": "^[A-Za-z0-9._/-]{1,512}$"}, "requestIdentitySha256": identity, "responseArtifact": {"type": "string", "pattern": "^[A-Za-z0-9._/-]{1,512}$"}, "responseIdentitySha256": identity, "status": {"enum": ["cancelled", "failed", "succeeded", "timed-out"]}, "elapsedMs": {"type": "integer", "minimum": 0, "maximum": 86400000}, "secretDisclosure": secret_disclosure})
    replay = closed_schema(["command", "adapterInvoked", "modelAccessed", "independentRescoreMatched"], {"command": {"type": "string", "minLength": 1, "maxLength": 2048}, "adapterInvoked": {"const": False}, "modelAccessed": {"const": False}, "independentRescoreMatched": {"type": "boolean"}})
    result = closed_schema(
        ["kind", "version", "registry", "submission", "system", "cohort", "case", "model", "track", "contamination", "hardware", "outcome", "score", "efficiency", "economics", "attempts", "replay", "contentIdentitySha256"],
        {"kind": {"const": "genesis/genesisbench-registry-result-v0.1"}, "version": {"const": "0.1.0"}, "registry": registry_binding, "submission": result_submission, "system": system, "cohort": cohort, "case": case, "model": model, "track": track, "contamination": result_contamination, "hardware": hardware, "outcome": outcome, "score": score, "efficiency": efficiency, "economics": economics, "attempts": {"type": "array", "minItems": 1, "maxItems": 16, "items": attempt}, "replay": replay, "contentIdentitySha256": identity},
    )
    event_statement = closed_schema(["kind", "version", "registryId", "policyIdentitySha256", "sequence", "previousEventIdentitySha256", "resultIdentitySha256", "submissionIdentitySha256", "bundleSha256", "decision", "contentIdentitySha256"], {"kind": {"const": "genesis/genesisbench-registry-event-statement-v0.1"}, "version": {"const": "0.1.0"}, "registryId": identifier, "policyIdentitySha256": identity, "sequence": {"type": "integer", "minimum": 1}, "previousEventIdentitySha256": nullable_hash, "resultIdentitySha256": identity, "submissionIdentitySha256": identity, "bundleSha256": identity, "decision": {"enum": ["invalid", "ranked", "unranked"]}, "contentIdentitySha256": identity})
    signed_event = closed_schema(["kind", "version", "statement", "envelope", "contentIdentitySha256"], {"kind": {"const": "genesis/genesisbench-signed-event-v0.1"}, "version": {"const": "0.1.0"}, "statement": event_statement, "envelope": envelope_schema(EVENT_PAYLOAD_TYPE), "contentIdentitySha256": identity})
    checkpoint_statement_schema = closed_schema(["kind", "version", "registryId", "policyIdentitySha256", "sequence", "headEventIdentitySha256", "signedEventLogIdentitySha256", "eventStatementLogIdentitySha256", "resultSetIdentitySha256", "resultCount", "contentIdentitySha256"], {"kind": {"const": "genesis/genesisbench-registry-checkpoint-statement-v0.1"}, "version": {"const": "0.1.0"}, "registryId": identifier, "policyIdentitySha256": identity, "sequence": nonnegative, "headEventIdentitySha256": nullable_hash, "signedEventLogIdentitySha256": identity, "eventStatementLogIdentitySha256": identity, "resultSetIdentitySha256": identity, "resultCount": nonnegative, "contentIdentitySha256": identity})
    signed_checkpoint = closed_schema(["kind", "version", "statement", "envelope", "contentIdentitySha256"], {"kind": {"const": "genesis/genesisbench-signed-checkpoint-v0.1"}, "version": {"const": "0.1.0"}, "statement": checkpoint_statement_schema, "envelope": envelope_schema(CHECKPOINT_PAYLOAD_TYPE), "contentIdentitySha256": identity})

    per_lineage = closed_schema(["lineageId", "resultIdentitySha256", "outcome", "verifiedSolve"], {"lineageId": identifier, "resultIdentitySha256": nullable_hash, "outcome": {"enum": ["abstained", "failed", "invalid", "missing", "verified"]}, "verifiedSolve": {"type": "boolean"}})
    per_class = closed_schema(["taskClass", "observed", "verifiedSolved", "conditionalQualityMeanBasisPoints"], {"taskClass": identifier, "observed": nonnegative, "verifiedSolved": nonnegative, "conditionalQualityMeanBasisPoints": {"oneOf": [{"type": "integer", "minimum": 0, "maximum": 10000}, {"type": "null"}]}})
    system_summary_fields = {"systemId": identity, "model": model, "track": track, "completeEvaluationSet": {"type": "boolean"}, "rankEligible": {"type": "boolean"}, "expectedLineages": {"type": "integer", "minimum": 1}, "observedLineages": nonnegative, "verifiedSolvedLineages": nonnegative, "solveRateBasisPoints": {"type": "integer", "minimum": 0, "maximum": 10000}, "solveRateWilson95BasisPoints": {"type": "array", "minItems": 2, "maxItems": 2, "items": {"type": "integer", "minimum": 0, "maximum": 10000}}, "conditionalQualityDenominator": nonnegative, "conditionalQualityMeanBasisPoints": {"oneOf": [{"type": "integer", "minimum": 0, "maximum": 10000}, {"type": "null"}]}, "capabilityExcessUnits": nonnegative, "contextBytes": nonnegative, "toolCalls": nonnegative, "repairCalls": nonnegative, "economics": {"type": "array", "items": economics}, "resultIdentities": {"type": "array", "uniqueItems": True, "items": identity}, "missingLineageIds": {"type": "array", "uniqueItems": True, "items": identifier}, "perLineage": {"type": "array", "items": per_lineage}, "perClass": {"type": "array", "items": per_class}}
    system_required = list(system_summary_fields)
    unranked_system = closed_schema(system_required, system_summary_fields)
    ranked_fields = dict(system_summary_fields); ranked_fields["rank"] = {"type": "integer", "minimum": 1}
    ranked_system = closed_schema(system_required + ["rank"], ranked_fields)
    cohort_publication = closed_schema(["cohort", "rankedSystems", "unrankedSystems", "rankingKeys", "costLatencyAffectRank"], {"cohort": cohort, "rankedSystems": {"type": "array", "items": ranked_system}, "unrankedSystems": {"type": "array", "items": unranked_system}, "rankingKeys": {"const": render_authority()["ranking"]["lexicographicKeys"]}, "costLatencyAffectRank": {"const": False}})
    publication_result = closed_schema(["resultIdentitySha256", "systemId", "cohortIdentitySha256", "case", "outcome", "score", "attempts", "model", "track", "contamination", "hardware", "economics", "replay"], {"resultIdentitySha256": identity, "systemId": identity, "cohortIdentitySha256": identity, "case": case, "outcome": outcome, "score": score, "attempts": {"type": "array", "items": attempt}, "model": model, "track": track, "contamination": result_contamination, "hardware": hardware, "economics": economics, "replay": replay})
    leaderboard = closed_schema(["kind", "version", "registry", "history", "cohorts", "results", "contentIdentitySha256"], {"kind": {"const": "genesis/genesisbench-leaderboard-v0.1"}, "version": {"const": "0.1.0"}, "registry": closed_schema(["id", "identitySha256", "policyIdentitySha256", "protocolIdentitySha256"], {"id": identifier, "identitySha256": identity, "policyIdentitySha256": identity, "protocolIdentitySha256": identity}), "history": closed_schema(["eventCount", "headEventStatementIdentitySha256", "headSignedEventIdentitySha256", "signedEventLogIdentitySha256", "checkpointIdentitySha256", "allEventStatementIdentities", "historicalResultsMutable", "silentSuppressionAllowed"], {"eventCount": nonnegative, "headEventStatementIdentitySha256": nullable_hash, "headSignedEventIdentitySha256": nullable_hash, "signedEventLogIdentitySha256": identity, "checkpointIdentitySha256": identity, "allEventStatementIdentities": {"type": "array", "uniqueItems": True, "items": identity}, "historicalResultsMutable": {"const": False}, "silentSuppressionAllowed": {"const": False}}), "cohorts": {"type": "array", "items": cohort_publication}, "results": {"type": "array", "items": publication_result}, "contentIdentitySha256": identity})

    return {
        CLAIM_SCHEMA_PATH: schema("GenesisBench submission claim v0.1", "genesisbench-submission-claim-v0.1", claim["required"], claim["properties"]),
        POLICY_SCHEMA_PATH: schema("GenesisBench registry policy v0.1", "genesisbench-registry-policy-v0.1", policy["required"], policy["properties"]),
        SUBMISSION_SCHEMA_PATH: schema("Signed GenesisBench submission v0.1", "genesisbench-signed-submission-v0.1", signed_submission["required"], signed_submission["properties"]),
        RESULT_SCHEMA_PATH: schema("GenesisBench registry result v0.1", "genesisbench-registry-result-v0.1", result["required"], result["properties"]),
        EVENT_SCHEMA_PATH: schema("Signed GenesisBench registry event v0.1", "genesisbench-registry-event-v0.1", signed_event["required"], signed_event["properties"]),
        CHECKPOINT_SCHEMA_PATH: schema("Signed GenesisBench registry checkpoint v0.1", "genesisbench-registry-checkpoint-v0.1", signed_checkpoint["required"], signed_checkpoint["properties"]),
        PUBLICATION_SCHEMA_PATH: schema("GenesisBench static leaderboard v0.1", "genesisbench-leaderboard-v0.1", leaderboard["required"], leaderboard["properties"]),
    }


def validate_policy(value: Any) -> dict[str, Any]:
    policy = closed(value, {"kind", "version", "registryId", "protocol", "operator", "submitters", "admission", "ranking", "contentIdentitySha256"}, "registry policy")
    require(policy["kind"] == "genesis/genesisbench-registry-policy-v0.1" and policy["version"] == "0.1.0", "registry policy version drift")
    bounded_id(policy["registryId"], "registry id")
    profile = front_door.load_json(PROFILE_PATH)
    protocol = closed(policy["protocol"], {"id", "version", "identitySha256"}, "policy protocol")
    require(protocol == {"id": profile["protocolId"], "version": profile["version"], "identitySha256": profile["contentIdentitySha256"]}, "registry policy protocol mismatch")
    operator = validate_public_key(policy["operator"], "operator")
    submitters = policy["submitters"]
    require(isinstance(submitters, list) and 1 <= len(submitters) <= 1024, "registry must pin a bounded non-empty submitter set")
    for row in submitters:
        validate_public_key(row, "submitter")
    require([row["id"] for row in submitters] == sorted(set(row["id"] for row in submitters)), "submitters must be sorted and unique")
    require(operator["keyId"] not in {row["keyId"] for row in submitters}, "operator and submitter keys must be separated")
    admission = closed(policy["admission"], {"allowedTracks", "maxBundleBytes", "rankPublicReferences"}, "admission policy")
    require(admission["allowedTracks"] == sorted(set(admission["allowedTracks"])) and set(admission["allowedTracks"]) <= TRACKS and admission["allowedTracks"], "invalid allowed tracks")
    require(isinstance(admission["maxBundleBytes"], int) and 1 <= admission["maxBundleBytes"] <= MAX_BUNDLE_BYTES, "invalid bundle byte ceiling")
    require(admission["rankPublicReferences"] is False, "public references must never be ranked")
    ranking = closed(policy["ranking"], {"lexicographicKeys", "costLatencyAffectRank", "completeEvaluationSetRequired", "tiesRemainTies"}, "ranking policy")
    require(ranking == {
        "lexicographicKeys": render_authority()["ranking"]["lexicographicKeys"],
        "costLatencyAffectRank": False,
        "completeEvaluationSetRequired": True,
        "tiesRemainTies": True,
    }, "registry ranking policy differs from authority")
    validate_identity(policy)
    return policy


def validate_claim(value: Any) -> dict[str, Any]:
    claim = closed(value, {"kind", "version", "registryPolicyIdentitySha256", "evaluation", "track", "model", "contamination", "hardware", "economics", "contentIdentitySha256"}, "submission claim")
    require(claim["kind"] == "genesis/genesisbench-submission-claim-v0.1" and claim["version"] == "0.1.0", "claim version drift")
    hash_value(claim["registryPolicyIdentitySha256"], "claim policy identity")
    evaluation = closed(claim["evaluation"], {"id", "taskEpochId", "contextMode", "interactionMode", "expectedLineageIds", "expectedLineagesIdentitySha256"}, "claim evaluation")
    bounded_id(evaluation["id"], "evaluation id"); bounded_id(evaluation["taskEpochId"], "task epoch id")
    require(evaluation["contextMode"] in {"compact-small", "standard-medium", "extended-large"}, "invalid context mode")
    require(evaluation["interactionMode"] in {"artifact-response-v0.1", "genesis-mcp-2025-11-25"}, "invalid interaction mode")
    lineages = sorted_strings(evaluation["expectedLineageIds"], "expected lineage ids", maximum=1024)
    require(lineages and evaluation["expectedLineagesIdentitySha256"] == sha256_bytes(canonical(lineages)), "expected lineage set identity mismatch")
    track = closed(claim["track"], {"id", "scaffoldClass", "scaffoldIdentitySha256", "genesisSpecificTraining", "adaptationIdentitySha256", "inferenceMode", "networkMode"}, "claim track")
    require(track["id"] in TRACKS and track["scaffoldClass"] in {"fixed-reference", "disclosed-custom"}, "invalid track declaration")
    hash_value(track["scaffoldIdentitySha256"], "scaffold identity")
    require(track["genesisSpecificTraining"] in {"none", "declared-public", "unknown"}, "invalid training declaration")
    hash_value(track["adaptationIdentitySha256"], "adaptation identity", nullable=True)
    require((track["genesisSpecificTraining"] == "declared-public") == (track["adaptationIdentitySha256"] is not None), "adaptation binding disagrees with training declaration")
    require(track["inferenceMode"] in {"local-offline", "remote-disclosed"} and track["networkMode"] in {"deny", "provider-only"}, "invalid inference declaration")
    model = closed(claim["model"], {"familyId", "providerId", "id", "revision", "runtimeId", "runtimeVersion", "runtimeArtifactSha256"}, "claim model")
    for key in ("familyId", "providerId", "id", "revision", "runtimeId", "runtimeVersion"):
        bounded_id(model[key], f"model {key}")
    hash_value(model["runtimeArtifactSha256"], "runtime artifact identity", nullable=True)
    contamination = closed(claim["contamination"], {"label", "evidenceCodes", "evidenceIdentitySha256"}, "claim contamination")
    require(contamination["label"] in CONTAMINATION, "invalid contamination label")
    sorted_strings(contamination["evidenceCodes"], "contamination evidence codes")
    hash_value(contamination["evidenceIdentitySha256"], "contamination evidence identity", nullable=True)
    hardware = closed(claim["hardware"], {"classId", "combinedResidentBytes", "evidenceIdentitySha256", "measurementMethod"}, "claim hardware")
    require(hardware["classId"] is None or hardware["classId"] in {"embedded-s", "embedded-m", "embedded-l"}, "invalid hardware class")
    require(hardware["combinedResidentBytes"] is None or isinstance(hardware["combinedResidentBytes"], int) and hardware["combinedResidentBytes"] >= 0, "invalid resident bytes")
    hash_value(hardware["evidenceIdentitySha256"], "hardware evidence identity", nullable=True)
    require(hardware["measurementMethod"] in {"measured-peak-rss", "enforced-hard-ceiling", "not-claimed"}, "invalid hardware measurement method")
    economics = closed(claim["economics"], {"currency", "costMicrounits", "latencyMs", "energyMillijoules"}, "claim economics")
    require(economics["currency"] is None or isinstance(economics["currency"], str) and len(economics["currency"]) == 3 and economics["currency"].isupper(), "invalid currency")
    for key in ("costMicrounits", "latencyMs", "energyMillijoules"):
        require(economics[key] is None or isinstance(economics[key], int) and economics[key] >= 0, f"invalid {key}")
    require((economics["currency"] is None) == (economics["costMicrounits"] is None), "currency and cost must be jointly present")
    validate_identity(claim)
    return claim


def validate_submission_statement(value: Any) -> dict[str, Any]:
    statement = closed(value, {"kind", "version", "submitterId", "bundle", "claim", "contentIdentitySha256"}, "submission statement")
    require(statement["kind"] == "genesis/genesisbench-submission-statement-v0.1" and statement["version"] == "0.1.0", "submission statement version drift")
    bounded_id(statement["submitterId"], "submitter id")
    bundle = closed(statement["bundle"], {"sha256", "bytes", "manifestIdentitySha256", "runIdentitySha256"}, "submission bundle")
    hash_value(bundle["sha256"], "bundle hash"); hash_value(bundle["manifestIdentitySha256"], "bundle manifest identity"); hash_value(bundle["runIdentitySha256"], "bundle run identity")
    require(isinstance(bundle["bytes"], int) and 1 <= bundle["bytes"] <= MAX_BUNDLE_BYTES, "invalid bundle size")
    validate_claim(statement["claim"])
    validate_identity(statement)
    return statement


def crypto(helper: Path, args: list[str]) -> dict[str, Any]:
    require(helper.is_file() and not helper.is_symlink(), "crypto helper must be a regular non-symlink executable")
    process = subprocess.run([str(helper), "--json", "bench", *args], cwd=ROOT, stdout=subprocess.PIPE, stderr=subprocess.PIPE, timeout=60, check=False)
    require(len(process.stdout) <= MAX_JSON_BYTES and len(process.stderr) <= MAX_JSON_BYTES, "crypto helper output exceeds finite limit")
    if process.returncode != 0:
        try:
            failure = json.loads(process.stdout)
            message = failure.get("error", {}).get("message", "crypto helper failed")
        except (ValueError, AttributeError):
            message = process.stderr.decode("utf-8", "replace").strip() or "crypto helper failed"
        raise RegistryError(message)
    report = json.loads(process.stdout, object_pairs_hook=unique_object)
    require(report.get("ok") is True and isinstance(report.get("data"), dict), "crypto helper emitted an invalid result")
    return report["data"]


def sign_statement(helper: Path, statement: dict[str, Any], key: Path, payload_type: str, signed_kind: str) -> dict[str, Any]:
    with tempfile.TemporaryDirectory(prefix="genesisbench-sign-") as temporary:
        payload = Path(temporary) / "payload.json"
        payload.write_bytes(canonical(statement))
        result = crypto(helper, ["__crypto-sign", "--payload", str(payload), "--key", str(key), "--payload-type", payload_type])
    signed = identified({"kind": signed_kind, "version": "0.1.0", "statement": statement, "envelope": result["envelope"], "contentIdentitySha256": ""})
    return signed


def verify_signed(helper: Path, signed: Any, public: dict[str, Any], payload_type: str, signed_kind: str) -> dict[str, Any]:
    document = closed(signed, {"kind", "version", "statement", "envelope", "contentIdentitySha256"}, "signed document")
    require(document["kind"] == signed_kind and document["version"] == "0.1.0", "signed document version drift")
    validate_identity(document)
    envelope = closed(document["envelope"], {"payloadType", "payload", "signatures"}, "DSSE envelope")
    require(envelope["payloadType"] == payload_type and isinstance(envelope["signatures"], list) and len(envelope["signatures"]) == 1, "DSSE envelope policy mismatch")
    try:
        payload = base64.b64decode(envelope["payload"], validate=True)
    except (ValueError, TypeError) as error:
        raise RegistryError(f"invalid DSSE payload: {error}") from error
    require(payload == canonical(document["statement"]), "DSSE payload does not equal the canonical statement")
    with tempfile.TemporaryDirectory(prefix="genesisbench-verify-") as temporary:
        path = Path(temporary) / "envelope.json"
        path.write_bytes(canonical(envelope))
        report = crypto(helper, ["__crypto-verify", "--envelope", str(path), "--public-key-base64", public["publicKeyBase64"], "--expected-keyid", public["keyId"], "--payload-type", payload_type])
    require(report == {"kind": "genesis/genesisbench-dsse-verification-v0.1", "version": "0.1.0", "verified": True, "keyId": public["keyId"], "payloadType": payload_type, "payloadSha256": sha256_bytes(payload)}, "crypto verification report drift")
    return document["statement"]


def write_new(path: Path, payload: bytes, label: str) -> None:
    path.parent.mkdir(parents=True, exist_ok=True)
    require(not path.parent.is_symlink(), f"{label} parent cannot be a symlink")
    try:
        with path.open("xb") as stream:
            stream.write(payload)
            stream.flush()
            os.fsync(stream.fileno())
    except FileExistsError:
        require(path.is_file() and not path.is_symlink() and path.read_bytes() == payload, f"{label} identity collision")


def submit(bundle: Path, claim_path: Path, outbox: Path, submitter: str, key: Path, helper: Path) -> dict[str, Any]:
    bounded_id(submitter, "submitter id")
    claim = validate_claim(load_json(claim_path, "submission claim"))
    manifest, bundle_sha = front_door.validate_bundle(bundle)
    require(bundle.stat().st_size <= MAX_BUNDLE_BYTES, "bundle exceeds submission size limit")
    statement = identified({
        "kind": "genesis/genesisbench-submission-statement-v0.1", "version": "0.1.0", "submitterId": submitter,
        "bundle": {"sha256": bundle_sha, "bytes": bundle.stat().st_size, "manifestIdentitySha256": manifest["contentIdentitySha256"], "runIdentitySha256": manifest["runIdentitySha256"]},
        "claim": claim, "contentIdentitySha256": "",
    })
    validate_submission_statement(statement)
    signed = sign_statement(helper, statement, key, SUBMISSION_PAYLOAD_TYPE, "genesis/genesisbench-signed-submission-v0.1")
    require(not outbox.exists() or outbox.is_dir() and not outbox.is_symlink(), "outbox must be a regular directory")
    outbox.mkdir(parents=True, exist_ok=True)
    bundle_target = outbox / f"{bundle_sha}.gcbundle"
    if not bundle_target.exists():
        temporary = outbox / f".{bundle_sha}.tmp"
        shutil.copyfile(bundle, temporary)
        require(sha256_file(temporary) == bundle_sha, "outbox bundle copy mismatch")
        os.replace(temporary, bundle_target)
    else:
        require(bundle_target.is_file() and not bundle_target.is_symlink() and sha256_file(bundle_target) == bundle_sha, "outbox bundle collision")
    submission_target = outbox / f"{signed['contentIdentitySha256']}.submission.json"
    write_new(submission_target, pretty(signed), "signed submission")
    return {"kind": "genesis/genesisbench-submission-result-v0.1", "version": "0.1.0", "transport": "local-signed-immutable-outbox-v0.1", "submission": submission_target.name, "submissionIdentitySha256": signed["contentIdentitySha256"], "bundleSha256": bundle_sha, "submitter": submitter, "keyId": signed["envelope"]["signatures"][0]["keyid"]}


def validate_root_path(path: Path) -> Path:
    require(path.is_dir() and not path.is_symlink(), "registry root must be a regular non-symlink directory")
    return path.resolve(strict=True)


def registry_descriptor(policy: dict[str, Any]) -> dict[str, Any]:
    return identified({"kind": "genesis/genesisbench-registry-root-v0.1", "version": "0.1.0", "registryId": policy["registryId"], "policyIdentitySha256": policy["contentIdentitySha256"], "protocolIdentitySha256": policy["protocol"]["identitySha256"], "operatorKeyId": policy["operator"]["keyId"], "storage": "append-only-content-addressed-v0.1", "contentIdentitySha256": ""})


def list_events(root: Path) -> list[Path]:
    paths = directory_files(root / "events", "event log")
    require(all(path.suffix == ".json" for path in paths), "event log contains an unknown entry")
    return paths


def list_checkpoints(root: Path) -> list[Path]:
    paths = directory_files(root / "checkpoints", "checkpoint log")
    require(all(path.suffix == ".json" for path in paths), "checkpoint log contains an unknown entry")
    return paths


def checkpoint_statement(policy: dict[str, Any], events: list[tuple[dict[str, Any], dict[str, Any]]]) -> dict[str, Any]:
    signed_event_ids = [signed["contentIdentitySha256"] for signed, _ in events]
    statement_ids = [statement["contentIdentitySha256"] for _, statement in events]
    result_ids = [statement["resultIdentitySha256"] for _, statement in events]
    return identified({
        "kind": "genesis/genesisbench-registry-checkpoint-statement-v0.1", "version": "0.1.0",
        "registryId": policy["registryId"], "policyIdentitySha256": policy["contentIdentitySha256"],
        "sequence": len(events), "headEventIdentitySha256": signed_event_ids[-1] if signed_event_ids else None,
        "signedEventLogIdentitySha256": sha256_bytes(canonical(signed_event_ids)),
        "eventStatementLogIdentitySha256": sha256_bytes(canonical(statement_ids)),
        "resultSetIdentitySha256": sha256_bytes(canonical(sorted(result_ids))),
        "resultCount": len(result_ids), "contentIdentitySha256": "",
    })


def init_registry(registry: Path, policy_path: Path, operator_key: Path, helper: Path) -> dict[str, Any]:
    require(not registry.exists() and not registry.is_symlink(), "registry root already exists")
    policy = validate_policy(load_json(policy_path, "registry policy"))
    parent = registry.parent.resolve(strict=True)
    temporary = Path(tempfile.mkdtemp(prefix=f".{registry.name}.", dir=parent))
    try:
        for relative in ("authority", "events", "checkpoints", "objects/bundles", "objects/submissions", "objects/results"):
            (temporary / relative).mkdir(parents=True, exist_ok=False)
        (temporary / "authority/policy.json").write_bytes(pretty(policy))
        descriptor = registry_descriptor(policy)
        (temporary / "registry.json").write_bytes(pretty(descriptor))
        checkpoint = sign_statement(helper, checkpoint_statement(policy, []), operator_key, CHECKPOINT_PAYLOAD_TYPE, "genesis/genesisbench-signed-checkpoint-v0.1")
        require(checkpoint["envelope"]["signatures"][0]["keyid"] == policy["operator"]["keyId"], "operator key does not match registry policy")
        (temporary / f"checkpoints/000000000000-{checkpoint['contentIdentitySha256']}.json").write_bytes(pretty(checkpoint))
        os.replace(temporary, registry)
    except Exception:
        shutil.rmtree(temporary, ignore_errors=True)
        raise
    return {"kind": "genesis/genesisbench-registry-init-v0.1", "version": "0.1.0", "registryId": policy["registryId"], "registryIdentitySha256": descriptor["contentIdentitySha256"], "policyIdentitySha256": policy["contentIdentitySha256"], "checkpointIdentitySha256": checkpoint["contentIdentitySha256"], "sequence": 0}


def extract_bundle(bundle: Path, destination: Path) -> None:
    front_door.validate_bundle(bundle)
    destination.mkdir(parents=True, exist_ok=False)
    with tarfile.open(bundle, mode="r:gz") as archive:
        for member in archive:
            if member.name == "genesisbench-bundle/bundle-manifest.json":
                continue
            relative = member.name.removeprefix("genesisbench-bundle/")
            target = destination / relative
            target.parent.mkdir(parents=True, exist_ok=True)
            stream = archive.extractfile(member)
            require(stream is not None, "bundle member cannot be read")
            payload = stream.read(MAX_JSON_BYTES + 1)
            require(len(payload) <= MAX_JSON_BYTES, "bundle member exceeds extraction ceiling")
            target.write_bytes(payload)


def expected_hardware_class(resident: int) -> str | None:
    for identifier, ceiling in (("embedded-s", 4 * 1024**3), ("embedded-m", 16 * 1024**3), ("embedded-l", 64 * 1024**3)):
        if resident <= ceiling:
            return identifier
    return None


def derive_result(policy: dict[str, Any], statement: dict[str, Any], run_root: Path, rescore: dict[str, Any]) -> dict[str, Any]:
    run = front_door.validate_run(run_root / "run.json", check_files=True)
    claim = statement["claim"]
    require(claim["registryPolicyIdentitySha256"] == policy["contentIdentitySha256"], "submission targets a different registry policy")
    adapter = front_door.load_json(run_root / run["adapter"]["artifact"])
    plan = front_door.load_json(run_root / run["referenceAgent"]["planArtifact"])
    score = front_door.load_json(run_root / "score.json") if run["scoreIdentitySha256"] is not None else None
    suite = front_door.load_json(SUITE_PATH)
    case = next((row for row in suite["cases"] if row["id"] == run["case"]["id"]), None)
    require(case is not None, "run case is absent from benchmark authority")
    require(claim["model"]["id"] == adapter["model"]["id"] and claim["model"]["revision"] == adapter["model"]["revision"], "model claim does not match the immutable adapter")
    require(claim["track"]["scaffoldIdentitySha256"] == run["referenceAgent"]["profileIdentitySha256"], "scaffold claim does not match the executed reference agent")
    require(case["lineageId"] in claim["evaluation"]["expectedLineageIds"], "case lineage is outside the declared evaluation set")
    tier_mode = {"small": "compact-small", "medium": "standard-medium", "large": "extended-large"}[case["contextTier"]]
    require(claim["evaluation"]["contextMode"] == tier_mode and claim["evaluation"]["interactionMode"] == "artifact-response-v0.1", "evaluation context or interaction mode mismatch")

    invalid: list[str] = []
    unranked: list[str] = []
    outcome = run["outcome"]
    solved = outcome == "verified" and score is not None and score["validity"]["passed"] is True
    if outcome in {"failed", "invalid"} or score is not None and score["validity"]["passed"] is not True:
        invalid.append("run/invalid")
    if rescore["allFieldsValidated"] is not True or (score is not None and rescore["independentRescoreMatched"] is not True):
        invalid.append("score/mismatch")
    visibility = "public-development-reference" if case["oracleExposure"] == "public-development-reference" else "held-out-commitment"
    strongest_contamination = "declared-contaminated" if visibility == "public-development-reference" else claim["contamination"]["label"]
    if visibility == "public-development-reference":
        unranked.extend(["task/public-reference", "visibility/practice-only"])
    if claim["contamination"]["label"] != strongest_contamination:
        invalid.append("contamination/overclaim")
    track = claim["track"]
    if track["id"] not in policy["admission"]["allowedTracks"]:
        invalid.append("track/declaration-mismatch")
    reference = front_door.load_json(REFERENCE_PATH)
    if track["id"] == "cold-acquisition" and (track["scaffoldClass"] != "fixed-reference" or track["scaffoldIdentitySha256"] != reference["contentIdentitySha256"] or track["genesisSpecificTraining"] != "none"):
        invalid.append("track/scaffold-mismatch")
    if track["id"] == "genesis-adapted" and track["genesisSpecificTraining"] != "declared-public":
        invalid.append("track/declaration-mismatch")
    if track["genesisSpecificTraining"] == "unknown":
        unranked.append("track/training-provenance-incomplete")
    hardware = claim["hardware"]
    if track["id"] == "embedded-local":
        if track["inferenceMode"] != "local-offline" or track["networkMode"] != "deny" or adapter["class"] not in {"direct-local-runtime", "local-openai-compatible"}:
            invalid.append("track/offline-violation")
        complete_hardware = hardware["classId"] is not None and hardware["combinedResidentBytes"] is not None and hardware["evidenceIdentitySha256"] is not None and hardware["measurementMethod"] != "not-claimed"
        if not complete_hardware:
            unranked.append("track/hardware-evidence-incomplete")
        elif expected_hardware_class(hardware["combinedResidentBytes"]) != hardware["classId"]:
            invalid.append("track/hardware-class-mismatch")
    elif any(hardware[key] is not None for key in ("classId", "combinedResidentBytes", "evidenceIdentitySha256")) or hardware["measurementMethod"] != "not-claimed":
        invalid.append("track/hardware-class-mismatch")
    if len(run["attempts"]) != 1:
        unranked.append("attempt/multiple")
    if adapter["class"] == "deterministic-mock" or adapter["id"].startswith("fixture-"):
        unranked.append("model/conformance-fixture")
    invalid = sorted(set(invalid)); unranked = sorted(set(unranked))
    decision = "invalid" if invalid else ("ranked" if not unranked and solved else "unranked")
    reasons = invalid if invalid else unranked
    cohort_material = {
        "protocolIdentitySha256": policy["protocol"]["identitySha256"], "trackId": track["id"],
        "scaffoldIdentitySha256": track["scaffoldIdentitySha256"], "taskEpochId": claim["evaluation"]["taskEpochId"],
        "contextMode": claim["evaluation"]["contextMode"], "interactionMode": claim["evaluation"]["interactionMode"],
        "hardwareClassId": hardware["classId"], "contaminationLabel": strongest_contamination,
        "visibilityClass": visibility, "expectedLineagesIdentitySha256": claim["evaluation"]["expectedLineagesIdentitySha256"],
    }
    cohort_id = sha256_bytes(canonical(cohort_material))
    system_material = {"submitterId": statement["submitterId"], "evaluationId": claim["evaluation"]["id"], "cohortIdentitySha256": cohort_id, "model": claim["model"], "track": track}
    system_id = sha256_bytes(canonical(system_material))
    quality = score["qualityScoreBasisPoints"] if solved else None
    failed_dimensions = [] if score is None else score["validity"]["failedDimensions"]
    capability_excess = 0 if score is None else len(score["policy"]["broadenedAuthorities"])
    submitter = next(row for row in policy["submitters"] if row["id"] == statement["submitterId"])
    return identified({
        "kind": "genesis/genesisbench-registry-result-v0.1", "version": "0.1.0",
        "registry": {"id": policy["registryId"], "policyIdentitySha256": policy["contentIdentitySha256"], "protocolIdentitySha256": policy["protocol"]["identitySha256"]},
        "submission": {"identitySha256": statement["contentIdentitySha256"], "submitterId": statement["submitterId"], "submitterKeyId": submitter["keyId"], "submitterProvenance": submitter["provenance"], "bundleSha256": statement["bundle"]["sha256"], "runIdentitySha256": run["contentIdentitySha256"]},
        "system": {"id": system_id, "evaluationId": claim["evaluation"]["id"], "expectedLineageIds": claim["evaluation"]["expectedLineageIds"], "expectedLineagesIdentitySha256": claim["evaluation"]["expectedLineagesIdentitySha256"]},
        "cohort": cohort_material | {"contentIdentitySha256": cohort_id},
        "case": {"id": case["id"], "lineageId": case["lineageId"], "lineageIdentitySha256": case["lineageIdentitySha256"], "conditionId": case["conditionId"], "conditionIdentitySha256": case["conditionIdentitySha256"], "taskClass": case["taskClass"], "contextTier": case["contextTier"], "visibilityClass": visibility},
        "model": claim["model"], "track": track, "contamination": {"claimedLabel": claim["contamination"]["label"], "strongestSupportedLabel": strongest_contamination, "evidenceCodes": claim["contamination"]["evidenceCodes"], "evidenceIdentitySha256": claim["contamination"]["evidenceIdentitySha256"]},
        "hardware": hardware,
        "outcome": {"runOutcome": outcome, "verifiedSolve": solved, "admissionDecision": decision, "reasonCodes": reasons, "validityFailures": failed_dimensions},
        "score": {"identitySha256": run["scoreIdentitySha256"], "qualityScoreBasisPoints": quality, "dimensions": [] if score is None else score["dimensions"], "invalidPartialQualityBasisPoints": 0 if not solved else None},
        "efficiency": {"capabilityExcessUnits": capability_excess, "contextBytes": plan["context"]["totalBytes"], "toolCalls": len(plan["toolAllowlist"]), "repairCalls": plan["features"]["repairs"], "attempts": len(run["attempts"])},
        "economics": claim["economics"], "attempts": run["attempts"],
        "replay": {"command": f"genesis --json --selfhost-artifact SELFHOST bench replay --run extracted/{run['contentIdentitySha256']}/run.json", "adapterInvoked": False, "modelAccessed": False, "independentRescoreMatched": rescore["independentRescoreMatched"]},
        "contentIdentitySha256": "",
    })


def validate_result(result: Any, policy: dict[str, Any]) -> dict[str, Any]:
    fields = {"kind", "version", "registry", "submission", "system", "cohort", "case", "model", "track", "contamination", "hardware", "outcome", "score", "efficiency", "economics", "attempts", "replay", "contentIdentitySha256"}
    document = closed(result, fields, "registry result")
    require(document["kind"] == "genesis/genesisbench-registry-result-v0.1" and document["version"] == "0.1.0", "registry result version drift")
    require(document["registry"]["policyIdentitySha256"] == policy["contentIdentitySha256"], "result policy binding drift")
    require(document["outcome"]["admissionDecision"] in {"invalid", "ranked", "unranked"} and document["outcome"]["runOutcome"] in OUTCOMES, "invalid result outcome")
    require(document["outcome"]["reasonCodes"] == sorted(set(document["outcome"]["reasonCodes"])), "result reasons are not canonical")
    require(document["score"]["qualityScoreBasisPoints"] is not None if document["outcome"]["verifiedSolve"] else document["score"]["qualityScoreBasisPoints"] is None, "conditional quality leaked into an unsolved result")
    require(document["score"]["invalidPartialQualityBasisPoints"] is None if document["outcome"]["verifiedSolve"] else document["score"]["invalidPartialQualityBasisPoints"] == 0, "invalid partial quality policy drift")
    validate_identity(document)
    return document


def event_statement(policy: dict[str, Any], sequence: int, previous: str | None, result: dict[str, Any], signed_submission: dict[str, Any]) -> dict[str, Any]:
    return identified({
        "kind": "genesis/genesisbench-registry-event-statement-v0.1", "version": "0.1.0", "registryId": policy["registryId"],
        "policyIdentitySha256": policy["contentIdentitySha256"], "sequence": sequence, "previousEventIdentitySha256": previous,
        "resultIdentitySha256": result["contentIdentitySha256"], "submissionIdentitySha256": signed_submission["contentIdentitySha256"],
        "bundleSha256": result["submission"]["bundleSha256"], "decision": result["outcome"]["admissionDecision"], "contentIdentitySha256": "",
    })


def directory_files(path: Path, label: str) -> list[Path]:
    require(path.is_dir() and not path.is_symlink(), f"{label} must be a regular directory")
    entries = sorted(path.iterdir(), key=lambda row: row.name)
    require(all(row.is_file() and not row.is_symlink() for row in entries), f"{label} contains a non-regular entry")
    return entries


def scan_registry(
    root_path: Path,
    helper: Path,
    *,
    genesis_bin: Path | None = None,
    selfhost_artifact: Path | None = None,
    require_latest_checkpoint: bool = True,
) -> tuple[dict[str, Any], dict[str, Any], list[dict[str, Any]], list[dict[str, Any]]]:
    root = validate_root_path(root_path)
    allowed_root = {"authority", "checkpoints", "events", "objects", "registry.json", ".writer.lock"}
    require({row.name for row in root.iterdir()} <= allowed_root and {"authority", "checkpoints", "events", "objects", "registry.json"} <= {row.name for row in root.iterdir()}, "registry root topology is not closed")
    lock_path = root / ".writer.lock"
    require(not lock_path.exists() or (lock_path.is_file() and not lock_path.is_symlink()), "registry writer lock must be a regular non-symlink file")
    require([row.name for row in directory_files(root / "authority", "registry authority directory")] == ["policy.json"], "registry authority directory drift")
    objects_root = root / "objects"
    require(objects_root.is_dir() and not objects_root.is_symlink(), "registry object store must be a regular directory")
    require({row.name for row in objects_root.iterdir()} == {"bundles", "results", "submissions"}, "registry object-store topology is not closed")
    require(all(row.is_dir() and not row.is_symlink() for row in objects_root.iterdir()), "registry object store contains a non-regular directory")
    descriptor = load_stored_json(root / "registry.json", "registry descriptor")
    validate_identity(descriptor)
    policy = validate_policy(load_stored_json(root / "authority/policy.json", "registry policy"))
    require(descriptor == registry_descriptor(policy), "registry descriptor drift")
    events: list[dict[str, Any]] = []
    signed_events: list[dict[str, Any]] = []
    results: list[dict[str, Any]] = []
    previous = None
    for index, path in enumerate(list_events(root), 1):
        signed = load_stored_json(path, "registry event")
        statement = verify_signed(helper, signed, policy["operator"], EVENT_PAYLOAD_TYPE, "genesis/genesisbench-signed-event-v0.1")
        expected_fields = {"kind", "version", "registryId", "policyIdentitySha256", "sequence", "previousEventIdentitySha256", "resultIdentitySha256", "submissionIdentitySha256", "bundleSha256", "decision", "contentIdentitySha256"}
        closed(statement, expected_fields, "event statement"); validate_identity(statement)
        require(statement["sequence"] == index and statement["previousEventIdentitySha256"] == previous, "event chain sequence or predecessor mismatch")
        require(path.name == f"{index:012d}-{statement['contentIdentitySha256']}.json", "event filename is not content-addressed")
        result_path = root / f"objects/results/{statement['resultIdentitySha256']}.json"
        result = validate_result(load_stored_json(result_path, "registry result"), policy)
        require(result["contentIdentitySha256"] == statement["resultIdentitySha256"] and result["outcome"]["admissionDecision"] == statement["decision"], "event result binding drift")
        submission_path = root / f"objects/submissions/{statement['submissionIdentitySha256']}.json"
        signed_submission = load_stored_json(submission_path, "stored submission")
        submitter = next((row for row in policy["submitters"] if row["id"] == result["submission"]["submitterId"]), None)
        require(submitter is not None, "result submitter is not pinned by policy")
        submission_statement = verify_signed(helper, signed_submission, submitter, SUBMISSION_PAYLOAD_TYPE, "genesis/genesisbench-signed-submission-v0.1")
        validate_submission_statement(submission_statement)
        require(submission_statement["contentIdentitySha256"] == result["submission"]["identitySha256"] and signed_submission["contentIdentitySha256"] == statement["submissionIdentitySha256"], "submission binding drift")
        bundle_path = root / f"objects/bundles/{statement['bundleSha256']}.gcbundle"
        require(bundle_path.is_file() and not bundle_path.is_symlink() and sha256_file(bundle_path) == statement["bundleSha256"], "stored bundle identity mismatch")
        events.append(statement); results.append(result); previous = statement["contentIdentitySha256"]
        signed_events.append(signed)
        if genesis_bin is not None or selfhost_artifact is not None:
            require(genesis_bin is not None and selfhost_artifact is not None, "deep registry verification requires both runtime paths")
            with tempfile.TemporaryDirectory(prefix="genesisbench-registry-rederive-") as temporary:
                run_root = Path(temporary) / "run"
                extract_bundle(bundle_path, run_root)
                rescore = front_door.replay_run(run_root / "run.json", genesis_bin, selfhost_artifact)
                require(derive_result(policy, submission_statement, run_root, rescore) == result, "stored result differs from independent bundle rederivation")
    checkpoints = list_checkpoints(root)
    require(len(checkpoints) >= 1, "registry has no signed checkpoint")
    checkpoint_statements: list[dict[str, Any]] = []
    for index, path in enumerate(checkpoints):
        signed = load_stored_json(path, "registry checkpoint")
        statement = verify_signed(helper, signed, policy["operator"], CHECKPOINT_PAYLOAD_TYPE, "genesis/genesisbench-signed-checkpoint-v0.1")
        validate_identity(statement)
        require(isinstance(statement.get("sequence"), int) and not isinstance(statement["sequence"], bool) and 0 <= statement["sequence"] <= len(events), "invalid checkpoint sequence")
        prefix = list(zip(signed_events[: statement["sequence"]], events[: statement["sequence"]]))
        require(statement == checkpoint_statement(policy, prefix), "checkpoint does not bind its exact signed event prefix")
        require(path.name == f"{statement['sequence']:012d}-{signed['contentIdentitySha256']}.json", "checkpoint filename is not content-addressed")
        checkpoint_statements.append(statement)
    require([row["sequence"] for row in checkpoint_statements] == list(range(len(checkpoint_statements))), "every retained event prefix must have exactly one checkpoint")
    if require_latest_checkpoint:
        require(checkpoint_statements[-1]["sequence"] == len(events), "latest registry prefix is not checkpointed")
    require(len({row["resultIdentitySha256"] for row in events}) == len(events), "result identity appears in multiple append events")
    expected_results = {f"{row['resultIdentitySha256']}.json" for row in events}
    expected_submissions = {f"{row['submissionIdentitySha256']}.json" for row in events}
    expected_bundles = {f"{row['bundleSha256']}.gcbundle" for row in events}
    require({row.name for row in directory_files(root / "objects/results", "result object store")} == expected_results, "result object store has missing or unreferenced history")
    require({row.name for row in directory_files(root / "objects/submissions", "submission object store")} == expected_submissions, "submission object store has missing or unreferenced history")
    require({row.name for row in directory_files(root / "objects/bundles", "bundle object store")} == expected_bundles, "bundle object store has missing or unreferenced history")
    return descriptor, policy, events, results


class RegistryLock:
    def __init__(self, root: Path):
        self.path = root / ".writer.lock"
        self.handle: Any = None

    def __enter__(self) -> "RegistryLock":
        self.handle = self.path.open("a+b")
        if fcntl is not None:
            fcntl.flock(self.handle.fileno(), fcntl.LOCK_EX)
        else:
            if self.handle.seek(0, os.SEEK_END) == 0:
                self.handle.write(b"\0")
                self.handle.flush()
            self.handle.seek(0)
            msvcrt.locking(self.handle.fileno(), msvcrt.LK_LOCK, 1)
        return self

    def __exit__(self, *_: object) -> None:
        if fcntl is not None:
            fcntl.flock(self.handle.fileno(), fcntl.LOCK_UN)
        else:
            self.handle.seek(0)
            msvcrt.locking(self.handle.fileno(), msvcrt.LK_UNLCK, 1)
        self.handle.close()


def add_checkpoint(root: Path, policy: dict[str, Any], events: list[tuple[dict[str, Any], dict[str, Any]]], operator_key: Path, helper: Path) -> dict[str, Any]:
    checkpoint = sign_statement(helper, checkpoint_statement(policy, events), operator_key, CHECKPOINT_PAYLOAD_TYPE, "genesis/genesisbench-signed-checkpoint-v0.1")
    require(checkpoint["envelope"]["signatures"][0]["keyid"] == policy["operator"]["keyId"], "operator key does not match registry policy")
    write_new(root / f"checkpoints/{len(events):012d}-{checkpoint['contentIdentitySha256']}.json", pretty(checkpoint), "checkpoint")
    return checkpoint


def admit(registry: Path, submission_path: Path, bundle: Path, operator_key: Path, helper: Path, genesis_bin: Path, selfhost_artifact: Path) -> dict[str, Any]:
    root = validate_root_path(registry)
    with RegistryLock(root):
        _, policy, events, results = scan_registry(root, helper)
        signed_submission = load_json(submission_path, "signed submission")
        preliminary = closed(signed_submission, {"kind", "version", "statement", "envelope", "contentIdentitySha256"}, "signed submission")
        submitter_id = preliminary.get("statement", {}).get("submitterId")
        submitter = next((row for row in policy["submitters"] if row["id"] == submitter_id), None)
        require(submitter is not None, "submitter is not pinned by registry policy")
        statement = verify_signed(helper, signed_submission, submitter, SUBMISSION_PAYLOAD_TYPE, "genesis/genesisbench-signed-submission-v0.1")
        validate_submission_statement(statement)
        require(statement["claim"]["registryPolicyIdentitySha256"] == policy["contentIdentitySha256"], "submission targets a different registry")
        require(bundle.is_file() and not bundle.is_symlink() and bundle.stat().st_size <= policy["admission"]["maxBundleBytes"] and sha256_file(bundle) == statement["bundle"]["sha256"], "submitted bundle bytes do not match the signed statement")
        manifest, _ = front_door.validate_bundle(bundle)
        require(manifest["contentIdentitySha256"] == statement["bundle"]["manifestIdentitySha256"] and manifest["runIdentitySha256"] == statement["bundle"]["runIdentitySha256"], "signed bundle bindings mismatch")
        with tempfile.TemporaryDirectory(prefix="genesisbench-admit-") as temporary:
            run_root = Path(temporary) / "run"
            extract_bundle(bundle, run_root)
            rescore = front_door.replay_run(run_root / "run.json", genesis_bin, selfhost_artifact)
            result = derive_result(policy, statement, run_root, rescore)
        validate_result(result, policy)
        existing = next((event for event in events if event["resultIdentitySha256"] == result["contentIdentitySha256"]), None)
        if existing is not None:
            return {"kind": "genesis/genesisbench-registry-admission-v0.1", "version": "0.1.0", "idempotent": True, "sequence": existing["sequence"], "resultIdentitySha256": result["contentIdentitySha256"], "decision": result["outcome"]["admissionDecision"], "checkpointIdentitySha256": list_checkpoints(root)[-1].stem.split("-", 1)[1]}
        require(all(row["submission"]["runIdentitySha256"] != result["submission"]["runIdentitySha256"] or row["contentIdentitySha256"] == result["contentIdentitySha256"] for row in results), "one immutable run identity cannot be rebound to a different registry result")
        write_new(root / f"objects/bundles/{statement['bundle']['sha256']}.gcbundle", bundle.read_bytes(), "bundle object")
        write_new(root / f"objects/submissions/{signed_submission['contentIdentitySha256']}.json", pretty(signed_submission), "submission object")
        write_new(root / f"objects/results/{result['contentIdentitySha256']}.json", pretty(result), "result object")
        event = event_statement(policy, len(events) + 1, events[-1]["contentIdentitySha256"] if events else None, result, signed_submission)
        signed_event = sign_statement(helper, event, operator_key, EVENT_PAYLOAD_TYPE, "genesis/genesisbench-signed-event-v0.1")
        require(signed_event["envelope"]["signatures"][0]["keyid"] == policy["operator"]["keyId"], "operator key does not match registry policy")
        write_new(root / f"events/{event['sequence']:012d}-{event['contentIdentitySha256']}.json", pretty(signed_event), "registry event")
        prior_signed_events = [load_stored_json(path, "registry event") for path in list_events(root)[:-1]]
        checkpoint = add_checkpoint(root, policy, list(zip(prior_signed_events, events)) + [(signed_event, event)], operator_key, helper)
        scan_registry(root, helper, genesis_bin=genesis_bin, selfhost_artifact=selfhost_artifact)
        return {"kind": "genesis/genesisbench-registry-admission-v0.1", "version": "0.1.0", "idempotent": False, "sequence": event["sequence"], "resultIdentitySha256": result["contentIdentitySha256"], "decision": result["outcome"]["admissionDecision"], "reasonCodes": result["outcome"]["reasonCodes"], "checkpointIdentitySha256": checkpoint["contentIdentitySha256"]}


def round_mean(values: list[int]) -> int | None:
    return None if not values else int((Decimal(sum(values)) / Decimal(len(values))).quantize(Decimal(1), rounding=ROUND_HALF_EVEN))


def summarize_system(system_id: str, rows: list[dict[str, Any]]) -> dict[str, Any]:
    expected = rows[0]["system"]["expectedLineageIds"]
    observed = [row["case"]["lineageId"] for row in rows]
    complete = sorted(observed) == expected and len(observed) == len(set(observed))
    solved = sum(row["outcome"]["verifiedSolve"] for row in rows)
    qualities = [row["score"]["qualityScoreBasisPoints"] for row in rows if row["outcome"]["verifiedSolve"]]
    rank_eligible = complete and all(row["outcome"]["admissionDecision"] == "ranked" for row in rows)
    denominator = len(expected)
    missing = sorted(set(expected) - set(observed))
    per_lineage = [{
        "lineageId": lineage,
        "resultIdentitySha256": next((row["contentIdentitySha256"] for row in rows if row["case"]["lineageId"] == lineage), None),
        "outcome": next((row["outcome"]["runOutcome"] for row in rows if row["case"]["lineageId"] == lineage), "missing"),
        "verifiedSolve": any(row["case"]["lineageId"] == lineage and row["outcome"]["verifiedSolve"] for row in rows),
    } for lineage in expected]
    per_class = []
    for task_class in sorted({row["case"]["taskClass"] for row in rows}):
        class_rows = [row for row in rows if row["case"]["taskClass"] == task_class]
        class_quality = [row["score"]["qualityScoreBasisPoints"] for row in class_rows if row["outcome"]["verifiedSolve"]]
        per_class.append({"taskClass": task_class, "observed": len(class_rows), "verifiedSolved": sum(row["outcome"]["verifiedSolve"] for row in class_rows), "conditionalQualityMeanBasisPoints": round_mean(class_quality)})
    return {
        "systemId": system_id, "model": rows[0]["model"], "track": rows[0]["track"], "completeEvaluationSet": complete,
        "rankEligible": rank_eligible, "expectedLineages": denominator, "observedLineages": len(set(observed)), "verifiedSolvedLineages": solved,
        "solveRateBasisPoints": int((Decimal(solved) * Decimal(10_000) / Decimal(denominator)).quantize(Decimal(1), rounding=ROUND_HALF_EVEN)),
        "solveRateWilson95BasisPoints": genesisbench_analysis.wilson(solved, denominator), "conditionalQualityDenominator": len(qualities),
        "conditionalQualityMeanBasisPoints": round_mean(qualities), "capabilityExcessUnits": sum(row["efficiency"]["capabilityExcessUnits"] for row in rows),
        "contextBytes": sum(row["efficiency"]["contextBytes"] for row in rows), "toolCalls": sum(row["efficiency"]["toolCalls"] for row in rows),
        "repairCalls": sum(row["efficiency"]["repairCalls"] for row in rows), "economics": [row["economics"] for row in rows],
        "resultIdentities": sorted(row["contentIdentitySha256"] for row in rows),
        "missingLineageIds": missing, "perLineage": per_lineage, "perClass": per_class,
    }


def publication(descriptor: dict[str, Any], policy: dict[str, Any], events: list[dict[str, Any]], results: list[dict[str, Any]], checkpoint: dict[str, Any]) -> dict[str, Any]:
    cohorts: list[dict[str, Any]] = []
    for cohort_id in sorted({row["cohort"]["contentIdentitySha256"] for row in results}):
        cohort_rows = [row for row in results if row["cohort"]["contentIdentitySha256"] == cohort_id]
        systems = []
        for system_id in sorted({row["system"]["id"] for row in cohort_rows}):
            systems.append(summarize_system(system_id, [row for row in cohort_rows if row["system"]["id"] == system_id]))
        eligible = [row for row in systems if row["rankEligible"]]
        eligible.sort(key=lambda row: (-row["verifiedSolvedLineages"], -(row["conditionalQualityMeanBasisPoints"] or 0), row["capabilityExcessUnits"], row["contextBytes"], row["toolCalls"], row["repairCalls"], row["systemId"]))
        previous_key = None; rank = 0
        for index, row in enumerate(eligible, 1):
            key = (row["verifiedSolvedLineages"], row["conditionalQualityMeanBasisPoints"], row["capabilityExcessUnits"], row["contextBytes"], row["toolCalls"], row["repairCalls"])
            if key != previous_key:
                rank = index
            row["rank"] = rank; previous_key = key
        cohorts.append({"cohort": cohort_rows[0]["cohort"], "rankedSystems": eligible, "unrankedSystems": [row for row in systems if not row["rankEligible"]], "rankingKeys": policy["ranking"]["lexicographicKeys"], "costLatencyAffectRank": False})
    result_rows = []
    for row in sorted(results, key=lambda item: item["contentIdentitySha256"]):
        result_rows.append({
            "resultIdentitySha256": row["contentIdentitySha256"], "systemId": row["system"]["id"], "cohortIdentitySha256": row["cohort"]["contentIdentitySha256"],
            "case": row["case"], "outcome": row["outcome"], "score": row["score"], "attempts": row["attempts"], "model": row["model"],
            "track": row["track"], "contamination": row["contamination"], "hardware": row["hardware"], "economics": row["economics"], "replay": row["replay"],
        })
    return identified({
        "kind": "genesis/genesisbench-leaderboard-v0.1", "version": "0.1.0",
        "registry": {"id": policy["registryId"], "identitySha256": descriptor["contentIdentitySha256"], "policyIdentitySha256": policy["contentIdentitySha256"], "protocolIdentitySha256": policy["protocol"]["identitySha256"]},
        "history": {"eventCount": len(events), "headEventStatementIdentitySha256": events[-1]["contentIdentitySha256"] if events else None, "headSignedEventIdentitySha256": checkpoint["statement"]["headEventIdentitySha256"], "signedEventLogIdentitySha256": checkpoint["statement"]["signedEventLogIdentitySha256"], "checkpointIdentitySha256": checkpoint["contentIdentitySha256"], "allEventStatementIdentities": [row["contentIdentitySha256"] for row in events], "historicalResultsMutable": False, "silentSuppressionAllowed": False},
        "cohorts": cohorts, "results": result_rows, "contentIdentitySha256": "",
    })


def html_publication(board: dict[str, Any]) -> bytes:
    rows = []
    for cohort in board["cohorts"]:
        cohort_id = cohort["cohort"]["contentIdentitySha256"]
        for system in cohort["rankedSystems"]:
            rows.append(f"<tr><td>{system['rank']}</td><td><code>{system['systemId'][:16]}</code></td><td>{system['verifiedSolvedLineages']}/{system['expectedLineages']}</td><td>{system['conditionalQualityMeanBasisPoints']}</td><td><code>{cohort_id[:16]}</code></td></tr>")
    table = "".join(rows) or '<tr><td colspan="5">No complete rank-eligible evaluation is admitted.</td></tr>'
    text = f'''<!doctype html><html lang="en"><head><meta charset="utf-8"><meta name="viewport" content="width=device-width,initial-scale=1"><title>GenesisBench Registry</title><style>:root{{--ink:#17201b;--paper:#f1eee4;--accent:#b8432f}}*{{box-sizing:border-box}}body{{margin:0;background:linear-gradient(135deg,#f8f5eb,var(--paper));color:var(--ink);font:17px Georgia,serif}}main{{max-width:1100px;margin:auto;padding:5rem 1.5rem}}h1{{font-size:clamp(3rem,8vw,7rem);line-height:.85;margin:0 0 2rem;letter-spacing:-.06em}}p{{max-width:70ch}}table{{width:100%;border-collapse:collapse;margin-top:3rem;background:#fff9}}th,td{{padding:1rem;border-bottom:1px solid #aaa8;text-align:left}}th{{color:var(--accent);text-transform:uppercase;font:700 .75rem ui-monospace,monospace;letter-spacing:.12em}}code{{font-family:ui-monospace,monospace}}.seal{{border-left:5px solid var(--accent);padding-left:1rem}}</style></head><body><main><p class="seal">Signed append-only registry | checkpoint <code>{board['history']['checkpointIdentitySha256'][:20]}</code></p><h1>Genesis<br>Bench</h1><p>Ranked only inside exact content-addressed cohorts. Solve rate leads; invalid partial quality and financial spend never affect rank. Every attempt, failure, abstention, model fact, and replay instruction remains in <a href="leaderboard.json">the canonical record</a>.</p><table><thead><tr><th>Rank</th><th>System</th><th>Solved</th><th>Quality</th><th>Cohort</th></tr></thead><tbody>{table}</tbody></table></main></body></html>'''
    return text.encode("ascii")


def build_publication(registry: Path, out: Path, helper: Path, genesis_bin: Path, selfhost_artifact: Path) -> dict[str, Any]:
    require(not out.exists() and not out.is_symlink(), "publication output already exists")
    descriptor, policy, events, results = scan_registry(registry, helper, genesis_bin=genesis_bin, selfhost_artifact=selfhost_artifact)
    latest_checkpoint = load_stored_json(list_checkpoints(registry)[-1], "latest checkpoint")
    board = publication(descriptor, policy, events, results, latest_checkpoint)
    temporary = Path(tempfile.mkdtemp(prefix=f".{out.name}.", dir=out.parent.resolve(strict=True)))
    try:
        (temporary / "results").mkdir()
        (temporary / "leaderboard.json").write_bytes(pretty(board))
        (temporary / "index.html").write_bytes(html_publication(board))
        (temporary / "replay.md").write_bytes(("# GenesisBench replay\n\nEvery result in `leaderboard.json` publishes its exact offline replay command. Extract the SHA-named `.gcbundle`, then execute that command with the protocol-pinned GenesisCode binary and self-host artifact. Model and adapter access are forbidden during replay.\n").encode("ascii"))
        for result in results:
            (temporary / f"results/{result['contentIdentitySha256']}.json").write_bytes(pretty(result))
        inventory = front_door.inventory_tree(temporary)
        manifest = identified({"kind": "genesis/genesisbench-static-publication-manifest-v0.1", "version": "0.1.0", "leaderboardIdentitySha256": board["contentIdentitySha256"], "artifacts": inventory, "artifactInventoryIdentitySha256": front_door.artifact_identity(inventory), "contentIdentitySha256": ""})
        (temporary / "publication-manifest.json").write_bytes(pretty(manifest))
        os.replace(temporary, out)
    except Exception:
        shutil.rmtree(temporary, ignore_errors=True)
        raise
    return {"kind": "genesis/genesisbench-registry-build-v0.1", "version": "0.1.0", "leaderboardIdentitySha256": board["contentIdentitySha256"], "publicationManifestIdentitySha256": manifest["contentIdentitySha256"], "events": len(events), "results": len(results), "cohorts": len(board["cohorts"])}


def verify_registry(registry: Path, helper: Path, genesis_bin: Path, selfhost_artifact: Path) -> dict[str, Any]:
    descriptor, policy, events, results = scan_registry(registry, helper, genesis_bin=genesis_bin, selfhost_artifact=selfhost_artifact)
    latest = load_stored_json(list_checkpoints(registry)[-1], "latest checkpoint")
    board = publication(descriptor, policy, events, results, latest)
    return {"kind": "genesis/genesisbench-registry-verification-v0.1", "version": "0.1.0", "verified": True, "registryIdentitySha256": descriptor["contentIdentitySha256"], "policyIdentitySha256": policy["contentIdentitySha256"], "events": len(events), "results": len(results), "checkpoints": len(list_checkpoints(registry)), "headEventStatementIdentitySha256": events[-1]["contentIdentitySha256"] if events else None, "headSignedEventIdentitySha256": latest["statement"]["headEventIdentitySha256"], "leaderboardIdentitySha256": board["contentIdentitySha256"], "historyComplete": True, "silentSuppressionDetected": False}


def check_authorities(*, write: bool) -> dict[str, Any]:
    authority = render_authority()
    schemas = render_schemas()
    if write:
        AUTHORITY_PATH.write_bytes(pretty(authority))
        for path, document in schemas.items():
            path.write_bytes(pretty(document))
    require(load_json(AUTHORITY_PATH, "registry authority") == authority, "registry authority drift; run --write")
    for path, document in schemas.items():
        require(load_json(path, f"schema {path.name}") == document, f"registry schema drift: {path.name}; run --write")
    return authority


def self_test() -> int:
    controls = 0
    authority = render_authority()
    require(authority["ranking"]["costLatencyAffectRank"] is False and authority["admission"]["adapterOrRunMaySelfRank"] is False, "authority anti-gaming invariant drift")
    controls += 2
    synthetic = []
    for system, solved, quality, capability, context, tools, repairs, cost in (("a", 8, 9000, 0, 100, 1, 0, 999999), ("b", 7, 10000, 0, 1, 0, 0, 0), ("c", 8, 9000, 1, 1, 0, 0, 0), ("d", 8, 9000, 0, 90, 1, 0, 999999)):
        synthetic.append({"systemId": system, "verifiedSolvedLineages": solved, "conditionalQualityMeanBasisPoints": quality, "capabilityExcessUnits": capability, "contextBytes": context, "toolCalls": tools, "repairCalls": repairs, "cost": cost})
    ordered = sorted(synthetic, key=lambda row: (-row["verifiedSolvedLineages"], -row["conditionalQualityMeanBasisPoints"], row["capabilityExcessUnits"], row["contextBytes"], row["toolCalls"], row["repairCalls"], row["systemId"]))
    require([row["systemId"] for row in ordered] == ["d", "a", "c", "b"], "lexicographic rank order drift")
    controls += 1
    changed_cost = copy.deepcopy(synthetic); changed_cost[0]["cost"] = 0; changed_cost[3]["cost"] = 2**63
    reordered = sorted(changed_cost, key=lambda row: (-row["verifiedSolvedLineages"], -row["conditionalQualityMeanBasisPoints"], row["capabilityExcessUnits"], row["contextBytes"], row["toolCalls"], row["repairCalls"], row["systemId"]))
    require([row["systemId"] for row in reordered] == [row["systemId"] for row in ordered], "financial spend affected rank")
    controls += 1
    for mutation in (
        lambda d: d["ranking"].__setitem__("costLatencyAffectRank", True),
        lambda d: d["admission"].__setitem__("adapterOrRunMaySelfRank", True),
        lambda d: d["history"].__setitem__("silentSuppressionAllowed", True),
        lambda d: d["ranking"]["lexicographicKeys"].reverse(),
    ):
        candidate = copy.deepcopy(authority); mutation(candidate)
        require(candidate != render_authority(), "authority mutation survived exact regeneration")
        controls += 1
    return controls


def parser() -> argparse.ArgumentParser:
    out = argparse.ArgumentParser(description=__doc__)
    commands = out.add_subparsers(dest="command", required=True)
    check = commands.add_parser("check"); check.add_argument("--write", action="store_true"); check.add_argument("--self-test", action="store_true")
    submit_cmd = commands.add_parser("submit"); submit_cmd.add_argument("--bundle", type=Path, required=True); submit_cmd.add_argument("--claim", type=Path, required=True); submit_cmd.add_argument("--outbox", type=Path, required=True); submit_cmd.add_argument("--submitter", required=True); submit_cmd.add_argument("--key", type=Path, required=True); submit_cmd.add_argument("--crypto-helper", type=Path, required=True)
    init = commands.add_parser("init"); init.add_argument("--registry", type=Path, required=True); init.add_argument("--policy", type=Path, required=True); init.add_argument("--operator-key", type=Path, required=True); init.add_argument("--crypto-helper", type=Path, required=True)
    admit_cmd = commands.add_parser("admit"); admit_cmd.add_argument("--registry", type=Path, required=True); admit_cmd.add_argument("--submission", type=Path, required=True); admit_cmd.add_argument("--bundle", type=Path, required=True); admit_cmd.add_argument("--operator-key", type=Path, required=True); admit_cmd.add_argument("--crypto-helper", type=Path, required=True); admit_cmd.add_argument("--genesis-bin", type=Path, required=True); admit_cmd.add_argument("--selfhost-artifact", type=Path, required=True)
    verify = commands.add_parser("verify"); verify.add_argument("--registry", type=Path, required=True); verify.add_argument("--crypto-helper", type=Path, required=True); verify.add_argument("--genesis-bin", type=Path, required=True); verify.add_argument("--selfhost-artifact", type=Path, required=True)
    build = commands.add_parser("build"); build.add_argument("--registry", type=Path, required=True); build.add_argument("--out", type=Path, required=True); build.add_argument("--crypto-helper", type=Path, required=True); build.add_argument("--genesis-bin", type=Path, required=True); build.add_argument("--selfhost-artifact", type=Path, required=True)
    return out


def main() -> int:
    args = parser().parse_args()
    if args.command == "check":
        authority = check_authorities(write=args.write)
        controls = self_test() if args.self_test else 0
        print(json.dumps({"kind": "genesis/genesisbench-registry-authority-check-v0.1", "version": "0.1.0", "authorityIdentitySha256": authority["contentIdentitySha256"], "schemas": len(render_schemas()), "controls": controls}, sort_keys=True, separators=(",", ":")))
    elif args.command == "submit":
        print(json.dumps(submit(args.bundle, args.claim, args.outbox, args.submitter, args.key, args.crypto_helper), sort_keys=True, separators=(",", ":")))
    elif args.command == "init":
        print(json.dumps(init_registry(args.registry, args.policy, args.operator_key, args.crypto_helper), sort_keys=True, separators=(",", ":")))
    elif args.command == "admit":
        print(json.dumps(admit(args.registry, args.submission, args.bundle, args.operator_key, args.crypto_helper, args.genesis_bin, args.selfhost_artifact), sort_keys=True, separators=(",", ":")))
    elif args.command == "verify":
        print(json.dumps(verify_registry(args.registry, args.crypto_helper, args.genesis_bin, args.selfhost_artifact), sort_keys=True, separators=(",", ":")))
    elif args.command == "build":
        print(json.dumps(build_publication(args.registry, args.out, args.crypto_helper, args.genesis_bin, args.selfhost_artifact), sort_keys=True, separators=(",", ":")))
    return 0


if __name__ == "__main__":
    try:
        raise SystemExit(main())
    except (RegistryError, front_door.FrontDoorError, OSError, subprocess.SubprocessError, KeyError, TypeError, ValueError) as error:
        print(json.dumps({"kind": "genesis/genesisbench-registry-error-v0.1", "message": str(error)}, sort_keys=True, separators=(",", ":")), file=sys.stderr)
        raise SystemExit(2)
