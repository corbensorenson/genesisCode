#!/usr/bin/env python3
"""Independent custody verifier for self-hosted package semver selection."""

from __future__ import annotations

import argparse
import copy
import hashlib
import json
import re
import sys
from pathlib import Path


class CheckError(RuntimeError):
    pass


def fail(message: str) -> None:
    raise CheckError(message)


def unique_object(pairs):
    out = {}
    for key, value in pairs:
        if key in out:
            fail(f"duplicate JSON key: {key}")
        out[key] = value
    return out


def load_json(path: Path):
    try:
        value = json.loads(path.read_text(), object_pairs_hook=unique_object)
    except (OSError, json.JSONDecodeError) as error:
        fail(f"cannot read {path}: {error}")
    if not isinstance(value, dict):
        fail(f"JSON root is not an object: {path}")
    return value


FIELDS = {
    "artifact", "auditDate", "binding", "contentIdentitySha256", "decisionInventory",
    "hostMechanisms", "hostOracle", "independentVerifier", "kind", "nonclaims",
    "productionEntrypoints", "requestKind", "resultKind", "schema", "sourceModule",
    "sourceSha256", "spec", "version",
}
CONSTANTS = {
    "artifact": "selfhost/toolchain.gc",
    "binding": "core/pkg::semver-select-authority",
    "decisionInventory": [
        "closed-candidate-admission",
        "highest-and-lowest-policy-extremum-selection",
        "equal-precedence-lexicographic-ref-tie-break",
        "empty-candidate-no-match-decision",
        "request-bound-exact-member-result",
    ],
    "hostMechanisms": [
        "artifact-only-shared-context-bootstrap-and-bounded-evaluation",
        "semver-syntax-range-matching-and-precedence-ranking",
        "local-and-remote-ref-observation",
        "registry-network-and-artifact-transport",
        "strict-result-membership-and-contradiction-checking",
    ],
    "hostOracle": {"parityOnly": True, "productionRequired": False, "removalTask": "R4.2.e"},
    "independentVerifier": "scripts/lib/selfhost_pkg_semver_select_authority.py",
    "kind": "genesis/selfhost-pkg-semver-select-authority-v0.1",
    "productionEntrypoints": ["genesis", "genesis_wasi"],
    "requestKind": "genesis/pkg-semver-select-request-v0.1",
    "resultKind": "genesis/pkg-semver-select-result-v0.1",
    "schema": "docs/spec/SELFHOST_PKG_SEMVER_SELECT_AUTHORITY_v0.1.schema.json",
    "sourceModule": "selfhost/pkg_semver_select_authority_v1.gc",
    "spec": "docs/spec/SELFHOST_PKG_SEMVER_SELECT_AUTHORITY_v0.1.md",
    "version": "0.1.0",
}
NONCLAIMS = {
    "bootstrap-fixpoint", "complete-graph-solving", "generic-lock-codec-authority",
    "h2-package-resolution", "r4-2-e-closure", "ref-and-registry-transport-authority",
    "release-qualification", "semver-grammar-and-range-matching-authority", "sh-c-closure",
    "workspace-authority",
}


def canonical_identity(profile) -> str:
    value = copy.deepcopy(profile)
    value.pop("contentIdentitySha256", None)
    encoded = json.dumps(value, sort_keys=True, separators=(",", ":")).encode()
    return hashlib.sha256(encoded).hexdigest()


def source_identity(relative: str, data: bytes) -> str:
    digest = hashlib.sha256()
    digest.update(relative.encode())
    digest.update(b"\0")
    digest.update(data)
    digest.update(b"\0")
    return digest.hexdigest()


def read(root: Path, relative: str, overrides) -> str:
    if relative in overrides:
        return overrides[relative]
    try:
        return (root / relative).read_text()
    except OSError as error:
        fail(f"cannot read {relative}: {error}")


def validate_profile(profile, schema, check_identity=True) -> None:
    if set(profile) != FIELDS:
        fail("profile field closure drift")
    if (schema.get("$schema") != "https://json-schema.org/draft/2020-12/schema"
            or schema.get("type") != "object" or schema.get("additionalProperties") is not False
            or set(schema.get("required", [])) != FIELDS
            or set(schema.get("properties", {})) != FIELDS):
        fail("schema closure drift")
    for name, expected in CONSTANTS.items():
        if profile.get(name) != expected:
            fail(f"profile {name} drift")
    if set(profile.get("nonclaims", [])) != NONCLAIMS:
        fail("profile nonclaim inventory drift")
    if not re.fullmatch(r"\d{4}-\d{2}-\d{2}", str(profile.get("auditDate", ""))):
        fail("profile auditDate invalid")
    for name in ("contentIdentitySha256", "sourceSha256"):
        if not re.fullmatch(r"[0-9a-f]{64}", str(profile.get(name, ""))):
            fail(f"profile {name} invalid")
    if check_identity and canonical_identity(profile) != profile["contentIdentitySha256"]:
        fail("profile content identity mismatch")


def validate_sources(root: Path, profile, overrides=None) -> None:
    overrides = overrides or {}
    module = read(root, profile["sourceModule"], overrides)
    manifest = read(root, "selfhost/toolchain_manifest.gc", overrides)
    artifact = read(root, profile["artifact"], overrides)
    owner = read(root, "crates/gc_effects/src/pkg_resolution_identity_authority.rs", overrides)
    adapter = read(root, "crates/gc_effects/src/pkg_semver_select_authority.rs", overrides)
    resolver = read(root, "crates/gc_effects/src/runner_vcs_pkg_helpers/pkg_resolution.rs", overrides)
    ledger = load_json(root / "docs/spec/SEMANTIC_OWNERSHIP_LEDGER_v0.1.json")

    for marker in (
        "(def core/pkg::semver-select-authority", profile["requestKind"], profile["resultKind"],
        "selfhost/pkg-semver-select::better?", "selfhost/pkg-semver-select::str-lex-lt?",
        "selfhost/pkg-semver-select::candidates-valid?", "selfhost/hash::hash-term request",
    ):
        if marker not in module:
            fail(f"GenesisCode authority missing marker: {marker}")
    if f'    "{profile["sourceModule"]}"' not in manifest or profile["binding"] not in manifest:
        fail("toolchain manifest does not custody semver selection authority")
    if f':path "{profile["sourceModule"]}"' not in artifact or profile["binding"] not in artifact:
        fail("published artifact does not contain semver selection authority")
    for marker in (
        "semver_select_authority: Value", "SEMVER_SELECT_BINDING", ".get(SEMVER_SELECT_BINDING)",
        "mod semver_select;",
    ):
        if marker not in owner:
            fail(f"shared authority context missing marker: {marker}")
    for marker in (
        "pub(crate) fn select_semver(", "decode_select_result", "request_hash", "outside the supplied candidate set",
        "no match for a nonempty candidate set", "candidate rank exceeds the protocol integer range",
    ):
        if marker not in adapter:
            fail(f"strict adapter missing marker: {marker}")
    parity_marker = '\n#[cfg(any(test, feature = "parity-oracle"))]\nfn select_semver_tag_ref_parity'
    if parity_marker not in resolver:
        fail("test-only Rust semver selection oracle boundary missing")
    production = resolver.split(parity_marker, 1)[0]
    for marker in (
        "fn collect_semver_candidates(", "fn select_semver_tag_ref(",
        ".select_semver(candidates, policy)",
    ):
        if marker not in production:
            fail(f"production semver authority route missing marker: {marker}")
    if resolver.count("identity_authority.as_deref_mut()") < 2:
        fail("lock and remote semver routes do not both pass the shared authority")
    for forbidden in ("let mut best:", ".min_by(|left, right|", "candidate.2 > *best_version"):
        if forbidden in production:
            fail(f"production route retains Rust selection oracle: {forbidden}")

    row = next((item for item in ledger.get("semanticDecisions", [])
                if item.get("id") == "SD-PACKAGE-RESOLUTION"), None)
    if not row or row.get("currentLevel") != "H0":
        fail("SD-PACKAGE-RESOLUTION must remain truthful H0")
    for path in (profile["sourceModule"], "crates/gc_effects/src/pkg_semver_select_authority.rs"):
        if path not in row.get("productionAuthorityPaths", []):
            fail(f"semantic ledger missing semver authority path: {path}")
    if profile["spec"] not in row.get("specAuthorityPaths", []):
        fail("semantic ledger missing semver authority specification")
    limitations = "\n".join(row.get("limitations", [])).lower()
    for marker in ("semver", "selection", "graph", "remain"):
        if marker not in limitations:
            fail(f"semantic ledger lacks partial semver claim/residual marker: {marker}")
    if source_identity(profile["sourceModule"], module.encode()) != profile["sourceSha256"]:
        fail("semver authority source identity mismatch")


def validate_all(root, profile, schema, overrides=None, check_identity=True) -> None:
    validate_profile(profile, schema, check_identity)
    validate_sources(root, profile, overrides)


def self_test(root: Path, profile, schema) -> int:
    paths = [
        profile["sourceModule"], "selfhost/toolchain_manifest.gc", profile["artifact"],
        "crates/gc_effects/src/pkg_resolution_identity_authority.rs",
        "crates/gc_effects/src/pkg_semver_select_authority.rs",
        "crates/gc_effects/src/runner_vcs_pkg_helpers/pkg_resolution.rs",
    ]
    sources = {path: read(root, path, {}) for path in paths}
    mutations = []

    def profile_mutation(name, value):
        changed = copy.deepcopy(profile)
        changed[name] = value
        changed["contentIdentitySha256"] = canonical_identity(changed)
        mutations.append((changed, {}, name))

    profile_mutation("binding", "core/pkg::wrong")
    profile_mutation("requestKind", "genesis/wrong")
    profile_mutation("resultKind", "genesis/wrong")
    profile_mutation("sourceSha256", "0" * 64)
    changed = copy.deepcopy(profile)
    changed["extra"] = True
    mutations.append((changed, {}, "open profile"))

    def source_mutation(path, old, new, label):
        if old not in sources[path]:
            fail(f"self-test mutation marker absent: {path}: {old}")
        mutations.append((profile, {path: sources[path].replace(old, new, 1)}, label))

    source_mutation(profile["sourceModule"], "(def core/pkg::semver-select-authority", "(def core/pkg::legacy", "binding")
    source_mutation(profile["sourceModule"], "selfhost/pkg-semver-select::better?", "selfhost/pkg-semver-select::legacy?", "selection")
    source_mutation(profile["sourceModule"], profile["resultKind"], "genesis/wrong-result", "result")
    source_mutation("selfhost/toolchain_manifest.gc", profile["sourceModule"], "selfhost/missing.gc", "manifest module")
    source_mutation("selfhost/toolchain_manifest.gc", profile["binding"], "core/pkg::missing-select", "manifest binding")
    source_mutation("crates/gc_effects/src/pkg_resolution_identity_authority.rs", ".get(SEMVER_SELECT_BINDING)", ".get(\"core/pkg::legacy-select\")", "context")
    source_mutation("crates/gc_effects/src/pkg_semver_select_authority.rs", "outside the supplied candidate set", "legacy acceptance", "membership")
    source_mutation("crates/gc_effects/src/pkg_semver_select_authority.rs", "no match for a nonempty candidate set", "legacy no match", "false no-match")
    source_mutation("crates/gc_effects/src/runner_vcs_pkg_helpers/pkg_resolution.rs", ".select_semver(candidates, policy)", ".legacy_select(candidates, policy)", "route")
    source_mutation("crates/gc_effects/src/runner_vcs_pkg_helpers/pkg_resolution.rs", '\n#[cfg(any(test, feature = "parity-oracle"))]\nfn select_semver_tag_ref_parity', "\nfn select_semver_tag_ref_parity", "oracle reachability")

    killed = 0
    for changed_profile, overrides, label in mutations:
        try:
            validate_all(root, changed_profile, schema, overrides, check_identity=True)
        except CheckError:
            killed += 1
        else:
            fail(f"self-test mutation survived: {label}")
    print(f"selfhost-pkg-semver-select-authority: self-test ok ({killed} mutations)")
    return killed


def main() -> int:
    parser = argparse.ArgumentParser()
    parser.add_argument("--root", type=Path, default=Path("."))
    parser.add_argument("--profile", type=Path, required=True)
    parser.add_argument("--schema", type=Path, required=True)
    parser.add_argument("--self-test", action="store_true")
    args = parser.parse_args()
    root = args.root.resolve()
    try:
        profile = load_json(root / args.profile)
        schema = load_json(root / args.schema)
        validate_all(root, profile, schema)
        killed = self_test(root, profile, schema) if args.self_test else 0
        print(
            "selfhost-pkg-semver-select-authority: ok "
            f"(profile={profile['contentIdentitySha256']} source={profile['sourceSha256']} mutations={killed})"
        )
        return 0
    except CheckError as error:
        print(f"selfhost-pkg-semver-select-authority: {error}", file=sys.stderr)
        return 2


if __name__ == "__main__":
    raise SystemExit(main())
