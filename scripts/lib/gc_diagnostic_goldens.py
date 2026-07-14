#!/usr/bin/env python3
from __future__ import annotations

import argparse
import copy
import json
import re
from pathlib import Path
from typing import Any

ROOT = Path(__file__).resolve().parents[2]
GOLDEN = ROOT / "tests/diagnostics/goldens/v0.1/diagnostics.json"
SCHEMA = "genesis/diagnostic-goldens-v0.1"
REQUIRED_CLASSES = {
    "exhausted-budgets",
    "incompatible-profiles",
    "invalid-packages",
    "malformed-syntax",
    "path-normalization",
    "replay-tampering",
    "seal-misuse",
    "stale-patches",
    "type-effect-mismatch",
    "unhandled-effects",
}
EXPECTED_ROUTE = {
    "exhausted-budgets": ("genesis/error-v0.2", "eval/error", "evaluator", "step-limit"),
    "incompatible-profiles": ("genesis/error-v0.2", "pkg/run", "package", "legacy-error"),
    "invalid-packages": ("genesis/error-v0.2", "manifest/error", "package", "manifest"),
    "malformed-syntax": ("genesis/error-v0.2", "parse/coreform", "parser", "unexpected-token"),
    "path-normalization": ("genesis/error-v0.2", "manifest/error", "package", "manifest"),
    "replay-tampering": ("genesis/error-v0.2", "replay/mismatch", "replay", "fact-mismatch"),
    "seal-misuse": ("genesis/error-v0.2", "eval/error", "evaluator", "selfhost-protocol"),
    "stale-patches": ("genesis/error-v0.2", "patch/invalid", "patch", "validation"),
    "type-effect-mismatch": (
        "genesis/typecheck-v0.2",
        "typecheck/error",
        "typechecker",
        "diagnostics",
    ),
    "unhandled-effects": ("genesis/error-v0.2", "effects/run", "policy", "selfhost-protocol"),
}
ROOT_KEYS = {"schema", "case_count", "cases"}
CASE_KEYS = {"id", "class", "envelope"}
ENVELOPE_KEYS = {
    "diagnostic_catalog",
    "diagnostics",
    "diagnostics_schema",
    "error",
    "kind",
    "ok",
}
CONTEXT_KEYS = {
    "schema",
    "domain",
    "kind",
    "operation",
    "facts",
    "primary_span",
    "related_spans",
}
ABSOLUTE_HOST_PATH = re.compile(
    r"(?:^|[\s`'\"({\[])"
    r"(?:/(?:Users|home|tmp|var|private|Volumes)/|[A-Za-z]:[\\/])"
)


class GoldenError(ValueError):
    pass


def require(condition: bool, message: str) -> None:
    if not condition:
        raise GoldenError(message)


def strings(value: Any):
    if isinstance(value, str):
        yield value
    elif isinstance(value, list):
        for item in value:
            yield from strings(item)
    elif isinstance(value, dict):
        for item in value.values():
            yield from strings(item)


def validate(data: Any) -> None:
    require(isinstance(data, dict), "golden root must be an object")
    require(set(data) == ROOT_KEYS, "golden root field drift")
    require(data["schema"] == SCHEMA, "golden schema drift")
    cases = data["cases"]
    require(isinstance(cases, list), "golden cases must be an array")
    require(data["case_count"] == len(cases) == 10, "golden case count drift")
    ids: list[str] = []
    classes: list[str] = []
    for index, case in enumerate(cases):
        require(isinstance(case, dict), f"case {index} must be an object")
        require(set(case) == CASE_KEYS, f"case {index} field drift")
        case_id = case["id"]
        case_class = case["class"]
        require(isinstance(case_id, str) and case_id, f"case {index} invalid id")
        require(isinstance(case_class, str), f"case {case_id} invalid class")
        ids.append(case_id)
        classes.append(case_class)
        envelope = case["envelope"]
        require(isinstance(envelope, dict), f"case {case_id} envelope must be an object")
        require(set(envelope) == ENVELOPE_KEYS, f"case {case_id} envelope field drift")
        require(envelope["ok"] is False, f"case {case_id} must fail")
        expected_kind, expected_code, expected_domain, expected_context_kind = EXPECTED_ROUTE[
            case_class
        ]
        require(envelope["kind"] == expected_kind, f"case {case_id} kind drift")
        require(
            envelope["diagnostics_schema"] == "genesis/diagnostics-schema-v1",
            f"case {case_id} diagnostics schema drift",
        )
        diagnostics = envelope["diagnostics"]
        require(
            isinstance(diagnostics, list) and len(diagnostics) == 1,
            f"case {case_id} must carry exactly one diagnostic",
        )
        error = envelope["error"]
        require(
            isinstance(error, dict) and set(error) == {"code", "context", "message"},
            f"case {case_id} error field drift",
        )
        context = error["context"]
        require(
            isinstance(context, dict) and set(context) == CONTEXT_KEYS,
            f"case {case_id} failure context field drift",
        )
        require(
            context["schema"] == "genesis/failure-context-v0.1",
            f"case {case_id} failure context schema drift",
        )
        require(error["code"] == expected_code, f"case {case_id} error route drift")
        require(context["domain"] == expected_domain, f"case {case_id} domain drift")
        require(context["kind"] == expected_context_kind, f"case {case_id} context kind drift")
        diagnostic = diagnostics[0]
        require(
            diagnostic.get("code") == error["code"],
            f"case {case_id} diagnostic/error code mismatch",
        )
        require(
            diagnostic.get("id") == f"genesis/diagnostic/v1/{error['code']}",
            f"case {case_id} diagnostic id drift",
        )
        repair = diagnostic.get("repair_plan")
        require(isinstance(repair, dict), f"case {case_id} missing repair plan")
        authorization = repair.get("authorization")
        require(isinstance(authorization, dict), f"case {case_id} missing authorization")
        require(
            authorization.get("policy_change_allowed") is False,
            f"case {case_id} permits policy broadening",
        )
        require(
            authorization.get("obligation_suppression_allowed") is False,
            f"case {case_id} permits obligation suppression",
        )
        for text in strings(case):
            require(
                ABSOLUTE_HOST_PATH.search(text) is None,
                f"case {case_id} leaks an absolute host path: {text!r}",
            )
    require(ids == sorted(ids), "golden cases must be sorted by id")
    require(len(ids) == len(set(ids)), "golden case ids must be unique")
    require(len(classes) == len(set(classes)), "golden classes must be unique")
    require(set(classes) == REQUIRED_CLASSES, "roadmap diagnostic class coverage drift")


def self_test(data: Any) -> None:
    mutations = []

    def mutate(name, operation):
        candidate = copy.deepcopy(data)
        operation(candidate)
        mutations.append((name, candidate))

    mutate("root-field", lambda value: value.__setitem__("extra", True))
    mutate("case-count", lambda value: value.__setitem__("case_count", 9))
    mutate("missing-class", lambda value: value["cases"].pop())
    mutate(
        "duplicate-class",
        lambda value: value["cases"][1].__setitem__("class", value["cases"][0]["class"]),
    )
    mutate("unsorted", lambda value: value["cases"].reverse())
    mutate(
        "success-fallthrough",
        lambda value: value["cases"][0]["envelope"].__setitem__("ok", True),
    )
    mutate(
        "missing-context",
        lambda value: value["cases"][0]["envelope"]["error"].pop("context"),
    )
    mutate(
        "policy-broadening",
        lambda value: value["cases"][0]["envelope"]["diagnostics"][0]["repair_plan"][
            "authorization"
        ].__setitem__("policy_change_allowed", True),
    )
    mutate(
        "obligation-suppression",
        lambda value: value["cases"][0]["envelope"]["diagnostics"][0]["repair_plan"][
            "authorization"
        ].__setitem__("obligation_suppression_allowed", True),
    )
    mutate(
        "host-path",
        lambda value: value["cases"][0]["envelope"]["error"].__setitem__(
            "message", "read /Users/example/private/source.gc"
        ),
    )
    for name, candidate in mutations:
        try:
            validate(candidate)
        except GoldenError:
            continue
        raise GoldenError(f"negative control accepted: {name}")
    print(f"diagnostic-goldens: self-test ok (negative_controls={len(mutations)})")


def main() -> int:
    parser = argparse.ArgumentParser()
    parser.add_argument("--check", action="store_true")
    parser.add_argument("--self-test", action="store_true")
    args = parser.parse_args()
    if not args.check and not args.self_test:
        parser.error("one of --check or --self-test is required")
    data = json.loads(GOLDEN.read_text(encoding="utf-8"))
    validate(data)
    if args.check:
        print("diagnostic-goldens: ok (cases=10 classes=10 path_leaks=0)")
    if args.self_test:
        self_test(data)
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
