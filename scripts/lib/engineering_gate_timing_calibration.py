#!/usr/bin/env python3
"""Validate class-separated engineering timing calibration evidence."""

from __future__ import annotations

import copy
import hashlib
import json
import math
from pathlib import Path
from typing import Any


ROOT = Path(__file__).resolve().parents[2]
POLICY_PATH = ROOT / "policies/engineering_gate_timing_calibration_v0.1.json"
EVIDENCE_PATH = ROOT / "docs/program/ENGINEERING_GATE_TIMING_CALIBRATION_v0.1.json"
SCHEMA_PATH = ROOT / "docs/spec/ENGINEERING_GATE_TIMING_CALIBRATION_v0.1.schema.json"
CLASS_IDS = ["local-warm", "local-clean-fallback", "hosted-cold-shared-runner"]
HEX_FIELDS = {
    "gitCommit": 40,
    "hostIdentitySha256": 64,
    "toolchainIdentitySha256": 64,
    "workloadIdentitySha256": 64,
}
SAMPLE_FIELDS = {
    "sequence",
    "durationMs",
    "outcome",
    *HEX_FIELDS,
    "cachePrecondition",
    "competingLaneState",
    "sourceIdentity",
}
POLICY_FIELDS = {
    "kind",
    "version",
    "roadmapTask",
    "schemaPath",
    "evidencePath",
    "sampling",
    "statistics",
    "trend",
    "ceilingDerivation",
    "classes",
}
EVIDENCE_FIELDS = {
    "kind",
    "version",
    "status",
    "policySha256",
    "schemaSha256",
    "classes",
    "nonclaims",
}
CLASS_POLICY_FIELDS = {
    "id",
    "profile",
    "workloadIdentity",
    "cachePrecondition",
    "hostClass",
    "competingLaneState",
    "sourceIdentityKind",
    "ceilingStatus",
    "hardCeilingMs",
}
CLASS_EVIDENCE_FIELDS = {
    "id",
    "warmups",
    "retainedSamples",
    "failedSamples",
    "statistics",
    "trend",
    "derivedHardCeiling",
}


class TimingCalibrationError(ValueError):
    pass


def require(condition: bool, message: str) -> None:
    if not condition:
        raise TimingCalibrationError(message)


def unique_pairs(pairs: list[tuple[str, Any]]) -> dict[str, Any]:
    result: dict[str, Any] = {}
    for key, value in pairs:
        require(key not in result, f"duplicate JSON key: {key}")
        result[key] = value
    return result


def load_json(path: Path) -> Any:
    try:
        return json.loads(path.read_text(encoding="utf-8"), object_pairs_hook=unique_pairs)
    except (OSError, json.JSONDecodeError) as exc:
        raise TimingCalibrationError(f"cannot load {path.relative_to(ROOT)}: {exc}") from exc


def file_sha256(path: Path) -> str:
    return hashlib.sha256(path.read_bytes()).hexdigest()


def canonical_path(raw: Any, expected: str, field: str) -> None:
    require(raw == expected, f"{field} path mismatch")
    require((ROOT / expected).is_file(), f"{field} path does not exist")


def normalize_rational(numerator: int, denominator: int) -> dict[str, int]:
    require(denominator > 0, "rational denominator must be positive")
    divisor = math.gcd(numerator, denominator)
    return {"numerator": numerator // divisor, "denominator": denominator // divisor}


def ceil_div(numerator: int, denominator: int) -> int:
    require(numerator >= 0 and denominator > 0, "ceil division requires non-negative inputs")
    return (numerator + denominator - 1) // denominator


def rational_median(values: list[int]) -> dict[str, int]:
    require(bool(values), "median requires at least one value")
    ordered = sorted(values)
    size = len(ordered)
    if size % 2:
        return {"numerator": ordered[size // 2], "denominator": 1}
    return normalize_rational(ordered[size // 2 - 1] + ordered[size // 2], 2)


def rational_mad(values: list[int]) -> dict[str, int]:
    median = rational_median(values)
    scaled_deviations = [
        abs(value * median["denominator"] - median["numerator"])
        for value in values
    ]
    scaled_median = rational_median(scaled_deviations)
    return normalize_rational(
        scaled_median["numerator"],
        scaled_median["denominator"] * median["denominator"],
    )


def statistics(values: list[int], policy: dict[str, Any]) -> dict[str, Any]:
    sample_count = policy["sampling"]["retainedConformantSamples"]
    require(len(values) >= sample_count, "statistics require the complete calibration population")
    ordered = sorted(values[:sample_count])
    interval = policy["statistics"]["medianConfidenceInterval95"]
    p95_index = ceil_div(95 * len(ordered), 100) - 1
    return {
        "population": "first-retained-calibration-samples",
        "sampleCount": len(ordered),
        "medianMs": rational_median(ordered),
        "p95NearestRankMs": ordered[p95_index],
        "madMs": rational_mad(ordered),
        "medianConfidenceInterval95Ms": {
            "lowerRank": interval["lowerRank"],
            "upperRank": interval["upperRank"],
            "lowerMs": ordered[interval["lowerRank"] - 1],
            "upperMs": ordered[interval["upperRank"] - 1],
        },
        "minimumMs": ordered[0],
        "maximumMs": ordered[-1],
    }


def trend(values: list[int], policy: dict[str, Any]) -> dict[str, Any]:
    window = policy["trend"]["windowSamples"]
    current_values = values[-window:]
    current_median = rational_median(current_values)
    current_p95 = sorted(current_values)[ceil_div(95 * len(current_values), 100) - 1]
    result: dict[str, Any] = {
        "windowSamples": window,
        "status": "baseline-only" if len(values) < 2 * window else "compared",
        "medianRegressionAlarm": False,
        "p95RegressionAlarm": False,
        "currentMedianMs": current_median,
        "currentP95NearestRankMs": current_p95,
        "priorMedianMs": None,
        "priorP95NearestRankMs": None,
    }
    if len(values) >= 2 * window:
        prior_values = values[-2 * window : -window]
        prior_median = rational_median(prior_values)
        prior_p95 = sorted(prior_values)[ceil_div(95 * len(prior_values), 100) - 1]
        result["priorMedianMs"] = prior_median
        result["priorP95NearestRankMs"] = prior_p95
        median_limit = policy["trend"]["medianRegressionBasisPoints"]
        current_scaled = current_median["numerator"] * prior_median["denominator"]
        prior_scaled = prior_median["numerator"] * current_median["denominator"]
        result["medianRegressionAlarm"] = (
            current_scaled * 10000 > prior_scaled * (10000 + median_limit)
        )
        result["p95RegressionAlarm"] = (
            current_p95 * 10000
            > prior_p95 * (10000 + policy["trend"]["p95RegressionBasisPoints"])
        )
    return result


def ceil_rational_times(value: dict[str, int], multiplier: int) -> int:
    return ceil_div(value["numerator"] * multiplier, value["denominator"])


def derive_hard_ceiling(
    stats: dict[str, Any], class_policy: dict[str, Any], policy: dict[str, Any]
) -> dict[str, Any]:
    derivation = policy["ceilingDerivation"]
    base = stats["p95NearestRankMs"]
    dispersion = ceil_rational_times(stats["madMs"], derivation["madMultiplier"])
    minimum = ceil_div(base * derivation["minimumHeadroomBasisPoints"], 10000)
    headroom = max(dispersion, minimum)
    unrounded = base + headroom
    quantum = derivation["roundUpQuantumMs"]
    derived = ceil_div(unrounded, quantum) * quantum
    return {
        "method": "p95-plus-max-mad-or-proportional-headroom-rounded-up",
        "baseP95NearestRankMs": base,
        "madMultiplier": derivation["madMultiplier"],
        "dispersionHeadroomMs": dispersion,
        "minimumHeadroomBasisPoints": derivation["minimumHeadroomBasisPoints"],
        "minimumHeadroomMs": minimum,
        "appliedHeadroomMs": headroom,
        "unroundedMs": unrounded,
        "roundUpQuantumMs": quantum,
        "derivedMs": derived,
        "withinDeclaredContainmentCeiling": derived <= class_policy["hardCeilingMs"],
    }


def validate_schema() -> None:
    schema = load_json(SCHEMA_PATH)
    require(
        schema.get("$schema") == "https://json-schema.org/draft/2020-12/schema"
        and schema.get("$id")
        == "https://genesiscode.dev/schemas/engineering-gate-timing-calibration-v0.1.json"
        and schema.get("type") == "object"
        and schema.get("additionalProperties") is False,
        "timing calibration schema identity/closure drift",
    )
    require(set(schema.get("required", [])) == EVIDENCE_FIELDS, "timing schema field closure drift")
    classes = schema.get("properties", {}).get("classes", {})
    require(
        classes.get("minItems") == 3
        and classes.get("maxItems") == 3
        and classes.get("items", {}).get("additionalProperties") is False
        and set(classes.get("items", {}).get("required", [])) == CLASS_EVIDENCE_FIELDS,
        "timing schema class closure drift",
    )


def validate_policy(policy: Any) -> dict[str, Any]:
    require(isinstance(policy, dict) and set(policy) == POLICY_FIELDS, "timing policy fields mismatch")
    require(
        policy["kind"] == "genesis/engineering-gate-timing-calibration-policy-v0.1"
        and policy["version"] == "0.1"
        and policy["roadmapTask"] == "R0.4.j",
        "timing policy identity mismatch",
    )
    canonical_path(policy["schemaPath"], "docs/spec/ENGINEERING_GATE_TIMING_CALIBRATION_v0.1.schema.json", "schema")
    canonical_path(policy["evidencePath"], "docs/program/ENGINEERING_GATE_TIMING_CALIBRATION_v0.1.json", "evidence")
    require(
        policy["sampling"]
        == {
            "discardedWarmups": 5,
            "retainedConformantSamples": 30,
            "retainAdditionalConformantSamples": True,
            "retainHardFailures": True,
        },
        "timing sampling contract mismatch",
    )
    require(
        policy["statistics"]
        == {
            "median": "exact-rational",
            "p95": "nearest-rank-ceil",
            "mad": "exact-rational",
            "medianConfidenceInterval95": {"lowerRank": 10, "upperRank": 21},
        },
        "timing statistics contract mismatch",
    )
    require(
        policy["trend"]
        == {
            "windowSamples": 30,
            "medianRegressionBasisPoints": 1000,
            "p95RegressionBasisPoints": 1000,
        },
        "timing trend contract mismatch",
    )
    require(
        policy["ceilingDerivation"]
        == {
            "baseStatistic": "p95-nearest-rank",
            "madMultiplier": 6,
            "minimumHeadroomBasisPoints": 1000,
            "roundUpQuantumMs": 1000,
            "requireWithinDeclaredContainmentCeiling": True,
        },
        "timing ceiling derivation drift",
    )
    require(
        isinstance(policy["classes"], list)
        and [row.get("id") for row in policy["classes"]] == CLASS_IDS,
        "timing classes are not exact and ordered",
    )
    expected_classes = {
        "local-warm": {
            "profile": "prepush-standard",
            "workloadIdentity": "prepush-standard-v1",
            "cachePrecondition": "warm-reusable-root-host-target",
            "hostClass": "declared-local-reference-host",
            "competingLaneState": "exclusive",
            "sourceIdentityKind": "local-observation",
        },
        "local-clean-fallback": {
            "profile": "prepush-standard",
            "workloadIdentity": "prepush-standard-v1",
            "cachePrecondition": "empty-generated-authority-stage-and-declared-fallback-cache",
            "hostClass": "declared-local-reference-host",
            "competingLaneState": "exclusive",
            "sourceIdentityKind": "local-observation",
        },
        "hosted-cold-shared-runner": {
            "profile": "standard",
            "workloadIdentity": "ci-standard-v1",
            "cachePrecondition": "github-hosted-cold-checkout-with-declared-shared-caches",
            "hostClass": "ubuntu-24.04-shared-runner",
            "competingLaneState": "declared-ci-matrix",
            "sourceIdentityKind": "github-actions-run",
        },
    }
    for row in policy["classes"]:
        require(set(row) == CLASS_POLICY_FIELDS, f"{row.get('id')}: timing class fields mismatch")
        expected = expected_classes[row["id"]]
        require(
            all(row[field] == value for field, value in expected.items()),
            f"{row['id']}: timing class identity drift",
        )
        require(row["ceilingStatus"] in {"provisional", "ratified"}, f"{row['id']}: ceiling status invalid")
        require(isinstance(row["hardCeilingMs"], int) and row["hardCeilingMs"] > 0, f"{row['id']}: hard ceiling invalid")
        require(
            all(
                isinstance(row[field], str) and row[field]
                for field in (
                    "profile",
                    "workloadIdentity",
                    "cachePrecondition",
                "hostClass",
                "competingLaneState",
                "sourceIdentityKind",
                )
            ),
            f"{row['id']}: class identity incomplete",
        )
    return policy


def validate_sample(sample: Any, class_policy: dict[str, Any], expected_outcome: str) -> None:
    require(isinstance(sample, dict) and set(sample) == SAMPLE_FIELDS, "timing sample fields mismatch")
    require(isinstance(sample["sequence"], int) and sample["sequence"] > 0, "timing sample sequence invalid")
    require(isinstance(sample["durationMs"], int) and sample["durationMs"] > 0, "timing sample duration invalid")
    require(sample["outcome"] == expected_outcome, "timing sample outcome mismatch")
    for field, width in HEX_FIELDS.items():
        value = sample[field]
        require(
            isinstance(value, str)
            and len(value) == width
            and all(char in "0123456789abcdef" for char in value),
            f"timing sample {field} invalid",
        )
    require(sample["cachePrecondition"] == class_policy["cachePrecondition"], "timing sample cache class relabeled")
    require(sample["competingLaneState"] == class_policy["competingLaneState"], "timing sample competing-lane state mismatch")
    expected_workload = hashlib.sha256(class_policy["workloadIdentity"].encode("utf-8")).hexdigest()
    require(sample["workloadIdentitySha256"] == expected_workload, "timing sample workload identity mismatch")
    source_prefix = f"{class_policy['sourceIdentityKind']}:"
    require(
        isinstance(sample["sourceIdentity"], str)
        and sample["sourceIdentity"].startswith(source_prefix)
        and len(sample["sourceIdentity"]) > len(source_prefix),
        "timing sample source identity missing or wrong for class",
    )


def verify(
    policy_doc: Any | None = None,
    evidence_doc: Any | None = None,
    roadmap_text: str | None = None,
) -> dict[str, Any]:
    validate_schema()
    policy = validate_policy(load_json(POLICY_PATH) if policy_doc is None else policy_doc)
    evidence = load_json(EVIDENCE_PATH) if evidence_doc is None else evidence_doc
    require(isinstance(evidence, dict) and set(evidence) == EVIDENCE_FIELDS, "timing evidence fields mismatch")
    require(
        evidence["kind"] == "genesis/engineering-gate-timing-calibration-evidence-v0.1"
        and evidence["version"] == "0.1",
        "timing evidence identity mismatch",
    )
    require(evidence["status"] in {"collecting", "complete"}, "timing evidence status invalid")
    # Supplied policies are mutation-test inputs. Retained evidence always binds the
    # exact policy bytes on disk, never a reserialized in-memory approximation.
    if policy_doc is None:
        require(evidence["policySha256"] == file_sha256(POLICY_PATH), "timing policy identity mismatch")
    require(evidence["schemaSha256"] == file_sha256(SCHEMA_PATH), "timing schema identity mismatch")
    require(
        isinstance(evidence["classes"], list)
        and [row.get("id") for row in evidence["classes"]] == CLASS_IDS,
        "timing evidence classes mismatch",
    )
    complete = True
    summaries = []
    global_sources: set[str] = set()
    required_warmups = policy["sampling"]["discardedWarmups"]
    required_retained = policy["sampling"]["retainedConformantSamples"]
    for class_policy, row in zip(policy["classes"], evidence["classes"]):
        require(set(row) == CLASS_EVIDENCE_FIELDS, f"{row.get('id')}: timing evidence row fields mismatch")
        for field in ("warmups", "retainedSamples", "failedSamples"):
            require(isinstance(row[field], list), f"{row['id']}: {field} is not an array")
        for sample in row["warmups"]:
            validate_sample(sample, class_policy, "semantic-pass")
        for sample in row["retainedSamples"]:
            validate_sample(sample, class_policy, "semantic-pass")
            require(sample["durationMs"] <= class_policy["hardCeilingMs"], f"{row['id']}: retained sample exceeded hard ceiling")
        for sample in row["failedSamples"]:
            validate_sample(sample, class_policy, "hard-failure")
        all_samples = row["warmups"] + row["retainedSamples"] + row["failedSamples"]
        sequences = [sample["sequence"] for sample in all_samples]
        sources = [sample["sourceIdentity"] for sample in all_samples]
        require(len(sequences) == len(set(sequences)), f"{row['id']}: duplicate sample sequence")
        require(len(sources) == len(set(sources)), f"{row['id']}: duplicate sample source identity")
        for field in ("warmups", "retainedSamples", "failedSamples"):
            field_sequences = [sample["sequence"] for sample in row[field]]
            require(field_sequences == sorted(field_sequences), f"{row['id']}: {field} sequence order drift")
        require(global_sources.isdisjoint(sources), f"{row['id']}: source identity reused across classes")
        global_sources.update(sources)
        class_complete = len(row["warmups"]) == required_warmups and len(row["retainedSamples"]) >= required_retained
        complete = complete and class_complete
        if class_complete:
            expected_statistics = statistics([sample["durationMs"] for sample in row["retainedSamples"]], policy)
            expected_trend = trend([sample["durationMs"] for sample in row["retainedSamples"]], policy)
            expected_ceiling = derive_hard_ceiling(expected_statistics, class_policy, policy)
            require(row["statistics"] == expected_statistics, f"{row['id']}: derived timing statistics drift")
            require(row["trend"] == expected_trend, f"{row['id']}: rolling timing trend drift")
            require(row["derivedHardCeiling"] == expected_ceiling, f"{row['id']}: hard-ceiling derivation drift")
            require(expected_ceiling["withinDeclaredContainmentCeiling"], f"{row['id']}: calibrated ceiling exceeds containment")
            require(class_policy["ceilingStatus"] == "ratified", f"{row['id']}: complete evidence uses provisional ceiling")
            require(class_policy["hardCeilingMs"] == expected_ceiling["derivedMs"], f"{row['id']}: ratified ceiling does not equal calibration")
        else:
            require(
                row["statistics"] is None
                and row["trend"] is None
                and row["derivedHardCeiling"] is None,
                f"{row['id']}: partial evidence claims derived results",
            )
        summaries.append(
            {
                "id": row["id"],
                "warmups": len(row["warmups"]),
                "retained": len(row["retainedSamples"]),
                "failures": len(row["failedSamples"]),
                "ceilingStatus": class_policy["ceilingStatus"],
            }
        )
    require(
        isinstance(evidence["nonclaims"], list)
        and len(evidence["nonclaims"]) == 3
        and all(isinstance(item, str) and item for item in evidence["nonclaims"]),
        "timing evidence nonclaims incomplete",
    )
    require((evidence["status"] == "complete") == complete, "timing evidence completion claim mismatch")
    roadmap = (ROOT / "ROADMAP.md").read_text(encoding="utf-8") if roadmap_text is None else roadmap_text
    task_done = "- [x] **R0.4.j Restore an authentic, hermetic release-full profile within GB-4.**" in roadmap
    require(not task_done or complete, "R0.4.j is checked without complete timing calibration")
    return {"status": evidence["status"], "classes": summaries}


def fixture_sample(sequence: int, duration: int, outcome: str = "semantic-pass") -> dict[str, Any]:
    return {
        "sequence": sequence,
        "durationMs": duration,
        "outcome": outcome,
        "gitCommit": "1" * 40,
        "hostIdentitySha256": "2" * 64,
        "toolchainIdentitySha256": "3" * 64,
        "workloadIdentitySha256": "4" * 64,
        "cachePrecondition": "",
        "competingLaneState": "",
        "sourceIdentity": f"local-observation:fixture-{sequence}",
    }


def complete_fixture(policy: dict[str, Any], evidence: dict[str, Any]) -> tuple[dict[str, Any], dict[str, Any]]:
    policy = copy.deepcopy(policy)
    evidence = copy.deepcopy(evidence)
    evidence["status"] = "complete"
    for class_policy, row in zip(policy["classes"], evidence["classes"]):
        warmups = [fixture_sample(index, 100_000 + index) for index in range(1, 6)]
        retained = [fixture_sample(index, 100_000 + index * 100) for index in range(6, 36)]
        for sample in warmups + retained:
            sample["cachePrecondition"] = class_policy["cachePrecondition"]
            sample["competingLaneState"] = class_policy["competingLaneState"]
            sample["workloadIdentitySha256"] = hashlib.sha256(
                class_policy["workloadIdentity"].encode("utf-8")
            ).hexdigest()
            sample["sourceIdentity"] = (
                f"{class_policy['sourceIdentityKind']}:{row['id']}-{sample['sequence']}"
            )
        row["warmups"] = warmups
        row["retainedSamples"] = retained
        stats = statistics([sample["durationMs"] for sample in retained], policy)
        ceiling = derive_hard_ceiling(stats, class_policy, policy)
        class_policy["ceilingStatus"] = "ratified"
        class_policy["hardCeilingMs"] = ceiling["derivedMs"]
        ceiling = derive_hard_ceiling(stats, class_policy, policy)
        row["statistics"] = stats
        row["trend"] = trend([sample["durationMs"] for sample in retained], policy)
        row["derivedHardCeiling"] = ceiling
    return policy, evidence


def expect_rejection(policy: Any, evidence: Any, roadmap: str | None = None) -> None:
    try:
        verify(policy, evidence, roadmap)
    except TimingCalibrationError:
        return
    raise TimingCalibrationError("timing calibration negative control was accepted")


def self_test() -> int:
    policy = load_json(POLICY_PATH)
    evidence = load_json(EVIDENCE_PATH)
    complete_policy, complete_evidence = complete_fixture(policy, evidence)
    verify(complete_policy, complete_evidence, "")
    controls = 1

    candidate = copy.deepcopy(policy)
    candidate["sampling"]["retainedConformantSamples"] = 29
    expect_rejection(candidate, evidence)
    controls += 1

    candidate = copy.deepcopy(evidence)
    candidate["classes"][0]["id"] = "local-clean-fallback"
    expect_rejection(policy, candidate)
    controls += 1

    candidate = copy.deepcopy(evidence)
    candidate["classes"][0]["statistics"] = {}
    expect_rejection(policy, candidate)
    controls += 1

    expect_rejection(
        policy,
        evidence,
        "- [x] **R0.4.j Restore an authentic, hermetic release-full profile within GB-4.**",
    )
    controls += 1

    candidate = copy.deepcopy(complete_evidence)
    candidate["classes"][0]["retainedSamples"][0]["cachePrecondition"] = "warm"
    expect_rejection(complete_policy, candidate, "")
    controls += 1

    candidate = copy.deepcopy(complete_evidence)
    candidate["classes"][0]["retainedSamples"][1]["sequence"] = 6
    expect_rejection(complete_policy, candidate, "")
    controls += 1

    interleaved = copy.deepcopy(complete_evidence)
    for row in interleaved["classes"]:
        for sample in row["warmups"] + row["retainedSamples"]:
            sample["sequence"] *= 2
    failure = fixture_sample(9, 101_000, "hard-failure")
    failure["cachePrecondition"] = complete_policy["classes"][0]["cachePrecondition"]
    failure["competingLaneState"] = complete_policy["classes"][0]["competingLaneState"]
    failure["workloadIdentitySha256"] = hashlib.sha256(
        complete_policy["classes"][0]["workloadIdentity"].encode("utf-8")
    ).hexdigest()
    failure["sourceIdentity"] = "local-observation:interleaved-failure"
    interleaved["classes"][0]["failedSamples"] = [failure]
    verify(complete_policy, interleaved, "")
    controls += 1

    candidate = copy.deepcopy(complete_evidence)
    candidate["classes"][1]["retainedSamples"][0]["sourceIdentity"] = (
        candidate["classes"][0]["retainedSamples"][0]["sourceIdentity"]
    )
    expect_rejection(complete_policy, candidate, "")
    controls += 1

    candidate = copy.deepcopy(complete_evidence)
    candidate["classes"][0]["retainedSamples"][0]["workloadIdentitySha256"] = "f" * 64
    expect_rejection(complete_policy, candidate, "")
    controls += 1

    candidate = copy.deepcopy(complete_evidence)
    candidate["classes"][0]["statistics"]["medianMs"] = {"numerator": 1, "denominator": 1}
    expect_rejection(complete_policy, candidate, "")
    controls += 1

    candidate_policy = copy.deepcopy(complete_policy)
    candidate_policy["classes"][0]["ceilingStatus"] = "provisional"
    expect_rejection(candidate_policy, complete_evidence, "")
    controls += 1

    candidate_policy = copy.deepcopy(complete_policy)
    candidate_policy["classes"][0]["hardCeilingMs"] += 1000
    expect_rejection(candidate_policy, complete_evidence, "")
    controls += 1

    require(rational_mad([0, 1]) == {"numerator": 1, "denominator": 2}, "exact MAD regression")
    controls += 1
    return controls


if __name__ == "__main__":
    try:
        summary = verify()
        controls = self_test()
        print(
            "engineering-gate-timing-calibration: ok "
            f"(status={summary['status']} classes={len(summary['classes'])} controls={controls})"
        )
    except (TimingCalibrationError, OSError, UnicodeError) as exc:
        print(f"engineering-gate-timing-calibration: {exc}")
        raise SystemExit(1)
