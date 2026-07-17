#!/usr/bin/env python3
"""Execute the model-agnostic GenesisCode agent benchmark scorer."""

from __future__ import annotations

import argparse
import json
import os
import re
import signal
import subprocess
import sys
import tempfile
from pathlib import Path, PurePosixPath
from typing import Any

from gc_agent_scoring_contract import (
    BENCHMARK,
    DIMENSION_IDS,
    ROOT,
    SCORING,
    ScoringError,
    canonical_bytes,
    content_identity,
    load_json,
    require,
    safe_relative,
    self_test,
    sha256_bytes,
    render_scoring,
    validate_schema_markers,
    validate_score,
    validate_scoring,
)

RUNTIME_LOG_PATH_RE = re.compile(
    r"^\.genesis/logs/(?P<command>[a-z0-9][a-z0-9_.-]*)-[0-9]+-[0-9]+\.gclog$"
)

def inventory_tree(
    root: Path, *, max_files: int, max_bytes: int, max_file_bytes: int
) -> dict[str, bytes]:
    require(root.is_dir() and not root.is_symlink(), "candidate root must be a regular directory")
    result: dict[str, bytes] = {}
    total = 0
    for directory, dirnames, filenames in os.walk(root, followlinks=False):
        current = Path(directory)
        for name in list(dirnames):
            path = current / name
            require(not path.is_symlink(), "candidate tree contains a symlink directory")
        for name in filenames:
            path = current / name
            require(not path.is_symlink(), "candidate tree contains a symlink file")
            require(path.is_file(), "candidate tree contains a non-regular file")
            relative = path.relative_to(root).as_posix()
            safe_relative(relative, "candidate path")
            payload = path.read_bytes()
            require(len(payload) <= max_file_bytes, f"candidate file exceeds limit: {relative}")
            require(relative not in result, f"duplicate candidate path: {relative}")
            result[relative] = payload
            total += len(payload)
            require(len(result) <= max_files, "candidate file count exceeds limit")
            require(total <= max_bytes, "candidate byte count exceeds limit")
    require(result, "candidate tree is empty")
    return dict(sorted(result.items()))


def authority_files(case: dict[str, Any], side: str) -> dict[str, bytes]:
    root_key = "inputRoot" if side == "input" else "referenceRoot"
    files_key = "inputFiles" if side == "input" else "referenceFiles"
    base = ROOT / case[root_key]
    result: dict[str, bytes] = {}
    for row in case[files_key]:
        relative = row["path"]
        safe_relative(relative, f"{side} path")
        path = base / relative
        require(path.is_file() and not path.is_symlink(), f"missing {side} file: {relative}")
        payload = path.read_bytes()
        require(len(payload) == row["bytes"], f"{side} byte count drift: {relative}")
        require(sha256_bytes(payload) == row["sha256"], f"{side} hash drift: {relative}")
        result[relative] = payload
    return dict(sorted(result.items()))


def inventory_identity(files: dict[str, bytes]) -> str:
    rows = [
        {"path": path, "bytes": len(payload), "sha256": sha256_bytes(payload)}
        for path, payload in sorted(files.items())
    ]
    return sha256_bytes(canonical_bytes(rows))


def normalize_runtime_generated_path(path: str) -> str:
    """Canonicalize only the runtime's PID/clock-derived private log names."""
    match = RUNTIME_LOG_PATH_RE.fullmatch(path)
    if match is None:
        return path
    return f".genesis/logs/{match.group('command')}-$EPHEMERAL.gclog"


def generated_inventory_identity(files: dict[str, bytes]) -> str:
    # Keep every payload and duplicate row; only the runtime-private filename is unstable.
    rows = sorted(
        (
            {
                "path": normalize_runtime_generated_path(path),
                "bytes": len(payload),
                "sha256": sha256_bytes(payload),
            }
            for path, payload in files.items()
        ),
        key=lambda row: (row["path"], row["sha256"], row["bytes"]),
    )
    return sha256_bytes(canonical_bytes(rows))


def materialize(files: dict[str, bytes], root: Path) -> None:
    for relative, payload in files.items():
        path = root.joinpath(*PurePosixPath(relative).parts)
        path.parent.mkdir(parents=True, exist_ok=True)
        path.write_bytes(payload)


def changed_files(before: dict[str, bytes], after: dict[str, bytes]) -> dict[str, bytes]:
    return {
        path: after.get(path, b"")
        for path in sorted(before.keys() | after.keys())
        if before.get(path) != after.get(path)
    }


def generated_path_allowed(path: str, before: dict[str, bytes], prefixes: list[str]) -> bool:
    if path in before or path == ".genesis" or path.startswith(".genesis/"):
        return True
    return any(
        path.startswith(prefix) if prefix.endswith("/") else path == prefix
        for prefix in prefixes
    )


def changed_span_units(left: bytes, right: bytes) -> int:
    prefix = 0
    shared = min(len(left), len(right))
    while prefix < shared and left[prefix] == right[prefix]:
        prefix += 1
    suffix = 0
    left_remaining = len(left) - prefix
    right_remaining = len(right) - prefix
    while (
        suffix < left_remaining
        and suffix < right_remaining
        and left[len(left) - 1 - suffix] == right[len(right) - 1 - suffix]
    ):
        suffix += 1
    return (len(left) - prefix - suffix) + (len(right) - prefix - suffix)


def patch_units(before: dict[str, bytes], after: dict[str, bytes]) -> tuple[list[str], int]:
    paths = sorted(path for path in before.keys() | after.keys() if before.get(path) != after.get(path))
    units = sum(
        1024 + changed_span_units(before.get(path, b""), after.get(path, b""))
        for path in paths
    )
    return paths, units


def json_pointer(document: Any, pointer: str) -> Any:
    if pointer == "":
        return document
    require(pointer.startswith("/"), "assertion pointer is not absolute")
    current = document
    for raw in pointer.split("/")[1:]:
        token = raw.replace("~1", "/").replace("~0", "~")
        if isinstance(current, list):
            require(token.isdigit(), f"invalid array pointer token: {token}")
            index = int(token)
            require(index < len(current), f"array pointer out of range: {pointer}")
            current = current[index]
        else:
            require(isinstance(current, dict) and token in current, f"missing pointer: {pointer}")
            current = current[token]
    return current


def assertion_matches(document: Any, assertion: dict[str, Any]) -> bool:
    try:
        actual = json_pointer(document, assertion["pointer"])
    except ScoringError:
        return False
    if assertion["operator"] == "equals":
        return actual == assertion["value"]
    return isinstance(actual, str) and isinstance(assertion["value"], str) and assertion["value"] in actual


def normalize_execution_document(
    value: Any, selfhost_artifact: Path, *, field_name: str | None = None
) -> Any:
    """Remove scorer-selected host and runtime-private names without changing results."""
    if isinstance(value, dict):
        return {
            key: normalize_execution_document(item, selfhost_artifact, field_name=key)
            for key, item in value.items()
        }
    if isinstance(value, list):
        return [
            normalize_execution_document(item, selfhost_artifact, field_name=field_name)
            for item in value
        ]
    if isinstance(value, str):
        if value == str(selfhost_artifact):
            return "$SELFHOST_ARTIFACT"
        if field_name == "log":
            return normalize_runtime_generated_path(value)
    return value


def execution_environment(home: Path) -> dict[str, str]:
    temporary = home / "tmp"
    temporary.mkdir(parents=True, exist_ok=True)
    return {
        "HOME": str(home),
        "TMPDIR": str(temporary),
        "TZ": "UTC",
        "LANG": "C",
        "LC_ALL": "C",
        "NO_COLOR": "1",
        "GENESIS_ALLOW_RUST_ENGINE": "0",
        "GENESIS_SELFHOST_COMPILED_CACHE_DISABLE": "1",
        "PATH": os.environ.get("PATH", "/usr/bin:/bin"),
    }


def terminate_process_group(process: subprocess.Popen[Any]) -> None:
    if process.poll() is not None:
        return
    try:
        os.killpg(process.pid, signal.SIGKILL)
    except ProcessLookupError:
        pass
    process.wait()


def run_step(
    workspace: Path,
    home: Path,
    step: dict[str, Any],
    resources: dict[str, Any],
    genesis_bin: Path,
    selfhost_artifact: Path,
    generated_path_prefixes: list[str],
) -> tuple[dict[str, Any], dict[str, Any]]:
    before = inventory_tree(
        workspace,
        max_files=resources["maxCandidateFiles"] + resources["maxGeneratedFiles"],
        max_bytes=resources["maxCandidateBytes"] + resources["maxGeneratedBytes"],
        max_file_bytes=resources["maxGeneratedBytes"],
    )
    argv = step["argv"]
    require(argv[0] == "--json", "verification must use JSON mode")
    require("--no-step-limit" not in argv, "verification cannot disable the evaluator budget")
    command = [
        str(genesis_bin),
        "--json",
        "--selfhost-artifact",
        str(selfhost_artifact),
    ]
    if "--step-limit" not in argv:
        command.extend(["--step-limit", str(resources["defaultEvaluatorStepLimit"])])
    command.extend(argv[1:])
    timed_out = False
    with tempfile.TemporaryFile() as stdout_file, tempfile.TemporaryFile() as stderr_file:
        process = subprocess.Popen(
            command,
            cwd=workspace,
            env=execution_environment(home),
            stdin=subprocess.DEVNULL,
            stdout=stdout_file,
            stderr=stderr_file,
            start_new_session=True,
        )
        try:
            exit_code = process.wait(timeout=resources["processTimeoutMs"] / 1000)
        except subprocess.TimeoutExpired:
            timed_out = True
            terminate_process_group(process)
            exit_code = -1
        stdout_size = os.fstat(stdout_file.fileno()).st_size
        stderr_size = os.fstat(stderr_file.fileno()).st_size
        stdout_file.seek(0)
        stderr_file.seek(0)
        stdout = stdout_file.read(resources["maxStdoutBytes"] + 1)
        stderr_file.read(resources["maxStderrBytes"] + 1)
    output_within_limit = (
        stdout_size <= resources["maxStdoutBytes"]
        and stderr_size <= resources["maxStderrBytes"]
    )
    document: Any = None
    if output_within_limit and not timed_out:
        try:
            document = json.loads(stdout.decode("utf-8"), object_pairs_hook=_reject_pairs)
        except (UnicodeDecodeError, json.JSONDecodeError, ScoringError):
            document = None
    after = inventory_tree(
        workspace,
        max_files=resources["maxCandidateFiles"] + resources["maxGeneratedFiles"],
        max_bytes=resources["maxCandidateBytes"] + resources["maxGeneratedBytes"],
        max_file_bytes=resources["maxGeneratedBytes"],
    )
    generated = changed_files(before, after)
    generated_bytes = sum(len(payload) for payload in generated.values())
    generated_scope_ok = all(
        generated_path_allowed(path, before, generated_path_prefixes) for path in generated
    )
    generated_limits = (
        len(generated) <= resources["maxGeneratedFiles"]
        and generated_bytes <= resources["maxGeneratedBytes"]
        and generated_scope_ok
    )
    assertions_passed = 0
    if document is not None:
        assertions_passed = sum(
            assertion_matches(document, assertion) for assertion in step["assertions"]
        )
    ok_value = document.get("ok") if isinstance(document, dict) else None
    kind_value = document.get("kind") if isinstance(document, dict) else None
    passed = (
        output_within_limit
        and generated_limits
        and not timed_out
        and exit_code == step["exitCode"]
        and ok_value == step["ok"]
        and kind_value == step["kind"]
        and assertions_passed == len(step["assertions"])
    )
    normalized_document = (
        normalize_execution_document(document, selfhost_artifact)
        if document is not None
        else None
    )
    accounted_output = (
        canonical_bytes(normalized_document)
        if document is not None
        else stdout[: resources["maxStdoutBytes"]]
    )
    output_identity = sha256_bytes(accounted_output)
    generated_identity = generated_inventory_identity(generated)
    units = generated_bytes + len(accounted_output) + stderr_size + 1
    report = {
        "id": step["id"],
        "passed": passed,
        "exitCode": exit_code,
        "ok": ok_value if isinstance(ok_value, bool) else None,
        "kind": kind_value if isinstance(kind_value, str) else None,
        "assertionsPassed": assertions_passed,
        "outputIdentitySha256": output_identity,
        "generatedIdentitySha256": generated_identity,
        "resourceUnits": max(1, units),
    }
    internal = {
        "generatedBytes": generated_bytes,
        "limitsSatisfied": output_within_limit and generated_limits and not timed_out,
        "outputIdentitySha256": output_identity,
        "generatedIdentitySha256": generated_identity,
    }
    return report, internal


def _reject_pairs(rows: list[tuple[str, Any]]) -> dict[str, Any]:
    result: dict[str, Any] = {}
    for key, value in rows:
        require(key not in result, f"duplicate command JSON key: {key}")
        result[key] = value
    return result


def strip_toml_comment(line: str) -> str:
    quoted = False
    escaped = False
    for index, char in enumerate(line):
        if escaped:
            escaped = False
        elif char == "\\" and quoted:
            escaped = True
        elif char == '"':
            quoted = not quoted
        elif char == "#" and not quoted:
            return line[:index]
    require(not quoted, "unterminated TOML string")
    return line


def parse_toml_value(value: str) -> Any:
    require(value, "empty TOML value")
    try:
        parsed = json.loads(value, object_pairs_hook=_reject_pairs)
    except json.JSONDecodeError as exc:
        raise ScoringError("unsupported TOML value in scoring input") from exc
    require(
        isinstance(parsed, (str, int, bool, list)) and not isinstance(parsed, float),
        "unsupported TOML value type in scoring input",
    )
    require(
        not isinstance(parsed, list)
        or all(isinstance(item, (str, int, bool)) and not isinstance(item, float) for item in parsed),
        "nested TOML values are outside the scoring subset",
    )
    return parsed


def parse_toml_subset(payload: bytes) -> dict[str, Any]:
    try:
        source = payload.decode("utf-8")
    except UnicodeDecodeError as exc:
        raise ScoringError("TOML scoring input is not UTF-8") from exc
    document: dict[str, Any] = {}
    current: dict[str, Any] = document
    for raw in source.splitlines():
        line = strip_toml_comment(raw).strip()
        if not line:
            continue
        array_table = re.fullmatch(r"\[\[([A-Za-z0-9_-]+)\]\]", line)
        if array_table:
            key = array_table.group(1)
            existing = document.setdefault(key, [])
            require(isinstance(existing, list), "TOML table kind collision")
            row: dict[str, Any] = {}
            existing.append(row)
            current = row
            continue
        table = re.fullmatch(r"\[([A-Za-z0-9_-]+)(?:\.\"([^\"]+)\")?\]", line)
        if table:
            outer, inner = table.groups()
            existing = document.setdefault(outer, {})
            require(isinstance(existing, dict), "TOML table kind collision")
            if inner is None:
                current = existing
            else:
                nested = existing.setdefault(inner, {})
                require(isinstance(nested, dict), "TOML nested table kind collision")
                current = nested
            continue
        assignment = re.fullmatch(r"([A-Za-z0-9_-]+)\s*=\s*(.+)", line)
        require(assignment is not None, "unsupported TOML syntax in scoring input")
        key, value = assignment.groups()
        require(key not in current, f"duplicate TOML key: {key}")
        current[key] = parse_toml_value(value.strip())
    return document


def artifact_contract_report(
    files: dict[str, bytes], checks: list[dict[str, Any]]
) -> tuple[dict[str, Any], dict[str, Any]]:
    results = []
    passed = 0
    for check in checks:
        payload = files.get(check["path"])
        document: Any = None
        if payload is not None:
            try:
                if check["format"] == "json":
                    document = json.loads(payload.decode("utf-8"), object_pairs_hook=_reject_pairs)
                elif check["format"] == "toml":
                    document = parse_toml_subset(payload)
            except (UnicodeDecodeError, json.JSONDecodeError, ScoringError):
                document = None
        matched = document is not None and assertion_matches(document, check)
        passed += int(matched)
        results.append(
            {
                "format": check["format"],
                "matched": matched,
                "operator": check["operator"],
                "path": check["path"],
                "pointer": check["pointer"],
            }
        )
    all_passed = passed == len(checks)
    output = canonical_bytes(results)
    report = {
        "id": "artifact-contract",
        "passed": all_passed,
        "exitCode": 0 if all_passed else 1,
        "ok": all_passed,
        "kind": "genesis/artifact-contract-v0.1",
        "assertionsPassed": passed,
        "outputIdentitySha256": sha256_bytes(output),
        "generatedIdentitySha256": inventory_identity({}),
        "resourceUnits": max(1, len(output)),
    }
    internal = {
        "generatedBytes": 0,
        "limitsSatisfied": True,
        "outputIdentitySha256": report["outputIdentitySha256"],
        "generatedIdentitySha256": report["generatedIdentitySha256"],
    }
    return report, internal


def run_submission(
    files: dict[str, bytes],
    case: dict[str, Any],
    resources: dict[str, Any],
    genesis_bin: Path,
    selfhost_artifact: Path,
    generated_path_prefixes: list[str],
) -> tuple[list[dict[str, Any]], dict[str, dict[str, Any]], int, int, bool]:
    verification_count = len(case["verification"]) + bool(case["artifactAssertions"])
    require(
        verification_count <= resources["maxVerificationSteps"],
        "verification count exceeds scorer policy",
    )
    reports = []
    internal: dict[str, dict[str, Any]] = {}
    generated_bytes = 0
    execution_units = 0
    limits_satisfied = True
    with tempfile.TemporaryDirectory(prefix="genesis-agent-score-") as temporary:
        temp = Path(temporary)
        workspace = temp / "workspace"
        workspace.mkdir()
        materialize(files, workspace)
        if case["artifactAssertions"]:
            report, facts = artifact_contract_report(files, case["artifactAssertions"])
            reports.append(report)
            internal[report["id"]] = facts
            execution_units += report["resourceUnits"]
        for step in case["verification"]:
            source_append = step["sourceAppend"]
            if source_append is not None:
                relative = safe_relative(source_append["path"], "source append path")
                require(
                    source_append["path"] in case["editablePaths"],
                    "source append escapes the editable surface",
                )
                source_path = workspace.joinpath(*relative.parts)
                require(
                    source_path.is_file() and not source_path.is_symlink(),
                    "source append target is unavailable",
                )
                with source_path.open("ab") as handle:
                    handle.write(source_append["source"].encode("utf-8"))
            report, facts = run_step(
                workspace,
                temp / "home",
                step,
                resources,
                genesis_bin,
                selfhost_artifact,
                generated_path_prefixes,
            )
            reports.append(report)
            internal[step["id"]] = facts
            generated_bytes += facts["generatedBytes"]
            execution_units += report["resourceUnits"]
            limits_satisfied = limits_satisfied and facts["limitsSatisfied"]
    source_bytes = sum(len(payload) for payload in files.values())
    total_units = source_bytes + execution_units
    return reports, internal, generated_bytes, total_units, limits_satisfied


def policy_authorities(files: dict[str, bytes], paths: list[str]) -> tuple[set[str], str]:
    authorities: set[str] = set()
    canonical_policies = []
    for relative in paths:
        payload = files.get(relative)
        if payload is None:
            continue
        try:
            document = parse_toml_subset(payload)
        except ScoringError:
            authorities.add(f"invalid:{relative}")
            canonical_policies.append({"path": relative, "invalid": True})
            continue
        allow = document.get("allow", [])
        if isinstance(allow, list):
            for op in allow:
                if isinstance(op, str):
                    authorities.add(f"allow:{op}")
        op_configs = document.get("op", {})
        if isinstance(op_configs, dict):
            for op, config in sorted(op_configs.items()):
                if isinstance(op, str):
                    digest = sha256_bytes(canonical_bytes(config))
                    authorities.add(f"config:{op}:{digest}")
        canonical_policies.append({"path": relative, "document": document})
    return authorities, sha256_bytes(canonical_bytes(canonical_policies))


def obligation_identity(files: dict[str, bytes]) -> str:
    rows = []
    for relative, payload in sorted(files.items()):
        if not relative.endswith(".toml"):
            continue
        try:
            document = parse_toml_subset(payload)
        except ScoringError:
            rows.append({"path": relative, "invalid": True})
            continue
        selected = {
            key: document[key]
            for key in ("modules", "obligations", "tests", "limits")
            if key in document
        }
        if selected:
            rows.append({"path": relative, "requirements": selected})
    return sha256_bytes(canonical_bytes(rows))


def ratio_score(reference: int, candidate: int) -> int:
    if candidate <= reference:
        return 10000
    return min(10000, (reference * 10000) // max(1, candidate))


def step_score(ids: list[str], reports: dict[str, dict[str, Any]]) -> int:
    if not ids:
        return 10000
    return sum(10000 for step_id in ids if reports[step_id]["passed"]) // len(ids)


def score_candidate(
    scoring: dict[str, Any],
    case_id: str,
    candidate_root: Path,
    genesis_bin: Path,
    selfhost_artifact: Path,
    *,
    suite_document: dict[str, Any] | None = None,
) -> dict[str, Any]:
    require(genesis_bin.is_file() and not genesis_bin.is_symlink(), "genesis binary is unavailable")
    require(
        selfhost_artifact.is_file() and not selfhost_artifact.is_symlink(),
        "selfhost artifact is unavailable",
    )
    genesis_bin = genesis_bin.resolve(strict=True)
    selfhost_artifact = selfhost_artifact.resolve(strict=True)
    require(
        candidate_root.is_dir() and not candidate_root.is_symlink(),
        "candidate root must be a regular non-symlink directory",
    )
    candidate_root = candidate_root.resolve(strict=True)
    suite = suite_document or load_json(BENCHMARK)
    case = next((row for row in suite["cases"] if row["id"] == case_id), None)
    require(case is not None, f"unknown benchmark case: {case_id}")
    policy = next(row for row in scoring["taskPolicies"] if row["taskClass"] == case["taskClass"])
    resources = scoring["resourcePolicy"]
    candidate_files = inventory_tree(
        candidate_root,
        max_files=resources["maxCandidateFiles"],
        max_bytes=resources["maxCandidateBytes"],
        max_file_bytes=resources["maxFileBytes"],
    )
    input_files = authority_files(case, "input")
    reference_files = authority_files(case, "reference")
    required_unchanged = set(input_files) - set(case["editablePaths"])
    allowed = set(input_files) | set(case["editablePaths"])
    editable_scope_ok = set(candidate_files).issubset(allowed) and all(
        candidate_files.get(path) == input_files[path] for path in required_unchanged
    )

    candidate_reports, candidate_internal, candidate_generated, candidate_units, candidate_limits = run_submission(
        candidate_files,
        case,
        resources,
        genesis_bin,
        selfhost_artifact,
        policy["generatedPathPrefixes"],
    )
    reference_reports, reference_internal, reference_generated, reference_units, reference_limits = run_submission(
        reference_files,
        case,
        resources,
        genesis_bin,
        selfhost_artifact,
        policy["generatedPathPrefixes"],
    )
    require(reference_limits and all(row["passed"] for row in reference_reports), "reference execution failed closed")
    candidate_by_id = {row["id"]: row for row in candidate_reports}
    reference_by_id = {row["id"]: row for row in reference_reports}

    semantic_score = sum(10000 for row in candidate_reports if row["passed"]) // len(candidate_reports)
    obligation_ids = policy["obligationStepIds"]
    obligation_score = step_score(obligation_ids, candidate_by_id)
    if obligation_ids and obligation_identity(candidate_files) != obligation_identity(reference_files):
        obligation_score = 0
    effect_ids = policy["effectStepIds"]
    effect_matches = 0
    for step_id in effect_ids:
        candidate_step = candidate_by_id[step_id]
        reference_step = reference_by_id[step_id]
        candidate_facts = candidate_internal[step_id]
        reference_facts = reference_internal[step_id]
        if (
            candidate_step["passed"]
            and candidate_facts["outputIdentitySha256"] == reference_facts["outputIdentitySha256"]
            and candidate_facts["generatedIdentitySha256"] == reference_facts["generatedIdentitySha256"]
        ):
            effect_matches += 1
    effect_score = 10000 if not effect_ids else (effect_matches * 10000) // len(effect_ids)

    changed_paths, candidate_patch_units = patch_units(input_files, candidate_files)
    expected_changed_paths, reference_patch_units = patch_units(input_files, reference_files)
    require(reference_patch_units > 0, "reference patch has no measurable change")
    patch_score = ratio_score(reference_patch_units, candidate_patch_units) if editable_scope_ok else 0

    candidate_authorities, candidate_authority_identity = policy_authorities(
        candidate_files, policy["policyPaths"]
    )
    reference_authorities, reference_authority_identity = policy_authorities(
        reference_files, policy["policyPaths"]
    )
    broadened = sorted(candidate_authorities - reference_authorities)
    wildcard = any("*" in authority for authority in candidate_authorities)
    policy_scope_ok = not broadened and not wildcard
    policy_score = 10000 if policy_scope_ok else 0
    resource_score = ratio_score(reference_units, candidate_units) if candidate_limits else 0

    dimension_scores = {
        "semantics": (True, semantic_score),
        "obligations": (bool(obligation_ids), obligation_score),
        "effects": (bool(effect_ids), effect_score),
        "patch-minimality": (True, patch_score),
        "resource-use": (True, resource_score),
        "policy-scope": (True, policy_score),
    }
    dimensions = []
    for definition in scoring["dimensions"]:
        applicable, value = dimension_scores[definition["id"]]
        dimensions.append(
            {
                "id": definition["id"],
                "applicable": applicable,
                "weightBasisPoints": definition["weightBasisPoints"],
                "scoreBasisPoints": value,
            }
        )
    failed_dimensions = sorted(
        dimension
        for dimension in scoring["validityGate"]["requiredPerfectDimensions"]
        if dimension_scores[dimension][0] and dimension_scores[dimension][1] != 10000
    )
    if not editable_scope_ok:
        failed_dimensions.append("editable-scope")
        failed_dimensions.sort()
    validity_passed = not failed_dimensions
    applicable = [row for row in dimensions if row["applicable"]]
    denominator = sum(row["weightBasisPoints"] for row in applicable)
    aggregate = sum(
        row["weightBasisPoints"] * row["scoreBasisPoints"] for row in applicable
    ) // denominator
    quality_score = aggregate if validity_passed else scoring["validityGate"]["failureScoreBasisPoints"]

    report = {
        "kind": "genesis/agent-benchmark-score-v0.1",
        "version": "0.1.0",
        "scoringId": scoring["scoringId"],
        "caseId": case["id"],
        "taskClass": case["taskClass"],
        "contextTier": case["contextTier"],
        "bindings": {
            "scoringContentIdentitySha256": scoring["contentIdentitySha256"],
            "benchmarkContentIdentitySha256": scoring["benchmark"]["contentIdentitySha256"],
            "profileSha256": scoring["profile"]["sha256"],
            "scorerRuntimeSha256": scoring["implementation"]["runtimeSha256"],
            "scorerContractSha256": scoring["implementation"]["contractSha256"],
        },
        "candidate": {
            "identitySha256": inventory_identity(candidate_files),
            "fileCount": len(candidate_files),
            "bytes": sum(len(payload) for payload in candidate_files.values()),
        },
        "validity": {"passed": validity_passed, "failedDimensions": failed_dimensions},
        "dimensions": dimensions,
        "qualityScoreBasisPoints": quality_score,
        "verification": candidate_reports,
        "patch": {
            "changedPaths": changed_paths,
            "expectedChangedPaths": expected_changed_paths,
            "editableScopeOk": editable_scope_ok,
            "candidateUnits": candidate_patch_units,
            "referenceUnits": reference_patch_units,
        },
        "policy": {
            "scopeOk": policy_scope_ok,
            "candidateAuthorityIdentitySha256": candidate_authority_identity,
            "referenceAuthorityIdentitySha256": reference_authority_identity,
            "broadenedAuthorities": broadened,
        },
        "resources": {
            "candidateUnits": candidate_units,
            "referenceUnits": reference_units,
            "candidateGeneratedBytes": candidate_generated,
            "referenceGeneratedBytes": reference_generated,
            "limitsSatisfied": candidate_limits,
        },
        "modelSpecificMetrics": {
            "includedInQualityScore": False,
            "recordedBy": "genesis/agent-benchmark-run-v0.1",
            "present": False,
        },
        "scoreIdentitySha256": "",
    }
    report["scoreIdentitySha256"] = content_identity(report, "scoreIdentitySha256")
    validate_score(report)
    return report




def main() -> int:
    parser = argparse.ArgumentParser()
    mode = parser.add_mutually_exclusive_group(required=True)
    mode.add_argument("--check", action="store_true")
    mode.add_argument("--score", action="store_true")
    mode.add_argument("--render", action="store_true")
    parser.add_argument("--self-test", action="store_true")
    parser.add_argument("--case")
    parser.add_argument("--candidate", type=Path)
    parser.add_argument("--genesis-bin", type=Path)
    parser.add_argument("--selfhost-artifact", type=Path)
    args = parser.parse_args()
    validate_schema_markers()
    if args.render:
        require(
            not args.self_test
            and args.case is None
            and args.candidate is None
            and args.genesis_bin is None
            and args.selfhost_artifact is None,
            "render mode does not accept execution inputs",
        )
        sys.stdout.buffer.write(canonical_bytes(render_scoring(load_json(SCORING))))
        return 0
    scoring = validate_scoring(load_json(SCORING))
    if args.check:
        require(
            args.case is None
            and args.candidate is None
            and args.genesis_bin is None
            and args.selfhost_artifact is None,
            "check mode does not accept execution inputs",
        )
        controls = self_test(scoring) if args.self_test else 0
        if args.self_test:
            require(ratio_score(100, 100) == 10000, "equal ratio score drift")
            require(ratio_score(100, 125) == 8000, "bounded ratio score drift")
            require(
                changed_span_units(b"abc", b"axc") == 2,
                "changed-span accounting drift",
            )
            require(
                generated_path_allowed("effect.gclog", {}, ["effect.gclog"]),
                "exact generated-file scope drift",
            )
            require(
                not generated_path_allowed("effect.gclog.extra", {}, ["effect.gclog"]),
                "generated-file prefix broadening",
            )
            require(
                generated_path_allowed("dist/service/app", {}, ["dist/"]),
                "generated-directory scope drift",
            )
            require(
                canonical_bytes({"ok": True, "value": 1})
                == canonical_bytes(json.loads(b'{\n  "value": 1, "ok": true\n}')),
                "canonical output resource accounting drift",
            )
            require(
                normalize_execution_document(
                    {
                        "artifact": "/host/a/toolchain.gc",
                        "log": ".genesis/logs/pkg-build-123-456.gclog",
                        "value": "42",
                    },
                    Path("/host/a/toolchain.gc"),
                )
                == {
                    "artifact": "$SELFHOST_ARTIFACT",
                    "log": ".genesis/logs/pkg-build-$EPHEMERAL.gclog",
                    "value": "42",
                },
                "execution provenance normalization drift",
            )
            require(
                generated_inventory_identity(
                    {".genesis/logs/pkg-build-123-456.gclog": b"stable-log\n"}
                )
                == generated_inventory_identity(
                    {".genesis/logs/pkg-build-999-888.gclog": b"stable-log\n"}
                ),
                "runtime-private log filename affected generated identity",
            )
            require(
                generated_inventory_identity(
                    {".genesis/logs/pkg-build-123-456.gclog": b"stable-log\n"}
                )
                != generated_inventory_identity(
                    {".genesis/logs/pkg-build-999-888.gclog": b"tampered-log\n"}
                ),
                "runtime-private log payload was excluded from generated identity",
            )
            require(
                normalize_runtime_generated_path(
                    ".genesis/logs/pkg-build-user-controlled.gclog"
                )
                == ".genesis/logs/pkg-build-user-controlled.gclog",
                "non-runtime log name was normalized",
            )
            require(
                normalize_execution_document(
                    {"stdout": ".genesis/logs/pkg-build-123-456.gclog"},
                    Path("/host/a/toolchain.gc"),
                )
                == {"stdout": ".genesis/logs/pkg-build-123-456.gclog"},
                "user-controlled output was normalized as runtime provenance",
            )
            artifact_files = {
                "answer.json": b'{"answer":42,"ok":true}\n',
                "case.toml": b'name = "benchmark"\nschema = 1\n',
            }
            json_report, _ = artifact_contract_report(
                artifact_files,
                [{"path": "answer.json", "format": "json", "pointer": "/answer", "operator": "equals", "value": 42}],
            )
            require(json_report["passed"], "JSON artifact assertion drift")
            toml_report, _ = artifact_contract_report(
                artifact_files,
                [{"path": "case.toml", "format": "toml", "pointer": "", "operator": "equals", "value": {"name": "benchmark", "schema": 1}}],
            )
            require(toml_report["passed"], "TOML artifact assertion drift")
            controls += 14
        print(
            "gc-agent-scoring: ok "
            f"(dimensions={len(scoring['dimensions'])} tasks={len(scoring['taskPolicies'])} "
            f"controls={controls} identity={scoring['contentIdentitySha256']})"
        )
        return 0
    require(
        args.case is not None
        and args.candidate is not None
        and args.genesis_bin is not None
        and args.selfhost_artifact is not None,
        "score mode requires --case, --candidate, --genesis-bin, and --selfhost-artifact",
    )
    report = score_candidate(
        scoring,
        args.case,
        args.candidate,
        args.genesis_bin,
        args.selfhost_artifact,
    )
    sys.stdout.buffer.write(canonical_bytes(report))
    return 0


if __name__ == "__main__":
    try:
        raise SystemExit(main())
    except (ScoringError, FileNotFoundError, OSError) as exc:
        print(f"gc-agent-scoring: {exc}", file=sys.stderr)
        raise SystemExit(1)
