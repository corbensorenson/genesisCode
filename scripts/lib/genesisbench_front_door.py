#!/usr/bin/env python3
"""Canonical GenesisBench execution, validation, replay, bundle, and submission front door."""

from __future__ import annotations

import argparse
import base64
import copy
import gzip
import hashlib
import io
import json
import os
import re
import shutil
import signal
import http.server
import subprocess
import sys
import tarfile
import tempfile
import threading
import time
import urllib.error
import urllib.parse
import urllib.request
from pathlib import Path
from typing import Any, Callable

import gc_agent_scoring
import genesisbench_open_agent
import genesisbench_reference_agent


ROOT = Path(__file__).resolve().parents[2]
SUITE_PATH = ROOT / "benchmarks/agent_tasks/v0.1/suite.json"
SCORING_PATH = ROOT / "docs/spec/GC_AGENT_BENCHMARK_SCORING_v0.1.json"
PROFILE_PATH = ROOT / "docs/spec/GENESISBENCH_ADAPTERS_v0.1.json"
PROFILE_SCHEMA_PATH = ROOT / "docs/spec/GENESISBENCH_ADAPTERS_v0.1.schema.json"
ADAPTER_SCHEMA_PATH = ROOT / "docs/spec/GENESISBENCH_ADAPTER_v0.1.schema.json"
REQUEST_SCHEMA_PATH = ROOT / "docs/spec/GENESISBENCH_ADAPTER_REQUEST_v0.1.schema.json"
RESPONSE_SCHEMA_PATH = ROOT / "docs/spec/GENESISBENCH_ADAPTER_RESPONSE_v0.1.schema.json"
RUN_SCHEMA_PATH = ROOT / "docs/spec/GENESISBENCH_EXECUTION_RUN_v0.1.schema.json"
BUNDLE_SCHEMA_PATH = ROOT / "docs/spec/GENESISBENCH_BUNDLE_MANIFEST_v0.1.schema.json"
FIXTURE_ROOT = ROOT / "benchmarks/genesisbench/v0.1/adapters"
FIXTURE_TOOL = FIXTURE_ROOT / "command_fixture.py"
SHA_RE = re.compile(r"^[0-9a-f]{64}$")
ID_RE = re.compile(r"^[A-Za-z0-9][A-Za-z0-9._-]{0,127}$")
ENV_RE = re.compile(r"^[A-Z][A-Z0-9_]{0,127}$")
REL_RE = re.compile(r"^(?!/)(?!.*(?:^|/)\.\.(?:/|$))[A-Za-z0-9._/-]{1,512}$")
ADAPTER_CLASSES = (
    "hosted-api",
    "local-openai-compatible",
    "direct-local-runtime",
    "command-plugin",
    "deterministic-mock",
)
KIND_REQUEST = "genesis/genesisbench-adapter-request-v0.1"
KIND_RESPONSE = "genesis/genesisbench-adapter-response-v0.1"
KIND_RUN = "genesis/genesisbench-execution-run-v0.1"
KIND_BUNDLE = "genesis/genesisbench-bundle-manifest-v0.1"
KIND_SUBMISSION = "genesis/genesisbench-submission-v0.1"
MAX_CAPTURE_BYTES = 16 * 1024 * 1024


class FrontDoorError(RuntimeError):
    pass


def require(condition: bool, message: str) -> None:
    if not condition:
        raise FrontDoorError(message)


def canonical_bytes(value: Any) -> bytes:
    return json.dumps(value, sort_keys=True, separators=(",", ":"), ensure_ascii=True).encode("ascii")


def pretty_bytes(value: Any) -> bytes:
    return (json.dumps(value, sort_keys=True, indent=2, ensure_ascii=True) + "\n").encode("ascii")


def sha256_bytes(value: bytes) -> str:
    return hashlib.sha256(value).hexdigest()


def sha256_file(path: Path) -> str:
    hasher = hashlib.sha256()
    with path.open("rb") as stream:
        while chunk := stream.read(1024 * 1024):
            hasher.update(chunk)
    return hasher.hexdigest()


def identified(value: dict[str, Any], field: str = "contentIdentitySha256") -> dict[str, Any]:
    out = copy.deepcopy(value)
    out[field] = ""
    out[field] = sha256_bytes(canonical_bytes(out))
    return out


def validate_identity(value: dict[str, Any], field: str = "contentIdentitySha256") -> None:
    require(isinstance(value.get(field), str) and SHA_RE.fullmatch(value[field]) is not None, f"invalid {field}")
    require(identified(value, field) == value, f"{field} mismatch")


def load_json(path: Path) -> Any:
    with path.open("r", encoding="ascii") as stream:
        return json.load(stream)


def write_json(path: Path, value: Any) -> None:
    path.parent.mkdir(parents=True, exist_ok=True)
    path.write_bytes(pretty_bytes(value))


def closed(value: Any, fields: set[str], label: str) -> dict[str, Any]:
    require(isinstance(value, dict), f"{label} must be an object")
    require(set(value) == fields, f"{label} fields are not closed")
    return value


def relative_path(value: Any, label: str) -> str:
    require(isinstance(value, str) and REL_RE.fullmatch(value) is not None, f"invalid {label}")
    require("//" not in value and not value.endswith("/"), f"non-canonical {label}")
    return value


def regular_file(path: Path, label: str) -> Path:
    require(path.is_file() and not path.is_symlink(), f"{label} must be a regular non-symlink file")
    return path.resolve(strict=True)


def regular_directory(path: Path, label: str) -> Path:
    require(path.is_dir() and not path.is_symlink(), f"{label} must be a regular non-symlink directory")
    return path.resolve(strict=True)


def inventory_tree(root: Path, *, max_files: int = 4096, max_bytes: int = 64 * 1024 * 1024) -> list[dict[str, Any]]:
    root = regular_directory(root, "inventory root")
    rows: list[dict[str, Any]] = []
    total = 0
    for path in sorted(root.rglob("*"), key=lambda p: p.relative_to(root).as_posix()):
        require(not path.is_symlink(), f"symlink forbidden in inventory: {path.relative_to(root)}")
        if path.is_dir():
            continue
        require(path.is_file(), f"non-regular artifact forbidden: {path.relative_to(root)}")
        rel = relative_path(path.relative_to(root).as_posix(), "artifact path")
        size = path.stat().st_size
        total += size
        require(len(rows) < max_files and total <= max_bytes, "artifact inventory exceeds finite limits")
        rows.append({"path": rel, "bytes": size, "sha256": sha256_file(path)})
    return rows


def artifact_identity(rows: list[dict[str, Any]]) -> str:
    return sha256_bytes(canonical_bytes(rows))


def base_schema(title: str, kind: str, properties: dict[str, Any], required: list[str]) -> dict[str, Any]:
    return {
        "$schema": "https://json-schema.org/draft/2020-12/schema",
        "$id": f"https://genesiscode.dev/schemas/{kind}-v0.1.json",
        "title": title,
        "type": "object",
        "additionalProperties": False,
        "required": required,
        "properties": properties,
        "$defs": {
            "hash": {"type": "string", "pattern": "^[0-9a-f]{64}$"},
            "id": {"type": "string", "pattern": "^[A-Za-z0-9][A-Za-z0-9._-]{0,127}$"},
            "path": {"type": "string", "pattern": "^(?!/)(?!.*(?:^|/)\\.\\.(?:/|$))[A-Za-z0-9._/-]{1,512}$"},
            "artifact": {
                "type": "object",
                "additionalProperties": False,
                "required": ["path", "bytes", "sha256"],
                "properties": {
                    "path": {"$ref": "#/$defs/path"},
                    "bytes": {"type": "integer", "minimum": 0, "maximum": 67108864},
                    "sha256": {"$ref": "#/$defs/hash"},
                },
            },
        },
    }


def render_schemas() -> dict[Path, dict[str, Any]]:
    model_schema = {
        "type": "object",
        "additionalProperties": False,
        "required": ["id", "revision", "immutable", "artifactSha256"],
        "properties": {
            "id": {"$ref": "#/$defs/id"},
            "revision": {"$ref": "#/$defs/id"},
            "immutable": {"const": True},
            "artifactSha256": {"oneOf": [{"$ref": "#/$defs/hash"}, {"type": "null"}]},
        },
    }
    authority_schema = {
        "type": "object",
        "additionalProperties": False,
        "required": ["network", "filesystem", "environment", "subprocess", "requestMapping", "responseMapping", "providerToolsAllowed", "hiddenRetriesAllowed", "secretPolicy"],
        "properties": {
            "network": {"enum": ["one-exact-https-origin-and-path", "one-exact-loopback-origin-and-path", "none", "none-by-contract"]},
            "filesystem": {"enum": ["none", "digest-bound-executable-and-model-artifact-read-only", "digest-bound-executable-read-only-plus-empty-temporary-cwd"]},
            "environment": {"enum": ["one-declared-secret-name", "none-or-one-declared-secret-name", "closed-runtime-locator-names", "closed-empty", "none"]},
            "subprocess": {"enum": ["none", "one-process-group"]},
            "requestMapping": {"const": "typed-lossless-no-authority-injection"},
            "responseMapping": {"const": "typed-lossless-no-semantic-rewrite"},
            "providerToolsAllowed": {"const": False},
            "hiddenRetriesAllowed": {"const": False},
            "secretPolicy": {"const": "redact-values-record-name-presence-and-policy"},
        },
    }
    http_transport = {
        "type": "object", "additionalProperties": False,
        "required": ["protocol", "origin", "path", "redirects", "secretEnvName", "timeoutMs", "maxOutputBytes", "mutableFacts"],
        "properties": {
            "protocol": {"const": "openai-chat-completions-v1"}, "origin": {"type": "string", "maxLength": 2048},
            "path": {"type": "string", "pattern": "^/", "maxLength": 1024}, "redirects": {"const": "deny"},
            "secretEnvName": {"oneOf": [{"type": "string", "pattern": "^[A-Z][A-Z0-9_]{0,127}$"}, {"type": "null"}]},
            "timeoutMs": {"type": "integer", "minimum": 1, "maximum": 3600000},
            "maxOutputBytes": {"type": "integer", "minimum": 1, "maximum": MAX_CAPTURE_BYTES},
            "mutableFacts": {"type": "array", "maxItems": 32, "uniqueItems": True, "items": {"$ref": "#/$defs/id"}},
        },
    }
    process_properties = {
        "protocol": {"const": "json-stdio-v0.1"}, "executableSha256": {"$ref": "#/$defs/hash"},
        "argv": {"type": "array", "maxItems": 32, "items": {"type": "string", "maxLength": 1024}},
        "timeoutMs": {"type": "integer", "minimum": 1, "maximum": 3600000},
        "maxOutputBytes": {"type": "integer", "minimum": 1, "maximum": MAX_CAPTURE_BYTES},
        "mutableFacts": {"type": "array", "maxItems": 32, "uniqueItems": True, "items": {"$ref": "#/$defs/id"}},
    }
    command_transport = {"type": "object", "additionalProperties": False, "required": list(process_properties), "properties": process_properties}
    direct_transport = copy.deepcopy(command_transport)
    direct_transport["required"] = [*direct_transport["required"], "modelArtifactSha256"]
    direct_transport["properties"]["modelArtifactSha256"] = {"$ref": "#/$defs/hash"}
    mock_transport = {
        "type": "object", "additionalProperties": False, "required": ["protocol", "fixtureId", "mutableFacts"],
        "properties": {"protocol": {"const": "request-hash-fixture-v0.1"}, "fixtureId": {"$ref": "#/$defs/id"}, "mutableFacts": {"type": "array", "maxItems": 32, "uniqueItems": True, "items": {"$ref": "#/$defs/id"}}},
    }
    adapter = base_schema(
        "GenesisBench capability-minimal adapter v0.1",
        "genesisbench-adapter",
        {
            "kind": {"const": "genesis/genesisbench-adapter-v0.1"},
            "version": {"const": "0.1.0"},
            "id": {"$ref": "#/$defs/id"},
            "class": {"enum": list(ADAPTER_CLASSES)},
            "model": model_schema,
            "transport": {"oneOf": [http_transport, direct_transport, command_transport, mock_transport]},
            "authority": authority_schema,
            "requestMapping": {"const": "typed-lossless-no-authority-injection"},
            "responseMapping": {"const": "typed-lossless-no-semantic-rewrite"},
            "providerToolsAllowed": {"const": False},
            "hiddenRetriesAllowed": {"const": False},
            "secretPolicy": {"const": "redact-values-record-name-presence-and-policy"},
            "rankEligible": {"const": False},
            "contentIdentitySha256": {"$ref": "#/$defs/hash"},
        },
        ["kind", "version", "id", "class", "model", "transport", "authority", "requestMapping", "responseMapping", "providerToolsAllowed", "hiddenRetriesAllowed", "secretPolicy", "rankEligible", "contentIdentitySha256"],
    )
    profile = base_schema(
        "GenesisBench closed adapter profile v0.1", "genesisbench-adapters-profile",
        {
            "kind": {"const": "genesis/genesisbench-adapters-profile-v0.1"}, "version": {"const": "0.1.0"},
            "classes": {"const": list(ADAPTER_CLASSES)},
            "adapters": {"type": "array", "minItems": 5, "maxItems": 5, "items": {"$ref": "https://genesiscode.dev/schemas/genesisbench-adapter-v0.1.json"}},
            "normalization": {"type": "object", "additionalProperties": False, "required": ["request", "response", "semanticRewriteAllowed", "allAttemptsRetained"], "properties": {"request": {"const": KIND_REQUEST}, "response": {"const": KIND_RESPONSE}, "semanticRewriteAllowed": {"const": False}, "allAttemptsRetained": {"const": True}}},
            "capabilityPolicy": {"type": "object", "additionalProperties": False, "required": ["ambientAuthorityAllowed", "providerToolsAllowed", "hiddenRetriesAllowed", "exactDeclaredAuthorityOnly"], "properties": {"ambientAuthorityAllowed": {"const": False}, "providerToolsAllowed": {"const": False}, "hiddenRetriesAllowed": {"const": False}, "exactDeclaredAuthorityOnly": {"const": True}}},
            "conformance": {"type": "object", "additionalProperties": False, "required": ["sameRequestResponseVectors", "timeoutHardKillAndReap", "cancellationHardKillAndReap", "replayReinvocationAllowed"], "properties": {"sameRequestResponseVectors": {"const": True}, "timeoutHardKillAndReap": {"const": True}, "cancellationHardKillAndReap": {"const": True}, "replayReinvocationAllowed": {"const": False}}},
            "contentIdentitySha256": {"$ref": "#/$defs/hash"},
        },
        ["kind", "version", "classes", "adapters", "normalization", "capabilityPolicy", "conformance", "contentIdentitySha256"],
    )
    message_schema = {
        "type": "object",
        "additionalProperties": False,
        "required": ["role", "content", "contentSha256"],
        "properties": {
            "role": {"enum": genesisbench_reference_agent.PROMPT_ROLES},
            "content": {"type": "string", "maxLength": 8388608},
            "contentSha256": {"$ref": "#/$defs/hash"},
        },
    }
    output_contract_schema = {
        "type": "object",
        "additionalProperties": False,
        "required": ["kind", "candidateFiles", "encoding", "semanticRewriteAllowed"],
        "properties": {
            "kind": {"const": KIND_RESPONSE},
            "candidateFiles": {
                "type": "array", "maxItems": 128, "uniqueItems": True,
                "items": {"$ref": "#/$defs/path"},
            },
            "encoding": {"const": "base64-bytes"},
            "semanticRewriteAllowed": {"const": False},
        },
    }
    sampling_schema = {
        "type": "object",
        "additionalProperties": False,
        "required": ["attempt", "providerTools", "hiddenRetries"],
        "properties": {
            "attempt": {"type": "integer", "minimum": 0, "maximum": 1},
            "providerTools": {"const": False},
            "hiddenRetries": {"const": 0},
        },
    }
    request = base_schema(
        "GenesisBench normalized adapter request v0.1",
        "genesisbench-adapter-request",
        {
            "kind": {"const": KIND_REQUEST},
            "version": {"const": "0.1.0"},
            "requestId": {"$ref": "#/$defs/id"},
            "caseId": {"$ref": "#/$defs/id"},
            "planIdentitySha256": {"$ref": "#/$defs/hash"},
            "model": model_schema,
            "messages": {"type": "array", "minItems": 1, "maxItems": 16, "items": message_schema},
            "outputContract": output_contract_schema,
            "tools": {"const": []},
            "sampling": sampling_schema,
            "contentIdentitySha256": {"$ref": "#/$defs/hash"},
        },
        ["kind", "version", "requestId", "caseId", "planIdentitySha256", "model", "messages", "outputContract", "tools", "sampling", "contentIdentitySha256"],
    )
    candidate_file_schema = {
        "type": "object", "additionalProperties": False,
        "required": ["path", "contentBase64"],
        "properties": {
            "path": {"$ref": "#/$defs/path"},
            "contentBase64": {"type": "string", "contentEncoding": "base64", "maxLength": 11184812},
        },
    }
    usage_schema = {
        "oneOf": [
            {
                "type": "object", "additionalProperties": False,
                "required": ["inputTokens", "outputTokens"],
                "properties": {
                    "inputTokens": {"type": "integer", "minimum": 0},
                    "outputTokens": {"type": "integer", "minimum": 0},
                },
            },
            {"type": "object", "additionalProperties": False, "required": [], "properties": {}},
        ],
    }
    nullable_fact = {"type": ["string", "integer", "boolean", "null"]}
    provider_fact_schemas = []
    for names in [
        [],
        ["fixture-id"],
        ["runtime-build"],
        ["provider-request-id", "service-tier", "system-fingerprint"],
        ["provider-request-id", "service-tier", "server-build", "system-fingerprint"],
    ]:
        provider_fact_schemas.append({
            "type": "object", "additionalProperties": False,
            "required": names, "properties": {name: nullable_fact for name in names},
        })
    error_schema = {
        "type": "object", "additionalProperties": False,
        "required": ["code", "message"],
        "properties": {
            "code": {"type": "string", "pattern": "^[A-Za-z0-9][A-Za-z0-9._/-]{0,127}$"},
            "message": {"type": "string", "minLength": 1, "maxLength": 1024},
        },
    }
    response = base_schema(
        "GenesisBench normalized adapter response v0.1",
        "genesisbench-adapter-response",
        {
            "kind": {"const": KIND_RESPONSE},
            "version": {"const": "0.1.0"},
            "requestIdentitySha256": {"$ref": "#/$defs/hash"},
            "status": {"enum": ["succeeded", "failed", "cancelled", "timed-out"]},
            "finishReason": {"type": "string", "minLength": 1, "maxLength": 128},
            "candidateFiles": {"type": "array", "maxItems": 128, "items": candidate_file_schema},
            "usage": usage_schema,
            "providerFacts": {"oneOf": provider_fact_schemas},
            "error": {"oneOf": [error_schema, {"type": "null"}]},
            "contentIdentitySha256": {"$ref": "#/$defs/hash"},
        },
        ["kind", "version", "requestIdentitySha256", "status", "finishReason", "candidateFiles", "usage", "providerFacts", "error", "contentIdentitySha256"],
    )
    case_schema = {
        "type": "object", "additionalProperties": False,
        "required": ["id", "lineageId", "lineageIdentitySha256", "conditionId", "conditionIdentitySha256"],
        "properties": {
            "id": {"$ref": "#/$defs/id"}, "lineageId": {"$ref": "#/$defs/id"},
            "lineageIdentitySha256": {"$ref": "#/$defs/hash"}, "conditionId": {"$ref": "#/$defs/id"},
            "conditionIdentitySha256": {"$ref": "#/$defs/hash"},
        },
    }
    reference_binding_schema = {
        "type": "object", "additionalProperties": False,
        "required": ["planArtifact", "planIdentitySha256", "profileIdentitySha256"],
        "properties": {
            "planArtifact": {"const": "plan.json"}, "planIdentitySha256": {"$ref": "#/$defs/hash"},
            "profileIdentitySha256": {"$ref": "#/$defs/hash"},
        },
    }
    adapter_binding_schema = {
        "type": "object", "additionalProperties": False,
        "required": ["artifact", "id", "class", "identitySha256", "rankEligible"],
        "properties": {
            "artifact": {"const": "adapter.json"}, "id": {"$ref": "#/$defs/id"},
            "class": {"enum": list(ADAPTER_CLASSES)}, "identitySha256": {"$ref": "#/$defs/hash"},
            "rankEligible": {"const": False},
        },
    }
    secret_disclosure_schema = {
        "type": "object", "additionalProperties": False,
        "required": ["valuesRecorded", "declaredNames", "presentNames", "presenceRecorded"],
        "properties": {
            "valuesRecorded": {"const": False},
            "declaredNames": {"type": "array", "maxItems": 1, "uniqueItems": True, "items": {"$ref": "#/$defs/id"}},
            "presentNames": {"type": "array", "maxItems": 1, "uniqueItems": True, "items": {"$ref": "#/$defs/id"}},
            "presenceRecorded": {"const": True},
        },
    }
    attempt_schema = {
        "type": "object", "additionalProperties": False,
        "required": ["index", "requestArtifact", "requestIdentitySha256", "responseArtifact", "responseIdentitySha256", "status", "elapsedMs", "secretDisclosure"],
        "properties": {
            "index": {"type": "integer", "minimum": 0, "maximum": 1},
            "requestArtifact": {"$ref": "#/$defs/path"}, "requestIdentitySha256": {"$ref": "#/$defs/hash"},
            "responseArtifact": {"$ref": "#/$defs/path"}, "responseIdentitySha256": {"$ref": "#/$defs/hash"},
            "status": {"enum": ["succeeded", "failed", "cancelled", "timed-out"]},
            "elapsedMs": {"type": "integer", "minimum": 0, "maximum": 86400000},
            "secretDisclosure": secret_disclosure_schema,
        },
    }
    replay_schema = {
        "type": "object", "additionalProperties": False,
        "required": ["adapterReinvocationAllowed", "modelAccessAllowed", "requestResponseValidation", "independentRescoreRequired"],
        "properties": {
            "adapterReinvocationAllowed": {"const": False}, "modelAccessAllowed": {"const": False},
            "requestResponseValidation": {"const": "strict-all-fields"},
            "independentRescoreRequired": {"type": "boolean"},
        },
    }
    run = base_schema(
        "GenesisBench canonical execution run v0.1",
        "genesisbench-execution-run",
        {
            "kind": {"const": KIND_RUN}, "version": {"const": "0.1.0"},
            "case": case_schema, "referenceAgent": reference_binding_schema, "adapter": adapter_binding_schema,
            "attempts": {"type": "array", "minItems": 1, "maxItems": 2, "items": attempt_schema},
            "outcome": {"enum": ["verified", "failed", "invalid", "abstained"]},
            "candidateInventory": {"type": "array", "maxItems": 4096, "items": {"$ref": "#/$defs/artifact"}},
            "candidateInventoryIdentitySha256": {"$ref": "#/$defs/hash"},
            "scoreIdentitySha256": {"oneOf": [{"$ref": "#/$defs/hash"}, {"type": "null"}]},
            "artifactInventory": {"type": "array", "maxItems": 4096, "items": {"$ref": "#/$defs/artifact"}},
            "artifactInventoryIdentitySha256": {"$ref": "#/$defs/hash"},
            "replay": replay_schema, "contentIdentitySha256": {"$ref": "#/$defs/hash"},
        },
        ["kind", "version", "case", "referenceAgent", "adapter", "attempts", "outcome", "candidateInventory", "candidateInventoryIdentitySha256", "scoreIdentitySha256", "artifactInventory", "artifactInventoryIdentitySha256", "replay", "contentIdentitySha256"],
    )
    bundle = base_schema(
        "GenesisBench deterministic run bundle manifest v0.1", "genesisbench-bundle-manifest",
        {
            "kind": {"const": KIND_BUNDLE}, "version": {"const": "0.1.0"},
            "runIdentitySha256": {"$ref": "#/$defs/hash"},
            "artifacts": {"type": "array", "minItems": 1, "maxItems": 4096, "items": {"$ref": "#/$defs/artifact"}},
            "artifactInventoryIdentitySha256": {"$ref": "#/$defs/hash"},
            "contentIdentitySha256": {"$ref": "#/$defs/hash"},
        },
        ["kind", "version", "runIdentitySha256", "artifacts", "artifactInventoryIdentitySha256", "contentIdentitySha256"],
    )
    return {PROFILE_SCHEMA_PATH: profile, ADAPTER_SCHEMA_PATH: adapter, REQUEST_SCHEMA_PATH: request, RESPONSE_SCHEMA_PATH: response, RUN_SCHEMA_PATH: run, BUNDLE_SCHEMA_PATH: bundle}


def authority(class_name: str) -> dict[str, Any]:
    common = {
        "requestMapping": "typed-lossless-no-authority-injection",
        "responseMapping": "typed-lossless-no-semantic-rewrite",
        "providerToolsAllowed": False,
        "hiddenRetriesAllowed": False,
        "secretPolicy": "redact-values-record-name-presence-and-policy",
    }
    rows: dict[str, dict[str, Any]] = {
        "hosted-api": {"network": "one-exact-https-origin-and-path", "filesystem": "none", "environment": "one-declared-secret-name", "subprocess": "none"},
        "local-openai-compatible": {"network": "one-exact-loopback-origin-and-path", "filesystem": "none", "environment": "none-or-one-declared-secret-name", "subprocess": "none"},
        "direct-local-runtime": {"network": "none", "filesystem": "digest-bound-executable-and-model-artifact-read-only", "environment": "closed-runtime-locator-names", "subprocess": "one-process-group"},
        "command-plugin": {"network": "none-by-contract", "filesystem": "digest-bound-executable-read-only-plus-empty-temporary-cwd", "environment": "closed-empty", "subprocess": "one-process-group"},
        "deterministic-mock": {"network": "none", "filesystem": "none", "environment": "none", "subprocess": "none"},
    }
    return {**common, **rows[class_name]}


def fixture_adapter(class_name: str, fixture_sha: str) -> dict[str, Any]:
    transport: dict[str, Any]
    if class_name == "hosted-api":
        transport = {"protocol": "openai-chat-completions-v1", "origin": "https://provider.invalid", "path": "/v1/chat/completions", "redirects": "deny", "secretEnvName": "GENESISBENCH_FIXTURE_API_KEY", "timeoutMs": 600000, "maxOutputBytes": 1048576, "mutableFacts": ["provider-request-id", "service-tier", "system-fingerprint"]}
    elif class_name == "local-openai-compatible":
        transport = {"protocol": "openai-chat-completions-v1", "origin": "http://127.0.0.1:65535", "path": "/v1/chat/completions", "redirects": "deny", "secretEnvName": None, "timeoutMs": 600000, "maxOutputBytes": 1048576, "mutableFacts": ["provider-request-id", "service-tier", "server-build", "system-fingerprint"]}
    elif class_name in {"direct-local-runtime", "command-plugin"}:
        transport = {"protocol": "json-stdio-v0.1", "executableSha256": fixture_sha, "argv": ["--respond"], "timeoutMs": 5000, "maxOutputBytes": 1048576, "mutableFacts": ["runtime-build"]}
        if class_name == "direct-local-runtime":
            transport["modelArtifactSha256"] = sha256_bytes(b"genesisbench-fixture-model-v0.1\n")
    else:
        transport = {"protocol": "request-hash-fixture-v0.1", "fixtureId": "generation-42-v0.1", "mutableFacts": ["fixture-id"]}
    return identified({
        "kind": "genesis/genesisbench-adapter-v0.1", "version": "0.1.0",
        "id": f"fixture-{class_name}-v0.1", "class": class_name,
        "model": {"id": "genesisbench-fixture-model", "revision": "v0.1", "immutable": True, "artifactSha256": None if class_name in {"hosted-api", "local-openai-compatible", "deterministic-mock"} else sha256_bytes(b"genesisbench-fixture-model-v0.1\n")},
        "transport": transport, "authority": authority(class_name),
        "requestMapping": "typed-lossless-no-authority-injection", "responseMapping": "typed-lossless-no-semantic-rewrite",
        "providerToolsAllowed": False, "hiddenRetriesAllowed": False,
        "secretPolicy": "redact-values-record-name-presence-and-policy", "rankEligible": False,
    })


def render_profile(fixture_sha: str) -> dict[str, Any]:
    adapters = [fixture_adapter(class_name, fixture_sha) for class_name in ADAPTER_CLASSES]
    return identified({
        "kind": "genesis/genesisbench-adapters-profile-v0.1", "version": "0.1.0",
        "classes": list(ADAPTER_CLASSES), "adapters": adapters,
        "normalization": {"request": KIND_REQUEST, "response": KIND_RESPONSE, "semanticRewriteAllowed": False, "allAttemptsRetained": True},
        "capabilityPolicy": {"ambientAuthorityAllowed": False, "providerToolsAllowed": False, "hiddenRetriesAllowed": False, "exactDeclaredAuthorityOnly": True},
        "conformance": {"sameRequestResponseVectors": True, "timeoutHardKillAndReap": True, "cancellationHardKillAndReap": True, "replayReinvocationAllowed": False},
    })


def render_fixture_tool() -> bytes:
    return b'''#!/usr/bin/env python3\nimport argparse,json,sys,time\np=argparse.ArgumentParser(); p.add_argument("--respond",action="store_true"); p.add_argument("--hang",action="store_true"); a=p.parse_args()\nif a.hang:\n    time.sleep(3600)\nrequest=json.load(sys.stdin)\nresponse={"candidateFiles":[{"contentBase64":"NDIK","path":"main.gc"}],"finishReason":"stop","providerFacts":{"runtime-build":"fixture-v0.1"},"requestIdentitySha256":request["contentIdentitySha256"],"status":"succeeded","usage":{"inputTokens":0,"outputTokens":1}}\njson.dump(response,sys.stdout,sort_keys=True,separators=(",",":")); sys.stdout.write("\\n")\n'''


def validate_adapter(adapter: Any) -> dict[str, Any]:
    fields = {"kind", "version", "id", "class", "model", "transport", "authority", "requestMapping", "responseMapping", "providerToolsAllowed", "hiddenRetriesAllowed", "secretPolicy", "rankEligible", "contentIdentitySha256"}
    adapter = closed(adapter, fields, "adapter")
    require(adapter["kind"] == "genesis/genesisbench-adapter-v0.1" and adapter["version"] == "0.1.0", "adapter version mismatch")
    require(isinstance(adapter["id"], str) and ID_RE.fullmatch(adapter["id"]) is not None, "invalid adapter id")
    class_name = adapter["class"]
    require(class_name in ADAPTER_CLASSES, "unknown adapter class")
    require(adapter["authority"] == authority(class_name), "adapter authority drift")
    require(adapter["requestMapping"] == "typed-lossless-no-authority-injection", "request mapping may inject authority")
    require(adapter["responseMapping"] == "typed-lossless-no-semantic-rewrite", "response mapping may rewrite semantics")
    require(adapter["providerToolsAllowed"] is False and adapter["hiddenRetriesAllowed"] is False, "adapter concealed provider authority")
    require(adapter["secretPolicy"] == "redact-values-record-name-presence-and-policy", "adapter secret policy drift")
    require(adapter["rankEligible"] is False, "adapter cannot self-assert ranked eligibility")
    model = closed(adapter["model"], {"id", "revision", "immutable", "artifactSha256"}, "adapter model")
    require(model["immutable"] is True and all(isinstance(model[key], str) and ID_RE.fullmatch(model[key]) for key in ("id", "revision")), "model binding is mutable or invalid")
    require(model["artifactSha256"] is None or SHA_RE.fullmatch(model["artifactSha256"]) is not None, "invalid model artifact identity")
    transport = adapter["transport"]
    require(isinstance(transport, dict), "adapter transport must be an object")
    if class_name in {"hosted-api", "local-openai-compatible"}:
        expected = {"protocol", "origin", "path", "redirects", "secretEnvName", "timeoutMs", "maxOutputBytes", "mutableFacts"}
        closed(transport, expected, "HTTP transport")
        origin = urllib.parse.urlsplit(transport["origin"])
        require(origin.path in {"", "/"} and not origin.query and not origin.fragment and origin.hostname is not None and origin.username is None and origin.password is None, "HTTP origin must be exact and credential-free")
        require(transport["path"].startswith("/") and "?" not in transport["path"] and "#" not in transport["path"], "HTTP path must be exact")
        if class_name == "hosted-api":
            require(origin.scheme == "https" and origin.hostname not in {"localhost", "127.0.0.1", "::1"}, "hosted adapter requires non-loopback HTTPS")
            require(isinstance(transport["secretEnvName"], str) and ENV_RE.fullmatch(transport["secretEnvName"]) is not None, "hosted adapter requires one canonical secret name")
        else:
            require(origin.scheme == "http" and origin.hostname in {"localhost", "127.0.0.1", "::1"}, "local OpenAI adapter must be loopback-only")
            require(transport["secretEnvName"] is None or (isinstance(transport["secretEnvName"], str) and ENV_RE.fullmatch(transport["secretEnvName"]) is not None), "invalid local adapter secret name")
        require(transport["redirects"] == "deny", "HTTP redirects broaden authority")
        require(isinstance(transport["timeoutMs"], int) and 1 <= transport["timeoutMs"] <= 3_600_000, "invalid HTTP timeout")
        require(isinstance(transport["maxOutputBytes"], int) and 1 <= transport["maxOutputBytes"] <= MAX_CAPTURE_BYTES, "invalid HTTP output limit")
    elif class_name in {"direct-local-runtime", "command-plugin"}:
        expected = {"protocol", "executableSha256", "argv", "timeoutMs", "maxOutputBytes", "mutableFacts"}
        if class_name == "direct-local-runtime":
            expected.add("modelArtifactSha256")
        closed(transport, expected, "process transport")
        require(transport["protocol"] == "json-stdio-v0.1" and SHA_RE.fullmatch(transport["executableSha256"]) is not None, "invalid process transport")
        require(isinstance(transport["argv"], list) and len(transport["argv"]) <= 32 and all(isinstance(v, str) and len(v) <= 1024 for v in transport["argv"]), "invalid process argv")
        require(isinstance(transport["timeoutMs"], int) and 1 <= transport["timeoutMs"] <= 3_600_000, "invalid process timeout")
        require(isinstance(transport["maxOutputBytes"], int) and 1 <= transport["maxOutputBytes"] <= MAX_CAPTURE_BYTES, "invalid output limit")
        if class_name == "direct-local-runtime":
            require(transport["modelArtifactSha256"] == model["artifactSha256"], "model artifact identity drift")
    else:
        closed(transport, {"protocol", "fixtureId", "mutableFacts"}, "mock transport")
        require(transport["protocol"] == "request-hash-fixture-v0.1" and adapter["rankEligible"] is False, "mock adapter cannot be rank eligible")
    validate_identity(adapter)
    return adapter


def find_case(case_id: str) -> dict[str, Any]:
    suite = load_json(SUITE_PATH)
    rows = [row for row in suite["cases"] if row["id"] == case_id]
    require(len(rows) == 1, f"unknown or duplicate benchmark case: {case_id}")
    return rows[0]


def build_request(case_id: str, adapter: dict[str, Any], ablation: str = "retrieval") -> tuple[dict[str, Any], dict[str, Any]]:
    rendered = genesisbench_reference_agent.render_all()
    plan = genesisbench_reference_agent.compile_plan(case_id, ablation, rendered["profile"], rendered["ablations"], rendered["retrieval"])
    case = find_case(case_id)
    messages: list[dict[str, Any]] = []
    role_sources = {
        "system-policy": (ROOT / genesisbench_reference_agent.SYSTEM_PATH).read_text(encoding="utf-8"),
        "agent-profile": json.dumps(rendered["profile"], sort_keys=True, separators=(",", ":")),
        "task-card": (ROOT / "docs/spec/GC_AGENT_TASK_CARDS_v0.3.md").read_text(encoding="utf-8"),
        "context-pack-or-retrieval-transcript": json.dumps(plan["context"], sort_keys=True, separators=(",", ":")),
        "task-prompt": case["prompt"],
        "task-inputs": json.dumps({row["path"]: (ROOT / case["inputRoot"] / row["path"]).read_text(encoding="utf-8") for row in case["inputFiles"]}, sort_keys=True, separators=(",", ":")),
    }
    for row in plan["promptSegments"]:
        content = role_sources[row["role"]]
        require(sha256_bytes(content.encode("utf-8")) == row["identitySha256"] or row["role"] in {"agent-profile", "context-pack-or-retrieval-transcript", "task-inputs"}, f"prompt segment identity drift: {row['role']}")
        messages.append({"role": row["role"], "content": content, "contentSha256": sha256_bytes(content.encode("utf-8"))})
    request = identified({
        "kind": KIND_REQUEST, "version": "0.1.0", "requestId": f"{case_id}-attempt-000", "caseId": case_id,
        "planIdentitySha256": plan["contentIdentitySha256"],
        "model": adapter["model"], "messages": messages,
        "outputContract": {"kind": KIND_RESPONSE, "candidateFiles": case["editablePaths"], "encoding": "base64-bytes", "semanticRewriteAllowed": False},
        "tools": [], "sampling": {"attempt": 0, "providerTools": False, "hiddenRetries": 0},
    })
    validate_request(request, adapter, plan)
    return plan, request


def validate_request(request: Any, adapter: dict[str, Any], plan: dict[str, Any] | None = None) -> dict[str, Any]:
    fields = {"kind", "version", "requestId", "caseId", "planIdentitySha256", "model", "messages", "outputContract", "tools", "sampling", "contentIdentitySha256"}
    request = closed(request, fields, "adapter request")
    require(request["kind"] == KIND_REQUEST and request["version"] == "0.1.0", "request version mismatch")
    require(request["model"] == adapter["model"], "request model binding drift")
    require(request["tools"] == [] and request["sampling"] == {"attempt": 0, "providerTools": False, "hiddenRetries": 0}, "request gained hidden authority or retry")
    case = find_case(request["caseId"])
    require(
        request["outputContract"] == {
            "kind": KIND_RESPONSE,
            "candidateFiles": case["editablePaths"],
            "encoding": "base64-bytes",
            "semanticRewriteAllowed": False,
        },
        "request output contract drift",
    )
    require(isinstance(request["messages"], list) and [row.get("role") for row in request["messages"]] == genesisbench_reference_agent.PROMPT_ROLES, "request prompt order drift")
    for row in request["messages"]:
        closed(row, {"role", "content", "contentSha256"}, "request message")
        require(isinstance(row["content"], str) and sha256_bytes(row["content"].encode("utf-8")) == row["contentSha256"], "request message identity drift")
    if plan is not None:
        require(request["caseId"] == plan["caseId"] and request["planIdentitySha256"] == plan["contentIdentitySha256"], "request plan binding drift")
    validate_identity(request)
    return request


def normalized_success(request: dict[str, Any], candidate_files: list[dict[str, Any]], usage: dict[str, Any], provider_facts: dict[str, Any], finish_reason: str = "stop") -> dict[str, Any]:
    return identified({"kind": KIND_RESPONSE, "version": "0.1.0", "requestIdentitySha256": request["contentIdentitySha256"], "status": "succeeded", "finishReason": finish_reason, "candidateFiles": candidate_files, "usage": usage, "providerFacts": provider_facts, "error": None})


def validate_response(response: Any, request: dict[str, Any], case: dict[str, Any], adapter: dict[str, Any]) -> dict[str, Any]:
    fields = {"kind", "version", "requestIdentitySha256", "status", "finishReason", "candidateFiles", "usage", "providerFacts", "error", "contentIdentitySha256"}
    response = closed(response, fields, "adapter response")
    require(response["kind"] == KIND_RESPONSE and response["version"] == "0.1.0", "response version mismatch")
    require(response["requestIdentitySha256"] == request["contentIdentitySha256"], "response request binding drift")
    require(response["status"] in {"succeeded", "failed", "cancelled", "timed-out"}, "invalid response status")
    require(isinstance(response["finishReason"], str) and 1 <= len(response["finishReason"]) <= 128, "invalid finish reason")
    require(
        isinstance(response["usage"], dict)
        and set(response["usage"]).issubset({"inputTokens", "outputTokens"})
        and all(isinstance(value, int) and not isinstance(value, bool) and value >= 0 for value in response["usage"].values()),
        "invalid usage facts",
    )
    require(isinstance(response["providerFacts"], dict) and all(isinstance(key, str) and isinstance(value, (str, int, bool, type(None))) for key, value in response["providerFacts"].items()), "invalid provider facts")
    expected_provider_facts = set(adapter["transport"]["mutableFacts"])
    files = response["candidateFiles"]
    require(isinstance(files, list) and len(files) <= 128, "invalid candidate file count")
    paths: list[str] = []
    total = 0
    for row in files:
        closed(row, {"path", "contentBase64"}, "candidate file")
        path = relative_path(row["path"], "candidate path")
        require(path in case["editablePaths"], f"adapter returned non-editable path: {path}")
        require(path not in paths, f"duplicate candidate path: {path}")
        require(isinstance(row["contentBase64"], str), "candidate content must be base64")
        try:
            payload = base64.b64decode(row["contentBase64"], validate=True)
        except ValueError as exc:
            raise FrontDoorError("candidate content is not canonical base64") from exc
        require(base64.b64encode(payload).decode("ascii") == row["contentBase64"], "candidate base64 is non-canonical")
        total += len(payload)
        require(total <= 8 * 1024 * 1024, "candidate response exceeds byte limit")
        paths.append(path)
    if response["status"] == "succeeded":
        require(files and response["error"] is None and set(response["usage"]) == {"inputTokens", "outputTokens"} and set(response["providerFacts"]) == expected_provider_facts, "successful response must contain files, complete usage/provider facts, and no error")
    else:
        require(files == [] and response["usage"] == {} and response["providerFacts"] == {} and isinstance(response["error"], dict), "failed response must contain a closed error and no invented usage/provider facts")
        closed(response["error"], {"code", "message"}, "adapter error")
        require(
            isinstance(response["error"]["code"], str)
            and ID_RE.fullmatch(response["error"]["code"].replace("/", "-")) is not None
            and isinstance(response["error"]["message"], str)
            and 1 <= len(response["error"]["message"]) <= 1024,
            "invalid adapter error",
        )
    validate_identity(response)
    return response


class NoRedirect(urllib.request.HTTPRedirectHandler):
    def redirect_request(self, req: Any, fp: Any, code: int, msg: str, headers: Any, newurl: str) -> None:
        return None


def openai_payload(request: dict[str, Any], adapter: dict[str, Any]) -> dict[str, Any]:
    return {
        "model": adapter["model"]["id"],
        "messages": [{"role": "system" if row["role"] == "system-policy" else "user", "content": f"[{row['role']}]\n{row['content']}"} for row in request["messages"]],
        "tools": [], "tool_choice": "none", "n": 1, "stream": False,
        "metadata": {"genesisbench_request_sha256": request["contentIdentitySha256"]},
    }


def normalize_openai_response(raw: Any, request: dict[str, Any], adapter: dict[str, Any]) -> dict[str, Any]:
    require(isinstance(raw, dict), "OpenAI-compatible response must be an object")
    choices = raw.get("choices")
    require(isinstance(choices, list) and len(choices) == 1, "OpenAI-compatible response must contain exactly one choice")
    choice = choices[0]
    require(isinstance(choice, dict) and isinstance(choice.get("message"), dict), "OpenAI-compatible choice is malformed")
    require(choice["message"].get("tool_calls") is None or choice["message"].get("tool_calls") == [], "provider tools are forbidden")
    content = choice["message"].get("content")
    require(isinstance(content, str), "OpenAI-compatible response content must be text")
    try:
        payload = json.loads(content)
    except json.JSONDecodeError as exc:
        raise FrontDoorError("adapter response text is not exact candidate JSON") from exc
    require(isinstance(payload, dict) and set(payload) == {"candidateFiles"}, "adapter response JSON fields are not closed")
    usage_raw = raw.get("usage") or {}
    require(
        isinstance(usage_raw, dict)
        and all(
            isinstance(usage_raw.get(key, 0), int)
            and not isinstance(usage_raw.get(key, 0), bool)
            and usage_raw.get(key, 0) >= 0
            for key in ("prompt_tokens", "completion_tokens")
        ),
        "OpenAI-compatible usage is invalid",
    )
    usage = {"inputTokens": usage_raw.get("prompt_tokens", 0), "outputTokens": usage_raw.get("completion_tokens", 0)}
    fact_mapping = {
        "id": "provider-request-id",
        "service_tier": "service-tier",
        "system_fingerprint": "system-fingerprint",
    }
    facts = {name: None for name in adapter["transport"]["mutableFacts"]}
    for source, target in fact_mapping.items():
        if target in facts and source in raw and isinstance(raw[source], (str, int, bool, type(None))):
            facts[target] = raw[source]
    return normalized_success(request, payload["candidateFiles"], usage, facts, str(choice.get("finish_reason") or "stop"))


def invoke_http_direct(adapter: dict[str, Any], request: dict[str, Any]) -> dict[str, Any]:
    transport = adapter["transport"]
    endpoint = transport["origin"].rstrip("/") + transport["path"]
    headers = {"Content-Type": "application/json", "Accept": "application/json", "User-Agent": "GenesisBench/0.1"}
    secret_name = transport["secretEnvName"]
    if secret_name:
        secret = os.environ.get(secret_name)
        require(secret is not None and secret != "", f"declared adapter secret is absent: {secret_name}")
        headers["Authorization"] = f"Bearer {secret}"
    opener = urllib.request.build_opener(NoRedirect())
    req = urllib.request.Request(endpoint, data=canonical_bytes(openai_payload(request, adapter)), headers=headers, method="POST")
    try:
        with opener.open(req, timeout=transport["timeoutMs"] / 1000) as response:
            require(response.geturl() == endpoint, "HTTP adapter endpoint changed")
            payload = response.read(transport["maxOutputBytes"] + 1)
    except (urllib.error.URLError, TimeoutError) as exc:
        raise FrontDoorError(f"HTTP adapter failed without retry: {type(exc).__name__}") from exc
    require(len(payload) <= transport["maxOutputBytes"], "HTTP adapter response exceeds byte limit")
    try:
        return normalize_openai_response(json.loads(payload), request, adapter)
    except (UnicodeError, json.JSONDecodeError) as exc:
        raise FrontDoorError("HTTP adapter emitted invalid UTF-8 JSON") from exc


def terminate_group(process: subprocess.Popen[bytes]) -> None:
    try:
        if os.name == "posix":
            os.killpg(process.pid, signal.SIGKILL)
        else:
            process.kill()
    except ProcessLookupError:
        pass
    process.wait()


def invoke_http(
    adapter: dict[str, Any],
    request: dict[str, Any],
    *,
    cancel_after_ms: int | None = None,
) -> dict[str, Any]:
    transport = adapter["transport"]
    child_env = {"PATH": "/usr/bin:/bin", "LANG": "C", "LC_ALL": "C"}
    secret_name = transport["secretEnvName"]
    if secret_name:
        secret = os.environ.get(secret_name)
        require(secret is not None and secret != "", f"declared adapter secret is absent: {secret_name}")
        child_env[secret_name] = secret
    process = subprocess.Popen(
        [sys.executable, str(Path(__file__).resolve()), "http-worker"],
        cwd=ROOT,
        env=child_env,
        stdin=subprocess.PIPE,
        stdout=subprocess.PIPE,
        stderr=subprocess.PIPE,
        start_new_session=True,
    )
    timeout_ms = transport["timeoutMs"]
    if cancel_after_ms is not None:
        timeout_ms = min(timeout_ms, cancel_after_ms)
    try:
        stdout, stderr = process.communicate(
            canonical_bytes({"adapter": adapter, "request": request}) + b"\n",
            timeout=timeout_ms / 1000,
        )
    except subprocess.TimeoutExpired:
        terminate_group(process)
        status = "cancelled" if cancel_after_ms is not None and cancel_after_ms <= transport["timeoutMs"] else "timed-out"
        return identified({"kind": KIND_RESPONSE, "version": "0.1.0", "requestIdentitySha256": request["contentIdentitySha256"], "status": status, "finishReason": status, "candidateFiles": [], "usage": {}, "providerFacts": {}, "error": {"code": f"adapter/{status}", "message": f"HTTP adapter {status} and was killed and reaped"}})
    except BaseException:
        terminate_group(process)
        raise
    require(len(stdout) <= transport["maxOutputBytes"] and len(stderr) <= transport["maxOutputBytes"], "HTTP worker output exceeds byte limit")
    require(process.returncode == 0, "HTTP adapter failed without retry")
    try:
        return json.loads(stdout)
    except (UnicodeError, json.JSONDecodeError) as exc:
        raise FrontDoorError("HTTP worker emitted invalid UTF-8 JSON") from exc


def invoke_process(
    adapter: dict[str, Any],
    request: dict[str, Any],
    executable: Path,
    model_artifact: Path | None = None,
    *,
    cancel_after_ms: int | None = None,
) -> dict[str, Any]:
    transport = adapter["transport"]
    executable = regular_file(executable, "adapter executable")
    require(sha256_file(executable) == transport["executableSha256"], "adapter executable identity mismatch")
    child_env = {"PATH": "/usr/bin:/bin", "LANG": "C", "LC_ALL": "C"}
    if adapter["class"] == "direct-local-runtime":
        require(model_artifact is not None, "direct local runtime requires --model-artifact")
        model_artifact = regular_file(model_artifact, "model artifact")
        require(
            sha256_file(model_artifact) == transport["modelArtifactSha256"],
            "model artifact identity mismatch",
        )
        child_env["GENESISBENCH_MODEL_ARTIFACT"] = str(model_artifact)
    else:
        require(model_artifact is None, "command plugin cannot receive a model artifact")
    with tempfile.TemporaryDirectory(prefix="genesisbench-adapter-") as cwd:
        process = subprocess.Popen(
            [str(executable), *transport["argv"]], cwd=cwd,
            env=child_env,
            stdin=subprocess.PIPE, stdout=subprocess.PIPE, stderr=subprocess.PIPE,
            start_new_session=True,
        )
        input_bytes = canonical_bytes(request) + b"\n"
        timeout = transport["timeoutMs"] / 1000
        if cancel_after_ms is not None:
            timeout = min(timeout, cancel_after_ms / 1000)
        try:
            stdout, stderr = process.communicate(input_bytes, timeout=timeout)
        except subprocess.TimeoutExpired:
            terminate_group(process)
            status = "cancelled" if cancel_after_ms is not None and cancel_after_ms <= transport["timeoutMs"] else "timed-out"
            return identified({"kind": KIND_RESPONSE, "version": "0.1.0", "requestIdentitySha256": request["contentIdentitySha256"], "status": status, "finishReason": status, "candidateFiles": [], "usage": {}, "providerFacts": {}, "error": {"code": f"adapter/{status}", "message": f"adapter process {status} and was killed and reaped"}})
        except BaseException:
            terminate_group(process)
            raise
        require(len(stdout) <= transport["maxOutputBytes"] and len(stderr) <= transport["maxOutputBytes"], "adapter process output exceeds byte limit")
        require(process.returncode == 0, f"adapter process failed with exit {process.returncode}")
        try:
            raw = json.loads(stdout)
        except (UnicodeError, json.JSONDecodeError) as exc:
            raise FrontDoorError("adapter process emitted invalid UTF-8 JSON") from exc
        expected = {"requestIdentitySha256", "status", "finishReason", "candidateFiles", "usage", "providerFacts"}
        closed(raw, expected, "stdio adapter response")
        require(raw["status"] == "succeeded", "stdio adapter returned non-success without normalized error")
        return normalized_success(request, raw["candidateFiles"], raw["usage"], raw["providerFacts"], raw["finishReason"])


def invoke_adapter(
    adapter: dict[str, Any],
    request: dict[str, Any],
    executable: Path | None,
    model_artifact: Path | None,
) -> dict[str, Any]:
    class_name = adapter["class"]
    if class_name in {"hosted-api", "local-openai-compatible"}:
        return invoke_http(adapter, request)
    if class_name in {"direct-local-runtime", "command-plugin"}:
        require(executable is not None, "process adapter requires --adapter-executable")
        return invoke_process(adapter, request, executable, model_artifact)
    require(request["caseId"] == "generation-small", "deterministic fixture is bound to generation-small")
    return normalized_success(request, [{"path": "main.gc", "contentBase64": base64.b64encode(b"42\n").decode("ascii")}], {"inputTokens": 0, "outputTokens": 1}, {"fixture-id": "generation-42-v0.1"})


def materialize_candidate(case: dict[str, Any], response: dict[str, Any], candidate_root: Path) -> None:
    source = regular_directory(ROOT / case["inputRoot"], "benchmark input root")
    shutil.copytree(source, candidate_root, symlinks=False)
    for row in response["candidateFiles"]:
        target = candidate_root / row["path"]
        target.parent.mkdir(parents=True, exist_ok=True)
        target.write_bytes(base64.b64decode(row["contentBase64"], validate=True))


def score(case_id: str, candidate: Path, genesis_bin: Path, selfhost_artifact: Path) -> dict[str, Any]:
    return gc_agent_scoring.score_candidate(load_json(SCORING_PATH), case_id, candidate, genesis_bin, selfhost_artifact)


def build_run_record(out: Path, case: dict[str, Any], plan: dict[str, Any], adapter: dict[str, Any], response: dict[str, Any], elapsed_ms: int, score_doc: dict[str, Any] | None) -> dict[str, Any]:
    candidate_rows = inventory_tree(out / "candidate")
    attempt = {
        "index": 0, "requestArtifact": "attempts/000/request.json", "requestIdentitySha256": load_json(out / "attempts/000/request.json")["contentIdentitySha256"],
        "responseArtifact": "attempts/000/response.json", "responseIdentitySha256": response["contentIdentitySha256"],
        "status": response["status"], "elapsedMs": elapsed_ms,
        "secretDisclosure": {
            "valuesRecorded": False,
            "declaredNames": [adapter["transport"]["secretEnvName"]] if adapter["class"] in {"hosted-api", "local-openai-compatible"} and adapter["transport"]["secretEnvName"] else [],
            "presentNames": [adapter["transport"]["secretEnvName"]] if adapter["class"] in {"hosted-api", "local-openai-compatible"} and adapter["transport"]["secretEnvName"] and os.environ.get(adapter["transport"]["secretEnvName"]) else [],
            "presenceRecorded": True,
        },
    }
    provisional = {
        "kind": KIND_RUN, "version": "0.1.0",
        "case": {"id": case["id"], "lineageId": case["lineageId"], "lineageIdentitySha256": case["lineageIdentitySha256"], "conditionId": case["conditionId"], "conditionIdentitySha256": case["conditionIdentitySha256"]},
        "referenceAgent": {"planArtifact": "plan.json", "planIdentitySha256": plan["contentIdentitySha256"], "profileIdentitySha256": genesisbench_reference_agent.render_all()["profile"]["contentIdentitySha256"]},
        "adapter": {"artifact": "adapter.json", "id": adapter["id"], "class": adapter["class"], "identitySha256": adapter["contentIdentitySha256"], "rankEligible": adapter["rankEligible"]},
        "attempts": [attempt],
        "outcome": ("verified" if score_doc["validity"]["passed"] else "failed") if score_doc is not None else "invalid",
        "candidateInventory": candidate_rows, "candidateInventoryIdentitySha256": artifact_identity(candidate_rows),
        "scoreIdentitySha256": sha256_bytes(canonical_bytes(score_doc)) if score_doc is not None else None,
        "artifactInventory": [], "artifactInventoryIdentitySha256": "",
        "replay": {"adapterReinvocationAllowed": False, "modelAccessAllowed": False, "requestResponseValidation": "strict-all-fields", "independentRescoreRequired": score_doc is not None},
    }
    # Inventory is closed over every artifact except run.json, avoiding a recursive file hash.
    rows = inventory_tree(out)
    rows = [row for row in rows if row["path"] != "run.json"]
    provisional["artifactInventory"] = rows
    provisional["artifactInventoryIdentitySha256"] = artifact_identity(rows)
    return identified(provisional)


def run_benchmark(case_id: str, adapter_path: Path, out: Path, genesis_bin: Path, selfhost_artifact: Path, executable: Path | None, model_artifact: Path | None, ablation: str) -> dict[str, Any]:
    require(not out.exists(), "run output path already exists; immutable runs never overwrite")
    require(ablation == "retrieval", "front-door v0.1 executes only the canonical retrieval condition; ablation authorities remain separately predeclared")
    adapter = validate_adapter(load_json(regular_file(adapter_path, "adapter manifest")))
    case = find_case(case_id)
    plan, request = build_request(case_id, adapter, ablation)
    out.mkdir(parents=True)
    try:
        write_json(out / "adapter.json", adapter)
        write_json(out / "plan.json", plan)
        write_json(out / "attempts/000/request.json", request)
        started = time.monotonic_ns()
        try:
            response = invoke_adapter(adapter, request, executable, model_artifact)
        except (FrontDoorError, OSError, UnicodeError, json.JSONDecodeError):
            response = identified({
                "kind": KIND_RESPONSE, "version": "0.1.0",
                "requestIdentitySha256": request["contentIdentitySha256"],
                "status": "failed", "finishReason": "adapter-error",
                "candidateFiles": [], "usage": {}, "providerFacts": {},
                "error": {"code": "adapter/invocation-failed", "message": "adapter invocation failed without retry"},
            })
        elapsed_ms = max(0, (time.monotonic_ns() - started) // 1_000_000)
        validate_response(response, request, case, adapter)
        write_json(out / "attempts/000/response.json", response)
        score_doc = None
        if response["status"] == "succeeded":
            materialize_candidate(case, response, out / "candidate")
            score_doc = score(case_id, out / "candidate", regular_file(genesis_bin, "genesis binary"), regular_file(selfhost_artifact, "selfhost artifact"))
            write_json(out / "score.json", score_doc)
        else:
            (out / "candidate").mkdir()
        run_doc = build_run_record(out, case, plan, adapter, response, elapsed_ms, score_doc)
        write_json(out / "run.json", run_doc)
        validate_run(out / "run.json", check_files=True)
        return run_doc
    except Exception:
        shutil.rmtree(out, ignore_errors=True)
        raise


def validate_inventory(root: Path, expected: list[dict[str, Any]], *, excluded: set[str] | None = None) -> None:
    observed = inventory_tree(root)
    if excluded:
        observed = [row for row in observed if row["path"] not in excluded]
    require(observed == expected, "artifact inventory does not match exact run bytes")


def validate_artifact_rows(rows: Any, label: str) -> list[dict[str, Any]]:
    require(isinstance(rows, list) and len(rows) <= 4096, f"invalid {label} count")
    paths: list[str] = []
    for row in rows:
        closed(row, {"path", "bytes", "sha256"}, label)
        path = relative_path(row["path"], f"{label} path")
        require(path not in paths, f"duplicate {label} path")
        require(isinstance(row["bytes"], int) and 0 <= row["bytes"] <= 64 * 1024 * 1024, f"invalid {label} byte count")
        require(isinstance(row["sha256"], str) and SHA_RE.fullmatch(row["sha256"]) is not None, f"invalid {label} identity")
        paths.append(path)
    require(paths == sorted(paths), f"{label} is not canonically sorted")
    return rows


def validate_run(run_path: Path, *, check_files: bool) -> dict[str, Any]:
    run_path = regular_file(run_path, "run record")
    run = load_json(run_path)
    fields = {"kind", "version", "case", "referenceAgent", "adapter", "attempts", "outcome", "candidateInventory", "candidateInventoryIdentitySha256", "scoreIdentitySha256", "artifactInventory", "artifactInventoryIdentitySha256", "replay", "contentIdentitySha256"}
    closed(run, fields, "execution run")
    require(run["kind"] == KIND_RUN and run["version"] == "0.1.0", "execution run version mismatch")
    validate_identity(run)
    case = find_case(run["case"]["id"])
    expected_case = {"id": case["id"], "lineageId": case["lineageId"], "lineageIdentitySha256": case["lineageIdentitySha256"], "conditionId": case["conditionId"], "conditionIdentitySha256": case["conditionIdentitySha256"]}
    require(run["case"] == expected_case, "run case or lineage binding drift")
    closed(run["referenceAgent"], {"planArtifact", "planIdentitySha256", "profileIdentitySha256"}, "reference agent binding")
    closed(run["adapter"], {"artifact", "id", "class", "identitySha256", "rankEligible"}, "adapter binding")
    closed(run["replay"], {"adapterReinvocationAllowed", "modelAccessAllowed", "requestResponseValidation", "independentRescoreRequired"}, "replay policy")
    require(
        run["replay"] == {
            "adapterReinvocationAllowed": False,
            "modelAccessAllowed": False,
            "requestResponseValidation": "strict-all-fields",
            "independentRescoreRequired": run["scoreIdentitySha256"] is not None,
        },
        "replay policy drift",
    )
    require(run["outcome"] in {"verified", "failed", "invalid", "abstained"}, "invalid run outcome")
    require(isinstance(run["attempts"], list) and 1 <= len(run["attempts"]) <= 2, "invalid attempt count")
    require([row.get("index") for row in run["attempts"]] == list(range(len(run["attempts"]))), "attempt indexes are not contiguous")
    for attempt in run["attempts"]:
        closed(attempt, {"index", "requestArtifact", "requestIdentitySha256", "responseArtifact", "responseIdentitySha256", "status", "elapsedMs", "secretDisclosure"}, "run attempt")
        closed(attempt["secretDisclosure"], {"valuesRecorded", "declaredNames", "presentNames", "presenceRecorded"}, "secret disclosure")
        require(attempt["secretDisclosure"]["valuesRecorded"] is False and attempt["secretDisclosure"]["presenceRecorded"] is True, "secret values were recorded or presence omitted")
        require(isinstance(attempt["elapsedMs"], int) and 0 <= attempt["elapsedMs"] <= 86_400_000, "invalid attempt elapsed time")
        require(attempt["requestArtifact"] == f"attempts/{attempt['index']:03d}/request.json" and attempt["responseArtifact"] == f"attempts/{attempt['index']:03d}/response.json", "attempt artifact path drift")
        require(attempt["status"] in {"succeeded", "failed", "cancelled", "timed-out"}, "invalid attempt status")
        require(SHA_RE.fullmatch(attempt["requestIdentitySha256"]) is not None and SHA_RE.fullmatch(attempt["responseIdentitySha256"]) is not None, "invalid attempt identity")
    validate_artifact_rows(run["candidateInventory"], "candidate inventory")
    validate_artifact_rows(run["artifactInventory"], "run artifact inventory")
    require(run["candidateInventoryIdentitySha256"] == artifact_identity(run["candidateInventory"]), "candidate inventory identity drift")
    require(run["artifactInventoryIdentitySha256"] == artifact_identity(run["artifactInventory"]), "run artifact inventory identity drift")
    require(
        run["scoreIdentitySha256"] is None
        or (isinstance(run["scoreIdentitySha256"], str) and SHA_RE.fullmatch(run["scoreIdentitySha256"]) is not None),
        "invalid score identity",
    )
    if check_files:
        root = run_path.parent
        validate_inventory(root, run["artifactInventory"], excluded={"run.json"})
        adapter = validate_adapter(load_json(root / run["adapter"]["artifact"]))
        require(run["adapter"] == {"artifact": "adapter.json", "id": adapter["id"], "class": adapter["class"], "identitySha256": adapter["contentIdentitySha256"], "rankEligible": adapter["rankEligible"]}, "run adapter binding drift")
        plan = load_json(root / run["referenceAgent"]["planArtifact"])
        rendered = genesisbench_reference_agent.render_all()
        matching_conditions = [row for row in rendered["ablations"]["conditions"] if row["id"] == plan.get("conditionId")]
        require(len(matching_conditions) == 1, "run plan condition is unknown")
        expected_plan = genesisbench_reference_agent.compile_plan(
            case["id"], matching_conditions[0]["ablationId"], rendered["profile"], rendered["ablations"], rendered["retrieval"]
        )
        genesisbench_reference_agent.validate_plan(plan, expected_plan)
        require(
            run["referenceAgent"] == {
                "planArtifact": "plan.json",
                "planIdentitySha256": plan["contentIdentitySha256"],
                "profileIdentitySha256": rendered["profile"]["contentIdentitySha256"],
            },
            "run reference-agent binding drift",
        )
        secret_name = adapter["transport"].get("secretEnvName")
        declared_names = [secret_name] if secret_name else []
        for attempt in run["attempts"]:
            require(attempt["secretDisclosure"]["declaredNames"] == declared_names, "attempt secret declaration drift")
            require(set(attempt["secretDisclosure"]["presentNames"]).issubset(set(declared_names)), "attempt disclosed an undeclared secret name")
            request = validate_request(load_json(root / attempt["requestArtifact"]), adapter, plan)
            response = validate_response(load_json(root / attempt["responseArtifact"]), request, case, adapter)
            require(attempt["requestIdentitySha256"] == request["contentIdentitySha256"] and attempt["responseIdentitySha256"] == response["contentIdentitySha256"] and attempt["status"] == response["status"], "attempt binding drift")
        validate_inventory(root / "candidate", run["candidateInventory"])
        if run["scoreIdentitySha256"] is None:
            require(not (root / "score.json").exists() and run["candidateInventory"] == [] and run["outcome"] == "invalid", "unscored run state is inconsistent")
            require(run["attempts"][-1]["status"] != "succeeded", "successful attempt omitted its score")
        else:
            require(SHA_RE.fullmatch(run["scoreIdentitySha256"]) is not None, "invalid score identity")
            score_doc = load_json(root / "score.json")
            require(run["scoreIdentitySha256"] == sha256_bytes(canonical_bytes(score_doc)), "score identity drift")
            require(run["outcome"] == ("verified" if score_doc["validity"]["passed"] else "failed"), "run outcome disagrees with score")
            require(run["attempts"][-1]["status"] == "succeeded", "scored run lacks a successful terminal attempt")
    return run


def replay_run(run_path: Path, genesis_bin: Path, selfhost_artifact: Path) -> dict[str, Any]:
    run = validate_run(run_path, check_files=True)
    root = run_path.resolve().parent
    matched = False
    if run["scoreIdentitySha256"] is not None:
        rescored = score(run["case"]["id"], root / "candidate", regular_file(genesis_bin, "genesis binary"), regular_file(selfhost_artifact, "selfhost artifact"))
        expected = load_json(root / "score.json")
        require(rescored == expected, "independent replay score mismatch")
        matched = True
    return {"kind": "genesis/genesisbench-replay-v0.1", "runIdentitySha256": run["contentIdentitySha256"], "adapterInvoked": False, "modelAccessed": False, "allFieldsValidated": True, "independentRescoreMatched": matched, "scoreIdentitySha256": run["scoreIdentitySha256"]}


def deterministic_bundle(run_path: Path, out: Path) -> dict[str, Any]:
    require(not out.exists(), "bundle output already exists; immutable bundles never overwrite")
    run = validate_run(run_path, check_files=True)
    root = run_path.resolve().parent
    artifacts = inventory_tree(root)
    manifest = identified({"kind": KIND_BUNDLE, "version": "0.1.0", "runIdentitySha256": run["contentIdentitySha256"], "artifacts": artifacts, "artifactInventoryIdentitySha256": artifact_identity(artifacts)})
    with out.open("xb") as target:
        with gzip.GzipFile(filename="", mode="wb", fileobj=target, compresslevel=9, mtime=0) as zipped:
            with tarfile.open(fileobj=zipped, mode="w", format=tarfile.USTAR_FORMAT) as archive:
                members = [("genesisbench-bundle/bundle-manifest.json", pretty_bytes(manifest))]
                members.extend((f"genesisbench-bundle/{row['path']}", (root / row["path"]).read_bytes()) for row in artifacts)
                for name, payload in sorted(members):
                    info = tarfile.TarInfo(name)
                    info.size = len(payload); info.mode = 0o644; info.uid = 0; info.gid = 0; info.mtime = 0
                    info.uname = ""; info.gname = ""
                    archive.addfile(info, io.BytesIO(payload))
    return {"kind": "genesis/genesisbench-bundle-result-v0.1", "bundle": out.name, "bytes": out.stat().st_size, "sha256": sha256_file(out), "manifestIdentitySha256": manifest["contentIdentitySha256"], "runIdentitySha256": run["contentIdentitySha256"]}


def validate_bundle(path: Path) -> tuple[dict[str, Any], str]:
    path = regular_file(path, "bundle")
    seen: list[str] = []
    payloads: dict[str, bytes] = {}
    with tarfile.open(path, mode="r:gz") as archive:
        for member in archive:
            require(member.isfile() and not member.issym() and not member.islnk(), "bundle contains non-regular member")
            require(member.name.startswith("genesisbench-bundle/") and member.name not in seen, "bundle member path or uniqueness violation")
            require(member.uid == 0 and member.gid == 0 and member.mtime == 0 and member.mode == 0o644, "bundle metadata is not deterministic")
            relative_path(member.name.removeprefix("genesisbench-bundle/"), "bundle member")
            stream = archive.extractfile(member)
            require(stream is not None, "bundle member unreadable")
            payloads[member.name] = stream.read(MAX_CAPTURE_BYTES + 1)
            require(len(payloads[member.name]) <= MAX_CAPTURE_BYTES, "bundle member exceeds limit")
            seen.append(member.name)
    require(seen == sorted(seen), "bundle members are not sorted")
    manifest = json.loads(payloads["genesisbench-bundle/bundle-manifest.json"])
    validate_identity(manifest)
    require(manifest["kind"] == KIND_BUNDLE and manifest["artifactInventoryIdentitySha256"] == artifact_identity(manifest["artifacts"]), "bundle manifest invalid")
    observed = [{"path": name.removeprefix("genesisbench-bundle/"), "bytes": len(payload), "sha256": sha256_bytes(payload)} for name, payload in sorted(payloads.items()) if name != "genesisbench-bundle/bundle-manifest.json"]
    require(observed == manifest["artifacts"], "bundle artifacts do not match manifest")
    run = json.loads(payloads["genesisbench-bundle/run.json"])
    validate_identity(run)
    require(run["contentIdentitySha256"] == manifest["runIdentitySha256"], "bundle run identity mismatch")
    return manifest, sha256_file(path)


def submit_bundle(bundle: Path, outbox: Path, submitter: str) -> dict[str, Any]:
    require(isinstance(submitter, str) and ID_RE.fullmatch(submitter) is not None, "invalid submitter id")
    manifest, bundle_sha = validate_bundle(bundle)
    outbox.mkdir(parents=True, exist_ok=True)
    require(not outbox.is_symlink(), "outbox cannot be a symlink")
    bundle_target = outbox / f"{bundle_sha}.gcbundle"
    envelope = identified({"kind": KIND_SUBMISSION, "version": "0.1.0", "submitter": submitter, "bundleSha256": bundle_sha, "bundleBytes": bundle.stat().st_size, "bundleManifestIdentitySha256": manifest["contentIdentitySha256"], "runIdentitySha256": manifest["runIdentitySha256"], "transport": "local-immutable-outbox-v0.1"})
    envelope_target = outbox / f"{bundle_sha}.submission.json"
    if bundle_target.exists() or envelope_target.exists():
        require(bundle_target.is_file() and sha256_file(bundle_target) == bundle_sha and load_json(envelope_target) == envelope, "outbox identity collision")
    else:
        temp_bundle = outbox / f".{bundle_sha}.tmp"
        shutil.copyfile(bundle, temp_bundle)
        require(sha256_file(temp_bundle) == bundle_sha, "outbox copy identity mismatch")
        os.replace(temp_bundle, bundle_target)
        write_json(envelope_target, envelope)
    return envelope


def inspect(case_id: str | None, adapter_path: Path | None) -> dict[str, Any]:
    suite = load_json(SUITE_PATH)
    profile = load_json(PROFILE_PATH)
    data: dict[str, Any] = {"kind": "genesis/genesisbench-inspect-v0.1", "profileIdentitySha256": profile["contentIdentitySha256"], "openAgentHarnessIdentitySha256": genesisbench_open_agent.authority()["contentIdentitySha256"], "cases": [{"id": row["id"], "taskClass": row["taskClass"], "lineageId": row["lineageId"], "conditionId": row["conditionId"]} for row in suite["cases"]], "adapterClasses": list(ADAPTER_CLASSES), "commands": ["inspect", "run", "agent-plan", "agent-run", "agent-validate", "agent-replay", "validate-run", "score", "replay", "bundle", "submit", "registry-init", "registry-admit", "registry-verify", "registry-build"]}
    if case_id is not None:
        data["case"] = find_case(case_id)
    if adapter_path is not None:
        data["adapter"] = validate_adapter(load_json(adapter_path))
    return data


def validate_authorities() -> dict[str, Any]:
    fixture_sha = sha256_bytes(render_fixture_tool())
    expected_profile = render_profile(fixture_sha)
    require(load_json(PROFILE_PATH) == expected_profile, "adapter profile drift; run --write")
    validate_identity(expected_profile)
    require(expected_profile["classes"] == list(ADAPTER_CLASSES) and len(expected_profile["adapters"]) == 5, "adapter class closure drift")
    for adapter in expected_profile["adapters"]:
        validate_adapter(adapter)
        require(load_json(FIXTURE_ROOT / f"{adapter['class']}.json") == adapter, f"adapter fixture drift: {adapter['class']}")
    schemas = render_schemas()
    for path, expected in schemas.items():
        require(load_json(path) == expected, f"schema drift: {path.name}")
    require(FIXTURE_TOOL.read_bytes() == render_fixture_tool(), "command fixture drift")
    genesisbench_open_agent.validate_authorities()
    return expected_profile


def self_test(profile: dict[str, Any]) -> int:
    controls = genesisbench_open_agent.self_test()
    case = find_case("generation-small")
    normalized: list[dict[str, Any]] = []
    with tempfile.TemporaryDirectory(prefix="genesisbench-self-test-") as tmp_raw:
        tmp = Path(tmp_raw)
        executable = tmp / "command_fixture.py"
        executable.write_bytes(render_fixture_tool()); executable.chmod(0o755)
        model = tmp / "model.fixture"; model.write_bytes(b"genesisbench-fixture-model-v0.1\n")
        for adapter in profile["adapters"]:
            plan, request = build_request("generation-small", adapter)
            if adapter["class"] in {"hosted-api", "local-openai-compatible"}:
                raw = {"id": "fixture-request", "choices": [{"finish_reason": "stop", "message": {"content": json.dumps({"candidateFiles": [{"path": "main.gc", "contentBase64": "NDIK"}]}), "tool_calls": None}}], "usage": {"prompt_tokens": 0, "completion_tokens": 1}, "system_fingerprint": "fixture-v0.1"}
                response = normalize_openai_response(raw, request, adapter)
            elif adapter["class"] in {"direct-local-runtime", "command-plugin"}:
                response = invoke_process(
                    adapter,
                    request,
                    executable,
                    model if adapter["class"] == "direct-local-runtime" else None,
                )
            else:
                response = invoke_adapter(adapter, request, None, None)
            normalized.append(validate_response(response, request, case, adapter))
        require(all(row["candidateFiles"] == normalized[0]["candidateFiles"] for row in normalized), "adapter classes normalized different candidate bytes")
        command = next(row for row in profile["adapters"] if row["class"] == "command-plugin")
        hanging = copy.deepcopy(command); hanging["transport"]["argv"] = ["--hang"]; hanging["transport"]["timeoutMs"] = 100
        hanging = identified({key: value for key, value in hanging.items() if key != "contentIdentitySha256"})
        _, request = build_request("generation-small", hanging)
        timed = invoke_process(hanging, request, executable)
        require(timed["status"] == "timed-out", "timeout did not hard-kill adapter")
        cancelled = invoke_process(hanging, request, executable, cancel_after_ms=20)
        require(cancelled["status"] == "cancelled", "cancellation did not hard-kill adapter")
        controls += 2

        class HangingHandler(http.server.BaseHTTPRequestHandler):
            def do_POST(self) -> None:
                time.sleep(10)

            def log_message(self, format: str, *args: Any) -> None:
                return

        server = http.server.ThreadingHTTPServer(("127.0.0.1", 0), HangingHandler)
        server_thread = threading.Thread(target=server.serve_forever, daemon=True)
        server_thread.start()
        try:
            local = copy.deepcopy(next(row for row in profile["adapters"] if row["class"] == "local-openai-compatible"))
            local["transport"]["origin"] = f"http://127.0.0.1:{server.server_port}"
            local["transport"]["timeoutMs"] = 100
            local = identified({key: value for key, value in local.items() if key != "contentIdentitySha256"})
            validate_adapter(local)
            _, local_request = build_request("generation-small", local)
            http_timed = invoke_http(local, local_request)
            require(http_timed["status"] == "timed-out", "HTTP timeout did not hard-kill worker")
            http_cancelled = invoke_http(local, local_request, cancel_after_ms=20)
            require(http_cancelled["status"] == "cancelled", "HTTP cancellation did not hard-kill worker")
            controls += 2
        finally:
            server.shutdown()
            server.server_close()

    def reject(value: dict[str, Any], label: str) -> None:
        nonlocal controls
        try:
            validate_adapter(value)
        except (FrontDoorError, KeyError, TypeError, AttributeError):
            controls += 1
        else:
            raise FrontDoorError(f"negative adapter control accepted: {label}")

    baseline = profile["adapters"][0]
    mutations: list[tuple[str, Callable[[dict[str, Any]], None]]] = [
        ("provider-tools", lambda d: d.__setitem__("providerToolsAllowed", True)),
        ("hidden-retry", lambda d: d.__setitem__("hiddenRetriesAllowed", True)),
        ("semantic-rewrite", lambda d: d.__setitem__("responseMapping", "rewrite")),
        ("authority-broadening", lambda d: d["authority"].__setitem__("filesystem", "all")),
        ("redirect", lambda d: d["transport"].__setitem__("redirects", "follow")),
        ("insecure-hosted", lambda d: d["transport"].__setitem__("origin", "http://provider.invalid")),
        ("mutable-model", lambda d: d["model"].__setitem__("immutable", False)),
        ("identity", lambda d: d.__setitem__("contentIdentitySha256", "0" * 64)),
    ]
    for label, mutate in mutations:
        candidate = copy.deepcopy(baseline); mutate(candidate); reject(candidate, label)
    controls += 8

    saved_secret = os.environ.pop("GENESISBENCH_FIXTURE_API_KEY", None)
    try:
        with tempfile.TemporaryDirectory(prefix="genesisbench-run-controls-") as tmp_raw:
            run_root = Path(tmp_raw) / "run"
            run_benchmark(
                "generation-small",
                FIXTURE_ROOT / "hosted-api.json",
                run_root,
                Path("missing-genesis"),
                Path("missing-selfhost"),
                None,
                None,
                "retrieval",
            )
            baseline_run = load_json(run_root / "run.json")

            def reject_run(label: str, mutate: Callable[[dict[str, Any]], None]) -> None:
                nonlocal controls
                candidate = copy.deepcopy(baseline_run)
                mutate(candidate)
                candidate = identified({key: value for key, value in candidate.items() if key != "contentIdentitySha256"})
                write_json(run_root / "run.json", candidate)
                try:
                    validate_run(run_root / "run.json", check_files=False)
                except (FrontDoorError, KeyError, TypeError, AttributeError):
                    controls += 1
                else:
                    raise FrontDoorError(f"negative run control accepted: {label}")

            for label, mutate in [
                ("run-case-binding", lambda d: d["case"].__setitem__("conditionId", "forged")),
                ("run-reinvoke", lambda d: d["replay"].__setitem__("adapterReinvocationAllowed", True)),
                ("run-secret-value", lambda d: d["attempts"][0]["secretDisclosure"].__setitem__("valuesRecorded", True)),
                ("run-attempt-status", lambda d: d["attempts"][0].__setitem__("status", "forged")),
                ("run-unknown-field", lambda d: d["attempts"][0].__setitem__("extra", True)),
                ("run-score-shape", lambda d: d.__setitem__("scoreIdentitySha256", "forged")),
            ]:
                reject_run(label, mutate)
    finally:
        if saved_secret is not None:
            os.environ["GENESISBENCH_FIXTURE_API_KEY"] = saved_secret
    return controls


def write_authorities() -> dict[str, Any]:
    FIXTURE_ROOT.mkdir(parents=True, exist_ok=True)
    FIXTURE_TOOL.write_bytes(render_fixture_tool()); FIXTURE_TOOL.chmod(0o755)
    profile = render_profile(sha256_file(FIXTURE_TOOL))
    write_json(PROFILE_PATH, profile)
    for adapter in profile["adapters"]:
        write_json(FIXTURE_ROOT / f"{adapter['class']}.json", adapter)
    for path, schema in render_schemas().items():
        write_json(path, schema)
    return profile


def parser() -> argparse.ArgumentParser:
    out = argparse.ArgumentParser(description=__doc__)
    modes = out.add_subparsers(dest="command", required=True)
    check = modes.add_parser("check"); check.add_argument("--self-test", action="store_true")
    modes.add_parser("http-worker", help=argparse.SUPPRESS)
    modes.add_parser("write")
    inspect_p = modes.add_parser("inspect"); inspect_p.add_argument("--case"); inspect_p.add_argument("--adapter", type=Path)
    run_p = modes.add_parser("run"); run_p.add_argument("--case", required=True); run_p.add_argument("--adapter", required=True, type=Path); run_p.add_argument("--out", required=True, type=Path); run_p.add_argument("--genesis-bin", required=True, type=Path); run_p.add_argument("--selfhost-artifact", required=True, type=Path); run_p.add_argument("--adapter-executable", type=Path); run_p.add_argument("--model-artifact", type=Path); run_p.add_argument("--ablation", default="retrieval", choices=["retrieval"])
    validate_p = modes.add_parser("validate-run"); validate_p.add_argument("--run", required=True, type=Path)
    score_p = modes.add_parser("score"); score_p.add_argument("--case", required=True); score_p.add_argument("--candidate", required=True, type=Path); score_p.add_argument("--genesis-bin", required=True, type=Path); score_p.add_argument("--selfhost-artifact", required=True, type=Path); score_p.add_argument("--out", type=Path)
    replay_p = modes.add_parser("replay"); replay_p.add_argument("--run", required=True, type=Path); replay_p.add_argument("--genesis-bin", required=True, type=Path); replay_p.add_argument("--selfhost-artifact", required=True, type=Path)
    bundle_p = modes.add_parser("bundle"); bundle_p.add_argument("--run", required=True, type=Path); bundle_p.add_argument("--out", required=True, type=Path)
    submit_p = modes.add_parser("submit"); submit_p.add_argument("--bundle", required=True, type=Path); submit_p.add_argument("--outbox", required=True, type=Path); submit_p.add_argument("--submitter", required=True)
    return out


def main() -> int:
    args = parser().parse_args()
    if args.command == "http-worker":
        payload = json.load(sys.stdin)
        closed(payload, {"adapter", "request"}, "HTTP worker input")
        adapter = validate_adapter(payload["adapter"])
        require(adapter["class"] in {"hosted-api", "local-openai-compatible"}, "HTTP worker received non-HTTP adapter")
        request = validate_request(payload["request"], adapter)
        result = invoke_http_direct(adapter, request)
        validate_response(result, request, find_case(request["caseId"]), adapter)
    elif args.command == "write":
        profile = write_authorities(); validate_authorities(); result = {"kind": "genesis/genesisbench-authority-refresh-v0.1", "profileIdentitySha256": profile["contentIdentitySha256"]}
    elif args.command == "check":
        profile = validate_authorities(); controls = self_test(profile) if args.self_test else 0; result = {"kind": "genesis/genesisbench-authority-check-v0.1", "profileIdentitySha256": profile["contentIdentitySha256"], "adapterClasses": len(profile["adapters"]), "controls": controls}
    elif args.command == "inspect": result = inspect(args.case, args.adapter)
    elif args.command == "run": result = run_benchmark(args.case, args.adapter, args.out, args.genesis_bin, args.selfhost_artifact, args.adapter_executable, args.model_artifact, args.ablation)
    elif args.command == "validate-run":
        run = validate_run(args.run, check_files=True); result = {"kind": "genesis/genesisbench-run-validation-v0.1", "valid": True, "runIdentitySha256": run["contentIdentitySha256"], "attempts": len(run["attempts"]), "outcome": run["outcome"]}
    elif args.command == "score":
        result = score(args.case, args.candidate, regular_file(args.genesis_bin, "genesis binary"), regular_file(args.selfhost_artifact, "selfhost artifact"));
        if args.out: require(not args.out.exists(), "score output already exists"); write_json(args.out, result)
    elif args.command == "replay": result = replay_run(args.run, args.genesis_bin, args.selfhost_artifact)
    elif args.command == "bundle": result = deterministic_bundle(args.run, args.out)
    else: result = submit_bundle(args.bundle, args.outbox, args.submitter)
    sys.stdout.buffer.write(pretty_bytes(result))
    return 0


if __name__ == "__main__":
    try:
        raise SystemExit(main())
    except (FrontDoorError, OSError, UnicodeError, json.JSONDecodeError, KeyError, ValueError, tarfile.TarError) as exc:
        error = {"kind": "genesis/genesisbench-front-door-error-v0.1", "code": "bench/front-door-failed", "message": str(exc)}
        sys.stderr.buffer.write(pretty_bytes(error))
        raise SystemExit(1)
