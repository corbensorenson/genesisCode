#!/usr/bin/env python3
"""Capture and validate append-only normalized roadmap baseline statements."""

from __future__ import annotations

import argparse
import base64
import copy
from fractions import Fraction
from hashlib import sha256
import json
import os
from pathlib import Path
import re
import subprocess
import sys
from typing import Any, Dict, Iterable, Mapping, Optional, Sequence, Tuple

import reference_host_profiles
import roadmap_workloads


ROOT = Path(__file__).resolve().parents[2]
STATEMENT_SCHEMA = ROOT / "docs/spec/ROADMAP_BASELINE_STATEMENT_v0.1.schema.json"
BUNDLE_SCHEMA = ROOT / "docs/spec/ROADMAP_BASELINE_BUNDLE_v0.1.schema.json"
PAYLOAD_TYPE = "application/vnd.genesiscode.roadmap-baseline.v0.1+json"
ACTIVE_IDS = ("PB-1", "PB-4", "PB-5", "PB-7")
SHA_RE = re.compile(r"^[0-9a-f]{64}$")
REVISION_RE = re.compile(r"^[0-9a-f]{40}$")


class BaselineError(ValueError):
    pass


def reject_duplicate_keys(pairs: Sequence[Tuple[str, Any]]) -> Dict[str, Any]:
    result: Dict[str, Any] = {}
    for key, value in pairs:
        if key in result:
            raise BaselineError(f"duplicate JSON key: {key}")
        result[key] = value
    return result


def load_json(path: Path) -> Any:
    try:
        return json.loads(path.read_text(encoding="utf-8"), object_pairs_hook=reject_duplicate_keys)
    except (OSError, UnicodeError, json.JSONDecodeError) as exc:
        raise BaselineError(f"cannot load {path}: {exc}") from exc


def canonical_bytes(value: Any) -> bytes:
    return (json.dumps(value, ensure_ascii=True, separators=(",", ":"), sort_keys=True) + "\n").encode("ascii")


def require_object(value: Any, fields: Iterable[str], label: str) -> Mapping[str, Any]:
    expected = set(fields)
    if not isinstance(value, dict) or set(value) != expected:
        observed = sorted(value) if isinstance(value, dict) else type(value).__name__
        raise BaselineError(f"{label} fields mismatch: {observed}")
    return value


def require_int(value: Any, label: str, minimum: int = 0) -> int:
    if not isinstance(value, int) or isinstance(value, bool) or value < minimum:
        raise BaselineError(f"{label} must be an integer >= {minimum}")
    return value


def validate_schema(path: Path, schema_id: str, required: Iterable[str]) -> None:
    schema = load_json(path)
    if (
        not isinstance(schema, dict)
        or schema.get("$schema") != "https://json-schema.org/draft/2020-12/schema"
        or schema.get("$id") != schema_id
        or schema.get("additionalProperties") is not False
        or set(schema.get("required", [])) != set(required)
    ):
        raise BaselineError(f"baseline schema identity or closure drift: {path.name}")


def run_text(args: Sequence[str], allow_failure: bool = False) -> str:
    proc = subprocess.run(args, cwd=ROOT, text=True, stdout=subprocess.PIPE, stderr=subprocess.PIPE, check=False)
    if proc.returncode != 0 and not allow_failure:
        raise BaselineError(f"command failed: {args[0]} exit={proc.returncode}")
    return proc.stdout.strip()


def source_state() -> Mapping[str, Any]:
    revision = run_text(("git", "rev-parse", "HEAD"))
    if REVISION_RE.fullmatch(revision) is None:
        raise BaselineError("Git revision is invalid")
    tracked = subprocess.run(
        ("git", "diff", "--binary", "--no-ext-diff", "HEAD", "--"),
        cwd=ROOT,
        stdout=subprocess.PIPE,
        stderr=subprocess.PIPE,
        check=False,
    )
    untracked = subprocess.run(
        ("git", "ls-files", "--others", "--exclude-standard", "-z"),
        cwd=ROOT,
        stdout=subprocess.PIPE,
        stderr=subprocess.PIPE,
        check=False,
    )
    if tracked.returncode != 0 or untracked.returncode != 0:
        raise BaselineError("cannot capture dirty source material")
    untracked_paths = sorted(path for path in untracked.stdout.split(b"\0") if path)
    digest = sha256()
    digest.update(b"GenesisCodeDirtyMaterialv0.1\0")
    digest.update(len(tracked.stdout).to_bytes(8, "big"))
    digest.update(tracked.stdout)
    for raw_path in untracked_paths:
        try:
            relative = raw_path.decode("utf-8")
        except UnicodeDecodeError as exc:
            raise BaselineError("untracked path is not UTF-8") from exc
        path = ROOT / relative
        if path.is_symlink() or not path.is_file():
            raise BaselineError(f"untracked baseline input is not a regular file: {relative}")
        payload = path.read_bytes()
        digest.update(len(raw_path).to_bytes(8, "big"))
        digest.update(raw_path)
        digest.update(len(payload).to_bytes(8, "big"))
        digest.update(sha256(payload).digest())
    dirty = bool(tracked.stdout or untracked_paths)
    return {
        "sourceDirty": dirty,
        "sourceDirtyMaterialSha256": digest.hexdigest() if dirty else None,
        "sourceRevision": revision,
    }


def rustc_identity() -> Tuple[str, str]:
    output = run_text(("rustc", "-vV"))
    version = re.search(r"^release: (\S+)$", output, re.MULTILINE)
    host = re.search(r"^host: (\S+)$", output, re.MULTILINE)
    if version is None or host is None:
        raise BaselineError("rustc identity is incomplete")
    return version.group(1), host.group(1)


def sample_once(binary: Path, workload: Mapping[str, Any], index: int) -> Mapping[str, Any]:
    timeout_seconds = workload["measurement"]["timeoutMs"] / 1000
    env = dict(os.environ)
    env["GENESIS_RUNTIME_WORKLOAD_PROFILE"] = "roadmap"
    try:
        proc = subprocess.run(
            (str(binary), "--mode", "workloads", "--roadmap-sample", workload["id"]),
            cwd=ROOT,
            env=env,
            text=True,
            stdout=subprocess.PIPE,
            stderr=subprocess.PIPE,
            timeout=timeout_seconds,
            check=False,
        )
    except subprocess.TimeoutExpired:
        return {"durationNs": None, "failureCode": "hard-timeout", "index": index, "outcome": "timeout", "semanticCheck": "failed"}
    if proc.returncode != 0 or proc.stderr or len(proc.stdout.splitlines()) != 1:
        return {"durationNs": None, "failureCode": "runner-error", "index": index, "outcome": "runner-error", "semanticCheck": "failed"}
    try:
        raw = json.loads(proc.stdout, object_pairs_hook=reject_duplicate_keys)
    except (json.JSONDecodeError, BaselineError):
        return {"durationNs": None, "failureCode": "runner-output-invalid", "index": index, "outcome": "runner-error", "semanticCheck": "failed"}
    expected = {
        "kind", "version", "workloadId", "durationNs", "expectedDescriptorSha256", "semanticCheck", "unit"
    }
    if (
        not isinstance(raw, dict)
        or set(raw) != expected
        or raw.get("kind") != "genesis/roadmap-workload-raw-sample-v0.1"
        or raw.get("version") != "0.1"
        or raw.get("workloadId") != workload["id"]
        or raw.get("expectedDescriptorSha256") != workload["expected"]["sha256"]
        or raw.get("semanticCheck") != "passed"
        or raw.get("unit") != "nanoseconds"
        or not isinstance(raw.get("durationNs"), int)
        or isinstance(raw.get("durationNs"), bool)
        or raw["durationNs"] <= 0
    ):
        return {"durationNs": None, "failureCode": "runner-output-mismatch", "index": index, "outcome": "runner-error", "semanticCheck": "failed"}
    return {"durationNs": raw["durationNs"], "failureCode": None, "index": index, "outcome": "passed", "semanticCheck": "passed"}


def median(values: Sequence[Fraction]) -> Fraction:
    ordered = sorted(values)
    middle = len(ordered) // 2
    if len(ordered) % 2:
        return ordered[middle]
    return (ordered[middle - 1] + ordered[middle]) / 2


def summary_for(workload: Mapping[str, Any], samples: Sequence[Mapping[str, Any]]) -> Optional[Mapping[str, Any]]:
    if any(sample["outcome"] != "passed" for sample in samples):
        return None
    durations = [require_int(sample["durationNs"], "sample.durationNs", 1) for sample in samples]
    if len(durations) != 30:
        raise BaselineError(f"{workload['id']} requires exactly 30 retained samples")
    if workload["id"] == "PB-7":
        corpus_bytes = next(item["value"] for item in workload["sizes"] if item["name"] == "corpus-bytes")
        values = [Fraction(corpus_bytes * 1_000_000_000, duration) for duration in durations]
        decision_statistic = "lower-median-95-bound"
        unit = "bytes-per-second"
        target_pass = sorted(values)[9] >= workload["target"]["value"]
        decision_value = sorted(values)[9]
    else:
        values = [Fraction(duration, 1) for duration in durations]
        decision_statistic = "upper-median-95-bound"
        unit = "nanoseconds"
        target_pass = sorted(values)[20] <= workload["target"]["value"] * 1_000_000
        decision_value = sorted(values)[20]
    center = median(values)
    deviations = [abs(value - center) for value in values]
    ordered = sorted(values)
    return {
        "decisionStatistic": decision_statistic,
        "decisionValue": int(decision_value),
        "lowerMedian95": int(ordered[9]),
        "mad": int(median(deviations)),
        "median": int(center),
        "p95": int(ordered[28]),
        "targetPass": target_pass,
        "unit": unit,
        "upperMedian95": int(ordered[20]),
    }


def failures_for(workload: Mapping[str, Any], status: str, samples: Sequence[Mapping[str, Any]], summary: Optional[Mapping[str, Any]]) -> list[Mapping[str, str]]:
    if status == "decision-gated":
        return [{"code": "decision-not-approved", "detail": "optional runner has no approved architecture decision"}]
    if status == "runner-unavailable":
        return [{"code": "runner-unavailable", "detail": f"normalized runner {workload['runner']} is not implemented"}]
    failed_samples = sum(1 for sample in samples if sample["outcome"] != "passed")
    if failed_samples:
        return [{"code": "sample-set-invalid", "detail": f"{failed_samples} of 30 retained samples failed"}]
    if summary is None:
        raise BaselineError("observed sample set has no summary")
    if summary["targetPass"] is not True:
        return [{"code": "budget-miss", "detail": "directional 95 percent median confidence bound misses target"}]
    return []


def render_statement(binary: Path, capture_date: str) -> Mapping[str, Any]:
    if re.fullmatch(r"[0-9]{4}-[0-9]{2}-[0-9]{2}", capture_date) is None:
        raise BaselineError("capture date must be YYYY-MM-DD")
    binary = binary.resolve(strict=True)
    if not binary.is_file():
        raise BaselineError("benchmark binary must be a regular file")
    policy = roadmap_workloads.validate_policy(roadmap_workloads.load_json(roadmap_workloads.POLICY))
    reference_policy = reference_host_profiles.validate_policy(
        reference_host_profiles.load_json(reference_host_profiles.POLICY)
    )
    host_observation = reference_host_profiles.probe(reference_policy)
    rustc_version, rustc_host = rustc_identity()
    source = source_state()
    build = {
        "binarySha256": sha256(binary.read_bytes()).hexdigest(),
        "profile": "selfhost-strict",
        "rustcHost": rustc_host,
        "rustcVersion": rustc_version,
        **source,
    }
    rows = []
    for workload in policy["workloads"]:
        policy_status = workload["status"]
        if policy_status == "active":
            status = "observed"
            warmups = [sample_once(binary, workload, index) for index in range(workload["measurement"]["warmup"]["count"])]
            samples = [sample_once(binary, workload, index) for index in range(workload["measurement"]["sampleCount"])]
            summary = summary_for(workload, samples)
        else:
            status = "decision-gated" if policy_status == "decision-gated" else "runner-unavailable"
            warmups = []
            samples = []
            summary = None
        failures = failures_for(workload, status, samples, summary)
        rows.append({
            "expectedDescriptorSha256": workload["expected"]["sha256"],
            "failures": failures,
            "id": workload["id"],
            "policyStatus": policy_status,
            "samples": samples,
            "status": status,
            "summary": summary,
            "target": workload["target"],
            "warmupSamples": warmups,
        })
    observed = [row for row in rows if row["status"] == "observed"]
    statement: Dict[str, Any] = {
        "authoritative": False,
        "build": build,
        "captureDate": capture_date,
        "evidenceClass": "E0",
        "hostObservation": host_observation,
        "kind": "genesis/roadmap-baseline-statement-v0.1",
        "overall": {
            "budgetFailing": sum(1 for row in observed if row["failures"]),
            "budgetPassing": sum(1 for row in observed if not row["failures"]),
            "decisionGated": sum(1 for row in rows if row["status"] == "decision-gated"),
            "observed": len(observed),
            "runnerUnavailable": sum(1 for row in rows if row["status"] == "runner-unavailable"),
            "status": "observed-with-failures",
        },
        "version": "0.1",
        "workloadPolicyIdentitySha256": roadmap_workloads.policy_identity(policy),
        "workloads": rows,
    }
    statement["baselineIdentitySha256"] = statement_identity(statement)
    validate_statement(statement, policy=policy, reference_policy=reference_policy)
    return statement


def statement_identity(statement: Mapping[str, Any]) -> str:
    payload = {key: value for key, value in statement.items() if key != "baselineIdentitySha256"}
    return sha256(canonical_bytes(payload)).hexdigest()


STATEMENT_FIELDS = {
    "authoritative", "baselineIdentitySha256", "build", "captureDate", "evidenceClass",
    "hostObservation", "kind", "overall", "version", "workloadPolicyIdentitySha256", "workloads",
}
WORKLOAD_FIELDS = {
    "expectedDescriptorSha256", "failures", "id", "policyStatus", "samples", "status", "summary", "target", "warmupSamples",
}
SAMPLE_FIELDS = {"durationNs", "failureCode", "index", "outcome", "semanticCheck"}
SUMMARY_FIELDS = {"decisionStatistic", "decisionValue", "lowerMedian95", "mad", "median", "p95", "targetPass", "unit", "upperMedian95"}


def validate_sample(sample: Any, expected_index: int, label: str) -> Mapping[str, Any]:
    row = require_object(sample, SAMPLE_FIELDS, label)
    if row["index"] != expected_index or row["outcome"] not in {"passed", "runner-error", "timeout"}:
        raise BaselineError(f"{label} identity/outcome drift")
    if row["outcome"] == "passed":
        require_int(row["durationNs"], label + ".durationNs", 1)
        if row["failureCode"] is not None or row["semanticCheck"] != "passed":
            raise BaselineError(f"{label} passing sample fields are inconsistent")
    elif row["durationNs"] is not None or not isinstance(row["failureCode"], str) or row["semanticCheck"] != "failed":
        raise BaselineError(f"{label} failed sample fields are inconsistent")
    return row


def validate_statement(statement: Any, policy: Optional[Mapping[str, Any]] = None, reference_policy: Optional[Mapping[str, Any]] = None) -> Mapping[str, Any]:
    row = require_object(statement, STATEMENT_FIELDS, "baseline statement")
    if row["kind"] != "genesis/roadmap-baseline-statement-v0.1" or row["version"] != "0.1" or row["evidenceClass"] != "E0" or row["authoritative"] is not False:
        raise BaselineError("baseline statement authority/identity drift")
    if row["baselineIdentitySha256"] != statement_identity(row):
        raise BaselineError("baseline statement identity mismatch")
    policy = policy or roadmap_workloads.validate_policy(roadmap_workloads.load_json(roadmap_workloads.POLICY))
    reference_policy = reference_policy or reference_host_profiles.validate_policy(reference_host_profiles.load_json(reference_host_profiles.POLICY))
    if row["workloadPolicyIdentitySha256"] != roadmap_workloads.policy_identity(policy):
        raise BaselineError("baseline workload policy identity is stale")
    reference_host_profiles.validate_observation(row["hostObservation"], reference_policy)
    build = require_object(row["build"], {"binarySha256", "profile", "rustcHost", "rustcVersion", "sourceDirty", "sourceDirtyMaterialSha256", "sourceRevision"}, "baseline build")
    if build["profile"] != "selfhost-strict" or SHA_RE.fullmatch(build["binarySha256"] or "") is None or REVISION_RE.fullmatch(build["sourceRevision"] or "") is None:
        raise BaselineError("baseline build identity drift")
    if not isinstance(build["sourceDirty"], bool) or (build["sourceDirty"] != isinstance(build["sourceDirtyMaterialSha256"], str)):
        raise BaselineError("baseline dirty source representation is inconsistent")
    workloads = row["workloads"]
    if not isinstance(workloads, list) or [item.get("id") for item in workloads if isinstance(item, dict)] != roadmap_workloads.WORKLOAD_IDS:
        raise BaselineError("baseline workload inventory/order drift")
    for item, workload in zip(workloads, policy["workloads"]):
        baseline = require_object(item, WORKLOAD_FIELDS, workload["id"])
        expected_status = "observed" if workload["status"] == "active" else "decision-gated" if workload["status"] == "decision-gated" else "runner-unavailable"
        if baseline["policyStatus"] != workload["status"] or baseline["status"] != expected_status or baseline["expectedDescriptorSha256"] != workload["expected"]["sha256"] or baseline["target"] != workload["target"]:
            raise BaselineError(f"{workload['id']} baseline policy binding drift")
        if expected_status == "observed":
            if len(baseline["warmupSamples"]) != workload["measurement"]["warmup"]["count"] or len(baseline["samples"]) != workload["measurement"]["sampleCount"]:
                raise BaselineError(f"{workload['id']} raw sample count drift")
            for index, sample in enumerate(baseline["warmupSamples"]):
                validate_sample(sample, index, f"{workload['id']}.warmup[{index}]")
            samples = [validate_sample(sample, index, f"{workload['id']}.samples[{index}]") for index, sample in enumerate(baseline["samples"])]
            expected_summary = summary_for(workload, samples)
            if baseline["summary"] != expected_summary:
                raise BaselineError(f"{workload['id']} summary was not derived from raw samples")
        elif baseline["samples"] or baseline["warmupSamples"] or baseline["summary"] is not None:
            raise BaselineError(f"{workload['id']} unavailable runner carries fabricated samples")
        expected_failures = failures_for(workload, expected_status, baseline["samples"], baseline["summary"])
        if baseline["failures"] != expected_failures:
            raise BaselineError(f"{workload['id']} current failure set drift")
    observed = [item for item in workloads if item["status"] == "observed"]
    expected_overall = {
        "budgetFailing": sum(1 for item in observed if item["failures"]),
        "budgetPassing": sum(1 for item in observed if not item["failures"]),
        "decisionGated": sum(1 for item in workloads if item["status"] == "decision-gated"),
        "observed": len(observed),
        "runnerUnavailable": sum(1 for item in workloads if item["status"] == "runner-unavailable"),
        "status": "observed-with-failures",
    }
    if row["overall"] != expected_overall:
        raise BaselineError("baseline overall summary drift")
    return row


BUNDLE_FIELDS = {"authority", "envelope", "evidenceClass", "kind", "signing", "statement", "version"}


def validate_bundle(bundle: Any) -> Mapping[str, Any]:
    row = require_object(bundle, BUNDLE_FIELDS, "baseline bundle")
    if row["kind"] != "genesis/roadmap-baseline-bundle-v0.1" or row["version"] != "0.1" or row["evidenceClass"] != "E0" or row["authority"] != "observation":
        raise BaselineError("baseline bundle authority/identity drift")
    statement = validate_statement(row["statement"])
    envelope = require_object(row["envelope"], {"payload", "payloadType", "signatures"}, "baseline envelope")
    if envelope["payloadType"] != PAYLOAD_TYPE:
        raise BaselineError("baseline payload type drift")
    try:
        payload = base64.b64decode(envelope["payload"], validate=True)
    except (ValueError, TypeError) as exc:
        raise BaselineError("baseline payload is not canonical base64") from exc
    if payload != canonical_bytes(statement):
        raise BaselineError("baseline payload does not equal canonical statement bytes")
    signatures = envelope["signatures"]
    if not isinstance(signatures, list) or len(signatures) != 1:
        raise BaselineError("baseline bundle requires exactly one fixture-integrity signature")
    signature = require_object(signatures[0], {"keyid", "sig"}, "baseline signature")
    try:
        signature_bytes = base64.b64decode(signature["sig"], validate=True)
    except (ValueError, TypeError) as exc:
        raise BaselineError("baseline signature is not canonical base64") from exc
    if len(signature_bytes) != 64 or not isinstance(signature["keyid"], str) or re.fullmatch(r"sha256:[0-9a-f]{64}", signature["keyid"]) is None:
        raise BaselineError("baseline signature identity/length drift")
    signing = require_object(row["signing"], {"keyId", "publicKeySha256", "signatureGrantsAuthority", "trust"}, "baseline signing")
    if signing != {
        "keyId": signature["keyid"],
        "publicKeySha256": signature["keyid"].split(":", 1)[1],
        "signatureGrantsAuthority": False,
        "trust": "externally-pinned-fixture-integrity-only",
    }:
        raise BaselineError("baseline signing policy drift")
    return row


def assemble_bundle(statement: Mapping[str, Any], signature_output: Mapping[str, Any]) -> Tuple[Mapping[str, Any], bytes]:
    statement = validate_statement(statement)
    signed = require_object(signature_output, {"envelope", "keyid", "kind", "publicKeyBase64", "publicKeySha256", "version"}, "producer output")
    if signed["kind"] != "genesis/roadmap-baseline-signature-v0.1" or signed["version"] != "0.1":
        raise BaselineError("producer output identity drift")
    try:
        public_key = base64.b64decode(signed["publicKeyBase64"], validate=True)
    except (ValueError, TypeError) as exc:
        raise BaselineError("producer public key is not canonical base64") from exc
    if len(public_key) != 32 or sha256(public_key).hexdigest() != signed["publicKeySha256"] or signed["keyid"] != "sha256:" + signed["publicKeySha256"]:
        raise BaselineError("producer public key identity mismatch")
    bundle = {
        "authority": "observation",
        "envelope": signed["envelope"],
        "evidenceClass": "E0",
        "kind": "genesis/roadmap-baseline-bundle-v0.1",
        "signing": {
            "keyId": signed["keyid"],
            "publicKeySha256": signed["publicKeySha256"],
            "signatureGrantsAuthority": False,
            "trust": "externally-pinned-fixture-integrity-only",
        },
        "statement": statement,
        "version": "0.1",
    }
    validate_bundle(bundle)
    return bundle, public_key


def self_test(statement: Mapping[str, Any]) -> int:
    controls = 0
    mutations = []
    candidate = copy.deepcopy(statement); candidate["authoritative"] = True; mutations.append(("authority-escalation", candidate))
    candidate = copy.deepcopy(statement); candidate["evidenceClass"] = "E3"; mutations.append(("class-escalation", candidate))
    candidate = copy.deepcopy(statement); candidate["workloadPolicyIdentitySha256"] = "0" * 64; mutations.append(("policy-substitution", candidate))
    candidate = copy.deepcopy(statement)
    for sample in candidate["workloads"][0]["samples"]:
        sample["durationNs"] += 1
    mutations.append(("raw-sample-set-tamper", candidate))
    candidate = copy.deepcopy(statement); candidate["workloads"][0]["summary"]["median"] += 1; mutations.append(("derived-summary-tamper", candidate))
    failing = next(index for index, item in enumerate(statement["workloads"]) if item["failures"])
    candidate = copy.deepcopy(statement); candidate["workloads"][failing]["failures"] = []; mutations.append(("failure-erasure", candidate))
    candidate = copy.deepcopy(statement); candidate["workloads"][1]["samples"] = [{"durationNs": 1, "failureCode": None, "index": 0, "outcome": "passed", "semanticCheck": "passed"}]; mutations.append(("unavailable-fabrication", candidate))
    for name, candidate in mutations:
        candidate["baselineIdentitySha256"] = statement_identity(candidate)
        try:
            validate_statement(candidate)
        except BaselineError:
            controls += 1
        else:
            raise BaselineError(f"negative control accepted: {name}")
    try:
        json.loads('{"kind":"a","kind":"b"}', object_pairs_hook=reject_duplicate_keys)
    except BaselineError:
        controls += 1
    else:
        raise BaselineError("negative control accepted: duplicate-key")
    print(f"roadmap-baseline: self-test ok (controls={controls})")
    return controls


def write_new(path: Path, value: Mapping[str, Any]) -> None:
    if path.exists():
        raise BaselineError(f"append-only output already exists: {path}")
    path.parent.mkdir(parents=True, exist_ok=True)
    flags = os.O_WRONLY | os.O_CREAT | os.O_EXCL
    fd = os.open(path, flags, 0o644)
    with os.fdopen(fd, "wb") as handle:
        handle.write(json.dumps(value, indent=2, ensure_ascii=True, sort_keys=True).encode("ascii") + b"\n")


def write_new_bytes(path: Path, payload: bytes) -> None:
    if path.exists():
        raise BaselineError(f"append-only output already exists: {path}")
    path.parent.mkdir(parents=True, exist_ok=True)
    fd = os.open(path, os.O_WRONLY | os.O_CREAT | os.O_EXCL, 0o644)
    with os.fdopen(fd, "wb") as handle:
        handle.write(payload)


def main(argv: Optional[Sequence[str]] = None) -> int:
    parser = argparse.ArgumentParser()
    parser.add_argument("command", choices=("assemble", "capture", "check", "check-bundle", "self-test"))
    parser.add_argument("--binary", type=Path)
    parser.add_argument("--capture-date", default="2026-07-10")
    parser.add_argument("--output", type=Path)
    parser.add_argument("--statement", type=Path)
    parser.add_argument("--signature", type=Path)
    parser.add_argument("--public-key-output", type=Path)
    args = parser.parse_args(argv)
    try:
        validate_schema(STATEMENT_SCHEMA, "https://genesiscode.dev/schemas/roadmap-baseline-statement-v0.1.json", STATEMENT_FIELDS)
        validate_schema(BUNDLE_SCHEMA, "https://genesiscode.dev/schemas/roadmap-baseline-bundle-v0.1.json", {"authority", "envelope", "evidenceClass", "kind", "signing", "statement", "version"})
        if args.command == "capture":
            if args.binary is None or args.output is None:
                raise BaselineError("capture requires --binary and --output")
            statement = render_statement(args.binary, args.capture_date)
            write_new(args.output, statement)
            print(f"roadmap-baseline: captured {args.output} identity={statement['baselineIdentitySha256']} status={statement['overall']['status']}")
        elif args.command == "assemble":
            if args.statement is None or args.signature is None or args.output is None or args.public_key_output is None:
                raise BaselineError("assemble requires --statement, --signature, --output, and --public-key-output")
            bundle, public_key = assemble_bundle(load_json(args.statement), load_json(args.signature))
            write_new(args.output, bundle)
            try:
                write_new_bytes(args.public_key_output, public_key)
            except Exception:
                args.output.unlink(missing_ok=True)
                raise
            print(f"roadmap-baseline: assembled {args.output} keyid={bundle['signing']['keyId']} identity={bundle['statement']['baselineIdentitySha256']}")
        elif args.command == "check-bundle":
            if args.statement is None:
                raise BaselineError("check-bundle requires --statement pointing to the bundle")
            bundle = validate_bundle(load_json(args.statement))
            print(f"roadmap-baseline-bundle: ok identity={bundle['statement']['baselineIdentitySha256']} keyid={bundle['signing']['keyId']} authority=observation")
        else:
            if args.statement is None:
                raise BaselineError(f"{args.command} requires --statement")
            statement = validate_statement(load_json(args.statement))
            if args.command == "check":
                print(f"roadmap-baseline: ok identity={statement['baselineIdentitySha256']} observed={statement['overall']['observed']} failures={statement['overall']['budgetFailing']}")
            else:
                self_test(statement)
        return 0
    except (BaselineError, OSError, UnicodeError, ValueError) as exc:
        print(f"roadmap-baseline: {exc}", file=sys.stderr)
        return 1


if __name__ == "__main__":
    raise SystemExit(main())
