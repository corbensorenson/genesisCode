#!/usr/bin/env python3
"""Execute the deterministic diagnostic repair-utility benchmark."""

from __future__ import annotations

import argparse
from hashlib import sha256
import json
import os
from pathlib import Path, PurePosixPath
import subprocess
import sys
import tempfile
from typing import Any, Mapping, Sequence


ROOT = Path(__file__).resolve().parents[2]
REQUEST_SCHEMA = "genesis/reference-repair-request-v0.1"
RESPONSE_SCHEMA = "genesis/reference-repair-response-v0.1"
REPORT_KIND = "genesis/repair-utility-report-v0.1"


class RunnerError(ValueError):
    pass


def load_json(path: Path) -> Any:
    def reject_duplicates(pairs: Sequence[tuple[str, Any]]) -> dict[str, Any]:
        result: dict[str, Any] = {}
        for key, value in pairs:
            if key in result:
                raise RunnerError(f"duplicate JSON key in {path}: {key}")
            result[key] = value
        return result

    try:
        return json.loads(path.read_text(encoding="utf-8"), object_pairs_hook=reject_duplicates)
    except (OSError, json.JSONDecodeError) as exc:
        raise RunnerError(f"cannot load {path}: {exc}") from exc


def digest_bytes(value: bytes) -> str:
    return sha256(value).hexdigest()


def digest_text(value: str) -> str:
    return digest_bytes(value.encode())


def canonical_bytes(value: Any) -> bytes:
    return (json.dumps(value, sort_keys=True, separators=(",", ":")) + "\n").encode()


def stable_path(raw: str) -> str:
    path = PurePosixPath(raw)
    if path.is_absolute() or ".." in path.parts or "." in path.parts or "\\" in raw:
        raise RunnerError(f"non-normalized benchmark path: {raw}")
    return path.as_posix()


def run_genesis(binary: Path, artifact: Path, root: Path, command: Sequence[str]) -> tuple[bool, Any]:
    args = [str(binary), "--selfhost-artifact", str(artifact), "--json", *command]
    completed = subprocess.run(args, cwd=root, capture_output=True, check=False)
    payload = completed.stdout.strip()
    if not payload:
        raise RunnerError(
            f"genesis emitted no JSON for {' '.join(command)}: "
            f"exit={completed.returncode} stderr={completed.stderr.decode(errors='replace')[-400:]}"
        )
    try:
        envelope = json.loads(payload)
    except json.JSONDecodeError as exc:
        raise RunnerError(f"genesis emitted invalid JSON for {' '.join(command)}: {exc}") from exc
    return completed.returncode == 0, envelope


def diagnostic_projection(envelope: Mapping[str, Any], guided: bool) -> dict[str, Any]:
    diagnostics = envelope.get("diagnostics")
    if not isinstance(diagnostics, list) or not diagnostics or not isinstance(diagnostics[0], dict):
        raise RunnerError("failed command did not emit a structured diagnostic")
    diagnostic = diagnostics[0]
    if guided:
        fields = ("id", "code", "message", "context", "repair_plan")
    else:
        fields = ("message",)
    return {field: diagnostic.get(field) for field in fields}


def observed_diagnostic(envelope: Mapping[str, Any]) -> dict[str, Any]:
    diagnostic = envelope["diagnostics"][0]
    return {
        "code": diagnostic.get("code"),
        "domain": diagnostic.get("context", {}).get("domain"),
        "kind": diagnostic.get("context", {}).get("kind"),
        "actionId": diagnostic.get("repair_plan", {}).get("action", {}).get("id"),
        "automaticAllowed": diagnostic.get("repair_plan", {})
        .get("authorization", {})
        .get("automatic_allowed"),
    }


def authorization(envelope: Mapping[str, Any]) -> dict[str, Any]:
    raw = envelope["diagnostics"][0].get("repair_plan", {}).get("authorization", {})
    return {
        "automatic_allowed": raw.get("automatic_allowed"),
        "policy_change_allowed": raw.get("policy_change_allowed"),
        "obligation_suppression_allowed": raw.get("obligation_suppression_allowed"),
        "requires_review": raw.get("requires_review"),
    }


def invoke_agent(
    agent_path: Path,
    agent_id: str,
    request: Mapping[str, Any],
    cwd: Path,
) -> tuple[dict[str, Any], int, int]:
    request_bytes = canonical_bytes(request)
    env = {
        "HOME": str(cwd),
        "LANG": "C",
        "LC_ALL": "C",
        "PATH": os.environ.get("PATH", "/usr/bin:/bin"),
        "PYTHONHASHSEED": "0",
    }
    completed = subprocess.run(
        [sys.executable, "-I", str(agent_path), "--agent", agent_id],
        cwd=cwd,
        env=env,
        input=request_bytes,
        capture_output=True,
        check=False,
    )
    if completed.returncode != 0:
        raise RunnerError(
            f"reference agent {agent_id} failed: {completed.stderr.decode(errors='replace')[-400:]}"
        )
    try:
        response = json.loads(completed.stdout)
    except json.JSONDecodeError as exc:
        raise RunnerError(f"reference agent {agent_id} emitted invalid JSON: {exc}") from exc
    if set(response) != {"schema", "decision", "reason", "patches"}:
        raise RunnerError(f"reference agent {agent_id} response is not closed")
    if response["schema"] != RESPONSE_SCHEMA or response["decision"] not in {"patch", "abstain"}:
        raise RunnerError(f"reference agent {agent_id} response contract drift")
    if not isinstance(response["patches"], list):
        raise RunnerError(f"reference agent {agent_id} patches must be an array")
    return response, len(request_bytes), len(completed.stdout)


def write_case(root: Path, case: Mapping[str, Any]) -> dict[str, Mapping[str, Any]]:
    files: dict[str, Mapping[str, Any]] = {}
    for record in case["files"]:
        path = stable_path(record["path"])
        if path in files:
            raise RunnerError(f"duplicate case path: {path}")
        destination = root / path
        destination.parent.mkdir(parents=True, exist_ok=True)
        destination.write_text(record["mutated"], encoding="utf-8")
        files[path] = record
    return files


def current_files(root: Path, records: Mapping[str, Mapping[str, Any]]) -> dict[str, str]:
    return {path: (root / path).read_text(encoding="utf-8") for path in sorted(records)}


def apply_response(
    root: Path,
    records: Mapping[str, Mapping[str, Any]],
    response: Mapping[str, Any],
) -> tuple[list[str], bool, bool]:
    changed: list[str] = []
    over_repair = False
    policy_broadening = False
    seen: set[str] = set()
    for raw_patch in response["patches"]:
        if not isinstance(raw_patch, dict) or set(raw_patch) != {"path", "before_sha256", "content"}:
            over_repair = True
            continue
        path = stable_path(str(raw_patch["path"]))
        if path in seen or path not in records:
            over_repair = True
            continue
        seen.add(path)
        record = records[path]
        if record["role"] == "policy":
            policy_broadening = True
        if record["mutable"] is not True:
            over_repair = True
            continue
        before = (root / path).read_text(encoding="utf-8")
        if raw_patch["before_sha256"] != digest_text(before) or not isinstance(raw_patch["content"], str):
            over_repair = True
            continue
        (root / path).write_text(raw_patch["content"], encoding="utf-8")
        changed.append(path)
    if response["decision"] == "abstain" and response["patches"]:
        over_repair = True
    if response["decision"] == "patch" and not changed:
        over_repair = True
    return sorted(changed), over_repair, policy_broadening


def run_one(
    binary: Path,
    artifact: Path,
    agent_path: Path,
    agent_id: str,
    max_turns: int,
    case: Mapping[str, Any],
) -> dict[str, Any]:
    with tempfile.TemporaryDirectory(prefix="genesis-repair-case-") as raw_root:
        root = Path(raw_root)
        records = write_case(root, case)
        initial_hashes = {path: digest_text(text) for path, text in current_files(root, records).items()}
        command_ok, envelope = run_genesis(binary, artifact, root, case["command"])
        if command_ok:
            raise RunnerError(f"mutation {case['id']} did not fail before repair")
        expected_diag = case["expectedDiagnostic"]
        observed_diag = observed_diagnostic(envelope)
        diagnostic_match = observed_diag == expected_diag
        failure_codes = [str(observed_diag["code"])]
        attempts: list[dict[str, Any]] = []
        total_input_tokens = 0
        total_output_tokens = 0
        changed_paths: set[str] = set()
        over_repair = False
        policy_broadening = False
        last_decision = "none"
        final_command_ok = False
        final_envelope: Any = None

        for turn in range(1, max_turns + 1):
            guided = agent_id == "catalog-guided-v0.1"
            request = {
                "schema": REQUEST_SCHEMA,
                "command": list(case["command"]),
                "files": current_files(root, records),
                "diagnostic": diagnostic_projection(envelope, guided),
                "authorization": authorization(envelope),
            }
            response, input_tokens, output_tokens = invoke_agent(
                agent_path, agent_id, request, root
            )
            total_input_tokens += input_tokens
            total_output_tokens += output_tokens
            last_decision = response["decision"]
            applied, turn_over_repair, turn_policy_broadening = apply_response(
                root, records, response
            )
            changed_paths.update(applied)
            over_repair = over_repair or turn_over_repair
            policy_broadening = policy_broadening or turn_policy_broadening
            attempt = {
                "turn": turn,
                "decision": response["decision"],
                "reason": response["reason"],
                "patchPaths": applied,
                "inputTokens": input_tokens,
                "outputTokens": output_tokens,
            }
            if response["decision"] == "abstain" or turn_over_repair or turn_policy_broadening:
                attempt["commandOkAfterTurn"] = False
                attempts.append(attempt)
                break
            command_ok, envelope = run_genesis(binary, artifact, root, case["command"])
            final_command_ok = command_ok
            final_envelope = envelope if command_ok else None
            attempt["commandOkAfterTurn"] = command_ok
            attempts.append(attempt)
            if command_ok:
                break
            next_diag = observed_diagnostic(envelope)
            failure_codes.append(str(next_diag["code"]))

        final_contents = current_files(root, records)
        expected_match = all(final_contents[path] == record["expected"] for path, record in records.items())
        policy_unchanged = all(
            final_contents[path] == record["mutated"]
            for path, record in records.items()
            if record["role"] == "policy"
        )
        policy_broadening = policy_broadening or not policy_unchanged
        verification_ok = final_command_ok
        if final_command_ok:
            for verification in case["verification"]:
                check_ok, check_envelope = run_genesis(binary, artifact, root, verification)
                same_result = verification != case["command"] or check_envelope == final_envelope
                verification_ok = verification_ok and check_ok and same_result
        automatic = case["repairability"] == "automatic"
        exact_recovery = (
            automatic
            and final_command_ok
            and verification_ok
            and expected_match
            and not over_repair
            and not policy_broadening
        )
        safe_abstention = (
            not automatic
            and last_decision == "abstain"
            and not changed_paths
            and expected_match
            and not over_repair
            and not policy_broadening
        )
        if final_command_ok and not expected_match:
            over_repair = True
            exact_recovery = False
        regression = bool(changed_paths) and final_command_ok and not verification_ok
        if exact_recovery:
            outcome = "exact-recovery"
        elif safe_abstention:
            outcome = "safe-abstention"
        elif over_repair:
            outcome = "over-repair"
        elif policy_broadening:
            outcome = "policy-broadening"
        elif regression:
            outcome = "regression"
        else:
            outcome = "unrecovered"

        return {
            "agentId": agent_id,
            "caseId": case["id"],
            "family": case["family"],
            "repairability": case["repairability"],
            "mutationIdentitySha256": digest_bytes(canonical_bytes(case)),
            "initialDiagnostic": observed_diag,
            "initialDiagnosticMatch": diagnostic_match,
            "attemptCount": len(attempts),
            "attempts": attempts,
            "failureCodes": failure_codes,
            "changedPaths": sorted(changed_paths),
            "initialFileSha256": initial_hashes,
            "finalFileSha256": {
                path: digest_text(text) for path, text in sorted(final_contents.items())
            },
            "expectedFileSha256": {
                path: digest_text(record["expected"]) for path, record in sorted(records.items())
            },
            "finalCommandOk": final_command_ok,
            "verificationOk": verification_ok,
            "exactRecovery": exact_recovery,
            "safeAbstention": safe_abstention,
            "overRepair": over_repair,
            "policyBroadening": policy_broadening,
            "regression": regression,
            "outcome": outcome,
            "tokenCost": {
                "profile": "genesis/utf8-byte-token-v0.1",
                "input": total_input_tokens,
                "output": total_output_tokens,
                "total": total_input_tokens + total_output_tokens,
            },
        }


def rate_basis_points(numerator: int, denominator: int) -> int:
    return 0 if denominator == 0 else (numerator * 10000) // denominator


def summarize(agent_id: str, results: Sequence[Mapping[str, Any]]) -> dict[str, Any]:
    selected = [result for result in results if result["agentId"] == agent_id]
    automatic = [result for result in selected if result["repairability"] == "automatic"]
    guarded = [result for result in selected if result["repairability"] == "review-required"]
    recovered = sum(bool(result["exactRecovery"]) for result in automatic)
    abstained = sum(bool(result["safeAbstention"]) for result in guarded)
    return {
        "agentId": agent_id,
        "caseCount": len(selected),
        "automaticCaseCount": len(automatic),
        "reviewRequiredCaseCount": len(guarded),
        "exactRecoveryCount": recovered,
        "recoveryRateBasisPoints": rate_basis_points(recovered, len(automatic)),
        "safeAbstentionCount": abstained,
        "safeAbstentionRateBasisPoints": rate_basis_points(abstained, len(guarded)),
        "overRepairCount": sum(bool(result["overRepair"]) for result in selected),
        "policyBroadeningCount": sum(bool(result["policyBroadening"]) for result in selected),
        "regressionCount": sum(bool(result["regression"]) for result in selected),
        "initialDiagnosticMismatchCount": sum(
            not bool(result["initialDiagnosticMatch"]) for result in selected
        ),
        "tokenCost": {
            "profile": "genesis/utf8-byte-token-v0.1",
            "input": sum(int(result["tokenCost"]["input"]) for result in selected),
            "output": sum(int(result["tokenCost"]["output"]) for result in selected),
            "total": sum(int(result["tokenCost"]["total"]) for result in selected),
        },
    }


def render(binary: Path, artifact: Path, output: Path, policy_path: Path) -> None:
    policy = load_json(policy_path)
    workload_path = ROOT / policy["workload"]
    agent_path = ROOT / policy["referenceAgent"]
    workload = load_json(workload_path)
    cases = workload["cases"]
    if workload.get("caseCount") != len(cases):
        raise RunnerError("workload caseCount drift")
    if [case["id"] for case in cases] != sorted(case["id"] for case in cases):
        raise RunnerError("workload cases must be sorted by ID")
    agents = policy["agents"]
    results = [
        run_one(binary, artifact, agent_path, agent["id"], policy["maxRepairTurns"], case)
        for agent in sorted(agents, key=lambda item: item["id"])
        for case in cases
    ]
    summaries = [summarize(agent["id"], results) for agent in sorted(agents, key=lambda item: item["id"])]
    primary_id = next(agent["id"] for agent in agents if agent["role"] == "primary")
    baseline_id = next(agent["id"] for agent in agents if agent["role"] == "baseline")
    primary = next(summary for summary in summaries if summary["agentId"] == primary_id)
    baseline = next(summary for summary in summaries if summary["agentId"] == baseline_id)
    acceptance = policy["acceptance"]
    checks = {
        "initialDiagnosticsMatch": primary["initialDiagnosticMismatchCount"] == 0,
        "overRepairBound": primary["overRepairCount"] <= acceptance["maxOverRepairs"],
        "policyBroadeningBound": primary["policyBroadeningCount"]
        <= acceptance["maxPolicyBroadenings"],
        "recoveryRate": primary["recoveryRateBasisPoints"]
        >= acceptance["primaryRecoveryRateBasisPoints"],
        "regressionBound": primary["regressionCount"] <= acceptance["maxRegressions"],
        "safeAbstentionRate": primary["safeAbstentionRateBasisPoints"]
        >= acceptance["primarySafeAbstentionRateBasisPoints"],
    }
    version = subprocess.run([str(binary), "--version"], capture_output=True, check=True, text=True).stdout.strip()
    catalog = load_json(ROOT / "docs/spec/GC_DIAGNOSTIC_CATALOG_v0.1.json")
    report = {
        "kind": REPORT_KIND,
        "version": "0.1.0",
        "ok": all(checks.values()),
        "runtime": {
            "name": "genesis",
            "version": version,
            "profile": "host-cli/selfhost-artifact",
            "selfhostArtifactSha256": digest_bytes(artifact.read_bytes()),
            "diagnosticCatalogIdentitySha256": catalog["catalogIdentitySha256"],
        },
        "inputs": {
            "policySha256": digest_bytes(policy_path.read_bytes()),
            "workloadSha256": digest_bytes(workload_path.read_bytes()),
            "referenceAgentSha256": digest_bytes(agent_path.read_bytes()),
        },
        "agents": [
            {
                **agent,
                "implementationSha256": digest_bytes(agent_path.read_bytes()),
                "maxRepairTurns": policy["maxRepairTurns"],
                "tokenization": policy["tokenization"],
            }
            for agent in sorted(agents, key=lambda item: item["id"])
        ],
        "caseCount": len(cases),
        "resultCount": len(results),
        "mutationFamilies": workload["mutationFamilies"],
        "acceptance": acceptance,
        "acceptanceChecks": checks,
        "summaries": summaries,
        "primaryVsBaseline": {
            "primaryAgentId": primary_id,
            "baselineAgentId": baseline_id,
            "additionalExactRecoveries": primary["exactRecoveryCount"]
            - baseline["exactRecoveryCount"],
            "recoveryRateDeltaBasisPoints": primary["recoveryRateBasisPoints"]
            - baseline["recoveryRateBasisPoints"],
            "tokenCostDelta": primary["tokenCost"]["total"] - baseline["tokenCost"]["total"],
        },
        "results": results,
    }
    output.parent.mkdir(parents=True, exist_ok=True)
    output.write_text(json.dumps(report, indent=2, sort_keys=True) + "\n", encoding="utf-8")
    if not report["ok"]:
        raise RunnerError(f"repair utility acceptance failed: {checks}")


def main(argv: Sequence[str]) -> int:
    parser = argparse.ArgumentParser()
    parser.add_argument("--genesis", type=Path, required=True)
    parser.add_argument("--selfhost-artifact", type=Path, required=True)
    parser.add_argument("--output", type=Path, required=True)
    parser.add_argument(
        "--policy", type=Path, default=ROOT / "policies/gc_repair_utility_v0.1.json"
    )
    args = parser.parse_args(argv)
    try:
        render(
            args.genesis.resolve(),
            args.selfhost_artifact.resolve(),
            args.output.resolve(),
            args.policy.resolve(),
        )
    except (RunnerError, KeyError, OSError, subprocess.SubprocessError) as exc:
        print(f"gc-repair-utility-runner: {exc}", file=sys.stderr)
        return 1
    return 0


if __name__ == "__main__":
    raise SystemExit(main(sys.argv[1:]))
