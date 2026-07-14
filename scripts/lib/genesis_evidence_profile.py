#!/usr/bin/env python3
"""Render and validate the GenesisCode in-toto/SLSA evidence profile."""

from __future__ import annotations

import argparse
import base64
from copy import deepcopy
from hashlib import sha256
import json
from pathlib import Path, PurePosixPath
import re
import sys
from typing import Any, Dict, Iterable, List, Mapping, Sequence, Tuple
from urllib.parse import urlparse


ROOT = Path(__file__).resolve().parents[2]
GENESIS_EVIDENCE_PROFILE_ID = "genesis/evidence-profile/v0.1"
DEFAULT_VECTOR = ROOT / "docs/program/evidence/GENESIS_EVIDENCE_BUNDLE_v0.1.json"
SCHEMAS = {
    "docs/spec/GENESIS_EVIDENCE_PREDICATE_v0.1.schema.json": "https://genesiscode.dev/schemas/evidence-predicate-v0.1.json",
    "docs/spec/GENESIS_EVIDENCE_STATEMENT_v0.1.schema.json": "https://genesiscode.dev/schemas/evidence-statement-v0.1.json",
    "docs/spec/GENESIS_SLSA_BUILD_v1.schema.json": "https://genesiscode.dev/schemas/slsa-build-profile-v1.json",
    "docs/spec/GENESIS_EVIDENCE_BUNDLE_v0.1.schema.json": "https://genesiscode.dev/schemas/evidence-bundle-v0.1.json",
}

STATEMENT_TYPE = "https://in-toto.io/Statement/v1"
GENESIS_PREDICATE_TYPE = "https://genesiscode.dev/attestations/evidence/v0.1"
SLSA_PREDICATE_TYPE = "https://slsa.dev/provenance/v1"
PAYLOAD_TYPE = "application/vnd.in-toto+json"
BUILD_TYPE = "https://genesiscode.dev/buildtypes/roadmap-evidence/v0.1"
BUILDER_ID = "https://genesiscode.dev/builders/local-roadmap-evidence/v0.1"

# Public, non-secret Ed25519 fixture key. R0.2.c owns independent crypto verification.
FIXTURE_PUBLIC_KEY_HEX = (
    "f4af0d699c7f13f1cb167c41e33c2bde2a86e64c0119fa9b1fbc4438e26a2261"
)
FIXTURE_KEY_ID = (
    "sha256:f76b9b7dfd8dfc5be35b538f36bc318afcfd37a2a7ae2ca6b7cf888ea1585336"
)
FIXTURE_ARTIFACT_PATH = "artifact/genesis-example.bin"
FIXTURE_ARTIFACT_BYTES = b"GenesisCode evidence fixture v0.1\n"
FIXTURE_SIGNATURES = {
    GENESIS_PREDICATE_TYPE: "IrPq1IZFkIZCzwbKFxDv3rR67420pv3ZR2T/Fxrc15LAjqrWIJL4qYYxyUAj6e2sdqz+QkRamKVo0bqJhC9NBQ==",
    SLSA_PREDICATE_TYPE: "ydX7i8B2mwgipZqCcYBFTI286Z8X/CTxRykPgN6vUx0AL+jJ5QK2km/3zwhxHsgV4v06ZnEIksdVk+12GbO9Bw==",
}

HEX64_RE = re.compile(r"^[0-9a-f]{64}$")
REVISION_RE = re.compile(r"^(?:[0-9a-f]{40}|[0-9a-f]{64})$")
ID_RE = re.compile(r"^[a-z0-9][a-z0-9._/-]*$")
ENV_RE = re.compile(r"^[A-Z_][A-Z0-9_]*$")
HOST_PATH_RE = re.compile(
    r"(?:^|[\s=:])(?:/Users/|/home/|/private/|/tmp/|[A-Za-z]:[\\/])"
)


class EvidenceError(ValueError):
    pass


def reject_duplicate_keys(pairs: Sequence[Tuple[str, Any]]) -> Dict[str, Any]:
    result: Dict[str, Any] = {}
    for key, value in pairs:
        if key in result:
            raise EvidenceError(f"duplicate JSON key: {key}")
        result[key] = value
    return result


def load_json(path: Path) -> Any:
    try:
        return json.loads(
            path.read_text(encoding="utf-8"), object_pairs_hook=reject_duplicate_keys
        )
    except FileNotFoundError as exc:
        raise EvidenceError(f"missing evidence file: {display_path(path)}") from exc
    except json.JSONDecodeError as exc:
        raise EvidenceError(
            f"invalid JSON in {display_path(path)}:{exc.lineno}:{exc.colno}: {exc.msg}"
        ) from exc


def display_path(path: Path) -> str:
    try:
        return path.resolve().relative_to(ROOT).as_posix()
    except ValueError:
        return path.as_posix()


def canonical_bytes(value: Any) -> bytes:
    reject_floats(value, "document")
    return json.dumps(
        value,
        ensure_ascii=False,
        allow_nan=False,
        sort_keys=True,
        separators=(",", ":"),
    ).encode("utf-8")


def iter_refs(value: Any) -> Iterable[str]:
    if isinstance(value, dict):
        for key, item in value.items():
            if key == "$ref" and isinstance(item, str):
                yield item
            yield from iter_refs(item)
    elif isinstance(value, list):
        for item in value:
            yield from iter_refs(item)


def validate_schema_contracts() -> None:
    known_ids = set(SCHEMAS.values())
    for relative_path, expected_id in SCHEMAS.items():
        schema = require_object(load_json(ROOT / relative_path), relative_path)
        if schema.get("$schema") != "https://json-schema.org/draft/2020-12/schema":
            raise EvidenceError(
                f"{relative_path} must declare JSON Schema Draft 2020-12"
            )
        if schema.get("$id") != expected_id:
            raise EvidenceError(f"{relative_path} has an unexpected $id")
        for ref in iter_refs(schema):
            if ref.startswith("#/"):
                continue
            if ref not in known_ids:
                raise EvidenceError(
                    f"{relative_path} has an unpinned or unknown $ref: {ref}"
                )


def retained_bytes(value: Any) -> bytes:
    return (
        json.dumps(
            value, ensure_ascii=False, allow_nan=False, sort_keys=True, indent=2
        ).encode("utf-8")
        + b"\n"
    )


def reject_floats(value: Any, label: str) -> None:
    if isinstance(value, float):
        raise EvidenceError(f"{label} must not contain floating-point values")
    if isinstance(value, list):
        for index, item in enumerate(value):
            reject_floats(item, f"{label}[{index}]")
    elif isinstance(value, dict):
        for key, item in value.items():
            reject_floats(item, f"{label}.{key}")


def require_object(value: Any, label: str) -> Mapping[str, Any]:
    if not isinstance(value, dict):
        raise EvidenceError(f"{label} must be an object")
    return value


def require_array(value: Any, label: str, *, non_empty: bool = False) -> List[Any]:
    if not isinstance(value, list):
        raise EvidenceError(f"{label} must be an array")
    if non_empty and not value:
        raise EvidenceError(f"{label} must not be empty")
    return value


def require_string(value: Any, label: str) -> str:
    if not isinstance(value, str) or not value:
        raise EvidenceError(f"{label} must be a non-empty string")
    if HOST_PATH_RE.search(value):
        raise EvidenceError(f"{label} leaks a host-specific path")
    return value


def require_int(value: Any, label: str, *, minimum: int | None = None) -> int:
    if isinstance(value, bool) or not isinstance(value, int):
        raise EvidenceError(f"{label} must be an integer")
    if minimum is not None and value < minimum:
        raise EvidenceError(f"{label} must be >= {minimum}")
    return value


def exact_keys(value: Mapping[str, Any], required: Iterable[str], label: str) -> None:
    expected = set(required)
    observed = set(value)
    missing = sorted(expected - observed)
    unknown = sorted(observed - expected)
    if missing:
        raise EvidenceError(f"{label} missing fields: {', '.join(missing)}")
    if unknown:
        raise EvidenceError(f"{label} contains unknown fields: {', '.join(unknown)}")


def require_sha256(value: Any, label: str) -> str:
    text = require_string(value, label)
    if not HEX64_RE.fullmatch(text):
        raise EvidenceError(f"{label} must be 64 lowercase hexadecimal characters")
    return text


def require_digest(value: Any, label: str) -> str:
    digest = require_object(value, label)
    exact_keys(digest, ("sha256",), label)
    return require_sha256(digest["sha256"], f"{label}.sha256")


def require_uri(value: Any, label: str) -> str:
    text = require_string(value, label)
    parsed = urlparse(text)
    if not parsed.scheme or parsed.scheme == "file":
        raise EvidenceError(f"{label} must be a non-file absolute URI")
    return text


def require_relative_path(value: Any, label: str) -> str:
    text = require_string(value, label)
    path = PurePosixPath(text)
    if text.startswith("/") or "\\" in text or ".." in path.parts or "//" in text:
        raise EvidenceError(f"{label} must be a normalized repository-relative path")
    if re.match(r"^[A-Za-z]:", text):
        raise EvidenceError(f"{label} must not contain a drive-qualified path")
    return text


def require_unique_sorted(values: Sequence[str], label: str) -> None:
    if list(values) != sorted(set(values)):
        raise EvidenceError(f"{label} must be sorted and unique")


def validate_artifact_identity(value: Any, label: str) -> None:
    identity = require_object(value, label)
    exact_keys(identity, ("uri", "digest"), label)
    require_uri(identity["uri"], f"{label}.uri")
    require_digest(identity["digest"], f"{label}.digest")


def validate_subjects(value: Any, label: str) -> List[Mapping[str, Any]]:
    subjects = require_array(value, label, non_empty=True)
    names = []
    result = []
    for index, raw in enumerate(subjects):
        subject = require_object(raw, f"{label}[{index}]")
        exact_keys(subject, ("name", "digest"), f"{label}[{index}]")
        names.append(require_relative_path(subject["name"], f"{label}[{index}].name"))
        require_digest(subject["digest"], f"{label}[{index}].digest")
        result.append(subject)
    require_unique_sorted(names, label)
    return result


def validate_genesis_predicate(value: Any) -> Mapping[str, Any]:
    predicate = require_object(value, "genesis predicate")
    exact_keys(
        predicate,
        (
            "kind",
            "version",
            "evidenceClass",
            "source",
            "toolchains",
            "environment",
            "networkPolicy",
            "commands",
            "negativeControls",
            "artifacts",
            "measurements",
            "verifier",
            "evidenceRefs",
        ),
        "genesis predicate",
    )
    if predicate["kind"] != "genesis/evidence-predicate-v0.1":
        raise EvidenceError("genesis predicate.kind has an unsupported value")
    if predicate["version"] != "0.1":
        raise EvidenceError("genesis predicate.version has an unsupported value")
    if predicate["evidenceClass"] not in ("E1", "E2", "E3", "E4"):
        raise EvidenceError("genesis predicate.evidenceClass is invalid")

    source = require_object(predicate["source"], "genesis predicate.source")
    exact_keys(
        source,
        (
            "vcs",
            "repositoryUri",
            "revision",
            "treeDigest",
            "dirty",
            "dirtyPolicy",
            "dirtyPathsDigest",
        ),
        "genesis predicate.source",
    )
    if source["vcs"] != "git":
        raise EvidenceError("genesis predicate.source.vcs must be git")
    require_uri(source["repositoryUri"], "genesis predicate.source.repositoryUri")
    revision = require_string(source["revision"], "genesis predicate.source.revision")
    if not REVISION_RE.fullmatch(revision):
        raise EvidenceError("genesis predicate.source.revision must be a Git object id")
    require_digest(source["treeDigest"], "genesis predicate.source.treeDigest")
    if not isinstance(source["dirty"], bool):
        raise EvidenceError("genesis predicate.source.dirty must be boolean")
    if source["dirty"]:
        if source["dirtyPolicy"] != "allow-declared":
            raise EvidenceError("dirty evidence requires dirtyPolicy=allow-declared")
        require_digest(
            source["dirtyPathsDigest"], "genesis predicate.source.dirtyPathsDigest"
        )
    elif source["dirtyPolicy"] not in ("reject", "allow-declared"):
        raise EvidenceError("clean evidence has an invalid dirtyPolicy")
    elif source["dirtyPathsDigest"] is not None:
        raise EvidenceError("clean evidence requires dirtyPathsDigest=null")

    toolchains = require_array(
        predicate["toolchains"], "genesis predicate.toolchains", non_empty=True
    )
    tool_names = []
    for index, raw in enumerate(toolchains):
        tool = require_object(raw, f"genesis predicate.toolchains[{index}]")
        exact_keys(tool, ("name", "version", "artifact"), f"toolchain[{index}]")
        tool_names.append(require_string(tool["name"], f"toolchain[{index}].name"))
        require_string(tool["version"], f"toolchain[{index}].version")
        validate_artifact_identity(tool["artifact"], f"toolchain[{index}].artifact")
    require_unique_sorted(tool_names, "genesis predicate.toolchains")

    environment = require_object(
        predicate["environment"], "genesis predicate.environment"
    )
    exact_keys(
        environment,
        ("profile", "os", "architecture", "container", "declaredVariables"),
        "genesis predicate.environment",
    )
    profile = require_string(environment["profile"], "environment.profile")
    if not ID_RE.fullmatch(profile):
        raise EvidenceError("environment.profile has an invalid identifier")
    require_string(environment["os"], "environment.os")
    require_string(environment["architecture"], "environment.architecture")
    if environment["container"] is not None:
        validate_artifact_identity(environment["container"], "environment.container")
    variables = require_array(
        environment["declaredVariables"], "environment.declaredVariables"
    )
    for index, variable in enumerate(variables):
        text = require_string(variable, f"environment.declaredVariables[{index}]")
        if not ENV_RE.fullmatch(text):
            raise EvidenceError(f"invalid declared environment variable: {text}")
    require_unique_sorted(variables, "environment.declaredVariables")

    network = require_object(
        predicate["networkPolicy"], "genesis predicate.networkPolicy"
    )
    exact_keys(network, ("mode", "inputs"), "genesis predicate.networkPolicy")
    if network["mode"] not in ("deny", "declared-only"):
        raise EvidenceError("networkPolicy.mode is invalid")
    inputs = require_array(network["inputs"], "networkPolicy.inputs")
    if network["mode"] == "deny" and inputs:
        raise EvidenceError("networkPolicy.mode=deny requires no inputs")
    if network["mode"] == "declared-only" and not inputs:
        raise EvidenceError("networkPolicy.mode=declared-only requires inputs")
    input_uris = []
    for index, raw in enumerate(inputs):
        item = require_object(raw, f"networkPolicy.inputs[{index}]")
        exact_keys(item, ("uri", "digest", "purpose"), f"network input[{index}]")
        input_uris.append(require_uri(item["uri"], f"network input[{index}].uri"))
        require_digest(item["digest"], f"network input[{index}].digest")
        require_string(item["purpose"], f"network input[{index}].purpose")
    require_unique_sorted(input_uris, "networkPolicy.inputs")

    commands = require_array(
        predicate["commands"], "genesis predicate.commands", non_empty=True
    )
    for index, raw in enumerate(commands):
        command = require_object(raw, f"genesis predicate.commands[{index}]")
        exact_keys(
            command,
            ("argv", "cwd", "declaredEnvironment", "exitCode"),
            f"command[{index}]",
        )
        argv = require_array(command["argv"], f"command[{index}].argv", non_empty=True)
        for arg_index, arg in enumerate(argv):
            require_string(arg, f"command[{index}].argv[{arg_index}]")
        require_relative_path(command["cwd"], f"command[{index}].cwd")
        env = require_array(
            command["declaredEnvironment"], f"command[{index}].declaredEnvironment"
        )
        for variable in env:
            if not isinstance(variable, str) or not ENV_RE.fullmatch(variable):
                raise EvidenceError(f"command[{index}] has an invalid environment name")
        require_unique_sorted(env, f"command[{index}].declaredEnvironment")
        require_int(command["exitCode"], f"command[{index}].exitCode")

    controls = require_array(
        predicate["negativeControls"],
        "genesis predicate.negativeControls",
        non_empty=True,
    )
    control_ids = []
    for index, raw in enumerate(controls):
        control = require_object(raw, f"negativeControl[{index}]")
        exact_keys(
            control,
            ("id", "expected", "observed", "passed", "artifact"),
            f"negativeControl[{index}]",
        )
        control_id = require_string(control["id"], f"negativeControl[{index}].id")
        if not ID_RE.fullmatch(control_id):
            raise EvidenceError(f"negativeControl[{index}].id is invalid")
        control_ids.append(control_id)
        require_string(control["expected"], f"negativeControl[{index}].expected")
        require_string(control["observed"], f"negativeControl[{index}].observed")
        if control["passed"] is not True:
            raise EvidenceError(f"negativeControl[{index}] did not pass")
        if control["artifact"] is not None:
            validate_artifact_identity(
                control["artifact"], f"negativeControl[{index}].artifact"
            )
    require_unique_sorted(control_ids, "genesis predicate.negativeControls")

    artifacts = require_array(
        predicate["artifacts"], "genesis predicate.artifacts", non_empty=True
    )
    artifact_names = []
    for index, raw in enumerate(artifacts):
        artifact = require_object(raw, f"artifact[{index}]")
        exact_keys(
            artifact,
            ("name", "path", "digest", "sizeBytes", "mediaType"),
            f"artifact[{index}]",
        )
        artifact_names.append(
            require_string(artifact["name"], f"artifact[{index}].name")
        )
        require_relative_path(artifact["path"], f"artifact[{index}].path")
        require_digest(artifact["digest"], f"artifact[{index}].digest")
        require_int(artifact["sizeBytes"], f"artifact[{index}].sizeBytes", minimum=0)
        require_string(artifact["mediaType"], f"artifact[{index}].mediaType")
    require_unique_sorted(artifact_names, "genesis predicate.artifacts")

    measurements = require_object(
        predicate["measurements"], "genesis predicate.measurements"
    )
    exact_keys(
        measurements,
        ("durationNs", "peakRssBytes", "diskDeltaBytes", "rawSamples"),
        "genesis predicate.measurements",
    )
    require_int(measurements["durationNs"], "measurements.durationNs", minimum=0)
    require_int(measurements["peakRssBytes"], "measurements.peakRssBytes", minimum=0)
    require_int(measurements["diskDeltaBytes"], "measurements.diskDeltaBytes")
    samples = require_array(
        measurements["rawSamples"], "measurements.rawSamples", non_empty=True
    )
    sample_names = []
    for index, raw in enumerate(samples):
        sample = require_object(raw, f"rawSample[{index}]")
        exact_keys(sample, ("metric", "unit", "values"), f"rawSample[{index}]")
        metric = require_string(sample["metric"], f"rawSample[{index}].metric")
        if not ID_RE.fullmatch(metric):
            raise EvidenceError(f"rawSample[{index}].metric is invalid")
        sample_names.append(metric)
        if sample["unit"] not in ("ns", "bytes", "count", "basis-points"):
            raise EvidenceError(f"rawSample[{index}].unit is invalid")
        values = require_array(
            sample["values"], f"rawSample[{index}].values", non_empty=True
        )
        for value_index, sample_value in enumerate(values):
            require_int(sample_value, f"rawSample[{index}].values[{value_index}]")
    require_unique_sorted(sample_names, "measurements.rawSamples")

    verifier = require_object(predicate["verifier"], "genesis predicate.verifier")
    exact_keys(verifier, ("name", "version", "artifact"), "genesis predicate.verifier")
    require_string(verifier["name"], "verifier.name")
    require_string(verifier["version"], "verifier.version")
    validate_artifact_identity(verifier["artifact"], "verifier.artifact")

    refs = require_array(predicate["evidenceRefs"], "genesis predicate.evidenceRefs")
    ref_uris = []
    for index, raw in enumerate(refs):
        ref = require_object(raw, f"evidenceRef[{index}]")
        exact_keys(ref, ("kind", "uri", "digest", "mediaType"), f"evidenceRef[{index}]")
        require_string(ref["kind"], f"evidenceRef[{index}].kind")
        ref_uris.append(require_uri(ref["uri"], f"evidenceRef[{index}].uri"))
        require_digest(ref["digest"], f"evidenceRef[{index}].digest")
        require_string(ref["mediaType"], f"evidenceRef[{index}].mediaType")
    require_unique_sorted(ref_uris, "genesis predicate.evidenceRefs")
    return predicate


def validate_genesis_statement(statement: Mapping[str, Any]) -> None:
    subjects = validate_subjects(statement["subject"], "statement.subject")
    predicate = validate_genesis_predicate(statement["predicate"])
    expected = {
        (artifact["path"], artifact["digest"]["sha256"])
        for artifact in predicate["artifacts"]
    }
    observed = {(subject["name"], subject["digest"]["sha256"]) for subject in subjects}
    if observed != expected:
        raise EvidenceError(
            "Genesis statement subjects must exactly match predicate.artifacts"
        )


def validate_resource_descriptor(value: Any, label: str) -> None:
    descriptor = require_object(value, label)
    allowed = {"uri", "name", "digest", "mediaType", "annotations"}
    unknown = sorted(set(descriptor) - allowed)
    if unknown:
        raise EvidenceError(
            f"{label} contains unsupported producer fields: {', '.join(unknown)}"
        )
    if "digest" not in descriptor:
        raise EvidenceError(f"{label} missing fields: digest")
    if "uri" in descriptor:
        require_uri(descriptor["uri"], f"{label}.uri")
    if "name" in descriptor:
        require_string(descriptor["name"], f"{label}.name")
    require_digest(descriptor["digest"], f"{label}.digest")
    if "mediaType" in descriptor:
        require_string(descriptor["mediaType"], f"{label}.mediaType")
    if "annotations" in descriptor:
        require_object(descriptor["annotations"], f"{label}.annotations")


def validate_slsa_statement(statement: Mapping[str, Any]) -> None:
    subjects = require_array(
        statement["subject"], "SLSA statement.subject", non_empty=True
    )
    for index, subject in enumerate(subjects):
        validate_resource_descriptor(subject, f"SLSA statement.subject[{index}]")
    predicate = require_object(statement["predicate"], "SLSA statement.predicate")
    exact_keys(predicate, ("buildDefinition", "runDetails"), "SLSA statement.predicate")
    definition = require_object(predicate["buildDefinition"], "SLSA buildDefinition")
    exact_keys(
        definition,
        (
            "buildType",
            "externalParameters",
            "internalParameters",
            "resolvedDependencies",
        ),
        "SLSA buildDefinition",
    )
    if definition["buildType"] != BUILD_TYPE:
        raise EvidenceError("SLSA buildDefinition.buildType is unsupported")
    external = require_object(
        definition["externalParameters"], "SLSA externalParameters"
    )
    exact_keys(external, ("commands", "evidenceProfile"), "SLSA externalParameters")
    if external["evidenceProfile"] != "0.1":
        raise EvidenceError("SLSA evidenceProfile is unsupported")
    require_array(
        external["commands"], "SLSA externalParameters.commands", non_empty=True
    )
    internal = require_object(
        definition["internalParameters"], "SLSA internalParameters"
    )
    exact_keys(
        internal, ("environmentProfile", "networkMode"), "SLSA internalParameters"
    )
    require_string(
        internal["environmentProfile"], "SLSA internalParameters.environmentProfile"
    )
    if internal["networkMode"] not in ("deny", "declared-only"):
        raise EvidenceError("SLSA internalParameters.networkMode is invalid")
    dependencies = require_array(
        definition["resolvedDependencies"], "SLSA resolvedDependencies", non_empty=True
    )
    for index, dependency in enumerate(dependencies):
        validate_resource_descriptor(dependency, f"SLSA resolvedDependencies[{index}]")
    details = require_object(predicate["runDetails"], "SLSA runDetails")
    exact_keys(details, ("builder", "metadata", "byproducts"), "SLSA runDetails")
    builder = require_object(details["builder"], "SLSA builder")
    exact_keys(builder, ("id", "version", "builderDependencies"), "SLSA builder")
    if builder["id"] != BUILDER_ID:
        raise EvidenceError("SLSA builder.id is unsupported")
    versions = require_object(builder["version"], "SLSA builder.version")
    if not versions:
        raise EvidenceError("SLSA builder.version must not be empty")
    builder_dependencies = require_array(
        builder["builderDependencies"], "SLSA builderDependencies"
    )
    for index, dependency in enumerate(builder_dependencies):
        validate_resource_descriptor(dependency, f"SLSA builderDependencies[{index}]")
    metadata = require_object(details["metadata"], "SLSA metadata")
    exact_keys(metadata, ("invocationId",), "SLSA metadata")
    require_string(metadata["invocationId"], "SLSA metadata.invocationId")
    byproducts = require_array(details["byproducts"], "SLSA byproducts", non_empty=True)
    for index, byproduct in enumerate(byproducts):
        validate_resource_descriptor(byproduct, f"SLSA byproducts[{index}]")


def validate_statement(value: Any) -> Mapping[str, Any]:
    statement = require_object(value, "statement")
    exact_keys(
        statement, ("_type", "subject", "predicateType", "predicate"), "statement"
    )
    if statement["_type"] != STATEMENT_TYPE:
        raise EvidenceError("statement._type must be in-toto Statement v1")
    predicate_type = statement["predicateType"]
    if predicate_type == GENESIS_PREDICATE_TYPE:
        validate_genesis_statement(statement)
    elif predicate_type == SLSA_PREDICATE_TYPE:
        validate_slsa_statement(statement)
    else:
        raise EvidenceError(f"unsupported statement.predicateType: {predicate_type}")
    return statement


def dsse_pae(payload_type: str, payload: bytes) -> bytes:
    type_bytes = payload_type.encode("utf-8")
    return b"DSSEv1 %d %s %d %s" % (
        len(type_bytes),
        type_bytes,
        len(payload),
        payload,
    )


def fixture_artifact_tree() -> Mapping[str, Any]:
    path = FIXTURE_ARTIFACT_PATH.encode("utf-8")
    digest = sha256(FIXTURE_ARTIFACT_BYTES).digest()
    leaf = sha256()
    leaf.update(b"GenesisCodeHashTreeLeafv0.1\0")
    leaf.update(len(path).to_bytes(8, "big"))
    leaf.update(path)
    leaf.update(len(FIXTURE_ARTIFACT_BYTES).to_bytes(8, "big"))
    leaf.update(digest)
    return {
        "kind": "genesis/artifact-hash-tree-v0.1",
        "version": "0.1",
        "algorithm": "sha256-merkle-v0.1",
        "rootDigest": {"sha256": leaf.hexdigest()},
        "entries": [
            {
                "path": FIXTURE_ARTIFACT_PATH,
                "digest": {"sha256": digest.hex()},
                "sizeBytes": len(FIXTURE_ARTIFACT_BYTES),
                "type": "file",
            }
        ],
    }


def validate_envelope(value: Any, statement: Mapping[str, Any], label: str) -> None:
    envelope = require_object(value, label)
    exact_keys(envelope, ("payloadType", "payload", "signatures"), label)
    if envelope["payloadType"] != PAYLOAD_TYPE:
        raise EvidenceError(f"{label}.payloadType is unsupported")
    try:
        payload = base64.b64decode(envelope["payload"], validate=True)
    except (ValueError, TypeError) as exc:
        raise EvidenceError(f"{label}.payload is not canonical base64") from exc
    expected = canonical_bytes(statement)
    if payload != expected:
        raise EvidenceError(
            f"{label}.payload does not equal the canonical statement bytes"
        )
    # Constructing PAE here makes the signed-byte contract executable even before R0.2.c.
    if not dsse_pae(PAYLOAD_TYPE, payload).startswith(b"DSSEv1 "):
        raise EvidenceError(f"{label} failed DSSE PAE construction")
    signatures = require_array(
        envelope["signatures"], f"{label}.signatures", non_empty=True
    )
    key_ids = []
    for index, raw in enumerate(signatures):
        signature = require_object(raw, f"{label}.signatures[{index}]")
        exact_keys(signature, ("keyid", "sig"), f"{label}.signatures[{index}]")
        key_id = require_string(
            signature["keyid"], f"{label}.signatures[{index}].keyid"
        )
        if not re.fullmatch(r"sha256:[0-9a-f]{64}", key_id):
            raise EvidenceError(f"{label}.signatures[{index}].keyid is invalid")
        key_ids.append(key_id)
        try:
            decoded = base64.b64decode(signature["sig"], validate=True)
        except (ValueError, TypeError) as exc:
            raise EvidenceError(
                f"{label}.signatures[{index}].sig is not base64"
            ) from exc
        if len(decoded) != 64:
            raise EvidenceError(f"{label}.signatures[{index}].sig must be 64 bytes")
    require_unique_sorted(key_ids, f"{label}.signatures")


def validate_bundle(value: Any, *, fixture: bool = False) -> Mapping[str, Any]:
    reject_floats(value, "evidence bundle")
    bundle = require_object(value, "evidence bundle")
    exact_keys(
        bundle, ("kind", "version", "profile", "attestations"), "evidence bundle"
    )
    if bundle["kind"] != "genesis/evidence-bundle-v0.1":
        raise EvidenceError("evidence bundle.kind is unsupported")
    if bundle["version"] != "0.1":
        raise EvidenceError("evidence bundle.version is unsupported")
    if bundle["profile"] not in ("E1", "E2", "E3", "E4"):
        raise EvidenceError("evidence bundle.profile is invalid")
    attestations = require_array(
        bundle["attestations"], "evidence bundle.attestations", non_empty=True
    )
    predicate_types = []
    for index, raw in enumerate(attestations):
        attestation = require_object(raw, f"attestation[{index}]")
        exact_keys(
            attestation, ("mediaType", "statement", "envelope"), f"attestation[{index}]"
        )
        if attestation["mediaType"] != PAYLOAD_TYPE:
            raise EvidenceError(f"attestation[{index}].mediaType is unsupported")
        statement = validate_statement(attestation["statement"])
        predicate_types.append(statement["predicateType"])
        if bundle["profile"] in ("E3", "E4") and attestation["envelope"] is None:
            raise EvidenceError(
                f"attestation[{index}] requires authentication for {bundle['profile']}"
            )
        if attestation["envelope"] is not None:
            validate_envelope(
                attestation["envelope"], statement, f"attestation[{index}].envelope"
            )
        if fixture:
            envelope = require_object(
                attestation["envelope"], f"attestation[{index}].envelope"
            )
            signature = envelope["signatures"][0]
            if signature["keyid"] != FIXTURE_KEY_ID:
                raise EvidenceError("fixture key identity drift")
            if signature["sig"] != FIXTURE_SIGNATURES[statement["predicateType"]]:
                raise EvidenceError("fixture signature drift")
    expected_order = [GENESIS_PREDICATE_TYPE, SLSA_PREDICATE_TYPE]
    if predicate_types != expected_order:
        raise EvidenceError(
            "bundle must contain Genesis then SLSA statements exactly once"
        )
    genesis_subject = attestations[0]["statement"]["subject"]
    slsa_subject = attestations[1]["statement"]["subject"]
    if genesis_subject != slsa_subject:
        raise EvidenceError("Genesis and SLSA statements must bind identical subjects")
    return bundle


def fixture_statements() -> List[Mapping[str, Any]]:
    artifact_digest = sha256(FIXTURE_ARTIFACT_BYTES).hexdigest()
    source_tree_digest = "b" * 64
    artifact_tree = fixture_artifact_tree()
    artifact_tree_digest = sha256(canonical_bytes(artifact_tree)).hexdigest()
    tool_digest = "c" * 64
    verifier_digest = "d" * 64
    control_digest = "e" * 64
    genesis_statement: Mapping[str, Any] = {
        "_type": STATEMENT_TYPE,
        "subject": [
            {
                "name": FIXTURE_ARTIFACT_PATH,
                "digest": {"sha256": artifact_digest},
            }
        ],
        "predicateType": GENESIS_PREDICATE_TYPE,
        "predicate": {
            "kind": "genesis/evidence-predicate-v0.1",
            "version": "0.1",
            "evidenceClass": "E3",
            "source": {
                "vcs": "git",
                "repositoryUri": "https://example.invalid/genesisCode.git",
                "revision": "0123456789abcdef0123456789abcdef01234567",
                "treeDigest": {"sha256": source_tree_digest},
                "dirty": False,
                "dirtyPolicy": "reject",
                "dirtyPathsDigest": None,
            },
            "toolchains": [
                {
                    "name": "genesis",
                    "version": "0.2.0",
                    "artifact": {
                        "uri": "urn:genesis:toolchain:genesis:0.2.0",
                        "digest": {"sha256": tool_digest},
                    },
                }
            ],
            "environment": {
                "profile": "darwin-arm64/hermetic-v0.1",
                "os": "darwin",
                "architecture": "arm64",
                "container": None,
                "declaredVariables": ["LANG", "SOURCE_DATE_EPOCH", "TZ"],
            },
            "networkPolicy": {"mode": "deny", "inputs": []},
            "commands": [
                {
                    "argv": ["bash", "scripts/check_genesis_evidence_profile.sh"],
                    "cwd": ".",
                    "declaredEnvironment": ["LANG", "SOURCE_DATE_EPOCH", "TZ"],
                    "exitCode": 0,
                }
            ],
            "negativeControls": [
                {
                    "id": "duplicate-json-key",
                    "expected": "reject",
                    "observed": "rejected",
                    "passed": True,
                    "artifact": {
                        "uri": "urn:genesis:evidence:negative-control:duplicate-json-key",
                        "digest": {"sha256": control_digest},
                    },
                }
            ],
            "artifacts": [
                {
                    "name": "genesis-example",
                    "path": FIXTURE_ARTIFACT_PATH,
                    "digest": {"sha256": artifact_digest},
                    "sizeBytes": len(FIXTURE_ARTIFACT_BYTES),
                    "mediaType": "application/octet-stream",
                }
            ],
            "measurements": {
                "durationNs": 12500000,
                "peakRssBytes": 67108864,
                "diskDeltaBytes": 4096,
                "rawSamples": [
                    {
                        "metric": "duration",
                        "unit": "ns",
                        "values": [12400000, 12500000, 12600000],
                    },
                    {
                        "metric": "peak-rss",
                        "unit": "bytes",
                        "values": [66060288, 67108864],
                    },
                ],
            },
            "verifier": {
                "name": "genesis-evidence-profile",
                "version": "0.1.0",
                "artifact": {
                    "uri": "urn:genesis:verifier:evidence-profile:0.1.0",
                    "digest": {"sha256": verifier_digest},
                },
            },
            "evidenceRefs": [
                {
                    "kind": "genesis/artifact-hash-tree-v0.1",
                    "uri": "urn:genesis:hash-tree:sha256:" + artifact_tree_digest,
                    "digest": {"sha256": artifact_tree_digest},
                    "mediaType": "application/vnd.genesiscode.artifact-hash-tree+json",
                },
                {
                    "kind": "genesis/acceptance-v0.2",
                    "uri": "urn:genesis:store:sha256:" + control_digest,
                    "digest": {"sha256": control_digest},
                    "mediaType": "application/vnd.genesiscode.coreform",
                },
            ],
        },
    }
    genesis_digest = sha256(canonical_bytes(genesis_statement)).hexdigest()
    slsa_statement: Mapping[str, Any] = {
        "_type": STATEMENT_TYPE,
        "subject": deepcopy(genesis_statement["subject"]),
        "predicateType": SLSA_PREDICATE_TYPE,
        "predicate": {
            "buildDefinition": {
                "buildType": BUILD_TYPE,
                "externalParameters": {
                    "commands": deepcopy(genesis_statement["predicate"]["commands"]),
                    "evidenceProfile": "0.1",
                },
                "internalParameters": {
                    "environmentProfile": "darwin-arm64/hermetic-v0.1",
                    "networkMode": "deny",
                },
                "resolvedDependencies": [
                    {
                        "uri": "git+https://example.invalid/genesisCode.git@0123456789abcdef0123456789abcdef01234567",
                        "digest": {"sha256": source_tree_digest},
                        "annotations": {"genesis_vcs": "git"},
                    },
                    {
                        "uri": "urn:genesis:toolchain:genesis:0.2.0",
                        "digest": {"sha256": tool_digest},
                        "annotations": {"genesis_version": "0.2.0"},
                    },
                ],
            },
            "runDetails": {
                "builder": {
                    "id": BUILDER_ID,
                    "version": {"genesis-evidence-profile": "0.1.0"},
                    "builderDependencies": [],
                },
                "metadata": {
                    "invocationId": "urn:genesis:invocation:sha256:" + genesis_digest
                },
                "byproducts": [
                    {
                        "uri": "urn:genesis:statement:sha256:" + genesis_digest,
                        "digest": {"sha256": genesis_digest},
                        "mediaType": PAYLOAD_TYPE,
                    }
                ],
            },
        },
    }
    return [genesis_statement, slsa_statement]


def render_fixture() -> Mapping[str, Any]:
    attestations = []
    for statement in fixture_statements():
        payload = canonical_bytes(statement)
        predicate_type = statement["predicateType"]
        attestations.append(
            {
                "mediaType": PAYLOAD_TYPE,
                "statement": statement,
                "envelope": {
                    "payloadType": PAYLOAD_TYPE,
                    "payload": base64.b64encode(payload).decode("ascii"),
                    "signatures": [
                        {
                            "keyid": FIXTURE_KEY_ID,
                            "sig": FIXTURE_SIGNATURES[predicate_type],
                        }
                    ],
                },
            }
        )
    return {
        "kind": "genesis/evidence-bundle-v0.1",
        "version": "0.1",
        "profile": "E3",
        "attestations": attestations,
    }


def expect_rejected(value: Any, label: str) -> None:
    try:
        validate_bundle(value)
    except EvidenceError:
        return
    raise EvidenceError(f"negative control was accepted: {label}")


def self_test() -> int:
    validate_schema_contracts()
    fixture = render_fixture()
    validate_bundle(fixture, fixture=True)
    public_key = bytes.fromhex(FIXTURE_PUBLIC_KEY_HEX)
    if "sha256:" + sha256(public_key).hexdigest() != FIXTURE_KEY_ID:
        raise EvidenceError("fixture public key does not match fixture key identity")
    mutations = []

    def case(label: str, mutator: Any) -> None:
        value = deepcopy(fixture)
        mutator(value)
        mutations.append((label, value))

    case("unsupported bundle version", lambda value: value.update(version="9"))
    case(
        "missing statement type",
        lambda value: value["attestations"][0]["statement"].pop("_type"),
    )
    case(
        "unknown predicate field",
        lambda value: value["attestations"][0]["statement"]["predicate"].update(
            extra=True
        ),
    )
    case(
        "dirty input without policy",
        lambda value: value["attestations"][0]["statement"]["predicate"][
            "source"
        ].update(dirty=True),
    )
    case(
        "host path",
        lambda value: value["attestations"][0]["statement"]["predicate"]["commands"][
            0
        ].update(cwd="/Users/alice/repo"),
    )
    case(
        "shell string",
        lambda value: value["attestations"][0]["statement"]["predicate"]["commands"][
            0
        ].update(argv="cargo test"),
    )
    case(
        "undeclared network",
        lambda value: value["attestations"][0]["statement"]["predicate"][
            "networkPolicy"
        ].update(
            inputs=[
                {
                    "uri": "https://example.invalid/input",
                    "digest": {"sha256": "f" * 64},
                    "purpose": "test",
                }
            ]
        ),
    )
    case(
        "failed negative control",
        lambda value: value["attestations"][0]["statement"]["predicate"][
            "negativeControls"
        ][0].update(passed=False),
    )
    case(
        "floating sample",
        lambda value: value["attestations"][0]["statement"]["predicate"][
            "measurements"
        ]["rawSamples"][0]["values"].append(1.5),
    )
    case(
        "subject mismatch",
        lambda value: value["attestations"][0]["statement"]["subject"][0][
            "digest"
        ].update(sha256="f" * 64),
    )
    case(
        "missing E3 envelope",
        lambda value: value["attestations"][0].update(envelope=None),
    )
    case(
        "payload substitution",
        lambda value: value["attestations"][0]["envelope"].update(
            payload=base64.b64encode(b"{}").decode("ascii")
        ),
    )
    case(
        "wrong payload type",
        lambda value: value["attestations"][0]["envelope"].update(
            payloadType="application/json"
        ),
    )
    case(
        "short signature",
        lambda value: value["attestations"][0]["envelope"]["signatures"][0].update(
            sig=base64.b64encode(b"short").decode("ascii")
        ),
    )
    case("statement reorder", lambda value: value["attestations"].reverse())
    case(
        "SLSA type confusion",
        lambda value: value["attestations"][1]["statement"].update(
            predicateType="https://slsa.dev/provenance/v2"
        ),
    )
    for label, value in mutations:
        expect_rejected(value, label)
    return len(mutations)


def render_to(path: Path) -> None:
    fixture = render_fixture()
    validate_bundle(fixture, fixture=True)
    path.parent.mkdir(parents=True, exist_ok=True)
    path.write_bytes(retained_bytes(fixture))


def check_vector(path: Path) -> int:
    validate_schema_contracts()
    observed = load_json(path)
    validate_bundle(observed, fixture=True)
    expected = render_fixture()
    if observed != expected or path.read_bytes() != retained_bytes(expected):
        raise EvidenceError(
            "generated vector drift: run bash scripts/update_genesis_evidence_profile.sh"
        )
    return self_test()


def main(argv: Sequence[str]) -> int:
    parser = argparse.ArgumentParser()
    mode = parser.add_mutually_exclusive_group(required=True)
    mode.add_argument("--check", action="store_true")
    mode.add_argument("--update", action="store_true")
    mode.add_argument("--render", action="store_true")
    mode.add_argument("--self-test", action="store_true")
    parser.add_argument("--input", type=Path, default=DEFAULT_VECTOR)
    parser.add_argument("--output", type=Path)
    args = parser.parse_args(argv)
    try:
        if args.check:
            count = check_vector(args.input.resolve())
            print(
                "genesis-evidence-profile: ok "
                f"(profile=0.1 attestations=2 negative_controls={count})"
            )
        elif args.update:
            target = (args.output or args.input).resolve()
            render_to(target)
            print(f"genesis-evidence-profile: updated {display_path(target)}")
        elif args.render:
            if args.output is None:
                raise EvidenceError("--render requires --output")
            render_to(args.output.resolve())
        else:
            count = self_test()
            print(f"genesis-evidence-profile: self-test ok (negative_controls={count})")
    except EvidenceError as exc:
        print(f"genesis-evidence-profile: {exc}", file=sys.stderr)
        return 1
    return 0


if __name__ == "__main__":
    raise SystemExit(main(sys.argv[1:]))
