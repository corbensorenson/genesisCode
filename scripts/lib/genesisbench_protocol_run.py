#!/usr/bin/env python3
"""Bind a benchmark run to observed GenesisBench context and interaction modes."""

from __future__ import annotations

import copy
from typing import Any


class RunBindingError(ValueError):
    pass


def require(condition: bool, message: str) -> None:
    if not condition:
        raise RunBindingError(message)


def validate_run_modes(
    profile: dict[str, Any],
    run: dict[str, Any],
    case: dict[str, Any],
    suite: dict[str, Any],
) -> tuple[str, str]:
    context_mode = f"compact-{case['contextTier']}"
    mode = next(
        (row for row in profile["contextPolicy"]["modes"] if row["id"] == context_mode),
        None,
    )
    tier = next(
        (row for row in suite["contextTiers"] if row["id"] == case["contextTier"]),
        None,
    )
    require(mode is not None and tier is not None, "run context mode is absent")
    require(
        mode["artifacts"] == [row["path"] for row in tier["artifacts"]],
        "run context authority drift",
    )

    inventory = {row["key"]: row for row in run["artifactInventory"]}
    context_keys = [f"repository:{path}" for path in mode["artifacts"]]
    for artifact, key in zip(tier["artifacts"], context_keys):
        require(key in inventory, f"run omits frozen context artifact: {key}")
        require(
            inventory[key]["sha256"] == artifact["sha256"]
            and inventory[key]["bytes"] == artifact["bytes"],
            f"run context artifact drift: {key}",
        )

    prompt = run["invocation"]["promptAssembly"]
    require(
        prompt["cards"]
        == [
            "repository:docs/spec/GC_AGENT_CORE_CARD_v0.3.json",
            "repository:docs/spec/GC_AGENT_TASK_CARDS_v0.3.json",
        ],
        "run card assembly drift",
    )
    require(
        [row["role"] for row in prompt["messages"]] == ["system", "user"],
        "run message assembly drift",
    )
    input_keys = [
        f"bundle:{run['candidate']['root']}/{row['path']}" for row in case["inputFiles"]
    ]
    expected_artifacts = (
        [prompt["messages"][0]["artifact"]]
        + prompt["cards"]
        + context_keys
        + [prompt["messages"][1]["artifact"]]
        + input_keys
    )
    expected_roles = (
        ["system-policy", "agent-profile", "task-card"]
        + ["context-pack"] * len(context_keys)
        + ["task-prompt"]
        + ["task-input"] * len(input_keys)
    )
    require(
        prompt["assemblyOrder"]
        == [
            {"role": role, "artifact": artifact}
            for role, artifact in zip(expected_roles, expected_artifacts)
        ],
        "run prompt order disagrees with protocol authority",
    )
    require(
        prompt["contextArtifacts"] == context_keys + input_keys,
        "run context and task-input partition drift",
    )

    interaction_mode = "artifact-response-v0.1"
    require(
        interaction_mode in profile["toolPolicy"]["allowedInteractionModes"],
        "artifact response mode is not allowed",
    )
    require(
        run["toolProtocol"]["operations"] == ["host/plugin::command"]
        and run["invocation"]["decoding"]["responseFormat"] == "text",
        "run is not an artifact-response invocation",
    )
    return context_mode, interaction_mode


def self_test(
    profile: dict[str, Any],
    run: dict[str, Any],
    case: dict[str, Any],
    suite: dict[str, Any],
) -> int:
    validate_run_modes(profile, run, case, suite)
    mutations: list[tuple[str, Any]] = [
        (
            "assembly-reorder",
            lambda d: d["invocation"]["promptAssembly"]["assemblyOrder"].reverse(),
        ),
        (
            "context-omission",
            lambda d: d["invocation"]["promptAssembly"]["contextArtifacts"].pop(0),
        ),
        (
            "context-hash",
            lambda d: next(
                row
                for row in d["artifactInventory"]
                if row["key"] == "repository:docs/spec/GC_AGENT_CORE_CARD_v0.3.md"
            ).__setitem__("sha256", "0" * 64),
        ),
        (
            "role-injection",
            lambda d: d["invocation"]["promptAssembly"]["assemblyOrder"][0].__setitem__(
                "role", "task-input"
            ),
        ),
        (
            "interaction-mode",
            lambda d: d["invocation"]["decoding"].__setitem__(
                "responseFormat", "tool-calls"
            ),
        ),
    ]
    rejected = 0
    for name, mutate in mutations:
        candidate = copy.deepcopy(run)
        mutate(candidate)
        try:
            validate_run_modes(profile, candidate, case, suite)
        except RunBindingError:
            rejected += 1
        else:
            raise RunBindingError(f"negative run-binding control accepted: {name}")
    return rejected
