#!/usr/bin/env python3
"""Capability-minimal, replayable GenesisBench Open Agent harness."""

from __future__ import annotations

import argparse
import ast
import copy
import gzip
import hashlib
import json
import os
import platform
import re
import shutil
import signal
import socket
import stat
import subprocess
import sys
import tarfile
import tempfile
import time
import urllib.error
import urllib.request
from pathlib import Path, PurePosixPath
from typing import Any

import gc_agent_scoring
import gc_task_benchmarks
import genesisbench_mlx_custody
import genesisbench_mlx_responses
import genesisbench_protocol


ROOT = Path(__file__).resolve().parents[2]
AUTHORITY_PATH = ROOT / "docs/spec/GENESISBENCH_OPEN_AGENT_v0.5.json"
LEGACY_AUTHORITY_PATHS = (
    ROOT / "docs/spec/GENESISBENCH_OPEN_AGENT_v0.4.json",
    ROOT / "docs/spec/GENESISBENCH_OPEN_AGENT_v0.3.json",
    ROOT / "docs/spec/GENESISBENCH_OPEN_AGENT_v0.2.json",
    ROOT / "docs/spec/GENESISBENCH_OPEN_AGENT_v0.1.json",
)
PREDECLARATION_SCHEMA_PATH = ROOT / "docs/spec/GENESISBENCH_OPEN_AGENT_PREDECLARATION_v0.1.schema.json"
CAMPAIGN_SCHEMA_PATH = ROOT / "docs/spec/GENESISBENCH_OPEN_AGENT_CAMPAIGN_v0.1.schema.json"
RUN_SCHEMA_PATH = ROOT / "docs/spec/GENESISBENCH_OPEN_AGENT_RUN_v0.1.schema.json"
TOOL_ARCHIVE_SCHEMA_PATH = ROOT / "docs/spec/GENESISBENCH_OPEN_AGENT_TOOL_ARCHIVE_v0.1.schema.json"
SUITE_PATH = ROOT / "benchmarks/agent_tasks/v0.1/suite.json"
PROTOCOL_PATH = ROOT / "docs/spec/GENESISBENCH_PROTOCOL_v0.1.json"
SCORING_PATH = ROOT / "docs/spec/GC_AGENT_BENCHMARK_SCORING_v0.1.json"
AUTHORITY_ARCHIVE_ROOT = ROOT / "benchmarks/genesisbench/v0.1/authority-archive"

KIND_PREDECLARATION = "genesis/genesisbench-open-agent-predeclaration-v0.1"
KIND_CAMPAIGN = "genesis/genesisbench-open-agent-campaign-v0.1"
KIND_RUN = "genesis/genesisbench-open-agent-run-v0.1"
KIND_AUTHORITY = "genesis/genesisbench-open-agent-harness-v0.1"
KIND_AUTHORITY_V2 = "genesis/genesisbench-open-agent-harness-v0.2"
KIND_AUTHORITY_V3 = "genesis/genesisbench-open-agent-harness-v0.3"
KIND_AUTHORITY_V4 = "genesis/genesisbench-open-agent-harness-v0.4"
KIND_AUTHORITY_V5 = "genesis/genesisbench-open-agent-harness-v0.5"
SHA_RE = re.compile(r"^[0-9a-f]{64}$")
ID_RE = re.compile(r"^[A-Za-z0-9][A-Za-z0-9._:@+-]{0,191}$")
REL_RE = re.compile(r"^(?!/)(?!.*(?:^|/)\.\.(?:/|$))[A-Za-z0-9._/-]{1,512}$")
MAX_CAPTURE_BYTES = 16 * 1024 * 1024
ALLOWED_ENVIRONMENT = tuple(sorted((
    "CODEX_HOME",
    "HOME",
    "HTTPS_PROXY",
    "HTTP_PROXY",
    "NO_PROXY",
    "PATH",
    "SSL_CERT_FILE",
    "SSL_CERT_DIR",
)))
V3_FIXED_ENVIRONMENT = {
    "GENESIS_SELFHOST_COMPILED_CACHE_DISABLE": "1",
    "NO_COLOR": "1",
}
IMPLEMENTATION_ENTRYPOINTS = (
    "scripts/lib/genesisbench_open_agent.py",
    "scripts/lib/genesisbench_open_agent_report.py",
)
IMPLEMENTATION_PATHS = (
    "scripts/lib/gc_agent_benchmark_run.py",
    "scripts/lib/gc_agent_scoring.py",
    "scripts/lib/gc_agent_scoring_contract.py",
    "scripts/lib/gc_task_benchmarks.py",
    "scripts/lib/genesisbench_contamination.py",
    "scripts/lib/genesisbench_eligibility.py",
    "scripts/lib/genesisbench_local_models.py",
    "scripts/lib/genesisbench_mlx_custody.py",
    "scripts/lib/genesisbench_mlx_responses.py",
    "scripts/lib/genesisbench_open_agent.py",
    "scripts/lib/genesisbench_open_agent_report.py",
    "scripts/lib/genesisbench_protocol.py",
    "scripts/lib/genesisbench_protocol_contract.py",
    "scripts/lib/genesisbench_protocol_run.py",
    "scripts/lib/genesisbench_reference_agent.py",
    "scripts/lib/genesisbench_tracks.py",
)
V4_PURPOSE = (
    "Execute atomically predeclared repository-editing campaigns with a transitively "
    "content-bound harness and byte-complete evidence for every safely inventoried "
    "invalid workspace, without broadening Cold Acquisition."
)
V4_INVOCATION_PROFILES = {
    "codex-cli-hosted": {
        "argvPolicy": "fixed-codex-exec-ephemeral-json-workspace-write-one-prompt-ancestry-isolated-v0.4",
        "id": "codex-cli-hosted-v0.4",
        "network": "provider-only",
        "provider": None,
    },
    "codex-cli-local": {
        "argvPolicy": "fixed-codex-exec-oss-ephemeral-json-workspace-write-one-prompt-ancestry-isolated-v0.4",
        "id": "codex-cli-local-v0.4",
        "network": "loopback-provider-only",
        "provider": "predeclared-lmstudio-or-ollama",
    },
}
V4_SECURITY_CONTROLS = sorted((
    "agent-and-model-never-accessed-during-replay",
    "all-environment-values-redacted",
    "ambient-project-instruction-and-skill-discovery-blocked",
    "ancestry-isolated-process-workspace",
    "atomic-complete-campaign-precommitment",
    "attempt-membership-and-common-field-binding",
    "bounded-large-event-line-validation",
    "candidate-path-closure",
    "complete-transitive-local-implementation-binding",
    "declared-editable-output-creation-and-retention",
    "ephemeral-stage-path-normalization",
    "exact-executable-digest-and-version",
    "finite-capture-and-wall-time-budgets",
    "frozen-repository-post-run-rehash",
    "immutable-predeclaration-before-execution",
    "inventory-valid-invalid-workspace-payload-retention",
    "non-selective-campaign-stop-policy",
    "observed-workspace-byte-parity-validation",
    "one-attempt-no-resume-no-hidden-retry",
    "permission-aware-temporary-snapshot-teardown",
    "predeclared-supplied-tool-digests",
    "process-group-hard-kill-and-reap",
    "provider-network-only-or-loopback-only",
    "read-only-frozen-source-outside-writable-workspace",
    "strict-jsonl-transcript-validation",
    "supplied-genesis-tool-cache-disabled",
    "symlink-and-non-regular-file-rejection",
))
V5_PURPOSE = (
    "Execute transitively bound hosted campaigns and credential-free local MLX campaigns "
    "under an independently tested outer read/write/network sandbox with exact custody, "
    "single-request-per-turn transport, retained wire evidence, and hard server teardown."
)
V5_INVOCATION_PROFILES = {
    "codex-cli-hosted": {
        "argvPolicy": "fixed-codex-exec-ephemeral-json-workspace-write-one-prompt-ancestry-isolated-v0.5",
        "id": "codex-cli-hosted-v0.5",
        "network": "provider-only",
        "provider": None,
    },
    "codex-cli-local": {
        "argvPolicy": "fixed-codex-exec-custom-responses-auth-free-outer-sandbox-v0.5",
        "id": "codex-cli-local-v0.5",
        "network": "one-loopback-responses-endpoint-only",
        "provider": "predeclared-mlx-responses",
    },
}
V5_DISABLED_FEATURES = (
    "apps", "browser_use", "computer_use", "enable_mcp_apps", "hooks", "image_generation",
    "in_app_browser", "memories", "multi_agent", "plugins", "skill_mcp_dependency_install",
    "skill_search", "workspace_dependencies",
)
V5_DISABLED_SYSTEM_SKILLS = (
    "imagegen", "openai-docs", "plugin-creator", "skill-creator", "skill-installer",
)
V5_SECURITY_CONTROLS = sorted(set(V4_SECURITY_CONTROLS).union({
    "auth-free-home-and-codex-home",
    "authorization-cookie-and-api-key-header-rejection",
    "canonical-model-runtime-license-custody-before-load",
    "codex-request-and-mlx-response-byte-retention",
    "declared-tool-scaffold-only",
    "fresh-home-system-skill-disablement",
    "local-server-no-fallback",
    "outer-darwin-read-write-network-allowlist",
    "single-backend-request-per-agent-turn-zero-transport-retries",
    "supervised-adapter-and-model-server-hard-teardown",
    "web-multi-agent-plugin-app-hook-memory-and-mcp-disablement",
}))


class OpenAgentError(RuntimeError):
    pass


def require(condition: bool, message: str) -> None:
    if not condition:
        raise OpenAgentError(message)


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


def identified(value: dict[str, Any]) -> dict[str, Any]:
    out = copy.deepcopy(value)
    out["contentIdentitySha256"] = ""
    out["contentIdentitySha256"] = sha256_bytes(canonical_bytes(out))
    return out


def validate_identity(value: dict[str, Any]) -> None:
    digest = value.get("contentIdentitySha256")
    require(isinstance(digest, str) and SHA_RE.fullmatch(digest) is not None, "invalid content identity")
    require(identified(value) == value, "content identity mismatch")


def load_json(path: Path) -> Any:
    require(path.is_file() and not path.is_symlink(), f"JSON authority must be a regular file: {path.name}")
    with path.open("r", encoding="ascii") as stream:
        return json.load(stream)


def write_json(path: Path, value: Any) -> None:
    path.parent.mkdir(parents=True, exist_ok=True)
    path.write_bytes(pretty_bytes(value))


def closed(value: Any, fields: set[str], label: str) -> dict[str, Any]:
    require(isinstance(value, dict), f"{label} must be an object")
    require(set(value) == fields, f"{label} fields are not closed")
    return value


def safe_id(value: Any, label: str) -> str:
    require(isinstance(value, str) and ID_RE.fullmatch(value) is not None, f"invalid {label}")
    return value


def safe_relative(value: Any, label: str) -> PurePosixPath:
    require(isinstance(value, str) and REL_RE.fullmatch(value) is not None, f"invalid {label}")
    require("//" not in value and not value.endswith("/"), f"non-canonical {label}")
    path = PurePosixPath(value)
    require(all(part not in {"", ".", ".."} for part in path.parts), f"unsafe {label}")
    return path


def regular_file(path: Path, label: str) -> Path:
    require(path.is_file() and not path.is_symlink(), f"{label} must be a regular non-symlink file")
    return path.resolve(strict=True)


def identified_authority(current: Path, archive: Path, identity: str) -> dict[str, Any]:
    value = load_json(current)
    if value.get("contentIdentitySha256") != identity:
        value = load_json(archive / f"{identity}.json")
    require(value.get("contentIdentitySha256") == identity, "authority archive identity drift")
    return value


def suite_authority(identity: str) -> dict[str, Any]:
    value = identified_authority(SUITE_PATH, AUTHORITY_ARCHIVE_ROOT / "suites", identity)
    require(gc_task_benchmarks.identity(value) == identity, "suite authority content identity drift")
    return value


def protocol_authority(identity: str) -> dict[str, Any]:
    value = identified_authority(PROTOCOL_PATH, AUTHORITY_ARCHIVE_ROOT / "protocols", identity)
    require(genesisbench_protocol.content_identity(value) == identity, "protocol authority content identity drift")
    return value


def protocol_bound_json(protocol: dict[str, Any], relative_path: str) -> dict[str, Any]:
    row = next(
        (item for item in protocol["authorities"] if item["path"] == relative_path),
        None,
    )
    require(row is not None, f"protocol does not bind authority: {relative_path}")
    current = ROOT / relative_path
    payload = current.read_bytes() if current.is_file() and sha256_file(current) == row["sha256"] else None
    archived = AUTHORITY_ARCHIVE_ROOT / "documents" / f"{row['sha256']}.json"
    if payload is None and archived.is_file():
        payload = archived.read_bytes()
    if payload is None:
        commit = protocol["sourceSnapshot"]["commitSha1"]
        result = subprocess.run(
            ["git", "show", f"{commit}:{relative_path}"], cwd=ROOT,
            stdout=subprocess.PIPE, stderr=subprocess.PIPE, check=False,
        )
        require(result.returncode == 0, f"frozen authority is unavailable: {relative_path}")
        payload = result.stdout
    require(len(payload) == row["bytes"], f"frozen authority byte count drift: {relative_path}")
    require(sha256_bytes(payload) == row["sha256"], f"frozen authority digest drift: {relative_path}")
    value = json.loads(payload)
    require(isinstance(value, dict), f"frozen authority must be an object: {relative_path}")
    return value


def suite_and_case(case_id: str, suite: dict[str, Any] | None = None) -> tuple[dict[str, Any], dict[str, Any]]:
    suite = suite or load_json(SUITE_PATH)
    case = next((row for row in suite["cases"] if row["id"] == case_id), None)
    require(case is not None, f"unknown benchmark case: {case_id}")
    return suite, case


def authority(identity: str | None = None) -> dict[str, Any]:
    documents = [load_json(path) for path in (AUTHORITY_PATH, *LEGACY_AUTHORITY_PATHS)]
    if identity is None:
        return documents[0]
    match = next((document for document in documents if document["contentIdentitySha256"] == identity), None)
    require(match is not None, "unknown Open Agent harness authority")
    return match


def harness_minor_version(harness: dict[str, Any]) -> int:
    match = re.fullmatch(r"0\.([0-9]+)\.0", harness["version"])
    require(match is not None, "invalid Open Agent harness version")
    return int(match.group(1))


def is_v3_or_later_harness(harness: dict[str, Any]) -> bool:
    return harness_minor_version(harness) >= 3


def is_v4_harness(harness: dict[str, Any]) -> bool:
    return harness["version"] == "0.4.0"


def is_v4_or_later_harness(harness: dict[str, Any]) -> bool:
    return harness_minor_version(harness) >= 4


def is_v5_harness(harness: dict[str, Any]) -> bool:
    return harness["version"] == "0.5.0"


def implementation_rows() -> list[dict[str, Any]]:
    rows = []
    for relative in IMPLEMENTATION_PATHS:
        path = regular_file(ROOT / relative, "Open Agent implementation file")
        rows.append({"path": relative, "bytes": path.stat().st_size, "sha256": sha256_file(path)})
    return rows


def discovered_implementation_paths() -> tuple[str, ...]:
    scripts_root = ROOT / "scripts/lib"
    pending = [Path(path).name for path in IMPLEMENTATION_ENTRYPOINTS]
    seen: set[str] = set()
    while pending:
        name = pending.pop()
        if name in seen:
            continue
        source = regular_file(scripts_root / name, "Open Agent implementation closure file")
        seen.add(name)
        tree = ast.parse(source.read_text(encoding="ascii"), filename=name)
        for node in ast.walk(tree):
            modules: list[str] = []
            if isinstance(node, ast.Import):
                modules = [alias.name.split(".")[0] for alias in node.names]
            elif isinstance(node, ast.ImportFrom) and node.module:
                modules = [node.module.split(".")[0]]
            for module in modules:
                candidate = f"{module}.py"
                if (scripts_root / candidate).is_file() and candidate not in seen:
                    pending.append(candidate)
    return tuple(sorted(f"scripts/lib/{name}" for name in seen))


def validate_implementation_binding(document: Any, *, check_files: bool) -> dict[str, Any]:
    binding = closed(
        document,
        {"entrypoints", "files", "identitySha256"},
        "Open Agent implementation binding",
    )
    require(binding["entrypoints"] == list(IMPLEMENTATION_ENTRYPOINTS), "implementation entrypoint drift")
    require(isinstance(binding["files"], list), "implementation files must be an array")
    rows = []
    for index, raw in enumerate(binding["files"]):
        row = closed(raw, {"path", "bytes", "sha256"}, f"implementation file[{index}]")
        safe_relative(row["path"], "implementation path")
        require(isinstance(row["bytes"], int) and row["bytes"] > 0, "invalid implementation byte count")
        require(isinstance(row["sha256"], str) and SHA_RE.fullmatch(row["sha256"]) is not None, "invalid implementation digest")
        rows.append(row)
    paths = [row["path"] for row in rows]
    require(paths == sorted(set(paths)), "implementation file closure is not sorted and unique")
    require(binding["identitySha256"] == sha256_bytes(canonical_bytes(rows)), "implementation identity drift")
    if check_files:
        require(paths == list(IMPLEMENTATION_PATHS), "implementation file closure drift")
        require(discovered_implementation_paths() == IMPLEMENTATION_PATHS, "unbound local implementation import")
        require(rows == implementation_rows(), "Open Agent implementation bytes drift")
    return binding


def v4_scaffold_identity(harness: dict[str, Any]) -> str:
    material = {
        "kind": "genesis/genesisbench-open-agent-scaffold-v0.4",
        "implementationIdentitySha256": harness["implementation"]["identitySha256"],
        "invocationProfiles": harness["invocationProfiles"],
        "fixedEnvironment": V3_FIXED_ENVIRONMENT,
        "promptPolicy": "one-fixed-task-prompt-frozen-repository-one-writable-workspace-v0.4",
    }
    return sha256_bytes(canonical_bytes(material))


def render_v4_authority() -> dict[str, Any]:
    files = implementation_rows()
    document = {
        "coldAcquisitionAdapterProfileUnchanged": True,
        "contentIdentitySha256": "",
        "implementation": {
            "entrypoints": list(IMPLEMENTATION_ENTRYPOINTS),
            "files": files,
            "identitySha256": sha256_bytes(canonical_bytes(files)),
        },
        "invocationProfiles": copy.deepcopy(V4_INVOCATION_PROFILES),
        "kind": KIND_AUTHORITY_V4,
        "limits": {
            "maxEventLineBytes": MAX_CAPTURE_BYTES,
            "maxStderrBytes": MAX_CAPTURE_BYTES,
            "maxStdoutBytes": MAX_CAPTURE_BYTES,
            "maxTimeoutMs": 3_600_000,
            "maxWorkspaceBytes": 64 * 1024 * 1024,
            "maxWorkspaceFiles": 4_096,
        },
        "purpose": V4_PURPOSE,
        "scaffoldIdentitySha256": "",
        "securityControls": V4_SECURITY_CONTROLS,
        "version": "0.4.0",
    }
    document["scaffoldIdentitySha256"] = v4_scaffold_identity(document)
    return identified(document)


def v5_local_execution() -> dict[str, Any]:
    schema = regular_file(genesisbench_mlx_custody.SCHEMA_PATH, "MLX custody schema")
    return {
        "custodySchema": {
            "bytes": schema.stat().st_size,
            "path": str(schema.relative_to(ROOT)),
            "sha256": sha256_file(schema),
        },
        "disabledFeatures": list(V5_DISABLED_FEATURES),
        "disabledSystemSkills": list(V5_DISABLED_SYSTEM_SKILLS),
        "expectedToolNames": list(genesisbench_mlx_responses.EXPECTED_TOOL_NAMES),
        "acceleratorPolicy": "apple-metal-system-graphics",
        "isolationBackend": "darwin-sandbox-exec-v0.1",
        "providerProtocol": "responses-to-mlx-chat-completions-v0.1",
        "requestRetries": 0,
        "streamRetries": 0,
    }


def v5_scaffold_identity(harness: dict[str, Any]) -> str:
    material = {
        "kind": "genesis/genesisbench-open-agent-scaffold-v0.5",
        "implementationIdentitySha256": harness["implementation"]["identitySha256"],
        "invocationProfiles": harness["invocationProfiles"],
        "localExecution": harness["localExecution"],
        "fixedEnvironment": V3_FIXED_ENVIRONMENT,
        "promptPolicy": "one-fixed-task-prompt-frozen-repository-one-writable-workspace-v0.5",
    }
    return sha256_bytes(canonical_bytes(material))


def render_v5_authority() -> dict[str, Any]:
    files = implementation_rows()
    document = {
        "coldAcquisitionAdapterProfileUnchanged": True,
        "contentIdentitySha256": "",
        "implementation": {
            "entrypoints": list(IMPLEMENTATION_ENTRYPOINTS),
            "files": files,
            "identitySha256": sha256_bytes(canonical_bytes(files)),
        },
        "invocationProfiles": copy.deepcopy(V5_INVOCATION_PROFILES),
        "kind": KIND_AUTHORITY_V5,
        "limits": {
            "maxEventLineBytes": MAX_CAPTURE_BYTES,
            "maxStderrBytes": MAX_CAPTURE_BYTES,
            "maxStdoutBytes": MAX_CAPTURE_BYTES,
            "maxTimeoutMs": 3_600_000,
            "maxWorkspaceBytes": 64 * 1024 * 1024,
            "maxWorkspaceFiles": 4_096,
        },
        "localExecution": v5_local_execution(),
        "purpose": V5_PURPOSE,
        "scaffoldIdentitySha256": "",
        "securityControls": V5_SECURITY_CONTROLS,
        "version": "0.5.0",
    }
    document["scaffoldIdentitySha256"] = v5_scaffold_identity(document)
    return identified(document)


def declared_environment_names(harness: dict[str, Any], runner_class: str | None = None) -> list[str]:
    if is_v5_harness(harness) and runner_class == "codex-cli-local":
        return sorted({
            "CODEX_HOME", "GENESIS_SELFHOST_COMPILED_CACHE_DISABLE", "HOME", "NO_COLOR",
            "NO_PROXY", "PATH", "TMPDIR",
        })
    names = set(ALLOWED_ENVIRONMENT)
    if is_v3_or_later_harness(harness):
        names.update(V3_FIXED_ENVIRONMENT)
    return sorted(names)


def event_line_limit(harness: dict[str, Any]) -> int:
    return harness["limits"].get("maxEventLineBytes", 1024 * 1024)


def expected_workspace_paths(case: dict[str, Any], harness: dict[str, Any]) -> set[str]:
    paths = {row["path"] for row in case["inputFiles"]}
    if is_v3_or_later_harness(harness):
        paths.update(case["editablePaths"])
    return paths


def source_rows(protocol: dict[str, Any] | None = None) -> list[dict[str, Any]]:
    commit = (protocol or load_json(PROTOCOL_PATH))["sourceSnapshot"]["commitSha1"]
    return genesisbench_protocol.tree_rows(commit)


def rows_identity(rows: list[dict[str, Any]]) -> str:
    return sha256_bytes(canonical_bytes(rows))


def source_rows_identity(rows: list[dict[str, Any]]) -> str:
    return genesisbench_protocol.sha256_bytes(genesisbench_protocol.canonical_bytes(rows))


def case_binding(case: dict[str, Any]) -> dict[str, Any]:
    return {
        "id": case["id"],
        "lineageId": case["lineageId"],
        "lineageIdentitySha256": case["lineageIdentitySha256"],
        "conditionId": case["conditionId"],
        "conditionIdentitySha256": case["conditionIdentitySha256"],
        "editablePaths": case["editablePaths"],
    }


def executable_version(executable: Path) -> str:
    result = subprocess.run(
        [str(executable), "--version"], stdout=subprocess.PIPE, stderr=subprocess.PIPE,
        check=False, timeout=10,
    )
    require(result.returncode == 0, "agent executable --version failed")
    payload = result.stdout.strip() or result.stderr.strip()
    require(payload and len(payload) <= 512 and payload.isascii(), "invalid agent executable version")
    version = payload.decode("ascii")
    require(re.fullmatch(r"codex-cli [0-9][A-Za-z0-9.+-]{0,127}", version) is not None, "runner is not an identified Codex CLI")
    return version


def campaign_case_ids(suite: dict[str, Any], phase: str) -> list[str]:
    if phase == "reality-gate":
        expected = [f"{task_class}-small" for task_class in suite["taskClasses"]]
    elif phase == "full-public":
        expected = [case["id"] for case in suite["cases"]]
    else:
        raise OpenAgentError("unsupported Open Agent campaign phase")
    return sorted(expected)


def common_predeclaration_fields(
    *, runner_class: str, executable: Path, model_id: str, model_revision: str,
    immutable_revision: bool, reasoning_effort: str, timeout_ms: int,
    local_provider: str | None, model_artifact_sha256: str | None,
    genesis_bin: Path, selfhost_artifact: Path, custody_manifest: Path | None = None,
) -> dict[str, Any]:
    executable = regular_file(executable, "agent executable")
    genesis_bin = regular_file(genesis_bin, "GenesisCode executable")
    selfhost_artifact = regular_file(selfhost_artifact, "self-host artifact")
    harness = authority()
    require(runner_class in {"codex-cli-hosted", "codex-cli-local"}, "unsupported Open Agent runner class")
    require(reasoning_effort in {"low", "medium", "high", "xhigh"}, "unsupported reasoning effort")
    require(1_000 <= timeout_ms <= harness["limits"]["maxTimeoutMs"], "timeout is outside harness limits")
    if runner_class == "codex-cli-local":
        allowed_providers = {"mlx-responses"} if is_v5_harness(harness) else {"lmstudio", "ollama"}
        require(local_provider in allowed_providers, "local runner requires a supported provider")
        require(model_artifact_sha256 is not None and SHA_RE.fullmatch(model_artifact_sha256) is not None, "local runner requires a model artifact digest")
        network_mode = "loopback-provider-only"
    else:
        require(local_provider is None and model_artifact_sha256 is None and custody_manifest is None, "hosted runner cannot declare local provider material")
        network_mode = "provider-only"
    common = {
        "track": {
            "id": "open-agent",
            "scaffoldClass": "codex-cli",
            "scaffoldIdentitySha256": harness["scaffoldIdentitySha256"],
            "rankEligible": False,
        },
        "runner": {
            "class": runner_class,
            "executableSha256": sha256_file(executable),
            "executableVersion": executable_version(executable),
            "invocationProfile": harness["invocationProfiles"][runner_class]["id"],
            "localProvider": local_provider,
        },
        "model": {
            "requestedId": safe_id(model_id, "model id"),
            "revision": safe_id(model_revision, "model revision"),
            "immutableRevision": immutable_revision,
            "reasoningEffort": reasoning_effort,
            "artifactSha256": model_artifact_sha256,
        },
        "attemptPolicy": {"attempts": 1, "hiddenRetriesAllowed": False, "resumeAllowed": False},
        "capabilities": {
            "workspace": "case-input-write-only",
            "repository": "frozen-read-only",
            "network": network_mode,
            "approvalPolicy": "never",
            "additionalWritableRoots": [],
        },
        "limits": {
            "timeoutMs": timeout_ms,
            "maxStdoutBytes": harness["limits"]["maxStdoutBytes"],
            "maxStderrBytes": harness["limits"]["maxStderrBytes"],
            "maxWorkspaceFiles": harness["limits"]["maxWorkspaceFiles"],
            "maxWorkspaceBytes": harness["limits"]["maxWorkspaceBytes"],
        },
        "disclosure": {
            "environmentNames": declared_environment_names(harness, runner_class),
            "environmentValuesRecorded": False,
            "genesisSpecificTraining": "unknown",
            "contaminationLabel": "unknown",
        },
    }
    if is_v3_or_later_harness(harness):
        common["tools"] = {
            "genesisExecutableSha256": sha256_file(genesis_bin),
            "selfhostArtifactSha256": sha256_file(selfhost_artifact),
        }
    if is_v5_harness(harness) and runner_class == "codex-cli-local":
        require(custody_manifest is not None, "v0.5 local runner requires a custody manifest")
        custody = genesisbench_mlx_custody.validate(load_json(custody_manifest))
        require(custody["model"]["id"] == model_id and custody["model"]["revision"] == model_revision, "custody model binding drift")
        require(custody["model"]["artifactIdentitySha256"] == model_artifact_sha256, "custody artifact binding drift")
        common["custody"] = {
            "adapterIdentitySha256": custody["adapter"]["identitySha256"],
            "inventoryIdentitySha256": custody["inventoryIdentitySha256"],
            "manifestIdentitySha256": custody["contentIdentitySha256"],
            "preselectionIdentitySha256": custody["preselectionIdentitySha256"],
            "runtimeIdentitySha256": custody["runtime"]["runtimeIdentitySha256"],
        }
    return common


def build_campaign(
    *, campaign_id: str, phase: str, case_ids: list[str], runner_class: str,
    executable: Path, model_id: str, model_revision: str, immutable_revision: bool,
    reasoning_effort: str, timeout_ms: int, local_provider: str | None,
    model_artifact_sha256: str | None, hardware_class: str,
    genesis_bin: Path, selfhost_artifact: Path, custody_manifest: Path | None = None,
) -> dict[str, Any]:
    suite = load_json(SUITE_PATH)
    protocol = load_json(PROTOCOL_PATH)
    harness = authority()
    if is_v4_or_later_harness(harness):
        validate_implementation_binding(harness["implementation"], check_files=True)
    expected_ids = campaign_case_ids(suite, phase)
    require(sorted(case_ids) == expected_ids and len(case_ids) == len(set(case_ids)), "campaign case matrix is incomplete or non-canonical")
    cases = [case_binding(next(case for case in suite["cases"] if case["id"] == case_id)) for case_id in expected_ids]
    common = common_predeclaration_fields(
        runner_class=runner_class, executable=executable, model_id=model_id,
        model_revision=model_revision, immutable_revision=immutable_revision,
        reasoning_effort=reasoning_effort, timeout_ms=timeout_ms,
        local_provider=local_provider, model_artifact_sha256=model_artifact_sha256,
        genesis_bin=genesis_bin, selfhost_artifact=selfhost_artifact,
        custody_manifest=custody_manifest,
    )
    document = {
        "kind": KIND_CAMPAIGN,
        "version": "0.1.0",
        "campaignId": safe_id(campaign_id, "campaign id"),
        "phase": phase,
        "cases": cases,
        "authorities": {
            "harnessIdentitySha256": harness["contentIdentitySha256"],
            "protocolIdentitySha256": protocol["contentIdentitySha256"],
            "suiteIdentitySha256": suite["contentIdentitySha256"],
            "sourceSnapshotManifestIdentitySha256": protocol["sourceSnapshot"]["manifestIdentitySha256"],
        },
        **common,
        "stopPolicy": {
            "executeEveryPredeclaredCase": True,
            "stopOnModelFailure": False,
            "stopOnInvalidAttempt": False,
            "expansionRequiresCompleteValidationAndReplay": True,
        },
        "host": {
            "hardwareClass": safe_id(hardware_class, "hardware class"),
            "architecture": safe_id(platform.machine(), "host architecture"),
            "operatingSystem": safe_id(platform.system(), "host operating system"),
        },
        "secrets": {
            "source": "none-auth-free-homes" if runner_class == "codex-cli-local" and is_v5_harness(harness) else "codex-auth-store",
            "valuesRecorded": False,
            "forwardedToWorkspace": False,
        },
        "publication": {
            "class": "authentic-unranked-open-agent",
            "missingImmutableModelIdentityForcesUnranked": True,
            "expectedAttemptCount": len(cases),
        },
        "contentIdentitySha256": "",
    }
    return identified(document)


def validate_campaign(document: Any) -> dict[str, Any]:
    top = {
        "kind", "version", "campaignId", "phase", "cases", "authorities", "track",
        "runner", "model", "attemptPolicy", "capabilities", "limits", "disclosure",
        "stopPolicy", "host", "secrets", "publication", "contentIdentitySha256",
    }
    require(isinstance(document, dict), "Open Agent campaign must be an object")
    harness_identity = document.get("authorities", {}).get("harnessIdentitySha256")
    harness = authority(harness_identity)
    if is_v3_or_later_harness(harness):
        top.add("tools")
    if is_v5_harness(harness) and document.get("runner", {}).get("class") == "codex-cli-local":
        top.add("custody")
    doc = closed(document, top, "Open Agent campaign")
    require(doc["kind"] == KIND_CAMPAIGN and doc["version"] == "0.1.0", "campaign kind/version drift")
    safe_id(doc["campaignId"], "campaign id")
    authority_binding = closed(doc["authorities"], {"harnessIdentitySha256", "protocolIdentitySha256", "suiteIdentitySha256", "sourceSnapshotManifestIdentitySha256"}, "campaign authorities")
    suite = suite_authority(authority_binding["suiteIdentitySha256"])
    expected_ids = campaign_case_ids(suite, doc["phase"])
    expected_cases = [case_binding(next(case for case in suite["cases"] if case["id"] == case_id)) for case_id in expected_ids]
    require(doc["cases"] == expected_cases, "campaign case bindings drift")
    protocol = protocol_authority(authority_binding["protocolIdentitySha256"])
    require(harness["contentIdentitySha256"] == authority_binding["harnessIdentitySha256"], "campaign harness binding drift")
    require(doc["authorities"] == {
        "harnessIdentitySha256": harness["contentIdentitySha256"],
        "protocolIdentitySha256": protocol["contentIdentitySha256"],
        "suiteIdentitySha256": suite["contentIdentitySha256"],
        "sourceSnapshotManifestIdentitySha256": protocol["sourceSnapshot"]["manifestIdentitySha256"],
    }, "campaign authority binding drift")
    validate_common_fields(doc, harness)
    require(doc["stopPolicy"] == {
        "executeEveryPredeclaredCase": True, "stopOnModelFailure": False,
        "stopOnInvalidAttempt": False, "expansionRequiresCompleteValidationAndReplay": True,
    }, "campaign stop policy drift")
    host = closed(doc["host"], {"hardwareClass", "architecture", "operatingSystem"}, "campaign host")
    for field in host: safe_id(host[field], f"host {field}")
    secret_source = "none-auth-free-homes" if is_v5_harness(harness) and doc["runner"]["class"] == "codex-cli-local" else "codex-auth-store"
    require(doc["secrets"] == {"source": secret_source, "valuesRecorded": False, "forwardedToWorkspace": False}, "campaign secret policy drift")
    require(doc["publication"] == {
        "class": "authentic-unranked-open-agent",
        "missingImmutableModelIdentityForcesUnranked": True,
        "expectedAttemptCount": len(expected_cases),
    }, "campaign publication policy drift")
    validate_identity(doc)
    return doc


def build_predeclaration(
    *, case_id: str, campaign: dict[str, Any],
) -> dict[str, Any]:
    campaign = validate_campaign(campaign)
    suite = suite_authority(campaign["authorities"]["suiteIdentitySha256"])
    suite, case = suite_and_case(case_id, suite)
    protocol = protocol_authority(campaign["authorities"]["protocolIdentitySha256"])
    harness = authority(campaign["authorities"]["harnessIdentitySha256"])
    require(case_binding(case) in campaign["cases"], "case is not predeclared by campaign")
    common_fields = ["track", "runner", "model", "attemptPolicy", "capabilities", "limits", "disclosure"]
    if is_v3_or_later_harness(harness):
        common_fields.append("tools")
    if is_v5_harness(harness) and campaign["runner"]["class"] == "codex-cli-local":
        common_fields.append("custody")
    document = {
        "kind": KIND_PREDECLARATION,
        "version": "0.1.0",
        "campaignId": campaign["campaignId"],
        "campaignIdentitySha256": campaign["contentIdentitySha256"],
        "case": case_binding(case),
        "authorities": {
            "harnessIdentitySha256": harness["contentIdentitySha256"],
            "protocolIdentitySha256": protocol["contentIdentitySha256"],
            "suiteIdentitySha256": suite["contentIdentitySha256"],
            "sourceSnapshotManifestIdentitySha256": protocol["sourceSnapshot"]["manifestIdentitySha256"],
        },
        **{field: copy.deepcopy(campaign[field]) for field in common_fields},
        "contentIdentitySha256": "",
    }
    return identified(document)


def validate_common_fields(doc: dict[str, Any], harness: dict[str, Any] | None = None) -> None:
    harness = harness or authority()
    runner = closed(doc["runner"], {"class", "executableSha256", "executableVersion", "invocationProfile", "localProvider"}, "runner")
    require(runner["class"] in harness["invocationProfiles"], "unknown runner class")
    require(runner["invocationProfile"] == harness["invocationProfiles"][runner["class"]]["id"], "invocation profile drift")
    require(SHA_RE.fullmatch(runner["executableSha256"] or "") is not None, "invalid executable digest")
    require(isinstance(runner["executableVersion"], str) and runner["executableVersion"], "invalid executable version")
    model = closed(doc["model"], {"requestedId", "revision", "immutableRevision", "reasoningEffort", "artifactSha256"}, "model")
    safe_id(model["requestedId"], "model id"); safe_id(model["revision"], "model revision")
    require(type(model["immutableRevision"]) is bool, "immutableRevision must be boolean")
    require(model["reasoningEffort"] in {"low", "medium", "high", "xhigh"}, "invalid reasoning effort")
    if runner["class"] == "codex-cli-local":
        allowed_providers = {"mlx-responses"} if is_v5_harness(harness) else {"lmstudio", "ollama"}
        require(runner["localProvider"] in allowed_providers, "invalid local provider")
        require(SHA_RE.fullmatch(model["artifactSha256"] or "") is not None, "local model artifact is unbound")
        expected_network = "loopback-provider-only"
    else:
        require(runner["localProvider"] is None and model["artifactSha256"] is None, "hosted runner contains local bindings")
        expected_network = "provider-only"
    require(doc["track"] == {
        "id": "open-agent", "scaffoldClass": "codex-cli",
        "scaffoldIdentitySha256": harness["scaffoldIdentitySha256"], "rankEligible": False,
    }, "track binding drift")
    require(doc["attemptPolicy"] == {"attempts": 1, "hiddenRetriesAllowed": False, "resumeAllowed": False}, "attempt policy drift")
    require(doc["capabilities"] == {
        "workspace": "case-input-write-only", "repository": "frozen-read-only",
        "network": expected_network, "approvalPolicy": "never", "additionalWritableRoots": [],
    }, "capability policy drift")
    limits = closed(doc["limits"], {"timeoutMs", "maxStdoutBytes", "maxStderrBytes", "maxWorkspaceFiles", "maxWorkspaceBytes"}, "limits")
    require(1_000 <= limits["timeoutMs"] <= harness["limits"]["maxTimeoutMs"], "invalid timeout")
    for field in ("maxStdoutBytes", "maxStderrBytes", "maxWorkspaceFiles", "maxWorkspaceBytes"):
        require(limits[field] == harness["limits"][field], f"{field} limit drift")
    require(doc["disclosure"] == {
        "environmentNames": declared_environment_names(harness, runner["class"]), "environmentValuesRecorded": False,
        "genesisSpecificTraining": "unknown", "contaminationLabel": "unknown",
    }, "disclosure drift")
    if is_v3_or_later_harness(harness):
        tools = closed(
            doc["tools"], {"genesisExecutableSha256", "selfhostArtifactSha256"},
            "supplied tool binding",
        )
        require(
            all(SHA_RE.fullmatch(tools[field] or "") is not None for field in tools),
            "invalid supplied tool digest",
        )
    if is_v5_harness(harness) and runner["class"] == "codex-cli-local":
        custody = closed(doc["custody"], {
            "adapterIdentitySha256", "inventoryIdentitySha256", "manifestIdentitySha256",
            "preselectionIdentitySha256", "runtimeIdentitySha256",
        }, "local custody binding")
        require(all(SHA_RE.fullmatch(custody[field] or "") is not None for field in custody), "invalid local custody digest")


def validate_predeclaration(document: Any, campaign: dict[str, Any]) -> dict[str, Any]:
    campaign = validate_campaign(campaign)
    harness = authority(campaign["authorities"]["harnessIdentitySha256"])
    top = {
        "kind", "version", "campaignId", "campaignIdentitySha256", "case", "authorities", "track", "runner",
        "model", "attemptPolicy", "capabilities", "limits", "disclosure", "contentIdentitySha256",
    }
    if is_v3_or_later_harness(harness):
        top.add("tools")
    if is_v5_harness(harness) and document.get("runner", {}).get("class") == "codex-cli-local":
        top.add("custody")
    doc = closed(document, top, "Open Agent predeclaration")
    require(doc["kind"] == KIND_PREDECLARATION and doc["version"] == "0.1.0", "predeclaration kind/version drift")
    require(doc["campaignId"] == campaign["campaignId"] and doc["campaignIdentitySha256"] == campaign["contentIdentitySha256"], "attempt campaign binding drift")
    suite = suite_authority(campaign["authorities"]["suiteIdentitySha256"])
    suite, case = suite_and_case(doc["case"].get("id"), suite)
    require(doc["case"] == case_binding(case), "case binding drift")
    require(doc["case"] in campaign["cases"], "attempt case is absent from campaign")
    protocol = protocol_authority(campaign["authorities"]["protocolIdentitySha256"])
    require(doc["authorities"] == {
        "harnessIdentitySha256": harness["contentIdentitySha256"],
        "protocolIdentitySha256": protocol["contentIdentitySha256"],
        "suiteIdentitySha256": suite["contentIdentitySha256"],
        "sourceSnapshotManifestIdentitySha256": protocol["sourceSnapshot"]["manifestIdentitySha256"],
    }, "predeclaration authority binding drift")
    validate_common_fields(doc, harness)
    common_fields = ["track", "runner", "model", "attemptPolicy", "capabilities", "limits", "disclosure"]
    if is_v3_or_later_harness(harness):
        common_fields.append("tools")
    if is_v5_harness(harness) and campaign["runner"]["class"] == "codex-cli-local":
        common_fields.append("custody")
    for field in common_fields:
        require(doc[field] == campaign[field], f"attempt {field} differs from campaign")
    validate_identity(doc)
    return doc


def validate_tool_archive(path: Path, campaign: dict[str, Any]) -> dict[str, Any]:
    campaign = validate_campaign(campaign)
    doc = closed(load_json(path), {
        "kind", "version", "campaignId", "campaignIdentitySha256", "authorities",
        "platform", "genesisExecutable", "selfhostArtifact", "contentIdentitySha256",
    }, "Open Agent tool archive")
    require(
        doc["kind"] == "genesis/genesisbench-open-agent-tool-archive-v0.1"
        and doc["version"] == "0.1.0",
        "tool archive kind/version drift",
    )
    require(doc["campaignId"] == campaign["campaignId"], "tool archive campaign id drift")
    require(doc["campaignIdentitySha256"] == campaign["contentIdentitySha256"], "tool archive campaign identity drift")
    require(doc["authorities"] == campaign["authorities"], "tool archive authority binding drift")
    closed(doc["platform"], {"architecture", "operatingSystem"}, "tool archive platform")
    executable = closed(doc["genesisExecutable"], {
        "path", "compression", "compressedBytes", "compressedSha256", "uncompressedBytes",
        "uncompressedSha256", "executableVersion",
    }, "archived Genesis executable")
    artifact = closed(
        doc["selfhostArtifact"], {"path", "bytes", "sha256"}, "archived self-host artifact",
    )
    require(executable["compression"] == "gzip-mtime-zero-level-9", "tool archive compression drift")
    compressed_path = path.parent.parent / safe_relative(executable["path"], "archived executable path")
    artifact_path = path.parent.parent / safe_relative(artifact["path"], "archived artifact path")
    require(compressed_path.is_file() and not compressed_path.is_symlink(), "archived executable is unavailable")
    require(artifact_path.is_file() and not artifact_path.is_symlink(), "archived self-host artifact is unavailable")
    require(compressed_path.stat().st_size == executable["compressedBytes"], "archived executable byte count drift")
    require(sha256_file(compressed_path) == executable["compressedSha256"], "archived executable digest drift")
    raw = gzip.decompress(compressed_path.read_bytes())
    require(len(raw) == executable["uncompressedBytes"], "uncompressed executable byte count drift")
    require(sha256_bytes(raw) == executable["uncompressedSha256"], "uncompressed executable digest drift")
    require(artifact_path.stat().st_size == artifact["bytes"], "archived artifact byte count drift")
    require(sha256_file(artifact_path) == artifact["sha256"], "archived artifact digest drift")
    if is_v3_or_later_harness(authority(campaign["authorities"]["harnessIdentitySha256"])):
        require(executable["uncompressedSha256"] == campaign["tools"]["genesisExecutableSha256"], "archived executable campaign binding drift")
        require(artifact["sha256"] == campaign["tools"]["selfhostArtifactSha256"], "archived artifact campaign binding drift")
    validate_identity(doc)
    return doc


def require_compatible_archive_platform(archive: dict[str, Any]) -> None:
    archived = archive["platform"]
    host = {
        "architecture": platform.machine(),
        "operatingSystem": platform.system(),
    }
    require(
        archived == host,
        "archived replay tool is incompatible with this host: "
        f"archive={archived['operatingSystem']}/{archived['architecture']} "
        f"host={host['operatingSystem']}/{host['architecture']}",
    )


def archive_snapshot(destination: Path, protocol: dict[str, Any] | None = None) -> tuple[list[dict[str, Any]], str]:
    protocol = protocol or load_json(PROTOCOL_PATH)
    rows = source_rows(protocol)
    destination.mkdir(parents=True)
    archive = subprocess.run(
        ["git", "archive", "--format=tar", protocol["sourceSnapshot"]["commitSha1"]],
        cwd=ROOT, stdout=subprocess.PIPE, stderr=subprocess.PIPE, check=False,
    )
    require(archive.returncode == 0, "cannot materialize frozen source snapshot")
    with tempfile.TemporaryFile() as stream:
        stream.write(archive.stdout); stream.seek(0)
        with tarfile.open(fileobj=stream, mode="r:") as bundle:
            members = [member for member in bundle.getmembers() if member.isfile()]
            require(len(members) == len(rows), "snapshot archive file count drift")
            by_path = {row["path"]: row for row in rows}
            for member in members:
                relative = safe_relative(member.name, "snapshot member")
                row = by_path.get(member.name)
                require(row is not None and row["type"] == "blob", "unexpected snapshot member")
                source = bundle.extractfile(member)
                require(source is not None, "snapshot member has no payload")
                payload = source.read()
                require(len(payload) == row["bytes"] and sha256_bytes(payload) == row["sha256"], f"snapshot byte drift: {member.name}")
                target = destination.joinpath(*relative.parts)
                target.parent.mkdir(parents=True, exist_ok=True)
                target.write_bytes(payload)
                target.chmod(0o555 if row["mode"] == "100755" else 0o444)
    for path in sorted((p for p in destination.rglob("*") if p.is_dir()), key=lambda p: len(p.parts), reverse=True):
        path.chmod(0o555)
    return rows, source_rows_identity(rows)


def validate_snapshot_root(root: Path, expected: list[dict[str, Any]]) -> str:
    expected_by_path = {row["path"]: row for row in expected}
    expected_directories = {
        PurePosixPath(row["path"]).parent.as_posix()
        for row in expected
        if PurePosixPath(row["path"]).parent.as_posix() != "."
    }
    pending = list(expected_directories)
    while pending:
        parent = PurePosixPath(pending.pop()).parent.as_posix()
        if parent != "." and parent not in expected_directories:
            expected_directories.add(parent)
            pending.append(parent)
    observed_files: set[str] = set()
    observed_directories: set[str] = set()
    for path in sorted(root.rglob("*"), key=lambda item: item.relative_to(root).as_posix()):
        relative = path.relative_to(root).as_posix()
        require(not path.is_symlink(), f"symlink in frozen snapshot: {relative}")
        if path.is_dir():
            observed_directories.add(relative)
            require(stat.S_IMODE(path.stat().st_mode) == 0o555, f"snapshot directory mode drift: {relative}")
            continue
        require(path.is_file(), f"non-regular frozen snapshot entry: {relative}")
        observed_files.add(relative)
        row = expected_by_path.get(relative)
        require(row is not None, f"extra frozen snapshot file: {relative}")
        expected_mode = 0o555 if row["mode"] == "100755" else 0o444
        require(stat.S_IMODE(path.stat().st_mode) == expected_mode, f"snapshot mode drift: {relative}")
        require(path.stat().st_size == row["bytes"] and sha256_file(path) == row["sha256"], f"snapshot byte drift: {relative}")
    require(observed_files == set(expected_by_path), "frozen snapshot file topology drift")
    require(observed_directories == expected_directories, "frozen snapshot directory topology drift")
    return source_rows_identity(expected)


def inventory(root: Path, *, max_files: int, max_bytes: int) -> list[dict[str, Any]]:
    require(root.is_dir() and not root.is_symlink(), "inventory root is unavailable")
    rows: list[dict[str, Any]] = []
    total = 0
    for path in sorted(root.rglob("*"), key=lambda item: item.relative_to(root).as_posix()):
        relative = path.relative_to(root).as_posix()
        require(not path.is_symlink(), f"symlink forbidden: {relative}")
        if path.is_dir():
            continue
        require(path.is_file(), f"non-regular workspace entry: {relative}")
        safe_relative(relative, "workspace path")
        size = path.stat().st_size
        total += size
        require(len(rows) < max_files and total <= max_bytes, "workspace exceeds finite limits")
        rows.append({"path": relative, "bytes": size, "sha256": sha256_file(path)})
    return rows


def validate_inventory_rows(rows: Any, label: str) -> list[dict[str, Any]]:
    require(isinstance(rows, list), f"{label} must be an array")
    paths: list[str] = []
    total = 0
    for index, raw in enumerate(rows):
        row = closed(raw, {"path", "bytes", "sha256"}, f"{label}[{index}]")
        path = safe_relative(row["path"], f"{label} path").as_posix()
        require(isinstance(row["bytes"], int) and 0 <= row["bytes"] <= 64 * 1024 * 1024, f"invalid {label} byte count")
        require(isinstance(row["sha256"], str) and SHA_RE.fullmatch(row["sha256"]) is not None, f"invalid {label} digest")
        paths.append(path); total += row["bytes"]
    require(paths == sorted(set(paths)), f"{label} paths must be sorted and unique")
    require(len(rows) <= 4096 and total <= 128 * 1024 * 1024, f"{label} exceeds finite limits")
    return rows


def materialize_case(case: dict[str, Any], workspace: Path) -> list[dict[str, Any]]:
    source = ROOT / case["inputRoot"]
    require(source.is_dir() and not source.is_symlink(), "case input root is unavailable")
    workspace.mkdir(parents=True)
    for expected in case["inputFiles"]:
        relative = safe_relative(expected["path"], "case input path")
        source_path = source.joinpath(*relative.parts)
        require(source_path.is_file() and not source_path.is_symlink(), "case input is not regular")
        payload = source_path.read_bytes()
        require(len(payload) == expected["bytes"] and sha256_bytes(payload) == expected["sha256"], "case input drift")
        target = workspace.joinpath(*relative.parts)
        target.parent.mkdir(parents=True, exist_ok=True)
        target.write_bytes(payload)
    return inventory(workspace, max_files=4096, max_bytes=64 * 1024 * 1024)


def prompt_for(case: dict[str, Any]) -> str:
    editable = ", ".join(case["editablePaths"])
    return (
        "You are executing one predeclared GenesisBench Open Agent attempt. "
        "The current directory contains only the task inputs and is the only writable benchmark directory. "
        "A frozen read-only GenesisCode repository is at ../input/repository and the GenesisCode executable is "
        "../input/tools/genesis. Read its documentation and source as needed. Do not use network tools, create extra "
        "files, modify the repository, or access files outside these declared roots. Complete this task: "
        f"{case['prompt']} You may modify only: {editable}. Stop after one solution; the harness scores the files."
    )


def invocation(
    predeclaration: dict[str, Any], prompt: str, *, provider_url: str | None = None,
    codex_home: Path | None = None,
) -> list[str]:
    runner = predeclaration["runner"]
    model = predeclaration["model"]
    harness = authority(predeclaration["authorities"]["harnessIdentitySha256"])
    args = [
        "exec", "--ignore-user-config", "--ignore-rules", "--strict-config", "--ephemeral", "--json",
        "--color", "never", "--sandbox", "workspace-write", "--skip-git-repo-check",
        "--model", model["requestedId"], "-c", f'model_reasoning_effort="{model["reasoningEffort"]}"',
        "-c", 'approval_policy="never"',
    ]
    if runner["class"] == "codex-cli-local":
        if is_v5_harness(harness):
            require(provider_url is not None and codex_home is not None, "v0.5 local invocation requires an isolated provider and Codex home")
            require(re.fullmatch(r"http://127\.0\.0\.1:[0-9]{4,5}/v1", provider_url) is not None, "invalid local Responses endpoint")
            for feature in V5_DISABLED_FEATURES:
                args.extend(["--disable", feature])
            skills = ",".join(
                '{path="' + str(codex_home / "skills/.system" / skill / "SKILL.md") + '",enabled=false}'
                for skill in V5_DISABLED_SYSTEM_SKILLS
            )
            args.extend([
                "-c", 'model_provider="genesisbench_mlx"',
                "-c", 'web_search="disabled"',
                "-c", f"skills.config=[{skills}]",
                "-c", 'model_providers.genesisbench_mlx.name="GenesisBench MLX"',
                "-c", f'model_providers.genesisbench_mlx.base_url="{provider_url}"',
                "-c", 'model_providers.genesisbench_mlx.wire_api="responses"',
                "-c", "model_providers.genesisbench_mlx.request_max_retries=0",
                "-c", "model_providers.genesisbench_mlx.stream_max_retries=0",
            ])
        else:
            args.extend(["--oss", "--local-provider", runner["localProvider"]])
    args.append(prompt)
    return args


def sanitized_environment(harness: dict[str, Any]) -> tuple[dict[str, str], list[str]]:
    environment: dict[str, str] = {}
    present = []
    for name in ALLOWED_ENVIRONMENT:
        value = os.environ.get(name)
        if value is not None:
            environment[name] = value
            present.append(name)
    environment.setdefault("PATH", "/usr/bin:/bin:/usr/sbin:/sbin")
    if is_v3_or_later_harness(harness):
        environment.update(V3_FIXED_ENVIRONMENT)
        present.extend(V3_FIXED_ENVIRONMENT)
    else:
        environment["NO_COLOR"] = "1"
    return environment, sorted(set(present))


def terminate_group(process: subprocess.Popen[bytes]) -> None:
    if process.poll() is not None:
        return
    try:
        if os.name == "posix":
            os.killpg(process.pid, signal.SIGKILL)
        else:
            process.kill()
    except ProcessLookupError:
        pass
    process.wait()


def remove_tree(path: Path) -> None:
    if not path.exists():
        return
    for root, directories, _ in os.walk(path):
        os.chmod(root, stat.S_IRWXU)
        for name in directories:
            directory = Path(root) / name
            if not directory.is_symlink():
                os.chmod(directory, stat.S_IRWXU)
    shutil.rmtree(path)


def isolated_stage() -> Path:
    stage = Path(tempfile.mkdtemp(prefix="genesisbench-open-agent-")).resolve()
    require(ROOT.resolve() not in stage.parents and stage not in ROOT.resolve().parents, "agent stage is not ancestry-isolated")
    return stage


def run_process(
    executable: Path, args: list[str], cwd: Path, timeout_ms: int,
    stdout_path: Path, stderr_path: Path, max_stdout: int, max_stderr: int,
    harness: dict[str, Any], *, environment_override: dict[str, str] | None = None,
    command_prefix: list[str] | None = None,
) -> tuple[int | None, str, int, list[str]]:
    if environment_override is None:
        environment, present = sanitized_environment(harness)
    else:
        environment = dict(environment_override)
        present = sorted(environment)
    started = time.monotonic_ns()
    with stdout_path.open("wb") as stdout, stderr_path.open("wb") as stderr:
        process = subprocess.Popen(
            [*(command_prefix or []), str(executable), *args], cwd=cwd, env=environment, stdin=subprocess.DEVNULL,
            stdout=stdout, stderr=stderr, start_new_session=(os.name == "posix"),
        )
        reason = "exited"
        while process.poll() is None:
            elapsed_ms = (time.monotonic_ns() - started) // 1_000_000
            if elapsed_ms >= timeout_ms:
                reason = "timeout"; terminate_group(process); break
            stdout.flush(); stderr.flush()
            if stdout_path.stat().st_size > max_stdout or stderr_path.stat().st_size > max_stderr:
                reason = "capture-limit"; terminate_group(process); break
            time.sleep(0.02)
        return_code = process.poll()
    elapsed_ms = (time.monotonic_ns() - started) // 1_000_000
    if stdout_path.stat().st_size > max_stdout:
        with stdout_path.open("r+b") as stream: stream.truncate(max_stdout)
    if stderr_path.stat().st_size > max_stderr:
        with stderr_path.open("r+b") as stream: stream.truncate(max_stderr)
    return return_code, reason, elapsed_ms, present


def unused_loopback_port() -> int:
    with socket.socket(socket.AF_INET, socket.SOCK_STREAM) as listener:
        listener.bind(("127.0.0.1", 0))
        return int(listener.getsockname()[1])


def wait_for_local_provider(port: int, process: subprocess.Popen[bytes]) -> None:
    deadline = time.monotonic() + 130
    while time.monotonic() < deadline:
        require(process.poll() is None, "local MLX provider exited before readiness")
        try:
            with urllib.request.urlopen(
                f"http://127.0.0.1:{port}/__genesisbench__/health", timeout=0.25,
            ) as response:
                if response.status == 200:
                    return
        except (urllib.error.URLError, TimeoutError, OSError):
            time.sleep(0.05)
    raise OpenAgentError("local MLX provider readiness timeout")


def terminate_supervised_group(process: subprocess.Popen[bytes]) -> None:
    if process.poll() is None:
        try:
            if os.name == "posix":
                os.killpg(process.pid, signal.SIGTERM)
            else:
                process.terminate()
        except ProcessLookupError:
            pass
        try:
            process.wait(timeout=10)
        except subprocess.TimeoutExpired:
            terminate_group(process)
    else:
        process.wait()


def local_environment(home: Path, codex_home: Path, temp: Path) -> dict[str, str]:
    return {
        "CODEX_HOME": str(codex_home),
        "GENESIS_SELFHOST_COMPILED_CACHE_DISABLE": "1",
        "HOME": str(home),
        "NO_COLOR": "1",
        "NO_PROXY": "127.0.0.1,localhost",
        "PATH": "/usr/bin:/bin:/usr/sbin:/sbin",
        "TMPDIR": str(temp),
    }


def local_sandbox_system_roots() -> list[Path]:
    return [
        Path("/System"), Path("/usr"), Path("/bin"), Path("/Library"), Path("/dev"),
        Path("/private/etc"), Path("/private/var/db"),
    ]


def start_local_provider(
    *, stage: Path, custody_manifest: Path, model_root: Path, python: Path,
    predeclaration: dict[str, Any],
) -> tuple[subprocess.Popen[bytes], int, Path, Path, Path]:
    manifest = genesisbench_mlx_custody.validate(
        load_json(custody_manifest), check_local=True, model_root=model_root, python=python,
    )
    require(manifest["contentIdentitySha256"] == predeclaration["custody"]["manifestIdentitySha256"], "runtime custody manifest substitution")
    python = regular_file(python, "MLX Python executable")
    model_root = model_root.resolve(strict=True)
    provider_home = stage / "provider-home"; provider_home.mkdir()
    provider_temp = stage / "provider-tmp"; provider_temp.mkdir()
    evidence = stage / "provider-evidence"
    supervisor_stdout = stage / "provider-supervisor-stdout.txt"
    supervisor_stderr = stage / "provider-supervisor-stderr.txt"
    listen_port = unused_loopback_port(); backend_port = unused_loopback_port()
    require(listen_port != backend_port, "local provider port collision")
    model_repository = model_root.parent.parent
    profile = genesisbench_mlx_custody.darwin_profile(
        read_roots=[
            *local_sandbox_system_roots(), python.parent.parent, model_repository,
            ROOT / "scripts/lib", provider_home, provider_temp,
        ],
        write_roots=[provider_home, provider_temp, evidence],
        connect_ports=[backend_port],
        listen_ports=[listen_port, backend_port],
        allow_graphics=True,
    )
    command = [
        *genesisbench_mlx_custody.sandbox_prefix(profile), str(python), "-I",
        str(ROOT / "scripts/lib/genesisbench_mlx_responses.py"), "serve",
        "--model-root", str(model_root), "--model-id", predeclaration["model"]["requestedId"],
        "--listen-port", str(listen_port), "--backend-port", str(backend_port),
        "--evidence", str(evidence), "--home", str(provider_home), "--max-tokens", "4096",
    ]
    provider_environment = {
        "HOME": str(provider_home), "NO_COLOR": "1", "NO_PROXY": "127.0.0.1,localhost",
        "PATH": "/usr/bin:/bin:/usr/sbin:/sbin", "TMPDIR": str(provider_temp),
    }
    stdout = supervisor_stdout.open("xb"); stderr = supervisor_stderr.open("xb")
    try:
        process = subprocess.Popen(
            command, cwd=provider_home, env=provider_environment, stdin=subprocess.DEVNULL,
            stdout=stdout, stderr=stderr, start_new_session=True,
        )
    finally:
        stdout.close(); stderr.close()
    try:
        wait_for_local_provider(listen_port, process)
    except BaseException:
        terminate_supervised_group(process)
        raise
    return process, listen_port, evidence, supervisor_stdout, supervisor_stderr


def local_codex_boundary(
    *, executable: Path, stage: Path, workspace: Path, input_root: Path, listen_port: int,
) -> tuple[dict[str, str], list[str], Path]:
    home = stage / "agent-home"; home.mkdir()
    codex_home = stage / "agent-codex-home"; codex_home.mkdir()
    temp = stage / "agent-tmp"; temp.mkdir()
    profile = genesisbench_mlx_custody.darwin_profile(
        read_roots=[
            *local_sandbox_system_roots(), executable.parent.parent, input_root, workspace,
            home, codex_home, temp,
        ],
        write_roots=[workspace, home, codex_home, temp],
        connect_ports=[listen_port],
        listen_ports=[],
    )
    return local_environment(home, codex_home, temp), genesisbench_mlx_custody.sandbox_prefix(profile), codex_home


def retain_local_custody(
    *, retained: Path, manifest_path: Path, evidence: Path, supervisor_stdout: Path,
    supervisor_stderr: Path, stage: Path, model_root: Path,
) -> None:
    root = retained / "custody"; root.mkdir()
    shutil.copyfile(manifest_path, root / "manifest.json")
    shutil.copytree(evidence, root / "adapter")
    shutil.copyfile(supervisor_stdout, root / "supervisor-stdout.txt")
    shutil.copyfile(supervisor_stderr, root / "supervisor-stderr.txt")
    spellings = sorted({str(stage), str(stage).replace("/private/", "/")}, key=len, reverse=True)
    model_spellings = sorted({str(model_root.resolve()), str(model_root)}, key=len, reverse=True)
    for path in root.rglob("*"):
        if not path.is_file() or path.is_symlink():
            continue
        payload = path.read_bytes()
        for spelling in spellings:
            payload = payload.replace(spelling.encode("utf-8"), b"$GENESISBENCH_STAGE")
        for spelling in model_spellings:
            payload = payload.replace(spelling.encode("utf-8"), b"$GENESISBENCH_MODEL_ROOT")
        path.write_bytes(payload)
    session_path = root / "adapter/session.json"
    session = load_json(session_path)
    for record in session["records"]:
        index = record["index"]
        request = load_json(root / f"adapter/turn-{index:03d}-responses-request.json")
        events = (root / f"adapter/turn-{index:03d}-responses-events.sse").read_bytes()
        record["requestIdentitySha256"] = sha256_bytes(canonical_bytes(request))
        record["responseIdentitySha256"] = sha256_bytes(events)
    write_json(session_path, session)


def provider_violations(session: dict[str, Any] | None) -> list[str]:
    if session is None:
        return []
    violations: list[str] = []
    if session["backendRequestCount"] == 0:
        violations.append("provider-no-request")
    if session["rejections"] or session["authorizationHeadersObserved"] or session["hiddenRetriesObserved"]:
        violations.append("provider-policy-rejection")
    return violations


def validate_jsonl(path: Path, max_line_bytes: int = 1024 * 1024) -> tuple[bool, int]:
    count = 0
    try:
        with path.open("rb") as stream:
            for raw in stream:
                require(len(raw) <= max_line_bytes, "agent event exceeds line limit")
                if not raw.strip():
                    continue
                value = json.loads(raw)
                require(isinstance(value, dict), "agent event must be an object")
                count += 1
                require(count <= 100_000, "agent event count exceeds limit")
    except (UnicodeDecodeError, json.JSONDecodeError, OpenAgentError):
        return False, count
    return count > 0, count


def copy_candidate(
    workspace: Path, candidate: Path, case: dict[str, Any], harness: dict[str, Any],
) -> None:
    candidate.mkdir()
    for path in sorted(expected_workspace_paths(case, harness)):
        relative = safe_relative(path, "candidate path")
        source = workspace.joinpath(*relative.parts)
        require(source.is_file() and not source.is_symlink(), "candidate file is unavailable")
        target = candidate.joinpath(*relative.parts)
        target.parent.mkdir(parents=True, exist_ok=True)
        shutil.copyfile(source, target)


def copy_inventory_payload(
    workspace: Path, payload_root: Path, rows: list[dict[str, Any]],
) -> None:
    payload_root.mkdir()
    for row in rows:
        relative = safe_relative(row["path"], "observed workspace path")
        source = workspace.joinpath(*relative.parts)
        require(source.is_file() and not source.is_symlink(), "observed workspace file is unavailable")
        require(
            source.stat().st_size == row["bytes"] and sha256_file(source) == row["sha256"],
            "observed workspace changed after inventory",
        )
        target = payload_root.joinpath(*relative.parts)
        target.parent.mkdir(parents=True, exist_ok=True)
        shutil.copyfile(source, target)
    require(
        inventory(payload_root, max_files=4096, max_bytes=128 * 1024 * 1024) == rows,
        "observed workspace retention drift",
    )


def normalize_retained_stage_paths(paths: tuple[Path, ...], stage: Path) -> None:
    replacement = b"$GENESISBENCH_STAGE"
    spellings = {str(stage).encode("utf-8"), os.path.realpath(stage).encode("utf-8")}
    for path in paths:
        payload = path.read_bytes()
        for spelling in spellings:
            payload = payload.replace(spelling, replacement)
        path.write_bytes(payload)


def run_agent(
    campaign_path: Path, predeclaration_path: Path, out: Path, executable: Path,
    genesis_bin: Path, selfhost_artifact: Path, *, local_custody_manifest: Path | None = None,
    local_model_root: Path | None = None, local_python: Path | None = None,
) -> dict[str, Any]:
    require(not out.exists(), "Open Agent run output already exists")
    campaign = validate_campaign(load_json(campaign_path))
    predeclaration = validate_predeclaration(load_json(predeclaration_path), campaign)
    executable = regular_file(executable, "agent executable")
    genesis_bin = regular_file(genesis_bin, "GenesisCode executable")
    selfhost_artifact = regular_file(selfhost_artifact, "self-host artifact")
    require(sha256_file(executable) == predeclaration["runner"]["executableSha256"], "agent executable digest mismatch")
    require(executable_version(executable) == predeclaration["runner"]["executableVersion"], "agent executable version mismatch")
    harness = authority(campaign["authorities"]["harnessIdentitySha256"])
    if is_v4_or_later_harness(harness):
        require(
            harness["contentIdentitySha256"] == authority()["contentIdentitySha256"],
            "content-bound campaign execution requires the active harness authority",
        )
        validate_implementation_binding(harness["implementation"], check_files=True)
    if is_v3_or_later_harness(harness):
        require(sha256_file(genesis_bin) == predeclaration["tools"]["genesisExecutableSha256"], "GenesisCode executable digest mismatch")
        require(sha256_file(selfhost_artifact) == predeclaration["tools"]["selfhostArtifactSha256"], "self-host artifact digest mismatch")
    suite = suite_authority(campaign["authorities"]["suiteIdentitySha256"])
    _, case = suite_and_case(predeclaration["case"]["id"], suite)
    protocol = protocol_authority(campaign["authorities"]["protocolIdentitySha256"])
    out.parent.mkdir(parents=True, exist_ok=True)
    # Keep the process cwd outside the repository ancestry so project instructions
    # and skills cannot enter an otherwise isolated campaign through discovery.
    stage = isolated_stage()
    retained = stage / "retained"; retained.mkdir()
    input_root = stage / "input"; input_root.mkdir()
    repository = input_root / "repository"
    workspace = stage / "workspace"
    tools = input_root / "tools"; tools.mkdir()
    try:
        snapshot_rows, snapshot_identity = archive_snapshot(repository, protocol)
        before_workspace = materialize_case(case, workspace)
        shutil.copyfile(genesis_bin, tools / "genesis"); (tools / "genesis").chmod(0o555)
        shutil.copyfile(selfhost_artifact, tools / "toolchain.gc"); (tools / "toolchain.gc").chmod(0o444)
        tools.chmod(0o555); input_root.chmod(0o555)
        write_json(retained / "campaign.json", campaign)
        write_json(retained / "predeclaration.json", predeclaration)
        stdout_path = retained / "events.jsonl"; stderr_path = retained / "stderr.txt"
        local_v5 = is_v5_harness(harness) and predeclaration["runner"]["class"] == "codex-cli-local"
        local_provider_session: dict[str, Any] | None = None
        if local_v5:
            require(local_custody_manifest is not None and local_model_root is not None and local_python is not None, "v0.5 local execution inputs are incomplete")
            custody_path = regular_file(local_custody_manifest, "local custody manifest")
            provider, listen_port, provider_evidence, provider_stdout, provider_stderr = start_local_provider(
                stage=stage, custody_manifest=custody_path, model_root=local_model_root,
                python=local_python, predeclaration=predeclaration,
            )
            environment, prefix, codex_home = local_codex_boundary(
                executable=executable, stage=stage, workspace=workspace, input_root=input_root,
                listen_port=listen_port,
            )
            args = invocation(
                predeclaration, prompt_for(case), provider_url=f"http://127.0.0.1:{listen_port}/v1",
                codex_home=codex_home,
            )
            try:
                return_code, termination, elapsed_ms, present_names = run_process(
                    executable, args, workspace, predeclaration["limits"]["timeoutMs"],
                    stdout_path, stderr_path, predeclaration["limits"]["maxStdoutBytes"],
                    predeclaration["limits"]["maxStderrBytes"], harness,
                    environment_override=environment, command_prefix=prefix,
                )
            finally:
                terminate_supervised_group(provider)
            require(provider.returncode == 0, "local MLX provider did not terminate cleanly")
            local_provider_session = genesisbench_mlx_responses.validate_evidence(
                provider_evidence, predeclaration["model"]["requestedId"],
            )
            retain_local_custody(
                retained=retained, manifest_path=custody_path, evidence=provider_evidence,
                supervisor_stdout=provider_stdout, supervisor_stderr=provider_stderr,
                stage=stage, model_root=local_model_root,
            )
        else:
            require(local_custody_manifest is None and local_model_root is None and local_python is None, "non-v0.5 run received local custody inputs")
            args = invocation(predeclaration, prompt_for(case))
            return_code, termination, elapsed_ms, present_names = run_process(
                executable, args, workspace, predeclaration["limits"]["timeoutMs"],
                stdout_path, stderr_path, predeclaration["limits"]["maxStdoutBytes"],
                predeclaration["limits"]["maxStderrBytes"], harness,
            )
        if is_v3_or_later_harness(harness):
            normalize_retained_stage_paths((stdout_path, stderr_path), stage)
        violations: list[str] = []
        violations.extend(provider_violations(local_provider_session))
        try:
            repository_identity = validate_snapshot_root(repository, snapshot_rows)
        except OpenAgentError:
            repository_identity = None; violations.append("repository-invalid")
        try:
            after_workspace = inventory(
                workspace, max_files=predeclaration["limits"]["maxWorkspaceFiles"],
                max_bytes=predeclaration["limits"]["maxWorkspaceBytes"],
            )
            expected_paths = expected_workspace_paths(case, harness)
            actual_paths = {row["path"] for row in after_workspace}
            if actual_paths != expected_paths:
                violations.append("workspace-path-drift")
            before_by_path = {row["path"]: row for row in before_workspace}
            after_by_path = {row["path"]: row for row in after_workspace}
            editable = set(case["editablePaths"])
            if any(before_by_path[path] != after_by_path.get(path) for path in expected_paths - editable):
                violations.append("noneditable-input-drift")
        except OpenAgentError:
            after_workspace = []; workspace_inventory_status = "invalid"; violations.append("workspace-invalid")
        else:
            workspace_inventory_status = "valid"
        transcript_valid, event_count = validate_jsonl(stdout_path, event_line_limit(harness))
        if not transcript_valid:
            violations.append("malformed-event-transcript")
        if termination != "exited":
            violations.append(termination)
        if return_code != 0:
            violations.append("nonzero-exit")
        violations = sorted(set(violations))
        score_doc = None
        if violations and workspace_inventory_status == "valid" and is_v4_or_later_harness(harness):
            copy_inventory_payload(workspace, retained / "observed-workspace", after_workspace)
        if not violations:
            copy_candidate(workspace, retained / "candidate", case, harness)
            score_doc = gc_agent_scoring.score_candidate(
                protocol_bound_json(protocol, str(SCORING_PATH.relative_to(ROOT))),
                case["id"], retained / "candidate", genesis_bin, selfhost_artifact,
                suite_document=suite,
            )
            write_json(retained / "score.json", score_doc)
        artifact_rows = inventory(retained, max_files=4096, max_bytes=128 * 1024 * 1024)
        run = identified({
            "kind": KIND_RUN,
            "version": "0.1.0",
            "predeclarationIdentitySha256": predeclaration["contentIdentitySha256"],
            "case": case_binding(case),
            "attempt": {
                "index": 0, "returnCode": return_code, "termination": termination,
                "elapsedMs": elapsed_ms, "eventCount": event_count,
                "environmentPresentNames": present_names, "environmentValuesRecorded": False,
            },
            "workspace": {
                "beforeInventory": before_workspace, "afterInventory": after_workspace,
                "afterInventoryStatus": workspace_inventory_status,
                "sourceSnapshotBeforeIdentitySha256": snapshot_identity,
                "sourceSnapshotAfterIdentitySha256": repository_identity,
                "violations": violations,
            },
            "outcome": ("verified" if score_doc and score_doc["validity"]["passed"] else "failed") if score_doc is not None else "invalid",
            "scoreIdentitySha256": sha256_bytes(canonical_bytes(score_doc)) if score_doc is not None else None,
            "artifactInventory": artifact_rows,
            "artifactInventoryIdentitySha256": rows_identity(artifact_rows),
            "replay": {"agentAccessAllowed": False, "modelAccessAllowed": False, "independentRescoreRequired": score_doc is not None},
            "contentIdentitySha256": "",
        })
        write_json(retained / "run.json", run)
        publish_stage = Path(tempfile.mkdtemp(prefix=f".{out.name}-", dir=out.parent.resolve()))
        remove_tree(publish_stage)
        shutil.copytree(retained, publish_stage)
        publish_stage.rename(out)
        return run
    except BaseException:
        remove_tree(stage)
        raise
    finally:
        if stage.exists():
            remove_tree(stage)


def validate_run(run_path: Path, *, check_files: bool) -> dict[str, Any]:
    run = load_json(run_path)
    top = {
        "kind", "version", "predeclarationIdentitySha256", "case", "attempt", "workspace",
        "outcome", "scoreIdentitySha256", "artifactInventory", "artifactInventoryIdentitySha256",
        "replay", "contentIdentitySha256",
    }
    closed(run, top, "Open Agent run")
    require(run["kind"] == KIND_RUN and run["version"] == "0.1.0", "Open Agent run kind/version drift")
    validate_identity(run)
    root = run_path.parent
    campaign = validate_campaign(load_json(root / "campaign.json"))
    predeclaration = validate_predeclaration(load_json(root / "predeclaration.json"), campaign)
    harness = authority(campaign["authorities"]["harnessIdentitySha256"])
    require(run["predeclarationIdentitySha256"] == predeclaration["contentIdentitySha256"], "run predeclaration binding drift")
    suite = suite_authority(campaign["authorities"]["suiteIdentitySha256"])
    _, case = suite_and_case(run["case"].get("id"), suite)
    require(run["case"] == case_binding(case) == predeclaration["case"], "run case binding drift")
    attempt = closed(run["attempt"], {"index", "returnCode", "termination", "elapsedMs", "eventCount", "environmentPresentNames", "environmentValuesRecorded"}, "attempt")
    require(attempt["index"] == 0 and attempt["termination"] in {"exited", "timeout", "capture-limit"}, "invalid attempt facts")
    require(attempt["environmentValuesRecorded"] is False, "environment values must never be recorded")
    require(attempt["environmentPresentNames"] == sorted(set(attempt["environmentPresentNames"])), "environment names must be sorted and unique")
    require(
        set(attempt["environmentPresentNames"]).issubset(predeclaration["disclosure"]["environmentNames"]),
        "undeclared environment name",
    )
    workspace = closed(run["workspace"], {"beforeInventory", "afterInventory", "afterInventoryStatus", "sourceSnapshotBeforeIdentitySha256", "sourceSnapshotAfterIdentitySha256", "violations"}, "workspace evidence")
    protocol = protocol_authority(campaign["authorities"]["protocolIdentitySha256"])
    expected_snapshot = protocol["sourceSnapshot"]["manifestIdentitySha256"]
    require(workspace["sourceSnapshotBeforeIdentitySha256"] == expected_snapshot, "source snapshot before identity drift")
    require(workspace["violations"] == sorted(set(workspace["violations"])), "workspace violations must be sorted and unique")
    before_rows = validate_inventory_rows(workspace["beforeInventory"], "workspace before inventory")
    after_rows = validate_inventory_rows(workspace["afterInventory"], "workspace after inventory")
    expected_before = [
        {"path": row["path"], "bytes": row["bytes"], "sha256": row["sha256"]}
        for row in case["inputFiles"]
    ]
    require(before_rows == expected_before, "workspace before inventory drift")
    derived_violations: list[str] = []
    if workspace["sourceSnapshotAfterIdentitySha256"] != expected_snapshot:
        derived_violations.append("repository-invalid")
    require(workspace["afterInventoryStatus"] in {"valid", "invalid"}, "invalid workspace inventory status")
    if workspace["afterInventoryStatus"] == "invalid":
        require(after_rows == [], "invalid workspace inventory must be empty")
        derived_violations.append("workspace-invalid")
    else:
        expected_paths = expected_workspace_paths(case, harness)
        after_by_path = {row["path"]: row for row in after_rows}
        if set(after_by_path) != expected_paths:
            derived_violations.append("workspace-path-drift")
        editable = set(case["editablePaths"])
        before_by_path = {row["path"]: row for row in before_rows}
        if any(before_by_path[path] != after_by_path.get(path) for path in expected_paths - editable):
            derived_violations.append("noneditable-input-drift")
    if attempt["termination"] != "exited":
        derived_violations.append(attempt["termination"])
    if attempt["returnCode"] != 0:
        derived_violations.append("nonzero-exit")
    local_v5 = is_v5_harness(harness) and predeclaration["runner"]["class"] == "codex-cli-local"
    provider_session: dict[str, Any] | None = None
    if local_v5:
        provider_session = genesisbench_mlx_responses.validate_evidence(
            root / "custody/adapter", predeclaration["model"]["requestedId"],
        )
        derived_violations.extend(provider_violations(provider_session))
    score_path = root / "score.json"
    score_doc = load_json(score_path) if score_path.exists() else None
    if score_doc is None:
        require(run["scoreIdentitySha256"] is None and run["outcome"] == "invalid", "scoreless run outcome drift")
        require(not run["replay"]["independentRescoreRequired"], "scoreless run cannot require rescore")
    else:
        require(not workspace["violations"], "scored run contains workspace violations")
        require(run["scoreIdentitySha256"] == sha256_bytes(canonical_bytes(score_doc)), "score binding drift")
        expected_outcome = "verified" if score_doc["validity"]["passed"] else "failed"
        require(run["outcome"] == expected_outcome and run["replay"]["independentRescoreRequired"], "scored run outcome drift")
    require(run["replay"] == {"agentAccessAllowed": False, "modelAccessAllowed": False, "independentRescoreRequired": score_doc is not None}, "replay policy drift")
    artifact_rows = validate_inventory_rows(run["artifactInventory"], "run artifact inventory")
    require(run["artifactInventoryIdentitySha256"] == rows_identity(artifact_rows), "artifact inventory identity drift")
    if check_files:
        actual = inventory(root, max_files=4097, max_bytes=128 * 1024 * 1024)
        actual = [row for row in actual if row["path"] != "run.json"]
        require(actual == artifact_rows, "run artifact inventory drift")
        if score_doc is not None:
            require(
                inventory(root / "candidate", max_files=4096, max_bytes=64 * 1024 * 1024) == after_rows,
                "retained candidate differs from observed workspace",
            )
        observed_root = root / "observed-workspace"
        if is_v4_or_later_harness(harness) and run["outcome"] == "invalid" and workspace["afterInventoryStatus"] == "valid":
            require(observed_root.is_dir() and not observed_root.is_symlink(), "invalid run lacks observed workspace payload")
            require(
                inventory(observed_root, max_files=4096, max_bytes=128 * 1024 * 1024) == after_rows,
                "retained observed workspace differs from recorded inventory",
            )
        else:
            require(not observed_root.exists(), "unexpected observed workspace payload")
        custody_root = root / "custody"
        if local_v5:
            manifest = genesisbench_mlx_custody.validate(load_json(custody_root / "manifest.json"))
            require(manifest["contentIdentitySha256"] == predeclaration["custody"]["manifestIdentitySha256"], "retained custody manifest binding drift")
            require(manifest["model"]["artifactIdentitySha256"] == predeclaration["model"]["artifactSha256"], "retained custody model binding drift")
            require(manifest["adapter"]["identitySha256"] == predeclaration["custody"]["adapterIdentitySha256"], "retained adapter binding drift")
            require(manifest["runtime"]["runtimeIdentitySha256"] == predeclaration["custody"]["runtimeIdentitySha256"], "retained runtime binding drift")
            require((custody_root / "supervisor-stdout.txt").is_file() and (custody_root / "supervisor-stderr.txt").is_file(), "local supervisor evidence is incomplete")
        else:
            require(not custody_root.exists(), "unexpected local custody evidence")
        valid_jsonl, event_count = validate_jsonl(
            root / "events.jsonl", event_line_limit(harness),
        )
        require(valid_jsonl == ("malformed-event-transcript" not in workspace["violations"]), "transcript validity drift")
        require(event_count == attempt["eventCount"], "event count drift")
        if not valid_jsonl:
            derived_violations.append("malformed-event-transcript")
        require(sorted(set(derived_violations)) == workspace["violations"], "workspace violation derivation drift")
    return run


def replay_run(run_path: Path, genesis_bin: Path, selfhost_artifact: Path) -> dict[str, Any]:
    run = validate_run(run_path, check_files=True)
    campaign = validate_campaign(load_json(run_path.parent / "campaign.json"))
    harness = authority(campaign["authorities"]["harnessIdentitySha256"])
    protocol = protocol_authority(campaign["authorities"]["protocolIdentitySha256"])
    suite = suite_authority(campaign["authorities"]["suiteIdentitySha256"])
    genesis_bin = regular_file(genesis_bin, "GenesisCode executable")
    selfhost_artifact = regular_file(selfhost_artifact, "self-host artifact")
    archive_path = run_path.parents[2] / "tools" / "archive.json"
    archive = validate_tool_archive(archive_path, campaign) if archive_path.is_file() else None
    if archive is None and is_v3_or_later_harness(harness):
        require(sha256_file(genesis_bin) == campaign["tools"]["genesisExecutableSha256"], "replay GenesisCode executable digest mismatch")
        require(sha256_file(selfhost_artifact) == campaign["tools"]["selfhostArtifactSha256"], "replay self-host artifact digest mismatch")
    matched = None
    with tempfile.TemporaryDirectory(prefix="genesisbench-replay-tools-") as temporary:
        if archive is not None:
            archive_root = archive_path.parent.parent
            compressed = archive_root / safe_relative(
                archive["genesisExecutable"]["path"], "archived executable path",
            )
            genesis_bin = Path(temporary) / "genesis"
            genesis_bin.write_bytes(gzip.decompress(compressed.read_bytes()))
            genesis_bin.chmod(0o500)
            selfhost_artifact = archive_root / safe_relative(
                archive["selfhostArtifact"]["path"], "archived artifact path",
            )
        if run["replay"]["independentRescoreRequired"]:
            if archive is not None:
                require_compatible_archive_platform(archive)
            rescored = gc_agent_scoring.score_candidate(
                protocol_bound_json(protocol, str(SCORING_PATH.relative_to(ROOT))),
                run["case"]["id"], run_path.parent / "candidate",
                genesis_bin, selfhost_artifact, suite_document=suite,
            )
            matched = rescored == load_json(run_path.parent / "score.json")
            require(matched, "independent Open Agent rescore mismatch")
    return {
        "kind": "genesis/genesisbench-open-agent-replay-v0.1",
        "runIdentitySha256": run["contentIdentitySha256"],
        "agentAccessed": False,
        "modelAccessed": False,
        "allFieldsValidated": True,
        "independentRescoreMatched": matched,
    }


def validate_authorities() -> dict[str, Any]:
    documents = [authority(), *(load_json(path) for path in LEGACY_AUTHORITY_PATHS)]
    require(
        [doc["version"] for doc in documents] == ["0.5.0", "0.4.0", "0.3.0", "0.2.0", "0.1.0"],
        "Open Agent authority order drift",
    )
    for doc in documents:
        fields = {"kind", "version", "purpose", "coldAcquisitionAdapterProfileUnchanged", "scaffoldIdentitySha256", "invocationProfiles", "limits", "securityControls", "contentIdentitySha256"}
        if is_v4_or_later_harness(doc):
            fields.add("implementation")
        if is_v5_harness(doc):
            fields.add("localExecution")
        closed(doc, fields, "Open Agent authority")
        require((doc["kind"], doc["version"]) in {
            (KIND_AUTHORITY, "0.1.0"), (KIND_AUTHORITY_V2, "0.2.0"),
            (KIND_AUTHORITY_V3, "0.3.0"), (KIND_AUTHORITY_V4, "0.4.0"),
            (KIND_AUTHORITY_V5, "0.5.0"),
        }, "Open Agent authority kind/version drift")
        require(doc["coldAcquisitionAdapterProfileUnchanged"] is True, "Open Agent authority silently broadens Cold Acquisition adapters")
        require(set(doc["invocationProfiles"]) == {"codex-cli-hosted", "codex-cli-local"}, "Open Agent invocation profile drift")
        require(doc["securityControls"] == sorted(set(doc["securityControls"])), "security controls must be sorted and unique")
        if is_v3_or_later_harness(doc):
            require(doc["limits"]["maxEventLineBytes"] == MAX_CAPTURE_BYTES, "v0.3+ event line limit drift")
        if is_v4_or_later_harness(doc):
            validate_implementation_binding(doc["implementation"], check_files=is_v5_harness(doc))
        if is_v4_harness(doc):
            require(doc["scaffoldIdentitySha256"] == v4_scaffold_identity(doc), "v0.4 scaffold identity drift")
        if is_v5_harness(doc):
            require(doc["localExecution"] == v5_local_execution(), "v0.5 local execution authority drift")
            require(doc["scaffoldIdentitySha256"] == v5_scaffold_identity(doc), "v0.5 scaffold identity drift")
            require(doc == render_v5_authority(), "v0.5 rendered authority is stale")
        validate_identity(doc)
    for path in (CAMPAIGN_SCHEMA_PATH, PREDECLARATION_SCHEMA_PATH, RUN_SCHEMA_PATH, TOOL_ARCHIVE_SCHEMA_PATH):
        schema = load_json(path)
        require(schema["$schema"] == "https://json-schema.org/draft/2020-12/schema", "Open Agent schema draft drift")
        require(schema["additionalProperties"] is False, "Open Agent schema must be closed")
    return documents[0]


def self_test() -> int:
    controls = 0
    with tempfile.TemporaryDirectory(prefix="genesisbench-open-agent-controls-") as raw:
        temp = Path(raw)
        fixture = temp / "fixture.py"
        fixture.write_text(
            "#!/usr/bin/python3\n"
            "import sys\n"
            "if '--version' in sys.argv:\n"
            " print('codex-cli 0.0.0-fixture')\n"
            " raise SystemExit(0)\n"
            "print('{\"type\":\"fixture\"}')\n",
            encoding="ascii",
        )
        fixture.chmod(0o755)
        suite = load_json(SUITE_PATH)
        campaign = build_campaign(
            campaign_id="conformance-v0.1", phase="reality-gate",
            case_ids=campaign_case_ids(suite, "reality-gate"),
            runner_class="codex-cli-hosted", executable=fixture,
            model_id="fixture", model_revision="fixture-revision", immutable_revision=False,
            reasoning_effort="xhigh", timeout_ms=1_000, local_provider=None,
            model_artifact_sha256=None, hardware_class="fixture-host",
            genesis_bin=fixture, selfhost_artifact=fixture,
        )
        validate_campaign(campaign)

        implementation = copy.deepcopy(authority()["implementation"])
        implementation["files"][0]["sha256"] = "0" * 64
        implementation["identitySha256"] = sha256_bytes(canonical_bytes(implementation["files"]))
        try:
            validate_implementation_binding(implementation, check_files=True)
        except OpenAgentError:
            controls += 1
        else:
            raise OpenAgentError("implementation byte substitution accepted")

        implementation = copy.deepcopy(authority()["implementation"])
        implementation["files"].pop()
        implementation["identitySha256"] = sha256_bytes(canonical_bytes(implementation["files"]))
        try:
            validate_implementation_binding(implementation, check_files=True)
        except OpenAgentError:
            controls += 1
        else:
            raise OpenAgentError("incomplete implementation closure accepted")

        campaign_mutations = [
            (lambda d: d["cases"].pop(), True),
            (lambda d: d["model"].__setitem__("requestedId", "post-hoc-model"), False),
            (lambda d: d["stopPolicy"].__setitem__("stopOnModelFailure", True), True),
            (lambda d: d["publication"].__setitem__("expectedAttemptCount", 8), True),
            (lambda d: d.__setitem__("contentIdentitySha256", "0" * 64), False),
        ]
        for mutate, reseal in campaign_mutations:
            candidate = copy.deepcopy(campaign)
            mutate(candidate)
            if reseal:
                candidate = identified(candidate)
            try:
                validate_campaign(candidate)
            except (OpenAgentError, KeyError, TypeError):
                controls += 1
            else:
                raise OpenAgentError("negative campaign control accepted")

        baseline = build_predeclaration(case_id="completion-small", campaign=campaign)
        validate_predeclaration(baseline, campaign)

        custody_path = temp / "custody.json"
        custody = genesisbench_mlx_custody.capture_fixture()
        write_json(custody_path, custody)
        local_campaign = build_campaign(
            campaign_id="local-conformance-v0.1", phase="reality-gate",
            case_ids=campaign_case_ids(suite, "reality-gate"),
            runner_class="codex-cli-local", executable=fixture,
            model_id=custody["model"]["id"], model_revision=custody["model"]["revision"],
            immutable_revision=True, reasoning_effort="xhigh", timeout_ms=1_000,
            local_provider="mlx-responses",
            model_artifact_sha256=custody["model"]["artifactIdentitySha256"],
            hardware_class="fixture-apple-silicon", genesis_bin=fixture,
            selfhost_artifact=fixture, custody_manifest=custody_path,
        )
        local_predeclaration = build_predeclaration(case_id="completion-small", campaign=local_campaign)
        validate_predeclaration(local_predeclaration, local_campaign)
        local_mutations = (
            lambda d: d["custody"].__setitem__("manifestIdentitySha256", "0" * 64),
            lambda d: d["runner"].__setitem__("localProvider", "ollama"),
            lambda d: d["disclosure"].__setitem__("secretSource", "ambient-env"),
        )
        for mutate in local_mutations:
            candidate = copy.deepcopy(local_predeclaration); mutate(candidate); candidate = identified(candidate)
            try:
                validate_predeclaration(candidate, local_campaign)
            except (OpenAgentError, KeyError, TypeError):
                controls += 1
            else:
                raise OpenAgentError("negative local predeclaration control accepted")
        require(provider_violations({"backendRequestCount": 0, "rejections": [], "authorizationHeadersObserved": False, "hiddenRetriesObserved": False}) == ["provider-no-request"], "zero-request provider violation drift")
        require(provider_violations({"backendRequestCount": 1, "rejections": [{}], "authorizationHeadersObserved": False, "hiddenRetriesObserved": False}) == ["provider-policy-rejection"], "provider rejection violation drift")
        controls += 2

        _, output_case = suite_and_case("generation-small", suite)
        output_workspace = temp / "output-workspace"
        materialize_case(output_case, output_workspace)
        (output_workspace / "main.gc").write_text("42\n", encoding="ascii")
        output_rows = inventory(output_workspace, max_files=16, max_bytes=1024)
        require(
            {row["path"] for row in output_rows} == expected_workspace_paths(output_case, authority()),
            "declared editable output was not admitted",
        )
        output_candidate = temp / "output-candidate"
        copy_candidate(output_workspace, output_candidate, output_case, authority())
        require((output_candidate / "main.gc").read_bytes() == b"42\n", "declared output was not retained")
        controls += 1

        _, invalid_case = suite_and_case("deployment-small", suite)
        invalid_workspace = temp / "invalid-workspace"
        materialize_case(invalid_case, invalid_workspace)
        (invalid_workspace / "deployment.json").write_text("{}\n", encoding="ascii")
        with (invalid_workspace / "package.toml").open("a", encoding="ascii") as stream:
            stream.write("# drift\n")
        invalid_rows = inventory(invalid_workspace, max_files=16, max_bytes=4096)
        observed = temp / "observed-workspace"
        copy_inventory_payload(invalid_workspace, observed, invalid_rows)
        require(
            inventory(observed, max_files=16, max_bytes=4096) == invalid_rows,
            "invalid workspace payload was not retained",
        )
        (observed / "package.toml").write_text("tampered\n", encoding="ascii")
        require(
            inventory(observed, max_files=16, max_bytes=4096) != invalid_rows,
            "observed workspace tamper was not detected",
        )
        controls += 2

        archived_protocol_path = (
            AUTHORITY_ARCHIVE_ROOT / "protocols" /
            "b3aee80131ab951586bab2404e8ef52a774d3c8fe64346b0b25511014f6fbe6b.json"
        )
        if archived_protocol_path.is_file():
            archived_scoring = protocol_bound_json(
                load_json(archived_protocol_path), str(SCORING_PATH.relative_to(ROOT)),
            )
            require(
                archived_scoring["contentIdentitySha256"] ==
                "5313abe263de08d2c2a5d1d1294c60fdc0cbe6e08554b4876b084c65f2958b87",
                "historical scoring authority resolution drift",
            )
            controls += 1

        mutations = [
            lambda d: d["attemptPolicy"].__setitem__("attempts", 2),
            lambda d: d["attemptPolicy"].__setitem__("hiddenRetriesAllowed", True),
            lambda d: d["capabilities"].__setitem__("additionalWritableRoots", ["ambient"]),
            lambda d: d["capabilities"].__setitem__("network", "unrestricted"),
            lambda d: d["runner"].__setitem__("executableSha256", "forged"),
            lambda d: d["model"].__setitem__("artifactSha256", "0" * 64),
            lambda d: d["tools"].__setitem__("genesisExecutableSha256", "0" * 64),
            lambda d: d["track"].__setitem__("rankEligible", True),
            lambda d: d.__setitem__("contentIdentitySha256", "0" * 64),
        ]
        for mutate in mutations:
            candidate = copy.deepcopy(baseline)
            mutate(candidate)
            if candidate["contentIdentitySha256"] != "0" * 64:
                candidate = identified(candidate)
            try:
                validate_predeclaration(candidate, campaign)
            except (OpenAgentError, KeyError, TypeError):
                controls += 1
            else:
                raise OpenAgentError("negative predeclaration control accepted")

        repository = temp / "repository"
        rows, identity = archive_snapshot(repository)
        require(validate_snapshot_root(repository, rows) == identity, "valid snapshot rejected")
        target_row = next(row for row in rows if row["mode"] == "100644")
        target = repository / target_row["path"]
        original = target.read_bytes()

        target.chmod(0o644); target.write_bytes(original + b"x")
        try:
            validate_snapshot_root(repository, rows)
        except OpenAgentError:
            controls += 1
        else:
            raise OpenAgentError("snapshot byte mutation accepted")
        target.write_bytes(original); target.chmod(0o444)

        target.chmod(0o644)
        try:
            validate_snapshot_root(repository, rows)
        except OpenAgentError:
            controls += 1
        else:
            raise OpenAgentError("snapshot mode mutation accepted")
        target.chmod(0o444)

        target.parent.chmod(0o755); target.chmod(0o644); target.unlink(); target.symlink_to("missing"); target.parent.chmod(0o555)
        try:
            validate_snapshot_root(repository, rows)
        except OpenAgentError:
            controls += 1
        else:
            raise OpenAgentError("snapshot symlink mutation accepted")
        target.parent.chmod(0o755); target.unlink(); target.write_bytes(original); target.chmod(0o444); target.parent.chmod(0o555)

        extra = repository / "undeclared-empty-directory"
        repository.chmod(0o755); extra.mkdir(); repository.chmod(0o555)
        try:
            validate_snapshot_root(repository, rows)
        except OpenAgentError:
            controls += 1
        else:
            raise OpenAgentError("snapshot topology mutation accepted")
        repository.chmod(0o755); extra.rmdir(); repository.chmod(0o555)

        valid_events = temp / "valid.jsonl"; valid_events.write_text('{"type":"fixture"}\n', encoding="ascii")
        malformed_events = temp / "malformed.jsonl"; malformed_events.write_text('not-json\n', encoding="ascii")
        require(validate_jsonl(valid_events) == (True, 1), "valid event transcript rejected")
        require(validate_jsonl(malformed_events)[0] is False, "malformed event transcript accepted")
        controls += 1

        large_events = temp / "large.jsonl"
        large_events.write_bytes(canonical_bytes({"payload": "x" * (1024 * 1024)}) + b"\n")
        require(validate_jsonl(large_events)[0] is False, "historical event limit accepted oversized line")
        require(validate_jsonl(large_events, MAX_CAPTURE_BYTES) == (True, 1), "v0.3 event limit rejected bounded line")
        controls += 1

        environment, present = sanitized_environment(authority())
        require(environment["GENESIS_SELFHOST_COMPILED_CACHE_DISABLE"] == "1", "Genesis cache was not disabled")
        require(set(V3_FIXED_ENVIRONMENT).issubset(present), "fixed environment disclosure drift")
        controls += 1

        retained = temp / "retained-paths.jsonl"
        retained.write_text(json.dumps({"cwd": str(temp)}) + "\n", encoding="ascii")
        normalize_retained_stage_paths((retained,), temp)
        require(str(temp).encode("utf-8") not in retained.read_bytes(), "ephemeral stage path leaked")
        require(validate_jsonl(retained) == (True, 1), "path normalization damaged JSONL")
        controls += 1

        marker = temp / "descendant-survived"
        timeout_fixture = temp / "timeout.py"
        timeout_fixture.write_text(
            "#!/usr/bin/python3\n"
            "import subprocess,sys,time\n"
            "if '--version' in sys.argv:\n"
            " print('codex-cli 0.0.0-fixture')\n"
            " raise SystemExit(0)\n"
            f"subprocess.Popen(['/usr/bin/python3','-c',\"import pathlib,time;time.sleep(1);pathlib.Path({str(marker)!r}).write_text('alive')\"])\n"
            "time.sleep(30)\n",
            encoding="ascii",
        )
        timeout_fixture.chmod(0o755)
        stdout = temp / "timeout.stdout"; stderr = temp / "timeout.stderr"
        return_code, reason, _, _ = run_process(
            timeout_fixture, [], temp, 200, stdout, stderr, 1024, 1024, authority(),
        )
        require(reason == "timeout" and return_code is not None, "timeout fixture did not terminate")
        time.sleep(1.2)
        require(not marker.exists(), "timeout descendant survived process-group kill")
        controls += 1

        provider_marker = temp / "provider-descendant-survived"
        provider_fixture = temp / "provider.py"
        provider_fixture.write_text(
            "#!/usr/bin/python3\n"
            "import subprocess,time\n"
            f"subprocess.Popen(['/usr/bin/python3','-c',\"import pathlib,time;time.sleep(1);pathlib.Path({str(provider_marker)!r}).write_text('alive')\"])\n"
            "time.sleep(30)\n",
            encoding="ascii",
        )
        provider_fixture.chmod(0o755)
        supervised = subprocess.Popen([str(provider_fixture)], cwd=temp, start_new_session=True)
        time.sleep(0.2); terminate_supervised_group(supervised); time.sleep(1.2)
        require(supervised.poll() is not None and not provider_marker.exists(), "supervised provider descendant survived termination")
        controls += 1

        readonly = temp / "readonly-cleanup"
        (readonly / "nested").mkdir(parents=True)
        (readonly / "nested" / "payload").write_text("fixture", encoding="ascii")
        external = temp / "external-cleanup-target"; external.mkdir(); external.chmod(0o500)
        (readonly / "nested" / "external-link").symlink_to(external, target_is_directory=True)
        (readonly / "nested" / "payload").chmod(0o444)
        (readonly / "nested").chmod(0o555); readonly.chmod(0o555)
        remove_tree(readonly)
        require(not readonly.exists(), "read-only snapshot cleanup leaked")
        require(stat.S_IMODE(external.stat().st_mode) == 0o500, "cleanup followed an external symlink")
        external.chmod(0o700); external.rmdir()
        stage = isolated_stage(); remove_tree(stage)
        require(not stage.exists(), "ancestry-isolated stage cleanup leaked")
        controls += 3
    return controls


def parser() -> argparse.ArgumentParser:
    out = argparse.ArgumentParser(description=__doc__)
    modes = out.add_subparsers(dest="command", required=True)
    check = modes.add_parser("check"); check.add_argument("--self-test", action="store_true")
    modes.add_parser("render-authority")
    campaign = modes.add_parser("campaign-plan")
    campaign.add_argument("--campaign", required=True); campaign.add_argument("--phase", required=True, choices=["reality-gate", "full-public"])
    campaign.add_argument("--case", required=True, action="append"); campaign.add_argument("--runner", required=True, choices=["codex-cli-hosted", "codex-cli-local"])
    campaign.add_argument("--agent-executable", required=True, type=Path); campaign.add_argument("--model", required=True)
    campaign.add_argument("--model-revision", required=True); campaign.add_argument("--immutable-revision", action="store_true")
    campaign.add_argument("--reasoning-effort", default="xhigh", choices=["low", "medium", "high", "xhigh"])
    campaign.add_argument("--timeout-ms", type=int, default=900_000); campaign.add_argument("--local-provider", choices=["mlx-responses"])
    campaign.add_argument("--model-artifact-sha256"); campaign.add_argument("--hardware-class", required=True); campaign.add_argument("--out", required=True, type=Path)
    campaign.add_argument("--genesis-bin", required=True, type=Path); campaign.add_argument("--selfhost-artifact", required=True, type=Path)
    campaign.add_argument("--local-custody-manifest", type=Path)
    plan = modes.add_parser("plan"); plan.add_argument("--case", required=True)
    plan.add_argument("--campaign-predeclaration", required=True, type=Path); plan.add_argument("--out", required=True, type=Path)
    run = modes.add_parser("run"); run.add_argument("--campaign-predeclaration", required=True, type=Path); run.add_argument("--predeclaration", required=True, type=Path); run.add_argument("--out", required=True, type=Path)
    run.add_argument("--agent-executable", required=True, type=Path); run.add_argument("--genesis-bin", required=True, type=Path); run.add_argument("--selfhost-artifact", required=True, type=Path)
    run.add_argument("--local-custody-manifest", type=Path); run.add_argument("--local-model-root", type=Path); run.add_argument("--local-python", type=Path)
    validate = modes.add_parser("validate"); validate.add_argument("--run", required=True, type=Path)
    replay = modes.add_parser("replay"); replay.add_argument("--run", required=True, type=Path); replay.add_argument("--genesis-bin", required=True, type=Path); replay.add_argument("--selfhost-artifact", required=True, type=Path)
    return out


def main() -> int:
    args = parser().parse_args()
    if args.command == "check":
        doc = validate_authorities()
        controls = self_test() if args.self_test else 0
        result = {"kind": "genesis/genesisbench-open-agent-check-v0.1", "authorityIdentitySha256": doc["contentIdentitySha256"], "controls": controls}
    elif args.command == "render-authority":
        result = render_v5_authority()
    elif args.command == "campaign-plan":
        require(not args.out.exists(), "predeclaration output already exists")
        result = build_campaign(
            campaign_id=args.campaign, phase=args.phase, case_ids=args.case, runner_class=args.runner,
            executable=args.agent_executable, model_id=args.model, model_revision=args.model_revision,
            immutable_revision=args.immutable_revision, reasoning_effort=args.reasoning_effort,
            timeout_ms=args.timeout_ms, local_provider=args.local_provider,
            model_artifact_sha256=args.model_artifact_sha256, hardware_class=args.hardware_class,
            genesis_bin=args.genesis_bin, selfhost_artifact=args.selfhost_artifact,
            custody_manifest=args.local_custody_manifest,
        )
        write_json(args.out, result)
    elif args.command == "plan":
        require(not args.out.exists(), "predeclaration output already exists")
        campaign = validate_campaign(load_json(args.campaign_predeclaration))
        result = build_predeclaration(case_id=args.case, campaign=campaign)
        write_json(args.out, result)
    elif args.command == "run":
        result = run_agent(
            args.campaign_predeclaration, args.predeclaration, args.out, args.agent_executable,
            args.genesis_bin, args.selfhost_artifact, local_custody_manifest=args.local_custody_manifest,
            local_model_root=args.local_model_root, local_python=args.local_python,
        )
    elif args.command == "validate":
        run = validate_run(args.run, check_files=True)
        result = {"kind": "genesis/genesisbench-open-agent-validation-v0.1", "valid": True, "runIdentitySha256": run["contentIdentitySha256"], "outcome": run["outcome"]}
    else:
        result = replay_run(args.run, args.genesis_bin, args.selfhost_artifact)
    sys.stdout.buffer.write(pretty_bytes(result))
    return 0


if __name__ == "__main__":
    try:
        raise SystemExit(main())
    except (OpenAgentError, OSError, UnicodeError, json.JSONDecodeError, KeyError, ValueError, tarfile.TarError) as exc:
        sys.stderr.buffer.write(pretty_bytes({"kind": "genesis/genesisbench-open-agent-error-v0.1", "code": "bench/open-agent-failed", "message": str(exc)}))
        raise SystemExit(1)
