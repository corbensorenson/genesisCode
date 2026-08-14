#!/usr/bin/env python3
"""Independent custody verifier for package verify workflow authority."""

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


def parse_json(raw: str, name: str):
    try:
        value = json.loads(raw, object_pairs_hook=unique_object)
    except json.JSONDecodeError as error:
        fail(f"cannot parse {name}: {error}")
    if not isinstance(value, dict):
        fail(f"{name} root must be object")
    return value


def load_json(path: Path):
    try:
        return parse_json(path.read_text(), str(path))
    except OSError as error:
        fail(f"cannot read {path}: {error}")


SOURCE_MODULES = [
    "selfhost/pkg_verify_core_v1.gc",
    "selfhost/pkg_verify_plan_v1.gc",
    "selfhost/pkg_verify_finalize_v1.gc",
    "selfhost/pkg_verify_authority_v1.gc",
]
FIELDS = {
    "artifact", "auditDate", "binding", "contentIdentitySha256", "decisionInventory",
    "hostMechanisms", "hostOracle", "independentVerifier", "kind", "nonclaims",
    "productionEntrypoints", "requestKind", "resultKind", "schema", "sourceModules",
    "sourceSha256", "spec", "version",
}
CONSTANTS = {
    "artifact": "selfhost/toolchain.gc",
    "binding": "core/pkg::verify-authority",
    "decisionInventory": [
        "ordered-locked-dependency-verification-plan",
        "canonical-hash-observation-order-and-prefix-binding",
        "missing-snapshot-and-shallow-reference-reporting",
        "fail-fast-terminal-observation-admission",
        "closed-corruption-and-schema-error-disposition",
        "commit-closure-error-code-and-message-selection",
        "checked-missing-and-success-accounting",
        "request-bound-exact-public-report",
    ],
    "hostMechanisms": [
        "artifact-only-shared-context-bootstrap-and-bounded-evaluation",
        "typed-lock-model-plan-observation-and-result-transport",
        "bounded-artifact-presence-and-blake3-integrity-checks",
        "snapshot-commit-evidence-and-attestation-schema-parsing",
        "bounded-commit-closure-traversal",
        "sealed-diagnostic-rendering",
    ],
    "hostOracle": {"parityOnly": True, "productionRequired": False, "removalTask": "R4.2.e"},
    "independentVerifier": "scripts/lib/selfhost_pkg_verify_authority.py",
    "kind": "genesis/selfhost-pkg-verify-authority-v0.1",
    "productionEntrypoints": ["genesis", "genesis_wasi"],
    "requestKind": "genesis/pkg-verify-request-v0.1",
    "resultKind": "genesis/pkg-verify-result-v0.1",
    "schema": "docs/spec/SELFHOST_PKG_VERIFY_AUTHORITY_v0.1.schema.json",
    "sourceModules": SOURCE_MODULES,
    "spec": "docs/spec/SELFHOST_PKG_VERIFY_AUTHORITY_v0.1.md",
    "version": "0.1.0",
}
NONCLAIMS = {
    "artifact-store-or-blake3-mechanism-authority", "bootstrap-fixpoint",
    "complete-graph-solving", "generic-lock-or-toml-syntax-authority",
    "h2-package-resolution", "r4-2-e-closure",
    "ref-or-registry-transport-authority", "release-qualification",
    "schema-parser-authority", "sh-c-closure", "workspace-scaffolding-authority",
}


def canonical_identity(profile) -> str:
    value = copy.deepcopy(profile)
    value.pop("contentIdentitySha256", None)
    return hashlib.sha256(
        json.dumps(value, sort_keys=True, separators=(",", ":")).encode()
    ).hexdigest()


def text(root: Path, relative: str, overrides) -> str:
    if relative in overrides:
        return overrides[relative]
    try:
        return (root / relative).read_text()
    except OSError as error:
        fail(f"cannot read {relative}: {error}")


def source_identity(root: Path, overrides) -> str:
    digest = hashlib.sha256()
    for relative in SOURCE_MODULES:
        digest.update(relative.encode())
        digest.update(b"\0")
        digest.update(text(root, relative, overrides).encode())
        digest.update(b"\0")
    return digest.hexdigest()


def validate_profile(profile, schema, check_identity=True) -> None:
    if set(profile) != FIELDS:
        fail("profile field closure drift")
    if (schema.get("$schema") != "https://json-schema.org/draft/2020-12/schema"
            or schema.get("type") != "object"
            or schema.get("additionalProperties") is not False
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
    modules = "\n".join(text(root, path, overrides) for path in SOURCE_MODULES)
    manifest = text(root, "selfhost/toolchain_manifest.gc", overrides)
    artifact = text(root, profile["artifact"], overrides)
    adapter = text(root, "crates/gc_effects/src/pkg_verify_authority.rs", overrides)
    shared = text(root, "crates/gc_effects/src/pkg_resolution_identity_authority.rs", overrides)
    runner = text(root, "crates/gc_effects/src/runner.rs", overrides)
    route = text(
        root,
        "crates/gc_effects/src/runner_cap_pkg_low/dispatch_resolution/install_verify.rs",
        overrides,
    )
    parity = text(
        root,
        "crates/gc_effects/src/runner_cap_pkg_low/dispatch_resolution/install_verify/parity.rs",
        overrides,
    )
    mechanism = text(
        root,
        "crates/gc_effects/src/runner_cap_pkg_low/dispatch_resolution/install_verify/verify_observation.rs",
        overrides,
    )
    tests = text(root, "crates/gc_cli/tests/cli_pkg_engine.rs", overrides)
    lock_tests = text(root, "crates/gc_cli/tests/cli_pkg_lock.rs", overrides)
    ledger = parse_json(
        text(root, "docs/spec/SEMANTIC_OWNERSHIP_LEDGER_v0.1.json", overrides), "ledger"
    )

    for marker in (
        "(def core/pkg::verify-authority", profile["requestKind"], profile["resultKind"],
        "selfhost/pkg-verify::build-plan", "selfhost/pkg-verify::hashes-valid-loop",
        "selfhost/pkg-verify::closure-coherent?", "selfhost/pkg-verify::observations-loop",
        "((core/int::eq? checked) 0)",
        "verify observations continue after terminal result",
        "selfhost/hash::hash-term declared", "artifact store corruption: ",
        "commit.result != locked.snapshot for ",
    ):
        if marker not in modules:
            fail(f"GenesisCode verify authority missing marker: {marker}")
    for source in SOURCE_MODULES:
        if source not in manifest:
            fail(f"toolchain manifest missing verify module: {source}")
    if profile["binding"] not in manifest:
        fail("toolchain manifest missing required verify binding")
    for marker in (profile["binding"], *SOURCE_MODULES):
        if marker not in artifact:
            fail(f"published artifact missing verify marker: {marker}")
    for marker in (
        "const VERIFY_BINDING", ".get(VERIFY_BINDING)", "verify_authority: Value",
        "pub(crate) fn plan_verify", "pub(crate) fn finalize_verify",
        "verify plan term and :plan-h contradict", "closed error inventory",
        "verify report :ok must match whether :missing is empty",
    ):
        if marker not in shared + adapter:
            fail(f"strict verify adapter missing marker: {marker}")
    verify_route = route[route.index("pub(super) fn handle_pkg_verify("):]
    plan_at = verify_route.find(".plan_verify(")
    store_at = verify_route.find("store.path_for(")
    finalize_at = verify_route.find(".finalize_verify(")
    if min(plan_at, store_at, finalize_at) < 0:
        fail("verify causal protocol marker missing")
    if not plan_at < store_at < finalize_at:
        fail("verify causal ordering drift")
    for marker in (
        "package verify requires the artifact-loaded GenesisCode verify authority",
        "for step in &plan.steps", "hashes.sort();", "hashes.dedup();",
        "observe_verify_commit_closure(", "if terminal", "break;",
        "PkgVerifyFinalized::Report", "PkgVerifyFinalized::Error",
    ):
        if marker not in verify_route:
            fail(f"verify production boundary missing marker: {marker}")
    for forbidden in ("let mut ok = true", "missing_hashes", "validate_commit_artifact_closure("):
        if forbidden in verify_route:
            fail(f"native verify semantic oracle remains in production route: {forbidden}")
    for marker in (
        "pub(super) fn observe_verify_commit_closure(", "store.path_for(hash).exists()",
        "store.verify_hex(hash)", "gc_vcs::Commit::from_term", "gc_vcs::Evidence::from_term",
        "gc_vcs::Attestation::from_term",
    ):
        if marker not in mechanism:
            fail(f"bounded verify mechanism missing marker: {marker}")
    if '"core/pkg-low::verify"' not in runner or "PkgResolutionIdentityAuthority::load" not in runner:
        fail("verify lazy authority route missing")
    if "pub(super) fn handle_pkg_verify_parity(" not in parity:
        fail("compile-time verify parity oracle missing")
    for marker in (
        "pkg_install_verify_values_match_between_frontends",
        "pkg_lock_install_verify_roundtrip_local_snapshot_selector",
        "pkg_verify_rejects_commit_with_missing_patch_closure",
    ):
        if marker not in tests + lock_tests:
            fail(f"authentic verify test marker missing: {marker}")

    row = next((item for item in ledger.get("semanticDecisions", [])
                if item.get("id") == "SD-PACKAGE-RESOLUTION"), None)
    if not row or row.get("currentLevel") != "H0":
        fail("SD-PACKAGE-RESOLUTION must remain truthful H0")
    required_paths = set(SOURCE_MODULES + [
        "crates/gc_effects/src/pkg_verify_authority.rs",
        "crates/gc_effects/src/runner_cap_pkg_low/dispatch_resolution/install_verify.rs",
        "crates/gc_effects/src/runner_cap_pkg_low/dispatch_resolution/install_verify/verify_observation.rs",
    ])
    if not required_paths.issubset(set(row.get("productionAuthorityPaths", []))):
        fail("semantic ledger missing verify production authority paths")
    if profile["spec"] not in row.get("specAuthorityPaths", []):
        fail("semantic ledger missing verify specification")
    limitations = "\n".join(row.get("limitations", [])).lower()
    for marker in ("verify", "fail-fast", "schema parsing", "remain"):
        if marker not in limitations:
            fail(f"semantic ledger lacks verify claim/residual marker: {marker}")
    if source_identity(root, overrides) != profile["sourceSha256"]:
        fail("verify authority source identity mismatch")


def validate_all(root, profile, schema, overrides=None, check_identity=True) -> None:
    validate_profile(profile, schema, check_identity)
    validate_sources(root, profile, overrides)


def self_test(root, profile, schema) -> int:
    paths = SOURCE_MODULES + [
        "selfhost/toolchain_manifest.gc", profile["artifact"],
        "crates/gc_effects/src/pkg_verify_authority.rs",
        "crates/gc_effects/src/pkg_resolution_identity_authority.rs",
        "crates/gc_effects/src/runner.rs",
        "crates/gc_effects/src/runner_cap_pkg_low/dispatch_resolution/install_verify.rs",
        "crates/gc_effects/src/runner_cap_pkg_low/dispatch_resolution/install_verify/verify_observation.rs",
        "crates/gc_effects/src/runner_cap_pkg_low/dispatch_resolution/install_verify/parity.rs",
        "crates/gc_cli/tests/cli_pkg_engine.rs", "crates/gc_cli/tests/cli_pkg_lock.rs",
        "docs/spec/SEMANTIC_OWNERSHIP_LEDGER_v0.1.json",
    ]
    sources = {path: text(root, path, {}) for path in paths}
    mutations = []

    def profile_mutation(name, value):
        changed = copy.deepcopy(profile)
        changed[name] = value
        changed["contentIdentitySha256"] = canonical_identity(changed)
        mutations.append((changed, {}, name))

    for name, value in (
        ("binding", "core/pkg::legacy-verify"),
        ("decisionInventory", profile["decisionInventory"][:-1]),
        ("hostMechanisms", profile["hostMechanisms"][:-1]),
        ("hostOracle", {"parityOnly": False, "productionRequired": True, "removalTask": "R4.2.e"}),
        ("nonclaims", profile["nonclaims"][:-1]),
        ("sourceSha256", "f" * 64),
    ):
        profile_mutation(name, value)

    def source_mutation(path, old, new, name):
        if old not in sources[path]:
            fail(f"self-test marker absent for {name}")
        mutations.append((profile, {path: sources[path].replace(old, new, 1)}, name))

    source_mutation(SOURCE_MODULES[-1], "(def core/pkg::verify-authority", "(def core/pkg::legacy-verify", "source")
    source_mutation(SOURCE_MODULES[-1], "selfhost/hash::hash-term declared", "selfhost/hash::hash-term expected", "plan binding")
    source_mutation(SOURCE_MODULES[2], "strictly sorted and unique", "unordered hashes accepted", "hash order")
    source_mutation(SOURCE_MODULES[2], "selfhost/pkg-verify::closure-coherent?", "selfhost/pkg-verify::closure-observation?", "status coherence")
    source_mutation(SOURCE_MODULES[2], "((core/int::eq? checked) 0)", "((core/int::eq? checked) -1)", "positive successful closure accounting")
    source_mutation(SOURCE_MODULES[2], "continue after terminal result", "continue after result", "terminal prefix")
    source_mutation("selfhost/toolchain_manifest.gc", profile["binding"], "core/pkg::missing-verify", "manifest")
    source_mutation("crates/gc_effects/src/pkg_verify_authority.rs", "verify plan term and :plan-h contradict", "verify plan accepted", "decoder")
    source_mutation("crates/gc_effects/src/pkg_verify_authority.rs", "verify report :ok must match whether :missing is empty", "verify report accepted", "report coherence decoder")
    source_mutation("crates/gc_effects/src/pkg_resolution_identity_authority.rs", ".get(VERIFY_BINDING)", ".get(PLAN_BINDING)", "loader")
    source_mutation("crates/gc_effects/src/runner_cap_pkg_low/dispatch_resolution/install_verify.rs", ".plan_verify(", ".legacy_plan_verify(", "causal plan")
    source_mutation("crates/gc_effects/src/runner_cap_pkg_low/dispatch_resolution/install_verify.rs", ".finalize_verify(", ".legacy_finalize_verify(", "causal finalize")
    source_mutation("crates/gc_effects/src/runner_cap_pkg_low/dispatch_resolution/install_verify/verify_observation.rs", "pub(super) fn observe_verify_commit_closure(", "pub(super) fn native_verify_policy(", "mechanism custody")
    source_mutation("crates/gc_effects/src/runner_cap_pkg_low/dispatch_resolution/install_verify/parity.rs", "pub(super) fn handle_pkg_verify_parity(", "pub(super) fn legacy_verify(", "parity custody")
    source_mutation("crates/gc_cli/tests/cli_pkg_lock.rs", "pkg_verify_rejects_commit_with_missing_patch_closure", "legacy_verify_test", "integration")

    controls = 0
    for changed_profile, overrides, name in mutations:
        try:
            validate_all(root, changed_profile, schema, overrides)
        except CheckError:
            controls += 1
        else:
            fail(f"negative control survived: {name}")
    if controls != 21:
        fail(f"negative control inventory drift: {controls}")
    return controls


def write_identities(path: Path, profile, root: Path) -> None:
    profile["sourceSha256"] = source_identity(root, {})
    profile["contentIdentitySha256"] = canonical_identity(profile)
    path.write_text(json.dumps(profile, indent=2) + "\n")


def main(argv=None) -> int:
    parser = argparse.ArgumentParser()
    parser.add_argument("--root", type=Path, required=True)
    parser.add_argument("--profile", type=Path, required=True)
    parser.add_argument("--schema", type=Path, required=True)
    parser.add_argument("--self-test", action="store_true")
    parser.add_argument("--write-identities", action="store_true")
    args = parser.parse_args(argv)
    root = args.root.resolve()
    try:
        profile = load_json(args.profile)
        schema = load_json(args.schema)
        if args.write_identities:
            write_identities(args.profile, profile, root)
            profile = load_json(args.profile)
        validate_all(root, profile, schema)
        controls = self_test(root, profile, schema) if args.self_test else 0
        print(
            "selfhost-pkg-verify-authority: ok "
            f"profile={profile['contentIdentitySha256']} controls={controls}"
        )
        return 0
    except CheckError as error:
        print(f"selfhost-pkg-verify-authority: {error}", file=sys.stderr)
        return 1


if __name__ == "__main__":
    raise SystemExit(main())
