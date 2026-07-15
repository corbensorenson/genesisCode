#!/usr/bin/env python3
"""Closed contamination evidence classifier for GenesisBench."""

from __future__ import annotations

import copy
import hashlib
import json
import re
from datetime import datetime, timezone
from typing import Any


SHA_RE = re.compile(r"^[0-9a-f]{64}$")
ID_RE = re.compile(r"^[A-Za-z0-9][A-Za-z0-9._/-]{0,127}$")
MODEL_REVISION_RE = re.compile(r"^(?:sha256:)?[0-9a-f]{64}$")
REASON_RE = re.compile(r"^[a-z][a-z0-9./-]{1,127}$")
HOST_PATH_RE = re.compile(r"(?:/Users/|/home/|[A-Za-z]:\\\\Users\\\\)")
LABELS = {
    "declared-contaminated",
    "declared-uncontaminated",
    "temporal-clean",
    "unknown",
}
VISIBILITY_CLASSES = {
    "held-out-commitment",
    "public-anchor-hidden-oracle",
    "public-development-reference",
    "temporal-held-out",
}


class ContaminationError(ValueError):
    pass


def require(condition: bool, message: str) -> None:
    if not condition:
        raise ContaminationError(message)


def closed(value: Any, keys: set[str], label: str) -> dict[str, Any]:
    require(isinstance(value, dict) and set(value) == keys, f"{label} fields are not closed")
    return value


def canonical_bytes(value: Any) -> bytes:
    return (
        json.dumps(value, sort_keys=True, separators=(",", ":"), ensure_ascii=True)
        + "\n"
    ).encode("ascii")


def content_identity(document: dict[str, Any]) -> str:
    unsigned = copy.deepcopy(document)
    unsigned["contentIdentitySha256"] = ""
    return hashlib.sha256(canonical_bytes(unsigned)).hexdigest()


def timestamp(value: str, label: str) -> datetime:
    require(
        isinstance(value, str)
        and re.fullmatch(
            r"20[0-9]{2}-[0-9]{2}-[0-9]{2}T[0-9]{2}:[0-9]{2}:[0-9]{2}Z",
            value,
        )
        is not None,
        f"invalid {label}",
    )
    try:
        return datetime.strptime(value, "%Y-%m-%dT%H:%M:%SZ").replace(
            tzinfo=timezone.utc
        )
    except ValueError as exc:
        raise ContaminationError(f"invalid {label}") from exc


def nullable_timestamp(value: Any, label: str) -> datetime | None:
    return None if value is None else timestamp(value, label)


def nullable_hash(value: Any, label: str) -> None:
    require(
        value is None or (isinstance(value, str) and SHA_RE.fullmatch(value) is not None),
        f"invalid {label}",
    )


def classify_attestation(
    document: Any,
    run: dict[str, Any],
    visibility: str,
    held_out: dict[str, Any],
) -> tuple[str, list[str]]:
    doc = closed(
        document,
        {
            "kind",
            "version",
            "runIdentitySha256",
            "claim",
            "knownExposure",
            "modelRelease",
            "training",
            "task",
            "custody",
            "contentIdentitySha256",
        },
        "contamination attestation",
    )
    require(
        doc["kind"] == "genesis/genesisbench-contamination-attestation-v0.1"
        and doc["version"] == "0.1.0",
        "contamination attestation version drift",
    )
    require(
        isinstance(doc["runIdentitySha256"], str)
        and SHA_RE.fullmatch(doc["runIdentitySha256"]) is not None,
        "invalid attestation run identity",
    )
    require(isinstance(doc["claim"], str) and doc["claim"] in LABELS, "invalid contamination claim")
    require(
        doc["runIdentitySha256"] == run["contentIdentitySha256"],
        "contamination attestation run binding drift",
    )
    exposure = closed(
        doc["knownExposure"],
        {"exposed", "reasonCodes", "evidenceIdentitySha256"},
        "known exposure",
    )
    require(isinstance(exposure["exposed"], bool), "known exposure flag must be boolean")
    require(
        isinstance(exposure["reasonCodes"], list)
        and len(exposure["reasonCodes"]) <= 32
        and all(isinstance(code, str) and REASON_RE.fullmatch(code) for code in exposure["reasonCodes"]),
        "invalid known exposure reason codes",
    )
    require(
        exposure["reasonCodes"] == sorted(set(exposure["reasonCodes"])),
        "known exposure reasons must be sorted and unique",
    )
    absent = (
        not exposure["exposed"]
        and not exposure["reasonCodes"]
        and exposure["evidenceIdentitySha256"] is None
    )
    present = (
        exposure["exposed"]
        and bool(exposure["reasonCodes"])
        and SHA_RE.fullmatch(exposure["evidenceIdentitySha256"] or "") is not None
    )
    require(absent or present, "known exposure evidence is inconsistent")
    release = closed(
        doc["modelRelease"],
        {"modelId", "modelRevision", "releasedAt", "immutableEvidenceIdentitySha256"},
        "model release",
    )
    require(
        isinstance(release["modelId"], str)
        and ID_RE.fullmatch(release["modelId"]) is not None,
        "invalid release model id",
    )
    require(
        isinstance(release["modelRevision"], str)
        and MODEL_REVISION_RE.fullmatch(release["modelRevision"]) is not None,
        "invalid release model revision",
    )
    release_at = nullable_timestamp(release["releasedAt"], "model release timestamp")
    nullable_hash(release["immutableEvidenceIdentitySha256"], "model release evidence identity")
    require(
        release["modelId"] == run["model"]["modelId"]
        and release["modelRevision"] == run["model"]["modelRevision"],
        "model release binding drift",
    )
    training = closed(
        doc["training"],
        {
            "cutoffAt",
            "provenanceComplete",
            "nonExposureAttested",
            "attestationIdentitySha256",
        },
        "training provenance",
    )
    cutoff_at = nullable_timestamp(training["cutoffAt"], "training cutoff timestamp")
    require(
        isinstance(training["provenanceComplete"], bool)
        and isinstance(training["nonExposureAttested"], bool),
        "training evidence flags must be boolean",
    )
    nullable_hash(training["attestationIdentitySha256"], "training attestation identity")
    task = closed(
        doc["task"],
        {
            "visibilityClass",
            "epochId",
            "commitmentSnapshotIdentitySha256",
            "precommittedAt",
            "disclosedAt",
        },
        "task contamination evidence",
    )
    custody = closed(
        doc["custody"],
        {"separated", "attestationIdentitySha256"},
        "evaluation custody",
    )
    require(
        isinstance(task["visibilityClass"], str)
        and task["visibilityClass"] in VISIBILITY_CLASSES,
        "invalid task visibility class",
    )
    require(
        task["epochId"] is None
        or (isinstance(task["epochId"], str) and ID_RE.fullmatch(task["epochId"]) is not None),
        "invalid task epoch id",
    )
    nullable_hash(task["commitmentSnapshotIdentitySha256"], "commitment snapshot identity")
    precommit_at = nullable_timestamp(task["precommittedAt"], "task precommit timestamp")
    nullable_timestamp(task["disclosedAt"], "task disclosure timestamp")
    require(isinstance(custody["separated"], bool), "custody separation flag must be boolean")
    nullable_hash(custody["attestationIdentitySha256"], "custody attestation identity")
    require(task["visibilityClass"] == visibility, "attestation task visibility drift")
    require(
        task["epochId"] == run["benchmark"]["heldOutEpochId"],
        "attestation epoch binding drift",
    )

    evidence_codes: list[str] = []
    public_exposure = visibility == "public-development-reference"
    if public_exposure:
        evidence_codes.append("known-exposure/public-development-reference")
    if exposure["exposed"]:
        evidence_codes.extend(
            f"known-exposure/{code}" for code in exposure["reasonCodes"]
        )
    if public_exposure or exposure["exposed"]:
        supported = "declared-contaminated"
    else:
        declared_complete = (
            training["provenanceComplete"] is True
            and training["nonExposureAttested"] is True
            and SHA_RE.fullmatch(training["attestationIdentitySha256"] or "")
            is not None
        )
        temporal_complete = False
        if declared_complete and visibility == "temporal-held-out":
            epoch = next(
                (row for row in held_out["epochs"] if row["id"] == task["epochId"]),
                None,
            )
            if epoch is not None:
                temporal_complete = (
                    epoch["status"] == "active"
                    and task["disclosedAt"] is None
                    and task["commitmentSnapshotIdentitySha256"]
                    == epoch["commitmentSnapshotIdentity"]
                    and SHA_RE.fullmatch(
                        release["immutableEvidenceIdentitySha256"] or ""
                    )
                    is not None
                    and release_at is not None
                    and cutoff_at is not None
                    and precommit_at is not None
                    and cutoff_at <= release_at < precommit_at
                    and custody["separated"] is True
                    and SHA_RE.fullmatch(custody["attestationIdentitySha256"] or "")
                    is not None
                )
        if temporal_complete:
            supported = "temporal-clean"
            evidence_codes.extend(
                [
                    "temporal/active-undisclosed-epoch",
                    "temporal/commitment-custody-verified",
                    "temporal/task-precommitted-after-model-release",
                    "training/complete-non-exposure-attestation",
                ]
            )
        elif declared_complete:
            supported = "declared-uncontaminated"
            evidence_codes.append("training/complete-non-exposure-attestation")
        else:
            supported = "unknown"
            evidence_codes.append("insufficient-evidence/training-or-temporal-proof")
    evidence_codes = sorted(set(evidence_codes))
    require(
        SHA_RE.fullmatch(doc["contentIdentitySha256"]) is not None
        and doc["contentIdentitySha256"] == content_identity(doc),
        "contamination attestation identity drift",
    )
    require(
        HOST_PATH_RE.search(canonical_bytes(doc).decode("ascii")) is None,
        "contamination attestation leaks a host path",
    )
    return supported, evidence_codes


def validate_attestation(
    document: Any,
    run: dict[str, Any],
    visibility: str,
    held_out: dict[str, Any],
) -> tuple[str, list[str]]:
    supported, evidence_codes = classify_attestation(
        document, run, visibility, held_out
    )
    require(
        document["claim"] == supported,
        "contamination attestation claim exceeds or contradicts its evidence",
    )
    require(
        run["benchmark"]["contamination"] == supported,
        "run contamination label disagrees with attestation",
    )
    return supported, evidence_codes


def self_test(
    public_attestation: dict[str, Any],
    public_run: dict[str, Any],
    held_out: dict[str, Any],
) -> int:
    """Reject evidence rebinding and false clean claims at the trust boundary."""
    supported, _ = validate_attestation(
        public_attestation,
        public_run,
        "public-development-reference",
        held_out,
    )
    require(supported == "declared-contaminated", "public control classification drift")

    rejected = 0

    def reject_mutations(
        base: dict[str, Any],
        run: dict[str, Any],
        visibility: str,
        mutations: list[tuple[str, Any, bool]],
    ) -> None:
        nonlocal rejected
        for name, mutate, preserve_identity in mutations:
            candidate = copy.deepcopy(base)
            mutate(candidate)
            if not preserve_identity:
                candidate["contentIdentitySha256"] = content_identity(candidate)
            try:
                validate_attestation(candidate, run, visibility, held_out)
            except ContaminationError:
                rejected += 1
            else:
                raise ContaminationError(
                    f"negative contamination control accepted: {name}"
                )

    reject_mutations(
        public_attestation,
        public_run,
        "public-development-reference",
        [
            (
                "run-rebinding",
                lambda d: d.__setitem__("runIdentitySha256", "0" * 64),
                False,
            ),
            (
                "false-clean-claim",
                lambda d: d.__setitem__("claim", "temporal-clean"),
                False,
            ),
            (
                "exposure-without-evidence",
                lambda d: d["knownExposure"].__setitem__(
                    "evidenceIdentitySha256", None
                ),
                False,
            ),
            (
                "model-revision-rebinding",
                lambda d: d["modelRelease"].__setitem__(
                    "modelRevision", "sha256:" + "0" * 64
                ),
                False,
            ),
            (
                "visibility-rebinding",
                lambda d: d["task"].__setitem__(
                    "visibilityClass", "temporal-held-out"
                ),
                False,
            ),
            (
                "attestation-identity-drift",
                lambda d: d.__setitem__("contentIdentitySha256", "0" * 64),
                True,
            ),
            (
                "unknown-attestation-field",
                lambda d: d.__setitem__("note", "trust me"),
                False,
            ),
            (
                "invalid-release-timestamp",
                lambda d: d["modelRelease"].__setitem__("releasedAt", "recently"),
                False,
            ),
            (
                "non-boolean-exposure",
                lambda d: d["knownExposure"].__setitem__("exposed", 1),
                False,
            ),
            (
                "invalid-reason-code",
                lambda d: d["knownExposure"]["reasonCodes"].__setitem__(0, "Public Reference"),
                False,
            ),
            (
                "non-boolean-custody",
                lambda d: d["custody"].__setitem__("separated", "false"),
                False,
            ),
        ],
    )

    epoch = next((row for row in held_out["epochs"] if row["status"] == "active"), None)
    require(epoch is not None, "temporal control requires an active held-out epoch")
    temporal_run = {
        "contentIdentitySha256": "1" * 64,
        "model": {
            "modelId": "temporal-control-model",
            "modelRevision": "sha256:" + "2" * 64,
        },
        "benchmark": {
            "heldOutEpochId": epoch["id"],
            "contamination": "temporal-clean",
        },
    }
    temporal = {
        "kind": "genesis/genesisbench-contamination-attestation-v0.1",
        "version": "0.1.0",
        "runIdentitySha256": temporal_run["contentIdentitySha256"],
        "claim": "temporal-clean",
        "knownExposure": {
            "exposed": False,
            "reasonCodes": [],
            "evidenceIdentitySha256": None,
        },
        "modelRelease": {
            "modelId": temporal_run["model"]["modelId"],
            "modelRevision": temporal_run["model"]["modelRevision"],
            "releasedAt": "2026-07-01T00:00:00Z",
            "immutableEvidenceIdentitySha256": "3" * 64,
        },
        "training": {
            "cutoffAt": "2026-06-30T00:00:00Z",
            "provenanceComplete": True,
            "nonExposureAttested": True,
            "attestationIdentitySha256": "4" * 64,
        },
        "task": {
            "visibilityClass": "temporal-held-out",
            "epochId": epoch["id"],
            "commitmentSnapshotIdentitySha256": epoch[
                "commitmentSnapshotIdentity"
            ],
            "precommittedAt": "2026-07-15T00:00:00Z",
            "disclosedAt": None,
        },
        "custody": {
            "separated": True,
            "attestationIdentitySha256": "5" * 64,
        },
        "contentIdentitySha256": "",
    }
    temporal["contentIdentitySha256"] = content_identity(temporal)
    supported, _ = validate_attestation(
        temporal, temporal_run, "temporal-held-out", held_out
    )
    require(supported == "temporal-clean", "valid temporal control rejected")

    reject_mutations(
        temporal,
        temporal_run,
        "temporal-held-out",
        [
            (
                "precommit-before-release",
                lambda d: d["task"].__setitem__(
                    "precommittedAt", "2026-06-01T00:00:00Z"
                ),
                False,
            ),
            (
                "custody-not-separated",
                lambda d: d["custody"].__setitem__("separated", False),
                False,
            ),
            (
                "disclosed-epoch",
                lambda d: d["task"].__setitem__(
                    "disclosedAt", "2026-07-16T00:00:00Z"
                ),
                False,
            ),
            (
                "incomplete-training-provenance",
                lambda d: d["training"].__setitem__(
                    "provenanceComplete", False
                ),
                False,
            ),
            (
                "commitment-snapshot-rebinding",
                lambda d: d["task"].__setitem__(
                    "commitmentSnapshotIdentitySha256", "0" * 64
                ),
                False,
            ),
        ],
    )
    return rejected
