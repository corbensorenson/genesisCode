#!/usr/bin/env python3
"""Validate class-separated engineering timing calibration evidence."""

from __future__ import annotations

import copy
import hashlib
import json
import math
from pathlib import Path
from typing import Any, Optional

import reference_host_profiles


ROOT = Path(__file__).resolve().parents[2]
POLICY_PATH = ROOT / "policies/engineering_gate_timing_calibration_v0.1.json"
EVIDENCE_PATH = ROOT / "docs/program/ENGINEERING_GATE_TIMING_CALIBRATION_v0.1.json"
SCHEMA_PATH = ROOT / "docs/spec/ENGINEERING_GATE_TIMING_CALIBRATION_v0.1.schema.json"
OBSERVATION_SCHEMA_PATH = ROOT / "docs/spec/ENGINEERING_GATE_TIMING_OBSERVATION_v0.1.schema.json"
CLASS_IDS = ["local-warm", "local-clean-fallback", "hosted-cold-shared-runner"]
HEX_FIELDS = {
    "gitCommit": 40,
    "hostIdentitySha256": 64,
    "toolchainIdentitySha256": 64,
    "workloadIdentitySha256": 64,
    "observationIdentitySha256": 64,
    "previousObservationSha256": 64,
}
SAMPLE_FIELDS = {
    "sequence",
    "observedAtUnixSeconds",
    "durationMs",
    "outcome",
    "failureKind",
    "exitCode",
    "cleanupStatus",
    *HEX_FIELDS,
    "hostObservationCanonicalJson",
    "toolchainIdentityCanonicalJson",
    "controlObservationCanonicalJson",
    "cachePrecondition",
    "competingLaneState",
    "sourceIdentity",
    "chainScope",
}
POLICY_FIELDS = {
    "kind",
    "version",
    "roadmapTask",
    "schemaPath",
    "observationSchemaPath",
    "evidencePath",
    "observationHistoryPath",
    "sampling",
    "statistics",
    "trend",
    "ceilingDerivation",
    "workloads",
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
WORKLOAD_FIELDS = {
    "id",
    "measurement",
    "command",
    "arguments",
    "changedFiles",
    "runner",
    "budgetMs",
    "networkMode",
}
OBSERVATION_FIELDS = {
    "kind",
    "version",
    "classId",
    "observedAtUnixSeconds",
    "durationMs",
    "outcome",
    "failureKind",
    "exitCode",
    "cleanupStatus",
    "gitCommit",
    "hostObservationCanonicalJson",
    "hostIdentitySha256",
    "toolchainIdentityCanonicalJson",
    "toolchainIdentitySha256",
    "controlObservationCanonicalJson",
    "workloadIdentitySha256",
    "cachePrecondition",
    "competingLaneState",
    "sourceIdentity",
    "chainScope",
    "previousObservationSha256",
    "identitySha256",
}
TOOLCHAIN_FIELDS = {
    "bash",
    "cargo",
    "node",
    "python",
    "runner",
    "runnerImage",
    "runnerVersion",
    "rustc",
}
CONTROL_FIELDS = {
    "backgroundLoadBasisPoints",
    "backgroundLoadLimitBasisPoints",
    "cacheState",
    "competingLaneState",
    "competingProcessCount",
    "exactRevision",
    "referenceHostConformant",
    "source",
    "thermalState",
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


def canonical_bytes(value: Any) -> bytes:
    return (
        json.dumps(value, sort_keys=True, separators=(",", ":"), ensure_ascii=True)
        + "\n"
    ).encode("ascii")


def canonical_sha256(value: Any) -> str:
    return hashlib.sha256(canonical_bytes(value)).hexdigest()


def parse_canonical_json_text(raw: Any, field: str) -> Any:
    require(isinstance(raw, str) and raw, f"timing sample {field} is empty")
    try:
        value = json.loads(raw, object_pairs_hook=unique_pairs)
    except (json.JSONDecodeError, TimingCalibrationError) as exc:
        raise TimingCalibrationError(f"timing sample {field} is invalid JSON: {exc}") from exc
    require(
        (raw + "\n").encode("ascii") == canonical_bytes(value),
        f"timing sample {field} is not canonical JSON",
    )
    return value


def validate_toolchain_observation(document: Any) -> dict[str, Any]:
    require(
        isinstance(document, dict) and set(document) == TOOLCHAIN_FIELDS,
        "timing toolchain observation fields mismatch",
    )
    for field in TOOLCHAIN_FIELDS - {"node"}:
        require(
            isinstance(document[field], str) and document[field],
            f"timing toolchain {field} is empty",
        )
    require(
        document["node"] is None
        or (isinstance(document["node"], str) and bool(document["node"])),
        "timing toolchain node identity is invalid",
    )
    require(
        document["runner"] in {"cargo", "nextest", "github-actions"},
        "unknown timing runner",
    )
    return document


def validate_control_observation(document: Any, class_id: str) -> dict[str, Any]:
    require(
        isinstance(document, dict) and set(document) == CONTROL_FIELDS,
        "timing control observation fields mismatch",
    )
    require(
        document["source"] in {"local-preflight", "github-actions-context"},
        "timing control source invalid",
    )
    require(document["exactRevision"] is True, "timing observation is not exact-revision")
    require(
        isinstance(document["referenceHostConformant"], bool),
        "timing host conformance fact invalid",
    )
    require(
        document["thermalState"] in {"nominal", "unknown"},
        "timing thermal state invalid",
    )
    require(
        isinstance(document["competingProcessCount"], int)
        and not isinstance(document["competingProcessCount"], bool)
        and document["competingProcessCount"] >= 0,
        "timing competing process count invalid",
    )
    if class_id.startswith("local-"):
        require(document["source"] == "local-preflight", "local timing class lacks local preflight")
        require(document["referenceHostConformant"] is True, "local timing host is not conformant")
        require(document["thermalState"] == "nominal", "local timing thermal state is not nominal")
        require(document["competingProcessCount"] == 0, "local timing host is not exclusive")
        require(
            isinstance(document["backgroundLoadBasisPoints"], int)
            and not isinstance(document["backgroundLoadBasisPoints"], bool)
            and isinstance(document["backgroundLoadLimitBasisPoints"], int)
            and not isinstance(document["backgroundLoadLimitBasisPoints"], bool)
            and document["backgroundLoadBasisPoints"]
            <= document["backgroundLoadLimitBasisPoints"],
            "local timing background load exceeds its declared limit",
        )
    else:
        require(
            document["source"] == "github-actions-context",
            "hosted timing class lacks CI context",
        )
        require(
            document["backgroundLoadBasisPoints"] is None
            and document["backgroundLoadLimitBasisPoints"] is None,
            "hosted shared-runner load must remain unclaimed",
        )
    return document


def reconstruct_observation(sample: dict[str, Any], class_id: str) -> dict[str, Any]:
    return {
        "kind": "genesis/engineering-gate-timing-observation-v0.1",
        "version": "0.1",
        "classId": class_id,
        "observedAtUnixSeconds": sample["observedAtUnixSeconds"],
        "durationMs": sample["durationMs"],
        "outcome": sample["outcome"],
        "failureKind": sample["failureKind"],
        "exitCode": sample["exitCode"],
        "cleanupStatus": sample["cleanupStatus"],
        "gitCommit": sample["gitCommit"],
        "hostObservationCanonicalJson": sample["hostObservationCanonicalJson"],
        "hostIdentitySha256": sample["hostIdentitySha256"],
        "toolchainIdentityCanonicalJson": sample["toolchainIdentityCanonicalJson"],
        "toolchainIdentitySha256": sample["toolchainIdentitySha256"],
        "controlObservationCanonicalJson": sample["controlObservationCanonicalJson"],
        "workloadIdentitySha256": sample["workloadIdentitySha256"],
        "cachePrecondition": sample["cachePrecondition"],
        "competingLaneState": sample["competingLaneState"],
        "sourceIdentity": sample["sourceIdentity"],
        "chainScope": sample["chainScope"],
        "previousObservationSha256": sample["previousObservationSha256"],
        "identitySha256": sample["observationIdentitySha256"],
    }


def validate_append_only_chain(
    records: list[dict[str, Any]], identity_field: str, label: str
) -> None:
    local = [row for row in records if row["chainScope"] == "append-only-local"]
    if not local:
        return
    by_identity = {row[identity_field]: row for row in local}
    require(len(by_identity) == len(local), f"{label}: duplicate local observation identity")
    successors: dict[str, str] = {}
    for row in local:
        predecessor = row["previousObservationSha256"]
        require(
            predecessor == "0" * 64 or predecessor in by_identity,
            f"{label}: local observation predecessor is absent",
        )
        require(
            predecessor not in successors,
            f"{label}: local observation chain forks",
        )
        successors[predecessor] = row[identity_field]
    ordered: list[dict[str, Any]] = []
    current = "0" * 64
    while current in successors:
        current = successors[current]
        require(
            len(ordered) < len(local),
            f"{label}: local observation chain cycles",
        )
        ordered.append(by_identity[current])
    require(
        len(ordered) == len(local),
        f"{label}: local observation chain is disconnected",
    )
    require(
        [row["observedAtUnixSeconds"] for row in ordered]
        == sorted(row["observedAtUnixSeconds"] for row in ordered),
        f"{label}: local observation chain chronology moved backward",
    )


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
    sample = schema.get("$defs", {}).get("sample", {})
    require(
        sample.get("additionalProperties") is False
        and set(sample.get("required", [])) == SAMPLE_FIELDS
        and set(sample.get("properties", {})) == SAMPLE_FIELDS,
        "timing schema sample closure drift",
    )
    observation_schema = load_json(OBSERVATION_SCHEMA_PATH)
    require(
        observation_schema.get("$schema") == "https://json-schema.org/draft/2020-12/schema"
        and observation_schema.get("$id")
        == "https://genesiscode.dev/schemas/engineering-gate-timing-observation-v0.1.json"
        and observation_schema.get("type") == "object"
        and observation_schema.get("additionalProperties") is False
        and set(observation_schema.get("required", [])) == OBSERVATION_FIELDS
        and set(observation_schema.get("properties", {})) == OBSERVATION_FIELDS,
        "timing observation schema identity/closure drift",
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
    canonical_path(
        policy["observationSchemaPath"],
        "docs/spec/ENGINEERING_GATE_TIMING_OBSERVATION_v0.1.schema.json",
        "observation schema",
    )
    canonical_path(policy["evidencePath"], "docs/program/ENGINEERING_GATE_TIMING_CALIBRATION_v0.1.json", "evidence")
    require(
        policy["observationHistoryPath"]
        == ".genesis/perf/engineering_gate_timing_observations_v0.1.jsonl",
        "timing observation history path drift",
    )
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
    expected_workloads = [
        {
            "id": "prepush-standard-v1",
            "measurement": "monotonic-process-wall",
            "command": "scripts/test_changed_fast.sh",
            "arguments": [
                "--base",
                "HEAD",
                "--runner",
                "cargo",
                "--min-history",
                "1",
                "--changed-files-from",
                "<collector-owned>",
                "--report",
                "<collector-owned>",
                "--history",
                "<collector-owned>",
            ],
            "changedFiles": ["policies/engineering_gate_budgets_v0.1.json"],
            "runner": "cargo",
            "budgetMs": 720000,
            "networkMode": "deny",
        },
        {
            "id": "ci-standard-v1",
            "measurement": "github-actions-job-wall",
            "command": ".github/workflows/ci.yml#test_suite",
            "arguments": [
                "profile=standard",
                "lane=standard",
                "start=Begin Hosted Timing Calibration",
                "end=Finalize Hosted Timing Calibration",
            ],
            "changedFiles": [],
            "runner": "github-actions",
            "budgetMs": 7200000,
            "networkMode": "declared-ci-dependency-network",
        },
    ]
    require(policy["workloads"] == expected_workloads, "timing workload contracts drift")
    require(
        all(set(row) == WORKLOAD_FIELDS for row in policy["workloads"]),
        "timing workload fields mismatch",
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
        workload = next(
            (item for item in policy["workloads"] if item["id"] == row["workloadIdentity"]),
            None,
        )
        require(workload is not None, f"{row['id']}: unknown workload identity")
        require(
            row["hardCeilingMs"] <= workload["budgetMs"],
            f"{row['id']}: hard ceiling exceeds workload containment budget",
        )
    return policy


def validate_sample(
    sample: Any,
    class_policy: dict[str, Any],
    expected_outcome: str,
    policy: dict[str, Any],
) -> None:
    require(isinstance(sample, dict) and set(sample) == SAMPLE_FIELDS, "timing sample fields mismatch")
    require(isinstance(sample["sequence"], int) and sample["sequence"] > 0, "timing sample sequence invalid")
    require(
        isinstance(sample["observedAtUnixSeconds"], int)
        and sample["observedAtUnixSeconds"] > 0,
        "timing sample observation time invalid",
    )
    require(isinstance(sample["durationMs"], int) and sample["durationMs"] > 0, "timing sample duration invalid")
    require(sample["outcome"] == expected_outcome, "timing sample outcome mismatch")
    if expected_outcome == "semantic-pass":
        require(
            sample["failureKind"] is None
            and sample["exitCode"] == 0
            and sample["cleanupStatus"] == "reaped",
            "timing semantic-pass terminal fields mismatch",
        )
    else:
        require(
            sample["failureKind"]
            in {
                "command-failure",
                "hard-timeout",
                "infrastructure-failure",
                "telemetry-budget",
                "interrupted",
            }
            and (
                sample["exitCode"] is None
                or (
                    isinstance(sample["exitCode"], int)
                    and not isinstance(sample["exitCode"], bool)
                )
            )
            and sample["cleanupStatus"] in {"reaped", "containment-failure"},
            "timing hard-failure terminal fields mismatch",
        )
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
    workloads = {row["id"]: row for row in policy["workloads"]}
    expected_workload = canonical_sha256(workloads[class_policy["workloadIdentity"]])
    require(sample["workloadIdentitySha256"] == expected_workload, "timing sample workload identity mismatch")
    source_prefix = f"{class_policy['sourceIdentityKind']}:"
    require(
        isinstance(sample["sourceIdentity"], str)
        and sample["sourceIdentity"].startswith(source_prefix)
        and len(sample["sourceIdentity"]) > len(source_prefix),
        "timing sample source identity missing or wrong for class",
    )
    host = parse_canonical_json_text(
        sample["hostObservationCanonicalJson"], "hostObservationCanonicalJson"
    )
    try:
        host_policy = reference_host_profiles.validate_policy(
            reference_host_profiles.load_json(reference_host_profiles.POLICY)
        )
        reference_host_profiles.validate_observation(host, host_policy)
    except reference_host_profiles.HostProfileError as exc:
        raise TimingCalibrationError(
            f"timing sample host observation is invalid: {exc}"
        ) from exc
    require(
        isinstance(host, dict)
        and host.get("identitySha256") == sample["hostIdentitySha256"],
        "timing sample host observation identity mismatch",
    )
    toolchain = validate_toolchain_observation(
        parse_canonical_json_text(
            sample["toolchainIdentityCanonicalJson"],
            "toolchainIdentityCanonicalJson",
        )
    )
    require(
        canonical_sha256(toolchain) == sample["toolchainIdentitySha256"],
        "timing sample toolchain observation identity mismatch",
    )
    control = validate_control_observation(
        parse_canonical_json_text(
            sample["controlObservationCanonicalJson"],
            "controlObservationCanonicalJson",
        ),
        class_policy["id"],
    )
    require(
        control["referenceHostConformant"] == host["conformance"]["ok"],
        "timing sample host-conformance claim mismatches its host observation",
    )
    require(
        control["cacheState"] == class_policy["cachePrecondition"]
        and control["competingLaneState"] == class_policy["competingLaneState"],
        "timing sample control observation does not prove its class",
    )
    expected_chain_scope = (
        "append-only-local"
        if class_policy["sourceIdentityKind"] == "local-observation"
        else "standalone-hosted"
    )
    require(sample["chainScope"] == expected_chain_scope, "timing sample chain scope mismatch")
    if expected_chain_scope == "standalone-hosted":
        require(
            sample["previousObservationSha256"] == "0" * 64,
            "hosted timing sample claims a local history predecessor",
        )
        require(
            host["platformId"] == "linux-x86-64"
            and host["metadata"]["operatingSystem"]["family"] == "linux"
            and toolchain["runner"] == "github-actions"
            and toolchain["runnerImage"].startswith("ubuntu24/"),
            "hosted timing sample does not bind the declared shared runner",
        )
    else:
        require(
            toolchain["runner"] == workloads[class_policy["workloadIdentity"]]["runner"],
            "local timing sample runner does not match its workload",
        )
    observation = reconstruct_observation(sample, class_policy["id"])
    require(
        canonical_sha256(
            {key: value for key, value in observation.items() if key != "identitySha256"}
        )
        == sample["observationIdentitySha256"],
        "timing sample observation content identity mismatch",
    )


def verify(
    policy_doc: Optional[Any] = None,
    evidence_doc: Optional[Any] = None,
    roadmap_text: Optional[str] = None,
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
    global_observations: set[str] = set()
    all_evidence_samples: list[dict[str, Any]] = []
    required_warmups = policy["sampling"]["discardedWarmups"]
    required_retained = policy["sampling"]["retainedConformantSamples"]
    for class_policy, row in zip(policy["classes"], evidence["classes"]):
        require(set(row) == CLASS_EVIDENCE_FIELDS, f"{row.get('id')}: timing evidence row fields mismatch")
        for field in ("warmups", "retainedSamples", "failedSamples"):
            require(isinstance(row[field], list), f"{row['id']}: {field} is not an array")
        for sample in row["warmups"]:
            validate_sample(sample, class_policy, "semantic-pass", policy)
        for sample in row["retainedSamples"]:
            validate_sample(sample, class_policy, "semantic-pass", policy)
            require(sample["durationMs"] <= class_policy["hardCeilingMs"], f"{row['id']}: retained sample exceeded hard ceiling")
        for sample in row["failedSamples"]:
            validate_sample(sample, class_policy, "hard-failure", policy)
        all_samples = row["warmups"] + row["retainedSamples"] + row["failedSamples"]
        sequences = [sample["sequence"] for sample in all_samples]
        sources = [sample["sourceIdentity"] for sample in all_samples]
        require(len(sequences) == len(set(sequences)), f"{row['id']}: duplicate sample sequence")
        require(
            sorted(sequences) == list(range(1, len(all_samples) + 1)),
            f"{row['id']}: sample chronology is not contiguous",
        )
        require(len(sources) == len(set(sources)), f"{row['id']}: duplicate sample source identity")
        for field in ("warmups", "retainedSamples", "failedSamples"):
            field_sequences = [sample["sequence"] for sample in row[field]]
            require(field_sequences == sorted(field_sequences), f"{row['id']}: {field} sequence order drift")
        chronological = sorted(all_samples, key=lambda sample: sample["sequence"])
        require(
            [sample["observedAtUnixSeconds"] for sample in chronological]
            == sorted(sample["observedAtUnixSeconds"] for sample in chronological),
            f"{row['id']}: observation chronology moved backward",
        )
        semantic_passes = [
            sample for sample in chronological if sample["outcome"] == "semantic-pass"
        ]
        require(
            row["warmups"] == semantic_passes[:required_warmups]
            and row["retainedSamples"] == semantic_passes[required_warmups:],
            f"{row['id']}: warmup/retained role assignment drift",
        )
        require(global_sources.isdisjoint(sources), f"{row['id']}: source identity reused across classes")
        observations = [sample["observationIdentitySha256"] for sample in all_samples]
        require(
            len(observations) == len(set(observations))
            and global_observations.isdisjoint(observations),
            f"{row['id']}: observation identity reused",
        )
        global_sources.update(sources)
        global_observations.update(observations)
        all_evidence_samples.extend(all_samples)
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
    validate_append_only_chain(
        all_evidence_samples,
        "observationIdentitySha256",
        "timing evidence",
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
    host = {"identitySha256": "2" * 64}
    toolchain = {"fixture": True}
    return {
        "sequence": sequence,
        "observedAtUnixSeconds": 1_800_000_000 + sequence,
        "durationMs": duration,
        "outcome": outcome,
        "failureKind": None if outcome == "semantic-pass" else "command-failure",
        "exitCode": 0 if outcome == "semantic-pass" else 1,
        "cleanupStatus": "reaped",
        "gitCommit": "1" * 40,
        "hostObservationCanonicalJson": canonical_bytes(host).decode("ascii").removesuffix("\n"),
        "hostIdentitySha256": "2" * 64,
        "toolchainIdentityCanonicalJson": canonical_bytes(toolchain).decode("ascii").removesuffix("\n"),
        "toolchainIdentitySha256": canonical_sha256(toolchain),
        "controlObservationCanonicalJson": canonical_bytes({"fixture": True}).decode("ascii").removesuffix("\n"),
        "workloadIdentitySha256": "4" * 64,
        "observationIdentitySha256": "5" * 64,
        "cachePrecondition": "",
        "competingLaneState": "",
        "sourceIdentity": f"local-observation:fixture-{sequence}",
        "chainScope": "append-only-local",
        "previousObservationSha256": "0" * 64,
    }


def fixture_environment(
    class_policy: dict[str, Any], workload: dict[str, Any]
) -> tuple[dict[str, Any], dict[str, Any], dict[str, Any]]:
    host_policy = reference_host_profiles.validate_policy(
        reference_host_profiles.load_json(reference_host_profiles.POLICY)
    )
    platform_id = (
        "linux-x86-64"
        if class_policy["sourceIdentityKind"] == "github-actions-run"
        else "darwin-arm64"
    )
    profile = reference_host_profiles.profile_map(host_policy)[platform_id]
    host = dict(reference_host_profiles.synthetic_observation(profile))
    runner = workload["runner"]
    toolchain = {
        "bash": "GNU bash 5.2",
        "cargo": "cargo 1.90.0",
        "node": "v22.23.2",
        "python": "3.12.0",
        "runner": runner,
        "runnerImage": (
            "ubuntu24/fixture" if runner == "github-actions" else "local"
        ),
        "runnerVersion": "fixture",
        "rustc": "rustc 1.90.0\nrelease: 1.90.0\nhost: fixture",
    }
    local = class_policy["sourceIdentityKind"] == "local-observation"
    control = {
        "backgroundLoadBasisPoints": 100 if local else None,
        "backgroundLoadLimitBasisPoints": 500 if local else None,
        "cacheState": class_policy["cachePrecondition"],
        "competingLaneState": class_policy["competingLaneState"],
        "competingProcessCount": 0,
        "exactRevision": True,
        "referenceHostConformant": host["conformance"]["ok"],
        "source": "local-preflight" if local else "github-actions-context",
        "thermalState": "nominal" if local else "unknown",
    }
    return host, toolchain, control


def complete_fixture(policy: dict[str, Any], evidence: dict[str, Any]) -> tuple[dict[str, Any], dict[str, Any]]:
    policy = copy.deepcopy(policy)
    evidence = copy.deepcopy(evidence)
    evidence["status"] = "complete"
    previous_local = "0" * 64
    observed_at = 1_800_000_000
    for class_policy, row in zip(policy["classes"], evidence["classes"]):
        workload = next(
            item
            for item in policy["workloads"]
            if item["id"] == class_policy["workloadIdentity"]
        )
        host, toolchain, control = fixture_environment(class_policy, workload)
        warmups = [fixture_sample(index, 100_000 + index) for index in range(1, 6)]
        retained = [fixture_sample(index, 100_000 + index * 100) for index in range(6, 36)]
        for sample in warmups + retained:
            observed_at += 1
            sample["observedAtUnixSeconds"] = observed_at
            sample["cachePrecondition"] = class_policy["cachePrecondition"]
            sample["competingLaneState"] = class_policy["competingLaneState"]
            sample["hostObservationCanonicalJson"] = canonical_bytes(host).decode("ascii").removesuffix("\n")
            sample["hostIdentitySha256"] = host["identitySha256"]
            sample["toolchainIdentityCanonicalJson"] = canonical_bytes(toolchain).decode("ascii").removesuffix("\n")
            sample["toolchainIdentitySha256"] = canonical_sha256(toolchain)
            sample["controlObservationCanonicalJson"] = canonical_bytes(control).decode("ascii").removesuffix("\n")
            sample["workloadIdentitySha256"] = canonical_sha256(workload)
            sample["sourceIdentity"] = (
                f"{class_policy['sourceIdentityKind']}:{row['id']}-{sample['sequence']}"
            )
            sample["chainScope"] = (
                "append-only-local"
                if class_policy["sourceIdentityKind"] == "local-observation"
                else "standalone-hosted"
            )
            sample["previousObservationSha256"] = (
                previous_local
                if sample["chainScope"] == "append-only-local"
                else "0" * 64
            )
            sample["observationIdentitySha256"] = canonical_sha256(
                {
                    key: value
                    for key, value in reconstruct_observation(sample, row["id"]).items()
                    if key != "identitySha256"
                }
            )
            if sample["chainScope"] == "append-only-local":
                previous_local = sample["observationIdentitySha256"]
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


def expect_rejection(policy: Any, evidence: Any, roadmap: Optional[str] = None) -> None:
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
    for sample in interleaved["classes"][0]["warmups"] + interleaved["classes"][0]["retainedSamples"]:
        if sample["sequence"] >= 4:
            sample["sequence"] += 1
    failure = copy.deepcopy(interleaved["classes"][0]["warmups"][3])
    failure["sequence"] = 4
    failure["durationMs"] = 101_000
    failure["outcome"] = "hard-failure"
    failure["failureKind"] = "command-failure"
    failure["exitCode"] = 1
    failure["sourceIdentity"] = "local-observation:interleaved-failure"
    interleaved["classes"][0]["failedSamples"] = [failure]
    local_fixture_samples = []
    for fixture_row in interleaved["classes"][:2]:
        for sample in (
            fixture_row["warmups"]
            + fixture_row["retainedSamples"]
            + fixture_row["failedSamples"]
        ):
            local_fixture_samples.append((fixture_row["id"], sample))
    previous_local = "0" * 64
    for fixture_class_id, sample in sorted(
        local_fixture_samples,
        key=lambda item: (
            item[1]["observedAtUnixSeconds"],
            item[1]["sourceIdentity"],
        ),
    ):
        sample["previousObservationSha256"] = previous_local
        sample["observationIdentitySha256"] = canonical_sha256(
            {
                key: value
                for key, value in reconstruct_observation(sample, fixture_class_id).items()
                if key != "identitySha256"
            }
        )
        previous_local = sample["observationIdentitySha256"]
    verify(complete_policy, interleaved, "")
    controls += 1

    candidate = copy.deepcopy(complete_evidence)
    sample = candidate["classes"][1]["retainedSamples"][-1]
    sample["previousObservationSha256"] = "0" * 64
    sample["observationIdentitySha256"] = canonical_sha256(
        {
            key: value
            for key, value in reconstruct_observation(sample, "local-clean-fallback").items()
            if key != "identitySha256"
        }
    )
    expect_rejection(complete_policy, candidate, "")
    controls += 1

    candidate = copy.deepcopy(complete_evidence)
    displaced_warmup = candidate["classes"][0]["warmups"].pop()
    candidate["classes"][0]["retainedSamples"].insert(0, displaced_warmup)
    expect_rejection(complete_policy, candidate, "")
    controls += 1

    candidate = copy.deepcopy(complete_evidence)
    sample = candidate["classes"][0]["retainedSamples"][0]
    sample["hostObservationCanonicalJson"] = canonical_bytes(
        {"identitySha256": "f" * 64}
    ).decode("ascii").removesuffix("\n")
    sample["observationIdentitySha256"] = canonical_sha256(
        {
            key: value
            for key, value in reconstruct_observation(sample, "local-warm").items()
            if key != "identitySha256"
        }
    )
    expect_rejection(complete_policy, candidate, "")
    controls += 1

    candidate = copy.deepcopy(complete_evidence)
    sample = candidate["classes"][2]["retainedSamples"][0]
    toolchain = parse_canonical_json_text(
        sample["toolchainIdentityCanonicalJson"],
        "toolchainIdentityCanonicalJson",
    )
    toolchain["runner"] = "cargo"
    sample["toolchainIdentityCanonicalJson"] = canonical_bytes(toolchain).decode(
        "ascii"
    ).removesuffix("\n")
    sample["toolchainIdentitySha256"] = canonical_sha256(toolchain)
    sample["observationIdentitySha256"] = canonical_sha256(
        {
            key: value
            for key, value in reconstruct_observation(
                sample, "hosted-cold-shared-runner"
            ).items()
            if key != "identitySha256"
        }
    )
    expect_rejection(complete_policy, candidate, "")
    controls += 1

    candidate = copy.deepcopy(complete_evidence)
    sample = candidate["classes"][2]["retainedSamples"][0]
    control = parse_canonical_json_text(
        sample["controlObservationCanonicalJson"],
        "controlObservationCanonicalJson",
    )
    control["referenceHostConformant"] = False
    sample["controlObservationCanonicalJson"] = canonical_bytes(control).decode(
        "ascii"
    ).removesuffix("\n")
    sample["observationIdentitySha256"] = canonical_sha256(
        {
            key: value
            for key, value in reconstruct_observation(
                sample, "hosted-cold-shared-runner"
            ).items()
            if key != "identitySha256"
        }
    )
    expect_rejection(complete_policy, candidate, "")
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
