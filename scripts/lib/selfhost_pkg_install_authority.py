#!/usr/bin/env python3
"""Independent custody verifier for package install workflow authority."""

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


SOURCE_MODULES = [
    "selfhost/pkg_install_core_v1.gc",
    "selfhost/pkg_install_plan_v1.gc",
    "selfhost/pkg_install_finalize_v1.gc",
    "selfhost/pkg_install_authority_v1.gc",
]
FIELDS = {
    "artifact", "auditDate", "binding", "contentIdentitySha256", "decisionInventory",
    "hostMechanisms", "hostOracle", "independentVerifier", "kind", "nonclaims",
    "productionEntrypoints", "requestKind", "resultKind", "schema", "sourceModules",
    "sourceSha256", "spec", "version",
}
CONSTANTS = {
    "artifact": "selfhost/toolchain.gc",
    "binding": "core/pkg::install-authority",
    "decisionInventory": [
        "frozen-missing-lock-admission-and-ordered-diagnostic-set",
        "ordered-dependency-install-plan",
        "locked-over-requirement-registry-precedence",
        "resolve-if-missing-eligibility",
        "strict-and-workspace-root-projection",
        "complete-observation-coverage-and-plan-binding",
        "checked-missing-and-success-verdict",
        "install-dependency-provenance-projection",
        "request-bound-final-report",
    ],
    "hostMechanisms": [
        "artifact-only-shared-context-bootstrap-and-bounded-evaluation",
        "typed-lock-model-request-observation-and-result-transport",
        "bounded-ref-registry-resolver-and-artifact-hydration",
        "snapshot-and-commit-schema-parsing",
        "non-publish-strict-commit-closure-validation",
        "sealed-diagnostic-rendering",
    ],
    "hostOracle": {"parityOnly": True, "productionRequired": False, "removalTask": "R4.2.e"},
    "independentVerifier": "scripts/lib/selfhost_pkg_install_authority.py",
    "kind": "genesis/selfhost-pkg-install-authority-v0.1",
    "productionEntrypoints": ["genesis", "genesis_wasi"],
    "requestKind": "genesis/pkg-install-request-v0.1",
    "resultKind": "genesis/pkg-install-result-v0.1",
    "schema": "docs/spec/SELFHOST_PKG_INSTALL_AUTHORITY_v0.1.schema.json",
    "sourceModules": SOURCE_MODULES,
    "spec": "docs/spec/SELFHOST_PKG_INSTALL_AUTHORITY_v0.1.md",
    "version": "0.1.0",
}
NONCLAIMS = {
    "bootstrap-fixpoint", "complete-graph-solving",
    "generic-lock-or-toml-syntax-authority", "h2-package-resolution",
    "non-publish-artifact-validation-authority", "r4-2-e-closure",
    "ref-or-registry-transport-authority", "release-qualification",
    "semver-grammar-range-rank-authority", "sh-c-closure",
    "workspace-scaffolding-authority",
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
    adapter = text(root, "crates/gc_effects/src/pkg_install_authority.rs", overrides)
    shared = text(root, "crates/gc_effects/src/pkg_resolution_identity_authority.rs", overrides)
    install = text(
        root,
        "crates/gc_effects/src/runner_cap_pkg_low/dispatch_resolution/install_verify.rs",
        overrides,
    )
    parity = text(
        root,
        "crates/gc_effects/src/runner_cap_pkg_low/dispatch_resolution/install_verify/parity.rs",
        overrides,
    )
    validation = text(
        root,
        "crates/gc_effects/src/runner_vcs_pkg_helpers/pkg_resolution/lock_validation.rs",
        overrides,
    )
    tests = text(root, "crates/gc_cli/tests/cli_pkg_engine.rs", overrides)
    lock_tests = text(root, "crates/gc_cli/tests/cli_pkg_lock.rs", overrides)
    ledger = load_json(root / "docs/spec/SEMANTIC_OWNERSHIP_LEDGER_v0.1.json")

    for marker in (
        "(def core/pkg::install-authority", profile["requestKind"], profile["resultKind"],
        "selfhost/pkg-install::missing-locks", "selfhost/pkg-install::build-plan",
        "selfhost/pkg-install::registry", "selfhost/pkg-install::resolution-coherent?",
        "selfhost/pkg-install::nonnegative-int?",
        "selfhost/pkg-resolution-workflow::provenance-loop",
        "selfhost/hash::hash-term declared-plan",
    ):
        if marker not in modules:
            fail(f"GenesisCode install authority missing marker: {marker}")
    for path in SOURCE_MODULES:
        if f'    "{path}"' not in manifest or f':path "{path}"' not in artifact:
            fail(f"toolchain custody missing install module: {path}")
    if profile["binding"] not in manifest or profile["binding"] not in artifact:
        fail("toolchain custody missing install binding")

    for marker in (
        "install_authority: Value", ".get(INSTALL_BINDING)",
        "STEP_LIMIT: u64 = 20_000_000", "ALLOC_LIMIT: u64 = 40_000_000",
        "max_vec_len: Some(65_536)",
    ):
        if marker not in shared:
            fail(f"shared artifact-only install context missing marker: {marker}")
    for marker in (
        "pub(crate) fn plan_install(", "pub(crate) fn finalize_install(",
        "decode_plan_result", "decode_finalize_result",
        "install plan term and :plan-h contradict", "install provenance workspace root contradicts report",
        "authority_rejects_frozen_missing_and_owns_registry_precedence",
        "authority_finalizes_exact_report_and_rejects_observation_substitution",
    ):
        if marker not in adapter:
            fail(f"strict install adapter missing marker: {marker}")

    plan_at = install.find(".plan_install(")
    store_at = install.find("store.path_for(")
    resolve_at = install.find("resolve_requirement(")
    finalize_at = install.find(".finalize_install(")
    if min(plan_at, store_at, resolve_at, finalize_at) < 0:
        fail("install causal route markers missing")
    if not plan_at < store_at < resolve_at < finalize_at:
        fail("install causal ordering drift")
    for marker in (
        "package install requires the artifact-loaded GenesisCode install authority",
        '#[cfg(any(test, feature = "parity-oracle"))]', "mod parity;",
        "PkgInstallPlanDecision::FrozenMissing", "for step in &plan.steps",
        "step.registry.as_deref()", "commit_observations(store, &lock.locked)",
    ):
        if marker not in install:
            fail(f"install production/parity boundary missing marker: {marker}")
    for marker in (
        "pub(super) fn handle_pkg_install_parity(", "dependency_registry_alias(",
        "locked_dependency_provenance(",
    ):
        if marker not in parity:
            fail(f"compile-time install parity oracle missing marker: {marker}")
    for forbidden in (
        "fn handle_pkg_install_parity(", "locked_dependency_provenance(",
        "fn dependency_registry_alias(",
    ):
        if forbidden in install:
            fail(f"native install semantic oracle remains in production route: {forbidden}")
    for marker in (
        '#[cfg(any(test, feature = "parity-oracle"))]\npub(crate) fn locked_dependency_provenance(',
        "compatibility path exists only for differential tests and the parity oracle",
    ):
        if marker not in validation:
            fail(f"shared provenance parity custody missing marker: {marker}")
    if "pkg_install_verify_values_match_between_frontends" not in tests:
        fail("authentic install frontend differential marker missing")
    for marker in (
        "pkg_install_hydrates_locked_closure_from_registry_after_store_wipe",
        "gcpm_lock_and_install_emit_workspace_and_dependency_provenance",
    ):
        if marker not in lock_tests:
            fail(f"authentic install test marker missing: {marker}")

    row = next((item for item in ledger.get("semanticDecisions", [])
                if item.get("id") == "SD-PACKAGE-RESOLUTION"), None)
    if not row or row.get("currentLevel") != "H0":
        fail("SD-PACKAGE-RESOLUTION must remain truthful H0")
    required_paths = set(SOURCE_MODULES + [
        "crates/gc_effects/src/pkg_install_authority.rs",
        "crates/gc_effects/src/runner_cap_pkg_low/dispatch_resolution/install_verify.rs",
    ])
    if not required_paths.issubset(set(row.get("productionAuthorityPaths", []))):
        fail("semantic ledger missing install production authority paths")
    if profile["spec"] not in row.get("specAuthorityPaths", []):
        fail("semantic ledger missing install specification")
    limitations = "\n".join(row.get("limitations", [])).lower()
    for marker in ("install", "registry precedence", "provenance", "verify", "remain"):
        if marker not in limitations:
            fail(f"semantic ledger lacks install claim/residual marker: {marker}")
    if source_identity(root, overrides) != profile["sourceSha256"]:
        fail("install authority source identity mismatch")


def validate_all(root, profile, schema, overrides=None, check_identity=True) -> None:
    validate_profile(profile, schema, check_identity)
    validate_sources(root, profile, overrides)


def self_test(root, profile, schema) -> int:
    paths = SOURCE_MODULES + [
        "selfhost/toolchain_manifest.gc", profile["artifact"],
        "crates/gc_effects/src/pkg_install_authority.rs",
        "crates/gc_effects/src/pkg_resolution_identity_authority.rs",
        "crates/gc_effects/src/runner_cap_pkg_low/dispatch_resolution/install_verify.rs",
        "crates/gc_effects/src/runner_cap_pkg_low/dispatch_resolution/install_verify/parity.rs",
        "crates/gc_effects/src/runner_vcs_pkg_helpers/pkg_resolution/lock_validation.rs",
        "crates/gc_cli/tests/cli_pkg_engine.rs", "crates/gc_cli/tests/cli_pkg_lock.rs",
    ]
    sources = {path: text(root, path, {}) for path in paths}
    mutations = []

    def profile_mutation(name, value):
        changed = copy.deepcopy(profile)
        changed[name] = value
        changed["contentIdentitySha256"] = canonical_identity(changed)
        mutations.append((changed, {}, name))

    for name, value in (
        ("binding", "core/pkg::legacy-install"),
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

    source_mutation(SOURCE_MODULES[-1], "(def core/pkg::install-authority", "(def core/pkg::legacy-install", "source")
    source_mutation(SOURCE_MODULES[-1], "selfhost/hash::hash-term declared-plan", "selfhost/hash::hash-term expected", "plan binding")
    source_mutation(SOURCE_MODULES[2], "selfhost/pkg-install::nonnegative-int?", "selfhost/pkg-lock-read::int?", "negative count")
    source_mutation("selfhost/toolchain_manifest.gc", profile["binding"], "core/pkg::missing-install", "manifest")
    if profile["binding"] not in sources[profile["artifact"]]:
        fail("self-test marker absent for artifact")
    mutations.append((profile, {profile["artifact"]: sources[profile["artifact"]].replace(
        profile["binding"], "core/pkg::missing-install")}, "artifact"))
    source_mutation("crates/gc_effects/src/pkg_install_authority.rs", "install plan term and :plan-h contradict", "plan accepted", "decoder")
    source_mutation("crates/gc_effects/src/pkg_resolution_identity_authority.rs", ".get(INSTALL_BINDING)", ".get(PLAN_BINDING)", "loader")
    source_mutation("crates/gc_effects/src/runner_cap_pkg_low/dispatch_resolution/install_verify.rs", ".plan_install(", ".legacy_plan(", "causal plan")
    source_mutation("crates/gc_effects/src/runner_cap_pkg_low/dispatch_resolution/install_verify.rs", ".finalize_install(", ".legacy_finalize(", "causal finalize")
    source_mutation("crates/gc_effects/src/runner_cap_pkg_low/dispatch_resolution/install_verify/parity.rs", "pub(super) fn handle_pkg_install_parity(", "pub(super) fn legacy_install(", "parity custody")
    source_mutation(
        "crates/gc_effects/src/runner_vcs_pkg_helpers/pkg_resolution/lock_validation.rs",
        '#[cfg(any(test, feature = "parity-oracle"))]\npub(crate) fn locked_dependency_provenance(',
        '#[cfg(test)]\npub(crate) fn locked_dependency_provenance(',
        "provenance custody",
    )
    source_mutation("crates/gc_cli/tests/cli_pkg_engine.rs", "pkg_install_verify_values_match_between_frontends", "legacy_install_test", "integration")

    controls = 0
    for changed_profile, overrides, name in mutations:
        try:
            validate_all(root, changed_profile, schema, overrides)
        except CheckError:
            controls += 1
        else:
            fail(f"negative control survived: {name}")
    if controls != 18:
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
            "selfhost-pkg-install-authority: ok "
            f"profile={profile['contentIdentitySha256']} controls={controls}"
        )
        return 0
    except CheckError as error:
        print(f"selfhost-pkg-install-authority: {error}", file=sys.stderr)
        return 1


if __name__ == "__main__":
    raise SystemExit(main())
