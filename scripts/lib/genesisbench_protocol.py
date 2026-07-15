#!/usr/bin/env python3
"""Validate GenesisBench profiles, frozen snapshots, and run eligibility."""

from __future__ import annotations

import argparse
import copy
import hashlib
import json
import re
import subprocess
import sys
from pathlib import Path, PurePosixPath
from typing import Any

from gc_agent_benchmark_run import validate_document as validate_run_document
from genesisbench_eligibility import validate_report as validate_eligibility_report
from genesisbench_contamination import classify_attestation
from genesisbench_contamination import self_test as contamination_self_test
from genesisbench_protocol_contract import (
    ALLOWED_TOOLS,
    ANALYSIS_POLICY,
    ATTEMPT_POLICY,
    AUTHORITY_PATHS,
    CAPABILITY_POLICY,
    COMPONENT_SELECTIONS,
    CONTAMINATION_POLICY,
    CONTEXT_BASE,
    ELIGIBILITY_POLICY,
    MODEL_DISCLOSURE_POLICY,
    SCORING_POLICY,
    SELF_HOSTING,
    TOOL_POLICY,
    TOP_KEYS,
)
from genesisbench_protocol_run import self_test as run_binding_self_test
from genesisbench_protocol_run import validate_run_modes
from genesisbench_tracks import (
    TRACK_POLICY, build_cohort, classify_track, cohort_id,
    self_test as track_self_test,
)


ROOT = Path(__file__).resolve().parents[2]
PROFILE = ROOT / "docs/spec/GENESISBENCH_PROTOCOL_v0.1.json"
PROFILE_SCHEMA = ROOT / "docs/spec/GENESISBENCH_PROTOCOL_v0.1.schema.json"
ELIGIBILITY_SCHEMA = ROOT / "docs/spec/GENESISBENCH_ELIGIBILITY_v0.1.schema.json"
ATTESTATION_SCHEMA = ROOT / "docs/spec/GENESISBENCH_CONTAMINATION_ATTESTATION_v0.1.schema.json"
ADAPTATION_SCHEMA = ROOT / "docs/spec/GENESISBENCH_ADAPTATION_MANIFEST_v0.1.schema.json"
HARDWARE_EVIDENCE_SCHEMA = ROOT / "docs/spec/GENESISBENCH_HARDWARE_EVIDENCE_v0.1.schema.json"
SCAFFOLD_SCHEMA = ROOT / "docs/spec/GENESISBENCH_SCAFFOLD_MANIFEST_v0.1.schema.json"
ANALYSIS_PLAN_SCHEMA = ROOT / "docs/spec/GENESISBENCH_ANALYSIS_PLAN_v0.1.schema.json"
OBSERVATIONS_SCHEMA = ROOT / "docs/spec/GENESISBENCH_OBSERVATIONS_v0.1.schema.json"
ANALYSIS_REPORT_SCHEMA = ROOT / "docs/spec/GENESISBENCH_ANALYSIS_REPORT_v0.1.schema.json"
RUN_EXAMPLE = ROOT / "examples/agent_benchmark_reproducibility/run.json"
ATTESTATION_FIXTURE = ROOT / "benchmarks/genesisbench/v0.1/contamination.fixture.json"
TASK_BENCHMARK = ROOT / "benchmarks/agent_tasks/v0.1/suite.json"
HELD_OUT = ROOT / "docs/spec/GC_AGENT_HELD_OUT_EVALUATION_v0.1.json"
MCP_CATALOG_SOURCE = ROOT / "crates/gc_cli_driver/src/mcp/catalog.rs"
SNAPSHOT_COMMIT = "ef1d56731f46cb1bbb8d6416006ab60512ed9f23"
SHA256_RE = re.compile(r"^[0-9a-f]{64}$")
SHA1_RE = re.compile(r"^[0-9a-f]{40}$")
HOST_PATH_RE = re.compile(r"(?:/Users/|/home/|[A-Za-z]:\\\\Users\\\\)")



class ProtocolError(ValueError):
    pass


def require(condition: bool, message: str) -> None:
    if not condition:
        raise ProtocolError(message)


def duplicate_safe_object(rows: list[tuple[str, Any]]) -> dict[str, Any]:
    result: dict[str, Any] = {}
    for key, value in rows:
        require(key not in result, f"duplicate JSON key: {key}")
        result[key] = value
    return result


def load_json(path: Path) -> Any:
    try:
        return json.loads(
            path.read_text(encoding="utf-8"), object_pairs_hook=duplicate_safe_object,
        )
    except (OSError, UnicodeDecodeError, json.JSONDecodeError) as exc:
        raise ProtocolError(f"cannot load JSON {path.name}: {exc}") from exc


def canonical_bytes(value: Any) -> bytes:
    return (
        json.dumps(value, sort_keys=True, separators=(",", ":"), ensure_ascii=True)
        + "\n"
    ).encode("ascii")


def sha256_bytes(value: bytes) -> str:
    return hashlib.sha256(value).hexdigest()


def file_sha256(path: Path) -> str:
    return sha256_bytes(path.read_bytes())


def content_identity(document: dict[str, Any]) -> str:
    unsigned = copy.deepcopy(document)
    unsigned["contentIdentitySha256"] = ""
    return sha256_bytes(canonical_bytes(unsigned))


def closed(value: Any, keys: set[str], label: str) -> dict[str, Any]:
    require(isinstance(value, dict) and set(value) == keys, f"{label} fields are not closed")
    return value


def sorted_unique(values: list[str], label: str) -> None:
    require(values == sorted(set(values)), f"{label} must be sorted and unique")


def safe_relative(value: str, label: str) -> PurePosixPath:
    require(isinstance(value, str) and value and len(value) <= 512, f"invalid {label}")
    require("\\" not in value and not value.startswith("/") and "//" not in value, f"unsafe {label}")
    path = PurePosixPath(value)
    require(all(part not in {"", ".", ".."} for part in path.parts), f"unsafe {label}")
    require(HOST_PATH_RE.search(value) is None, f"host path in {label}")
    return path


def repository_file(relative: str) -> Path:
    safe_relative(relative, "authority path")
    path = ROOT / relative
    require(path.is_file() and not path.is_symlink(), f"missing regular authority: {relative}")
    require(path.resolve().is_relative_to(ROOT.resolve()), f"escaped authority: {relative}")
    return path


def git(*args: str, input_bytes: bytes | None = None) -> bytes:
    try:
        result = subprocess.run(
            ["git", *args], cwd=ROOT, input=input_bytes, stdout=subprocess.PIPE,
            stderr=subprocess.PIPE, check=False,
        )
    except OSError as exc:
        raise ProtocolError(f"cannot execute git: {exc}") from exc
    require(result.returncode == 0, f"git {' '.join(args)} failed")
    return result.stdout


def tree_rows(commit: str) -> list[dict[str, Any]]:
    require(SHA1_RE.fullmatch(commit) is not None, "invalid snapshot commit")
    raw = git("ls-tree", "-r", "-l", "-z", "--full-tree", commit)
    entries: list[tuple[str, str, str, int | None, str]] = []
    for item in raw.split(b"\0"):
        if not item:
            continue
        try:
            header, path_bytes = item.split(b"\t", 1)
            mode, object_type, oid, size_token = header.decode("ascii").split(" ", 3)
            path = path_bytes.decode("utf-8")
        except (ValueError, UnicodeDecodeError) as exc:
            raise ProtocolError("cannot parse frozen Git tree") from exc
        safe_relative(path, "snapshot path")
        require(re.fullmatch(r"[0-7]{6}", mode) is not None, "invalid snapshot mode")
        require(object_type in {"blob", "commit"} and SHA1_RE.fullmatch(oid) is not None, "invalid snapshot object")
        declared_size = None if size_token == "-" else int(size_token)
        entries.append((mode, object_type, oid, declared_size, path))
    require(entries and [row[4] for row in entries] == sorted(row[4] for row in entries), "snapshot tree order drift")

    unique_oids = sorted({row[2] for row in entries})
    batch = git("cat-file", "--batch", input_bytes=("\n".join(unique_oids) + "\n").encode("ascii"))
    offset = 0
    payloads: dict[str, bytes] = {}
    for expected_oid in unique_oids:
        newline = batch.find(b"\n", offset)
        require(newline >= 0, "truncated Git object header")
        header = batch[offset:newline].decode("ascii").split()
        require(len(header) == 3 and header[0] == expected_oid, "Git object response drift")
        size = int(header[2])
        start = newline + 1
        end = start + size
        require(end < len(batch) and batch[end:end + 1] == b"\n", "truncated Git object payload")
        payloads[expected_oid] = batch[start:end]
        offset = end + 1
    require(offset == len(batch), "unexpected Git object response suffix")

    rows = []
    for mode, object_type, oid, declared_size, path in entries:
        payload = payloads[oid]
        if declared_size is not None:
            require(declared_size == len(payload), f"snapshot byte drift: {path}")
        rows.append({
            "path": path,
            "mode": mode,
            "type": object_type,
            "bytes": len(payload),
            "sha256": sha256_bytes(payload),
        })
    return rows


def selected_rows(rows: list[dict[str, Any]], selection: dict[str, list[str]]) -> list[dict[str, Any]]:
    exact = set(selection["includeExact"])
    prefixes = tuple(selection["includePrefixes"])
    excluded = tuple(selection["excludePrefixes"])
    selected = [
        row for row in rows
        if (row["path"] in exact or row["path"].startswith(prefixes))
        and not row["path"].startswith(excluded)
    ]
    require(selected, "snapshot component is empty")
    return selected


def source_snapshot() -> dict[str, Any]:
    commit = SNAPSHOT_COMMIT
    tree = git("rev-parse", f"{commit}^{{tree}}").decode("ascii").strip()
    committed_at = git("show", "-s", "--format=%cI", commit).decode("ascii").strip()
    rows = tree_rows(commit)
    components = []
    for component_id in sorted(COMPONENT_SELECTIONS):
        selection = COMPONENT_SELECTIONS[component_id]
        subset = selected_rows(rows, selection)
        components.append({
            "id": component_id,
            "includeExact": selection["includeExact"],
            "includePrefixes": selection["includePrefixes"],
            "excludePrefixes": selection["excludePrefixes"],
            "artifactCount": len(subset),
            "bytes": sum(row["bytes"] for row in subset),
            "identitySha256": sha256_bytes(canonical_bytes(subset)),
        })
    return {
        "repositoryUrl": "https://github.com/corbensorenson/genesisCode.git",
        "commitSha1": commit,
        "treeSha1": tree,
        "committedAt": committed_at,
        "objectFormat": "git-sha1-plus-complete-content-sha256-manifest",
        "completeTree": True,
        "artifactCount": len(rows),
        "bytes": sum(row["bytes"] for row in rows),
        "manifestAlgorithm": "sha256-canonical-json-path-mode-type-bytes-content-sha256-v0.1",
        "manifestIdentitySha256": sha256_bytes(canonical_bytes(rows)),
        "components": components,
    }


def authority_rows() -> list[dict[str, Any]]:
    rows = []
    for authority_id, relative in sorted(AUTHORITY_PATHS.items()):
        path = repository_file(relative)
        rows.append({
            "id": authority_id,
            "path": relative,
            "sha256": file_sha256(path),
            "bytes": path.stat().st_size,
        })
    return rows


def context_modes() -> list[dict[str, Any]]:
    suite = load_json(TASK_BENCHMARK)
    modes = []
    for tier in suite["contextTiers"]:
        tier_id = tier["id"]
        modes.append({
            "id": f"compact-{tier_id}",
            "contextTier": tier_id,
            "sourceComponent": "documentation",
            "exposure": "closed-artifact-pack",
            "retrieval": "none",
            "rankingCohort": f"context-compact-{tier_id}",
            "artifacts": [row["path"] for row in tier["artifacts"]],
            "contextBytes": tier["contextBytes"],
        })
    modes.append({
        "id": "repository-readonly",
        "contextTier": None,
        "sourceComponent": "repository",
        "exposure": "read-only-content-addressed-retrieval",
        "retrieval": "logged-exact-artifact",
        "rankingCohort": "context-repository-readonly",
        "artifacts": [],
        "contextBytes": None,
    })
    return sorted(modes, key=lambda row: row["id"])


def visibility_policy() -> dict[str, Any]:
    held_out = load_json(HELD_OUT)
    active = [row for row in held_out["epochs"] if row["status"] == "active"]
    require(len(active) == 1, "held-out authority must have one active epoch")
    return {
        "classes": [
            {
                "id": "held-out-commitment", "split": "held-out",
                "oracleExposure": "commitment-only", "rankedAllowed": True,
                "allowedContaminationLabels": [
                    "declared-contaminated", "declared-uncontaminated", "unknown",
                ],
            },
            {
                "id": "public-anchor-hidden-oracle", "split": "public-test",
                "oracleExposure": "hidden", "rankedAllowed": True,
                "allowedContaminationLabels": [
                    "declared-contaminated", "declared-uncontaminated", "unknown",
                ],
            },
            {
                "id": "public-development-reference", "split": "public-test",
                "oracleExposure": "public-development-reference", "rankedAllowed": False,
                "allowedContaminationLabels": ["declared-contaminated"],
            },
            {
                "id": "temporal-held-out", "split": "held-out",
                "oracleExposure": "commitment-only", "rankedAllowed": True,
                "allowedContaminationLabels": ["temporal-clean"],
            },
        ],
        "publicBenchmarkAuthorityId": "agent-task-benchmark",
        "heldOutAuthorityId": "held-out-evaluation",
        "activeHeldOutEpochId": active[0]["id"],
        "mixedEpochAggregationAllowed": False,
        "oracleAccessByModelAllowed": False,
        "visibilityRecordedBeforeInvocation": True,
    }


def validate_schema(path: Path, schema_id: str) -> None:
    schema = load_json(path)
    require(schema.get("$schema") == "https://json-schema.org/draft/2020-12/schema", f"{path.name}: schema draft drift")
    require(schema.get("$id") == schema_id, f"{path.name}: schema id drift")

    def walk(value: Any, label: str) -> None:
        if isinstance(value, dict):
            if value.get("type") == "object":
                require(value.get("additionalProperties") is False, f"schema object is open: {label}")
                require(set(value.get("required", [])) == set(value.get("properties", {})), f"schema object is not recursively required: {label}")
            for key, child in value.items():
                walk(child, f"{label}/{key}")
        elif isinstance(value, list):
            for index, child in enumerate(value):
                walk(child, f"{label}/{index}")

    walk(schema, path.name)


def expected_context_policy() -> dict[str, Any]:
    result = copy.deepcopy(CONTEXT_BASE)
    result["modes"] = context_modes()
    return result


def validate_profile(document: Any, *, check_identity: bool = True) -> dict[str, Any]:
    doc = closed(document, TOP_KEYS, "GenesisBench profile")
    require(doc["kind"] == "genesis/genesisbench-protocol-v0.1", "profile kind drift")
    require(doc["version"] == "0.1.0" and doc["protocolId"] == "GenesisBench-v0.1", "profile identity drift")
    require(doc["status"] == "active", "profile is not active")
    require(doc["sourceSnapshot"] == source_snapshot(), "frozen source snapshot drift")
    require(doc["authorities"] == authority_rows(), "profile authority closure drift")
    require(doc["contextPolicy"] == expected_context_policy(), "context policy drift")
    require(doc["toolPolicy"] == TOOL_POLICY, "tool policy drift")
    require(doc["capabilityPolicy"] == CAPABILITY_POLICY, "capability policy drift")
    require(doc["attemptPolicy"] == ATTEMPT_POLICY, "attempt policy drift")
    require(doc["modelDisclosurePolicy"] == MODEL_DISCLOSURE_POLICY, "model disclosure policy drift")
    require(doc["taskVisibilityPolicy"] == visibility_policy(), "task visibility policy drift")
    require(doc["scoringPolicy"] == SCORING_POLICY, "scoring policy drift")
    require(doc["analysisPolicy"] == ANALYSIS_POLICY, "analysis policy drift")
    require(doc["contaminationPolicy"] == CONTAMINATION_POLICY, "contamination policy drift")
    require(doc["trackPolicy"] == TRACK_POLICY, "track policy drift")
    require(doc["eligibilityPolicy"] == ELIGIBILITY_POLICY, "eligibility policy drift")
    require(doc["selfHosting"] == SELF_HOSTING, "self-hosting contract drift")
    source = MCP_CATALOG_SOURCE.read_text(encoding="utf-8")
    observed_tools = sorted(set(re.findall(r'route\(\s*"([a-z-]+)"', source)))
    require(observed_tools == ALLOWED_TOOLS, "MCP tool surface drift")
    serialized = canonical_bytes(doc).decode("ascii")
    require(HOST_PATH_RE.search(serialized) is None, "profile leaks a host path")
    if check_identity:
        require(SHA256_RE.fullmatch(doc["contentIdentitySha256"]) is not None, "invalid profile identity")
        require(doc["contentIdentitySha256"] == content_identity(doc), "profile content identity drift")
    return doc


def visibility_for_run(run: dict[str, Any], case: dict[str, Any]) -> str:
    benchmark = run["benchmark"]
    if benchmark["split"] == "public-test" and case["oracleExposure"] == "public-development-reference":
        return "public-development-reference"
    if benchmark["split"] == "held-out" and benchmark["heldOutEpochId"] is not None:
        return "held-out-commitment"
    raise ProtocolError("run task visibility is not represented by the active profile")



def evaluate_run(
    profile: dict[str, Any], run_path: Path, *, attestation_path: Path | None = None,
    independent_rescore_observed: bool = False,
) -> dict[str, Any]:
    run = validate_run_document(load_json(run_path), run_path.resolve())
    suite = load_json(TASK_BENCHMARK)
    case = next((row for row in suite["cases"] if row["id"] == run["benchmark"]["caseId"]), None)
    require(case is not None, "run case is absent from benchmark authority")
    context_mode, interaction_mode = validate_run_modes(profile, run, case, suite)
    visibility = visibility_for_run(run, case)
    attestation_claim = None
    if attestation_path is not None:
        attestation = load_json(attestation_path)
        supported_label, evidence_codes = classify_attestation(
            attestation, run, visibility, load_json(HELD_OUT),
        )
        attestation_claim = attestation["claim"]
    elif visibility == "public-development-reference":
        supported_label = "declared-contaminated"
        evidence_codes = ["known-exposure/public-development-reference"]
    else:
        supported_label = "unknown"
        evidence_codes = ["insufficient-evidence/no-external-attestation"]
    claimed_label = run["benchmark"]["contamination"]

    reasons: list[str] = []
    if visibility == "public-development-reference":
        reasons.extend(["task/public-reference", "visibility/practice-only"])
    if not independent_rescore_observed:
        reasons.append("score/not-independently-rescored")
    if run["model"]["providerId"] == "genesis.fixture.local":
        reasons.append("model/conformance-fixture")
    if len(run["invocation"]["attempts"]) > profile["attemptPolicy"]["rankedMaxAttempts"]:
        reasons.append("attempt/multiple")
    if supported_label == "unknown":
        reasons.append("evidence/incomplete")
    track_reasons, track_invalid_reasons = classify_track(run["track"], run)
    reasons.extend(track_reasons)
    invalid_reasons: list[str] = []
    if claimed_label != supported_label or (
        attestation_claim is not None and attestation_claim != supported_label
    ):
        invalid_reasons.append("contamination/overclaim")
    invalid_reasons.extend(track_invalid_reasons)
    reasons = sorted(set(reasons))
    invalid_reasons = sorted(set(invalid_reasons))
    decision = "invalid" if invalid_reasons else ("ranked" if not reasons else "unranked")
    cohort = build_cohort(
        profile, run, context_mode=context_mode,
        interaction_mode=interaction_mode, visibility=visibility,
        contamination_label=supported_label,
    )
    report = {
        "kind": "genesis/genesisbench-eligibility-v0.1",
        "version": "0.1.0",
        "protocol": {
            "id": profile["protocolId"],
            "version": profile["version"],
            "identitySha256": profile["contentIdentitySha256"],
        },
        "snapshot": {
            "commitSha1": profile["sourceSnapshot"]["commitSha1"],
            "treeSha1": profile["sourceSnapshot"]["treeSha1"],
            "manifestIdentitySha256": profile["sourceSnapshot"]["manifestIdentitySha256"],
        },
        "run": {
            "id": run["runId"],
            "identitySha256": run["contentIdentitySha256"],
            "modelId": run["model"]["modelId"],
            "modelRevision": run["model"]["modelRevision"],
            "attempts": len(run["invocation"]["attempts"]),
            "qualityScoreBasisPoints": run["score"]["qualityScoreBasisPoints"],
        },
        "case": {
            "id": case["id"],
            "lineageId": case["lineageId"],
            "lineageIdentitySha256": case["lineageIdentitySha256"],
            "conditionId": case["conditionId"],
            "conditionIdentitySha256": case["conditionIdentitySha256"],
            "taskClass": case["taskClass"],
            "contextTier": case["contextTier"],
            "contextMode": context_mode,
            "interactionMode": interaction_mode,
            "split": run["benchmark"]["split"],
            "visibilityClass": visibility,
            "heldOutEpochId": run["benchmark"]["heldOutEpochId"],
        },
        "validation": {
            "profileValid": True,
            "snapshotValid": True,
            "runRecordValid": True,
            "scoreRecordValid": True,
            "independentRescoreRequired": True,
            "independentRescoreObserved": independent_rescore_observed,
            "judgeModelUsed": False,
        },
        "contamination": {
            "claimedLabel": claimed_label,
            "strongestSupportedLabel": supported_label,
            "evidenceCodes": evidence_codes,
        },
        "cohort": cohort,
        "eligibility": {
            "decision": decision,
            "rankingCohort": cohort_id(cohort),
            "reasonCodes": invalid_reasons if invalid_reasons else reasons,
        },
        "contentIdentitySha256": "",
    }
    report["contentIdentitySha256"] = content_identity(report)
    validate_report(report, profile, run)
    return report


def validate_report(report: Any, profile: dict[str, Any], run: dict[str, Any]) -> dict[str, Any]:
    suite = load_json(TASK_BENCHMARK)
    source_case = next((row for row in suite["cases"] if row["id"] == run["benchmark"]["caseId"]), None)
    require(source_case is not None, "eligibility case authority drift")
    context_mode, interaction_mode = validate_run_modes(profile, run, source_case, suite)
    expected_case = {
        "id": source_case["id"],
        "lineageId": source_case["lineageId"],
        "lineageIdentitySha256": source_case["lineageIdentitySha256"],
        "conditionId": source_case["conditionId"],
        "conditionIdentitySha256": source_case["conditionIdentitySha256"],
        "taskClass": source_case["taskClass"],
        "contextTier": source_case["contextTier"], "contextMode": context_mode,
        "interactionMode": interaction_mode, "split": run["benchmark"]["split"],
        "visibilityClass": visibility_for_run(run, source_case),
        "heldOutEpochId": run["benchmark"]["heldOutEpochId"],
    }
    return validate_eligibility_report(report, profile, run, expected_case)


def resign(document: dict[str, Any]) -> None:
    document["contentIdentitySha256"] = content_identity(document)


def self_test(profile: dict[str, Any]) -> int:
    mutations: list[tuple[str, Any]] = []

    def add(name: str, mutate: Any, *, identity_drift: bool = False) -> None:
        candidate = copy.deepcopy(profile)
        mutate(candidate)
        if not identity_drift:
            resign(candidate)
        mutations.append((name, candidate))

    add("unknown-field", lambda d: d.__setitem__("judge", "model"))
    add("snapshot-commit", lambda d: d["sourceSnapshot"].__setitem__("commitSha1", "0" * 40))
    add("snapshot-tree", lambda d: d["sourceSnapshot"].__setitem__("treeSha1", "0" * 40))
    add("snapshot-manifest", lambda d: d["sourceSnapshot"].__setitem__("manifestIdentitySha256", "0" * 64))
    add("snapshot-component", lambda d: d["sourceSnapshot"]["components"][0].__setitem__("artifactCount", 1))
    add("authority-hash", lambda d: d["authorities"][0].__setitem__("sha256", "0" * 64))
    add("authority-removal", lambda d: d["authorities"].pop())
    add("context-order", lambda d: d["contextPolicy"]["authorityOrder"].reverse())
    add("oracle-path", lambda d: d["contextPolicy"]["forbiddenPaths"].remove("benchmarks/agent_tasks/v0.1/references/"))
    add("prompt-authority", lambda d: d["contextPolicy"].__setitem__("promptMaySelectAuthority", True))
    add("tool-broadening", lambda d: d["toolPolicy"]["allowedTools"].append("shell"))
    add("shell", lambda d: d["toolPolicy"].__setitem__("arbitraryShellAllowed", True))
    add("ambient-network", lambda d: d["toolPolicy"].__setitem__("ambientNetworkAllowed", True))
    add("capability-default", lambda d: d["capabilityPolicy"].__setitem__("defaultDecision", "allow"))
    add("capability-wildcard", lambda d: d["capabilityPolicy"].__setitem__("wildcardsAllowed", True))
    add("ranked-retries", lambda d: d["attemptPolicy"].__setitem__("rankedMaxAttempts", 8))
    add("best-of-n", lambda d: d["attemptPolicy"].__setitem__("bestOfNRankedAllowed", True))
    add("mutable-model", lambda d: d["modelDisclosurePolicy"].__setitem__("immutableRevisionRequired", False))
    add("unknown-clean", lambda d: d["modelDisclosurePolicy"].__setitem__("unknownProvenanceDefault", "temporal-clean"))
    add("oracle-access", lambda d: d["taskVisibilityPolicy"].__setitem__("oracleAccessByModelAllowed", True))
    add("public-ranked", lambda d: d["taskVisibilityPolicy"]["classes"][2].__setitem__("rankedAllowed", True))
    add("judge-score", lambda d: d["scoringPolicy"].__setitem__("judgeModelPreferenceIncluded", True))
    add("metric-score", lambda d: d["scoringPolicy"].__setitem__("modelMetricsIncluded", True))
    add("contamination-default", lambda d: d["contaminationPolicy"].__setitem__("defaultLabel", "temporal-clean"))
    add("newness-clean", lambda d: d["contaminationPolicy"].__setitem__("newLanguageImpliesClean", True))
    add("cross-track-ranking", lambda d: d["trackPolicy"].__setitem__("crossTrackRankingAllowed", True))
    add("track-removal", lambda d: d["trackPolicy"]["tracks"].pop())
    add("hardware-bound", lambda d: d["trackPolicy"]["hardwareClasses"][0].__setitem__("maxCombinedResidentBytes", 5 * 1024**3))
    add("temporal-order", lambda d: d["contaminationPolicy"]["temporalCleanEvidence"].__setitem__("taskPrecommitAfterModelReleaseRequired", False))
    add("missing-evidence-ranked", lambda d: d["eligibilityPolicy"].__setitem__("missingEvidenceDecision", "ranked"))
    add("silent-suppression", lambda d: d["eligibilityPolicy"].__setitem__("silentSuppressionAllowed", True))
    add("network-required", lambda d: d["selfHosting"].__setitem__("networkRequired", True))
    add("update-in-check", lambda d: d["selfHosting"].__setitem__("updateDuringCheckAllowed", True))
    add("identity-drift", lambda d: d.__setitem__("contentIdentitySha256", "0" * 64), identity_drift=True)

    rejected = 0
    for name, candidate in mutations:
        try:
            validate_profile(candidate)
        except ProtocolError:
            rejected += 1
        else:
            raise ProtocolError(f"negative control accepted: {name}")

    run = validate_run_document(load_json(RUN_EXAMPLE), RUN_EXAMPLE)
    held_out = load_json(HELD_OUT)
    suite = load_json(TASK_BENCHMARK)
    case = next(row for row in suite["cases"] if row["id"] == run["benchmark"]["caseId"])
    rejected += run_binding_self_test(profile, run, case, suite)
    rejected += track_self_test(run)
    rejected += contamination_self_test(
        load_json(ATTESTATION_FIXTURE), run, held_out,
    )
    report = evaluate_run(
        profile, RUN_EXAMPLE, attestation_path=ATTESTATION_FIXTURE,
    )
    report_mutations: list[tuple[str, Any]] = []

    def add_report(name: str, mutate: Any) -> None:
        candidate = copy.deepcopy(report)
        mutate(candidate)
        resign(candidate)
        report_mutations.append((name, candidate))

    add_report("report-judge", lambda d: d["validation"].__setitem__("judgeModelUsed", True))
    add_report("report-run-rebind", lambda d: d["run"].__setitem__("identitySha256", "0" * 64))
    add_report("report-contamination", lambda d: d["contamination"].__setitem__("strongestSupportedLabel", "temporal-clean"))
    add_report("report-ranked", lambda d: (d["eligibility"].__setitem__("decision", "ranked"), d["eligibility"].__setitem__("reasonCodes", [])))
    add_report("report-reason-order", lambda d: d["eligibility"]["reasonCodes"].reverse())
    add_report("report-reason-class", lambda d: d["eligibility"]["reasonCodes"].append("run/invalid"))
    add_report("report-track", lambda d: d["cohort"].__setitem__("trackId", "cold-acquisition"))
    add_report("report-scaffold", lambda d: d["cohort"].__setitem__("scaffoldIdentitySha256", "0" * 64))
    add_report("report-cohort-id", lambda d: d["eligibility"].__setitem__("rankingCohort", "genesisbench-cohort-v0.1/" + "0" * 64))
    for name, candidate in report_mutations:
        try:
            validate_report(candidate, profile, run)
        except (ProtocolError, ValueError):
            rejected += 1
        else:
            raise ProtocolError(f"negative report control accepted: {name}")

    overclaim_run = copy.deepcopy(run)
    overclaim_run["benchmark"]["contamination"] = "temporal-clean"
    resign(overclaim_run)
    invalid_report = copy.deepcopy(report)
    invalid_report["run"]["identitySha256"] = overclaim_run["contentIdentitySha256"]
    invalid_report["contamination"]["claimedLabel"] = "temporal-clean"
    invalid_report["eligibility"]["decision"] = "invalid"
    invalid_report["eligibility"]["reasonCodes"] = ["contamination/overclaim"]
    resign(invalid_report)
    validate_report(invalid_report, profile, overclaim_run)
    return rejected


def refresh_profile() -> None:
    document = load_json(PROFILE)
    document["sourceSnapshot"] = source_snapshot()
    document["authorities"] = authority_rows()
    document["contextPolicy"] = expected_context_policy()
    document["toolPolicy"] = copy.deepcopy(TOOL_POLICY)
    document["capabilityPolicy"] = copy.deepcopy(CAPABILITY_POLICY)
    document["attemptPolicy"] = copy.deepcopy(ATTEMPT_POLICY)
    document["modelDisclosurePolicy"] = copy.deepcopy(MODEL_DISCLOSURE_POLICY)
    document["taskVisibilityPolicy"] = visibility_policy()
    document["scoringPolicy"] = copy.deepcopy(SCORING_POLICY)
    document["analysisPolicy"] = copy.deepcopy(ANALYSIS_POLICY)
    document["contaminationPolicy"] = copy.deepcopy(CONTAMINATION_POLICY)
    document["trackPolicy"] = copy.deepcopy(TRACK_POLICY)
    document["eligibilityPolicy"] = copy.deepcopy(ELIGIBILITY_POLICY)
    document["selfHosting"] = copy.deepcopy(SELF_HOSTING)
    resign(document)
    PROFILE.write_text(json.dumps(document, indent=2, sort_keys=True) + "\n", encoding="utf-8")


def main() -> int:
    parser = argparse.ArgumentParser()
    mode = parser.add_mutually_exclusive_group(required=True)
    mode.add_argument("--check", action="store_true")
    mode.add_argument("--refresh-profile", action="store_true")
    parser.add_argument("--run", type=Path)
    parser.add_argument("--attestation", type=Path)
    parser.add_argument("--json", action="store_true")
    parser.add_argument("--self-test", action="store_true")
    args = parser.parse_args()
    validate_schema(PROFILE_SCHEMA, "https://genesiscode.dev/schemas/genesisbench-protocol-v0.1.json")
    validate_schema(ELIGIBILITY_SCHEMA, "https://genesiscode.dev/schemas/genesisbench-eligibility-v0.1.json")
    validate_schema(ATTESTATION_SCHEMA, "https://genesiscode.dev/schemas/genesisbench-contamination-attestation-v0.1.json")
    validate_schema(ADAPTATION_SCHEMA, "https://genesiscode.dev/schemas/genesisbench-adaptation-manifest-v0.1.json")
    validate_schema(HARDWARE_EVIDENCE_SCHEMA, "https://genesiscode.dev/schemas/genesisbench-hardware-evidence-v0.1.json")
    validate_schema(SCAFFOLD_SCHEMA, "https://genesiscode.dev/schemas/genesisbench-scaffold-manifest-v0.1.json")
    validate_schema(ANALYSIS_PLAN_SCHEMA, "https://genesiscode.dev/schemas/genesisbench-analysis-plan-v0.1.json")
    validate_schema(OBSERVATIONS_SCHEMA, "https://genesiscode.dev/schemas/genesisbench-observations-v0.1.json")
    validate_schema(ANALYSIS_REPORT_SCHEMA, "https://genesiscode.dev/schemas/genesisbench-analysis-report-v0.1.json")
    if args.refresh_profile:
        require(args.run is None and args.attestation is None and not args.json and not args.self_test, "refresh accepts no check inputs")
        refresh_profile()
        document = validate_profile(load_json(PROFILE))
        print(f"genesisbench-protocol: refreshed identity={document['contentIdentitySha256']}")
        return 0
    document = validate_profile(load_json(PROFILE))
    controls = self_test(document) if args.self_test else 0
    if args.run is not None:
        report = evaluate_run(
            document,
            args.run.resolve(),
            attestation_path=args.attestation.resolve() if args.attestation is not None else None,
        )
        if args.json:
            sys.stdout.buffer.write(canonical_bytes(report))
        else:
            print(
                "genesisbench-protocol: run "
                f"decision={report['eligibility']['decision']} "
                f"contamination={report['contamination']['strongestSupportedLabel']} "
                f"identity={report['contentIdentitySha256']}"
            )
    else:
        require(args.attestation is None, "--attestation requires --run")
        require(not args.json, "--json requires --run")
        print(
            "genesisbench-protocol: ok "
            f"(snapshot_files={document['sourceSnapshot']['artifactCount']} "
            f"authorities={len(document['authorities'])} controls={controls} "
            f"identity={document['contentIdentitySha256']})"
        )
    return 0


if __name__ == "__main__":
    try:
        raise SystemExit(main())
    except (ProtocolError, ValueError, OSError) as exc:
        print(f"genesisbench-protocol: {exc}", file=sys.stderr)
        raise SystemExit(1)
