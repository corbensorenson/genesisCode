#!/usr/bin/env python3
"""Deterministic compiler and verifier for the GenesisBench reference agent."""

from __future__ import annotations

import argparse
import copy
import hashlib
import json
import re
import sys
from pathlib import Path, PurePosixPath
from typing import Any, Callable


ROOT = Path(__file__).resolve().parents[2]
SUITE_PATH = "benchmarks/agent_tasks/v0.1/suite.json"
PROFILE_PATH = "docs/spec/GENESISBENCH_REFERENCE_AGENT_v0.1.json"
ABLATIONS_PATH = "docs/spec/GENESISBENCH_REFERENCE_AGENT_ABLATIONS_v0.1.json"
PROFILE_SCHEMA_PATH = "docs/spec/GENESISBENCH_REFERENCE_AGENT_v0.1.schema.json"
ABLATIONS_SCHEMA_PATH = "docs/spec/GENESISBENCH_REFERENCE_AGENT_ABLATIONS_v0.1.schema.json"
TRACE_SCHEMA_PATH = "docs/spec/GENESISBENCH_REFERENCE_AGENT_TRACE_v0.1.schema.json"
RETRIEVAL_PATH = "benchmarks/genesisbench/v0.1/reference-agent/retrieval.json"
SYSTEM_PATH = "benchmarks/genesisbench/v0.1/reference-agent/system.md"
PLAN_FIXTURE_PATH = "benchmarks/genesisbench/v0.1/reference-agent/plan.fixture.json"
TRACE_FIXTURE_PATH = "benchmarks/genesisbench/v0.1/reference-agent/trace.fixture.json"
RUNTIME_PATH = "scripts/lib/genesisbench_reference_agent.py"
MCP_CATALOG_PATH = "crates/gc_cli_driver/src/mcp/catalog.rs"

SHA_RE = re.compile(r"^[0-9a-f]{64}$")
TOKEN_RE = re.compile(r"[a-z0-9]+(?:[._:/-][a-z0-9]+)*")
FORBIDDEN_PARTS = {".git", "private", "references", "held-out-disclosures"}
PROMPT_ROLES = [
    "system-policy", "agent-profile", "task-card",
    "context-pack-or-retrieval-transcript", "task-prompt", "task-inputs",
]
SESSION_TOOLS = [
    "session-abort", "session-apply", "session-begin", "session-stage",
    "session-status", "session-test",
]
ALL_TOOLS = [
    "apply-patch", "build", "check", "diff", "explain", "format",
    "get-card", "package", "parse", "replay", "run", "search-symbol",
    *SESSION_TOOLS, "test", "verify",
]
FIXED_CONTEXT = [
    "docs/spec/GC_AGENT_CORE_CARD_v0.3.md",
    "docs/spec/GC_AGENT_TASK_CARDS_v0.3.md",
    "docs/spec/CLI.md",
    "docs/spec/GC_AGENT_PROFILE_v0.3.json",
    "docs/spec/GC_AGENT_SYMBOL_INDEX_v0.3.json",
]
ARTIFACTS = [
    ("agent-session-resources", "docs/spec/AGENT_SESSION_RESOURCES_v0.1.schema.json", "budget-contract"),
    ("agent-transaction-schema", "docs/spec/AGENT_TRANSACTION_v0.1.schema.json", "transaction-contract"),
    ("benchmark-run-schema", "docs/spec/GC_AGENT_BENCHMARK_RUN_v0.1.schema.json", "prompt-schema"),
    ("gc-agent-core-card", "docs/spec/GC_AGENT_CORE_CARD_v0.3.md", "card"),
    ("gc-agent-profile", "docs/spec/GC_AGENT_PROFILE_v0.3.json", "grammar"),
    ("gc-agent-symbol-index", "docs/spec/GC_AGENT_SYMBOL_INDEX_v0.3.json", "profile"),
    ("gc-agent-task-cards", "docs/spec/GC_AGENT_TASK_CARDS_v0.3.md", "card"),
    ("gc-diagnostic-catalog", "docs/spec/GC_DIAGNOSTIC_CATALOG_v0.1.json", "diagnostics"),
    ("gc-diagnostic-repair-plan", "docs/spec/GC_DIAGNOSTIC_REPAIR_PLAN_v0.1.schema.json", "repair-contract"),
    ("mcp-catalog-source", MCP_CATALOG_PATH, "mcp-catalog"),
    ("reference-agent-runtime", RUNTIME_PATH, "retrieval-runtime"),
    ("reference-retrieval-config", RETRIEVAL_PATH, "retrieval-config"),
    ("reference-system-prompt", SYSTEM_PATH, "prompt"),
    ("scaffold-manifest-schema", "docs/spec/GENESISBENCH_SCAFFOLD_MANIFEST_v0.1.schema.json", "adapter-contract"),
]

ABLATION_SPECS = {
    "bounded-repair": (8, "Full fixed agent with one deterministic, diagnostic-bound repair opportunity.",
        ("retrieval-seed", "integer-lexical-v0.1", "structured-catalog-v0.1", "semantic-transaction-v0.1", "gc-agent-v0.3-constraint", 2, 1), "one-attempt", "repair"),
    "core-card-only": (1, "Minimal acquisition control exposing only the compact core language card.",
        ("core-card-only", "disabled", "terminal-code-only", "artifact-response", "production-acceptance-only", 1, 0), None, "reference-control"),
    "diagnostics": (4, "Retrieval control plus the structured diagnostic catalog and no other agent aid.",
        ("retrieval-seed", "integer-lexical-v0.1", "structured-catalog-v0.1", "artifact-response", "production-acceptance-only", 1, 0), "retrieval", "diagnostics"),
    "fixed-context": (2, "Deterministic full context pack without task-conditioned document retrieval.",
        ("fixed-context", "disabled", "terminal-code-only", "artifact-response", "production-acceptance-only", 1, 0), "core-card-only", "context-breadth"),
    "grammar-constraint": (6, "Retrieval control plus the generated GenesisCode grammar acceptance constraint.",
        ("retrieval-seed", "integer-lexical-v0.1", "terminal-code-only", "artifact-response", "gc-agent-v0.3-constraint", 1, 0), "retrieval", "grammar"),
    "one-attempt": (7, "Full fixed agent with all aids enabled and exactly one model response attempt.",
        ("retrieval-seed", "integer-lexical-v0.1", "structured-catalog-v0.1", "semantic-transaction-v0.1", "gc-agent-v0.3-constraint", 1, 0), None, "reference-control"),
    "retrieval": (3, "Fixed-context control replaced by deterministic task-conditioned integer retrieval.",
        ("retrieval-seed", "integer-lexical-v0.1", "terminal-code-only", "artifact-response", "production-acceptance-only", 1, 0), "fixed-context", "retrieval"),
    "semantic-patch": (5, "Retrieval control plus content-addressed semantic transaction editing only.",
        ("retrieval-seed", "integer-lexical-v0.1", "terminal-code-only", "semantic-transaction-v0.1", "production-acceptance-only", 1, 0), "retrieval", "editing"),
}


class ReferenceAgentError(ValueError):
    pass


def require(condition: bool, message: str) -> None:
    if not condition:
        raise ReferenceAgentError(message)


def load_json(path: str | Path) -> Any:
    target = safe_path(path) if isinstance(path, str) else path

    def reject_duplicates(pairs: list[tuple[str, Any]]) -> dict[str, Any]:
        result: dict[str, Any] = {}
        for key, value in pairs:
            require(key not in result, f"duplicate JSON key: {key}")
            result[key] = value
        return result

    return json.loads(target.read_text(encoding="utf-8"), object_pairs_hook=reject_duplicates)


def canonical_bytes(value: Any) -> bytes:
    return (json.dumps(value, sort_keys=True, separators=(",", ":"), ensure_ascii=True) + "\n").encode("ascii")


def pretty_bytes(value: Any) -> bytes:
    return (json.dumps(value, indent=2, sort_keys=True, ensure_ascii=True) + "\n").encode("ascii")


def digest(payload: bytes) -> str:
    return hashlib.sha256(payload).hexdigest()


def object_identity(value: dict[str, Any]) -> str:
    material = copy.deepcopy(value)
    material["contentIdentitySha256"] = ""
    return digest(canonical_bytes(material))


def identified(value: dict[str, Any]) -> dict[str, Any]:
    result = copy.deepcopy(value)
    result["contentIdentitySha256"] = ""
    result["contentIdentitySha256"] = object_identity(result)
    return result


def safe_path(relative: str | Path) -> Path:
    text = relative.as_posix() if isinstance(relative, Path) else relative
    require(isinstance(text, str) and text and "\\" not in text, f"invalid repository path: {text!r}")
    pure = PurePosixPath(text)
    require(not pure.is_absolute() and all(part not in {"", ".", ".."} for part in pure.parts), f"path escapes repository: {text}")
    target = ROOT.joinpath(*pure.parts)
    try:
        target.resolve(strict=True).relative_to(ROOT.resolve())
    except (OSError, ValueError) as exc:
        raise ReferenceAgentError(f"missing or escaped repository path: {text}") from exc
    require(target.is_file() and not target.is_symlink(), f"path is not a regular non-symlink file: {text}")
    return target


def public_path(relative: str) -> Path:
    pure = PurePosixPath(relative)
    require(not FORBIDDEN_PARTS.intersection(pure.parts), f"private or oracle path forbidden: {relative}")
    return safe_path(relative)


def artifact(path: str, artifact_id: str | None = None, role: str | None = None) -> dict[str, Any]:
    payload = safe_path(path).read_bytes()
    result: dict[str, Any] = {"path": path, "bytes": len(payload), "sha256": digest(payload)}
    if artifact_id is not None:
        result = {"id": artifact_id, **result, "role": role}
    return result


def parse_mcp_tools() -> list[str]:
    source = safe_path(MCP_CATALOG_PATH).read_text(encoding="utf-8")
    tools = sorted(re.findall(r'route\(\s*"([a-z-]+)"', source))
    require(tools == ALL_TOOLS, "MCP catalog operations drift from the fixed reference allowlist")
    return tools


def validate_schema_markers(schema: Any, label: str) -> None:
    require(isinstance(schema, dict) and schema.get("$schema") == "https://json-schema.org/draft/2020-12/schema", f"{label} schema version drift")

    def visit(node: Any, location: str) -> None:
        if isinstance(node, dict):
            if node.get("type") == "object":
                require(node.get("additionalProperties") is False, f"open object schema at {label}{location}")
            for key, value in node.items():
                visit(value, f"{location}/{key}")
        elif isinstance(node, list):
            for index, value in enumerate(node):
                visit(value, f"{location}/{index}")

    visit(schema, "")


def render_retrieval() -> dict[str, Any]:
    config = load_json(RETRIEVAL_PATH)
    expected = copy.deepcopy(config)
    expected["contentIdentitySha256"] = ""
    expected["contentIdentitySha256"] = object_identity(expected)
    return expected


def render_profile(retrieval: dict[str, Any]) -> dict[str, Any]:
    rows = []
    for artifact_id, path, role in ARTIFACTS:
        if path == RETRIEVAL_PATH:
            payload = pretty_bytes(retrieval)
            rows.append({"id": artifact_id, "path": path, "role": role, "bytes": len(payload), "sha256": digest(payload)})
        else:
            rows.append(artifact(path, artifact_id, role))
    require([row["id"] for row in rows] == sorted(row["id"] for row in rows), "profile artifacts must be sorted")
    profile = {
        "kind": "genesis/genesisbench-reference-agent-v0.1", "version": "0.1.0",
        "id": "genesisbench-reference-agent-v0.1", "class": "fixed-reference", "status": "frozen",
        "artifacts": rows,
        "comparisonPolicy": {
            "allowedRunVariable": "immutable-model-revision",
            "fixedFacts": sorted(["adapter-contract", "budgets", "context-policy", "diagnostics", "grammar", "orchestration", "prompt-assembly", "repair-policy", "retrieval", "system-prompt", "tool-policy", "transaction-policy"]),
            "modelSpecificPromptingAllowed": False, "scaffoldChangePolicy": "new-profile-and-cohort-required",
            "providerMutableFactsTreatment": "recorded-but-not-semantic-input",
        },
        "promptAssembly": {
            "kind": "genesis/reference-agent-prompt-assembly-v0.1", "algorithm": "sha256-domain-separated-ordered-artifacts-v0.1",
            "domain": "genesis-reference-agent-prompt-v0.1\\u0000", "orderedRoles": PROMPT_ROLES,
            "typedEnvelopeSchema": "genesis/reference-agent-request-v0.1", "taskMaySelectAuthority": False, "completeCaptureRequired": True,
        },
        "retrieval": {"algorithm": retrieval["algorithm"], "configArtifactId": "reference-retrieval-config", "runtimeArtifactId": "reference-agent-runtime", "integerOnly": True, "networkAllowed": False, "transcriptRequired": True},
        "orchestration": {"agentCount": 1, "subagentsAllowed": False, "providerToolsAllowed": False, "planner": "deterministic-finite-state-v0.1", "states": ["abort", "assemble", "diagnose", "finalize", "inspect", "model-call", "repair", "retrieve", "stage", "verify"], "terminalStates": ["abort", "finalize"]},
        "transaction": {"mode": "content-addressed-semantic-session-v0.1", "operations": SESSION_TOOLS, "directWritesAllowed": False, "textFallbackAllowed": False, "applyRequiresVerifiedSnapshot": True, "integrityFailurePolicy": "terminal-abort"},
        "loop": {
            "phases": ["assemble", "retrieve", "inspect", "model-call", "stage", "verify", "diagnose", "repair", "finalize"],
            "diagnosticAuthority": "gc-diagnostic-catalog", "repairAuthority": "gc-diagnostic-repair-plan",
            "repairTrigger": "failed-deterministic-verification-with-safe-repair",
            "stopConditions": ["authority-violation", "budget-exhausted", "complete", "integrity-failure", "invalid-response", "no-safe-repair", "replay-mismatch"],
            "stopPrecedence": ["authority-violation", "integrity-failure", "replay-mismatch", "budget-exhausted", "invalid-response", "no-safe-repair", "complete"],
        },
        "grammar": {"id": "gc-agent-v0.3-coreform-grammar", "authorityArtifactId": "gc-agent-profile", "semanticRewriteAllowed": False, "acceptanceAuthority": "production-parser-and-obligations"},
        "tool": {"interfaceId": "genesis/mcp-interface-v0.1", "protocolVersion": "2025-11-25", "catalogArtifactId": "mcp-catalog-source", "operations": parse_mcp_tools(), "ambientAuthorityAllowed": False, "completeTranscriptRequired": True},
        "budgets": {"maxModelCalls": 2, "maxRepairCalls": 1, "maxRetrievalQueries": 1, "maxToolCalls": 64, "maxOutputBytes": 1048576, "maxContextBytes": 8388608, "wallTimeMs": 600000, "cpuTimeMs": 300000, "heapBytes": 1073741824, "workspaceGrowthBytes": 268435456, "processes": 8, "effects": 4096, "semanticSteps": 100000000},
        "adapter": {"contract": "genesisbench-reference-adapter-v0.1", "requestMapping": "typed-lossless-no-authority-injection", "responseMapping": "typed-lossless-no-semantic-rewrite", "semanticRewriteAllowed": False, "hiddenRetriesAllowed": False, "providerToolsAllowed": False, "secretPolicy": "redact-values-record-presence-and-policy", "owner": "R1.4.l"},
        "tracePolicy": {"allModelCallsRecorded": True, "allToolCallsRecorded": True, "allRetrievalsRecorded": True, "allDiagnosticsRecorded": True, "allPatchesRecorded": True, "allDecisionsRecorded": True, "allOutcomesRecorded": True, "sequenceRule": "strictly-increasing-contiguous-events", "redactionRule": "typed-marker-preserves-field-and-content-hash"},
        "contentIdentitySha256": "",
    }
    return identified(profile)


def features(values: tuple[Any, ...]) -> dict[str, Any]:
    keys = ["context", "retrieval", "diagnostics", "editing", "grammar", "attempts", "repairs"]
    require(len(values) == len(keys), "ablation feature tuple drift")
    return dict(zip(keys, values))


def render_ablations(profile: dict[str, Any]) -> dict[str, Any]:
    suite = load_json(SUITE_PATH)
    lineages = [{"id": row["id"], "taskClass": row["taskClass"], "lineageIdentitySha256": row["contentIdentitySha256"]} for row in suite["lineages"]]
    require(len(lineages) == 9 and [row["id"] for row in lineages] == sorted(row["id"] for row in lineages), "reference matrix requires nine sorted immutable lineages")
    ablations = []
    for ablation_id, (ordinal, purpose, values, baseline, dimension) in sorted(ABLATION_SPECS.items()):
        feature_set = features(values)
        allowed = SESSION_TOOLS if feature_set["editing"] == "semantic-transaction-v0.1" else []
        ablations.append(identified({"id": ablation_id, "ordinal": ordinal, "purpose": purpose, "features": feature_set, "allowedTools": allowed, "contrast": {"baselineId": baseline, "dimension": dimension}}))
    conditions = []
    for lineage in lineages:
        for ablation in ablations:
            conditions.append(identified({
                "id": f"condition-{lineage['id'].removeprefix('lineage-')}-{ablation['id']}",
                "lineageId": lineage["id"], "lineageIdentitySha256": lineage["lineageIdentitySha256"],
                "ablationId": ablation["id"], "ablationIdentitySha256": ablation["contentIdentitySha256"],
                "samplingUnit": "lineage-not-condition", "oracleExposure": "same-as-lineage",
            }))
    document = {
        "kind": "genesis/genesisbench-reference-agent-ablations-v0.1", "version": "0.1.0",
        "profile": {"id": profile["id"], "path": PROFILE_PATH, "sha256": digest(pretty_bytes(profile)), "contentIdentitySha256": profile["contentIdentitySha256"]},
        "analysis": {
            "independentUnit": "lineageId", "conditionUnit": "conditionId", "repeatedConditionsIndependent": False,
            "comparisonPolicy": "predeclared-within-lineage-paired-contrasts",
            "requiredContrasts": ["bounded-repair-vs-one-attempt", "diagnostics-vs-retrieval", "fixed-context-vs-core-card-only", "grammar-constraint-vs-retrieval", "retrieval-vs-fixed-context", "semantic-patch-vs-retrieval"],
            "crossLineagePooling": "cluster-or-hierarchical-only",
        },
        "ablations": ablations, "lineages": lineages, "conditions": sorted(conditions, key=lambda row: row["id"]),
        "contentIdentitySha256": "",
    }
    return identified(document)


def tokens(text: str) -> list[str]:
    return TOKEN_RE.findall(text.lower())


def retrieve(query: str, config: dict[str, Any]) -> dict[str, Any]:
    query_tokens = sorted(set(tokens(query)))[:config["limits"]["maxUniqueQueryTokens"]]
    candidates = []
    documents: dict[str, list[str]] = {}
    for path in config["candidateArtifacts"]:
        payload = public_path(path).read_bytes()
        require(len(payload) <= config["limits"]["maxArtifactBytes"], f"retrieval artifact exceeds bound: {path}")
        documents[path] = tokens(payload.decode("utf-8", errors="strict"))
    frequencies = {token: sum(token in set(document) for document in documents.values()) for token in query_tokens}
    for path, document in documents.items():
        path_tokens = set(tokens(path))
        counts = {token: document.count(token) for token in query_tokens if token in document}
        score = sum(
            config["ranking"]["exactSymbolWeight"] * (1 if any(ch in token for ch in "._:/-") else 0)
            + config["ranking"]["pathTokenWeight"] * (1 if token in path_tokens else 0)
            + config["ranking"]["termFrequencyWeight"] * count
            - config["ranking"]["documentFrequencyPenalty"] * frequencies[token]
            for token, count in counts.items()
        )
        payload = public_path(path).read_bytes()
        candidates.append({"path": path, "bytes": len(payload), "sha256": digest(payload), "score": score, "matchedTokens": sorted(counts)})
    candidates.sort(key=lambda row: (-row["score"], row["path"], row["sha256"]))
    selected, total = [], 0
    for row in candidates:
        if len(selected) == config["limits"]["maxResults"] or total + row["bytes"] > config["limits"]["maxTotalBytes"]:
            continue
        selected.append(row)
        total += row["bytes"]
    return identified({
        "kind": "genesis/reference-agent-retrieval-transcript-v0.1", "algorithm": config["algorithm"],
        "configIdentitySha256": config["contentIdentitySha256"], "queryIdentitySha256": digest(query.encode("utf-8")),
        "queryTokens": query_tokens, "results": selected, "totalBytes": total,
    })


def find_case(case_id: str) -> dict[str, Any]:
    cases = [row for row in load_json(SUITE_PATH)["cases"] if row["id"] == case_id]
    require(len(cases) == 1, f"unknown or duplicate benchmark case: {case_id}")
    return cases[0]


def compile_plan(case_id: str, ablation_id: str, profile: dict[str, Any], ablations: dict[str, Any], retrieval: dict[str, Any]) -> dict[str, Any]:
    case = find_case(case_id)
    rows = [row for row in ablations["ablations"] if row["id"] == ablation_id]
    require(len(rows) == 1, f"unknown ablation: {ablation_id}")
    ablation = rows[0]
    condition_id = f"condition-{case['lineageId'].removeprefix('lineage-')}-{ablation_id}"
    condition = next(row for row in ablations["conditions"] if row["id"] == condition_id)
    input_rows, input_text = [], []
    for row in case["inputFiles"]:
        path = f"{case['inputRoot']}/{row['path']}"
        payload = public_path(path).read_bytes()
        require(len(payload) == row["bytes"] and digest(payload) == row["sha256"], f"case input identity drift: {path}")
        input_rows.append({"path": row["path"], "bytes": len(payload), "sha256": digest(payload)})
        input_text.append(payload.decode("utf-8", errors="strict"))
    feature_set = ablation["features"]
    context: dict[str, Any]
    if feature_set["retrieval"] == "integer-lexical-v0.1":
        context = retrieve(case["prompt"] + "\n" + "\n".join(input_text), retrieval)
    else:
        paths = [FIXED_CONTEXT[0]] if feature_set["context"] == "core-card-only" else FIXED_CONTEXT
        context = identified({"kind": "genesis/reference-agent-fixed-context-v0.1", "artifacts": [artifact(path) for path in paths]})
    prompt_segments = [
        {"role": "system-policy", "identitySha256": artifact(SYSTEM_PATH)["sha256"]},
        {"role": "agent-profile", "identitySha256": profile["contentIdentitySha256"]},
        {"role": "task-card", "identitySha256": artifact("docs/spec/GC_AGENT_TASK_CARDS_v0.3.md")["sha256"]},
        {"role": "context-pack-or-retrieval-transcript", "identitySha256": context["contentIdentitySha256"]},
        {"role": "task-prompt", "identitySha256": digest(case["prompt"].encode("utf-8"))},
        {"role": "task-inputs", "identitySha256": digest(canonical_bytes(input_rows))},
    ]
    plan = {
        "kind": "genesis/genesisbench-reference-agent-plan-v0.1", "version": "0.1.0",
        "profileIdentitySha256": profile["contentIdentitySha256"], "conditionId": condition_id,
        "conditionIdentitySha256": condition["contentIdentitySha256"], "caseId": case_id,
        "lineageId": case["lineageId"], "lineageIdentitySha256": condition["lineageIdentitySha256"],
        "features": feature_set, "promptSegments": prompt_segments, "context": context,
        "task": {"prompt": case["prompt"], "inputRoot": case["inputRoot"], "inputs": input_rows},
        "toolAllowlist": ablation["allowedTools"],
        "budgets": {**profile["budgets"], "maxModelCalls": feature_set["attempts"], "maxRepairCalls": feature_set["repairs"], "maxRetrievalQueries": 1 if feature_set["retrieval"] != "disabled" else 0},
        "workflow": profile["loop"]["phases"], "oracleAccessed": False, "contentIdentitySha256": "",
    }
    return identified(plan)


def event(index: int, kind: str, attempt: int, operation: str, decision: str) -> dict[str, Any]:
    return {"index": index, "kind": kind, "attempt": attempt, "operation": operation, "inputIdentitySha256": digest(f"input:{index}:{operation}".encode()), "outputIdentitySha256": digest(f"output:{index}:{operation}".encode()), "decision": decision}


def render_trace(profile: dict[str, Any], ablations: dict[str, Any]) -> dict[str, Any]:
    condition = next(row for row in ablations["conditions"] if row["id"] == "condition-generation-001-bounded-repair")
    trace = {
        "kind": "genesis/genesisbench-reference-agent-trace-v0.1", "version": "0.1.0",
        "profileIdentitySha256": profile["contentIdentitySha256"], "conditionIdentitySha256": condition["contentIdentitySha256"],
        "agentCount": 1,
        "adapter": {"id": "fixture-lossless-adapter-v0.1", "conformanceIdentitySha256": digest(b"fixture-lossless-adapter-v0.1"), "providerToolsUsed": False, "hiddenRetries": 0, "semanticRewrite": False},
        "events": [
            event(0, "retrieval", 0, "integer-lexical-retrieval", "captured"),
            event(1, "model-call", 0, "candidate-response", "recorded"),
            event(2, "tool-call", 0, "session-begin", "allowed"),
            event(3, "tool-call", 0, "session-stage", "allowed"),
            event(4, "tool-call", 0, "session-test", "failed"),
            event(5, "diagnostic", 0, "structured-catalog", "safe-repair"),
            event(6, "model-call", 1, "bounded-repair", "recorded"),
            event(7, "tool-call", 1, "session-stage", "allowed"),
            event(8, "tool-call", 1, "session-test", "verified"),
            event(9, "tool-call", 1, "session-apply", "verified-snapshot"),
            event(10, "outcome", 1, "finalize", "verified"),
        ],
        "outcome": "verified", "contentIdentitySha256": "",
    }
    return identified(trace)


def render_all() -> dict[str, Any]:
    retrieval = render_retrieval()
    profile = render_profile(retrieval)
    ablations = render_ablations(profile)
    return {
        "retrieval": retrieval, "profile": profile, "ablations": ablations,
        "plan": compile_plan("generation-small", "retrieval", profile, ablations, retrieval),
        "trace": render_trace(profile, ablations),
    }


def write_all(rendered: dict[str, Any]) -> None:
    outputs = {
        "retrieval": RETRIEVAL_PATH,
        "profile": PROFILE_PATH,
        "ablations": ABLATIONS_PATH,
        "plan": PLAN_FIXTURE_PATH,
        "trace": TRACE_FIXTURE_PATH,
    }
    for key, path in outputs.items():
        (ROOT / path).write_text(
            json.dumps(rendered[key], indent=2, sort_keys=True) + "\n",
            encoding="ascii",
        )


def validate_plan(plan: dict[str, Any], expected: dict[str, Any]) -> None:
    require(plan == expected, "reference agent plan drift")
    require(plan["oracleAccessed"] is False and "referenceRoot" not in json.dumps(plan), "plan accessed an oracle path")
    require([row["role"] for row in plan["promptSegments"]] == PROMPT_ROLES, "prompt role order drift")


def validate_trace(trace: dict[str, Any], profile: dict[str, Any], ablations: dict[str, Any]) -> None:
    require(set(trace) == {"kind", "version", "profileIdentitySha256", "conditionIdentitySha256", "agentCount", "adapter", "events", "outcome", "contentIdentitySha256"}, "trace fields are not closed")
    require(trace["kind"] == "genesis/genesisbench-reference-agent-trace-v0.1" and trace["version"] == "0.1.0", "trace version drift")
    require(trace["profileIdentitySha256"] == profile["contentIdentitySha256"] and trace["agentCount"] == 1, "trace profile or agent count drift")
    conditions = {row["contentIdentitySha256"]: row for row in ablations["conditions"]}
    require(trace["conditionIdentitySha256"] in conditions, "trace condition binding drift")
    condition = conditions[trace["conditionIdentitySha256"]]
    ablation = next(row for row in ablations["ablations"] if row["id"] == condition["ablationId"])
    adapter = trace["adapter"]
    require(set(adapter) == {"id", "conformanceIdentitySha256", "providerToolsUsed", "hiddenRetries", "semanticRewrite"}, "trace adapter fields are not closed")
    require(adapter["providerToolsUsed"] is False and adapter["hiddenRetries"] == 0 and adapter["semanticRewrite"] is False, "adapter concealed authority or semantic work")
    events = trace["events"]
    require(isinstance(events, list) and 1 <= len(events) <= 256, "invalid event count")
    require([row.get("index") for row in events] == list(range(len(events))), "trace events are not contiguous")
    for row in events:
        require(set(row) == {"index", "kind", "attempt", "operation", "inputIdentitySha256", "outputIdentitySha256", "decision"}, "trace event fields are not closed")
        require(row["kind"] in {"decision", "diagnostic", "model-call", "outcome", "retrieval", "tool-call"}, "unknown trace event kind")
        require(isinstance(row["attempt"], int) and 0 <= row["attempt"] < ablation["features"]["attempts"], "trace attempt exceeds declared budget")
        require(SHA_RE.fullmatch(row["inputIdentitySha256"]) is not None and SHA_RE.fullmatch(row["outputIdentitySha256"]) is not None, "invalid trace event identity")
        if row["kind"] == "tool-call":
            require(row["operation"] in ablation["allowedTools"], "trace used an undeclared tool")
    model_calls = [row for row in events if row["kind"] == "model-call"]
    require(len(model_calls) <= ablation["features"]["attempts"], "trace exceeds model-call budget")
    repairs = [row for row in model_calls if row["attempt"] > 0]
    require(len(repairs) <= ablation["features"]["repairs"], "trace exceeds repair budget")
    retrievals = [row for row in events if row["kind"] == "retrieval"]
    require(len(retrievals) == (1 if ablation["features"]["retrieval"] != "disabled" else 0), "trace retrieval count drift")
    require(trace["outcome"] in {"abstained", "failed", "invalid", "verified"}, "invalid trace outcome")
    require(events[-1]["kind"] == "outcome" and events[-1]["decision"] == trace["outcome"], "trace lacks matching terminal outcome")
    if trace["outcome"] == "verified":
        require(any(row["kind"] == "tool-call" and row["operation"] in {"session-test", "verify"} and row["decision"] == "verified" for row in events), "verified trace lacks deterministic verification")
    require(trace["contentIdentitySha256"] == object_identity(trace), "trace identity drift")


def validate_all(rendered: dict[str, Any]) -> None:
    for path, label in [(PROFILE_SCHEMA_PATH, "profile"), (ABLATIONS_SCHEMA_PATH, "ablations"), (TRACE_SCHEMA_PATH, "trace")]:
        validate_schema_markers(load_json(path), label)
    require(load_json(RETRIEVAL_PATH) == rendered["retrieval"], "reference retrieval config drift; run scripts/lib/genesisbench_reference_agent.py --write")
    require(load_json(PROFILE_PATH) == rendered["profile"], "reference agent profile drift; run scripts/lib/genesisbench_reference_agent.py --write")
    ablations = load_json(ABLATIONS_PATH)
    require(ablations == rendered["ablations"], "reference agent ablations drift; run scripts/lib/genesisbench_reference_agent.py --write")
    require(len(ablations["ablations"]) == 8 and len(ablations["lineages"]) == 9 and len(ablations["conditions"]) == 72, "reference ablation matrix is incomplete")
    require(len({row["ordinal"] for row in ablations["ablations"]}) == 8, "ablation ordinals are not unique")
    require(all(row["samplingUnit"] == "lineage-not-condition" for row in ablations["conditions"]), "conditions are incorrectly treated as independent")
    validate_plan(load_json(PLAN_FIXTURE_PATH), rendered["plan"])
    validate_trace(load_json(TRACE_FIXTURE_PATH), rendered["profile"], ablations)


def self_test(rendered: dict[str, Any]) -> int:
    controls = 0

    def rejected(value: dict[str, Any], validator: Callable[[dict[str, Any]], None], label: str) -> None:
        nonlocal controls
        try:
            validator(value)
        except (ReferenceAgentError, KeyError, TypeError):
            controls += 1
        else:
            raise ReferenceAgentError(f"negative control was accepted: {label}")

    plan = rendered["plan"]
    for label, mutate in [
        ("oracle", lambda d: d.__setitem__("oracleAccessed", True)),
        ("reference-path", lambda d: d["task"].__setitem__("inputRoot", "benchmarks/agent_tasks/v0.1/references/generation")),
        ("role-order", lambda d: d["promptSegments"].reverse()),
        ("task-authority", lambda d: d["promptSegments"].append({"role": "system-policy", "identitySha256": "0" * 64})),
        ("tool-broadening", lambda d: d["toolAllowlist"].append("run")),
        ("budget-broadening", lambda d: d["budgets"].__setitem__("maxModelCalls", 3)),
    ]:
        candidate = copy.deepcopy(plan); mutate(candidate)
        rejected(candidate, lambda d: validate_plan(d, plan), label)

    trace = rendered["trace"]
    trace_mutations = [
        ("event-gap", lambda d: d["events"][1].__setitem__("index", 3)),
        ("hidden-retry", lambda d: d["adapter"].__setitem__("hiddenRetries", 1)),
        ("provider-tool", lambda d: d["adapter"].__setitem__("providerToolsUsed", True)),
        ("semantic-rewrite", lambda d: d["adapter"].__setitem__("semanticRewrite", True)),
        ("second-agent", lambda d: d.__setitem__("agentCount", 2)),
        ("unknown-tool", lambda d: d["events"][2].__setitem__("operation", "shell")),
        ("attempt-overrun", lambda d: d["events"][6].__setitem__("attempt", 2)),
        ("model-overrun", lambda d: d["events"].insert(6, event(6, "model-call", 0, "extra", "recorded"))),
        ("false-verified", lambda d: d["events"][8].__setitem__("decision", "failed")),
        ("missing-outcome", lambda d: d["events"].pop()),
        ("identity-tamper", lambda d: d.__setitem__("contentIdentitySha256", "0" * 64)),
    ]
    for label, mutate in trace_mutations:
        candidate = copy.deepcopy(trace); mutate(candidate)
        rejected(candidate, lambda d: validate_trace(d, rendered["profile"], rendered["ablations"]), label)

    profile_mutations = [
        ("model-prompt", lambda d: d["comparisonPolicy"].__setitem__("modelSpecificPromptingAllowed", True)),
        ("subagents", lambda d: d["orchestration"].__setitem__("subagentsAllowed", True)),
        ("provider-tools", lambda d: d["orchestration"].__setitem__("providerToolsAllowed", True)),
        ("direct-writes", lambda d: d["transaction"].__setitem__("directWritesAllowed", True)),
        ("text-fallback", lambda d: d["transaction"].__setitem__("textFallbackAllowed", True)),
        ("grammar-rewrite", lambda d: d["grammar"].__setitem__("semanticRewriteAllowed", True)),
        ("ambient-authority", lambda d: d["tool"].__setitem__("ambientAuthorityAllowed", True)),
        ("network-retrieval", lambda d: d["retrieval"].__setitem__("networkAllowed", True)),
    ]
    for label, mutate in profile_mutations:
        candidate = copy.deepcopy(rendered["profile"]); mutate(candidate)
        rejected(candidate, lambda d: require(d == rendered["profile"], "profile drift"), label)
    return controls


def main() -> int:
    parser = argparse.ArgumentParser(description=__doc__)
    mode = parser.add_mutually_exclusive_group(required=True)
    mode.add_argument("--check", action="store_true")
    mode.add_argument("--render", action="store_true")
    mode.add_argument("--write", action="store_true")
    mode.add_argument("--plan", action="store_true")
    parser.add_argument("--self-test", action="store_true")
    parser.add_argument("--case", default="generation-small")
    parser.add_argument("--ablation", choices=sorted(ABLATION_SPECS), default="retrieval")
    try:
        rendered = render_all()
        if args := parser.parse_args():
            if args.render:
                json.dump(rendered, sys.stdout, indent=2, sort_keys=True); sys.stdout.write("\n")
            elif args.write:
                write_all(rendered)
                validate_all(rendered)
                print(f"GenesisBench reference agent refreshed: profile={rendered['profile']['contentIdentitySha256']}")
            elif args.plan:
                json.dump(compile_plan(args.case, args.ablation, rendered["profile"], rendered["ablations"], rendered["retrieval"]), sys.stdout, indent=2, sort_keys=True); sys.stdout.write("\n")
            else:
                validate_all(rendered)
                controls = self_test(rendered) if args.self_test else 0
                print(f"GenesisBench reference agent OK: profile={rendered['profile']['contentIdentitySha256']} ablations=8 lineages=9 conditions=72 controls={controls}")
        return 0
    except (OSError, UnicodeError, json.JSONDecodeError, ReferenceAgentError) as exc:
        print(f"genesisbench reference agent error: {exc}", file=sys.stderr)
        return 1


if __name__ == "__main__":
    raise SystemExit(main())
