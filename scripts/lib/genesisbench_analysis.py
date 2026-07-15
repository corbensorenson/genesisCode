#!/usr/bin/env python3
"""Deterministic lineage-clustered GenesisBench analysis and controls."""

from __future__ import annotations

import argparse
import copy
import hashlib
import json
import math
from decimal import Decimal, ROUND_HALF_EVEN, getcontext
from fractions import Fraction
from pathlib import Path
from typing import Any, Callable

ROOT = Path(__file__).resolve().parents[2]
SUITE = ROOT / "benchmarks/agent_tasks/v0.1/suite.json"
PLAN = ROOT / "docs/spec/GENESISBENCH_ANALYSIS_PLAN_v0.1.json"
OBSERVATIONS = ROOT / "benchmarks/genesisbench/v0.1/analysis/observations.fixture.json"
REPORT = ROOT / "benchmarks/genesisbench/v0.1/analysis/report.fixture.json"
HASH = "0" * 64
getcontext().prec = 50


class AnalysisError(ValueError):
    pass


def require(condition: bool, message: str) -> None:
    if not condition:
        raise AnalysisError(message)


def load(path: Path) -> Any:
    def unique(rows: list[tuple[str, Any]]) -> dict[str, Any]:
        result: dict[str, Any] = {}
        for key, value in rows:
            require(key not in result, f"duplicate JSON key: {key}")
            result[key] = value
        return result
    return json.loads(path.read_text(encoding="utf-8"), object_pairs_hook=unique)


def canonical(value: Any) -> bytes:
    return (json.dumps(value, sort_keys=True, separators=(",", ":"), ensure_ascii=True) + "\n").encode("ascii")


def content_identity(document: dict[str, Any]) -> str:
    unsigned = copy.deepcopy(document)
    unsigned["contentIdentitySha256"] = ""
    return hashlib.sha256(canonical(unsigned)).hexdigest()


def seal(document: dict[str, Any]) -> dict[str, Any]:
    document["contentIdentitySha256"] = ""
    document["contentIdentitySha256"] = content_identity(document)
    return document


def expected_plan(suite: dict[str, Any]) -> dict[str, Any]:
    return seal({
        "kind": "genesis/genesisbench-analysis-plan-v0.1",
        "version": "0.1.0",
        "analysisId": "GenesisBench-Lineage-Analysis-v0.1",
        "status": "predeclared",
        "taskBenchmark": {
            "id": suite["benchmarkId"],
            "identitySha256": suite["contentIdentitySha256"],
            "independentLineages": 9,
            "repeatedConditions": 27,
        },
        "sampling": {
            "independentUnit": "lineageId",
            "clusterKey": "lineageIdentitySha256",
            "conditionUnit": "conditionId",
            "primaryConditionSelector": {"dimension": "contextTier", "value": "small"},
            "repeatedConditionsCountAsIndependent": False,
            "hierarchicalModel": None,
        },
        "outcomes": {
            "closedStates": ["abstained", "invalid", "missing", "solved", "unsolved"],
            "verifiedSolveNumerator": ["solved"],
            "verifiedSolveDenominator": ["abstained", "invalid", "missing", "solved", "unsolved"],
            "conditionalQualityStates": ["solved"],
            "missingness": "explicit-cell-counted-unsolved",
            "invalids": "reported-separately-counted-unsolved",
            "abstentions": "reported-separately-counted-unsolved",
        },
        "intervals": {
            "confidenceBasisPoints": 9500,
            "solveRate": "wilson-score-lineage-binomial-v0.1",
            "pairedEffect": "newcombe-wilson-risk-difference-v0.1",
            "rounding": "nearest-basis-point-half-even",
        },
        "comparisons": {
            "family": "all-system-pairs-within-identical-cohort-and-primary-lineage-set",
            "test": "exact-two-sided-mcnemar-binomial-v0.1",
            "multipleComparison": "holm-bonferroni-familywise-v0.1",
            "alphaPartsPerBillion": 50_000_000,
            "unsupportedPairwiseDecimalRanksAllowed": False,
            "orderingRule": "only-significant-paired-effect-otherwise-indeterminate",
        },
        "saturation": {
            "thresholdSolveRateBasisPoints": 9000,
            "minimumDistinctModelFamilies": 3,
            "consecutiveEpochs": 2,
            "unit": "primary-condition-lineage",
            "publicConformanceMayTrigger": False,
        },
        "publication": {
            "exactDenominatorsRequired": True,
            "allOutcomeCountsRequired": True,
            "lineageClusterDisclosureRequired": True,
            "effectSizesAndUncertaintyRequired": True,
            "crossCohortAggregationAllowed": False,
            "rawObservationsRetained": True,
        },
        "contentIdentitySha256": "",
    })


def validate_plan(plan: Any, suite: dict[str, Any]) -> dict[str, Any]:
    require(isinstance(plan, dict) and plan == expected_plan(suite), "analysis plan differs from predeclared authority")
    require(content_identity(plan) == plan["contentIdentitySha256"], "analysis plan identity drift")
    return plan


def fixture_observations(plan: dict[str, Any], suite: dict[str, Any]) -> dict[str, Any]:
    systems = [
        {"id": "reference-alpha", "modelFamilyId": "fixture-family-alpha"},
        {"id": "reference-beta", "modelFamilyId": "fixture-family-beta"},
        {"id": "reference-gamma", "modelFamilyId": "fixture-family-gamma"},
    ]
    states = {
        "reference-alpha": ["solved", "solved", "solved", "solved", "solved", "solved", "solved", "invalid", "abstained"],
        "reference-beta": ["solved", "solved", "unsolved", "solved", "unsolved", "solved", "solved", "invalid", "missing"],
        "reference-gamma": ["solved", "unsolved", "unsolved", "solved", "unsolved", "solved", "unsolved", "abstained", "missing"],
    }
    rows = []
    for system in systems:
        for case_index, case in enumerate(suite["cases"]):
            lineage_index = case_index // 3
            primary = states[system["id"]][lineage_index]
            if case["contextTier"] == "medium" and primary == "unsolved":
                outcome = "solved"
            elif case["contextTier"] == "large" and primary == "missing":
                outcome = "unsolved"
            else:
                outcome = primary
            quality = 9200 - (lineage_index * 37) - (["reference-alpha", "reference-beta", "reference-gamma"].index(system["id"]) * 211)
            rows.append({
                "systemId": system["id"],
                "epochId": "public-anchor-epoch-001",
                "caseId": case["id"],
                "lineageId": case["lineageId"],
                "lineageIdentitySha256": case["lineageIdentitySha256"],
                "conditionId": case["conditionId"],
                "conditionIdentitySha256": case["conditionIdentitySha256"],
                "outcome": outcome,
                "qualityScoreBasisPoints": quality if outcome == "solved" else None,
                "reasonCodes": [] if outcome == "solved" else [f"fixture/{outcome}"],
            })
    return seal({
        "kind": "genesis/genesisbench-observations-v0.1",
        "version": "0.1.0",
        "analysisPlanIdentitySha256": plan["contentIdentitySha256"],
        "taskBenchmarkIdentitySha256": suite["contentIdentitySha256"],
        "publicationMode": "unranked-reference-conformance",
        "cohortIdentitySha256": hashlib.sha256(b"genesisbench-reference-conformance-cohort-v0.1").hexdigest(),
        "systems": systems,
        "epochs": ["public-anchor-epoch-001"],
        "observations": rows,
        "contentIdentitySha256": "",
    })


def validate_observations(doc: Any, plan: dict[str, Any], suite: dict[str, Any]) -> dict[str, Any]:
    require(isinstance(doc, dict), "observations must be an object")
    required = {"kind", "version", "analysisPlanIdentitySha256", "taskBenchmarkIdentitySha256", "publicationMode", "cohortIdentitySha256", "systems", "epochs", "observations", "contentIdentitySha256"}
    require(set(doc) == required, "observation fields are not closed")
    require(doc["kind"] == "genesis/genesisbench-observations-v0.1" and doc["version"] == "0.1.0", "observation version drift")
    require(doc["analysisPlanIdentitySha256"] == plan["contentIdentitySha256"] and doc["taskBenchmarkIdentitySha256"] == suite["contentIdentitySha256"], "observation authority binding drift")
    require(doc["publicationMode"] in {"ranked", "unranked-reference-conformance", "unranked-research"}, "invalid publication mode")
    require(isinstance(doc["cohortIdentitySha256"], str) and len(doc["cohortIdentitySha256"]) == 64 and all(character in "0123456789abcdef" for character in doc["cohortIdentitySha256"]), "invalid cohort identity")
    require(content_identity(doc) == doc["contentIdentitySha256"], "observation identity drift")
    system_ids = [row.get("id") for row in doc["systems"]]
    require(system_ids == sorted(set(system_ids)) and len(system_ids) >= 1, "systems must be sorted and unique")
    family_ids = []
    for system in doc["systems"]:
        require(set(system) == {"id", "modelFamilyId"}, "system fields are not closed")
        family_ids.append(system["modelFamilyId"])
    require(doc["epochs"] == sorted(set(doc["epochs"])) and doc["epochs"], "epochs must be sorted and unique")
    case_map = {case["id"]: case for case in suite["cases"]}
    expected = {(system, epoch, case) for system in system_ids for epoch in doc["epochs"] for case in case_map}
    observed = set()
    for row in doc["observations"]:
        require(set(row) == {"systemId", "epochId", "caseId", "lineageId", "lineageIdentitySha256", "conditionId", "conditionIdentitySha256", "outcome", "qualityScoreBasisPoints", "reasonCodes"}, "observation cell fields are not closed")
        key = (row["systemId"], row["epochId"], row["caseId"])
        require(key in expected and key not in observed, "unknown or duplicate observation cell")
        observed.add(key)
        case = case_map[row["caseId"]]
        for field in ("lineageId", "lineageIdentitySha256", "conditionId", "conditionIdentitySha256"):
            require(row[field] == case[field], f"observation {field} binding drift")
        require(row["outcome"] in plan["outcomes"]["closedStates"], "unknown observation outcome")
        quality = row["qualityScoreBasisPoints"]
        require((row["outcome"] == "solved" and isinstance(quality, int) and 0 <= quality <= 10_000) or (row["outcome"] != "solved" and quality is None), "quality must exist exactly for solved cells")
        require(isinstance(row["reasonCodes"], list) and row["reasonCodes"] == sorted(set(row["reasonCodes"])), "reason codes must be sorted and unique")
    require(observed == expected, "observation matrix is incomplete; missing cells must be explicit")
    require(doc["observations"] == sorted(doc["observations"], key=lambda row: (row["systemId"], row["epochId"], row["caseId"])), "observations are not canonical")
    return doc


def bps(value: Decimal) -> int:
    return int((value * Decimal(10_000)).quantize(Decimal(1), rounding=ROUND_HALF_EVEN))


def wilson(successes: int, total: int) -> list[int]:
    require(total > 0 and 0 <= successes <= total, "invalid Wilson inputs")
    z = Decimal("1.959963984540054")
    n = Decimal(total)
    p = Decimal(successes) / n
    denominator = Decimal(1) + z * z / n
    center = (p + z * z / (Decimal(2) * n)) / denominator
    margin = z * ((p * (Decimal(1) - p) / n + z * z / (Decimal(4) * n * n)).sqrt()) / denominator
    return [max(0, bps(center - margin)), min(10_000, bps(center + margin))]


def outcome_counts(rows: list[dict[str, Any]], states: list[str]) -> dict[str, int]:
    return {state: sum(row["outcome"] == state for row in rows) for state in states}


def system_summary(system: str, rows: list[dict[str, Any]], states: list[str]) -> dict[str, Any]:
    counts = outcome_counts(rows, states)
    total = len(rows)
    solved = counts["solved"]
    quality = [row["qualityScoreBasisPoints"] for row in rows if row["outcome"] == "solved"]
    return {
        "systemId": system,
        "independentLineageDenominator": total,
        "verifiedSolvedLineages": solved,
        "solveRateBasisPoints": bps(Decimal(solved) / Decimal(total)),
        "solveRateWilson95BasisPoints": wilson(solved, total),
        "outcomes": counts,
        "conditionalQualityDenominator": len(quality),
        "conditionalQualityMeanBasisPoints": None if not quality else int((Decimal(sum(quality)) / Decimal(len(quality))).quantize(Decimal(1), rounding=ROUND_HALF_EVEN)),
    }


def exact_mcnemar(left: dict[str, bool], right: dict[str, bool]) -> tuple[int, int, Fraction]:
    left_only = sum(left[key] and not right[key] for key in left)
    right_only = sum(right[key] and not left[key] for key in left)
    discordant = left_only + right_only
    if discordant == 0:
        return left_only, right_only, Fraction(1, 1)
    tail = sum(math.comb(discordant, k) for k in range(min(left_only, right_only) + 1))
    return left_only, right_only, min(Fraction(1, 1), Fraction(2 * tail, 2 ** discordant))


def analyze(
    plan: dict[str, Any], observations: dict[str, Any], suite: dict[str, Any],
    *, validate_output: bool = True,
) -> dict[str, Any]:
    validate_plan(plan, suite)
    validate_observations(observations, plan, suite)
    primary_cases = {case["id"] for case in suite["cases"] if case["contextTier"] == plan["sampling"]["primaryConditionSelector"]["value"]}
    systems = [row["id"] for row in observations["systems"]]
    latest_epoch = observations["epochs"][-1]
    primary: dict[str, list[dict[str, Any]]] = {}
    condition_summaries = []
    for system in systems:
        primary[system] = [row for row in observations["observations"] if row["systemId"] == system and row["epochId"] == latest_epoch and row["caseId"] in primary_cases]
        require(len({row["lineageId"] for row in primary[system]}) == len(primary[system]) == len(suite["lineages"]), "primary analysis must contain one condition per lineage")
        for tier in ["small", "medium", "large"]:
            rows = [row for row in observations["observations"] if row["systemId"] == system and row["epochId"] == latest_epoch and next(case for case in suite["cases"] if case["id"] == row["caseId"])["contextTier"] == tier]
            item = system_summary(system, rows, plan["outcomes"]["closedStates"])
            item["contextTier"] = tier
            item["clusteredBy"] = "lineageId"
            condition_summaries.append(item)
    summaries = [system_summary(system, primary[system], plan["outcomes"]["closedStates"]) for system in systems]
    comparisons = []
    pvalues: list[Fraction] = []
    for left_index, left_system in enumerate(systems):
        for right_system in systems[left_index + 1:]:
            left_rows = {row["lineageId"]: row["outcome"] == "solved" for row in primary[left_system]}
            right_rows = {row["lineageId"]: row["outcome"] == "solved" for row in primary[right_system]}
            require(set(left_rows) == set(right_rows), "paired comparison lineage set drift")
            left_only, right_only, pvalue = exact_mcnemar(left_rows, right_rows)
            left_solved, right_solved, total = sum(left_rows.values()), sum(right_rows.values()), len(left_rows)
            left_interval, right_interval = wilson(left_solved, total), wilson(right_solved, total)
            effect = bps(Decimal(left_solved - right_solved) / Decimal(total))
            effect_interval = [left_interval[0] - right_interval[1], left_interval[1] - right_interval[0]]
            pvalues.append(pvalue)
            comparisons.append({
                "leftSystemId": left_system,
                "rightSystemId": right_system,
                "pairedLineages": total,
                "leftOnlySolved": left_only,
                "rightOnlySolved": right_only,
                "solveRateDifferenceBasisPoints": effect,
                "effectWilson95BasisPoints": effect_interval,
                "exactPValue": {"numerator": pvalue.numerator, "denominator": pvalue.denominator},
                "holmAdjustedPValuePartsPerBillion": None,
                "significant": False,
                "ordering": "indeterminate",
            })
    order = sorted(range(len(pvalues)), key=lambda index: (pvalues[index], comparisons[index]["leftSystemId"], comparisons[index]["rightSystemId"]))
    running = Fraction(0, 1)
    family = len(order)
    alpha = plan["comparisons"]["alphaPartsPerBillion"]
    for rank, index in enumerate(order):
        adjusted = min(Fraction(1, 1), pvalues[index] * (family - rank))
        running = max(running, adjusted)
        comparisons[index]["holmAdjustedPValuePartsPerBillion"] = min(1_000_000_000, int((Decimal(running.numerator) * Decimal(1_000_000_000) / Decimal(running.denominator)).quantize(Decimal(1), rounding=ROUND_HALF_EVEN)))
        significant = running * 1_000_000_000 <= alpha and observations["publicationMode"] == "ranked"
        comparisons[index]["significant"] = significant
        if significant and comparisons[index]["solveRateDifferenceBasisPoints"] != 0:
            comparisons[index]["ordering"] = comparisons[index]["leftSystemId"] if comparisons[index]["solveRateDifferenceBasisPoints"] > 0 else comparisons[index]["rightSystemId"]
    family_count = len({row["modelFamilyId"] for row in observations["systems"]})
    epoch_eligible = []
    for epoch in observations["epochs"]:
        rates = []
        for system in systems:
            rows = [row for row in observations["observations"] if row["systemId"] == system and row["epochId"] == epoch and row["caseId"] in primary_cases]
            rates.append(bps(Decimal(sum(row["outcome"] == "solved" for row in rows)) / Decimal(len(rows))))
        epoch_eligible.append(family_count >= plan["saturation"]["minimumDistinctModelFamilies"] and sorted(rates, reverse=True)[:3] and min(sorted(rates, reverse=True)[:3]) >= plan["saturation"]["thresholdSolveRateBasisPoints"])
    saturated = observations["publicationMode"] == "ranked" and len(epoch_eligible) >= 2 and all(epoch_eligible[-plan["saturation"]["consecutiveEpochs"]:])
    report = seal({
        "kind": "genesis/genesisbench-analysis-report-v0.1",
        "version": "0.1.0",
        "analysisPlanIdentitySha256": plan["contentIdentitySha256"],
        "observationsIdentitySha256": observations["contentIdentitySha256"],
        "taskBenchmarkIdentitySha256": suite["contentIdentitySha256"],
        "publicationMode": observations["publicationMode"],
        "cohortIdentitySha256": observations["cohortIdentitySha256"],
        "latestEpochId": latest_epoch,
        "independentLineages": len(suite["lineages"]),
        "repeatedConditions": len(suite["conditions"]),
        "primaryCondition": plan["sampling"]["primaryConditionSelector"],
        "systemSummaries": summaries,
        "conditionSummaries": condition_summaries,
        "comparisons": comparisons,
        "multipleComparison": {"method": "holm-bonferroni-familywise-v0.1", "familySize": len(comparisons), "alphaPartsPerBillion": alpha},
        "saturation": {"saturated": saturated, "eligibleEpochs": sum(epoch_eligible), "requiredConsecutiveEpochs": plan["saturation"]["consecutiveEpochs"], "distinctModelFamilies": family_count, "reason": "threshold-met" if saturated else "insufficient-ranked-consecutive-epoch-evidence"},
        "rankClaimsAllowed": observations["publicationMode"] == "ranked",
        "contentIdentitySha256": "",
    })
    if validate_output:
        validate_report(report, plan, observations, suite)
    return report


def validate_report(report: Any, plan: dict[str, Any], observations: dict[str, Any], suite: dict[str, Any]) -> dict[str, Any]:
    expected = analyze(plan, observations, suite, validate_output=False)
    require(report == expected, "analysis report differs from deterministic analysis")
    require(content_identity(report) == report["contentIdentitySha256"], "analysis report identity drift")
    return report
def self_test(plan: dict[str, Any], observations: dict[str, Any], suite: dict[str, Any], report: dict[str, Any]) -> int:
    invalid_index = next(index for index, row in enumerate(observations["observations"]) if row["outcome"] == "invalid")
    abstained_index = next(index for index, row in enumerate(observations["observations"]) if row["outcome"] == "abstained")
    controls: list[tuple[str, str, Callable[[dict[str, Any]], None]]] = [
        ("observations", "unknown-field", lambda d: d.update({"authority": True})),
        ("observations", "identity", lambda d: d.__setitem__("contentIdentitySha256", HASH)),
        ("observations", "missing-cell", lambda d: d["observations"].pop()),
        ("observations", "duplicate-cell", lambda d: d["observations"].append(copy.deepcopy(d["observations"][0]))),
        ("observations", "lineage-promotion", lambda d: d["observations"][1].__setitem__("lineageId", "lineage-false-999")),
        ("observations", "lineage-identity", lambda d: d["observations"][0].__setitem__("lineageIdentitySha256", HASH)),
        ("observations", "condition-rebind", lambda d: d["observations"][0].__setitem__("conditionId", d["observations"][1]["conditionId"])),
        ("observations", "condition-identity", lambda d: d["observations"][0].__setitem__("conditionIdentitySha256", HASH)),
        ("observations", "invalid-as-solved", lambda d: d["observations"][invalid_index].__setitem__("outcome", "solved")),
        ("observations", "quality-on-invalid", lambda d: d["observations"][invalid_index].__setitem__("qualityScoreBasisPoints", 9999)),
        ("observations", "quality-missing-on-solve", lambda d: d["observations"][0].__setitem__("qualityScoreBasisPoints", None)),
        ("observations", "malformed-cohort", lambda d: d.__setitem__("cohortIdentitySha256", "cross-cohort")),
        ("observations", "duplicate-system", lambda d: d["systems"].append(copy.deepcopy(d["systems"][0]))),
        ("observations", "unsorted-systems", lambda d: d["systems"].reverse()),
        ("observations", "unknown-state", lambda d: d["observations"][0].__setitem__("outcome", "partial")),
        ("observations", "silent-abstention", lambda d: d["observations"].pop(abstained_index)),
        ("report", "denominator", lambda d: d["systemSummaries"][0].__setitem__("independentLineageDenominator", 27)),
        ("report", "condition-as-independent", lambda d: d.__setitem__("independentLineages", 27)),
        ("report", "unsupported-rank", lambda d: d.__setitem__("rankClaimsAllowed", True)),
        ("report", "pvalue", lambda d: d["comparisons"][0].__setitem__("holmAdjustedPValuePartsPerBillion", 0)),
        ("report", "false-significance", lambda d: d["comparisons"][0].__setitem__("significant", True)),
        ("report", "false-saturation", lambda d: d["saturation"].__setitem__("saturated", True)),
        ("report", "missing-count", lambda d: d["systemSummaries"][1]["outcomes"].__setitem__("missing", 0)),
        ("report", "identity", lambda d: d.__setitem__("contentIdentitySha256", HASH)),
    ]
    passed = 0
    for target, name, mutate in controls:
        candidate = copy.deepcopy(observations if target == "observations" else report)
        mutate(candidate)
        if name != "identity":
            candidate["contentIdentitySha256"] = content_identity(candidate)
        try:
            if target == "observations":
                validate_observations(candidate, plan, suite)
            else:
                validate_report(candidate, plan, observations, suite)
        except AnalysisError:
            passed += 1
        else:
            raise AnalysisError(f"negative control accepted: {name}")
    return passed


def refresh() -> None:
    suite = load(SUITE)
    plan = expected_plan(suite)
    observations = fixture_observations(plan, suite)
    observations["observations"] = sorted(observations["observations"], key=lambda row: (row["systemId"], row["epochId"], row["caseId"]))
    observations["contentIdentitySha256"] = content_identity(observations)
    report = analyze(plan, observations, suite)
    PLAN.write_text(json.dumps(plan, indent=2, sort_keys=True) + "\n", encoding="utf-8")
    OBSERVATIONS.parent.mkdir(parents=True, exist_ok=True)
    OBSERVATIONS.write_text(json.dumps(observations, indent=2, sort_keys=True) + "\n", encoding="utf-8")
    REPORT.write_text(json.dumps(report, indent=2, sort_keys=True) + "\n", encoding="utf-8")


def main() -> int:
    parser = argparse.ArgumentParser()
    parser.add_argument("--check", action="store_true")
    parser.add_argument("--self-test", action="store_true")
    parser.add_argument("--refresh-fixtures", action="store_true")
    args = parser.parse_args()
    require(args.check != args.refresh_fixtures, "choose exactly one of --check or --refresh-fixtures")
    if args.refresh_fixtures:
        refresh()
        return 0
    suite, plan, observations, report = load(SUITE), load(PLAN), load(OBSERVATIONS), load(REPORT)
    validate_plan(plan, suite)
    validate_observations(observations, plan, suite)
    validate_report(report, plan, observations, suite)
    controls = self_test(plan, observations, suite, report) if args.self_test else 0
    print(f"genesisbench-analysis: ok (lineages={len(suite['lineages'])} conditions={len(suite['conditions'])} systems={len(observations['systems'])} controls={controls} identity={report['contentIdentitySha256']})")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
