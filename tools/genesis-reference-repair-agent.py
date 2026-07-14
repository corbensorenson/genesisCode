#!/usr/bin/env python3
"""Pinned deterministic reference agents for the diagnostic repair benchmark."""

from __future__ import annotations

import argparse
from hashlib import sha256
import json
import re
import sys
from typing import Any, Mapping, Sequence


SCHEMA = "genesis/reference-repair-response-v0.1"
AGENTS = {"diagnostic-blind-v0.1", "catalog-guided-v0.1"}
INT_PRIMITIVES = ("core/int::add", "core/int::mul", "core/int::sub")
OPEN_TO_CLOSE = {"(": ")", "[": "]", "{": "}"}
CLOSE_TO_OPEN = {value: key for key, value in OPEN_TO_CLOSE.items()}


class AgentError(ValueError):
    pass


def canonical_bytes(value: Any) -> bytes:
    return (json.dumps(value, sort_keys=True, separators=(",", ":")) + "\n").encode()


def digest(text: str) -> str:
    return sha256(text.encode()).hexdigest()


def response(decision: str, reason: str, patches: Sequence[Mapping[str, str]] = ()) -> dict[str, Any]:
    return {
        "schema": SCHEMA,
        "decision": decision,
        "reason": reason,
        "patches": list(patches),
    }


def patch(path: str, before: str, after: str) -> dict[str, str]:
    return {
        "path": path,
        "before_sha256": digest(before),
        "content": after,
    }


def repair_delimiters(source: str) -> str | None:
    stack: list[str] = []
    in_string = False
    escaped = False
    in_comment = False
    for index, char in enumerate(source):
        if in_comment:
            if char == "\n":
                in_comment = False
            continue
        if in_string:
            if escaped:
                escaped = False
            elif char == "\\":
                escaped = True
            elif char == '"':
                in_string = False
            continue
        if char == ";":
            in_comment = True
        elif char == '"':
            in_string = True
        elif char in OPEN_TO_CLOSE:
            stack.append(char)
        elif char in CLOSE_TO_OPEN:
            if not stack or stack[-1] != CLOSE_TO_OPEN[char]:
                return source[:index] + source[index + 1 :]
            stack.pop()
    if not stack:
        return None
    suffix = "".join(OPEN_TO_CLOSE[char] for char in reversed(stack))
    if source.endswith("\n"):
        return source[:-1] + suffix + "\n"
    return source + suffix


def edit_distance(left: str, right: str) -> int:
    previous = list(range(len(right) + 1))
    for left_index, left_char in enumerate(left, start=1):
        current = [left_index]
        for right_index, right_char in enumerate(right, start=1):
            current.append(
                min(
                    current[-1] + 1,
                    previous[right_index] + 1,
                    previous[right_index - 1] + (left_char != right_char),
                )
            )
        previous = current
    return previous[-1]


def repair_primitive(source: str, reason: str) -> str | None:
    match = re.search(r"unbound symbol:\s*(core/int::[A-Za-z0-9_-]+)", reason)
    if not match:
        return None
    unknown = match.group(1)
    ranked = sorted((edit_distance(unknown, candidate), candidate) for candidate in INT_PRIMITIVES)
    if ranked[0][0] > 2 or (len(ranked) > 1 and ranked[0][0] == ranked[1][0]):
        return None
    return source.replace(unknown, ranked[0][1], 1)


def repair_int_literal(source: str, payload: str) -> str | None:
    if "int op expects ints" not in payload:
        return None
    updated, count = re.subn(r'"(-?[0-9]+)"', r"\1", source, count=1)
    return updated if count == 1 else None


def repair_manifest_schema(source: str, message: str) -> str | None:
    if "unsupported package manifest schema" not in message:
        return None
    updated, count = re.subn(r"(?m)^schema\s*=\s*[0-9]+\s*$", "schema = 1", source, count=1)
    return updated if count == 1 else None


def first_patch(files: Mapping[str, str], transform: Any) -> dict[str, Any] | None:
    for path in sorted(files):
        before = files[path]
        after = transform(path, before)
        if after is not None and after != before:
            return response("patch", "applied one conservative diagnostic-scoped repair", [patch(path, before, after)])
    return None


def run_agent(agent: str, request: Mapping[str, Any]) -> dict[str, Any]:
    if set(request) != {"schema", "command", "files", "diagnostic", "authorization"}:
        raise AgentError("request shape is not closed")
    if request["schema"] != "genesis/reference-repair-request-v0.1":
        raise AgentError("unsupported request schema")
    files = request["files"]
    diagnostic = request["diagnostic"]
    authorization = request["authorization"]
    if not isinstance(files, dict) or not all(isinstance(k, str) and isinstance(v, str) for k, v in files.items()):
        raise AgentError("files must be a string map")
    if not isinstance(diagnostic, dict) or not isinstance(authorization, dict):
        raise AgentError("diagnostic and authorization must be objects")
    if authorization.get("automatic_allowed") is not True:
        return response("abstain", "automatic repair is not authorized")

    message = str(diagnostic.get("message", ""))
    delimiter_result = first_patch(
        files,
        lambda path, source: repair_delimiters(source) if path.endswith(".gc") else None,
    )
    if delimiter_result and ("delimiter" in message or "unterminated" in message):
        return delimiter_result
    if agent == "diagnostic-blind-v0.1":
        return response("abstain", "human-only baseline found no unambiguous delimiter repair")

    context = diagnostic.get("context", {})
    facts = context.get("facts", {}) if isinstance(context, dict) else {}
    reason = str(facts.get("reason", "")) if isinstance(facts, dict) else ""
    payload = str(facts.get("payload", "")) if isinstance(facts, dict) else ""

    primitive_result = first_patch(
        files,
        lambda path, source: repair_primitive(source, reason) if path.endswith(".gc") else None,
    )
    if primitive_result:
        return primitive_result
    literal_result = first_patch(
        files,
        lambda path, source: repair_int_literal(source, payload) if path.endswith(".gc") else None,
    )
    if literal_result:
        return literal_result
    manifest_result = first_patch(
        files,
        lambda path, source: repair_manifest_schema(source, message)
        if path.endswith("package.toml")
        else None,
    )
    if manifest_result:
        return manifest_result
    return response("abstain", "catalog-guided agent found no conservative repair")


def main(argv: Sequence[str]) -> int:
    parser = argparse.ArgumentParser()
    parser.add_argument("--agent", required=True, choices=sorted(AGENTS))
    args = parser.parse_args(argv)
    try:
        request = json.load(sys.stdin)
        result = run_agent(args.agent, request)
        sys.stdout.buffer.write(canonical_bytes(result))
    except (AgentError, json.JSONDecodeError) as exc:
        print(f"reference-repair-agent: {exc}", file=sys.stderr)
        return 2
    return 0


if __name__ == "__main__":
    raise SystemExit(main(sys.argv[1:]))
