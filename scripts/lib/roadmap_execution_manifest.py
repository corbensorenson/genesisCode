#!/usr/bin/env python3
"""Render and validate the deterministic GenesisCode roadmap execution graph."""

from __future__ import annotations

import argparse
import copy
from hashlib import sha256
import json
from pathlib import Path
import re
import sys
from typing import Any, Dict, Iterable, List, Mapping, Optional, Sequence, Tuple


ROOT = Path(__file__).resolve().parents[2]
DEFAULT_ROADMAP = ROOT / "ROADMAP.md"
DEFAULT_POLICY = ROOT / "policies/roadmap_execution_v0.1.json"
DEFAULT_SCHEMA = ROOT / "docs/spec/ROADMAP_EXECUTION_MANIFEST_v0.1.schema.json"
DEFAULT_MANIFEST = ROOT / "docs/program/ROADMAP_EXECUTION_MANIFEST_v0.1.json"

TASK_RE = re.compile(
    r"^- \[(?P<state>[ x])\] \*\*(?P<id>(?:R\d+\.\d+\.[a-z]|F\d+\.[a-z])) "
    r"(?P<title>.+?)\*\*\s*(?P<body>.*)$"
)
DONE_RE = re.compile(
    r"\bdone (?P<date>\d{4}-\d{2}-\d{2}); evidence: (?P<evidence>.+?); "
    r"input: `?(?P<input>[a-z0-9-]+-sha256:[0-9a-f]{64})`?$"
)
TASK_ID_RE = re.compile(r"^(?:R\d+\.\d+\.[a-z]|F\d+\.[a-z])$")
WORKSTREAM_RE = re.compile(r"^(?:R\d+\.\d+|F\d+)$")
DATE_RE = re.compile(r"^\d{4}-\d{2}-\d{2}$")
HASH_RE = re.compile(r"^[0-9a-f]{64}$")
EVIDENCE_ID_RE = re.compile(r"^[a-z0-9-]+-sha256:[0-9a-f]{64}$")
PATH_TOKEN_RE = re.compile(r"^[A-Za-z0-9_.-]+(?:/[A-Za-z0-9_.*?@+-]+)+/?$")
HOST_PATH_RE = re.compile(
    r"/(?:Users|home|private/var/folders|var/folders)/|(?i:[A-Z]:\\\\)"
)


class ManifestError(ValueError):
    pass


def reject_duplicate_keys(pairs: Sequence[Tuple[str, Any]]) -> Dict[str, Any]:
    result: Dict[str, Any] = {}
    for key, value in pairs:
        if key in result:
            raise ManifestError(f"duplicate JSON key: {key}")
        result[key] = value
    return result


def load_json(path: Path, label: str) -> Any:
    try:
        return json.loads(
            path.read_text(encoding="utf-8"), object_pairs_hook=reject_duplicate_keys
        )
    except FileNotFoundError as exc:
        raise ManifestError(f"missing {label}: {display_path(path)}") from exc
    except json.JSONDecodeError as exc:
        raise ManifestError(
            f"invalid JSON in {display_path(path)}:{exc.lineno}:{exc.colno}: {exc.msg}"
        ) from exc


def display_path(path: Path) -> str:
    try:
        return path.resolve().relative_to(ROOT).as_posix()
    except ValueError:
        return path.as_posix()


def digest(path: Path) -> str:
    return sha256(path.read_bytes()).hexdigest()


def require_object(value: Any, label: str) -> Mapping[str, Any]:
    if not isinstance(value, dict):
        raise ManifestError(f"{label} must be an object")
    return value


def require_string(value: Any, label: str) -> str:
    if not isinstance(value, str) or not value.strip():
        raise ManifestError(f"{label} must be a non-empty string")
    return value


def require_string_list(
    value: Any, label: str, *, non_empty: bool = False
) -> List[str]:
    if not isinstance(value, list):
        raise ManifestError(f"{label} must be an array")
    if non_empty and not value:
        raise ManifestError(f"{label} must not be empty")
    result: List[str] = []
    seen = set()
    for index, raw in enumerate(value):
        item = require_string(raw, f"{label}[{index}]")
        if item in seen:
            raise ManifestError(f"{label} contains duplicate value: {item}")
        seen.add(item)
        result.append(item)
    return result


def reject_unknown_fields(
    value: Mapping[str, Any], allowed: Iterable[str], label: str
) -> None:
    unknown = sorted(set(value) - set(allowed))
    if unknown:
        raise ManifestError(f"{label} contains unknown fields: {', '.join(unknown)}")


def validate_repo_path(raw: str, label: str, *, must_exist: bool) -> None:
    path = Path(raw)
    if path.is_absolute() or ".." in path.parts:
        raise ManifestError(f"{label} must be repository-relative: {raw}")
    if must_exist and not (ROOT / path).exists():
        raise ManifestError(f"{label} does not exist: {raw}")


def task_workstream(task_id: str) -> str:
    parts = task_id.split(".")
    return ".".join(parts[:2]) if task_id.startswith("R") else parts[0]


def task_phase(task_id: str) -> str:
    return task_id.split(".")[0]


def parse_evidence_commands(raw: str) -> List[str]:
    commands = re.findall(r"`([^`]+)`", raw)
    if not commands:
        commands = [raw.strip()]
    result: List[str] = []
    for command in commands:
        command = command.strip()
        if command and command not in result:
            result.append(command)
    return result


def declared_artifacts(title: str, objective: str) -> List[str]:
    result: List[str] = []
    for token in re.findall(r"`([^`]+)`", f"{title} {objective}"):
        token = token.strip().rstrip(".,;:")
        if " " in token or not PATH_TOKEN_RE.fullmatch(token):
            continue
        if Path(token).is_absolute() or ".." in Path(token).parts:
            continue
        if token not in result:
            result.append(token)
    return result


def parse_roadmap(path: Path) -> List[Dict[str, Any]]:
    if not path.is_file():
        raise ManifestError(f"missing roadmap: {display_path(path)}")
    tasks: List[Dict[str, Any]] = []
    seen = set()
    for line_number, line in enumerate(
        path.read_text(encoding="utf-8").splitlines(), 1
    ):
        match = TASK_RE.match(line)
        if match is None:
            continue
        task_id = match.group("id")
        if task_id in seen:
            raise ManifestError(f"duplicate roadmap task id: {task_id}")
        seen.add(task_id)
        state = "done" if match.group("state") == "x" else "open"
        title = match.group("title").strip().rstrip(".:")
        body = match.group("body").strip()
        done_match = DONE_RE.search(body)
        if state == "done" and done_match is None:
            raise ManifestError(
                f"done roadmap task lacks durable annotation: {task_id}"
            )
        if state == "open" and done_match is not None:
            raise ManifestError(
                f"open roadmap task carries a done annotation: {task_id}"
            )
        objective = body[: done_match.start()].rstrip() if done_match else body
        evidence = None
        if done_match:
            evidence = {
                "completed_date": done_match.group("date"),
                "commands": parse_evidence_commands(done_match.group("evidence")),
                "input_identity": done_match.group("input"),
            }
        tasks.append(
            {
                "id": task_id,
                "phase": task_phase(task_id),
                "workstream": task_workstream(task_id),
                "source_line": line_number,
                "state": state,
                "title": title,
                "objective": objective,
                "declared_artifacts": declared_artifacts(title, objective),
                "evidence": evidence,
            }
        )
    if not tasks:
        raise ManifestError("ROADMAP.md contains no recognized tasks")
    return tasks


def validate_policy(raw: Any, tasks: Sequence[Mapping[str, Any]]) -> Mapping[str, Any]:
    policy = require_object(raw, "policy")
    reject_unknown_fields(
        policy,
        (
            "kind",
            "version",
            "audit_date",
            "risk_classes",
            "resource_classes",
            "execution_profiles",
            "workstreams",
            "task_prerequisites",
        ),
        "policy",
    )
    if policy.get("kind") != "genesis/roadmap-execution-policy-v0.1":
        raise ManifestError("policy.kind must be genesis/roadmap-execution-policy-v0.1")
    if policy.get("version") != "0.1":
        raise ManifestError("policy.version must be 0.1")
    audit_date = require_string(policy.get("audit_date"), "policy.audit_date")
    if not DATE_RE.fullmatch(audit_date):
        raise ManifestError("policy.audit_date must use YYYY-MM-DD")

    risk_classes = require_object(policy.get("risk_classes"), "policy.risk_classes")
    if set(risk_classes) != {"low", "medium", "high", "critical"}:
        raise ManifestError(
            "policy.risk_classes must define low, medium, high, critical"
        )
    for risk, raw_rule in risk_classes.items():
        rule = require_object(raw_rule, f"policy.risk_classes[{risk}]")
        reject_unknown_fields(
            rule,
            ("negative_controls", "rollback"),
            f"policy.risk_classes[{risk}]",
        )
        require_string_list(
            rule.get("negative_controls"),
            f"policy.risk_classes[{risk}].negative_controls",
            non_empty=True,
        )
        require_string(rule.get("rollback"), f"policy.risk_classes[{risk}].rollback")

    resource_classes = require_string_list(
        policy.get("resource_classes"), "policy.resource_classes", non_empty=True
    )
    execution_profiles = require_object(
        policy.get("execution_profiles"), "policy.execution_profiles"
    )
    if not execution_profiles:
        raise ManifestError("policy.execution_profiles must not be empty")
    for profile_name, raw_profile in execution_profiles.items():
        profile = require_object(
            raw_profile, f"policy.execution_profiles[{profile_name}]"
        )
        reject_unknown_fields(
            profile,
            (
                "risk_class",
                "resource_class",
                "owner_paths",
                "guard_checks",
                "negative_controls",
            ),
            f"policy.execution_profiles[{profile_name}]",
        )
        risk = require_string(
            profile.get("risk_class"), f"execution profile {profile_name}.risk_class"
        )
        if risk not in risk_classes:
            raise ManifestError(
                f"execution profile {profile_name} uses unknown risk class: {risk}"
            )
        resource = require_string(
            profile.get("resource_class"),
            f"execution profile {profile_name}.resource_class",
        )
        if resource not in resource_classes:
            raise ManifestError(
                f"execution profile {profile_name} uses unknown resource class: {resource}"
            )
        owner_paths = require_string_list(
            profile.get("owner_paths"),
            f"execution profile {profile_name}.owner_paths",
            non_empty=True,
        )
        for index, owner_path in enumerate(owner_paths):
            validate_repo_path(
                owner_path,
                f"execution profile {profile_name}.owner_paths[{index}]",
                must_exist=True,
            )
        guard_checks = require_string_list(
            profile.get("guard_checks"),
            f"execution profile {profile_name}.guard_checks",
            non_empty=True,
        )
        for index, guard in enumerate(guard_checks):
            validate_repo_path(
                guard,
                f"execution profile {profile_name}.guard_checks[{index}]",
                must_exist=True,
            )
            if not guard.startswith("scripts/check_"):
                raise ManifestError(
                    f"execution profile {profile_name} guard is not a check: {guard}"
                )
        require_string_list(
            profile.get("negative_controls"),
            f"execution profile {profile_name}.negative_controls",
        )

    workstreams = require_object(policy.get("workstreams"), "policy.workstreams")
    task_workstreams = {str(task["workstream"]) for task in tasks}
    if set(workstreams) != task_workstreams:
        missing = sorted(task_workstreams - set(workstreams))
        extra = sorted(set(workstreams) - task_workstreams)
        raise ManifestError(
            f"policy workstream coverage drift: missing={missing} extra={extra}"
        )
    for workstream, raw_rule in workstreams.items():
        if not WORKSTREAM_RE.fullmatch(workstream):
            raise ManifestError(f"invalid workstream id: {workstream}")
        rule = require_object(raw_rule, f"policy.workstreams[{workstream}]")
        reject_unknown_fields(
            rule,
            (
                "start_after",
                "sequential",
                "profile",
                "owner_paths",
                "guard_checks",
                "negative_controls",
                "parallel_safe_with",
            ),
            f"policy.workstreams[{workstream}]",
        )
        require_string_list(rule.get("start_after"), f"{workstream}.start_after")
        if not isinstance(rule.get("sequential"), bool):
            raise ManifestError(f"{workstream}.sequential must be boolean")
        profile_name = require_string(rule.get("profile"), f"{workstream}.profile")
        if profile_name not in execution_profiles:
            raise ManifestError(
                f"{workstream} uses unknown execution profile: {profile_name}"
            )
        owner_paths = require_string_list(
            rule.get("owner_paths", []), f"{workstream}.owner_paths"
        )
        for index, owner_path in enumerate(owner_paths):
            validate_repo_path(
                owner_path, f"{workstream}.owner_paths[{index}]", must_exist=True
            )
        guard_checks = require_string_list(
            rule.get("guard_checks", []), f"{workstream}.guard_checks"
        )
        for index, guard in enumerate(guard_checks):
            validate_repo_path(
                guard, f"{workstream}.guard_checks[{index}]", must_exist=True
            )
            if not guard.startswith("scripts/check_"):
                raise ManifestError(
                    f"{workstream} guard is not a check entrypoint: {guard}"
                )
        require_string_list(
            rule.get("negative_controls", []), f"{workstream}.negative_controls"
        )
        require_string_list(
            rule.get("parallel_safe_with", []), f"{workstream}.parallel_safe_with"
        )

    task_ids = {str(task["id"]) for task in tasks}
    task_prerequisites = require_object(
        policy.get("task_prerequisites"), "policy.task_prerequisites"
    )
    for task_id, refs in task_prerequisites.items():
        if task_id not in task_ids:
            raise ManifestError(f"task_prerequisites contains unknown task: {task_id}")
        require_string_list(refs, f"policy.task_prerequisites[{task_id}]")
    return policy


def resolve_reference(
    ref: str,
    *,
    task_ids: Iterable[str],
    tasks_by_workstream: Mapping[str, Sequence[str]],
) -> str:
    task_id_set = set(task_ids)
    if ref in task_id_set:
        return ref
    if ref in tasks_by_workstream:
        candidates = tasks_by_workstream[ref]
        if not candidates:
            raise ManifestError(f"empty workstream reference: {ref}")
        return candidates[-1]
    raise ManifestError(f"unknown prerequisite reference: {ref}")


def validate_dag(tasks: Sequence[Mapping[str, Any]]) -> None:
    graph = {str(task["id"]): list(task["prerequisites"]) for task in tasks}
    visiting = set()
    visited = set()

    def visit(task_id: str, stack: List[str]) -> None:
        if task_id in visiting:
            cycle = stack[stack.index(task_id) :] + [task_id]
            raise ManifestError("roadmap prerequisite cycle: " + " -> ".join(cycle))
        if task_id in visited:
            return
        visiting.add(task_id)
        stack.append(task_id)
        for prerequisite in graph[task_id]:
            visit(prerequisite, stack)
        stack.pop()
        visiting.remove(task_id)
        visited.add(task_id)

    for task_id in graph:
        visit(task_id, [])


def validate_schema(raw: Any) -> Mapping[str, Any]:
    schema = require_object(raw, "schema")
    if schema.get("$schema") != "https://json-schema.org/draft/2020-12/schema":
        raise ManifestError("roadmap execution schema must use JSON Schema 2020-12")
    if (
        schema.get("$id")
        != "https://genesiscode.dev/schemas/roadmap-execution-manifest-v0.1.json"
    ):
        raise ManifestError("roadmap execution schema has unexpected $id")
    if (
        schema.get("type") != "object"
        or schema.get("additionalProperties") is not False
    ):
        raise ManifestError("roadmap execution schema root must be a closed object")
    properties = require_object(schema.get("properties"), "schema.properties")
    kind = require_object(properties.get("kind"), "schema.properties.kind")
    version = require_object(properties.get("version"), "schema.properties.version")
    if kind.get("const") != "genesis/roadmap-execution-manifest-v0.1":
        raise ManifestError("roadmap execution schema kind const drift")
    if version.get("const") != "0.1":
        raise ManifestError("roadmap execution schema version const drift")
    definitions = require_object(schema.get("$defs"), "schema.$defs")
    task = require_object(definitions.get("task"), "schema.$defs.task")
    if task.get("additionalProperties") is not False:
        raise ManifestError("roadmap execution task schema must be closed")
    required = set(
        require_string_list(task.get("required"), "schema.$defs.task.required")
    )
    expected = {
        "id",
        "phase",
        "workstream",
        "source",
        "state",
        "title",
        "objective",
        "prerequisites",
        "unsatisfied_prerequisites",
        "start_ready",
        "risk_class",
        "resource_class",
        "owner_paths",
        "guard_checks",
        "parallel_safe_with",
        "negative_controls",
        "expected_inputs",
        "expected_outputs",
        "rollback",
        "acceptance",
    }
    if required != expected:
        raise ManifestError(
            "roadmap execution task schema required-field drift: "
            f"missing={sorted(expected - required)} extra={sorted(required - expected)}"
        )
    return schema


def build_manifest(
    roadmap_path: Path, policy_path: Path, schema_path: Path
) -> Mapping[str, Any]:
    parsed_tasks = parse_roadmap(roadmap_path)
    policy = validate_policy(
        load_json(policy_path, "roadmap execution policy"), parsed_tasks
    )
    validate_schema(load_json(schema_path, "roadmap execution schema"))

    task_ids = [str(task["id"]) for task in parsed_tasks]
    state_by_id = {str(task["id"]): str(task["state"]) for task in parsed_tasks}
    tasks_by_workstream: Dict[str, List[str]] = {}
    for task in parsed_tasks:
        tasks_by_workstream.setdefault(str(task["workstream"]), []).append(
            str(task["id"])
        )

    task_overrides = require_object(
        policy.get("task_prerequisites"), "policy.task_prerequisites"
    )
    risk_classes = require_object(policy.get("risk_classes"), "policy.risk_classes")
    execution_profiles = require_object(
        policy.get("execution_profiles"), "policy.execution_profiles"
    )
    workstream_rules = require_object(policy.get("workstreams"), "policy.workstreams")
    resolved_tasks: List[Dict[str, Any]] = []

    for task in parsed_tasks:
        task_id = str(task["id"])
        workstream = str(task["workstream"])
        rule = require_object(workstream_rules[workstream], f"workstream {workstream}")
        profile_name = str(rule["profile"])
        execution_profile = require_object(
            execution_profiles[profile_name], f"execution profile {profile_name}"
        )
        refs = list(
            require_string_list(rule.get("start_after"), f"{workstream}.start_after")
        )
        refs.extend(
            require_string_list(
                task_overrides.get(task_id, []), f"prerequisites[{task_id}]"
            )
        )
        siblings = tasks_by_workstream[workstream]
        sibling_index = siblings.index(task_id)
        if bool(rule.get("sequential")) and sibling_index > 0:
            refs.append(siblings[sibling_index - 1])
        prerequisites: List[str] = []
        for ref in refs:
            resolved = resolve_reference(
                ref, task_ids=task_ids, tasks_by_workstream=tasks_by_workstream
            )
            if resolved == task_id:
                raise ManifestError(f"task cannot depend on itself: {task_id}")
            if resolved not in prerequisites:
                prerequisites.append(resolved)

        unsatisfied = [item for item in prerequisites if state_by_id[item] != "done"]
        evidence = copy.deepcopy(task["evidence"])
        if evidence is not None:
            if unsatisfied:
                raise ManifestError(
                    f"done task {task_id} has incomplete prerequisites: {unsatisfied}"
                )
            commands = evidence.get("commands", [])
            if not commands:
                raise ManifestError(f"done task {task_id} has no evidence command")
            for command in commands:
                command_token = command.split()[0]
                if (
                    Path(command_token).name.startswith("update_")
                    or "--update" in command.split()
                ):
                    raise ManifestError(
                        f"done task {task_id} cites mutating evidence command: {command}"
                    )
                if command.startswith("scripts/"):
                    script_path = command.split()[0]
                    validate_repo_path(
                        script_path, f"{task_id}.evidence.commands", must_exist=True
                    )
            if not EVIDENCE_ID_RE.fullmatch(str(evidence.get("input_identity", ""))):
                raise ManifestError(f"done task {task_id} has invalid input identity")

        risk = str(execution_profile["risk_class"])
        risk_rule = require_object(risk_classes[risk], f"risk {risk}")
        negative_controls = (
            require_string_list(
                risk_rule.get("negative_controls"), f"risk {risk}.negative_controls"
            )
            + require_string_list(
                execution_profile.get("negative_controls"),
                f"execution profile {profile_name}.negative_controls",
            )
            + require_string_list(
                rule.get("negative_controls", []), f"{workstream}.negative_controls"
            )
        )
        negative_controls = list(dict.fromkeys(negative_controls))
        owner_paths = list(
            dict.fromkeys(
                require_string_list(
                    execution_profile.get("owner_paths"),
                    f"execution profile {profile_name}.owner_paths",
                )
                + require_string_list(
                    rule.get("owner_paths", []), f"{workstream}.owner_paths"
                )
            )
        )
        guard_checks = list(
            dict.fromkeys(
                require_string_list(
                    execution_profile.get("guard_checks"),
                    f"execution profile {profile_name}.guard_checks",
                )
                + require_string_list(
                    rule.get("guard_checks", []), f"{workstream}.guard_checks"
                )
            )
        )
        resolved_tasks.append(
            {
                "id": task_id,
                "phase": task["phase"],
                "workstream": workstream,
                "source": {"path": "ROADMAP.md", "line": task["source_line"]},
                "state": task["state"],
                "title": task["title"],
                "objective": task["objective"],
                "prerequisites": prerequisites,
                "unsatisfied_prerequisites": unsatisfied,
                "start_ready": task["state"] == "open" and not unsatisfied,
                "risk_class": risk,
                "resource_class": execution_profile["resource_class"],
                "owner_paths": owner_paths,
                "guard_checks": guard_checks,
                "parallel_safe_with": require_string_list(
                    rule.get("parallel_safe_with", []),
                    f"{workstream}.parallel_safe_with",
                ),
                "negative_controls": negative_controls,
                "expected_inputs": {
                    "prerequisite_task_ids": prerequisites,
                    "owner_paths": owner_paths,
                },
                "expected_outputs": {
                    "deliverable": task["objective"],
                    "declared_artifacts": task["declared_artifacts"],
                    "durable_evidence_required": True,
                    "mutable_e0_sufficient": False,
                },
                "rollback": {
                    "automatic": False,
                    "strategy": risk_rule["rollback"],
                    "preserve_failed_evidence": risk in ("high", "critical"),
                },
                "acceptance": {
                    "status": "satisfied" if evidence is not None else "required",
                    "evidence": evidence,
                    "independent_verification_required": risk in ("high", "critical"),
                    "manifest_can_authorize_completion": False,
                },
            }
        )

    validate_dag(resolved_tasks)
    ready = [task["id"] for task in resolved_tasks if task["start_ready"]]
    completed = sum(1 for task in resolved_tasks if task["state"] == "done")
    manifest = {
        "kind": "genesis/roadmap-execution-manifest-v0.1",
        "version": "0.1",
        "audit_date": policy["audit_date"],
        "authority": {
            "roadmap": "ROADMAP.md",
            "policy": "policies/roadmap_execution_v0.1.json",
            "schema": "docs/spec/ROADMAP_EXECUTION_MANIFEST_v0.1.schema.json",
            "completion_rule": "ROADMAP.md done annotation plus independently checked durable evidence",
            "manifest_can_authorize_completion": False,
        },
        "input_identities": {
            "roadmap_sha256": digest(roadmap_path),
            "policy_sha256": digest(policy_path),
            "schema_sha256": digest(schema_path),
        },
        "summary": {
            "task_count": len(resolved_tasks),
            "completed_count": completed,
            "open_count": len(resolved_tasks) - completed,
            "ready_count": len(ready),
        },
        "ready_task_ids": ready,
        "tasks": resolved_tasks,
    }
    validate_manifest(manifest, parsed_tasks=parsed_tasks)
    return manifest


def validate_manifest(
    raw: Any,
    *,
    parsed_tasks: Sequence[Mapping[str, Any]],
    expected_identities: Optional[Mapping[str, str]] = None,
) -> Mapping[str, Any]:
    manifest = require_object(raw, "manifest")
    reject_unknown_fields(
        manifest,
        (
            "kind",
            "version",
            "audit_date",
            "authority",
            "input_identities",
            "summary",
            "ready_task_ids",
            "tasks",
        ),
        "manifest",
    )
    if manifest.get("kind") != "genesis/roadmap-execution-manifest-v0.1":
        raise ManifestError("manifest.kind is invalid")
    if manifest.get("version") != "0.1":
        raise ManifestError("manifest.version is invalid")
    if HOST_PATH_RE.search(json.dumps(manifest, sort_keys=True)):
        raise ManifestError("manifest contains a host-specific absolute path")
    if not DATE_RE.fullmatch(str(manifest.get("audit_date", ""))):
        raise ManifestError("manifest.audit_date must use YYYY-MM-DD")
    authority = require_object(manifest.get("authority"), "manifest.authority")
    reject_unknown_fields(
        authority,
        (
            "roadmap",
            "policy",
            "schema",
            "completion_rule",
            "manifest_can_authorize_completion",
        ),
        "manifest.authority",
    )
    if authority.get("manifest_can_authorize_completion") is not False:
        raise ManifestError("manifest must not authorize task completion")
    identities = require_object(
        manifest.get("input_identities"), "manifest.input_identities"
    )
    if set(identities) != {"roadmap_sha256", "policy_sha256", "schema_sha256"}:
        raise ManifestError("manifest input identity set is invalid")
    for name, value in identities.items():
        if not HASH_RE.fullmatch(str(value)):
            raise ManifestError(f"manifest identity is invalid: {name}")
    if expected_identities is not None and dict(identities) != dict(
        expected_identities
    ):
        raise ManifestError("manifest input identity drift")

    tasks_raw = manifest.get("tasks")
    if not isinstance(tasks_raw, list) or not tasks_raw:
        raise ManifestError("manifest.tasks must be a non-empty array")
    expected_ids = [str(task["id"]) for task in parsed_tasks]
    expected_by_id = {str(task["id"]): task for task in parsed_tasks}
    state_by_id = {str(task["id"]): str(task["state"]) for task in parsed_tasks}
    expected_workstreams = {str(task["workstream"]) for task in parsed_tasks}
    observed_ids: List[str] = []
    for index, raw_task in enumerate(tasks_raw):
        task = require_object(raw_task, f"manifest.tasks[{index}]")
        allowed = {
            "id",
            "phase",
            "workstream",
            "source",
            "state",
            "title",
            "objective",
            "prerequisites",
            "unsatisfied_prerequisites",
            "start_ready",
            "risk_class",
            "resource_class",
            "owner_paths",
            "guard_checks",
            "parallel_safe_with",
            "negative_controls",
            "expected_inputs",
            "expected_outputs",
            "rollback",
            "acceptance",
        }
        reject_unknown_fields(task, allowed, f"manifest.tasks[{index}]")
        task_id = require_string(task.get("id"), f"manifest.tasks[{index}].id")
        if not TASK_ID_RE.fullmatch(task_id) or task_id in observed_ids:
            raise ManifestError(f"invalid or duplicate manifest task id: {task_id}")
        observed_ids.append(task_id)
        expected_task = expected_by_id.get(task_id)
        if expected_task is None:
            raise ManifestError(f"manifest contains unknown roadmap task: {task_id}")
        for field in ("phase", "workstream", "state", "title", "objective"):
            if task.get(field) != expected_task.get(field):
                raise ManifestError(f"{task_id}.{field} drifts from ROADMAP.md")
        source = require_object(task.get("source"), f"{task_id}.source")
        reject_unknown_fields(source, ("path", "line"), f"{task_id}.source")
        if source.get("path") != "ROADMAP.md" or source.get(
            "line"
        ) != expected_task.get("source_line"):
            raise ManifestError(f"{task_id}.source drifts from ROADMAP.md")
        prerequisites = require_string_list(
            task.get("prerequisites"), f"{task_id}.prerequisites"
        )
        for prerequisite in prerequisites:
            if prerequisite not in expected_ids:
                raise ManifestError(
                    f"{task_id} references unknown prerequisite: {prerequisite}"
                )
        unsatisfied = require_string_list(
            task.get("unsatisfied_prerequisites"),
            f"{task_id}.unsatisfied_prerequisites",
        )
        if not set(unsatisfied).issubset(prerequisites):
            raise ManifestError(f"{task_id} has non-prerequisite unsatisfied ids")
        expected_unsatisfied = [
            prerequisite
            for prerequisite in prerequisites
            if state_by_id[prerequisite] != "done"
        ]
        if unsatisfied != expected_unsatisfied:
            raise ManifestError(f"{task_id}.unsatisfied_prerequisites drift")
        state = task.get("state")
        if state not in ("open", "done"):
            raise ManifestError(f"{task_id}.state must be open or done")
        if not isinstance(task.get("start_ready"), bool):
            raise ManifestError(f"{task_id}.start_ready must be boolean")
        expected_ready = state == "open" and not unsatisfied
        if task.get("start_ready") is not expected_ready:
            raise ManifestError(f"{task_id}.start_ready drift")
        risk = task.get("risk_class")
        if risk not in ("low", "medium", "high", "critical"):
            raise ManifestError(f"{task_id}.risk_class is invalid")
        if task.get("resource_class") not in (
            "static",
            "build",
            "benchmark",
            "proof",
            "release",
            "research",
        ):
            raise ManifestError(f"{task_id}.resource_class is invalid")
        owner_paths = require_string_list(
            task.get("owner_paths"), f"{task_id}.owner_paths", non_empty=True
        )
        guard_checks = require_string_list(
            task.get("guard_checks"), f"{task_id}.guard_checks", non_empty=True
        )
        require_string_list(
            task.get("negative_controls"),
            f"{task_id}.negative_controls",
            non_empty=True,
        )
        parallel_safe_with = require_string_list(
            task.get("parallel_safe_with"), f"{task_id}.parallel_safe_with"
        )
        for workstream in parallel_safe_with:
            if workstream not in expected_workstreams:
                raise ManifestError(
                    f"{task_id} references unknown parallel-safe workstream: {workstream}"
                )
        for owner_path in owner_paths:
            validate_repo_path(owner_path, f"{task_id}.owner_paths", must_exist=True)
        for guard in guard_checks:
            validate_repo_path(guard, f"{task_id}.guard_checks", must_exist=True)
            if not guard.startswith("scripts/check_"):
                raise ManifestError(
                    f"{task_id} guard is not a check entrypoint: {guard}"
                )
        expected_inputs = require_object(
            task.get("expected_inputs"), f"{task_id}.expected_inputs"
        )
        reject_unknown_fields(
            expected_inputs,
            ("prerequisite_task_ids", "owner_paths"),
            f"{task_id}.expected_inputs",
        )
        if expected_inputs.get("prerequisite_task_ids") != prerequisites:
            raise ManifestError(f"{task_id}.expected_inputs prerequisite drift")
        if expected_inputs.get("owner_paths") != owner_paths:
            raise ManifestError(f"{task_id}.expected_inputs owner drift")
        expected_outputs = require_object(
            task.get("expected_outputs"), f"{task_id}.expected_outputs"
        )
        reject_unknown_fields(
            expected_outputs,
            (
                "deliverable",
                "declared_artifacts",
                "durable_evidence_required",
                "mutable_e0_sufficient",
            ),
            f"{task_id}.expected_outputs",
        )
        if expected_outputs.get("deliverable") != expected_task.get("objective"):
            raise ManifestError(f"{task_id}.expected_outputs deliverable drift")
        if expected_outputs.get("declared_artifacts") != expected_task.get(
            "declared_artifacts"
        ):
            raise ManifestError(f"{task_id}.expected_outputs artifact drift")
        if expected_outputs.get("durable_evidence_required") is not True:
            raise ManifestError(f"{task_id} does not require durable evidence")
        if expected_outputs.get("mutable_e0_sufficient") is not False:
            raise ManifestError(f"{task_id} incorrectly accepts mutable E0 evidence")
        acceptance = require_object(task.get("acceptance"), f"{task_id}.acceptance")
        reject_unknown_fields(
            acceptance,
            (
                "status",
                "evidence",
                "independent_verification_required",
                "manifest_can_authorize_completion",
            ),
            f"{task_id}.acceptance",
        )
        if not isinstance(acceptance.get("independent_verification_required"), bool):
            raise ManifestError(
                f"{task_id}.acceptance.independent_verification_required must be boolean"
            )
        if acceptance.get("independent_verification_required") is not (
            risk in ("high", "critical")
        ):
            raise ManifestError(f"{task_id}.acceptance independent-verification drift")
        if acceptance.get("manifest_can_authorize_completion") is not False:
            raise ManifestError(f"{task_id} acceptance self-authorizes completion")
        if state == "done":
            if acceptance.get("status") != "satisfied" or not isinstance(
                acceptance.get("evidence"), dict
            ):
                raise ManifestError(f"done task {task_id} lacks satisfied evidence")
            evidence = require_object(
                acceptance.get("evidence"), f"{task_id}.acceptance.evidence"
            )
            reject_unknown_fields(
                evidence,
                ("completed_date", "commands", "input_identity"),
                f"{task_id}.acceptance.evidence",
            )
            if dict(evidence) != expected_task.get("evidence"):
                raise ManifestError(
                    f"{task_id}.acceptance evidence drifts from ROADMAP.md"
                )
        elif (
            acceptance.get("status") != "required"
            or acceptance.get("evidence") is not None
        ):
            raise ManifestError(f"open task {task_id} must retain required acceptance")
        rollback = require_object(task.get("rollback"), f"{task_id}.rollback")
        reject_unknown_fields(
            rollback,
            ("automatic", "strategy", "preserve_failed_evidence"),
            f"{task_id}.rollback",
        )
        if rollback.get("automatic") is not False:
            raise ManifestError(f"{task_id} permits automatic rollback")
        require_string(rollback.get("strategy"), f"{task_id}.rollback.strategy")
        if not isinstance(rollback.get("preserve_failed_evidence"), bool):
            raise ManifestError(f"{task_id}.rollback preserve flag must be boolean")
        if rollback.get("preserve_failed_evidence") is not (
            risk in ("high", "critical")
        ):
            raise ManifestError(f"{task_id}.rollback evidence-preservation drift")

    if observed_ids != expected_ids:
        raise ManifestError(
            "manifest task order/coverage does not exactly match ROADMAP.md"
        )
    validate_dag(tasks_raw)
    summary = require_object(manifest.get("summary"), "manifest.summary")
    done_count = sum(1 for task in tasks_raw if task.get("state") == "done")
    ready_ids = [task["id"] for task in tasks_raw if task.get("start_ready") is True]
    expected_summary = {
        "task_count": len(tasks_raw),
        "completed_count": done_count,
        "open_count": len(tasks_raw) - done_count,
        "ready_count": len(ready_ids),
    }
    if dict(summary) != expected_summary:
        raise ManifestError(
            f"manifest summary drift: expected={expected_summary} observed={dict(summary)}"
        )
    if manifest.get("ready_task_ids") != ready_ids:
        raise ManifestError("manifest ready_task_ids drift")
    return manifest


def canonical_bytes(doc: Mapping[str, Any]) -> bytes:
    return (json.dumps(doc, indent=2, sort_keys=True) + "\n").encode("utf-8")


def run_self_test(roadmap_path: Path, policy_path: Path, schema_path: Path) -> int:
    parsed = parse_roadmap(roadmap_path)
    baseline = build_manifest(roadmap_path, policy_path, schema_path)
    cases: List[Tuple[str, Any]] = []

    duplicate = copy.deepcopy(baseline)
    duplicate["tasks"][1]["id"] = duplicate["tasks"][0]["id"]
    cases.append(("duplicate-task-id", duplicate))

    missing = copy.deepcopy(baseline)
    missing["tasks"].pop()
    cases.append(("missing-roadmap-task", missing))

    stale = copy.deepcopy(baseline)
    stale["input_identities"]["roadmap_sha256"] = "0" * 64
    cases.append(("stale-roadmap-identity", stale))

    self_cycle = copy.deepcopy(baseline)
    self_cycle["tasks"][0]["prerequisites"] = [self_cycle["tasks"][0]["id"]]
    cases.append(("self-cycle", self_cycle))

    open_index = next(
        i for i, task in enumerate(baseline["tasks"]) if task["state"] == "open"
    )
    self_authorized = copy.deepcopy(baseline)
    self_authorized["tasks"][open_index]["acceptance"] = {
        "status": "satisfied",
        "evidence": {
            "commands": ["manifest"],
            "input_identity": "x-sha256:" + "0" * 64,
        },
        "independent_verification_required": False,
        "manifest_can_authorize_completion": True,
    }
    cases.append(("manifest-self-authorization", self_authorized))

    absolute_path = copy.deepcopy(baseline)
    absolute_path["tasks"][0]["owner_paths"][0] = "/tmp/host-specific"
    cases.append(("absolute-owner-path", absolute_path))

    unknown_field = copy.deepcopy(baseline)
    unknown_field["tasks"][0]["trust_me"] = True
    cases.append(("unknown-field", unknown_field))

    summary_drift = copy.deepcopy(baseline)
    summary_drift["summary"]["task_count"] += 1
    cases.append(("summary-drift", summary_drift))

    source_drift = copy.deepcopy(baseline)
    source_drift["tasks"][0]["source"]["line"] += 1
    cases.append(("source-line-drift", source_drift))

    deliverable_drift = copy.deepcopy(baseline)
    deliverable_drift["tasks"][0]["expected_outputs"]["deliverable"] = "trust me"
    cases.append(("deliverable-drift", deliverable_drift))

    readiness_drift = copy.deepcopy(baseline)
    readiness_drift["tasks"][open_index]["start_ready"] = not readiness_drift["tasks"][
        open_index
    ]["start_ready"]
    cases.append(("readiness-drift", readiness_drift))

    guard_bypass = copy.deepcopy(baseline)
    guard_bypass["tasks"][0]["guard_checks"][0] = (
        "scripts/update_capability_status_views.sh"
    )
    cases.append(("non-check-guard", guard_bypass))

    for label, fixture in cases:
        try:
            validate_manifest(
                fixture,
                parsed_tasks=parsed,
                expected_identities=baseline["input_identities"],
            )
        except ManifestError:
            continue
        raise ManifestError(f"self-test accepted adversarial fixture: {label}")

    schema_fixture = copy.deepcopy(load_json(schema_path, "roadmap execution schema"))
    schema_fixture["$defs"]["task"]["required"].remove("acceptance")
    try:
        validate_schema(schema_fixture)
    except ManifestError:
        pass
    else:
        raise ManifestError("self-test accepted weakened schema fixture")

    negative_controls = len(cases) + 1
    print(
        "roadmap-execution-manifest-self-test: ok "
        f"(negative_controls={negative_controls})"
    )
    return negative_controls


def main(argv: Sequence[str]) -> int:
    parser = argparse.ArgumentParser()
    mode = parser.add_mutually_exclusive_group(required=True)
    mode.add_argument("--check", action="store_true")
    mode.add_argument("--update", action="store_true")
    mode.add_argument("--render", action="store_true")
    mode.add_argument("--self-test", action="store_true")
    parser.add_argument("--roadmap", type=Path, default=DEFAULT_ROADMAP)
    parser.add_argument("--policy", type=Path, default=DEFAULT_POLICY)
    parser.add_argument("--schema", type=Path, default=DEFAULT_SCHEMA)
    parser.add_argument("--manifest", type=Path, default=DEFAULT_MANIFEST)
    parser.add_argument("--output", type=Path)
    args = parser.parse_args(argv)

    roadmap_path = args.roadmap.resolve()
    policy_path = args.policy.resolve()
    schema_path = args.schema.resolve()
    manifest_path = args.manifest.resolve()
    try:
        if args.self_test:
            run_self_test(roadmap_path, policy_path, schema_path)
            return 0
        rendered = build_manifest(roadmap_path, policy_path, schema_path)
        rendered_bytes = canonical_bytes(rendered)
        if args.render:
            if args.output is None:
                raise ManifestError("--render requires --output")
            output = args.output.resolve()
            output.parent.mkdir(parents=True, exist_ok=True)
            output.write_bytes(rendered_bytes)
            print(f"roadmap-execution-manifest: rendered {display_path(output)}")
        elif args.update:
            manifest_path.parent.mkdir(parents=True, exist_ok=True)
            manifest_path.write_bytes(rendered_bytes)
            print(f"roadmap-execution-manifest: updated {display_path(manifest_path)}")
        else:
            observed = load_json(manifest_path, "roadmap execution manifest")
            parsed = parse_roadmap(roadmap_path)
            validate_manifest(
                observed,
                parsed_tasks=parsed,
                expected_identities=rendered["input_identities"],
            )
            if canonical_bytes(observed) != rendered_bytes:
                raise ManifestError(
                    "manifest drift; run bash scripts/update_roadmap_execution_manifest.sh"
                )
            print(
                "roadmap-execution-manifest: ok "
                f"(tasks={rendered['summary']['task_count']} "
                f"done={rendered['summary']['completed_count']} "
                f"ready={rendered['summary']['ready_count']})"
            )
    except ManifestError as exc:
        print(f"roadmap-execution-manifest: {exc}", file=sys.stderr)
        return 1
    return 0


if __name__ == "__main__":
    raise SystemExit(main(sys.argv[1:]))
