#!/usr/bin/env python3
"""Render and check the standalone evidence verifier conformance vectors."""

from __future__ import annotations

import argparse
import base64
from hashlib import sha256
from pathlib import Path
import sys
from typing import Any, Mapping, Sequence

import genesis_evidence_profile as profile


ROOT = Path(__file__).resolve().parents[2]
VECTOR_ROOT = ROOT / "docs/program/evidence"
POLICY_PATH = ROOT / "policies/evidence_verifier_trust_v0.1.json"
TREE_NAME = "GENESIS_EVIDENCE_ARTIFACT_TREE_v0.1.json"
CATALOG_NAME = "GENESIS_EVIDENCE_VERIFIER_NEGATIVE_VECTORS_v0.1.json"
BUNDLE_NAME = "GENESIS_EVIDENCE_BUNDLE_v0.1.json"
ARTIFACT_PATH = Path(profile.FIXTURE_ARTIFACT_PATH)
SCHEMAS = {
    "docs/spec/GENESIS_ARTIFACT_HASH_TREE_v0.1.schema.json": "https://genesiscode.dev/schemas/artifact-hash-tree-v0.1.json",
    "docs/spec/GENESIS_EVIDENCE_TRUST_POLICY_v0.1.schema.json": "https://genesiscode.dev/schemas/evidence-verifier-trust-policy-v0.1.json",
    "docs/spec/GENESIS_EVIDENCE_VERIFIER_NEGATIVE_VECTORS_v0.1.schema.json": "https://genesiscode.dev/schemas/evidence-verifier-negative-vectors-v0.1.json",
}


class VectorError(ValueError):
    pass


def render_tree() -> Mapping[str, Any]:
    return profile.fixture_artifact_tree()


def render_policy() -> Mapping[str, Any]:
    tree_digest = sha256(profile.canonical_bytes(render_tree())).hexdigest()
    return {
        "kind": "genesis/evidence-verifier-trust-policy-v0.1",
        "version": "0.1",
        "compatibilityProfile": "genesis-evidence-v0.1+slsa-v1+dsse-v1",
        "acceptedBundleProfiles": ["E3"],
        "requiredPredicateTypes": [
            profile.GENESIS_PREDICATE_TYPE,
            profile.SLSA_PREDICATE_TYPE,
        ],
        "signaturePolicy": {
            "thresholdsByProfile": {"E3": 1},
            "predicateRoles": {
                profile.GENESIS_PREDICATE_TYPE: "genesis-evidence",
                profile.SLSA_PREDICATE_TYPE: "slsa-builder",
            },
            "trustedKeys": [
                {
                    "keyid": profile.FIXTURE_KEY_ID,
                    "algorithm": "ed25519",
                    "publicKey": base64.b64encode(
                        bytes.fromhex(profile.FIXTURE_PUBLIC_KEY_HEX)
                    ).decode("ascii"),
                    "roles": ["genesis-evidence", "slsa-builder"],
                }
            ],
        },
        "sourcePolicy": {
            "requireClean": True,
            "allowedDirtyPolicies": ["reject"],
            "expectedRepositoryUri": "https://example.invalid/genesisCode.git",
            "expectedRevision": "0123456789abcdef0123456789abcdef01234567",
            "expectedTreeDigest": {"sha256": "b" * 64},
        },
        "networkPolicy": {"requiredMode": "deny"},
        "negativeControlPolicy": {"requireAllPassed": True, "minimumCount": 1},
        "artifactTreePolicy": {
            "required": True,
            "algorithm": "sha256-merkle-v0.1",
            "manifestDigest": {"sha256": tree_digest},
        },
        "compatibility": {
            "allowedBuildTypes": [profile.BUILD_TYPE],
            "allowedBuilderIds": [profile.BUILDER_ID],
            "allowedEnvironmentProfiles": ["darwin-arm64/hermetic-v0.1"],
            "allowedVerifiers": [
                {
                    "name": "genesis-evidence-profile",
                    "version": "0.1.0",
                    "artifactUri": "urn:genesis:verifier:evidence-profile:0.1.0",
                    "artifactDigest": {"sha256": "d" * 64},
                }
            ],
        },
        "limits": {
            "maxInputBytes": 16777216,
            "maxAttestations": 8,
            "maxSignaturesPerAttestation": 16,
            "maxSubjects": 4096,
            "maxArtifacts": 4096,
            "maxArtifactBytes": 1073741824,
        },
    }


def render_negative_catalog() -> Mapping[str, Any]:
    cases = [
        ("artifact-content-substitution", "artifact", "artifact digest mismatch"),
        ("artifact-path-alias", "artifact-tree", "normalized relative path"),
        ("artifact-path-traversal", "artifact-tree", "normalized relative path"),
        ("artifact-tree-root-substitution", "artifact-tree", "Merkle root mismatch"),
        ("artifact-type-substitution", "artifact", "artifact size mismatch"),
        ("bundle-duplicate-key", "bundle", "duplicate JSON key"),
        ("bundle-missing-field", "bundle", "bundle missing fields: kind"),
        ("bundle-profile-unsupported", "bundle", "bundle profile is not allowed"),
        (
            "bundle-version-unsupported",
            "bundle",
            "bundle version: expected 0.1, observed 9.9",
        ),
        (
            "dirty-paths-digest-missing",
            "genesis-predicate",
            "dirty source requires dirtyPathsDigest",
        ),
        (
            "dirty-policy-missing",
            "genesis-predicate",
            "source missing fields: dirtyPolicy",
        ),
        ("dirty-source", "genesis-predicate", "clean source required"),
        (
            "failed-negative-control",
            "genesis-predicate",
            "negative control did not pass",
        ),
        ("float-number", "bundle", "floating-point JSON is forbidden"),
        ("forged-signature", "dsse", "signature verification failed"),
        (
            "hash-tree-policy-substitution",
            "policy",
            "artifact tree manifest digest does not match trust policy",
        ),
        ("missing-slsa-companion", "bundle", "required predicate is missing"),
        ("network-policy-bypass", "genesis-predicate", "network mode is not allowed"),
        ("payload-substitution", "dsse", "DSSE payload does not match statement"),
        ("policy-self-trust-injection", "policy", "policy SHA-256 mismatch"),
        ("signature-threshold-bypass", "dsse", "signature threshold not met"),
        ("slsa-build-type-substitution", "slsa-predicate", "build type is not allowed"),
        ("slsa-builder-substitution", "slsa-predicate", "builder id is not allowed"),
        (
            "source-repository-stale",
            "genesis-predicate",
            "source repository URI does not match trust policy",
        ),
        (
            "source-revision-stale",
            "genesis-predicate",
            "source revision does not match trust policy",
        ),
        (
            "source-tree-stale",
            "genesis-predicate",
            "source tree digest does not match trust policy",
        ),
        ("statement-subject-divergence", "statement", "statement subjects differ"),
        ("unsupported-predicate", "statement", "predicate type is not allowed"),
        ("untrusted-key", "dsse", "signature key is not trusted"),
        (
            "verifier-identity-substitution",
            "genesis-predicate",
            "verifier is not allowed",
        ),
    ]
    return {
        "kind": "genesis/evidence-verifier-negative-vectors-v0.1",
        "version": "0.1",
        "cases": [
            {"id": case_id, "surface": surface, "expectedDiagnostic": diagnostic}
            for case_id, surface, diagnostic in cases
        ],
    }


def rendered_files() -> Mapping[str, bytes]:
    return {
        BUNDLE_NAME: profile.retained_bytes(profile.render_fixture()),
        TREE_NAME: profile.retained_bytes(render_tree()),
        CATALOG_NAME: profile.retained_bytes(render_negative_catalog()),
        ARTIFACT_PATH.as_posix(): profile.FIXTURE_ARTIFACT_BYTES,
        "policy.json": profile.retained_bytes(render_policy()),
    }


def output_path(name: str, root: Path) -> Path:
    if name == "policy.json":
        return POLICY_PATH if root == VECTOR_ROOT else root / name
    return root / name


def validate_rendered() -> None:
    for relative_path, schema_id in SCHEMAS.items():
        schema = profile.require_object(
            profile.load_json(ROOT / relative_path), relative_path
        )
        if schema.get("$schema") != "https://json-schema.org/draft/2020-12/schema":
            raise VectorError(f"{relative_path} must use JSON Schema Draft 2020-12")
        if schema.get("$id") != schema_id:
            raise VectorError(f"{relative_path} has an unexpected $id")
    bundle = profile.render_fixture()
    profile.validate_bundle(bundle, fixture=True)
    tree = render_tree()
    entry = tree["entries"][0]
    if entry["path"] != profile.FIXTURE_ARTIFACT_PATH:
        raise VectorError("artifact tree path does not match the bundle fixture")
    digest = sha256(profile.FIXTURE_ARTIFACT_BYTES).hexdigest()
    if entry["digest"]["sha256"] != digest:
        raise VectorError("artifact tree digest does not match fixture bytes")
    subjects = bundle["attestations"][0]["statement"]["subject"]
    if subjects != [{"name": entry["path"], "digest": entry["digest"]}]:
        raise VectorError("artifact tree does not exactly cover statement subjects")
    policy = render_policy()
    key = policy["signaturePolicy"]["trustedKeys"][0]
    public_key = base64.b64decode(key["publicKey"], validate=True)
    expected_key_id = "sha256:" + sha256(public_key).hexdigest()
    if key["keyid"] != expected_key_id:
        raise VectorError("trust policy keyid does not match its public key")
    case_ids = [case["id"] for case in render_negative_catalog()["cases"]]
    if case_ids != sorted(set(case_ids)):
        raise VectorError("negative vector ids must be sorted and unique")


def write_files(root: Path) -> None:
    validate_rendered()
    for name, content in rendered_files().items():
        path = output_path(name, root)
        path.parent.mkdir(parents=True, exist_ok=True)
        path.write_bytes(content)


def check_files() -> None:
    validate_rendered()
    for name, expected in rendered_files().items():
        path = output_path(name, VECTOR_ROOT)
        if not path.is_file():
            raise VectorError(f"missing retained vector: {profile.display_path(path)}")
        if path.read_bytes() != expected:
            raise VectorError(
                "retained verifier vector drift: run "
                "bash scripts/update_genesis_evidence_verifier_vectors.sh"
            )


def main(argv: Sequence[str]) -> int:
    parser = argparse.ArgumentParser()
    mode = parser.add_mutually_exclusive_group(required=True)
    mode.add_argument("--check", action="store_true")
    mode.add_argument("--update", action="store_true")
    mode.add_argument("--render-dir", type=Path)
    args = parser.parse_args(argv)
    try:
        if args.check:
            check_files()
            print("genesis-evidence-verifier-vectors: ok (positive=4 negative=30)")
        elif args.update:
            write_files(VECTOR_ROOT)
            print("genesis-evidence-verifier-vectors: updated")
        else:
            write_files(args.render_dir.resolve())
    except (VectorError, profile.EvidenceError, ValueError, OSError) as exc:
        print(f"genesis-evidence-verifier-vectors: {exc}", file=sys.stderr)
        return 1
    return 0


if __name__ == "__main__":
    raise SystemExit(main(sys.argv[1:]))
