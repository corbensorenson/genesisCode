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
RELEASE_LANE_IDS = {
    "genesiscode-core",
    "genesisbench-trust",
    "foundry-calibration",
    "genesis-model-readiness",
    "genesis-model-release",
}
FROZEN_PROGRAM_CONCEPTS = [
    "GenesisCode",
    "GenesisBench",
    "GenesisChallenge",
    "Genesis Foundry",
    "Genesis Model",
]
FORBIDDEN_SCOPE_ADDITION_CLASSES = [
    "product",
    "program",
    "benchmark-track",
    "research-lane",
    "milestone-family",
    "task-family",
    "governed-gate-family",
]
PARALLEL_LANE_REQUIRED_PHRASES = {
    "read-only-selfhost-assurance": [
        "cannot modify repository files",
        "performs no target-model inference, benchmark custody or commissioning, result publication, Foundry implementation",
        "cannot authorize completion",
    ],
    "model-interface-portability-canary": [
        "no GenesisBench task, private payload, scorer",
        "cannot modify repository files",
        "creates no benchmark attempt, score, cohort, rank, result",
    ],
}
VALIDATION_READINESS_ORDER = [
    "contract",
    "focused",
    "integration",
    "assurance",
    "release",
]
VALIDATION_CAMPAIGN_FIELDS = [
    "decision",
    "subject-readiness",
    "independent-variable",
    "observation-reuse",
    "resource-budget",
    "stopping-rule",
    "terminal-artifact",
]


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
            "execution_frontier",
            "release_lane_contracts",
            "risk_classes",
            "resource_classes",
            "execution_profiles",
            "workstreams",
            "task_execution_profiles",
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
    task_execution_profiles = require_object(
        policy.get("task_execution_profiles"), "policy.task_execution_profiles"
    )
    for task_id, raw_profile_name in task_execution_profiles.items():
        if task_id not in task_ids:
            raise ManifestError(
                f"task_execution_profiles contains unknown task: {task_id}"
            )
        profile_name = require_string(
            raw_profile_name, f"policy.task_execution_profiles[{task_id}]"
        )
        if profile_name not in execution_profiles:
            raise ManifestError(
                f"{task_id} uses unknown task execution profile: {profile_name}"
            )

    frontier = require_object(
        policy.get("execution_frontier"), "policy.execution_frontier"
    )
    reject_unknown_fields(
        frontier,
        (
            "wip_limit",
            "scope_freeze",
            "ordered_task_ids",
            "rationale",
            "task_context",
            "validation_economy",
            "allowed_parallel_lanes",
        ),
        "policy.execution_frontier",
    )
    wip_limit = frontier.get("wip_limit")
    if isinstance(wip_limit, bool) or not isinstance(wip_limit, int):
        raise ManifestError("execution_frontier.wip_limit must be an integer")
    if wip_limit < 1 or wip_limit > 3:
        raise ManifestError("execution_frontier.wip_limit must be between 1 and 3")
    scope_freeze = require_object(
        frontier.get("scope_freeze"), "execution_frontier.scope_freeze"
    )
    reject_unknown_fields(
        scope_freeze,
        (
            "until_task_id",
            "frozen_program_concepts",
            "forbidden_addition_classes",
            "exception_rule",
        ),
        "execution_frontier.scope_freeze",
    )
    freeze_task_id = require_string(
        scope_freeze.get("until_task_id"),
        "execution_frontier.scope_freeze.until_task_id",
    )
    if freeze_task_id not in task_ids:
        raise ManifestError(
            f"execution_frontier.scope_freeze names unknown task: {freeze_task_id}"
        )
    frozen_concepts = require_string_list(
        scope_freeze.get("frozen_program_concepts"),
        "execution_frontier.scope_freeze.frozen_program_concepts",
        non_empty=True,
    )
    if frozen_concepts != FROZEN_PROGRAM_CONCEPTS:
        raise ManifestError("execution_frontier.scope_freeze concept set drift")
    forbidden_additions = require_string_list(
        scope_freeze.get("forbidden_addition_classes"),
        "execution_frontier.scope_freeze.forbidden_addition_classes",
        non_empty=True,
    )
    if forbidden_additions != FORBIDDEN_SCOPE_ADDITION_CLASSES:
        raise ManifestError("execution_frontier.scope_freeze addition-class drift")
    require_string(
        scope_freeze.get("exception_rule"),
        "execution_frontier.scope_freeze.exception_rule",
    )
    frontier_ids = require_string_list(
        frontier.get("ordered_task_ids"),
        "execution_frontier.ordered_task_ids",
        non_empty=True,
    )
    unknown_frontier_ids = sorted(set(frontier_ids) - task_ids)
    if unknown_frontier_ids:
        raise ManifestError(
            "execution_frontier contains unknown tasks: "
            + ", ".join(unknown_frontier_ids)
        )
    require_string(frontier.get("rationale"), "execution_frontier.rationale")
    task_context = require_object(
        frontier.get("task_context"), "execution_frontier.task_context"
    )
    if set(task_context) != set(frontier_ids):
        missing = sorted(set(frontier_ids) - set(task_context))
        extra = sorted(set(task_context) - set(frontier_ids))
        raise ManifestError(
            f"execution_frontier task context drift: missing={missing} extra={extra}"
        )
    for task_id, raw_context in task_context.items():
        context = require_object(
            raw_context, f"execution_frontier.task_context[{task_id}]"
        )
        reject_unknown_fields(
            context,
            ("product_lanes", "milestones", "nonclaims"),
            f"execution_frontier.task_context[{task_id}]",
        )
        require_string_list(
            context.get("product_lanes"),
            f"execution_frontier.task_context[{task_id}].product_lanes",
            non_empty=True,
        )
        require_string_list(
            context.get("milestones"),
            f"execution_frontier.task_context[{task_id}].milestones",
            non_empty=True,
        )
        require_string_list(
            context.get("nonclaims"),
            f"execution_frontier.task_context[{task_id}].nonclaims",
            non_empty=True,
        )
    validation_economy = require_object(
        frontier.get("validation_economy"),
        "execution_frontier.validation_economy",
    )
    reject_unknown_fields(
        validation_economy,
        (
            "identical_success_limit_per_exact_identity",
            "additional_identical_run_condition",
            "release_calibration_task_id",
            "whole_profile_sampling",
            "long_running_supervision",
            "subject_readiness_order",
            "required_campaign_fields",
        ),
        "execution_frontier.validation_economy",
    )
    if validation_economy.get("identical_success_limit_per_exact_identity") != 1:
        raise ManifestError("validation economy must allow one identical development success")
    if (
        validation_economy.get("additional_identical_run_condition")
        != "recorded-flake-or-nondeterminism-hypothesis"
    ):
        raise ManifestError("validation economy duplicate-run condition drift")
    calibration_task_id = require_string(
        validation_economy.get("release_calibration_task_id"),
        "execution_frontier.validation_economy.release_calibration_task_id",
    )
    if calibration_task_id != "R9.1.c" or calibration_task_id not in task_ids:
        raise ManifestError("validation economy release calibration task drift")
    if (
        validation_economy.get("whole_profile_sampling")
        != "one-outer-invocation-after-inner-harness-pass"
        or validation_economy.get("long_running_supervision")
        != "autonomous-state-transitions-only"
    ):
        raise ManifestError("validation economy execution contract drift")
    if require_string_list(
        validation_economy.get("subject_readiness_order"),
        "execution_frontier.validation_economy.subject_readiness_order",
        non_empty=True,
    ) != VALIDATION_READINESS_ORDER:
        raise ManifestError("validation economy readiness order drift")
    if require_string_list(
        validation_economy.get("required_campaign_fields"),
        "execution_frontier.validation_economy.required_campaign_fields",
        non_empty=True,
    ) != VALIDATION_CAMPAIGN_FIELDS:
        raise ManifestError("validation economy campaign field drift")
    parallel_lanes = frontier.get("allowed_parallel_lanes")
    if not isinstance(parallel_lanes, list) or not parallel_lanes:
        raise ManifestError(
            "execution_frontier.allowed_parallel_lanes must be a non-empty array"
        )
    observed_parallel_lane_ids: set[str] = set()
    for index, raw_lane in enumerate(parallel_lanes):
        lane = require_object(
            raw_lane, f"execution_frontier.allowed_parallel_lanes[{index}]"
        )
        reject_unknown_fields(
            lane,
            ("id", "description", "conditions"),
            f"execution_frontier.allowed_parallel_lanes[{index}]",
        )
        lane_id = require_string(
            lane.get("id"), f"execution_frontier.allowed_parallel_lanes[{index}].id"
        )
        if lane_id in observed_parallel_lane_ids:
            raise ManifestError(f"duplicate allowed parallel lane: {lane_id}")
        observed_parallel_lane_ids.add(lane_id)
        require_string(
            lane.get("description"),
            f"execution_frontier.allowed_parallel_lanes[{index}].description",
        )
        require_string_list(
            lane.get("conditions"),
            f"execution_frontier.allowed_parallel_lanes[{index}].conditions",
            non_empty=True,
        )
    if observed_parallel_lane_ids != set(PARALLEL_LANE_REQUIRED_PHRASES):
        raise ManifestError("execution_frontier allowed parallel lane set drift")
    parallel_lane_by_id = {
        str(lane["id"]): require_object(lane, "allowed parallel lane")
        for lane in parallel_lanes
    }
    for lane_id, required_phrases in PARALLEL_LANE_REQUIRED_PHRASES.items():
        contract = " ".join(
            require_string_list(
                parallel_lane_by_id[lane_id].get("conditions"),
                f"execution_frontier.allowed_parallel_lanes[{lane_id}].conditions",
                non_empty=True,
            )
        )
        for phrase in required_phrases:
            if phrase not in contract:
                raise ManifestError(
                    f"execution_frontier parallel lane {lane_id} lost {phrase!r}"
                )
    task_prerequisites = require_object(
        policy.get("task_prerequisites"), "policy.task_prerequisites"
    )
    for task_id, refs in task_prerequisites.items():
        if task_id not in task_ids:
            raise ManifestError(f"task_prerequisites contains unknown task: {task_id}")
        require_string_list(refs, f"policy.task_prerequisites[{task_id}]")
    validate_release_lane_contracts(
        policy.get("release_lane_contracts"),
        task_ids=task_ids,
        workstream_ids=task_workstreams,
    )
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


def release_lane_task_matches_forbidden_selector(
    task_id: str,
    contract: Mapping[str, Any],
    workstream_by_task: Mapping[str, str],
) -> bool:
    return (
        task_id in contract["forbidden_task_ids"]
        or workstream_by_task[task_id] in contract["forbidden_workstreams"]
        or any(
            task_id.startswith(prefix)
            for prefix in contract["forbidden_task_prefixes"]
        )
    )


def release_lane_task_is_forbidden(
    task_id: str,
    contract: Mapping[str, Any],
    workstream_by_task: Mapping[str, str],
) -> bool:
    return task_id not in contract["allowed_task_ids"] and (
        release_lane_task_matches_forbidden_selector(
            task_id, contract, workstream_by_task
        )
    )


def validate_release_lane_contracts(
    raw: Any,
    *,
    task_ids: Iterable[str],
    workstream_ids: Iterable[str],
) -> Mapping[str, Any]:
    contracts = require_object(raw, "policy.release_lane_contracts")
    if set(contracts) != RELEASE_LANE_IDS:
        raise ManifestError(
            "release-lane contract coverage drift: "
            f"missing={sorted(RELEASE_LANE_IDS - set(contracts))} "
            f"extra={sorted(set(contracts) - RELEASE_LANE_IDS)}"
        )
    known_tasks = set(task_ids)
    known_workstreams = set(workstream_ids)
    workstream_by_task = {task_id: task_workstream(task_id) for task_id in known_tasks}
    for lane_id, raw_contract in contracts.items():
        contract = require_object(
            raw_contract, f"policy.release_lane_contracts[{lane_id}]"
        )
        reject_unknown_fields(
            contract,
            (
                "root_task_id",
                "required_ancestor_task_ids",
                "forbidden_task_ids",
                "forbidden_workstreams",
                "forbidden_task_prefixes",
                "allowed_task_ids",
            ),
            f"policy.release_lane_contracts[{lane_id}]",
        )
        root_task_id = require_string(
            contract.get("root_task_id"), f"release lane {lane_id}.root_task_id"
        )
        required = require_string_list(
            contract.get("required_ancestor_task_ids"),
            f"release lane {lane_id}.required_ancestor_task_ids",
        )
        forbidden_tasks = require_string_list(
            contract.get("forbidden_task_ids"),
            f"release lane {lane_id}.forbidden_task_ids",
            non_empty=True,
        )
        forbidden_workstreams = require_string_list(
            contract.get("forbidden_workstreams"),
            f"release lane {lane_id}.forbidden_workstreams",
            non_empty=True,
        )
        forbidden_prefixes = require_string_list(
            contract.get("forbidden_task_prefixes"),
            f"release lane {lane_id}.forbidden_task_prefixes",
        )
        allowed = require_string_list(
            contract.get("allowed_task_ids"),
            f"release lane {lane_id}.allowed_task_ids",
        )
        referenced_tasks = set(required + forbidden_tasks + allowed + [root_task_id])
        unknown_tasks = sorted(referenced_tasks - known_tasks)
        if unknown_tasks:
            raise ManifestError(
                f"release lane {lane_id} references unknown tasks: "
                + ", ".join(unknown_tasks)
            )
        unknown_workstreams = sorted(set(forbidden_workstreams) - known_workstreams)
        if unknown_workstreams:
            raise ManifestError(
                f"release lane {lane_id} references unknown workstreams: "
                + ", ".join(unknown_workstreams)
            )
        for prefix in forbidden_prefixes:
            if not any(task_id.startswith(prefix) for task_id in known_tasks):
                raise ManifestError(
                    f"release lane {lane_id} has a stale task prefix: {prefix}"
                )
        for task_id in allowed:
            if not release_lane_task_matches_forbidden_selector(
                task_id, contract, workstream_by_task
            ):
                raise ManifestError(
                    f"release lane {lane_id} allows a task not selected as forbidden: "
                    f"{task_id}"
                )
        if release_lane_task_is_forbidden(
            root_task_id, contract, workstream_by_task
        ):
            raise ManifestError(
                f"release lane {lane_id} forbids its own root: {root_task_id}"
            )
        impossible_required = sorted(
            task_id
            for task_id in required
            if release_lane_task_is_forbidden(
                task_id, contract, workstream_by_task
            )
        )
        if impossible_required:
            raise ManifestError(
                f"release lane {lane_id} both requires and forbids: "
                + ", ".join(impossible_required)
            )
    return contracts


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


def transitive_prerequisites(
    tasks: Sequence[Mapping[str, Any]], root_id: str
) -> set[str]:
    graph = {str(task["id"]): list(task["prerequisites"]) for task in tasks}
    if root_id not in graph:
        raise ManifestError(f"release-lane root is missing: {root_id}")
    ancestors: set[str] = set()
    pending = [root_id]
    while pending:
        task_id = pending.pop()
        for prerequisite in graph[task_id]:
            if prerequisite not in ancestors:
                ancestors.add(prerequisite)
                pending.append(prerequisite)
    return ancestors


def validate_release_lane_isolation(
    tasks: Sequence[Mapping[str, Any]], contracts: Mapping[str, Any]
) -> None:
    workstream_by_task = {
        str(task["id"]): str(task["workstream"]) for task in tasks
    }
    for lane_id, raw_contract in contracts.items():
        contract = require_object(raw_contract, f"release lane {lane_id}")
        root_task_id = str(contract["root_task_id"])
        ancestors = transitive_prerequisites(tasks, root_task_id)
        required = set(contract["required_ancestor_task_ids"])
        missing = required - ancestors
        if missing:
            raise ManifestError(
                f"release lane {lane_id} is missing required ancestors: "
                + ", ".join(sorted(missing))
            )
        forbidden = {
            task_id
            for task_id in ancestors
            if release_lane_task_is_forbidden(
                task_id, contract, workstream_by_task
            )
        }
        if forbidden:
            raise ManifestError(
                f"release lane {lane_id} has forbidden ancestors: "
                + ", ".join(sorted(forbidden))
            )


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
    task_execution_profiles = require_object(
        policy.get("task_execution_profiles"), "policy.task_execution_profiles"
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
        profile_name = str(task_execution_profiles.get(task_id, rule["profile"]))
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
    release_lane_contracts = require_object(
        policy.get("release_lane_contracts"), "policy.release_lane_contracts"
    )
    validate_release_lane_isolation(resolved_tasks, release_lane_contracts)
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
    validate_manifest(
        manifest,
        parsed_tasks=parsed_tasks,
        release_lane_contracts=release_lane_contracts,
    )
    return manifest


def validate_manifest(
    raw: Any,
    *,
    parsed_tasks: Sequence[Mapping[str, Any]],
    release_lane_contracts: Mapping[str, Any],
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
    validate_release_lane_isolation(tasks_raw, release_lane_contracts)
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


def resolve_frontier_candidates(
    manifest: Mapping[str, Any],
    ordered_ids: Sequence[str],
    workstreams: Mapping[str, Any],
) -> List[Tuple[str, str]]:
    """Resolve each sequential anchor to its first unfinished task."""
    task_list = list(manifest["tasks"])
    tasks = {str(task["id"]): task for task in task_list}
    by_workstream: dict[str, List[Mapping[str, Any]]] = {}
    for task in task_list:
        by_workstream.setdefault(str(task["workstream"]), []).append(task)

    resolved: List[Tuple[str, str]] = []
    seen: set[str] = set()
    for anchor_id in ordered_ids:
        anchor = tasks[anchor_id]
        candidate = anchor
        if anchor["state"] == "done":
            workstream = str(anchor["workstream"])
            rule = require_object(
                workstreams.get(workstream), f"policy.workstreams[{workstream}]"
            )
            candidate = None
            if rule.get("sequential") is True:
                members = by_workstream[workstream]
                anchor_index = next(
                    index
                    for index, member in enumerate(members)
                    if member["id"] == anchor_id
                )
                candidate = next(
                    (
                        member
                        for member in members[anchor_index + 1 :]
                        if member["state"] == "open"
                    ),
                    None,
                )
        if candidate is None:
            continue
        candidate_id = str(candidate["id"])
        if candidate_id not in seen:
            resolved.append((candidate_id, anchor_id))
            seen.add(candidate_id)
    return resolved


def build_execution_slice(
    manifest: Mapping[str, Any], policy_path: Path
) -> Mapping[str, Any]:
    policy = require_object(load_json(policy_path, "roadmap execution policy"), "policy")
    frontier = require_object(
        policy.get("execution_frontier"), "policy.execution_frontier"
    )
    ordered_ids = require_string_list(
        frontier.get("ordered_task_ids"),
        "execution_frontier.ordered_task_ids",
        non_empty=True,
    )
    wip_limit = frontier.get("wip_limit")
    if isinstance(wip_limit, bool) or not isinstance(wip_limit, int):
        raise ManifestError("execution_frontier.wip_limit must be an integer")
    task_context = require_object(
        frontier.get("task_context"), "execution_frontier.task_context"
    )
    tasks = require_object(
        {str(task["id"]): task for task in manifest["tasks"]}, "task index"
    )
    workstreams = require_object(policy.get("workstreams"), "policy.workstreams")
    resolved = resolve_frontier_candidates(manifest, ordered_ids, workstreams)
    open_ids = [task_id for task_id, _ in resolved]
    context_anchors = {task_id: anchor_id for task_id, anchor_id in resolved}
    focused_ids = [
        task_id for task_id in open_ids if tasks[task_id]["start_ready"] is True
    ][:wip_limit]
    focus_tasks = []
    for task_id in focused_ids:
        task = tasks[task_id]
        anchor_id = context_anchors[task_id]
        context = require_object(
            task_context[anchor_id], f"execution_frontier.task_context[{anchor_id}]"
        )
        focus_tasks.append(
            {
                "id": task["id"],
                "title": task["title"],
                "phase": task["phase"],
                "workstream": task["workstream"],
                "product_lanes": context["product_lanes"],
                "milestones": context["milestones"],
                "start_ready": task["start_ready"],
                "prerequisites": task["prerequisites"],
                "unsatisfied_prerequisites": task["unsatisfied_prerequisites"],
                "risk_class": task["risk_class"],
                "resource_class": task["resource_class"],
                "owner_paths": task["owner_paths"],
                "guard_checks": task["guard_checks"],
                "parallel_safe_with": task["parallel_safe_with"],
                "negative_controls": task["negative_controls"],
                "nonclaims": context["nonclaims"],
                "expected_outputs": task["expected_outputs"],
                "acceptance": task["acceptance"],
                "rollback": task["rollback"],
                "source": task["source"],
            }
        )
    ready_ids = list(manifest["ready_task_ids"])
    queued_ids = [task_id for task_id in open_ids if task_id not in focused_ids]
    queued_tasks = []
    for task_id in queued_ids:
        task = tasks[task_id]
        anchor_id = context_anchors[task_id]
        context = require_object(
            task_context[anchor_id], f"execution_frontier.task_context[{anchor_id}]"
        )
        queued_tasks.append(
            {
                "id": task_id,
                "title": task["title"],
                "product_lanes": context["product_lanes"],
                "milestones": context["milestones"],
                "start_ready": task["start_ready"],
                "unsatisfied_prerequisites": task["unsatisfied_prerequisites"],
                "nonclaims": context["nonclaims"],
            }
        )
    return {
        "kind": "genesis/roadmap-execution-slice-v0.1",
        "version": "0.1",
        "authority": {
            "roadmap": manifest["authority"]["roadmap"],
            "policy": manifest["authority"]["policy"],
            "derived_view_only": True,
        },
        "input_identities": manifest["input_identities"],
        "wip_limit": wip_limit,
        "scope_freeze": frontier["scope_freeze"],
        "validation_economy": frontier["validation_economy"],
        "rationale": require_string(
            frontier.get("rationale"), "execution_frontier.rationale"
        ),
        "allowed_parallel_lanes": frontier["allowed_parallel_lanes"],
        "focus_tasks": focus_tasks,
        "queued_task_ids": queued_ids,
        "queued_tasks": queued_tasks,
        "ready_but_deprioritized_task_ids": [
            task_id for task_id in ready_ids if task_id not in focused_ids
        ],
    }


def build_start_readiness_report(
    manifest: Mapping[str, Any], policy_path: Path
) -> Mapping[str, Any]:
    """Render all graph-ready work without granting selection authority."""
    execution_slice = build_execution_slice(manifest, policy_path)
    tasks = {str(task["id"]): task for task in manifest["tasks"]}
    frontier_focus_ids = [
        str(task["id"]) for task in execution_slice["focus_tasks"]
    ]
    selected_ready_ids = [
        task_id
        for task_id in frontier_focus_ids
        if tasks[task_id]["start_ready"] is True
    ]
    ready_tasks = []
    for task_id in manifest["ready_task_ids"]:
        task = tasks[str(task_id)]
        selected = task_id in selected_ready_ids
        ready_tasks.append(
            {
                "id": task["id"],
                "title": task["title"],
                "phase": task["phase"],
                "workstream": task["workstream"],
                "riskClass": task["risk_class"],
                "resourceClass": task["resource_class"],
                "prerequisites": task["prerequisites"],
                "selectedByFrontier": selected,
                "selectionDisposition": (
                    "selected" if selected else "ready-but-deprioritized"
                ),
                "deprioritizedReason": (
                    None if selected else "wip-limit-and-frontier-priority"
                ),
                "source": task["source"],
            }
        )
    return {
        "kind": "genesis/roadmap-start-readiness-v0.1",
        "version": "0.1",
        "authority": {
            "roadmap": manifest["authority"]["roadmap"],
            "policy": manifest["authority"]["policy"],
            "derivedViewOnly": True,
            "selector": "--slice",
        },
        "inputIdentities": manifest["input_identities"],
        "wipLimit": execution_slice["wip_limit"],
        "openTaskCount": manifest["summary"]["open_count"],
        "startReadyTaskCount": len(ready_tasks),
        "frontierFocusTaskIds": frontier_focus_ids,
        "selectedReadyTaskIds": selected_ready_ids,
        "startReadyTasks": ready_tasks,
        "nonclaims": [
            "Graph readiness does not select work or widen repository-changing WIP.",
            "This derived report cannot authorize start, completion, promotion, capability, signing, or release.",
        ],
    }


def run_self_test(roadmap_path: Path, policy_path: Path, schema_path: Path) -> int:
    parsed = parse_roadmap(roadmap_path)
    policy = validate_policy(
        load_json(policy_path, "roadmap execution policy"), parsed
    )
    release_lane_contracts = require_object(
        policy.get("release_lane_contracts"), "policy.release_lane_contracts"
    )
    baseline = build_manifest(roadmap_path, policy_path, schema_path)
    readiness = build_start_readiness_report(baseline, policy_path)
    if set(readiness) != {
        "kind",
        "version",
        "authority",
        "inputIdentities",
        "wipLimit",
        "openTaskCount",
        "startReadyTaskCount",
        "frontierFocusTaskIds",
        "selectedReadyTaskIds",
        "startReadyTasks",
        "nonclaims",
    }:
        raise ManifestError("self-test found start-readiness report field drift")
    readiness_task_fields = {
        "id",
        "title",
        "phase",
        "workstream",
        "riskClass",
        "resourceClass",
        "prerequisites",
        "selectedByFrontier",
        "selectionDisposition",
        "deprioritizedReason",
        "source",
    }
    if (
        readiness["kind"] != "genesis/roadmap-start-readiness-v0.1"
        or readiness["authority"]["derivedViewOnly"] is not True
        or readiness["authority"]["selector"] != "--slice"
        or [task["id"] for task in readiness["startReadyTasks"]]
        != baseline["ready_task_ids"]
        or any(
            set(task) != readiness_task_fields
            for task in readiness["startReadyTasks"]
        )
        or any(
            task["selectedByFrontier"]
            != (task["id"] in readiness["selectedReadyTaskIds"])
            for task in readiness["startReadyTasks"]
        )
    ):
        raise ManifestError("self-test found start-readiness derivation drift")
    sequential_probe = copy.deepcopy(baseline)
    probe_by_id = {str(task["id"]): task for task in sequential_probe["tasks"]}
    probe_by_id["R1.5.a"]["state"] = "done"
    probe_by_id["R1.5.b"]["state"] = "done"
    probe_by_id["R1.5.c"]["state"] = "open"
    resolved_probe = resolve_frontier_candidates(
        sequential_probe,
        ["R1.5.a"],
        require_object(policy.get("workstreams"), "policy.workstreams"),
    )
    if resolved_probe != [("R1.5.c", "R1.5.a")]:
        raise ManifestError(
            "self-test failed to advance a completed sequential frontier anchor"
        )
    blocked_frontier_probe = copy.deepcopy(baseline)
    blocked_probe_by_id = {
        str(task["id"]): task for task in blocked_frontier_probe["tasks"]
    }
    blocked_probe_by_id["R2.2.f"]["start_ready"] = False
    if [
        task["id"]
        for task in build_execution_slice(blocked_frontier_probe, policy_path)[
            "focus_tasks"
        ]
    ] != ["R4.1.a"]:
        raise ManifestError("self-test let a blocked frontier anchor stall ready work")
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
                release_lane_contracts=release_lane_contracts,
                expected_identities=baseline["input_identities"],
            )
        except ManifestError:
            continue
        raise ManifestError(f"self-test accepted adversarial fixture: {label}")

    lane_cases: List[Tuple[str, Any]] = []

    core_platform_leak = copy.deepcopy(baseline)
    next(
        task for task in core_platform_leak["tasks"] if task["id"] == "R9.4.f"
    )["prerequisites"].append("R8.3.e")
    lane_cases.append(("core-platform-leak", core_platform_leak))

    core_data_ml_leak = copy.deepcopy(baseline)
    next(
        task for task in core_data_ml_leak["tasks"] if task["id"] == "R9.4.f"
    )["prerequisites"].append("R5.7.e")
    lane_cases.append(("core-data-ml-leak", core_data_ml_leak))

    core_benchmark_leak = copy.deepcopy(baseline)
    next(
        task for task in core_benchmark_leak["tasks"] if task["id"] == "R9.4.f"
    )["prerequisites"].append("R8.5.e")
    lane_cases.append(("core-benchmark-leak", core_benchmark_leak))

    bench_platform_leak = copy.deepcopy(baseline)
    next(
        task for task in bench_platform_leak["tasks"] if task["id"] == "R8.5.s"
    )["prerequisites"].append("R8.3.e")
    lane_cases.append(("bench-platform-leak", bench_platform_leak))

    bench_generator_gate_leak = copy.deepcopy(baseline)
    next(
        task
        for task in bench_generator_gate_leak["tasks"]
        if task["id"] == "R8.5.s"
    )["prerequisites"].append("R7.1.f")
    lane_cases.append(("bench-generator-gate-leak", bench_generator_gate_leak))

    bench_model_leak = copy.deepcopy(baseline)
    next(
        task for task in bench_model_leak["tasks"] if task["id"] == "R8.5.s"
    )["prerequisites"].append("R8.5.h")
    lane_cases.append(("bench-model-leak", bench_model_leak))

    foundry_platform_leak = copy.deepcopy(baseline)
    next(
        task for task in foundry_platform_leak["tasks"] if task["id"] == "F2.q"
    )["prerequisites"].append("R8.3.e")
    lane_cases.append(("foundry-platform-leak", foundry_platform_leak))

    foundry_hardware_gate_leak = copy.deepcopy(baseline)
    next(
        task
        for task in foundry_hardware_gate_leak["tasks"]
        if task["id"] == "F2.q"
    )["prerequisites"].append("R7.3.f")
    lane_cases.append(("foundry-hardware-gate-leak", foundry_hardware_gate_leak))

    foundry_model_leak = copy.deepcopy(baseline)
    next(
        task for task in foundry_model_leak["tasks"] if task["id"] == "F2.q"
    )["prerequisites"].append("R8.5.h")
    lane_cases.append(("foundry-model-leak", foundry_model_leak))

    model_gate_bypass = copy.deepcopy(baseline)
    model_gate = next(
        task for task in model_gate_bypass["tasks"] if task["id"] == "R8.5.t"
    )
    model_gate["prerequisites"].remove("R8.5.i")
    lane_cases.append(("model-gate-bypass", model_gate_bypass))

    model_platform_leak = copy.deepcopy(baseline)
    next(
        task for task in model_platform_leak["tasks"] if task["id"] == "R8.5.r"
    )["prerequisites"].append("R8.3.e")
    lane_cases.append(("model-platform-leak", model_platform_leak))

    model_device_soak_leak = copy.deepcopy(baseline)
    next(
        task for task in model_device_soak_leak["tasks"] if task["id"] == "R8.5.r"
    )["prerequisites"].append("R7.4.d")
    lane_cases.append(("model-device-soak-leak", model_device_soak_leak))

    model_foundry_expansion_leak = copy.deepcopy(baseline)
    next(
        task
        for task in model_foundry_expansion_leak["tasks"]
        if task["id"] == "R8.5.r"
    )["prerequisites"].append("F2.r")
    lane_cases.append(("model-foundry-expansion-leak", model_foundry_expansion_leak))

    for label, fixture in lane_cases:
        try:
            validate_release_lane_isolation(
                fixture["tasks"], release_lane_contracts
            )
        except ManifestError:
            continue
        raise ManifestError(f"self-test accepted release-lane fixture: {label}")

    schema_fixture = copy.deepcopy(load_json(schema_path, "roadmap execution schema"))
    schema_fixture["$defs"]["task"]["required"].remove("acceptance")
    try:
        validate_schema(schema_fixture)
    except ManifestError:
        pass
    else:
        raise ManifestError("self-test accepted weakened schema fixture")

    negative_controls = len(cases) + len(lane_cases) + 1
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
    mode.add_argument("--slice", action="store_true")
    mode.add_argument("--ready", action="store_true")
    mode.add_argument("--explain", metavar="TASK_ID")
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
        if args.slice:
            sys.stdout.buffer.write(
                canonical_bytes(build_execution_slice(rendered, policy_path))
            )
        elif args.ready:
            sys.stdout.buffer.write(
                canonical_bytes(build_start_readiness_report(rendered, policy_path))
            )
        elif args.explain is not None:
            task = next(
                (task for task in rendered["tasks"] if task["id"] == args.explain),
                None,
            )
            if task is None:
                raise ManifestError(f"unknown roadmap task: {args.explain}")
            sys.stdout.buffer.write(canonical_bytes(task))
        elif args.render:
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
            policy = validate_policy(
                load_json(policy_path, "roadmap execution policy"), parsed
            )
            release_lane_contracts = require_object(
                policy.get("release_lane_contracts"),
                "policy.release_lane_contracts",
            )
            validate_manifest(
                observed,
                parsed_tasks=parsed,
                release_lane_contracts=release_lane_contracts,
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
