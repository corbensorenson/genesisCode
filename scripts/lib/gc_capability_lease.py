#!/usr/bin/env python3
"""Validate the deterministic, deny-by-default capability lease protocol."""

from __future__ import annotations

import argparse
import copy
import hashlib
import json
import re
import sys
from pathlib import Path
from typing import Any


ROOT = Path(__file__).resolve().parents[2]
PROTOCOL = ROOT / "docs/spec/GC_CAPABILITY_LEASE_PROTOCOL_v0.1.json"
SCHEMA = ROOT / "docs/spec/GC_CAPABILITY_LEASE_PROTOCOL_v0.1.schema.json"
SHA_RE = re.compile(r"^[0-9a-f]{64}$")


class LeaseProtocolError(ValueError):
    pass


def require(condition: bool, message: str) -> None:
    if not condition:
        raise LeaseProtocolError(message)


def load_json(path: Path) -> Any:
    def pairs(rows: list[tuple[str, Any]]) -> dict[str, Any]:
        result: dict[str, Any] = {}
        for key, value in rows:
            require(key not in result, f"duplicate JSON key: {key}")
            result[key] = value
        return result

    return json.loads(path.read_text(encoding="utf-8"), object_pairs_hook=pairs)


def canonical_bytes(value: Any) -> bytes:
    return (
        json.dumps(value, sort_keys=True, separators=(",", ":"), ensure_ascii=True)
        + "\n"
    ).encode("ascii")


def identity(document: dict[str, Any]) -> str:
    unsigned = copy.deepcopy(document)
    unsigned["contentIdentitySha256"] = ""
    return hashlib.sha256(canonical_bytes(unsigned)).hexdigest()


def validate(document: Any, *, check_identity: bool = True) -> dict[str, Any]:
    required = {
        "kind", "version", "protocolId", "leaseContract", "decisionContract",
        "stateContract", "safetyInvariants", "contentIdentitySha256",
    }
    require(isinstance(document, dict) and set(document) == required, "protocol fields are not closed")
    require(document["kind"] == "genesis/capability-lease-protocol-v0.1", "protocol kind drift")
    require(document["version"] == "0.1.0" and document["protocolId"] == "GC-CAPABILITY-LEASE-v0.1", "protocol identity drift")
    require(document["leaseContract"] == {
        "fields": ["leaseId", "subject", "operation", "scopeIdentitySha256", "notBeforeStep", "expiresAfterStep", "maxUses"],
        "identityDomain": "genesis/capability-lease/v0.1\\0",
        "identityFormula": "sha256(domain || canonical-lease-without-leaseId)",
        "scopeBinding": "content-addressed-exact-scope",
        "stepClock": "explicit-monotonic-logical-step",
    }, "lease contract drift")
    require(document["decisionContract"] == {
        "denyReasons": ["expired", "not-yet-valid", "operation-mismatch", "scope-mismatch", "subject-mismatch", "unknown-lease", "use-budget-exhausted"],
        "permitReason": "lease-valid",
        "resultFields": ["decision", "leaseId", "reason", "remainingUses"],
    }, "decision contract drift")
    require(document["stateContract"] == {
        "acceptedTransitions": ["issue", "consume", "revoke"],
        "canonicalOrdering": "leaseId-ascending",
        "replayBindingFields": ["leaseId", "requestIdentitySha256", "logicalStep", "priorStateIdentitySha256", "decision", "nextStateIdentitySha256"],
        "unknownTransitionPolicy": "deny-and-preserve-state",
    }, "state contract drift")
    invariants = document["safetyInvariants"]
    require(isinstance(invariants, list) and len(invariants) == len(set(invariants)) == 7, "safety invariants drift")
    for required_text in ("deny", "pure", "host-time", "replay", "append-only"):
        require(any(required_text in row for row in invariants), f"missing {required_text} invariant")
    if check_identity:
        require(SHA_RE.fullmatch(document["contentIdentitySha256"]) is not None and identity(document) == document["contentIdentitySha256"], "protocol content identity drift")
    return document


def self_test(document: dict[str, Any]) -> int:
    mutations = [
        ("unknown-field", lambda d: d.update({"ambientAuthority": True})),
        ("ambient-clock", lambda d: d["leaseContract"].__setitem__("stepClock", "host-time")),
        ("scope-broadening", lambda d: d["leaseContract"].__setitem__("scopeBinding", "prefix")),
        ("permit-unknown", lambda d: d["decisionContract"]["denyReasons"].remove("unknown-lease")),
        ("mutable-replay", lambda d: d["stateContract"]["replayBindingFields"].remove("priorStateIdentitySha256")),
        ("unknown-transition", lambda d: d["stateContract"].__setitem__("unknownTransitionPolicy", "permit")),
        ("identity-drift", lambda d: d.__setitem__("contentIdentitySha256", "0" * 64)),
    ]
    for name, mutate in mutations:
        candidate = copy.deepcopy(document)
        mutate(candidate)
        if name != "identity-drift":
            candidate["contentIdentitySha256"] = identity(candidate)
        try:
            validate(candidate)
        except LeaseProtocolError:
            continue
        raise LeaseProtocolError(f"negative control accepted: {name}")
    return len(mutations)


def main() -> int:
    parser = argparse.ArgumentParser()
    parser.add_argument("--check", action="store_true")
    parser.add_argument("--self-test", action="store_true")
    args = parser.parse_args()
    require(args.check, "select --check")
    schema = load_json(SCHEMA)
    require(schema.get("$schema") == "https://json-schema.org/draft/2020-12/schema" and schema.get("additionalProperties") is False, "schema authority drift")
    document = validate(load_json(PROTOCOL))
    controls = self_test(document) if args.self_test else 0
    print(f"gc-capability-lease: ok (controls={controls} identity={document['contentIdentitySha256']})")
    return 0


if __name__ == "__main__":
    try:
        raise SystemExit(main())
    except (LeaseProtocolError, FileNotFoundError, json.JSONDecodeError) as exc:
        print(f"gc-capability-lease: {exc}", file=sys.stderr)
        raise SystemExit(1)
