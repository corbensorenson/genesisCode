#!/usr/bin/env python3
"""Capability-minimal, replayable GenesisBench Open Agent harness."""

from __future__ import annotations

import argparse
import copy
import hashlib
import json
import os
import re
import shutil
import signal
import stat
import subprocess
import sys
import tarfile
import tempfile
import time
from pathlib import Path, PurePosixPath
from typing import Any

import gc_agent_scoring
import genesisbench_protocol


ROOT = Path(__file__).resolve().parents[2]
AUTHORITY_PATH = ROOT / "docs/spec/GENESISBENCH_OPEN_AGENT_v0.1.json"
PREDECLARATION_SCHEMA_PATH = ROOT / "docs/spec/GENESISBENCH_OPEN_AGENT_PREDECLARATION_v0.1.schema.json"
RUN_SCHEMA_PATH = ROOT / "docs/spec/GENESISBENCH_OPEN_AGENT_RUN_v0.1.schema.json"
SUITE_PATH = ROOT / "benchmarks/agent_tasks/v0.1/suite.json"
PROTOCOL_PATH = ROOT / "docs/spec/GENESISBENCH_PROTOCOL_v0.1.json"
SCORING_PATH = ROOT / "docs/spec/GC_AGENT_BENCHMARK_SCORING_v0.1.json"

KIND_PREDECLARATION = "genesis/genesisbench-open-agent-predeclaration-v0.1"
KIND_RUN = "genesis/genesisbench-open-agent-run-v0.1"
KIND_AUTHORITY = "genesis/genesisbench-open-agent-harness-v0.1"
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


def suite_and_case(case_id: str) -> tuple[dict[str, Any], dict[str, Any]]:
    suite = load_json(SUITE_PATH)
    case = next((row for row in suite["cases"] if row["id"] == case_id), None)
    require(case is not None, f"unknown benchmark case: {case_id}")
    return suite, case


def authority() -> dict[str, Any]:
    return load_json(AUTHORITY_PATH)


def source_rows() -> list[dict[str, Any]]:
    return genesisbench_protocol.tree_rows(genesisbench_protocol.SNAPSHOT_COMMIT)


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


def build_predeclaration(
    *, case_id: str, campaign_id: str, runner_class: str, executable: Path,
    model_id: str, model_revision: str, immutable_revision: bool,
    reasoning_effort: str, timeout_ms: int, local_provider: str | None,
    model_artifact_sha256: str | None,
) -> dict[str, Any]:
    executable = regular_file(executable, "agent executable")
    suite, case = suite_and_case(case_id)
    protocol = load_json(PROTOCOL_PATH)
    harness = authority()
    require(runner_class in {"codex-cli-hosted", "codex-cli-local"}, "unsupported Open Agent runner class")
    require(reasoning_effort in {"low", "medium", "high", "xhigh"}, "unsupported reasoning effort")
    require(1_000 <= timeout_ms <= harness["limits"]["maxTimeoutMs"], "timeout is outside harness limits")
    if runner_class == "codex-cli-local":
        require(local_provider in {"lmstudio", "ollama"}, "local runner requires a supported provider")
        require(model_artifact_sha256 is not None and SHA_RE.fullmatch(model_artifact_sha256) is not None, "local runner requires a model artifact digest")
        network_mode = "loopback-provider-only"
    else:
        require(local_provider is None and model_artifact_sha256 is None, "hosted runner cannot declare local provider material")
        network_mode = "provider-only"
    document = {
        "kind": KIND_PREDECLARATION,
        "version": "0.1.0",
        "campaignId": safe_id(campaign_id, "campaign id"),
        "case": case_binding(case),
        "authorities": {
            "harnessIdentitySha256": harness["contentIdentitySha256"],
            "protocolIdentitySha256": protocol["contentIdentitySha256"],
            "suiteIdentitySha256": suite["contentIdentitySha256"],
            "sourceSnapshotManifestIdentitySha256": protocol["sourceSnapshot"]["manifestIdentitySha256"],
        },
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
            "environmentNames": list(ALLOWED_ENVIRONMENT),
            "environmentValuesRecorded": False,
            "genesisSpecificTraining": "unknown",
            "contaminationLabel": "unknown",
        },
        "contentIdentitySha256": "",
    }
    return identified(document)


def validate_predeclaration(document: Any) -> dict[str, Any]:
    top = {
        "kind", "version", "campaignId", "case", "authorities", "track", "runner",
        "model", "attemptPolicy", "capabilities", "limits", "disclosure", "contentIdentitySha256",
    }
    doc = closed(document, top, "Open Agent predeclaration")
    require(doc["kind"] == KIND_PREDECLARATION and doc["version"] == "0.1.0", "predeclaration kind/version drift")
    safe_id(doc["campaignId"], "campaign id")
    suite, case = suite_and_case(doc["case"].get("id"))
    require(doc["case"] == case_binding(case), "case binding drift")
    protocol = load_json(PROTOCOL_PATH)
    harness = authority()
    require(doc["authorities"] == {
        "harnessIdentitySha256": harness["contentIdentitySha256"],
        "protocolIdentitySha256": protocol["contentIdentitySha256"],
        "suiteIdentitySha256": suite["contentIdentitySha256"],
        "sourceSnapshotManifestIdentitySha256": protocol["sourceSnapshot"]["manifestIdentitySha256"],
    }, "predeclaration authority binding drift")
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
        require(runner["localProvider"] in {"lmstudio", "ollama"}, "invalid local provider")
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
        "environmentNames": list(ALLOWED_ENVIRONMENT), "environmentValuesRecorded": False,
        "genesisSpecificTraining": "unknown", "contaminationLabel": "unknown",
    }, "disclosure drift")
    validate_identity(doc)
    return doc


def archive_snapshot(destination: Path) -> tuple[list[dict[str, Any]], str]:
    rows = source_rows()
    destination.mkdir(parents=True)
    archive = subprocess.run(
        ["git", "archive", "--format=tar", genesisbench_protocol.SNAPSHOT_COMMIT],
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


def invocation(predeclaration: dict[str, Any], prompt: str) -> list[str]:
    runner = predeclaration["runner"]
    model = predeclaration["model"]
    args = [
        "exec", "--ignore-user-config", "--strict-config", "--ephemeral", "--json",
        "--color", "never", "--sandbox", "workspace-write", "--skip-git-repo-check",
        "--model", model["requestedId"], "-c", f'model_reasoning_effort="{model["reasoningEffort"]}"',
        "-c", 'approval_policy="never"',
    ]
    if runner["class"] == "codex-cli-local":
        args.extend(["--oss", "--local-provider", runner["localProvider"]])
    args.append(prompt)
    return args


def sanitized_environment() -> tuple[dict[str, str], list[str]]:
    environment: dict[str, str] = {}
    present = []
    for name in ALLOWED_ENVIRONMENT:
        value = os.environ.get(name)
        if value is not None:
            environment[name] = value
            present.append(name)
    environment.setdefault("PATH", "/usr/bin:/bin:/usr/sbin:/sbin")
    environment["NO_COLOR"] = "1"
    return environment, present


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


def run_process(
    executable: Path, args: list[str], cwd: Path, timeout_ms: int,
    stdout_path: Path, stderr_path: Path, max_stdout: int, max_stderr: int,
) -> tuple[int | None, str, int, list[str]]:
    environment, present = sanitized_environment()
    started = time.monotonic_ns()
    with stdout_path.open("wb") as stdout, stderr_path.open("wb") as stderr:
        process = subprocess.Popen(
            [str(executable), *args], cwd=cwd, env=environment, stdin=subprocess.DEVNULL,
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


def validate_jsonl(path: Path) -> tuple[bool, int]:
    count = 0
    try:
        with path.open("rb") as stream:
            for raw in stream:
                require(len(raw) <= 1024 * 1024, "agent event exceeds line limit")
                if not raw.strip():
                    continue
                value = json.loads(raw)
                require(isinstance(value, dict), "agent event must be an object")
                count += 1
                require(count <= 100_000, "agent event count exceeds limit")
    except (UnicodeDecodeError, json.JSONDecodeError, OpenAgentError):
        return False, count
    return count > 0, count


def copy_candidate(workspace: Path, candidate: Path, case: dict[str, Any]) -> None:
    candidate.mkdir()
    for row in case["inputFiles"]:
        relative = safe_relative(row["path"], "candidate path")
        source = workspace.joinpath(*relative.parts)
        require(source.is_file() and not source.is_symlink(), "candidate file is unavailable")
        target = candidate.joinpath(*relative.parts)
        target.parent.mkdir(parents=True, exist_ok=True)
        shutil.copyfile(source, target)


def run_agent(
    predeclaration_path: Path, out: Path, executable: Path,
    genesis_bin: Path, selfhost_artifact: Path,
) -> dict[str, Any]:
    require(not out.exists(), "Open Agent run output already exists")
    predeclaration = validate_predeclaration(load_json(predeclaration_path))
    executable = regular_file(executable, "agent executable")
    genesis_bin = regular_file(genesis_bin, "GenesisCode executable")
    selfhost_artifact = regular_file(selfhost_artifact, "self-host artifact")
    require(sha256_file(executable) == predeclaration["runner"]["executableSha256"], "agent executable digest mismatch")
    require(executable_version(executable) == predeclaration["runner"]["executableVersion"], "agent executable version mismatch")
    _, case = suite_and_case(predeclaration["case"]["id"])
    out.parent.mkdir(parents=True, exist_ok=True)
    stage = Path(tempfile.mkdtemp(prefix="genesisbench-open-agent-", dir=out.parent.resolve()))
    retained = stage / "retained"; retained.mkdir()
    input_root = stage / "input"; input_root.mkdir()
    repository = input_root / "repository"
    workspace = stage / "workspace"
    tools = input_root / "tools"; tools.mkdir()
    try:
        snapshot_rows, snapshot_identity = archive_snapshot(repository)
        before_workspace = materialize_case(case, workspace)
        shutil.copyfile(genesis_bin, tools / "genesis"); (tools / "genesis").chmod(0o555)
        shutil.copyfile(selfhost_artifact, tools / "toolchain.gc"); (tools / "toolchain.gc").chmod(0o444)
        tools.chmod(0o555); input_root.chmod(0o555)
        write_json(retained / "predeclaration.json", predeclaration)
        stdout_path = retained / "events.jsonl"; stderr_path = retained / "stderr.txt"
        args = invocation(predeclaration, prompt_for(case))
        return_code, termination, elapsed_ms, present_names = run_process(
            executable, args, workspace, predeclaration["limits"]["timeoutMs"],
            stdout_path, stderr_path, predeclaration["limits"]["maxStdoutBytes"],
            predeclaration["limits"]["maxStderrBytes"],
        )
        violations: list[str] = []
        try:
            repository_identity = validate_snapshot_root(repository, snapshot_rows)
        except OpenAgentError:
            repository_identity = None; violations.append("repository-invalid")
        try:
            after_workspace = inventory(
                workspace, max_files=predeclaration["limits"]["maxWorkspaceFiles"],
                max_bytes=predeclaration["limits"]["maxWorkspaceBytes"],
            )
            expected_paths = {row["path"] for row in case["inputFiles"]}
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
        transcript_valid, event_count = validate_jsonl(stdout_path)
        if not transcript_valid:
            violations.append("malformed-event-transcript")
        if termination != "exited":
            violations.append(termination)
        if return_code != 0:
            violations.append("nonzero-exit")
        violations = sorted(set(violations))
        score_doc = None
        if not violations:
            copy_candidate(workspace, retained / "candidate", case)
            score_doc = gc_agent_scoring.score_candidate(
                load_json(SCORING_PATH), case["id"], retained / "candidate", genesis_bin, selfhost_artifact,
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
        retained.rename(out)
        return run
    except BaseException:
        shutil.rmtree(stage, ignore_errors=True)
        raise
    finally:
        if stage.exists():
            shutil.rmtree(stage, ignore_errors=True)


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
    predeclaration = validate_predeclaration(load_json(root / "predeclaration.json"))
    require(run["predeclarationIdentitySha256"] == predeclaration["contentIdentitySha256"], "run predeclaration binding drift")
    _, case = suite_and_case(run["case"].get("id"))
    require(run["case"] == case_binding(case) == predeclaration["case"], "run case binding drift")
    attempt = closed(run["attempt"], {"index", "returnCode", "termination", "elapsedMs", "eventCount", "environmentPresentNames", "environmentValuesRecorded"}, "attempt")
    require(attempt["index"] == 0 and attempt["termination"] in {"exited", "timeout", "capture-limit"}, "invalid attempt facts")
    require(attempt["environmentValuesRecorded"] is False, "environment values must never be recorded")
    require(attempt["environmentPresentNames"] == sorted(set(attempt["environmentPresentNames"])), "environment names must be sorted and unique")
    require(set(attempt["environmentPresentNames"]).issubset(ALLOWED_ENVIRONMENT), "undeclared environment name")
    workspace = closed(run["workspace"], {"beforeInventory", "afterInventory", "afterInventoryStatus", "sourceSnapshotBeforeIdentitySha256", "sourceSnapshotAfterIdentitySha256", "violations"}, "workspace evidence")
    protocol = load_json(PROTOCOL_PATH)
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
        expected_paths = {row["path"] for row in expected_before}
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
        valid_jsonl, event_count = validate_jsonl(root / "events.jsonl")
        require(valid_jsonl == ("malformed-event-transcript" not in workspace["violations"]), "transcript validity drift")
        require(event_count == attempt["eventCount"], "event count drift")
        if not valid_jsonl:
            derived_violations.append("malformed-event-transcript")
        require(sorted(set(derived_violations)) == workspace["violations"], "workspace violation derivation drift")
    return run


def replay_run(run_path: Path, genesis_bin: Path, selfhost_artifact: Path) -> dict[str, Any]:
    run = validate_run(run_path, check_files=True)
    matched = None
    if run["replay"]["independentRescoreRequired"]:
        rescored = gc_agent_scoring.score_candidate(
            load_json(SCORING_PATH), run["case"]["id"], run_path.parent / "candidate",
            regular_file(genesis_bin, "GenesisCode executable"), regular_file(selfhost_artifact, "self-host artifact"),
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
    doc = authority()
    closed(doc, {"kind", "version", "purpose", "coldAcquisitionAdapterProfileUnchanged", "scaffoldIdentitySha256", "invocationProfiles", "limits", "securityControls", "contentIdentitySha256"}, "Open Agent authority")
    require(doc["kind"] == KIND_AUTHORITY and doc["version"] == "0.1.0", "Open Agent authority kind/version drift")
    require(doc["coldAcquisitionAdapterProfileUnchanged"] is True, "Open Agent authority silently broadens Cold Acquisition adapters")
    require(set(doc["invocationProfiles"]) == {"codex-cli-hosted", "codex-cli-local"}, "Open Agent invocation profile drift")
    require(doc["securityControls"] == sorted(set(doc["securityControls"])), "security controls must be sorted and unique")
    validate_identity(doc)
    for path in (PREDECLARATION_SCHEMA_PATH, RUN_SCHEMA_PATH):
        schema = load_json(path)
        require(schema["$schema"] == "https://json-schema.org/draft/2020-12/schema", "Open Agent schema draft drift")
        require(schema["additionalProperties"] is False, "Open Agent schema must be closed")
    return doc


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
        baseline = build_predeclaration(
            case_id="completion-small", campaign_id="conformance-v0.1",
            runner_class="codex-cli-hosted", executable=fixture,
            model_id="fixture", model_revision="fixture-revision", immutable_revision=False,
            reasoning_effort="xhigh", timeout_ms=1_000, local_provider=None,
            model_artifact_sha256=None,
        )
        validate_predeclaration(baseline)

        mutations = [
            lambda d: d["attemptPolicy"].__setitem__("attempts", 2),
            lambda d: d["attemptPolicy"].__setitem__("hiddenRetriesAllowed", True),
            lambda d: d["capabilities"].__setitem__("additionalWritableRoots", ["ambient"]),
            lambda d: d["capabilities"].__setitem__("network", "unrestricted"),
            lambda d: d["runner"].__setitem__("executableSha256", "forged"),
            lambda d: d["model"].__setitem__("artifactSha256", "0" * 64),
            lambda d: d["track"].__setitem__("rankEligible", True),
            lambda d: d.__setitem__("contentIdentitySha256", "0" * 64),
        ]
        for mutate in mutations:
            candidate = copy.deepcopy(baseline)
            mutate(candidate)
            if candidate["contentIdentitySha256"] != "0" * 64:
                candidate = identified(candidate)
            try:
                validate_predeclaration(candidate)
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
            timeout_fixture, [], temp, 200, stdout, stderr, 1024, 1024,
        )
        require(reason == "timeout" and return_code is not None, "timeout fixture did not terminate")
        time.sleep(1.2)
        require(not marker.exists(), "timeout descendant survived process-group kill")
        controls += 1
    return controls


def parser() -> argparse.ArgumentParser:
    out = argparse.ArgumentParser(description=__doc__)
    modes = out.add_subparsers(dest="command", required=True)
    check = modes.add_parser("check"); check.add_argument("--self-test", action="store_true")
    plan = modes.add_parser("plan")
    plan.add_argument("--case", required=True); plan.add_argument("--campaign", required=True)
    plan.add_argument("--runner", required=True, choices=["codex-cli-hosted", "codex-cli-local"])
    plan.add_argument("--agent-executable", required=True, type=Path); plan.add_argument("--model", required=True)
    plan.add_argument("--model-revision", required=True); plan.add_argument("--immutable-revision", action="store_true")
    plan.add_argument("--reasoning-effort", default="xhigh", choices=["low", "medium", "high", "xhigh"])
    plan.add_argument("--timeout-ms", type=int, default=900_000); plan.add_argument("--local-provider", choices=["lmstudio", "ollama"])
    plan.add_argument("--model-artifact-sha256"); plan.add_argument("--out", required=True, type=Path)
    run = modes.add_parser("run"); run.add_argument("--predeclaration", required=True, type=Path); run.add_argument("--out", required=True, type=Path)
    run.add_argument("--agent-executable", required=True, type=Path); run.add_argument("--genesis-bin", required=True, type=Path); run.add_argument("--selfhost-artifact", required=True, type=Path)
    validate = modes.add_parser("validate"); validate.add_argument("--run", required=True, type=Path)
    replay = modes.add_parser("replay"); replay.add_argument("--run", required=True, type=Path); replay.add_argument("--genesis-bin", required=True, type=Path); replay.add_argument("--selfhost-artifact", required=True, type=Path)
    return out


def main() -> int:
    args = parser().parse_args()
    if args.command == "check":
        doc = validate_authorities()
        controls = self_test() if args.self_test else 0
        result = {"kind": "genesis/genesisbench-open-agent-check-v0.1", "authorityIdentitySha256": doc["contentIdentitySha256"], "controls": controls}
    elif args.command == "plan":
        require(not args.out.exists(), "predeclaration output already exists")
        result = build_predeclaration(
            case_id=args.case, campaign_id=args.campaign, runner_class=args.runner,
            executable=args.agent_executable, model_id=args.model, model_revision=args.model_revision,
            immutable_revision=args.immutable_revision, reasoning_effort=args.reasoning_effort,
            timeout_ms=args.timeout_ms, local_provider=args.local_provider,
            model_artifact_sha256=args.model_artifact_sha256,
        )
        write_json(args.out, result)
    elif args.command == "run":
        result = run_agent(args.predeclaration, args.out, args.agent_executable, args.genesis_bin, args.selfhost_artifact)
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
