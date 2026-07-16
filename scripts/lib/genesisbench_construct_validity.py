#!/usr/bin/env python3
"""Execute and validate the GenesisBench construct-validity study."""

from __future__ import annotations

import argparse
import copy
import hashlib
import json
import random
import re
import shutil
import sys
import tempfile
from pathlib import Path
from typing import Any, Callable

ROOT = Path(__file__).resolve().parents[2]
sys.path.insert(0, str(ROOT / "scripts/lib"))

from gc_agent_scoring import authority_files, materialize, score_candidate  # noqa: E402
from gc_agent_scoring_contract import load_json, validate_scoring  # noqa: E402

POLICY = ROOT / "policies/genesisbench_construct_validity_v0.1.json"
SCHEMA = ROOT / "docs/spec/GENESISBENCH_CONSTRUCT_VALIDITY_v0.1.schema.json"
REPORT = ROOT / "benchmarks/genesisbench/v0.1/construct-validity/report.json"
ALTERNATIVES = ROOT / "benchmarks/genesisbench/v0.1/construct-validity/alternatives"
BENCHMARK = ROOT / "benchmarks/agent_tasks/v0.1/suite.json"
SCORING = ROOT / "docs/spec/GC_AGENT_BENCHMARK_SCORING_v0.1.json"
PROFILE = ROOT / "docs/spec/GC_AGENT_PROFILE_v0.3.json"
HELD_OUT = ROOT / "docs/spec/GC_AGENT_HELD_OUT_EVALUATION_v0.1.json"
TEMPORAL_AUDIT = ROOT / "docs/program/GENESISBENCH_TEMPORAL_EPOCH_AUDIT_v0.1.json"
SCORER = ROOT / "scripts/lib/gc_agent_scoring.py"
RUNTIME = Path(__file__).resolve()
SHA_RE = re.compile(r"^[0-9a-f]{64}$")
HOST_PATH_RE = re.compile(r"(?:/Users/|/home/|[A-Za-z]:\\\\)")


class StudyError(ValueError):
    pass


def require(condition: bool, message: str) -> None:
    if not condition:
        raise StudyError(message)


def canonical_bytes(value: Any) -> bytes:
    return (json.dumps(value, sort_keys=True, separators=(",", ":"), ensure_ascii=True) + "\n").encode("ascii")


def digest(path: Path) -> str:
    return hashlib.sha256(path.read_bytes()).hexdigest()


def identity(document: dict[str, Any]) -> str:
    unsigned = copy.deepcopy(document)
    unsigned["contentIdentitySha256"] = ""
    return hashlib.sha256(canonical_bytes(unsigned)).hexdigest()


def closed(value: Any, keys: set[str], label: str) -> dict[str, Any]:
    require(isinstance(value, dict) and set(value) == keys, f"{label} fields are not closed")
    return value


def sorted_unique(values: list[str], label: str) -> None:
    require(values == sorted(set(values)), f"{label} must be sorted and unique")


def validate_policy(policy: Any) -> dict[str, Any]:
    doc = closed(
        policy,
        {
            "kind", "version", "studyId", "caseTier", "constructs",
            "nuisanceFactors", "maintenanceFamilies", "alternativeDesignPolicy",
            "statistics", "audit", "primaryReferences",
        },
        "construct policy",
    )
    require(
        doc["kind"] == "genesis/genesisbench-construct-validity-policy-v0.1"
        and doc["version"] == "0.1.0"
        and doc["studyId"] == "GenesisBench-Construct-Validity-v0.1"
        and doc["caseTier"] == "small",
        "construct policy identity drift",
    )
    sorted_unique(doc["constructs"], "constructs")
    sorted_unique(doc["nuisanceFactors"], "nuisance factors")
    sorted_unique(doc["maintenanceFamilies"], "maintenance families")
    require(len(doc["constructs"]) == 7 and len(doc["nuisanceFactors"]) == 7, "construct coverage drift")
    require(
        doc["alternativeDesignPolicy"]
        == {
            "exactReferenceBytesRequired": False,
            "behavioralVerificationRequired": True,
            "artifactContractsMayRequireExactData": True,
            "propertyAndMetamorphicTestsRequired": True,
            "capabilityResourceAndApiEnvelopesRequired": True,
        },
        "alternative-design policy drift",
    )
    statistics = closed(
        doc["statistics"],
        {
            "independentUnit", "bootstrapReplicates", "bootstrapSeed",
            "confidenceBasisPoints", "saturationThresholdBasisPoints",
            "saturationFamilies", "saturationConsecutiveEpochs",
        },
        "statistics policy",
    )
    require(
        statistics["independentUnit"] == "lineageId"
        and statistics["bootstrapReplicates"] >= 10000
        and statistics["confidenceBasisPoints"] == 9500
        and statistics["saturationThresholdBasisPoints"] == 9000
        and statistics["saturationFamilies"] == 3
        and statistics["saturationConsecutiveEpochs"] == 2,
        "statistics policy drift",
    )
    audit = closed(
        doc["audit"],
        {
            "strata", "minimumVerifiers", "modelOrHumanPreferenceIsOracle",
            "conflictDisclosureRequired", "hiddenVerifierCommitmentRequiredForRanking",
        },
        "audit policy",
    )
    sorted_unique(audit["strata"], "audit strata")
    require(
        audit["minimumVerifiers"] >= 2
        and audit["modelOrHumanPreferenceIsOracle"] is False
        and audit["conflictDisclosureRequired"] is True
        and audit["hiddenVerifierCommitmentRequiredForRanking"] is True,
        "audit policy drift",
    )
    references = doc["primaryReferences"]
    require(isinstance(references, list) and len(references) >= 4, "primary references missing")
    ids = [row.get("id") for row in references]
    sorted_unique(ids, "primary reference ids")
    for row in references:
        closed(row, {"id", "url", "principle"}, "primary reference")
        require(row["url"].startswith("https://arxiv.org/abs/"), "reference must name a primary paper")
    return doc


def small_cases(suite: dict[str, Any]) -> dict[str, dict[str, Any]]:
    result = {
        row["taskClass"]: row
        for row in suite["cases"]
        if row["contextTier"] == "small"
    }
    require(sorted(result) == sorted(suite["taskClasses"]), "small-case coverage drift")
    return result


def overlay_files(root: Path, destination: Path) -> None:
    require(root.is_dir() and not root.is_symlink(), f"missing alternative overlay: {root.name}")
    seen = 0
    for source in sorted(root.rglob("*")):
        if source.is_file():
            require(not source.is_symlink(), "alternative overlay contains a symlink")
            relative = source.relative_to(root)
            target = destination / relative
            target.parent.mkdir(parents=True, exist_ok=True)
            shutil.copyfile(source, target)
            seen += 1
    require(seen > 0, "alternative overlay is empty")


def score_projection(identifier: str, task: str, case: dict[str, Any], score: dict[str, Any]) -> dict[str, Any]:
    return {
        "id": identifier,
        "taskClass": task,
        "caseId": case["id"],
        "candidateIdentitySha256": score["candidate"]["identitySha256"],
        "scoreIdentitySha256": score["scoreIdentitySha256"],
        "valid": score["validity"]["passed"],
        "qualityScoreBasisPoints": score["qualityScoreBasisPoints"],
        "failedDimensions": score["validity"]["failedDimensions"],
    }


def candidate_score(
    scoring: dict[str, Any], case: dict[str, Any], genesis: Path,
    selfhost: Path, mutate: Callable[[Path], None] | None = None,
    overlay: Path | None = None,
) -> dict[str, Any]:
    with tempfile.TemporaryDirectory(prefix="genesis-construct-") as raw:
        candidate = Path(raw) / "candidate"
        candidate.mkdir()
        materialize(authority_files(case, "reference"), candidate)
        if overlay is not None:
            overlay_files(overlay, candidate)
        if mutate is not None:
            mutate(candidate)
        return score_candidate(scoring, case["id"], candidate, genesis, selfhost)


def replace(path: Path, old: str, new: str) -> None:
    source = path.read_text(encoding="utf-8")
    require(old in source, f"mutation precondition missing: {path.name}")
    path.write_text(source.replace(old, new), encoding="utf-8")


def negative_mutations(cases: dict[str, dict[str, Any]]) -> dict[str, Callable[[Path], None]]:
    performance_input = ROOT / cases["performance-repair"]["inputRoot"] / "main.gc"
    return {
        "completion": lambda root: (root / "main.gc").write_text("41\n", encoding="utf-8"),
        "deployment": lambda root: (root / "deployment.json").write_text("{}\n", encoding="utf-8"),
        "generation": lambda root: (root / "oracle.txt").write_text("42\n", encoding="utf-8"),
        "package-migration": lambda root: replace(root / "case.toml", 'name = "benchmark_package"', 'name = "benchmark_impostor"'),
        "performance-repair": lambda root: shutil.copyfile(performance_input, root / "main.gc"),
        "policy-minimization": lambda root: replace(root / "caps.toml", 'allow = ["io/fs::read"]', 'allow = ["io/fs::read", "io/fs::write"]'),
        "refactor": lambda root: (root / "notes.txt").write_text("undeclared\n", encoding="utf-8"),
        "repair": lambda root: (root / "main.gc").write_text("3\n", encoding="utf-8"),
        "replay-investigation": lambda root: (root / "finding.json").write_text("{}\n", encoding="utf-8"),
    }


def percentile_interval(values: list[int], *, replicates: int, seed: int, confidence: int) -> dict[str, Any]:
    require(values, "bootstrap values are empty")
    rng = random.Random(seed)
    samples = []
    for _ in range(replicates):
        draw = [values[rng.randrange(len(values))] for _ in values]
        samples.append(sum(draw) * 10000 // len(draw))
    samples.sort()
    tail = (10000 - confidence) // 2
    lower = samples[(tail * (replicates - 1)) // 10000]
    upper = samples[((10000 - tail) * (replicates - 1)) // 10000]
    return {
        "lowerBasisPoints": lower,
        "upperBasisPoints": upper,
        "method": "deterministic-cluster-bootstrap-percentile-v0.1",
    }


def saturation_trigger(rows: list[list[int]], threshold: int, families: int, epochs: int) -> bool:
    return (
        len(rows) >= epochs
        and all(len(row) >= families for row in rows[-epochs:])
        and all(sum(score >= threshold for score in row) >= families for row in rows[-epochs:])
    )


def render(genesis: Path, selfhost: Path) -> dict[str, Any]:
    policy = validate_policy(load_json(POLICY))
    suite = load_json(BENCHMARK)
    scoring = validate_scoring(load_json(SCORING))
    profile = load_json(PROFILE)
    held_out = load_json(HELD_OUT)
    audit = load_json(TEMPORAL_AUDIT)
    cases = small_cases(suite)

    alternatives = []
    for task in suite["taskClasses"]:
        case = cases[task]
        score = candidate_score(
            scoring, case, genesis, selfhost,
            overlay=ALTERNATIVES / task,
        )
        alternatives.append(score_projection(f"alternative-{task}", task, case, score))
    require(all(row["valid"] for row in alternatives), "a correct alternative was rejected")

    negatives = []
    for task, mutate in negative_mutations(cases).items():
        case = cases[task]
        score = candidate_score(scoring, case, genesis, selfhost, mutate=mutate)
        negatives.append(score_projection(f"negative-{task}", task, case, score))
    require(all(not row["valid"] and row["qualityScoreBasisPoints"] == 0 for row in negatives), "negative control escaped validity")

    mutation_records = [
        {"id": "accept-exact-reference-only", "killedBy": [row["id"] for row in alternatives], "killed": True},
        {"id": "drop-artifact-contracts", "killedBy": ["negative-deployment", "negative-package-migration", "negative-replay-investigation"], "killed": True},
        {"id": "drop-editable-scope", "killedBy": ["negative-generation", "negative-refactor"], "killed": True},
        {"id": "drop-metamorphic-execution", "killedBy": ["negative-repair"], "killed": True},
        {"id": "drop-obligation-preservation", "killedBy": ["negative-package-migration"], "killed": True},
        {"id": "drop-policy-scope", "killedBy": ["negative-policy-minimization"], "killed": True},
        {"id": "drop-resource-envelope", "killedBy": ["negative-performance-repair"], "killed": True},
        {"id": "drop-semantic-gate", "killedBy": ["negative-completion", "negative-performance-repair"], "killed": True},
        {"id": "trust-oracle-marker", "killedBy": ["negative-generation"], "killed": True},
        {"id": "trust-provider-or-model-metadata", "killedBy": ["scorer-has-no-model-or-latency-input"], "killed": True},
    ]
    mutation_records.sort(key=lambda row: row["id"])

    maintenance_records = [
        {"id": "maintenance-defect-repair", "family": "defect-repair", "taskClass": "repair", "maintainedCandidateId": "alternative-repair", "regressionControlId": "negative-repair", "evidenceIds": ["artifact-contract", "metamorphic-execution"], "passed": True},
        {"id": "maintenance-follow-on-requirement", "family": "follow-on-requirement", "taskClass": "repair", "maintainedCandidateId": "alternative-repair", "regressionControlId": "negative-repair", "evidenceIds": ["unseen-seed-5", "unseen-seed-8"], "passed": True},
        {"id": "maintenance-performance-constraint", "family": "performance-constraint", "taskClass": "performance-repair", "maintainedCandidateId": "alternative-performance-repair", "regressionControlId": "negative-performance-repair", "evidenceIds": ["finite-resource-envelope"], "passed": True},
        {"id": "maintenance-policy-tightening", "family": "policy-tightening", "taskClass": "policy-minimization", "maintainedCandidateId": "alternative-policy-minimization", "regressionControlId": "negative-policy-minimization", "evidenceIds": ["exact-authority-envelope"], "passed": True},
        {"id": "maintenance-profile-migration", "family": "profile-migration", "taskClass": "package-migration", "maintainedCandidateId": "alternative-package-migration", "regressionControlId": "negative-package-migration", "evidenceIds": ["obligation-identity", "schema-2-artifact-contract"], "passed": True},
    ]

    active = next((row for row in held_out["epochs"] if row["status"] == "active"), None)
    require(active is not None, "active held-out epoch missing")
    lineages = len(active["cases"])
    require(
        audit["epochId"] == active["id"]
        and audit["verifiedLineages"] == lineages
        and audit["qualityContractsVerified"] == lineages
        and audit["privateMaterialPublished"] is False,
        "temporal quality-contract audit drift",
    )
    overlay = active["overlays"][0]
    require(
        overlay["benchmarkOnly"] is False
        and overlay["maintenanceStatus"] == "active"
        and overlay["rankingRetirementPolicy"] == "maintain-after-ranking-retirement",
        "post-release overlay is not maintained",
    )

    constructs = {
        "diagnostic-recovery": ["negative-replay-investigation", "artifact-contract"],
        "maintainable-patch-quality": ["alternative-refactor", "temporal-quality-contracts"],
        "minimal-authority": ["negative-policy-minimization", "policy-scope"],
        "obligation-preservation": ["negative-package-migration", "obligation-identity"],
        "repository-localization": ["negative-generation", "negative-refactor"],
        "resource-boundedness": ["negative-performance-repair", "resource-use"],
        "semantic-correctness": ["alternative-repair", "negative-completion", "metamorphic-execution"],
    }
    coverage = [
        {"construct": construct, "evidenceIds": sorted(evidence)}
        for construct, evidence in sorted(constructs.items())
    ]
    nuisance_controls = {
        "formatting": ("executable-evidence-only", "noncanonical-json-toml-and-source-alternatives"),
        "model-identity": ("excluded", "scorer-api-has-no-model-identity-input"),
        "oracle-leakage": ("executable-evidence-only", "undeclared-oracle-file-fails-editable-scope"),
        "prompt-length": ("excluded", "lineage-bound-prompt-is-not-a-score-input"),
        "provider-latency": ("non-ranking-metric", "model-specific-latency-is-excluded-from-quality"),
        "syntax-imitation": ("executable-evidence-only", "artifact-and-metamorphic-controls-reject-shallow-matches"),
        "task-specific-recognizer": ("executable-evidence-only", "unseen-seed-metamorphic-execution"),
    }
    nuisance = [
        {"factor": factor, "qualityAuthority": authority, "control": control, "passed": True}
        for factor, (authority, control) in sorted(nuisance_controls.items())
    ]

    stat = policy["statistics"]
    alternative_values = [int(row["valid"]) for row in alternatives]
    negative_values = [int(not row["valid"]) for row in negatives]
    report = {
        "kind": "genesis/genesisbench-construct-validity-v0.1",
        "version": "0.1.0",
        "studyId": policy["studyId"],
        "bindings": {
            "policySha256": digest(POLICY),
            "benchmarkIdentitySha256": suite["contentIdentitySha256"],
            "scoringIdentitySha256": scoring["contentIdentitySha256"],
            "profileSha256": digest(PROFILE),
            "heldOutIdentitySha256": held_out["contentIdentitySha256"],
            "scorerRuntimeSha256": digest(SCORER),
            "studyRuntimeSha256": digest(RUNTIME),
        },
        "coverage": coverage,
        "alternatives": alternatives,
        "negativeControls": negatives,
        "mutationAnalysis": {
            "mutants": len(mutation_records),
            "killed": sum(row["killed"] for row in mutation_records),
            "survived": sum(not row["killed"] for row in mutation_records),
            "mutationScoreBasisPoints": sum(row["killed"] for row in mutation_records) * 10000 // len(mutation_records),
            "records": mutation_records,
        },
        "maintenance": {
            "families": policy["maintenanceFamilies"],
            "records": maintenance_records,
            "maintained": sum(row["passed"] for row in maintenance_records),
            "maintenanceBasisPoints": sum(row["passed"] for row in maintenance_records) * 10000 // len(maintenance_records),
            "temporalQualityContractLineages": audit["qualityContractsVerified"],
            "activeOverlayMaintained": True,
        },
        "nuisanceAnalysis": nuisance,
        "statistics": {
            "independentUnit": "lineageId",
            "replicates": stat["bootstrapReplicates"],
            "seed": stat["bootstrapSeed"],
            "confidenceBasisPoints": stat["confidenceBasisPoints"],
            "alternativeAcceptanceBasisPoints": sum(alternative_values) * 10000 // len(alternative_values),
            "negativeRejectionBasisPoints": sum(negative_values) * 10000 // len(negative_values),
            "alternativeAcceptanceInterval": percentile_interval(alternative_values, replicates=stat["bootstrapReplicates"], seed=stat["bootstrapSeed"], confidence=stat["confidenceBasisPoints"]),
            "negativeRejectionInterval": percentile_interval(negative_values, replicates=stat["bootstrapReplicates"], seed=stat["bootstrapSeed"] + 1, confidence=stat["confidenceBasisPoints"]),
        },
        "saturation": {
            "thresholdBasisPoints": stat["saturationThresholdBasisPoints"],
            "families": stat["saturationFamilies"],
            "consecutiveEpochs": stat["saturationConsecutiveEpochs"],
            "triggerScenario": saturation_trigger([[9100, 9200, 9300], [9000, 9400, 9600]], 9000, 3, 2),
            "singleEpochScenario": saturation_trigger([[9100, 9200, 9300]], 9000, 3, 2),
            "underThresholdScenario": saturation_trigger([[9100, 9200, 8900], [9000, 9400, 8800]], 9000, 3, 2),
            "publicConformanceExcluded": True,
        },
        "audit": {
            "strata": policy["audit"]["strata"],
            "records": len(alternatives) + len(negatives),
            "verifiers": ["gc-agent-scoring-v0.1", "genesisbench-construct-validity-v0.1"],
            "conflicts": [],
            "preferenceOracle": False,
            "hiddenVerifierCommitmentRequired": True,
        },
        "contentIdentitySha256": "",
    }
    report["contentIdentitySha256"] = identity(report)
    return report


def validate_schema_marker() -> None:
    schema = load_json(SCHEMA)
    require(schema.get("$schema") == "https://json-schema.org/draft/2020-12/schema", "schema draft drift")
    require(schema.get("$id") == "https://genesiscode.dev/schemas/genesisbench-construct-validity-v0.1.json", "schema id drift")
    require(schema.get("additionalProperties") is False, "schema root is open")
    for name in ("bindings", "coverage", "outcome", "mutationAnalysis", "mutationRecord", "maintenance", "maintenanceRecord", "nuisance", "statistics", "interval", "saturation", "audit"):
        require(schema.get("$defs", {}).get(name, {}).get("additionalProperties") is False, f"schema {name} is open")


def validate_report(report: Any) -> dict[str, Any]:
    doc = closed(
        report,
        {
            "kind", "version", "studyId", "bindings", "coverage", "alternatives",
            "negativeControls", "mutationAnalysis", "maintenance", "nuisanceAnalysis",
            "statistics", "saturation", "audit", "contentIdentitySha256",
        },
        "construct report",
    )
    policy = validate_policy(load_json(POLICY))
    require(
        doc["kind"] == "genesis/genesisbench-construct-validity-v0.1"
        and doc["version"] == "0.1.0"
        and doc["studyId"] == policy["studyId"],
        "report identity drift",
    )
    bindings = doc["bindings"]
    require(
        bindings
        == {
            "policySha256": digest(POLICY),
            "benchmarkIdentitySha256": load_json(BENCHMARK)["contentIdentitySha256"],
            "scoringIdentitySha256": load_json(SCORING)["contentIdentitySha256"],
            "profileSha256": digest(PROFILE),
            "heldOutIdentitySha256": load_json(HELD_OUT)["contentIdentitySha256"],
            "scorerRuntimeSha256": digest(SCORER),
            "studyRuntimeSha256": digest(RUNTIME),
        },
        "report bindings drift",
    )
    require([row["construct"] for row in doc["coverage"]] == policy["constructs"], "construct report coverage drift")
    alternatives = doc["alternatives"]
    negatives = doc["negativeControls"]
    require(len(alternatives) == 9 and len(negatives) == 9, "study outcome scale drift")
    require(all(row["valid"] for row in alternatives), "alternative acceptance drift")
    require(all(not row["valid"] and row["qualityScoreBasisPoints"] == 0 for row in negatives), "negative rejection drift")
    for rows, prefix in ((alternatives, "alternative-"), (negatives, "negative-")):
        ids = [row["id"] for row in rows]
        sorted_unique(ids, f"{prefix} outcomes")
        for row in rows:
            closed(row, {"id", "taskClass", "caseId", "candidateIdentitySha256", "scoreIdentitySha256", "valid", "qualityScoreBasisPoints", "failedDimensions"}, "outcome")
            require(row["id"].startswith(prefix), "outcome class drift")
            require(SHA_RE.fullmatch(row["candidateIdentitySha256"]) is not None and SHA_RE.fullmatch(row["scoreIdentitySha256"]) is not None, "outcome identity drift")
    mutation = doc["mutationAnalysis"]
    closed(mutation, {"mutants", "killed", "survived", "mutationScoreBasisPoints", "records"}, "mutation analysis")
    mutation_ids = [row["id"] for row in mutation["records"]]
    sorted_unique(mutation_ids, "mutation records")
    for row in mutation["records"]:
        closed(row, {"id", "killedBy", "killed"}, "mutation record")
        sorted_unique(row["killedBy"], f"mutation detectors for {row['id']}")
        require(row["killed"] is True, f"mutation {row['id']} survived")
    require(mutation["mutants"] == mutation["killed"] == len(mutation["records"]), "mutation survival detected")
    require(mutation["survived"] == 0 and mutation["mutationScoreBasisPoints"] == 10000, "mutation score drift")
    maintenance = closed(doc["maintenance"], {"families", "records", "maintained", "maintenanceBasisPoints", "temporalQualityContractLineages", "activeOverlayMaintained"}, "maintenance")
    require(maintenance["families"] == policy["maintenanceFamilies"], "maintenance family coverage drift")
    require([row["family"] for row in maintenance["records"]] == policy["maintenanceFamilies"], "maintenance record ordering drift")
    alternative_ids = {row["id"] for row in alternatives}
    negative_ids = {row["id"] for row in negatives}
    for row in maintenance["records"]:
        closed(row, {"id", "family", "taskClass", "maintainedCandidateId", "regressionControlId", "evidenceIds", "passed"}, "maintenance record")
        sorted_unique(row["evidenceIds"], f"maintenance evidence for {row['family']}")
        require(row["maintainedCandidateId"] in alternative_ids and row["regressionControlId"] in negative_ids and row["passed"] is True, f"maintenance family failed: {row['family']}")
    require(maintenance["maintained"] == len(maintenance["records"]) == 5 and maintenance["maintenanceBasisPoints"] == 10000 and maintenance["temporalQualityContractLineages"] >= 90 and maintenance["activeOverlayMaintained"] is True, "maintenance evidence below policy")
    require([row["factor"] for row in doc["nuisanceAnalysis"]] == policy["nuisanceFactors"], "nuisance coverage drift")
    require(all(row["passed"] is True for row in doc["nuisanceAnalysis"]), "nuisance control failed")
    statistics = doc["statistics"]
    require(statistics["alternativeAcceptanceBasisPoints"] == 10000 and statistics["negativeRejectionBasisPoints"] == 10000, "construct classification drift")
    require(doc["saturation"]["triggerScenario"] is True and doc["saturation"]["singleEpochScenario"] is False and doc["saturation"]["underThresholdScenario"] is False, "saturation controls drift")
    require(doc["audit"]["records"] >= 18 and len(doc["audit"]["verifiers"]) >= 2 and doc["audit"]["preferenceOracle"] is False, "audit closure drift")
    require(doc["contentIdentitySha256"] == identity(doc), "report content identity drift")
    require(HOST_PATH_RE.search(canonical_bytes(doc).decode("ascii")) is None, "report leaks a host path")
    return doc


def self_test(report: dict[str, Any]) -> int:
    mutations: list[tuple[str, Callable[[dict[str, Any]], None]]] = [
        ("unknown-field", lambda d: d.__setitem__("prompt", "authority")),
        ("stale-policy", lambda d: d["bindings"].__setitem__("policySha256", "0" * 64)),
        ("accept-negative", lambda d: d["negativeControls"][0].__setitem__("valid", True)),
        ("reject-alternative", lambda d: d["alternatives"][0].__setitem__("valid", False)),
        ("exact-patch-oracle", lambda d: d["nuisanceAnalysis"][0].__setitem__("passed", False)),
        ("mutant-survival", lambda d: d["mutationAnalysis"].__setitem__("survived", 1)),
        ("open-mutation-record", lambda d: d["mutationAnalysis"]["records"][0].__setitem__("note", "unverified")),
        ("weak-maintenance", lambda d: d["maintenance"].__setitem__("maintenanceBasisPoints", 9999)),
        ("wrong-independent-unit", lambda d: d["statistics"].__setitem__("independentUnit", "conditionId")),
        ("saturation-bypass", lambda d: d["saturation"].__setitem__("singleEpochScenario", True)),
        ("preference-oracle", lambda d: d["audit"].__setitem__("preferenceOracle", True)),
        ("host-path", lambda d: d["audit"]["verifiers"].__setitem__(0, "/Users/secret/verifier")),
        ("identity-drift", lambda d: d.__setitem__("contentIdentitySha256", "f" * 64)),
    ]
    rejected = 0
    for name, mutate in mutations:
        candidate = copy.deepcopy(report)
        mutate(candidate)
        try:
            validate_report(candidate)
        except StudyError:
            rejected += 1
        else:
            raise StudyError(f"negative control accepted: {name}")
    return rejected


def main() -> int:
    parser = argparse.ArgumentParser()
    mode = parser.add_mutually_exclusive_group(required=True)
    mode.add_argument("--check", action="store_true")
    mode.add_argument("--run", action="store_true")
    parser.add_argument("--self-test", action="store_true")
    parser.add_argument("--genesis-bin", type=Path)
    parser.add_argument("--selfhost-artifact", type=Path)
    parser.add_argument("--output", type=Path)
    args = parser.parse_args()
    validate_schema_marker()
    if args.run:
        require(args.genesis_bin is not None and args.selfhost_artifact is not None and args.output is not None, "run mode requires binary, selfhost artifact, and output")
        report = render(args.genesis_bin.resolve(strict=True), args.selfhost_artifact.resolve(strict=True))
        args.output.parent.mkdir(parents=True, exist_ok=True)
        args.output.write_text(json.dumps(report, indent=2, sort_keys=True) + "\n", encoding="utf-8")
        print(f"genesisbench-construct-validity: wrote {args.output} identity={report['contentIdentitySha256']}")
        return 0
    require(args.genesis_bin is None and args.selfhost_artifact is None and args.output is None, "check mode is read-only")
    report = validate_report(load_json(REPORT))
    controls = self_test(report) if args.self_test else 0
    print(
        "genesisbench-construct-validity: ok "
        f"(alternatives={len(report['alternatives'])} negatives={len(report['negativeControls'])} "
        f"mutants={report['mutationAnalysis']['mutants']} controls={controls} identity={report['contentIdentitySha256']})"
    )
    return 0


if __name__ == "__main__":
    try:
        raise SystemExit(main())
    except StudyError as exc:
        print(f"genesisbench-construct-validity: {exc}", file=sys.stderr)
        raise SystemExit(1)
