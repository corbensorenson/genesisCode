#!/usr/bin/env python3
"""Independently verify the diagnostic repair-utility corpus and report."""

from __future__ import annotations

import argparse
import ast
import copy
from hashlib import sha256
import json
from pathlib import Path, PurePosixPath
import re
import sys
from typing import Any, Mapping, Sequence


ROOT = Path(__file__).resolve().parents[2]
POLICY_PATH = ROOT / "policies/gc_repair_utility_v0.1.json"
WORKLOAD_PATH = ROOT / "benchmarks/diagnostics/repair_utility/v0.1/workloads.json"
REPORT_PATH = ROOT / "benchmarks/diagnostics/repair_utility/v0.1/report.json"
SCHEMA_PATH = ROOT / "docs/spec/GC_REPAIR_UTILITY_REPORT_v0.1.schema.json"
AGENT_PATH = ROOT / "tools/genesis-reference-repair-agent.py"
ARTIFACT_PATH = ROOT / "selfhost/toolchain.gc"
CATALOG_PATH = ROOT / "docs/spec/GC_DIAGNOSTIC_CATALOG_v0.1.json"
SHA_RE = re.compile(r"^[0-9a-f]{64}$")
HOST_PATH_RE = re.compile(r"(?:^|[\s=:])(?:/Users/|/home/|/private/|/tmp/|[A-Za-z]:[\\/])")
FAMILIES = (
    "capability-policy-denial",
    "extra-closing-delimiter",
    "integer-literal-type",
    "missing-closing-delimiter",
    "primitive-name-edit",
    "unsupported-package-schema",
)
AGENT_IDS = ("catalog-guided-v0.1", "diagnostic-blind-v0.1")


class UtilityError(ValueError):
    pass


def load_json(path: Path) -> Any:
    def reject_duplicates(pairs: Sequence[tuple[str, Any]]) -> dict[str, Any]:
        result: dict[str, Any] = {}
        for key, value in pairs:
            if key in result:
                raise UtilityError(f"duplicate JSON key in {path}: {key}")
            result[key] = value
        return result

    try:
        return json.loads(path.read_text(encoding="utf-8"), object_pairs_hook=reject_duplicates)
    except (OSError, json.JSONDecodeError) as exc:
        raise UtilityError(f"cannot load {path}: {exc}") from exc


def closed(value: Any, fields: set[str], context: str) -> Mapping[str, Any]:
    if not isinstance(value, dict) or set(value) != fields:
        actual = set(value) if isinstance(value, dict) else {type(value).__name__}
        raise UtilityError(f"{context} field mismatch: expected={sorted(fields)} actual={sorted(actual)}")
    return value


def require_sha(value: Any, context: str) -> str:
    if not isinstance(value, str) or not SHA_RE.fullmatch(value):
        raise UtilityError(f"{context} must be lowercase sha256")
    return value


def digest_bytes(value: bytes) -> str:
    return sha256(value).hexdigest()


def canonical_bytes(value: Any) -> bytes:
    return (json.dumps(value, sort_keys=True, separators=(",", ":")) + "\n").encode()


def stable_path(raw: Any, context: str) -> str:
    if not isinstance(raw, str) or not raw:
        raise UtilityError(f"{context} must be a path string")
    path = PurePosixPath(raw)
    if path.is_absolute() or ".." in path.parts or "." in path.parts or "\\" in raw:
        raise UtilityError(f"{context} must be normalized and repository-relative")
    return path.as_posix()


def reject_host_paths(value: Any, context: str = "report") -> None:
    if isinstance(value, str) and HOST_PATH_RE.search(value):
        raise UtilityError(f"{context} contains a host path")
    if isinstance(value, list):
        for index, item in enumerate(value):
            reject_host_paths(item, f"{context}[{index}]")
    if isinstance(value, dict):
        for key, item in value.items():
            reject_host_paths(item, f"{context}.{key}")


def validate_policy(policy: Any) -> None:
    closed(
        policy,
        {
            "kind", "version", "workload", "report", "reportSchema", "referenceAgent",
            "agents", "maxRepairTurns", "tokenization", "acceptance",
        },
        "policy",
    )
    if policy["kind"] != "genesis/repair-utility-policy-v0.1" or policy["version"] != "0.1.0":
        raise UtilityError("policy kind/version drift")
    expected_paths = {
        "workload": WORKLOAD_PATH.relative_to(ROOT).as_posix(),
        "report": REPORT_PATH.relative_to(ROOT).as_posix(),
        "reportSchema": SCHEMA_PATH.relative_to(ROOT).as_posix(),
        "referenceAgent": AGENT_PATH.relative_to(ROOT).as_posix(),
    }
    for field, expected in expected_paths.items():
        if policy[field] != expected:
            raise UtilityError(f"policy {field} drift")
    if policy["maxRepairTurns"] != 2:
        raise UtilityError("maxRepairTurns must remain two")
    tokenization = closed(policy["tokenization"], {"profile", "definition", "fidelity"}, "tokenization")
    if tokenization != {
        "profile": "genesis/utf8-byte-token-v0.1",
        "definition": "One token is one byte of canonical UTF-8 request or response JSON.",
        "fidelity": "exact",
    }:
        raise UtilityError("tokenization contract drift")
    agents = policy["agents"]
    if not isinstance(agents, list) or len(agents) != 2:
        raise UtilityError("exactly two reference agents are required")
    if [agent["id"] for agent in agents] != ["catalog-guided-v0.1", "diagnostic-blind-v0.1"]:
        raise UtilityError("reference agent order/identity drift")
    roles = set()
    for index, agent in enumerate(agents):
        closed(agent, {"id", "role", "context", "model", "runtime", "decoding", "seed"}, f"agents[{index}]")
        roles.add(agent["role"])
        if agent["model"] != "deterministic-local-reference-agent":
            raise UtilityError("reference model must remain pinned and local")
        if agent["runtime"] != "python-isolated-stdio-v0.1" or agent["decoding"] != "deterministic-no-sampling":
            raise UtilityError("reference runtime/decoding drift")
        if agent["seed"] is not None or not isinstance(agent["context"], str) or not agent["context"]:
            raise UtilityError("reference agent context/seed contract drift")
    if roles != {"primary", "baseline"}:
        raise UtilityError("reference agents must define primary and baseline roles")
    acceptance = closed(
        policy["acceptance"],
        {
            "primaryRecoveryRateBasisPoints", "primarySafeAbstentionRateBasisPoints",
            "maxOverRepairs", "maxPolicyBroadenings", "maxRegressions",
            "requireInitialDiagnosticMatch", "requireDeterministicReplay",
        },
        "acceptance",
    )
    if acceptance["primaryRecoveryRateBasisPoints"] < 8500:
        raise UtilityError("AB-6 recovery threshold may not be below 85%")
    if acceptance["primarySafeAbstentionRateBasisPoints"] != 10000:
        raise UtilityError("review-required safe abstention must remain 100%")
    if any(acceptance[field] != 0 for field in ("maxOverRepairs", "maxPolicyBroadenings", "maxRegressions")):
        raise UtilityError("repair safety budgets must remain zero")
    if acceptance["requireInitialDiagnosticMatch"] is not True or acceptance["requireDeterministicReplay"] is not True:
        raise UtilityError("diagnostic matching and deterministic replay are mandatory")


def validate_agent_isolation(workload: Mapping[str, Any]) -> None:
    source = AGENT_PATH.read_text(encoding="utf-8")
    tree = ast.parse(source, filename=AGENT_PATH.as_posix())
    allowed_imports = {"__future__", "argparse", "hashlib", "json", "re", "sys", "typing"}
    observed_imports: set[str] = set()
    forbidden_calls = {"open", "compile", "eval", "exec", "__import__"}
    for node in ast.walk(tree):
        if isinstance(node, ast.Import):
            observed_imports.update(alias.name.split(".")[0] for alias in node.names)
        elif isinstance(node, ast.ImportFrom):
            observed_imports.add((node.module or "").split(".")[0])
        elif isinstance(node, ast.Call) and isinstance(node.func, ast.Name) and node.func.id in forbidden_calls:
            raise UtilityError(f"reference agent uses forbidden call: {node.func.id}")
    if not observed_imports <= allowed_imports:
        raise UtilityError(f"reference agent import surface broadened: {sorted(observed_imports - allowed_imports)}")
    forbidden_literals = {"workloads.json", "expectedDiagnostic", '"expected"', "caseId"}
    forbidden_literals.update(case["id"] for case in workload["cases"])
    leaked = sorted(literal for literal in forbidden_literals if literal in source)
    if leaked:
        raise UtilityError(f"reference agent contains oracle material: {leaked}")


def validate_workload(workload: Any) -> None:
    closed(workload, {"kind", "version", "caseCount", "mutationFamilies", "cases"}, "workload")
    if workload["kind"] != "genesis/repair-utility-workloads-v0.1" or workload["version"] != "0.1.0":
        raise UtilityError("workload kind/version drift")
    if tuple(workload["mutationFamilies"]) != FAMILIES:
        raise UtilityError("mutation family coverage/order drift")
    cases = workload["cases"]
    if not isinstance(cases, list) or workload["caseCount"] != len(cases) or len(cases) != 18:
        raise UtilityError("workload must contain exactly 18 cases")
    ids = [case["id"] for case in cases]
    if ids != sorted(ids) or len(ids) != len(set(ids)):
        raise UtilityError("workload case IDs must be sorted and unique")
    family_counts = {family: 0 for family in FAMILIES}
    repairability_counts = {"automatic": 0, "review-required": 0}
    for index, case in enumerate(cases):
        context = f"cases[{index}]"
        closed(case, {"id", "family", "repairability", "command", "verification", "expectedDiagnostic", "files"}, context)
        if case["family"] not in family_counts:
            raise UtilityError(f"{context}.family is unknown")
        family_counts[case["family"]] += 1
        if case["repairability"] not in repairability_counts:
            raise UtilityError(f"{context}.repairability is unknown")
        repairability_counts[case["repairability"]] += 1
        if not isinstance(case["command"], list) or not case["command"] or not all(isinstance(item, str) and item for item in case["command"]):
            raise UtilityError(f"{context}.command must be a nonempty string array")
        if not isinstance(case["verification"], list) or not all(isinstance(command, list) and command for command in case["verification"]):
            raise UtilityError(f"{context}.verification must contain command arrays")
        expected_diag = closed(case["expectedDiagnostic"], {"code", "domain", "kind", "actionId", "automaticAllowed"}, f"{context}.expectedDiagnostic")
        if expected_diag["automaticAllowed"] != (case["repairability"] == "automatic"):
            raise UtilityError(f"{context} repairability disagrees with diagnostic authorization")
        files = case["files"]
        if not isinstance(files, list) or not files:
            raise UtilityError(f"{context}.files must not be empty")
        paths: list[str] = []
        mutations = 0
        policy_files = 0
        for file_index, record in enumerate(files):
            file_context = f"{context}.files[{file_index}]"
            closed(record, {"path", "role", "mutable", "mutated", "expected"}, file_context)
            paths.append(stable_path(record["path"], f"{file_context}.path"))
            if record["role"] not in {"source", "manifest", "policy"}:
                raise UtilityError(f"{file_context}.role is unknown")
            if not isinstance(record["mutable"], bool) or not isinstance(record["mutated"], str) or not isinstance(record["expected"], str):
                raise UtilityError(f"{file_context} content/mutability shape drift")
            if record["role"] == "policy":
                policy_files += 1
                if record["mutable"] or record["mutated"] != record["expected"]:
                    raise UtilityError(f"{file_context} policy must be immutable")
            if record["mutated"] != record["expected"]:
                mutations += 1
                if not record["mutable"]:
                    raise UtilityError(f"{file_context} mutation is not repairable")
        if paths != sorted(paths) or len(paths) != len(set(paths)):
            raise UtilityError(f"{context}.files must be sorted and unique")
        if case["repairability"] == "automatic" and mutations != 1:
            raise UtilityError(f"{context} automatic case must contain one exact mutation")
        if case["repairability"] == "review-required" and (mutations != 0 or policy_files != 1):
            raise UtilityError(f"{context} reviewed case must preserve one immutable policy")
    if set(family_counts.values()) != {3}:
        raise UtilityError(f"each mutation family must contain exactly three cases: {family_counts}")
    if repairability_counts != {"automatic": 15, "review-required": 3}:
        raise UtilityError(f"repairability split drift: {repairability_counts}")
    validate_agent_isolation(workload)


def validate_schema_contract(schema: Any) -> None:
    if schema.get("$schema") != "https://json-schema.org/draft/2020-12/schema":
        raise UtilityError("report schema must use Draft 2020-12")
    if schema.get("additionalProperties") is not False:
        raise UtilityError("report schema root must be closed")
    required = {
        "kind", "version", "ok", "runtime", "inputs", "agents", "caseCount", "resultCount",
        "mutationFamilies", "acceptance", "acceptanceChecks", "summaries", "primaryVsBaseline", "results",
    }
    if set(schema.get("required", [])) != required:
        raise UtilityError("report schema root required fields drift")
    for name, definition in schema.get("$defs", {}).items():
        if definition.get("type") == "object" and name != "hashMap":
            if definition.get("additionalProperties") is not False:
                raise UtilityError(f"schema object definition is open: {name}")


def rate(numerator: int, denominator: int) -> int:
    return 0 if denominator == 0 else (numerator * 10000) // denominator


def recompute_summary(agent_id: str, results: Sequence[Mapping[str, Any]]) -> dict[str, Any]:
    selected = [result for result in results if result["agentId"] == agent_id]
    automatic = [result for result in selected if result["repairability"] == "automatic"]
    guarded = [result for result in selected if result["repairability"] == "review-required"]
    recovered = sum(result["exactRecovery"] is True for result in automatic)
    abstained = sum(result["safeAbstention"] is True for result in guarded)
    return {
        "agentId": agent_id,
        "caseCount": len(selected),
        "automaticCaseCount": len(automatic),
        "reviewRequiredCaseCount": len(guarded),
        "exactRecoveryCount": recovered,
        "recoveryRateBasisPoints": rate(recovered, len(automatic)),
        "safeAbstentionCount": abstained,
        "safeAbstentionRateBasisPoints": rate(abstained, len(guarded)),
        "overRepairCount": sum(result["overRepair"] is True for result in selected),
        "policyBroadeningCount": sum(result["policyBroadening"] is True for result in selected),
        "regressionCount": sum(result["regression"] is True for result in selected),
        "initialDiagnosticMismatchCount": sum(result["initialDiagnosticMatch"] is not True for result in selected),
        "tokenCost": {
            "profile": "genesis/utf8-byte-token-v0.1",
            "input": sum(result["tokenCost"]["input"] for result in selected),
            "output": sum(result["tokenCost"]["output"] for result in selected),
            "total": sum(result["tokenCost"]["total"] for result in selected),
        },
    }


def validate_report(report: Any, policy: Mapping[str, Any], workload: Mapping[str, Any]) -> None:
    root_fields = {
        "kind", "version", "ok", "runtime", "inputs", "agents", "caseCount", "resultCount",
        "mutationFamilies", "acceptance", "acceptanceChecks", "summaries", "primaryVsBaseline", "results",
    }
    closed(report, root_fields, "report")
    reject_host_paths(report)
    if report["kind"] != "genesis/repair-utility-report-v0.1" or report["version"] != "0.1.0":
        raise UtilityError("report kind/version drift")
    runtime = closed(report["runtime"], {"name", "version", "profile", "selfhostArtifactSha256", "diagnosticCatalogIdentitySha256"}, "runtime")
    catalog = load_json(CATALOG_PATH)
    cargo = (ROOT / "Cargo.toml").read_text(encoding="utf-8")
    version_match = re.search(r"(?m)^version\s*=\s*\"([0-9]+\.[0-9]+\.[0-9]+)\"", cargo)
    if not version_match:
        raise UtilityError("cannot derive workspace version")
    expected_runtime = {
        "name": "genesis",
        "version": f"genesis {version_match.group(1)}",
        "profile": "host-cli/selfhost-artifact",
        "selfhostArtifactSha256": digest_bytes(ARTIFACT_PATH.read_bytes()),
        "diagnosticCatalogIdentitySha256": catalog["catalogIdentitySha256"],
    }
    if runtime != expected_runtime:
        raise UtilityError("report runtime identity drift")
    inputs = closed(report["inputs"], {"policySha256", "workloadSha256", "referenceAgentSha256"}, "inputs")
    expected_inputs = {
        "policySha256": digest_bytes(POLICY_PATH.read_bytes()),
        "workloadSha256": digest_bytes(WORKLOAD_PATH.read_bytes()),
        "referenceAgentSha256": digest_bytes(AGENT_PATH.read_bytes()),
    }
    if inputs != expected_inputs:
        raise UtilityError("report input identity drift")
    expected_agents = []
    for agent in sorted(policy["agents"], key=lambda item: item["id"]):
        expected_agents.append({
            **agent,
            "implementationSha256": expected_inputs["referenceAgentSha256"],
            "maxRepairTurns": policy["maxRepairTurns"],
            "tokenization": policy["tokenization"],
        })
    if report["agents"] != expected_agents:
        raise UtilityError("report pinned-agent metadata drift")
    if report["caseCount"] != 18 or report["resultCount"] != 36:
        raise UtilityError("report case/result count drift")
    if report["mutationFamilies"] != list(FAMILIES) or report["acceptance"] != policy["acceptance"]:
        raise UtilityError("report workload/acceptance projection drift")
    results = report["results"]
    expected_pairs = [(agent, case["id"]) for agent in AGENT_IDS for case in workload["cases"]]
    observed_pairs = [(result.get("agentId"), result.get("caseId")) for result in results]
    if observed_pairs != expected_pairs:
        raise UtilityError("report result matrix order/coverage drift")
    case_by_id = {case["id"]: case for case in workload["cases"]}
    for index, result in enumerate(results):
        context = f"results[{index}]"
        closed(
            result,
            {
                "agentId", "caseId", "family", "repairability", "mutationIdentitySha256",
                "initialDiagnostic", "initialDiagnosticMatch", "attemptCount", "attempts",
                "failureCodes", "changedPaths", "initialFileSha256", "finalFileSha256",
                "expectedFileSha256", "finalCommandOk", "verificationOk", "exactRecovery",
                "safeAbstention", "overRepair", "policyBroadening", "regression", "outcome", "tokenCost",
            },
            context,
        )
        case = case_by_id[result["caseId"]]
        if result["family"] != case["family"] or result["repairability"] != case["repairability"]:
            raise UtilityError(f"{context} workload metadata drift")
        if result["mutationIdentitySha256"] != digest_bytes(canonical_bytes(case)):
            raise UtilityError(f"{context} mutation identity drift")
        if result["initialDiagnostic"] != case["expectedDiagnostic"] or result["initialDiagnosticMatch"] is not True:
            raise UtilityError(f"{context} initial diagnostic drift")
        attempts = result["attempts"]
        if result["attemptCount"] != len(attempts) or not 1 <= len(attempts) <= policy["maxRepairTurns"]:
            raise UtilityError(f"{context} attempt bound/count drift")
        for attempt_index, attempt in enumerate(attempts):
            closed(attempt, {"turn", "decision", "reason", "patchPaths", "inputTokens", "outputTokens", "commandOkAfterTurn"}, f"{context}.attempts[{attempt_index}]")
            if attempt["turn"] != attempt_index + 1 or attempt["decision"] not in {"patch", "abstain"}:
                raise UtilityError(f"{context} attempt sequence drift")
            if attempt["patchPaths"] != sorted(set(attempt["patchPaths"])):
                raise UtilityError(f"{context} patch paths must be sorted and unique")
        token_cost = closed(result["tokenCost"], {"profile", "input", "output", "total"}, f"{context}.tokenCost")
        expected_input = sum(attempt["inputTokens"] for attempt in attempts)
        expected_output = sum(attempt["outputTokens"] for attempt in attempts)
        if token_cost != {"profile": "genesis/utf8-byte-token-v0.1", "input": expected_input, "output": expected_output, "total": expected_input + expected_output}:
            raise UtilityError(f"{context} token accounting drift")
        paths = [record["path"] for record in case["files"]]
        expected_initial = {record["path"]: digest_bytes(record["mutated"].encode()) for record in case["files"]}
        expected_final = {record["path"]: digest_bytes(record["expected"].encode()) for record in case["files"]}
        if result["initialFileSha256"] != expected_initial or result["expectedFileSha256"] != expected_final:
            raise UtilityError(f"{context} oracle hash drift")
        for field in ("initialFileSha256", "finalFileSha256", "expectedFileSha256"):
            if set(result[field]) != set(paths) or not all(SHA_RE.fullmatch(value) for value in result[field].values()):
                raise UtilityError(f"{context}.{field} shape drift")
        if result["changedPaths"] != sorted(set(result["changedPaths"])) or not set(result["changedPaths"]) <= set(paths):
            raise UtilityError(f"{context} changed path drift")
        flags = [result[field] is True for field in ("exactRecovery", "safeAbstention", "overRepair", "policyBroadening", "regression")]
        if sum(flags) > 1:
            raise UtilityError(f"{context} has conflicting outcomes")
        expected_outcome = (
            "exact-recovery" if result["exactRecovery"] else
            "safe-abstention" if result["safeAbstention"] else
            "over-repair" if result["overRepair"] else
            "policy-broadening" if result["policyBroadening"] else
            "regression" if result["regression"] else
            "unrecovered"
        )
        if result["outcome"] != expected_outcome:
            raise UtilityError(f"{context} outcome drift")
        if result["exactRecovery"] and (
            result["repairability"] != "automatic" or not result["finalCommandOk"]
            or not result["verificationOk"] or result["finalFileSha256"] != expected_final
        ):
            raise UtilityError(f"{context} invalid exact recovery claim")
        if result["safeAbstention"] and (
            result["repairability"] != "review-required" or result["changedPaths"]
            or result["finalFileSha256"] != expected_initial or attempts[-1]["decision"] != "abstain"
        ):
            raise UtilityError(f"{context} invalid safe abstention claim")
    expected_summaries = [recompute_summary(agent_id, results) for agent_id in AGENT_IDS]
    if report["summaries"] != expected_summaries:
        raise UtilityError("report summary arithmetic drift")
    primary, baseline = expected_summaries
    comparison = {
        "primaryAgentId": primary["agentId"],
        "baselineAgentId": baseline["agentId"],
        "additionalExactRecoveries": primary["exactRecoveryCount"] - baseline["exactRecoveryCount"],
        "recoveryRateDeltaBasisPoints": primary["recoveryRateBasisPoints"] - baseline["recoveryRateBasisPoints"],
        "tokenCostDelta": primary["tokenCost"]["total"] - baseline["tokenCost"]["total"],
    }
    if report["primaryVsBaseline"] != comparison:
        raise UtilityError("primary/baseline comparison drift")
    acceptance = policy["acceptance"]
    checks = {
        "initialDiagnosticsMatch": primary["initialDiagnosticMismatchCount"] == 0,
        "overRepairBound": primary["overRepairCount"] <= acceptance["maxOverRepairs"],
        "policyBroadeningBound": primary["policyBroadeningCount"] <= acceptance["maxPolicyBroadenings"],
        "recoveryRate": primary["recoveryRateBasisPoints"] >= acceptance["primaryRecoveryRateBasisPoints"],
        "regressionBound": primary["regressionCount"] <= acceptance["maxRegressions"],
        "safeAbstentionRate": primary["safeAbstentionRateBasisPoints"] >= acceptance["primarySafeAbstentionRateBasisPoints"],
    }
    if report["acceptanceChecks"] != checks or report["ok"] is not all(checks.values()):
        raise UtilityError("report acceptance decision drift")
    if report["ok"] is not True:
        raise UtilityError("checked-in repair utility report must pass")


def validate_all(report_path: Path = REPORT_PATH) -> None:
    policy = load_json(POLICY_PATH)
    workload = load_json(WORKLOAD_PATH)
    schema = load_json(SCHEMA_PATH)
    report = load_json(report_path)
    validate_policy(policy)
    validate_workload(workload)
    validate_schema_contract(schema)
    validate_report(report, policy, workload)


def self_test() -> None:
    policy = load_json(POLICY_PATH)
    workload = load_json(WORKLOAD_PATH)
    report = load_json(REPORT_PATH)
    controls: list[tuple[str, Any]] = []

    def mutate_report(name: str, mutation: Any) -> None:
        value = copy.deepcopy(report)
        mutation(value)
        controls.append((name, lambda value=value: validate_report(value, policy, workload)))

    def mutate_workload(name: str, mutation: Any) -> None:
        value = copy.deepcopy(workload)
        mutation(value)
        controls.append((name, lambda value=value: validate_workload(value)))

    mutate_workload("missing-case", lambda value: value["cases"].pop())
    mutate_workload("case-order", lambda value: value["cases"].reverse())
    mutate_workload("unknown-field", lambda value: value["cases"][0].__setitem__("oracleHint", "x"))
    mutate_workload("mutable-policy", lambda value: value["cases"][0]["files"][0].__setitem__("mutable", True))
    mutate_workload("erased-mutation", lambda value: value["cases"][3]["files"][0].__setitem__("mutated", value["cases"][3]["files"][0]["expected"]))
    mutate_report("input-identity", lambda value: value["inputs"].__setitem__("workloadSha256", "0" * 64))
    mutate_report("inflated-recovery", lambda value: value["results"][24].__setitem__("exactRecovery", True))
    mutate_report("hidden-policy-broadening", lambda value: value["results"][0].__setitem__("policyBroadening", True))
    mutate_report("token-total", lambda value: value["results"][0]["tokenCost"].__setitem__("total", 0))
    mutate_report("attempt-bound", lambda value: value["results"][0].__setitem__("attemptCount", 3))
    mutate_report("host-path", lambda value: value["results"][0]["attempts"][0].__setitem__("reason", "/Users/example/private"))
    mutate_report("summary-arithmetic", lambda value: value["summaries"][0].__setitem__("exactRecoveryCount", 14))
    mutate_report("comparison-arithmetic", lambda value: value["primaryVsBaseline"].__setitem__("additionalExactRecoveries", 99))
    mutate_report("diagnostic-route", lambda value: value["results"][0]["initialDiagnostic"].__setitem__("code", "caps/allowed"))
    mutate_report("unsafe-ok", lambda value: value["acceptanceChecks"].__setitem__("policyBroadeningBound", False))

    for name, control in controls:
        try:
            control()
        except UtilityError:
            continue
        raise UtilityError(f"self-test negative control was accepted: {name}")
    print(f"gc-repair-utility: self-test ok (negative_controls={len(controls)})")


def main(argv: Sequence[str]) -> int:
    parser = argparse.ArgumentParser()
    mode = parser.add_mutually_exclusive_group(required=True)
    mode.add_argument("--check", action="store_true")
    mode.add_argument("--self-test", action="store_true")
    parser.add_argument("--report", type=Path, default=REPORT_PATH)
    args = parser.parse_args(argv)
    try:
        if args.self_test:
            self_test()
        else:
            validate_all(args.report.resolve())
            report = load_json(args.report.resolve())
            primary = report["summaries"][0]
            comparison = report["primaryVsBaseline"]
            print(
                "gc-repair-utility: ok "
                f"cases={report['caseCount']} agents={len(report['agents'])} "
                f"recovery_bps={primary['recoveryRateBasisPoints']} "
                f"safe_abstention_bps={primary['safeAbstentionRateBasisPoints']} "
                f"recovery_lift_bps={comparison['recoveryRateDeltaBasisPoints']} "
                f"tokens={primary['tokenCost']['total']}"
            )
    except (UtilityError, KeyError, OSError, TypeError) as exc:
        print(f"gc-repair-utility: {exc}", file=sys.stderr)
        return 1
    return 0


if __name__ == "__main__":
    raise SystemExit(main(sys.argv[1:]))
