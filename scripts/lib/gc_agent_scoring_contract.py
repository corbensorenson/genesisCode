#!/usr/bin/env python3
"""Closed contracts for model-agnostic GenesisCode agent benchmark scoring."""

from __future__ import annotations

import copy
import hashlib
import json
import re
from pathlib import Path, PurePosixPath
from typing import Any

ROOT = Path(__file__).resolve().parents[2]
SCORING = ROOT / "docs/spec/GC_AGENT_BENCHMARK_SCORING_v0.1.json"
SCORING_SCHEMA = ROOT / "docs/spec/GC_AGENT_BENCHMARK_SCORING_v0.1.schema.json"
SCORE_SCHEMA = ROOT / "docs/spec/GC_AGENT_BENCHMARK_SCORE_v0.1.schema.json"
BENCHMARK = ROOT / "benchmarks/agent_tasks/v0.1/suite.json"
PROFILE = ROOT / "docs/spec/GC_AGENT_PROFILE_v0.3.json"
SCORER_RUNTIME = ROOT / "scripts/lib/gc_agent_scoring.py"
SCORER_CONTRACT = ROOT / "scripts/lib/gc_agent_scoring_contract.py"
SCORER_TEST = ROOT / "crates/gc_cli/tests/cli_agent_benchmark_scoring.rs"
SHA_RE = re.compile(r"^[0-9a-f]{64}$")
DIMENSION_IDS = [
    "semantics",
    "obligations",
    "effects",
    "patch-minimality",
    "resource-use",
    "policy-scope",
]
TASK_CLASSES = [
    "completion",
    "deployment",
    "generation",
    "package-migration",
    "performance-repair",
    "policy-minimization",
    "refactor",
    "repair",
    "replay-investigation",
]


class ScoringError(ValueError):
    pass


def require(condition: bool, message: str) -> None:
    if not condition:
        raise ScoringError(message)


def load_json(path: Path) -> Any:
    def pairs(rows: list[tuple[str, Any]]) -> dict[str, Any]:
        result: dict[str, Any] = {}
        for key, value in rows:
            require(key not in result, f"duplicate JSON key: {key}")
            result[key] = value
        return result

    return json.loads(path.read_text(encoding="utf-8"), object_pairs_hook=pairs)


def canonical_bytes(value: Any) -> bytes:
    return (
        json.dumps(value, sort_keys=True, separators=(",", ":"), ensure_ascii=True)
        + "\n"
    ).encode("ascii")


def sha256_bytes(value: bytes) -> str:
    return hashlib.sha256(value).hexdigest()


def file_sha256(path: Path) -> str:
    return sha256_bytes(path.read_bytes())


def content_identity(document: dict[str, Any], field: str) -> str:
    unsigned = copy.deepcopy(document)
    unsigned[field] = ""
    return sha256_bytes(canonical_bytes(unsigned))


def closed(value: Any, keys: set[str], label: str) -> dict[str, Any]:
    require(isinstance(value, dict) and set(value) == keys, f"{label} fields are not closed")
    return value


def sorted_unique(values: list[str], label: str) -> None:
    require(values == sorted(set(values)), f"{label} must be sorted and unique")


def safe_relative(value: str, label: str) -> PurePosixPath:
    require(isinstance(value, str) and value and "\\" not in value, f"{label} is invalid")
    path = PurePosixPath(value)
    require(not path.is_absolute(), f"{label} is absolute")
    require(all(part not in {"", ".", ".."} for part in path.parts), f"{label} is non-canonical")
    return path


def validate_schema_markers() -> None:
    expected = {
        SCORING_SCHEMA: (
            "https://genesiscode.dev/schemas/gc-agent-benchmark-scoring-v0.1.json",
            [
                "benchmark",
                "profile",
                "implementation",
                "dimension",
                "validityGate",
                "aggregation",
                "resourcePolicy",
                "taskPolicy",
                "modelSpecificMetrics",
            ],
        ),
        SCORE_SCHEMA: (
            "https://genesiscode.dev/schemas/gc-agent-benchmark-score-v0.1.json",
            [
                "bindings",
                "candidate",
                "validity",
                "dimension",
                "verification",
                "patch",
                "policy",
                "resources",
                "modelSpecificMetrics",
            ],
        ),
    }
    for path, (schema_id, definitions) in expected.items():
        schema = load_json(path)
        require(
            schema.get("$schema") == "https://json-schema.org/draft/2020-12/schema",
            f"{path.name}: schema draft drift",
        )
        require(schema.get("$id") == schema_id, f"{path.name}: schema id drift")
        require(schema.get("additionalProperties") is False, f"{path.name}: root must be closed")
        for name in definitions:
            require(
                schema.get("$defs", {}).get(name, {}).get("additionalProperties") is False,
                f"{path.name}: {name} must be closed",
            )


def validate_scoring(document: Any, *, check_identity: bool = True) -> dict[str, Any]:
    doc = closed(
        document,
        {
            "kind",
            "version",
            "scoringId",
            "benchmark",
            "profile",
            "implementation",
            "qualityScaleBasisPoints",
            "dimensions",
            "validityGate",
            "aggregation",
            "resourcePolicy",
            "taskPolicies",
            "modelSpecificMetrics",
            "contentIdentitySha256",
        },
        "scoring authority",
    )
    require(
        doc["kind"] == "genesis/agent-benchmark-scoring-v0.1"
        and doc["version"] == "0.1.0"
        and doc["scoringId"] == "GC-AGENT-BENCHMARK-SCORING-v0.1",
        "scoring identity drift",
    )
    benchmark = closed(
        doc["benchmark"],
        {"id", "path", "sha256", "contentIdentitySha256"},
        "benchmark binding",
    )
    require(
        benchmark["id"] == "GC-AGENT-TASK-BENCHMARK-v0.1"
        and benchmark["path"] == "benchmarks/agent_tasks/v0.1/suite.json",
        "benchmark authority drift",
    )
    suite = load_json(BENCHMARK)
    require(file_sha256(BENCHMARK) == benchmark["sha256"], "benchmark file hash drift")
    require(
        suite.get("contentIdentitySha256") == benchmark["contentIdentitySha256"],
        "benchmark content identity drift",
    )
    profile = closed(doc["profile"], {"id", "path", "sha256"}, "profile binding")
    require(
        profile == {
            "id": "GC-AGENT-v0.3",
            "path": "docs/spec/GC_AGENT_PROFILE_v0.3.json",
            "sha256": file_sha256(PROFILE),
        },
        "profile authority drift",
    )
    implementation = closed(
        doc["implementation"],
        {
            "runtimePath",
            "runtimeSha256",
            "contractPath",
            "contractSha256",
            "integrationTestPath",
            "integrationTestSha256",
        },
        "scorer implementation binding",
    )
    require(
        implementation
        == {
            "runtimePath": "scripts/lib/gc_agent_scoring.py",
            "runtimeSha256": file_sha256(SCORER_RUNTIME),
            "contractPath": "scripts/lib/gc_agent_scoring_contract.py",
            "contractSha256": file_sha256(SCORER_CONTRACT),
            "integrationTestPath": "crates/gc_cli/tests/cli_agent_benchmark_scoring.rs",
            "integrationTestSha256": file_sha256(SCORER_TEST),
        },
        "scorer implementation authority drift",
    )
    require(doc["qualityScaleBasisPoints"] == 10000, "quality scale drift")

    dimensions = doc["dimensions"]
    require(isinstance(dimensions, list) and len(dimensions) == 6, "dimension coverage drift")
    require([row.get("id") for row in dimensions] == DIMENSION_IDS, "dimension order drift")
    expected_weights = [3500, 1200, 1200, 1500, 1300, 1300]
    require(
        [row.get("weightBasisPoints") for row in dimensions] == expected_weights,
        "dimension weight drift",
    )
    require(sum(expected_weights) == 10000, "dimension weights do not close")
    for row in dimensions:
        closed(row, {"id", "weightBasisPoints", "applicability", "metric"}, "dimension")
        require(row["applicability"] in {"all-cases", "task-policy-step-set"}, "unknown applicability")
        require(re.fullmatch(r"[a-z0-9-]+", row["metric"]) is not None, "invalid metric id")

    require(
        doc["validityGate"]
        == {
            "failureScoreBasisPoints": 0,
            "requiredPerfectDimensions": ["semantics", "obligations", "policy-scope"],
            "inapplicableDimensionPasses": True,
            "editableScopeRequired": True,
        },
        "validity gate drift",
    )
    require(
        doc["aggregation"]
        == {
            "algorithm": "weighted-applicable-floor-v0.1",
            "formula": "floor(sum(weight * score) / sum(applicable weight))",
            "mixedCaseAggregation": "arithmetic-mean-basis-points",
            "missingCasePolicy": "fail-closed",
        },
        "aggregation contract drift",
    )
    resources = closed(
        doc["resourcePolicy"],
        {
            "maxCandidateFiles",
            "maxCandidateBytes",
            "maxFileBytes",
            "maxVerificationSteps",
            "defaultEvaluatorStepLimit",
            "processTimeoutMs",
            "maxStdoutBytes",
            "maxStderrBytes",
            "maxGeneratedFiles",
            "maxGeneratedBytes",
            "resourceUnitFormula",
            "wallTimeInQualityScore",
        },
        "resource policy",
    )
    require(
        resources
        == {
            "maxCandidateFiles": 128,
            "maxCandidateBytes": 1048576,
            "maxFileBytes": 262144,
            "maxVerificationSteps": 8,
            "defaultEvaluatorStepLimit": 200000,
            "processTimeoutMs": 30000,
            "maxStdoutBytes": 1048576,
            "maxStderrBytes": 262144,
            "maxGeneratedFiles": 128,
            "maxGeneratedBytes": 16777216,
            "resourceUnitFormula": "candidate-source-bytes + generated-artifact-bytes + stdout-bytes + stderr-bytes + verification-command-count",
            "wallTimeInQualityScore": False,
        },
        "resource policy drift",
    )

    policies = doc["taskPolicies"]
    require(isinstance(policies, list) and len(policies) == 9, "task policy coverage drift")
    require([row.get("taskClass") for row in policies] == TASK_CLASSES, "task policy order drift")
    suite_tasks = suite.get("taskClasses")
    require(suite_tasks == TASK_CLASSES, "benchmark task class drift")
    steps_by_task: dict[str, set[str]] = {}
    for case in suite.get("cases", []):
        steps_by_task.setdefault(case["taskClass"], set()).update(
            step["id"] for step in case["verification"]
        )
    for policy in policies:
        closed(
            policy,
            {
                "taskClass",
                "obligationStepIds",
                "effectStepIds",
                "policyPaths",
                "generatedPathPrefixes",
            },
            "task policy",
        )
        for field in ("obligationStepIds", "effectStepIds", "policyPaths", "generatedPathPrefixes"):
            values = policy[field]
            require(isinstance(values, list), f"{policy['taskClass']}: {field} must be an array")
            sorted_unique(values, f"{policy['taskClass']}: {field}")
        declared_steps = set(policy["obligationStepIds"]) | set(policy["effectStepIds"])
        require(
            declared_steps.issubset(steps_by_task[policy["taskClass"]]),
            f"{policy['taskClass']}: scoring step is not in benchmark",
        )
        for path in policy["policyPaths"]:
            safe_relative(path, "policy path")
        for path in policy["generatedPathPrefixes"]:
            safe_relative(path, "generated path prefix")
            require(
                not path.startswith(".genesis") and "*" not in path,
                "generated path prefix claims scorer-owned or wildcard scope",
            )

    require(
        doc["modelSpecificMetrics"]
        == {
            "includedInQualityScore": False,
            "recordedBy": "genesis/agent-benchmark-run-v0.1",
            "separateFields": [
                "api-cost",
                "energy",
                "model-latency",
                "provider-queue-time",
            ],
        },
        "model-specific metric separation drift",
    )
    if check_identity:
        claimed = doc["contentIdentitySha256"]
        require(
            isinstance(claimed, str)
            and SHA_RE.fullmatch(claimed) is not None
            and claimed == content_identity(doc, "contentIdentitySha256"),
            "scoring content identity drift",
        )
    return doc


def validate_score(document: Any) -> dict[str, Any]:
    doc = closed(
        document,
        {
            "kind",
            "version",
            "scoringId",
            "caseId",
            "taskClass",
            "contextTier",
            "bindings",
            "candidate",
            "validity",
            "dimensions",
            "qualityScoreBasisPoints",
            "verification",
            "patch",
            "policy",
            "resources",
            "modelSpecificMetrics",
            "scoreIdentitySha256",
        },
        "score",
    )
    require(
        doc["kind"] == "genesis/agent-benchmark-score-v0.1"
        and doc["version"] == "0.1.0"
        and doc["scoringId"] == "GC-AGENT-BENCHMARK-SCORING-v0.1",
        "score identity drift",
    )
    closed(
        doc["bindings"],
        {
            "scoringContentIdentitySha256",
            "benchmarkContentIdentitySha256",
            "profileSha256",
            "scorerRuntimeSha256",
            "scorerContractSha256",
        },
        "score bindings",
    )
    closed(doc["candidate"], {"identitySha256", "fileCount", "bytes"}, "candidate facts")
    closed(doc["validity"], {"passed", "failedDimensions"}, "validity facts")
    require([row.get("id") for row in doc["dimensions"]] == DIMENSION_IDS, "score dimension order drift")
    for row in doc["dimensions"]:
        closed(row, {"id", "applicable", "weightBasisPoints", "scoreBasisPoints"}, "score dimension")
        require(isinstance(row["scoreBasisPoints"], int) and 0 <= row["scoreBasisPoints"] <= 10000, "invalid dimension score")
    require(isinstance(doc["qualityScoreBasisPoints"], int) and 0 <= doc["qualityScoreBasisPoints"] <= 10000, "invalid quality score")
    for row in doc["verification"]:
        closed(
            row,
            {
                "id",
                "passed",
                "exitCode",
                "ok",
                "kind",
                "assertionsPassed",
                "outputIdentitySha256",
                "generatedIdentitySha256",
                "resourceUnits",
            },
            "verification result",
        )
    closed(
        doc["patch"],
        {"changedPaths", "expectedChangedPaths", "editableScopeOk", "candidateUnits", "referenceUnits"},
        "patch facts",
    )
    closed(
        doc["policy"],
        {
            "scopeOk",
            "candidateAuthorityIdentitySha256",
            "referenceAuthorityIdentitySha256",
            "broadenedAuthorities",
        },
        "policy facts",
    )
    closed(
        doc["resources"],
        {
            "candidateUnits",
            "referenceUnits",
            "candidateGeneratedBytes",
            "referenceGeneratedBytes",
            "limitsSatisfied",
        },
        "resource facts",
    )
    require(
        doc["modelSpecificMetrics"]
        == {
            "includedInQualityScore": False,
            "recordedBy": "genesis/agent-benchmark-run-v0.1",
            "present": False,
        },
        "model-specific metrics entered the quality result",
    )
    require(
        SHA_RE.fullmatch(doc["scoreIdentitySha256"]) is not None
        and content_identity(doc, "scoreIdentitySha256") == doc["scoreIdentitySha256"],
        "score content identity drift",
    )
    serialized = canonical_bytes(doc).decode("ascii")
    require("/Users/" not in serialized and "/home/" not in serialized and "\\Users\\" not in serialized, "score leaks a host path")
    return doc


def self_test(document: dict[str, Any]) -> int:
    mutations = [
        ("unknown-field", lambda d: d.update({"model": "prompt-selected"})),
        ("benchmark-rebind", lambda d: d["benchmark"].__setitem__("sha256", "0" * 64)),
        ("profile-rebind", lambda d: d["profile"].__setitem__("sha256", "0" * 64)),
        ("runtime-rebind", lambda d: d["implementation"].__setitem__("runtimeSha256", "0" * 64)),
        ("weight-drift", lambda d: d["dimensions"][0].__setitem__("weightBasisPoints", 3499)),
        ("dimension-omission", lambda d: d["dimensions"].pop()),
        ("validity-weakening", lambda d: d["validityGate"]["requiredPerfectDimensions"].remove("policy-scope")),
        ("scope-weakening", lambda d: d["validityGate"].__setitem__("editableScopeRequired", False)),
        ("wall-time", lambda d: d["resourcePolicy"].__setitem__("wallTimeInQualityScore", True)),
        ("unbounded-timeout", lambda d: d["resourcePolicy"].__setitem__("processTimeoutMs", 0)),
        ("step-omission", lambda d: d["taskPolicies"][1]["obligationStepIds"].remove("build")),
        ("task-duplication", lambda d: d["taskPolicies"].__setitem__(1, copy.deepcopy(d["taskPolicies"][0]))),
        ("unsafe-policy-path", lambda d: d["taskPolicies"][1]["policyPaths"].__setitem__(0, "../caps.toml")),
        ("unsafe-output-prefix", lambda d: d["taskPolicies"][1]["generatedPathPrefixes"].__setitem__(0, ".genesis/")),
        ("model-cost", lambda d: d["modelSpecificMetrics"].__setitem__("includedInQualityScore", True)),
        ("identity-drift", lambda d: d.__setitem__("contentIdentitySha256", "0" * 64)),
    ]
    for name, mutate in mutations:
        candidate = copy.deepcopy(document)
        mutate(candidate)
        try:
            validate_scoring(candidate)
        except ScoringError:
            continue
        raise ScoringError(f"negative control accepted: {name}")
    return len(mutations)
