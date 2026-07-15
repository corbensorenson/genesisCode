#!/usr/bin/env python3
"""Validate public held-out commitments and optional private custody material."""

from __future__ import annotations

import argparse
import copy
from datetime import date
import hashlib
import json
import re
import sys
from pathlib import Path
from typing import Any


ROOT = Path(__file__).resolve().parents[2]
MANIFEST = ROOT / "docs/spec/GC_AGENT_HELD_OUT_EVALUATION_v0.1.json"
SCHEMA = ROOT / "docs/spec/GC_AGENT_HELD_OUT_EVALUATION_v0.1.schema.json"
PRIVATE_ROOT = ROOT / ".genesis/private/agent-evaluation"
SHA_RE = re.compile(r"^[0-9a-f]{64}$")
DATE_RE = re.compile(r"^20[0-9]{2}-[0-9]{2}-[0-9]{2}$")
TASK_CLASSES = [
    "completion", "deployment", "generation", "package-migration",
    "performance-repair", "policy-minimization", "refactor", "repair",
    "replay-investigation",
]
DOMAIN = b"genesis/agent-held-out-case/v0.1\0"


class HeldOutError(ValueError):
    pass


def require(condition: bool, message: str) -> None:
    if not condition:
        raise HeldOutError(message)


def load_json(path: Path) -> Any:
    def pairs(rows: list[tuple[str, Any]]) -> dict[str, Any]:
        result: dict[str, Any] = {}
        for key, value in rows:
            require(key not in result, f"duplicate JSON key: {key}")
            result[key] = value
        return result
    return json.loads(path.read_text(encoding="utf-8"), object_pairs_hook=pairs)


def canonical_bytes(value: Any) -> bytes:
    return (json.dumps(value, sort_keys=True, separators=(",", ":"), ensure_ascii=True) + "\n").encode("ascii")


def content_identity(document: dict[str, Any]) -> str:
    unsigned = copy.deepcopy(document)
    unsigned["contentIdentitySha256"] = ""
    return hashlib.sha256(canonical_bytes(unsigned)).hexdigest()


def commitment(salt_hex: str, payload: Any) -> str:
    require(re.fullmatch(r"[0-9a-f]{64}", salt_hex) is not None, "private salt must be exactly 32 bytes")
    return hashlib.sha256(DOMAIN + bytes.fromhex(salt_hex) + canonical_bytes(payload)).hexdigest()


def closed(value: Any, keys: set[str], label: str) -> dict[str, Any]:
    require(isinstance(value, dict) and set(value) == keys, f"{label} fields are not closed")
    return value


def validate_schema() -> None:
    schema = load_json(SCHEMA)
    require(schema.get("$schema") == "https://json-schema.org/draft/2020-12/schema", "schema draft drift")
    require(schema.get("$id") == "https://genesiscode.dev/schemas/gc-agent-held-out-evaluation-v0.1.json", "schema id drift")
    require(schema.get("additionalProperties") is False, "schema root must be closed")
    for name in ("protocol", "storage", "lifecycle", "contamination", "profile", "case", "epoch", "disclosure"):
        require(schema.get("$defs", {}).get(name, {}).get("additionalProperties") is False, f"schema {name} must be closed")


def validate(document: Any, *, check_identity: bool = True) -> dict[str, Any]:
    doc = closed(document, {
        "kind", "version", "evaluationId", "commitmentProtocol", "storagePolicy",
        "lifecyclePolicy", "contaminationPolicy", "epochs", "disclosures",
        "contentIdentitySha256",
    }, "manifest")
    require(doc["kind"] == "genesis/agent-held-out-evaluation-v0.1", "manifest kind drift")
    require(doc["version"] == "0.1.0" and doc["evaluationId"] == "GC-AGENT-HELD-OUT-v0.1", "manifest identity drift")
    require(doc["commitmentProtocol"] == {
        "algorithm": "sha256",
        "canonicalization": "genesis/canonical-json-v0.1",
        "caseDomain": "genesis/agent-held-out-case/v0.1\\0",
        "formula": "sha256(domain || 32-byte-secret-salt || canonical-case-bytes)",
        "saltBytes": 32,
        "saltVisibility": "withheld-until-disclosure",
    }, "commitment protocol drift")
    require(doc["storagePolicy"] == {
        "privateRoot": ".genesis/private/agent-evaluation",
        "publicContent": ["commitments", "disclosure-records", "lifecycle-metadata"],
        "forbiddenDistribution": ["case-payloads", "oracles", "salts"],
        "trainingPackExclusionRequired": True,
    }, "storage policy drift")
    require(doc["lifecyclePolicy"] == {
        "appendOnlyEpochs": True,
        "compromiseRequiresReplacement": True,
        "minimumDisclosureDelayDays": 30,
        "retiredCommitmentsRemainPublished": True,
        "rotationStates": ["active", "compromised", "retired"],
    }, "lifecycle policy drift")
    require(doc["contaminationPolicy"] == {
        "resultLabels": [
            "declared-contaminated", "declared-uncontaminated",
            "temporal-clean", "unknown",
        ],
        "defaultWhenTrainingProvenanceMissing": "unknown",
        "disclosedCaseRequiresContaminatedLabel": True,
        "mixedEpochAggregationForbidden": True,
        "requiredResultBindings": ["commitmentSnapshotIdentity", "epochId", "modelIdentity", "trainingCutoff"],
    }, "contamination policy drift")

    epochs = doc["epochs"]
    require(isinstance(epochs, list) and epochs, "at least one epoch is required")
    epoch_ids = [row.get("id") for row in epochs if isinstance(row, dict)]
    require(epoch_ids == sorted(set(epoch_ids)), "epoch ids must be sorted and unique")
    active = 0
    cases_by_epoch: dict[str, set[str]] = {}
    all_commitments: set[str] = set()
    for epoch in epochs:
        closed(epoch, {"id", "status", "activatedOn", "retiredOn", "replacementEpochId", "profile", "cases", "commitmentSnapshotIdentity"}, "epoch")
        require(re.fullmatch(r"epoch-20[0-9]{2}-[0-9]{2}-[a-z]", epoch["id"]) is not None, "invalid epoch id")
        require(epoch["status"] in doc["lifecyclePolicy"]["rotationStates"], f"{epoch['id']}: invalid status")
        require(DATE_RE.fullmatch(epoch["activatedOn"]) is not None, f"{epoch['id']}: invalid activation date")
        if epoch["status"] == "active":
            active += 1
            require(epoch["retiredOn"] is None and epoch["replacementEpochId"] is None, f"{epoch['id']}: active epoch has retirement metadata")
        else:
            require(isinstance(epoch["retiredOn"], str) and DATE_RE.fullmatch(epoch["retiredOn"]) is not None, f"{epoch['id']}: retired date missing")
            require(epoch["replacementEpochId"] in epoch_ids, f"{epoch['id']}: replacement epoch missing")
            require(epoch["replacementEpochId"] != epoch["id"], f"{epoch['id']}: epoch cannot replace itself")
            require(date.fromisoformat(epoch["retiredOn"]) >= date.fromisoformat(epoch["activatedOn"]), f"{epoch['id']}: retirement predates activation")
        profile = closed(epoch["profile"], {"id", "path", "sha256"}, f"{epoch['id']} profile")
        require(profile["id"] == "GC-AGENT-v0.3" and profile["path"] == "docs/spec/GC_AGENT_PROFILE_v0.3.json", "profile authority drift")
        profile_path = ROOT / profile["path"]
        require(profile_path.is_file() and hashlib.sha256(profile_path.read_bytes()).hexdigest() == profile["sha256"], "profile hash drift")
        cases = epoch["cases"]
        require(isinstance(cases, list) and len(cases) == len(TASK_CLASSES), f"{epoch['id']}: task coverage drift")
        ids = [case.get("id") for case in cases if isinstance(case, dict)]
        require(ids == sorted(set(ids)), f"{epoch['id']}: case ids must be sorted and unique")
        classes = []
        rows = []
        for case in cases:
            closed(case, {"id", "taskClass", "commitmentSha256", "oracleExposure"}, "case")
            require(re.fullmatch(r"ho-[a-z0-9-]+-[0-9]{2}", case["id"]) is not None, "invalid held-out case id")
            require(case["taskClass"] in TASK_CLASSES, f"{case['id']}: unknown task class")
            require(case["oracleExposure"] == "commitment-only", f"{case['id']}: oracle leakage")
            require(SHA_RE.fullmatch(case["commitmentSha256"]) is not None, f"{case['id']}: invalid commitment")
            require(case["commitmentSha256"] not in all_commitments, f"{case['id']}: commitment reuse")
            all_commitments.add(case["commitmentSha256"])
            classes.append(case["taskClass"])
            rows.append({"id": case["id"], "taskClass": case["taskClass"], "commitmentSha256": case["commitmentSha256"]})
        require(sorted(classes) == sorted(TASK_CLASSES), f"{epoch['id']}: task classes incomplete")
        expected_snapshot = hashlib.sha256(canonical_bytes(rows)).hexdigest()
        require(epoch["commitmentSnapshotIdentity"] == expected_snapshot, f"{epoch['id']}: snapshot identity drift")
        cases_by_epoch[epoch["id"]] = set(ids)
    require(active == 1, "exactly one active epoch is required")

    disclosures = doc["disclosures"]
    require(isinstance(disclosures, list), "disclosures must be an array")
    disclosure_ids = [row.get("id") for row in disclosures if isinstance(row, dict)]
    require(disclosure_ids == sorted(set(disclosure_ids)), "disclosure ids must be sorted and unique")
    for row in disclosures:
        closed(row, {"id", "epochId", "caseId", "disclosedOn", "reason", "payloadPath", "saltHex", "verifiedCommitmentSha256"}, "disclosure")
        require(row["epochId"] in cases_by_epoch and row["caseId"] in cases_by_epoch[row["epochId"]], "disclosure target is unknown")
        epoch = next(item for item in epochs if item["id"] == row["epochId"])
        case = next(item for item in epoch["cases"] if item["id"] == row["caseId"])
        require(epoch["status"] != "active", "active epoch cannot be disclosed")
        require(DATE_RE.fullmatch(row["disclosedOn"]) is not None, "invalid disclosure date")
        require(row["reason"] in {"scheduled-release", "compromise-forensics"}, "invalid disclosure reason")
        if row["reason"] == "scheduled-release":
            delay = (date.fromisoformat(row["disclosedOn"]) - date.fromisoformat(epoch["retiredOn"])).days
            require(epoch["status"] == "retired" and delay >= doc["lifecyclePolicy"]["minimumDisclosureDelayDays"], "scheduled disclosure is too early")
        else:
            require(epoch["status"] == "compromised", "forensic disclosure requires a compromised epoch")
        require(re.fullmatch(r"docs/program/held-out-disclosures/[A-Za-z0-9._/-]+", row["payloadPath"]) is not None, "disclosure path must be public and canonical")
        require(re.fullmatch(r"[0-9a-f]{64}", row["saltHex"]) is not None and SHA_RE.fullmatch(row["verifiedCommitmentSha256"]) is not None, "invalid disclosed commitment material")
        payload_path = ROOT / row["payloadPath"]
        require(payload_path.is_file() and not payload_path.is_symlink() and payload_path.resolve().is_relative_to(ROOT.resolve()), "disclosure payload is missing or escaped")
        opened = commitment(row["saltHex"], load_json(payload_path))
        require(opened == case["commitmentSha256"] == row["verifiedCommitmentSha256"], "disclosure does not open the published commitment")
    if check_identity:
        require(SHA_RE.fullmatch(doc["contentIdentitySha256"]) is not None and content_identity(doc) == doc["contentIdentitySha256"], "manifest content identity drift")
    return doc


def verify_private(document: dict[str, Any], path: Path) -> None:
    resolved = path.resolve(strict=True)
    require(resolved.is_relative_to(PRIVATE_ROOT.resolve()), "private pack must remain under the ignored custody root")
    require(path.is_file() and not path.is_symlink(), "private pack must be a regular non-symlink file")
    pack = closed(load_json(path), {"kind", "version", "evaluationId", "epochId", "cases"}, "private pack")
    require(pack["kind"] == "genesis/agent-held-out-private-pack-v0.1" and pack["version"] == "0.1.0", "private pack identity drift")
    require(pack["evaluationId"] == document["evaluationId"], "private pack evaluation mismatch")
    epoch = next((row for row in document["epochs"] if row["id"] == pack["epochId"]), None)
    require(epoch is not None, "private pack epoch is not public")
    public = {row["id"]: row for row in epoch["cases"]}
    require([row.get("id") for row in pack["cases"]] == sorted(public), "private pack case inventory drift")
    for row in pack["cases"]:
        closed(row, {"id", "taskClass", "saltHex", "payload"}, "private case")
        expected = public[row["id"]]
        require(row["taskClass"] == expected["taskClass"], f"{row['id']}: private task class drift")
        require(commitment(row["saltHex"], row["payload"]) == expected["commitmentSha256"], f"{row['id']}: commitment mismatch")


def self_test(document: dict[str, Any]) -> int:
    mutations = [
        ("unknown-field", lambda d: d.update({"oracle": True})),
        ("weak-salt", lambda d: d["commitmentProtocol"].__setitem__("saltBytes", 8)),
        ("raw-hash", lambda d: d["commitmentProtocol"].__setitem__("formula", "sha256(case)")),
        ("public-private-root", lambda d: d["storagePolicy"].__setitem__("privateRoot", "benchmarks/private")),
        ("distribution-broadening", lambda d: d["storagePolicy"]["forbiddenDistribution"].remove("oracles")),
        ("training-leak", lambda d: d["storagePolicy"].__setitem__("trainingPackExclusionRequired", False)),
        ("mutable-history", lambda d: d["lifecyclePolicy"].__setitem__("appendOnlyEpochs", False)),
        ("no-replacement", lambda d: d["lifecyclePolicy"].__setitem__("compromiseRequiresReplacement", False)),
        ("early-disclosure", lambda d: d["lifecyclePolicy"].__setitem__("minimumDisclosureDelayDays", 0)),
        ("contamination-default", lambda d: d["contaminationPolicy"].__setitem__("defaultWhenTrainingProvenanceMissing", "temporal-clean")),
        ("mixed-epoch", lambda d: d["contaminationPolicy"].__setitem__("mixedEpochAggregationForbidden", False)),
        ("case-omission", lambda d: d["epochs"][0]["cases"].pop()),
        ("oracle-leak", lambda d: d["epochs"][0]["cases"][0].__setitem__("oracleExposure", "public")),
        ("commitment-reuse", lambda d: d["epochs"][0]["cases"][1].__setitem__("commitmentSha256", d["epochs"][0]["cases"][0]["commitmentSha256"])),
        ("snapshot-drift", lambda d: d["epochs"][0].__setitem__("commitmentSnapshotIdentity", "0" * 64)),
        ("active-disclosure", lambda d: d["disclosures"].append({"id": "disclosure-01", "epochId": d["epochs"][0]["id"], "caseId": d["epochs"][0]["cases"][0]["id"], "disclosedOn": "2026-08-15", "reason": "scheduled-release", "payloadPath": "docs/program/held-out-disclosures/case.json", "saltHex": "0" * 64, "verifiedCommitmentSha256": d["epochs"][0]["cases"][0]["commitmentSha256"]})),
        ("identity-drift", lambda d: d.__setitem__("contentIdentitySha256", "0" * 64)),
    ]
    for name, mutate in mutations:
        candidate = copy.deepcopy(document)
        mutate(candidate)
        try:
            validate(candidate)
        except HeldOutError:
            continue
        raise HeldOutError(f"negative control accepted: {name}")
    return len(mutations)


def main() -> int:
    parser = argparse.ArgumentParser()
    parser.add_argument("--check", action="store_true")
    parser.add_argument("--self-test", action="store_true")
    parser.add_argument("--verify-private", type=Path)
    args = parser.parse_args()
    require(args.check or args.verify_private is not None, "select --check or --verify-private")
    validate_schema()
    document = validate(load_json(MANIFEST))
    if args.verify_private is not None:
        verify_private(document, args.verify_private)
    controls = self_test(document) if args.self_test else 0
    print(f"gc-held-out-evaluation: ok (epochs={len(document['epochs'])} cases={sum(len(e['cases']) for e in document['epochs'])} controls={controls} private_verified={args.verify_private is not None} identity={document['contentIdentitySha256']})")
    return 0


if __name__ == "__main__":
    try:
        raise SystemExit(main())
    except (HeldOutError, FileNotFoundError, json.JSONDecodeError) as exc:
        print(f"gc-held-out-evaluation: {exc}", file=sys.stderr)
        raise SystemExit(1)
