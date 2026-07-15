#!/usr/bin/env python3
"""Build and verify content-addressed GenesisCode agent benchmark run bundles."""

from __future__ import annotations

import argparse
import copy
import hashlib
import json
import os
import re
import stat
import subprocess
import sys
from pathlib import Path, PurePosixPath
from typing import Any

from gc_agent_scoring import parse_toml_subset
from gc_agent_scoring_contract import validate_score


ROOT = Path(__file__).resolve().parents[2]
SCHEMA = ROOT / "docs/spec/GC_AGENT_BENCHMARK_RUN_v0.1.schema.json"
EXAMPLE = ROOT / "examples/agent_benchmark_reproducibility/run.json"
BENCHMARK = ROOT / "benchmarks/agent_tasks/v0.1/suite.json"
SCORING = ROOT / "docs/spec/GC_AGENT_BENCHMARK_SCORING_v0.1.json"
MODEL_EFFECT_PROFILE = ROOT / "docs/spec/GC_AGENT_MODEL_RUNNER_EFFECT_v0.1.json"
SHA_RE = re.compile(r"^[0-9a-f]{64}$")
ID_RE = re.compile(r"^[A-Za-z0-9][A-Za-z0-9._-]{0,127}$")
HOST_PATH_RE = re.compile(r"(?:/Users/|/home/|[A-Za-z]:\\\\Users\\\\)")

TOP_KEYS = {
    "kind", "version", "runId", "status", "benchmark", "authorities", "model",
    "invocation", "toolProtocol", "host", "candidate", "score",
    "modelSpecificMetrics", "artifactInventory", "contentIdentitySha256",
}
REQUIRED_AUTHORITIES = {
    "agent-core-card": "repository:docs/spec/GC_AGENT_CORE_CARD_v0.3.json",
    "agent-profile": "repository:docs/spec/GC_AGENT_PROFILE_v0.3.json",
    "agent-task-cards": "repository:docs/spec/GC_AGENT_TASK_CARDS_v0.3.json",
    "benchmark-run-integration-test": "repository:crates/gc_cli/tests/cli_agent_benchmark_run.rs",
    "benchmark-run-schema": "repository:docs/spec/GC_AGENT_BENCHMARK_RUN_v0.1.schema.json",
    "benchmark-run-verifier": "repository:scripts/lib/gc_agent_benchmark_run.py",
    "model-runner-effect": "repository:docs/spec/GC_AGENT_MODEL_RUNNER_EFFECT_v0.1.json",
    "scoring-authority": "repository:docs/spec/GC_AGENT_BENCHMARK_SCORING_v0.1.json",
    "task-benchmark": "repository:benchmarks/agent_tasks/v0.1/suite.json",
}
ARTIFACT_ROLES = {
    "authority", "prompt", "card", "context", "model", "runtime", "request",
    "response", "model-output", "tool-catalog", "tool-policy", "tool-transcript",
    "effect-log", "candidate", "score",
}
MEDIA_TYPES = {
    ".gc": "text/plain",
    ".gclog": "text/plain",
    ".json": "application/json",
    ".jsonl": "application/x-ndjson",
    ".md": "text/markdown",
    ".py": "text/x-python",
    ".toml": "application/toml",
    ".txt": "text/plain",
    ".fixture": "application/octet-stream",
}


class RunError(ValueError):
    pass


def require(condition: bool, message: str) -> None:
    if not condition:
        raise RunError(message)


def reject_pairs(rows: list[tuple[str, Any]]) -> dict[str, Any]:
    result: dict[str, Any] = {}
    for key, value in rows:
        require(key not in result, f"duplicate JSON key: {key}")
        result[key] = value
    return result


def load_json(path: Path) -> Any:
    try:
        return json.loads(path.read_text(encoding="utf-8"), object_pairs_hook=reject_pairs)
    except (OSError, UnicodeDecodeError, json.JSONDecodeError) as exc:
        raise RunError(f"cannot load JSON {path.name}: {exc}") from exc


def canonical_bytes(value: Any) -> bytes:
    return (json.dumps(value, sort_keys=True, separators=(",", ":"), ensure_ascii=True) + "\n").encode("ascii")


def sha256_bytes(value: bytes) -> str:
    return hashlib.sha256(value).hexdigest()


def file_sha256(path: Path) -> str:
    return sha256_bytes(path.read_bytes())


def object_identity(value: dict[str, Any], field: str) -> str:
    material = copy.deepcopy(value)
    material.pop(field, None)
    return sha256_bytes(canonical_bytes(material))


def closed(value: Any, keys: set[str], label: str) -> dict[str, Any]:
    require(isinstance(value, dict) and set(value) == keys, f"{label} fields are not closed")
    return value


def sorted_unique(values: list[str], label: str) -> None:
    require(values == sorted(values) and len(values) == len(set(values)), f"{label} must be sorted and unique")


def safe_relative(value: str, label: str) -> PurePosixPath:
    require(isinstance(value, str) and value and len(value) <= 512, f"invalid {label}")
    require("\\" not in value and not value.startswith("/") and "//" not in value, f"unsafe {label}")
    relative = PurePosixPath(value)
    require(all(part not in {"", ".", ".."} for part in relative.parts), f"unsafe {label}")
    require(not HOST_PATH_RE.search(value), f"host path in {label}")
    return relative


def resolve_regular(root: Path, relative: str, label: str) -> Path:
    parts = safe_relative(relative, label).parts
    current = root
    for part in parts:
        current = current / part
        require(not current.is_symlink(), f"symlink in {label}: {relative}")
    require(current.is_file(), f"missing regular {label}: {relative}")
    return current


def validate_schema() -> None:
    schema = load_json(SCHEMA)
    require(schema.get("$schema") == "https://json-schema.org/draft/2020-12/schema", "run schema draft drift")
    require(schema.get("$id") == "https://genesiscode.dev/schemas/gc-agent-benchmark-run-v0.1.json", "run schema id drift")

    def walk(value: Any, label: str) -> None:
        if isinstance(value, dict):
            if value.get("type") == "object":
                require(value.get("additionalProperties") is False, f"schema object is open: {label}")
                require(set(value.get("required", [])) == set(value.get("properties", {})), f"schema object is not recursively required: {label}")
            for key, child in value.items():
                walk(child, f"{label}/{key}")
        elif isinstance(value, list):
            for index, child in enumerate(value):
                walk(child, f"{label}/{index}")

    walk(schema, "schema")


def validate_model_effect_profile(document: Any | None = None) -> dict[str, Any]:
    profile = load_json(MODEL_EFFECT_PROFILE) if document is None else document
    expected = {
        "capabilityPolicy": {
            "allowCommands": ["infer"],
            "allowOperations": ["host/plugin::command"],
            "allowPlugins": ["genesis.agent-model-runner.v0.1"],
            "bridgeCommandPath": "base-relative",
            "bridgeExecutableDigest": "sha256-required",
            "maxBytes": "finite-positive",
            "networkMode": "denied",
            "timeoutMs": "finite-positive-hard-kill-and-reap",
            "wildcardsAllowed": False,
        },
        "effect": {
            "command": "infer",
            "operation": "host/plugin::command",
            "plugin": "genesis.agent-model-runner.v0.1",
            "transports": ["persistent-stdio", "spawn-per-op"],
        },
        "inventoryRequirements": [
            "bridge-executable", "effect-log", "model-output", "policy",
            "request", "response", "tool-transcript",
        ],
        "kind": "genesis/agent-model-runner-effect-profile-v0.1",
        "privacy": {
            "forbidden": [
                "absolute-paths", "credentials", "held-out-oracles", "hostnames",
                "serial-numbers", "usernames",
            ],
            "hostFacts": "normalized-run-schema-dimensions",
            "modelMetricsIncludedInQuality": False,
        },
        "replay": {
            "bridgeInvocation": "forbidden",
            "claim": "effect-transcript-reproducibility",
            "modelWeightsAccess": "forbidden",
            "regenerationRequiresDeterministicRuntime": True,
        },
        "request": {
            "ambientChannelsForbidden": [
                "credentials", "environment", "paths", "prompt-text",
                "sampling-authority",
            ],
            "encoding": "closed-coreform-map",
            "fields": [
                "decoding-sha256", "model-id", "model-revision",
                "prompt-assembly-sha256", "request-sha256", "tokenizer-sha256",
                "weights-sha256",
            ],
            "hashAlgorithm": "sha256-lowercase",
        },
        "response": {
            "encoding": "closed-coreform-map",
            "failureFields": ["code", "message", "ok"],
            "successFields": [
                "finish-reason", "model-id", "model-revision", "ok", "output",
                "usage",
            ],
            "tokenUsage": "nonnegative-integers",
        },
        "scope": {
            "purpose": "r1.4-benchmark-reproducibility-only",
            "standardModelApiOwner": "R5.4.e",
        },
        "version": "0.1.0",
    }
    require(isinstance(profile, dict), "model effect profile must be an object")
    claimed = profile.get("contentIdentitySha256")
    unsigned = {key: value for key, value in profile.items() if key != "contentIdentitySha256"}
    require(unsigned == expected, "model effect profile authority drift")
    require(
        isinstance(claimed, str)
        and SHA_RE.fullmatch(claimed) is not None
        and claimed == object_identity(profile, "contentIdentitySha256"),
        "model effect profile identity drift",
    )
    return profile


def inventory_identity(files: dict[str, bytes]) -> str:
    rows = [
        {"path": path, "bytes": len(payload), "sha256": sha256_bytes(payload)}
        for path, payload in sorted(files.items())
    ]
    return sha256_bytes(canonical_bytes(rows))


def prompt_identity(document: dict[str, Any], artifacts: dict[str, Path]) -> str:
    assembly = document["invocation"]["promptAssembly"]
    ordered = [row["artifact"] for row in assembly["assemblyOrder"]]
    digest = hashlib.sha256()
    digest.update(b"genesis-agent-prompt-assembly-v0.1\0")
    for key in ordered:
        require(key in artifacts, f"prompt references unknown artifact: {key}")
        digest.update(key.encode("utf-8"))
        digest.update(b"\0")
        digest.update(artifacts[key].read_bytes())
        digest.update(b"\0")
    return digest.hexdigest()


def case_identity(case: dict[str, Any]) -> str:
    return sha256_bytes(canonical_bytes(case))


def identity_field(value: dict[str, Any], field: str, label: str) -> None:
    require(SHA_RE.fullmatch(value.get(field, "")) is not None, f"invalid {label} identity")
    require(value[field] == object_identity(value, field), f"{label} identity drift")


def artifact_map(document: dict[str, Any], bundle_root: Path) -> tuple[dict[str, dict[str, Any]], dict[str, Path]]:
    rows = document["artifactInventory"]
    require(isinstance(rows, list) and rows, "artifact inventory is empty")
    keys = [row.get("key") for row in rows if isinstance(row, dict)]
    require(len(keys) == len(rows), "artifact row must be an object")
    sorted_unique(keys, "artifact keys")
    records: dict[str, dict[str, Any]] = {}
    paths: dict[str, Path] = {}
    bundle_paths: set[str] = set()
    for row in rows:
        closed(row, {"key", "scope", "path", "sha256", "bytes", "mediaType", "role", "mode"}, "artifact")
        scope = row["scope"]
        require(scope in {"repository", "bundle"}, "invalid artifact scope")
        relative = row["path"]
        require(row["key"] == f"{scope}:{relative}", "artifact key does not bind scope and path")
        path = resolve_regular(ROOT if scope == "repository" else bundle_root, relative, "artifact path")
        payload = path.read_bytes()
        require(row["bytes"] == len(payload) and row["sha256"] == sha256_bytes(payload), f"stale artifact facts: {row['key']}")
        require(row["mediaType"] == MEDIA_TYPES.get(path.suffix, "application/octet-stream"), f"media type drift: {row['key']}")
        require(row["role"] in ARTIFACT_ROLES, f"invalid artifact role: {row['key']}")
        observed_mode = stat.S_IMODE(path.stat().st_mode)
        expected_mode = 0o755 if row["mode"] == "0755" else 0o644
        require(observed_mode & 0o111 == expected_mode & 0o111, f"executable mode drift: {row['key']}")
        records[row["key"]] = row
        paths[row["key"]] = path
        if scope == "bundle":
            bundle_paths.add(relative)

    observed_bundle: set[str] = set()
    for directory, dirnames, filenames in os.walk(bundle_root, followlinks=False):
        current = Path(directory)
        for name in dirnames:
            require(not (current / name).is_symlink(), "bundle contains a symlink directory")
        for name in filenames:
            path = current / name
            require(not path.is_symlink() and path.is_file(), "bundle contains a non-regular file")
            relative = path.relative_to(bundle_root).as_posix()
            if relative != "run.json":
                observed_bundle.add(relative)
    require(bundle_paths == observed_bundle, "artifact inventory does not cover the complete bundle tree")
    return records, paths


def validate_document(document: Any, run_path: Path = EXAMPLE, *, check_files: bool = True) -> dict[str, Any]:
    doc = closed(document, TOP_KEYS, "benchmark run")
    require(doc["kind"] == "genesis/agent-benchmark-run-v0.1" and doc["version"] == "0.1.0", "benchmark run version drift")
    require(ID_RE.fullmatch(doc["runId"]) is not None and doc["status"] == "complete", "example run is not complete")
    serialized = canonical_bytes(doc).decode("ascii")
    require(HOST_PATH_RE.search(serialized) is None, "run record leaks a host path")
    require(".genesis/private/agent-evaluation" not in serialized, "run record leaks held-out custody material")

    benchmark = closed(doc["benchmark"], {"benchmarkId", "caseId", "caseIdentitySha256", "taskClass", "contextTier", "split", "contamination", "heldOutEpochId"}, "benchmark")
    require(benchmark["benchmarkId"] == "GC-AGENT-TASK-BENCHMARK-v0.1", "benchmark id drift")
    require(benchmark["split"] == "public-test" and benchmark["contamination"] == "declared-contaminated" and benchmark["heldOutEpochId"] is None, "public example contamination policy drift")

    authority_rows = doc["authorities"]
    authority_ids = [row.get("id") for row in authority_rows]
    sorted_unique(authority_ids, "authority ids")
    authorities: dict[str, str] = {}
    for row in authority_rows:
        closed(row, {"id", "artifact"}, "authority")
        authorities[row["id"]] = row["artifact"]
    require(authorities == REQUIRED_AUTHORITIES, "run authority closure drift")

    bundle_root = run_path.parent
    records, paths = artifact_map(doc, bundle_root) if check_files else ({}, {})
    if check_files:
        for key in REQUIRED_AUTHORITIES.values():
            require(key in paths and records[key]["role"] in {"authority", "card"}, f"missing authority artifact: {key}")
        suite = load_json(paths[REQUIRED_AUTHORITIES["task-benchmark"]])
        case = next((row for row in suite["cases"] if row["id"] == benchmark["caseId"]), None)
        require(case is not None, "benchmark case is absent")
        require(case["taskClass"] == benchmark["taskClass"] and case["contextTier"] == benchmark["contextTier"], "benchmark case metadata drift")
        require(case_identity(case) == benchmark["caseIdentitySha256"], "benchmark case identity drift")

    model = closed(doc["model"], {"providerKind", "providerId", "modelId", "modelRevision", "weightsArtifact", "tokenizerArtifact", "runtime", "secretPolicy"}, "model")
    require(model["providerKind"] == "local" and model["providerId"] == "genesis.fixture.local", "example must exercise a fully local model")
    require(re.fullmatch(r"sha256:[0-9a-f]{64}", model["modelRevision"]) is not None, "model revision is mutable")
    runtime = closed(model["runtime"], {"runnerId", "version", "executableArtifact", "containerImageDigest", "backend", "quantization", "deterministicRegeneration"}, "model runtime")
    require(runtime["executableArtifact"] == "bundle:tools/local_model_bridge.py" and runtime["deterministicRegeneration"] is True, "local runtime binding drift")
    require(model["secretPolicy"] == {"credentialsRecorded": False, "promptRetention": "complete", "secretsPresent": False}, "secret policy drift")

    invocation = closed(doc["invocation"], {"promptAssembly", "decoding", "retryPolicy", "attempts", "selectedAttempt"}, "invocation")
    prompt = closed(invocation["promptAssembly"], {"algorithm", "messages", "cards", "contextArtifacts", "assemblyOrder", "identitySha256"}, "prompt assembly")
    require(prompt["algorithm"] == "sha256-domain-separated-ordered-artifacts-v0.1", "prompt assembly algorithm drift")
    require(prompt["cards"] == ["repository:docs/spec/GC_AGENT_CORE_CARD_v0.3.json", "repository:docs/spec/GC_AGENT_TASK_CARDS_v0.3.json"], "canonical card bindings drift")
    for row in prompt["messages"]:
        closed(row, {"role", "artifact"}, "prompt message")
    segments = prompt["assemblyOrder"]
    require(isinstance(segments, list) and 4 <= len(segments) <= 608, "invalid prompt assembly order")
    for row in segments:
        closed(row, {"role", "artifact"}, "prompt assembly segment")
        require(row["role"] in {"system-policy", "agent-profile", "task-card", "context-pack", "retrieval-transcript", "task-prompt", "task-input"}, "invalid prompt assembly role")
    ordered_artifacts = [row["artifact"] for row in segments]
    supplied_artifacts = [row["artifact"] for row in prompt["messages"]] + prompt["cards"] + prompt["contextArtifacts"]
    require(len(ordered_artifacts) == len(set(ordered_artifacts)), "prompt assembly repeats an artifact")
    require(sorted(ordered_artifacts) == sorted(supplied_artifacts), "prompt assembly order does not cover supplied artifacts exactly")
    decoding = closed(invocation["decoding"], {"seed", "temperatureMicros", "topPMicros", "topK", "maxOutputTokens", "stop", "responseFormat", "identitySha256"}, "decoding")
    retry = closed(invocation["retryPolicy"], {"maxAttempts", "backoff", "retryableCodes", "identitySha256"}, "retry policy")
    identity_field(decoding, "identitySha256", "decoding")
    identity_field(retry, "identitySha256", "retry")
    attempts = invocation["attempts"]
    require(isinstance(attempts, list) and 1 <= len(attempts) <= retry["maxAttempts"], "attempt count violates retry policy")
    require([row.get("index") for row in attempts] == list(range(len(attempts))), "attempt indices are not contiguous")
    for row in attempts:
        closed(row, {"index", "status", "requestArtifact", "responseArtifact", "outputArtifact", "errorCode", "inputTokens", "outputTokens"}, "attempt")
        if row["status"] == "succeeded":
            require(row["errorCode"] is None and row["outputArtifact"] is not None, "successful attempt is incomplete")
        else:
            require(row["errorCode"] is not None and row["outputArtifact"] is None, "failed attempt is incomplete")
    selected = invocation["selectedAttempt"]
    require(0 <= selected < len(attempts) and attempts[selected]["status"] == "succeeded", "selected attempt did not succeed")

    protocol = closed(doc["toolProtocol"], {"protocolId", "transport", "catalogArtifact", "programArtifact", "policyArtifact", "transcriptArtifact", "effectLogArtifact", "operations", "localModelEffect"}, "tool protocol")
    require(protocol["protocolId"] == "GC-AGENT-TOOL-PROTOCOL-v0.1" and protocol["transport"] == "genesis-effect-log-v3", "tool protocol drift")
    require(protocol["operations"] == ["host/plugin::command"], "tool operation broadening")
    effect = closed(protocol["localModelEffect"], {"operation", "plugin", "command", "bridgeArtifact", "transport", "timeoutMs", "maxBytes", "networkMode", "replayMode"}, "local model effect")
    require(effect == {
        "operation": "host/plugin::command", "plugin": "genesis.agent-model-runner.v0.1",
        "command": "infer", "bridgeArtifact": "bundle:tools/local_model_bridge.py",
        "transport": "spawn-per-op", "timeoutMs": 5000, "maxBytes": 65536,
        "networkMode": "deny", "replayMode": "effect-log-no-reinvoke",
    }, "local model effect policy drift")

    host = closed(doc["host"], {"platformId", "operatingSystem", "cpu", "memoryBytes", "accelerator", "isolation", "environment", "privacy", "identitySha256"}, "host")
    closed(host["operatingSystem"], {"family", "version", "kernelRelease"}, "host operating system")
    closed(host["cpu"], {"architecture", "model", "physicalCores", "logicalCores"}, "host cpu")
    closed(host["accelerator"], {"kind", "model", "memoryBytes", "driverVersion"}, "host accelerator")
    environment = closed(host["environment"], {"locale", "timezone", "networkMode", "declaredVariables"}, "host environment")
    require(environment["networkMode"] == "deny" and environment["declaredVariables"] == sorted(set(environment["declaredVariables"])), "host environment is ambient")
    require(host["privacy"] == {"absolutePathsRecorded": False, "hostnameRecorded": False, "serialNumbersRecorded": False, "userNameRecorded": False}, "host privacy drift")
    identity_field(host, "identitySha256", "host")

    candidate = closed(doc["candidate"], {"root", "completeTree", "selectedAttempt", "artifacts", "treeIdentitySha256"}, "candidate")
    require(candidate["root"] == "candidate" and candidate["completeTree"] is True and candidate["selectedAttempt"] == selected, "candidate provenance drift")
    sorted_unique(candidate["artifacts"], "candidate artifacts")
    score = closed(doc["score"], {"artifact", "scoreIdentitySha256", "qualityScoreBasisPoints", "includedModelMetrics"}, "score binding")
    require(score["includedModelMetrics"] is False, "model metrics entered score binding")
    metrics = closed(doc["modelSpecificMetrics"], {"includedInQualityScore", "modelLatencyNs", "providerQueueNs", "apiCostMicrounits", "currency", "energyMicrojoules", "measurementCompleteness"}, "model metrics")
    require(metrics["includedInQualityScore"] is False, "model metrics entered quality score")

    if check_files:
        referenced = set(REQUIRED_AUTHORITIES.values()) | {
            model["weightsArtifact"], model["tokenizerArtifact"], runtime["executableArtifact"],
            protocol["catalogArtifact"], protocol["programArtifact"], protocol["policyArtifact"], protocol["transcriptArtifact"],
            protocol["effectLogArtifact"], effect["bridgeArtifact"], score["artifact"],
        }
        referenced.update(row["artifact"] for row in prompt["messages"])
        referenced.update(prompt["cards"])
        referenced.update(prompt["contextArtifacts"])
        for row in attempts:
            referenced.update({row["requestArtifact"], row["responseArtifact"]})
            if row["outputArtifact"] is not None:
                referenced.add(row["outputArtifact"])
        referenced.update(candidate["artifacts"])
        require(
            referenced == set(paths),
            "artifact inventory contains unreferenced or missing material "
            f"(missing={sorted(referenced - set(paths))}, unreferenced={sorted(set(paths) - referenced)})",
        )
        require(prompt["identitySha256"] == prompt_identity(doc, paths), "prompt assembly identity drift")
        require(file_sha256(paths[model["weightsArtifact"]]) == model["modelRevision"].removeprefix("sha256:"), "model weights revision drift")

        candidate_root = bundle_root / candidate["root"]
        candidate_files: dict[str, bytes] = {}
        expected_candidate_keys: list[str] = []
        for directory, dirnames, filenames in os.walk(candidate_root, followlinks=False):
            current = Path(directory)
            for name in dirnames:
                require(not (current / name).is_symlink(), "candidate contains a symlink directory")
            for name in filenames:
                path = current / name
                require(not path.is_symlink() and path.is_file(), "candidate contains a non-regular file")
                relative = path.relative_to(candidate_root).as_posix()
                candidate_files[relative] = path.read_bytes()
                expected_candidate_keys.append(f"bundle:{candidate['root']}/{relative}")
        require(candidate["artifacts"] == sorted(expected_candidate_keys), "candidate artifact list is not a complete tree")
        require(candidate["treeIdentitySha256"] == inventory_identity(candidate_files), "candidate tree identity drift")

        score_doc = validate_score(load_json(paths[score["artifact"]]))
        require(score_doc["scoreIdentitySha256"] == score["scoreIdentitySha256"] and score_doc["qualityScoreBasisPoints"] == score["qualityScoreBasisPoints"], "score binding drift")
        require(score_doc["candidate"]["identitySha256"] == candidate["treeIdentitySha256"], "score is bound to another candidate")
        require(score_doc["modelSpecificMetrics"]["present"] is False, "quality score contains model metrics")

        policy = parse_toml_subset(paths[protocol["policyArtifact"]].read_bytes())
        require(policy.get("allow") == ["host/plugin::command"], "local model policy allowlist drift")
        op_policy = policy.get("op", {}).get("host/plugin::command", {})
        require(op_policy.get("allow_plugins") == [effect["plugin"]] and op_policy.get("allow_commands") == [effect["command"]], "local model plugin policy broadening")
        require(op_policy.get("bridge_cmd") == "tools/local_model_bridge.py" and op_policy.get("base_dir") == ".", "local model bridge path drift")
        require(op_policy.get("bridge_cmd_sha256") == f"sha256:{file_sha256(paths[effect['bridgeArtifact']])}", "local model bridge is not digest pinned")
        require(op_policy.get("timeout_ms") == effect["timeoutMs"] and op_policy.get("max_bytes") == effect["maxBytes"], "local model resource policy drift")

        request = load_json(paths[attempts[selected]["requestArtifact"]])
        response = load_json(paths[attempts[selected]["responseArtifact"]])
        require(request == {
            "cardsSha256": sha256_bytes(canonical_bytes(prompt["cards"])),
            "decodingSha256": decoding["identitySha256"],
            "modelId": model["modelId"], "modelRevision": model["modelRevision"],
            "promptAssemblySha256": prompt["identitySha256"],
            "tokenizerSha256": file_sha256(paths[model["tokenizerArtifact"]]),
            "weightsSha256": file_sha256(paths[model["weightsArtifact"]]),
        }, "model request artifact drift")
        require(response == {
            "finishReason": "stop", "modelId": model["modelId"], "modelRevision": model["modelRevision"],
            "ok": True, "outputSha256": file_sha256(paths[attempts[selected]["outputArtifact"]]),
            "usage": {"inputTokens": attempts[selected]["inputTokens"], "outputTokens": attempts[selected]["outputTokens"]},
        }, "model response artifact drift")
        transcript_lines = paths[protocol["transcriptArtifact"]].read_text(encoding="utf-8").splitlines()
        require(len(transcript_lines) == 1, "tool transcript must record exactly one local inference")
        transcript = json.loads(transcript_lines[0], object_pairs_hook=reject_pairs)
        require(transcript == {
            "attempt": selected, "command": effect["command"], "decision": "allow",
            "operation": effect["operation"], "plugin": effect["plugin"],
            "requestSha256": file_sha256(paths[attempts[selected]["requestArtifact"]]),
            "responseSha256": file_sha256(paths[attempts[selected]["responseArtifact"]]),
        }, "tool transcript drift")
        log_text = paths[protocol["effectLogArtifact"]].read_text(encoding="utf-8")
        require(":version 3" in log_text and "host/plugin::command" in log_text and ":decision :allow" in log_text, "effect log does not prove allowed local inference")
        require(HOST_PATH_RE.search(log_text) is None, "effect log leaks a host path")

    require(SHA_RE.fullmatch(doc["contentIdentitySha256"]) is not None and doc["contentIdentitySha256"] == object_identity(doc, "contentIdentitySha256"), "run content identity drift")
    return doc


def add_artifact(rows: list[dict[str, Any]], scope: str, path: str, role: str, mode: str = "0644") -> None:
    root = ROOT if scope == "repository" else EXAMPLE.parent
    artifact = resolve_regular(root, path, "refresh artifact")
    payload = artifact.read_bytes()
    rows.append({
        "key": f"{scope}:{path}", "scope": scope, "path": path,
        "sha256": sha256_bytes(payload), "bytes": len(payload),
        "mediaType": MEDIA_TYPES.get(artifact.suffix, "application/octet-stream"),
        "role": role, "mode": mode,
    })


def rewrite_single(source: str, pattern: str, replacement: str, label: str) -> str:
    rewritten, count = re.subn(pattern, replacement, source, count=2)
    require(count == 1, f"refresh binding count drift for {label}: {count}")
    return rewritten


def rewrite_file(path: Path, bindings: list[tuple[str, str, str]]) -> None:
    source = path.read_text(encoding="utf-8")
    for pattern, replacement, label in bindings:
        source = rewrite_single(source, pattern, replacement, f"{path.name}:{label}")
    path.write_text(source, encoding="utf-8")


def refresh_example(genesis_bin: Path, selfhost_artifact: Path) -> None:
    require(genesis_bin.is_file() and selfhost_artifact.is_file(), "refresh requires built Genesis and selfhost artifact")
    os.chmod(EXAMPLE.parent / "tools/local_model_bridge.py", 0o755)
    doc = load_json(EXAMPLE)
    weights = EXAMPLE.parent / "models/weights.fixture"
    tokenizer = EXAMPLE.parent / "models/tokenizer.json"
    model_revision = file_sha256(weights)
    doc["model"]["modelRevision"] = f"sha256:{model_revision}"
    decoding = doc["invocation"]["decoding"]
    decoding["identitySha256"] = object_identity(decoding, "identitySha256")
    retry = doc["invocation"]["retryPolicy"]
    retry["identitySha256"] = object_identity(retry, "identitySha256")

    provisional_paths = {
        "bundle:prompts/system.md": EXAMPLE.parent / "prompts/system.md",
        "bundle:prompts/user.md": EXAMPLE.parent / "prompts/user.md",
        "bundle:candidate/requirements.md": EXAMPLE.parent / "candidate/requirements.md",
        "repository:docs/spec/GC_AGENT_CORE_CARD_v0.3.json": ROOT / "docs/spec/GC_AGENT_CORE_CARD_v0.3.json",
        "repository:docs/spec/GC_AGENT_CORE_CARD_v0.3.md": ROOT / "docs/spec/GC_AGENT_CORE_CARD_v0.3.md",
        "repository:docs/spec/GC_AGENT_TASK_CARDS_v0.3.json": ROOT / "docs/spec/GC_AGENT_TASK_CARDS_v0.3.json",
    }
    doc["invocation"]["promptAssembly"]["identitySha256"] = prompt_identity(doc, provisional_paths)
    host = doc["host"]
    host["identitySha256"] = object_identity(host, "identitySha256")

    suite = load_json(BENCHMARK)
    case = next(row for row in suite["cases"] if row["id"] == doc["benchmark"]["caseId"])
    doc["benchmark"]["caseIdentitySha256"] = case_identity(case)
    request = {
        "cardsSha256": sha256_bytes(canonical_bytes(doc["invocation"]["promptAssembly"]["cards"])),
        "decodingSha256": decoding["identitySha256"],
        "modelId": doc["model"]["modelId"], "modelRevision": doc["model"]["modelRevision"],
        "promptAssemblySha256": doc["invocation"]["promptAssembly"]["identitySha256"],
        "tokenizerSha256": file_sha256(tokenizer), "weightsSha256": model_revision,
    }
    request_path = EXAMPLE.parent / "invocation/request.json"
    request_path.write_bytes(canonical_bytes(request))
    response = {
        "finishReason": "stop", "modelId": doc["model"]["modelId"],
        "modelRevision": doc["model"]["modelRevision"], "ok": True,
        "outputSha256": file_sha256(EXAMPLE.parent / "invocation/model-output.txt"),
        "usage": {"inputTokens": 96, "outputTokens": 7},
    }
    response_path = EXAMPLE.parent / "invocation/response.json"
    response_path.write_bytes(canonical_bytes(response))

    bridge = EXAMPLE.parent / "tools/local_model_bridge.py"
    rewrite_file(bridge, [
        (
            r'(?<=:model-revision "sha256:)(?:[0-9a-f]{64}|REPLACE_MODEL_REVISION)(?=")',
            model_revision,
            "model-revision",
        ),
    ])
    bridge_sha = file_sha256(bridge)
    caps = EXAMPLE.parent / "caps.toml"
    rewrite_file(caps, [
        (
            r'(?<=bridge_cmd_sha256 = "sha256:)(?:[0-9a-f]{64}|REPLACE_BRIDGE_SHA256)(?=")',
            bridge_sha,
            "bridge-sha256",
        ),
    ])
    program = EXAMPLE.parent / "model_effect.gc"
    program_values = {
        "decoding-sha256": decoding["identitySha256"],
        "model-revision": model_revision,
        "prompt-assembly-sha256": doc["invocation"]["promptAssembly"]["identitySha256"],
        "request-sha256": file_sha256(request_path),
        "tokenizer-sha256": file_sha256(tokenizer),
        "weights-sha256": model_revision,
    }
    rewrite_file(program, [
        (
            rf'(?<=:{re.escape(key)} ")(?:sha256:)?(?:[0-9a-f]{{64}}|REPLACE_[A-Z0-9_]+)(?=")',
            f"sha256:{value}" if key == "model-revision" else value,
            key,
        )
        for key, value in program_values.items()
    ])

    transcript = {
        "attempt": 0, "command": "infer", "decision": "allow", "operation": "host/plugin::command",
        "plugin": "genesis.agent-model-runner.v0.1", "requestSha256": file_sha256(request_path),
        "responseSha256": file_sha256(response_path),
    }
    (EXAMPLE.parent / "tool-transcript.jsonl").write_bytes(canonical_bytes(transcript))

    log_path = EXAMPLE.parent / "model-effect.gclog"
    run_env = dict(os.environ)
    run_env["GENESIS_SELFHOST_COMPILED_CACHE_DISABLE"] = "1"
    run = subprocess.run(
        [str(genesis_bin), "--no-step-limit", "--selfhost-artifact", str(selfhost_artifact),
         "run", "model_effect.gc", "--engine", "selfhost", "--caps", "caps.toml", "--log", "model-effect.gclog"],
        cwd=EXAMPLE.parent, env=run_env, stdout=subprocess.PIPE, stderr=subprocess.PIPE, check=False,
    )
    require(run.returncode == 0, f"local model effect failed during refresh: {run.stderr.decode('utf-8', 'replace')}")
    require(log_path.is_file(), "local model effect did not produce a log")

    score_path = EXAMPLE.parent / "score.json"
    scored = subprocess.run(
        [sys.executable, str(ROOT / "scripts/lib/gc_agent_scoring.py"), "--score", "--case", case["id"],
         "--candidate", str(EXAMPLE.parent / "candidate"), "--genesis-bin", str(genesis_bin),
         "--selfhost-artifact", str(selfhost_artifact)],
        cwd=ROOT, stdout=subprocess.PIPE, stderr=subprocess.PIPE, check=False,
    )
    require(scored.returncode == 0, f"scorer failed during refresh: {scored.stderr.decode('utf-8', 'replace')}")
    score_path.write_bytes(scored.stdout)
    score_doc = validate_score(load_json(score_path))
    doc["score"].update({
        "scoreIdentitySha256": score_doc["scoreIdentitySha256"],
        "qualityScoreBasisPoints": score_doc["qualityScoreBasisPoints"],
    })
    candidate_files = {
        path.relative_to(EXAMPLE.parent / "candidate").as_posix(): path.read_bytes()
        for path in sorted((EXAMPLE.parent / "candidate").rglob("*")) if path.is_file()
    }
    doc["candidate"]["treeIdentitySha256"] = inventory_identity(candidate_files)

    doc["authorities"] = [
        {"id": authority_id, "artifact": artifact}
        for authority_id, artifact in sorted(REQUIRED_AUTHORITIES.items())
    ]
    rows: list[dict[str, Any]] = []
    authority_roles = {
        "docs/spec/GC_AGENT_CORE_CARD_v0.3.json": "card",
        "docs/spec/GC_AGENT_TASK_CARDS_v0.3.json": "card",
    }
    for artifact in sorted(set(REQUIRED_AUTHORITIES.values())):
        path = artifact.removeprefix("repository:")
        mode = "0755" if path == "scripts/lib/gc_agent_benchmark_run.py" else "0644"
        add_artifact(rows, "repository", path, authority_roles.get(path, "authority"), mode)
    add_artifact(rows, "repository", "docs/spec/GC_AGENT_CORE_CARD_v0.3.md", "context")
    bundle_roles = {
        "candidate/main.gc": "candidate", "candidate/requirements.md": "candidate",
        "prompts/system.md": "prompt", "prompts/user.md": "prompt",
        "models/weights.fixture": "model", "models/tokenizer.json": "model",
        "tools/local_model_bridge.py": "runtime", "tools/catalog.json": "tool-catalog",
        "caps.toml": "tool-policy", "model_effect.gc": "context",
        "invocation/request.json": "request", "invocation/response.json": "response",
        "invocation/model-output.txt": "model-output", "tool-transcript.jsonl": "tool-transcript",
        "model-effect.gclog": "effect-log", "score.json": "score",
    }
    for path, role in sorted(bundle_roles.items()):
        add_artifact(rows, "bundle", path, role, "0755" if path == "tools/local_model_bridge.py" else "0644")
    doc["artifactInventory"] = sorted(rows, key=lambda row: row["key"])
    doc["contentIdentitySha256"] = object_identity(doc, "contentIdentitySha256")
    EXAMPLE.write_text(json.dumps(doc, indent=2, sort_keys=True) + "\n", encoding="utf-8")
    validate_document(load_json(EXAMPLE))


def self_test(document: dict[str, Any]) -> int:
    mutations: list[tuple[str, Any]] = []

    def add(name: str, mutate: Any) -> None:
        candidate = copy.deepcopy(document)
        mutate(candidate)
        mutations.append((name, candidate))

    add("unknown-field", lambda d: d.__setitem__("authority", "prompt"))
    add("host-path", lambda d: d["artifactInventory"][0].__setitem__("path", "/Users/private/model"))
    add("parent-path", lambda d: d["artifactInventory"][0].__setitem__("path", "../model"))
    add("stale-artifact", lambda d: d["artifactInventory"][0].__setitem__("sha256", "0" * 64))
    add("missing-authority", lambda d: d["authorities"].pop())
    add("prompt-selected-authority", lambda d: d["authorities"][0].__setitem__("artifact", "bundle:invocation/model-output.txt"))
    add("stale-card", lambda d: d["invocation"]["promptAssembly"]["cards"].pop())
    add("mutable-model", lambda d: d["model"].__setitem__("modelRevision", "latest"))
    add("remote-rebinding", lambda d: d["model"].__setitem__("providerKind", "remote"))
    add("secret-recording", lambda d: d["model"]["secretPolicy"].__setitem__("credentialsRecorded", True))
    add("decoding-drift", lambda d: d["invocation"]["decoding"].__setitem__("seed", 7))
    add("retry-broadening", lambda d: d["invocation"]["retryPolicy"].__setitem__("maxAttempts", 16))
    add("attempt-gap", lambda d: d["invocation"]["attempts"][0].__setitem__("index", 1))
    add("selected-failure", lambda d: d["invocation"]["attempts"][0].__setitem__("status", "failed"))
    add("operation-broadening", lambda d: d["toolProtocol"]["operations"].append("io/net::http-request"))
    add("effect-rebinding", lambda d: d["toolProtocol"]["localModelEffect"].__setitem__("operation", "sys/process::exec"))
    add("plugin-wildcard", lambda d: d["toolProtocol"]["localModelEffect"].__setitem__("plugin", "*"))
    add("network-broadening", lambda d: d["toolProtocol"]["localModelEffect"].__setitem__("networkMode", "allowlisted"))
    add("reinvoke-replay", lambda d: d["toolProtocol"]["localModelEffect"].__setitem__("replayMode", "reinvoke"))
    add("host-privacy", lambda d: d["host"]["privacy"].__setitem__("hostnameRecorded", True))
    add("ambient-env", lambda d: d["host"]["environment"]["declaredVariables"].append("HOME"))
    add("candidate-incomplete", lambda d: d["candidate"]["artifacts"].pop())
    add("candidate-rebinding", lambda d: d["candidate"].__setitem__("treeIdentitySha256", "0" * 64))
    add("score-model-metrics", lambda d: d["score"].__setitem__("includedModelMetrics", True))
    add("quality-model-metrics", lambda d: d["modelSpecificMetrics"].__setitem__("includedInQualityScore", True))
    add("heldout-leak", lambda d: d["benchmark"].__setitem__("split", "held-out"))
    add("contamination-erasure", lambda d: d["benchmark"].__setitem__("contamination", "temporal-clean"))
    add("identity-drift", lambda d: d.__setitem__("contentIdentitySha256", "0" * 64))
    rejected = 0
    for name, candidate in mutations:
        try:
            validate_document(candidate)
        except (RunError, ValueError):
            rejected += 1
        else:
            raise RunError(f"negative control accepted: {name}")
    require(
        rewrite_single(
            'bridge_cmd_sha256 = "sha256:' + "0" * 64 + '"',
            r'(?<=sha256:)[0-9a-f]{64}(?=")',
            "1" * 64,
            "self-test",
        )
        == 'bridge_cmd_sha256 = "sha256:' + "1" * 64 + '"',
        "refresh rebinding control drift",
    )
    profile = validate_model_effect_profile()
    profile["capabilityPolicy"]["wildcardsAllowed"] = True
    try:
        validate_model_effect_profile(profile)
    except RunError:
        pass
    else:
        raise RunError("negative control accepted: model-effect-policy-broadening")
    return rejected + 2


def main() -> int:
    parser = argparse.ArgumentParser()
    mode = parser.add_mutually_exclusive_group(required=True)
    mode.add_argument("--check", action="store_true")
    mode.add_argument("--refresh-example", action="store_true")
    parser.add_argument("--run", type=Path, default=EXAMPLE)
    parser.add_argument("--self-test", action="store_true")
    parser.add_argument("--genesis-bin", type=Path)
    parser.add_argument("--selfhost-artifact", type=Path)
    args = parser.parse_args()
    validate_schema()
    validate_model_effect_profile()
    if args.refresh_example:
        require(args.genesis_bin is not None and args.selfhost_artifact is not None, "refresh requires --genesis-bin and --selfhost-artifact")
        require(args.run == EXAMPLE and not args.self_test, "refresh is restricted to the canonical example")
        refresh_example(args.genesis_bin.resolve(), args.selfhost_artifact.resolve())
        print(f"gc-agent-benchmark-run: refreshed {EXAMPLE.relative_to(ROOT)}")
        return 0
    require(args.genesis_bin is None and args.selfhost_artifact is None, "check mode is read-only and accepts no execution inputs")
    document = validate_document(load_json(args.run), args.run.resolve())
    controls = self_test(document) if args.self_test else 0
    print(
        "gc-agent-benchmark-run: ok "
        f"(artifacts={len(document['artifactInventory'])} attempts={len(document['invocation']['attempts'])} "
        f"controls={controls} identity={document['contentIdentitySha256']})"
    )
    return 0


if __name__ == "__main__":
    try:
        raise SystemExit(main())
    except (RunError, ValueError, OSError) as exc:
        print(f"gc-agent-benchmark-run: {exc}", file=sys.stderr)
        raise SystemExit(1)
