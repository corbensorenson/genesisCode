#!/usr/bin/env python3
"""Validate the content-addressed PB-1 through PB-10 workload authority."""

from __future__ import annotations

import argparse
import copy
from hashlib import sha256
import json
from pathlib import Path, PurePosixPath
import re
import sys
from typing import Any, Dict, Iterable, Mapping, Optional, Sequence, Tuple


ROOT = Path(__file__).resolve().parents[2]
POLICY = ROOT / "policies/perf/roadmap_workloads_v0.1.json"
SCHEMA = ROOT / "docs/spec/ROADMAP_WORKLOADS_v0.1.schema.json"
REFERENCE_HOSTS = ROOT / "policies/reference_host_profiles_v0.1.json"
WORKLOAD_IDS = [f"PB-{index}" for index in range(1, 11)]
TOP_FIELDS = {
    "identityAlgorithm", "inputHashAlgorithm", "kind", "protocols",
    "referenceHostPolicy", "roadmapTask", "version", "workloads",
}
WORKLOAD_FIELDS = {
    "expected", "id", "inputs", "measurement", "runner", "sizes", "status", "target"
}
PROTOCOL_FIELDS = {
    "confidence", "decisionStatistic", "dispersion", "failurePolicy", "id", "integerRounding",
    "outlierPolicy", "primary", "secondary", "sampleOrdering",
}
INPUT_FIELDS = {"bytes", "path", "role", "sha256"}
MEASUREMENT_FIELDS = {
    "cacheState", "protocolId", "sampleCount", "sampleUnit", "timeoutMs", "warmup"
}
SHA_RE = re.compile(r"^[0-9a-f]{64}$")
TOKEN_RE = re.compile(r"^[a-z][a-z0-9-]+$")


class WorkloadError(ValueError):
    pass


def reject_duplicate_keys(pairs: Sequence[Tuple[str, Any]]) -> Dict[str, Any]:
    result: Dict[str, Any] = {}
    for key, value in pairs:
        if key in result:
            raise WorkloadError(f"duplicate JSON key: {key}")
        result[key] = value
    return result


def load_json(path: Path) -> Any:
    try:
        return json.loads(path.read_text(encoding="utf-8"), object_pairs_hook=reject_duplicate_keys)
    except (OSError, UnicodeError, json.JSONDecodeError) as exc:
        raise WorkloadError(f"cannot load {path}: {exc}") from exc


def canonical_bytes(value: Any) -> bytes:
    return (json.dumps(value, ensure_ascii=True, separators=(",", ":"), sort_keys=True) + "\n").encode("ascii")


def require_object(value: Any, fields: Iterable[str], label: str) -> Mapping[str, Any]:
    expected = set(fields)
    if not isinstance(value, dict) or set(value) != expected:
        observed = sorted(value) if isinstance(value, dict) else type(value).__name__
        raise WorkloadError(f"{label} fields mismatch: {observed}")
    return value


def require_integer(value: Any, label: str, minimum: int = 1) -> int:
    if not isinstance(value, int) or isinstance(value, bool) or value < minimum:
        raise WorkloadError(f"{label} must be an integer >= {minimum}")
    return value


def require_token(value: Any, label: str) -> str:
    if not isinstance(value, str) or TOKEN_RE.fullmatch(value) is None:
        raise WorkloadError(f"{label} must be a lowercase token")
    return value


def validate_schema() -> None:
    schema = load_json(SCHEMA)
    required = schema.get("required") if isinstance(schema, dict) else None
    if (
        not isinstance(schema, dict)
        or schema.get("$schema") != "https://json-schema.org/draft/2020-12/schema"
        or schema.get("$id") != "https://genesiscode.dev/schemas/roadmap-workloads-v0.1.json"
        or schema.get("additionalProperties") is not False
        or set(required or []) != TOP_FIELDS
    ):
        raise WorkloadError("roadmap workload schema identity or closure drift")
    for definition in ("input", "measurement", "protocol", "size", "target", "workload"):
        row = schema.get("$defs", {}).get(definition)
        if not isinstance(row, dict) or row.get("additionalProperties") is not False:
            raise WorkloadError(f"roadmap workload schema definition is open: {definition}")


EXPECTED_PROTOCOLS = {
    "exact-all-v0.1": {
        "confidence": "not-applicable-deterministic",
        "decisionStatistic": "all-observations-byte-and-semantic-identical",
        "dispersion": "not-applicable",
        "failurePolicy": "invalidate-entire-sample-set",
        "id": "exact-all-v0.1",
        "integerRounding": "not-applicable",
        "outlierPolicy": "retain-all",
        "primary": "all-match",
        "secondary": "none",
        "sampleOrdering": "lexicographic-case-then-tier",
    },
    "leak-30-v0.1": {
        "confidence": "all-30-quiescent-checkpoints-retained",
        "decisionStatistic": "maximum-quiescent-rss-growth-and-nonpositive-theil-sen-slope",
        "dispersion": "median-absolute-deviation",
        "failurePolicy": "invalidate-entire-session",
        "id": "leak-30-v0.1",
        "integerRounding": "floor-after-exact-rational",
        "outlierPolicy": "retain-all",
        "primary": "maximum-quiescent-rss-growth-percent",
        "secondary": "theil-sen-rss-slope-by-request",
        "sampleOrdering": "monotonic-request-index",
    },
    "timing-30-v0.1": {
        "confidence": "exact-binomial-median-95pct-ranks-10-21-for-n30",
        "decisionStatistic": "directional-95pct-median-confidence-bound",
        "dispersion": "median-absolute-deviation",
        "failurePolicy": "invalidate-entire-sample-set",
        "id": "timing-30-v0.1",
        "integerRounding": "floor-after-exact-rational",
        "outlierPolicy": "retain-all",
        "primary": "median",
        "secondary": "p95-nearest-rank-ceil",
        "sampleOrdering": "fixed-sequential",
    },
}


EXPECTED_ROWS = {
    "PB-1": ("active", "gc_runtime_bench/compiled-ast", "timing-30-v0.1", 5, 30, 2000, ("lte", "milliseconds", 250)),
    "PB-2": ("roadmap-blocked", "gc_runtime_bench/bytecode", "timing-30-v0.1", 5, 30, 1000, ("lte", "milliseconds", 40)),
    "PB-3": ("decision-gated", "gc_runtime_bench/validated-jit", "timing-30-v0.1", 5, 30, 500, ("lte", "milliseconds", 8)),
    "PB-4": ("active", "gc_runtime_bench/compiled-ast", "timing-30-v0.1", 5, 30, 5000, ("lte", "milliseconds", 150)),
    "PB-5": ("active", "gc_runtime_bench/compiled-ast", "timing-30-v0.1", 5, 30, 5000, ("lte", "milliseconds", 300)),
    "PB-6": ("roadmap-blocked", "genesis/check-snapshot-cold-process", "timing-30-v0.1", 5, 30, 2000, ("lte", "milliseconds", 30)),
    "PB-7": ("active", "gc_runtime_bench/selfhost-parser", "timing-30-v0.1", 5, 30, 120000, ("gte", "bytes-per-second", 1048576)),
    "PB-8": ("roadmap-blocked", "genesis/warm-rss-harness", "leak-30-v0.1", 1000, 30, 2700000, ("lte", "percent-rss-growth", 5)),
    "PB-9": ("roadmap-blocked", "genesis/semantic-tier-parity", "exact-all-v0.1", 0, 5, 30000, ("eq", "percent-identical", 100)),
    "PB-10": ("roadmap-blocked", "genesis/bootstrap-fixpoint", "exact-all-v0.1", 0, 2, 2700000, ("eq", "percent-byte-identical", 100)),
}


EXPECTED_DESCRIPTORS = {
    "PB-1": {"kind": "integer", "value": "75025"},
    "PB-2": {"kind": "integer", "value": "75025"},
    "PB-3": {"kind": "integer", "value": "75025"},
    "PB-4": {"endExclusive": 1000000, "kind": "integer-range-vector", "length": 1000000, "start": 0},
    "PB-5": {"entryCount": 100000, "keyEndExclusive": 100000, "keyStart": 0, "kind": "integer-identity-map"},
    "PB-6": {"canonicalModuleHashRequired": True, "kind": "successful-check", "moduleCount": 1},
    "PB-7": {"corpusBytes": 253896, "kind": "all-modules-parse", "moduleCount": 2},
    "PB-8": {"kind": "bounded-warm-session", "requestsHandled": 100000, "responseKind": "genesis/warm-response-v0.1"},
    "PB-9": {"caseCount": 5, "dimensions": ["canonical-hash", "effect-log", "resource-error", "schedule", "sealed-error", "value"], "kind": "all-tier-observations-identical"},
    "PB-10": {"artifactHashRelation": "stage2-equals-stage3", "kind": "cross-host-bootstrap-fixpoint", "tier1HostCount": 2},
}


EXPECTED_SIZES = {
    "PB-1": [("fib-n", "integer", 25)],
    "PB-2": [("fib-n", "integer", 25)],
    "PB-3": [("fib-n", "integer", 25)],
    "PB-4": [("vector-length", "elements", 1000000)],
    "PB-5": [("map-entries", "entries", 100000)],
    "PB-6": [("module-count", "modules", 1)],
    "PB-7": [("corpus-bytes", "bytes", 253896), ("module-count", "modules", 2)],
    "PB-8": [("checkpoint-count", "checkpoints", 30), ("measured-requests", "requests", 100000), ("quiescence-milliseconds", "milliseconds", 1000)],
    "PB-9": [("case-count", "cases", 5), ("comparison-dimensions", "dimensions", 6)],
    "PB-10": [("rebuilds-per-host", "rebuilds", 2), ("tier1-host-count", "hosts", 2)],
}

EXPECTED_MEASUREMENT_TEXT = {
    "PB-1": ("fresh-eval-context-warm-prelude-and-compiled-module", "process-local-evaluation", "process-local-evaluation"),
    "PB-2": ("fresh-vm-context-warm-prelude-and-verified-bytecode", "process-local-evaluation", "process-local-evaluation"),
    "PB-3": ("fresh-jit-context-warm-prelude-and-validated-code-cache", "process-local-evaluation", "process-local-evaluation"),
    "PB-4": ("fresh-eval-context-warm-prelude-and-compiled-module", "process-local-evaluation", "process-local-evaluation"),
    "PB-5": ("fresh-eval-context-warm-prelude-and-compiled-module", "process-local-evaluation", "process-local-evaluation"),
    "PB-6": ("fresh-process-warm-os-page-cache-pinned-prelude-snapshot", "fresh-process-check", "fresh-process-check"),
    "PB-7": ("fresh-eval-context-warm-prelude-parser-loaded", "full-corpus-parse", "full-corpus-parse"),
    "PB-8": ("single-persistent-process-quiesced-between-checkpoints", "quiescent-rss-checkpoint", "request"),
    "PB-9": ("isolated-fresh-context-no-result-cache", "case-per-runtime-tier", "case"),
    "PB-10": ("clean-content-store-offline-source-bootstrap", "rebuild-per-tier1-host", "bootstrap"),
}


def validate_repo_path(raw: Any, label: str) -> Path:
    if not isinstance(raw, str) or not raw or "\\" in raw:
        raise WorkloadError(f"{label} must be a non-empty POSIX repository path")
    pure = PurePosixPath(raw)
    if pure.is_absolute() or any(part in {"", ".", ".."} for part in pure.parts):
        raise WorkloadError(f"{label} escapes repository: {raw}")
    path = ROOT.joinpath(*pure.parts)
    try:
        path.resolve(strict=True).relative_to(ROOT.resolve())
    except (OSError, ValueError) as exc:
        raise WorkloadError(f"{label} is missing or escapes repository: {raw}") from exc
    if path.is_symlink() or not path.is_file():
        raise WorkloadError(f"{label} must be a regular non-symlink file: {raw}")
    return path


def validate_input(value: Any, label: str) -> Mapping[str, Any]:
    row = require_object(value, INPUT_FIELDS, label)
    path = validate_repo_path(row["path"], label + ".path")
    require_token(row["role"], label + ".role")
    expected_bytes = require_integer(row["bytes"], label + ".bytes")
    digest = row["sha256"]
    if not isinstance(digest, str) or SHA_RE.fullmatch(digest) is None:
        raise WorkloadError(f"{label}.sha256 must be lowercase SHA-256")
    payload = path.read_bytes()
    if len(payload) != expected_bytes:
        raise WorkloadError(f"{label} byte count drift: {row['path']}")
    if sha256(payload).hexdigest() != digest:
        raise WorkloadError(f"{label} content hash drift: {row['path']}")
    return row


def validate_descriptor(workload_id: str, value: Any) -> None:
    row = require_object(value, {"canonicalDescriptor", "sha256"}, workload_id + ".expected")
    raw = row["canonicalDescriptor"]
    if not isinstance(raw, str) or not raw.endswith("\n"):
        raise WorkloadError(f"{workload_id} expected descriptor must end with newline")
    try:
        parsed = json.loads(raw, object_pairs_hook=reject_duplicate_keys)
    except (json.JSONDecodeError, WorkloadError) as exc:
        raise WorkloadError(f"{workload_id} expected descriptor is invalid JSON: {exc}") from exc
    if canonical_bytes(parsed).decode("ascii") != raw:
        raise WorkloadError(f"{workload_id} expected descriptor is not canonical JSON")
    if parsed != EXPECTED_DESCRIPTORS[workload_id]:
        raise WorkloadError(f"{workload_id} expected semantic descriptor drift")
    if row["sha256"] != sha256(raw.encode("ascii")).hexdigest():
        raise WorkloadError(f"{workload_id} expected descriptor hash mismatch")


def validate_sizes(workload_id: str, value: Any) -> None:
    if not isinstance(value, list) or not value:
        raise WorkloadError(f"{workload_id}.sizes must be non-empty")
    observed = []
    for index, item in enumerate(value):
        row = require_object(item, {"name", "unit", "value"}, f"{workload_id}.sizes[{index}]")
        observed.append((require_token(row["name"], "size.name"), require_token(row["unit"], "size.unit"), require_integer(row["value"], "size.value")))
    if observed != sorted(observed) or observed != EXPECTED_SIZES[workload_id]:
        raise WorkloadError(f"{workload_id} normalized input sizes drift")


def validate_measurement(workload_id: str, value: Any, protocol_ids: set[str]) -> None:
    row = require_object(value, MEASUREMENT_FIELDS, workload_id + ".measurement")
    warmup = require_object(row["warmup"], {"count", "unit"}, workload_id + ".warmup")
    cache_state = require_token(row["cacheState"], workload_id + ".cacheState")
    sample_unit = require_token(row["sampleUnit"], workload_id + ".sampleUnit")
    warmup_unit = require_token(warmup["unit"], workload_id + ".warmup.unit")
    warmup_count = require_integer(warmup["count"], workload_id + ".warmup.count", 0)
    sample_count = require_integer(row["sampleCount"], workload_id + ".sampleCount")
    timeout_ms = require_integer(row["timeoutMs"], workload_id + ".timeoutMs")
    expected = EXPECTED_ROWS[workload_id]
    if row["protocolId"] not in protocol_ids or (row["protocolId"], warmup_count, sample_count, timeout_ms) != expected[2:6]:
        raise WorkloadError(f"{workload_id} measurement protocol/warmup/sample/timeout drift")
    if (cache_state, sample_unit, warmup_unit) != EXPECTED_MEASUREMENT_TEXT[workload_id]:
        raise WorkloadError(f"{workload_id} cache/sample/warmup unit drift")
    if row["protocolId"] == "timing-30-v0.1" and sample_count != 30:
        raise WorkloadError(f"{workload_id} timing evidence requires exactly 30 samples")


def validate_pb9(inputs: Sequence[Mapping[str, Any]]) -> None:
    manifest = load_json(ROOT / "benchmarks/roadmap/v0.1/pb9_semantic_parity_corpus.json")
    row = require_object(manifest, {"cases", "kind", "requiredDimensions", "version"}, "PB-9 corpus")
    if row["kind"] != "genesis/pb9-semantic-parity-corpus-v0.1" or row["version"] != "0.1":
        raise WorkloadError("PB-9 corpus identity drift")
    cases = row["cases"]
    if not isinstance(cases, list) or len(cases) != 5:
        raise WorkloadError("PB-9 corpus must contain exactly five normalized cases")
    case_ids = []
    dimensions = set()
    sources = []
    for index, case in enumerate(cases):
        case_row = require_object(case, {"dimensions", "id", "source"}, f"PB-9 case[{index}]")
        case_ids.append(require_token(case_row["id"], "PB-9 case id"))
        if not isinstance(case_row["dimensions"], list) or case_row["dimensions"] != sorted(set(case_row["dimensions"])):
            raise WorkloadError("PB-9 case dimensions must be sorted and unique")
        dimensions.update(case_row["dimensions"])
        validate_repo_path(case_row["source"], "PB-9 case source")
        sources.append(case_row["source"])
    if case_ids != sorted(case_ids) or len(set(case_ids)) != len(case_ids):
        raise WorkloadError("PB-9 case IDs must be sorted and unique")
    if row["requiredDimensions"] != sorted(dimensions):
        raise WorkloadError("PB-9 required dimensions do not equal case coverage")
    declared_sources = sorted(item["path"] for item in inputs if item["role"] == "corpus-source")
    if sorted(sources) != declared_sources:
        raise WorkloadError("PB-9 transitive corpus sources are not fully content-addressed")


def validate_pb10(inputs: Sequence[Mapping[str, Any]]) -> None:
    protocol = load_json(ROOT / "benchmarks/roadmap/v0.1/pb10_bootstrap_inputs.json")
    row = require_object(protocol, {"artifactSeed", "kind", "manifest", "stageCommands", "version"}, "PB-10 protocol")
    expected = {
        "artifactSeed": "selfhost/toolchain.gc",
        "kind": "genesis/pb10-bootstrap-inputs-v0.1",
        "manifest": "selfhost/toolchain_manifest.gc",
        "stageCommands": [
            "genesis selfhost-artifact --out <stage2>",
            "genesis --selfhost-artifact <stage2> selfhost-artifact --out <stage3>",
        ],
        "version": "0.1",
    }
    if row != expected:
        raise WorkloadError("PB-10 bootstrap protocol drift")
    declared = {item["path"] for item in inputs}
    if not {row["artifactSeed"], row["manifest"]} <= declared:
        raise WorkloadError("PB-10 stage inputs are not fully content-addressed")


def validate_policy(document: Any, verify_files: bool = True) -> Mapping[str, Any]:
    row = require_object(document, TOP_FIELDS, "roadmap workload policy")
    expected_header = {
        "identityAlgorithm": "sha256-canonical-json",
        "inputHashAlgorithm": "sha256",
        "kind": "genesis/roadmap-workloads-v0.1",
        "referenceHostPolicy": "policies/reference_host_profiles_v0.1.json",
        "roadmapTask": "R0.5.b",
        "version": "0.1",
    }
    for field, expected in expected_header.items():
        if row[field] != expected:
            raise WorkloadError(f"roadmap workload header drift: {field}")
    if not REFERENCE_HOSTS.is_file():
        raise WorkloadError("reference host policy is missing")

    protocols = row["protocols"]
    if not isinstance(protocols, list) or len(protocols) != 3:
        raise WorkloadError("roadmap workload policy requires exactly three protocols")
    protocol_map = {}
    for index, protocol in enumerate(protocols):
        protocol_row = require_object(protocol, PROTOCOL_FIELDS, f"protocol[{index}]")
        protocol_id = protocol_row["id"]
        if protocol_id in protocol_map:
            raise WorkloadError(f"duplicate protocol ID: {protocol_id}")
        protocol_map[protocol_id] = protocol_row
    if list(protocol_map) != sorted(EXPECTED_PROTOCOLS) or protocol_map != EXPECTED_PROTOCOLS:
        raise WorkloadError("statistical protocol drift")

    workloads = row["workloads"]
    if not isinstance(workloads, list) or [item.get("id") for item in workloads if isinstance(item, dict)] != WORKLOAD_IDS:
        raise WorkloadError("workloads must contain PB-1 through PB-10 in numeric order")
    for index, workload in enumerate(workloads):
        item = require_object(workload, WORKLOAD_FIELDS, f"workload[{index}]")
        workload_id = item["id"]
        expected = EXPECTED_ROWS[workload_id]
        if (item["status"], item["runner"]) != expected[:2]:
            raise WorkloadError(f"{workload_id} availability/runner drift")
        target = require_object(item["target"], {"comparison", "unit", "value"}, workload_id + ".target")
        if (target["comparison"], target["unit"], target["value"]) != expected[6]:
            raise WorkloadError(f"{workload_id} target drift")
        validate_descriptor(workload_id, item["expected"])
        validate_sizes(workload_id, item["sizes"])
        validate_measurement(workload_id, item["measurement"], set(protocol_map))
        inputs = item["inputs"]
        if not isinstance(inputs, list) or not inputs:
            raise WorkloadError(f"{workload_id} inputs must be non-empty")
        input_rows = []
        for input_index, input_value in enumerate(inputs):
            if verify_files:
                input_rows.append(validate_input(input_value, f"{workload_id}.inputs[{input_index}]"))
            else:
                input_rows.append(require_object(input_value, INPUT_FIELDS, f"{workload_id}.inputs[{input_index}]"))
        paths = [input_row["path"] for input_row in input_rows]
        if paths != sorted(paths) or len(paths) != len(set(paths)):
            raise WorkloadError(f"{workload_id} inputs must be path-sorted and unique")
        if workload_id == "PB-9":
            validate_pb9(input_rows)
        if workload_id == "PB-10":
            validate_pb10(input_rows)

    first_three = workloads[:3]
    if len({item["inputs"][0]["sha256"] for item in first_three}) != 1 or len({item["expected"]["sha256"] for item in first_three}) != 1:
        raise WorkloadError("PB-1/PB-2/PB-3 must share one exact fib source and outcome")
    return row


def policy_identity(document: Mapping[str, Any]) -> str:
    return sha256(canonical_bytes(document)).hexdigest()


def expect_reject(name: str, candidate: Any, verify_files: bool = True) -> None:
    try:
        validate_policy(candidate, verify_files=verify_files)
    except WorkloadError:
        return
    raise WorkloadError(f"negative control accepted: {name}")


def self_test(policy: Mapping[str, Any]) -> int:
    controls = 0
    mutations = []
    candidate = copy.deepcopy(policy); candidate["workloads"].pop(); mutations.append(("missing-pb", candidate))
    candidate = copy.deepcopy(policy); candidate["workloads"][1]["status"] = "active"; mutations.append(("false-availability", candidate))
    candidate = copy.deepcopy(policy); candidate["workloads"][0]["measurement"]["sampleCount"] = 3; mutations.append(("sample-count", candidate))
    candidate = copy.deepcopy(policy); candidate["workloads"][0]["measurement"]["cacheState"] = "unknown"; mutations.append(("cache-state", candidate))
    candidate = copy.deepcopy(policy); candidate["workloads"][0]["expected"]["sha256"] = "0" * 64; mutations.append(("descriptor-hash", candidate))
    candidate = copy.deepcopy(policy); candidate["workloads"][0]["inputs"][0]["sha256"] = "0" * 64; mutations.append(("source-hash", candidate))
    candidate = copy.deepcopy(policy); candidate["workloads"][0]["inputs"][0]["path"] = "../escape.gc"; mutations.append(("path-escape", candidate))
    candidate = copy.deepcopy(policy); candidate["protocols"][2]["outlierPolicy"] = "drop-slowest"; mutations.append(("outlier-policy", candidate))
    candidate = copy.deepcopy(policy); candidate["workloads"][8]["inputs"].pop(); mutations.append(("transitive-corpus", candidate))
    candidate = copy.deepcopy(policy); candidate["workloads"][9]["target"]["value"] = 99; mutations.append(("fixpoint-target", candidate))
    candidate = copy.deepcopy(policy); candidate["workloads"][3], candidate["workloads"][4] = candidate["workloads"][4], candidate["workloads"][3]; mutations.append(("workload-order", candidate))
    for name, candidate in mutations:
        expect_reject(name, candidate)
        controls += 1
    try:
        json.loads('{"kind":"a","kind":"b"}', object_pairs_hook=reject_duplicate_keys)
    except WorkloadError:
        controls += 1
    else:
        raise WorkloadError("negative control accepted: duplicate-key")
    print(f"roadmap-workloads: self-test ok (controls={controls})")
    return controls


def main(argv: Optional[Sequence[str]] = None) -> int:
    parser = argparse.ArgumentParser()
    parser.add_argument("command", choices=("check", "identity", "self-test"))
    parser.add_argument("--policy", type=Path, default=POLICY)
    args = parser.parse_args(argv)
    try:
        validate_schema()
        policy = validate_policy(load_json(args.policy))
        if args.command == "check":
            active = sum(1 for item in policy["workloads"] if item["status"] == "active")
            blocked = sum(1 for item in policy["workloads"] if item["status"] == "roadmap-blocked")
            print(f"roadmap-workloads: ok (workloads=10 active={active} blocked={blocked} decision_gated=1 identity={policy_identity(policy)})")
        elif args.command == "identity":
            print(policy_identity(policy))
        else:
            self_test(policy)
        return 0
    except (OSError, UnicodeError, ValueError, WorkloadError) as exc:
        print(f"roadmap-workloads: {exc}", file=sys.stderr)
        return 1


if __name__ == "__main__":
    raise SystemExit(main())
