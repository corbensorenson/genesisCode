#!/usr/bin/env python3
"""Independent custody verifier for package resolution workflow authority."""

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
    "selfhost/pkg_resolution_workflow_core_v1.gc",
    "selfhost/pkg_resolution_workflow_plan_v1.gc",
    "selfhost/pkg_resolution_workflow_finalize_v1.gc",
    "selfhost/pkg_resolution_workflow_authority_v1.gc",
]
FIELDS = {
    "artifact", "auditDate", "binding", "contentIdentitySha256", "decisionInventory",
    "hostMechanisms", "hostOracle", "independentVerifier", "kind", "nonclaims",
    "productionEntrypoints", "requestKind", "resultKind", "schema", "sourceModules",
    "sourceSha256", "spec", "version",
}
CONSTANTS = {
    "artifact": "selfhost/toolchain.gc",
    "binding": "core/pkg::resolution-workflow-authority",
    "decisionInventory": [
        "normalized-only-selection-set",
        "causal-ordered-resolver-step-plan",
        "complete-observation-coverage-and-plan-binding",
        "final-lock-update-classification-and-counts",
        "resolution-rationale-construction-and-identity",
        "workspace-snapshot-construction-and-identity",
        "lock-update-dependency-provenance-projection",
        "strict-malformed-commit-disposition",
        "request-bound-final-verdict",
    ],
    "hostMechanisms": [
        "artifact-only-shared-context-bootstrap-and-bounded-evaluation",
        "typed-request-observation-and-strict-result-transport",
        "selector-plan-semver-ref-registry-and-resolver-mechanisms",
        "non-publish-artifact-and-commit-closure-validation",
        "exact-authorized-byte-storage-and-atomic-lock-persistence",
        "sealed-diagnostic-rendering",
    ],
    "hostOracle": {"parityOnly": True, "productionRequired": False, "removalTask": "R4.2.e"},
    "independentVerifier": "scripts/lib/selfhost_pkg_resolution_workflow_authority.py",
    "kind": "genesis/selfhost-pkg-resolution-workflow-authority-v0.1",
    "productionEntrypoints": ["genesis", "genesis_wasi"],
    "requestKind": "genesis/pkg-resolution-workflow-request-v0.1",
    "resultKind": "genesis/pkg-resolution-workflow-result-v0.1",
    "schema": "docs/spec/SELFHOST_PKG_RESOLUTION_WORKFLOW_AUTHORITY_v0.1.schema.json",
    "sourceModules": SOURCE_MODULES,
    "spec": "docs/spec/SELFHOST_PKG_RESOLUTION_WORKFLOW_AUTHORITY_v0.1.md",
    "version": "0.1.0",
}
NONCLAIMS = {
    "bootstrap-fixpoint", "complete-graph-solving", "generic-lock-syntax-authority",
    "h2-package-resolution", "install-workflow-authority",
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


def source_identity(root: Path, overrides) -> str:
    digest = hashlib.sha256()
    for relative in SOURCE_MODULES:
        data = text(root, relative, overrides).encode()
        digest.update(relative.encode())
        digest.update(b"\0")
        digest.update(data)
        digest.update(b"\0")
    return digest.hexdigest()


def text(root: Path, relative: str, overrides) -> str:
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
    modules = "\n".join(text(root, path, overrides) for path in SOURCE_MODULES)
    manifest = text(root, "selfhost/toolchain_manifest.gc", overrides)
    artifact = text(root, profile["artifact"], overrides)
    adapter = text(root, "crates/gc_effects/src/pkg_resolution_workflow_authority.rs", overrides)
    shared = text(root, "crates/gc_effects/src/pkg_resolution_identity_authority.rs", overrides)
    workflow = text(
        root, "crates/gc_effects/src/runner_cap_pkg_low/dispatch_resolution/workflow.rs", overrides
    )
    parity = text(
        root,
        "crates/gc_effects/src/runner_cap_pkg_low/dispatch_resolution/workflow/parity.rs",
        overrides,
    )
    dispatch = text(
        root, "crates/gc_effects/src/runner_cap_pkg_low/dispatch_resolution.rs", overrides
    )
    rationale = text(
        root,
        "crates/gc_effects/src/runner_cap_pkg_low/dispatch_resolution/rationale_diagnostics.rs",
        overrides,
    )
    validation = text(
        root, "crates/gc_effects/src/runner_vcs_pkg_helpers/pkg_resolution/lock_validation.rs",
        overrides,
    )
    tests = text(root, "crates/gc_cli/tests/cli_pkg_engine.rs", overrides)
    ledger = load_json(root / "docs/spec/SEMANTIC_OWNERSHIP_LEDGER_v0.1.json")

    for marker in (
        "(def core/pkg::resolution-workflow-authority", profile["requestKind"],
        profile["resultKind"], "selfhost/pkg-resolution-workflow::build-plan",
        "selfhost/pkg-resolution-workflow::finalize-steps",
        "selfhost/pkg-resolution-workflow::provenance-loop",
        "selfhost/pkg-resolution-workflow::rationale-term",
        "selfhost/pkg-resolution-workflow::workspace-term",
        "selfhost/hash::hash-term declared-plan",
    ):
        if marker not in modules:
            fail(f"GenesisCode workflow authority missing marker: {marker}")
    for path in SOURCE_MODULES:
        if f'    "{path}"' not in manifest or f':path "{path}"' not in artifact:
            fail(f"toolchain custody missing workflow module: {path}")
    if profile["binding"] not in manifest or profile["binding"] not in artifact:
        fail("toolchain custody missing workflow binding")

    for marker in (
        "workflow_authority: Value", ".get(WORKFLOW_BINDING)", "STEP_LIMIT: u64 = 20_000_000",
        "ALLOC_LIMIT: u64 = 40_000_000", "max_vec_len: Some(65_536)",
    ):
        if marker not in shared:
            fail(f"shared artifact-only workflow context missing marker: {marker}")
    for marker in (
        "pub(crate) fn plan_workflow(", "pub(crate) fn finalize_workflow(",
        "decode_plan_result", "decode_finalize_result", "workflow plan term and :plan-h contradict",
        "workflow object :term, :bytes, and :h are malformed or contradictory",
        "authority_finalizes_exact_objects_and_rejects_observation_substitution",
        "authority_plans_normalized_only_filter_and_missing_selection",
    ):
        if marker not in adapter:
            fail(f"strict workflow adapter missing marker: {marker}")

    plan_at = workflow.find(".plan_workflow(")
    resolve_at = workflow.find("resolve_requirement(")
    finalize_at = workflow.find(".finalize_workflow(")
    persist_at = workflow.find("persist_object(")
    if min(plan_at, resolve_at, finalize_at, persist_at) < 0:
        fail("workflow causal route markers missing")
    if not plan_at < resolve_at or not finalize_at < persist_at:
        fail("workflow causal ordering drift")
    for marker in (
        "package resolution requires the artifact-loaded GenesisCode workflow authority",
        '#[cfg(any(test, feature = "parity-oracle"))]', "mod parity;",
        "artifact store identity contradicts the authorized",
        "validate_locked_entries_strict(",
    ):
        if marker not in workflow:
            fail(f"workflow production/parity boundary missing marker: {marker}")
    for marker in (
        "pub(super) fn plan_workflow_parity", "pub(super) fn finalize_workflow_parity",
        "fn requirement_term", "fn rationale_term", "fn workflow_object",
    ):
        if marker not in parity:
            fail(f"compile-time parity oracle missing marker: {marker}")
    for forbidden in ("fn plan_workflow_parity", "fn finalize_workflow_parity"):
        if forbidden in workflow:
            fail(f"native semantic oracle remains in production route module: {forbidden}")
    for marker in ("execute_workflow(", "finalize_workflow("):
        if dispatch.count(marker) != 2:
            fail(f"lock/update workflow route count drift: {marker}")
    for forbidden in (
        "build_lock_resolution_rationale", "persist_resolution_rationale_artifact",
        "update_rationale_term", "locked_entry_eq(", "normalize_only_filter(",
    ):
        if forbidden in dispatch or forbidden in rationale:
            fail(f"retired lock/update Rust semantic producer remains: {forbidden}")
    if "workspace_snapshot_term_from_lock" in validation:
        fail("retired Rust workspace snapshot producer remains")
    if "Install remains a separate R4.2.e residual" not in validation:
        fail("install-only provenance residual is not explicit")
    for marker in (
        "pkg_lock_value_matches_between_frontends", "pkg_update_value_matches_between_frontends",
        "fs::read(&rust_lock)", "fs::read(&self_lock)",
    ):
        if marker not in tests:
            fail(f"authentic differential test marker missing: {marker}")

    row = next((item for item in ledger.get("semanticDecisions", [])
                if item.get("id") == "SD-PACKAGE-RESOLUTION"), None)
    if not row or row.get("currentLevel") != "H0":
        fail("SD-PACKAGE-RESOLUTION must remain truthful H0")
    required_paths = set(SOURCE_MODULES + [
        "crates/gc_effects/src/pkg_resolution_workflow_authority.rs",
        "crates/gc_effects/src/runner_cap_pkg_low/dispatch_resolution/workflow.rs",
    ])
    if not required_paths.issubset(set(row.get("productionAuthorityPaths", []))):
        fail("semantic ledger missing workflow production authority paths")
    if profile["spec"] not in row.get("specAuthorityPaths", []):
        fail("semantic ledger missing workflow specification")
    limitations = "\n".join(row.get("limitations", [])).lower()
    for marker in ("workflow", "rationale", "workspace", "install", "remain"):
        if marker not in limitations:
            fail(f"semantic ledger lacks workflow claim/residual marker: {marker}")
    if source_identity(root, overrides) != profile["sourceSha256"]:
        fail("workflow authority source identity mismatch")


def validate_all(root, profile, schema, overrides=None, check_identity=True) -> None:
    validate_profile(profile, schema, check_identity)
    validate_sources(root, profile, overrides)


def self_test(root, profile, schema) -> int:
    paths = SOURCE_MODULES + [
        "selfhost/toolchain_manifest.gc", profile["artifact"],
        "crates/gc_effects/src/pkg_resolution_workflow_authority.rs",
        "crates/gc_effects/src/pkg_resolution_identity_authority.rs",
        "crates/gc_effects/src/runner_cap_pkg_low/dispatch_resolution/workflow.rs",
        "crates/gc_effects/src/runner_cap_pkg_low/dispatch_resolution/workflow/parity.rs",
        "crates/gc_effects/src/runner_cap_pkg_low/dispatch_resolution.rs",
        "crates/gc_effects/src/runner_cap_pkg_low/dispatch_resolution/rationale_diagnostics.rs",
        "crates/gc_effects/src/runner_vcs_pkg_helpers/pkg_resolution/lock_validation.rs",
        "crates/gc_cli/tests/cli_pkg_engine.rs",
    ]
    sources = {path: text(root, path, {}) for path in paths}
    mutations = []

    def profile_mutation(name, value):
        changed = copy.deepcopy(profile)
        changed[name] = value
        changed["contentIdentitySha256"] = canonical_identity(changed)
        mutations.append((changed, {}, name))

    for name, value in (
        ("binding", "core/pkg::legacy-workflow"),
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

    source_mutation(SOURCE_MODULES[-1], "(def core/pkg::resolution-workflow-authority", "(def core/pkg::legacy-workflow", "source")
    source_mutation(SOURCE_MODULES[-1], "selfhost/hash::hash-term declared-plan", "selfhost/hash::hash-term expected", "plan binding")
    source_mutation("selfhost/toolchain_manifest.gc", profile["binding"], "core/pkg::missing-workflow", "manifest")
    if profile["binding"] not in sources[profile["artifact"]]:
        fail("self-test marker absent for artifact")
    mutations.append((
        profile,
        {profile["artifact"]: sources[profile["artifact"]].replace(
            profile["binding"], "core/pkg::missing-workflow"
        )},
        "artifact",
    ))
    source_mutation("crates/gc_effects/src/pkg_resolution_workflow_authority.rs", "workflow plan term and :plan-h contradict", "plan accepted", "decoder")
    source_mutation("crates/gc_effects/src/pkg_resolution_identity_authority.rs", ".get(WORKFLOW_BINDING)", ".get(PLAN_BINDING)", "loader")
    source_mutation("crates/gc_effects/src/runner_cap_pkg_low/dispatch_resolution/workflow.rs", ".plan_workflow(", ".legacy_plan(", "causal plan")
    source_mutation("crates/gc_effects/src/runner_cap_pkg_low/dispatch_resolution/workflow.rs", ".finalize_workflow(", ".legacy_finalize(", "causal finalize")
    source_mutation("crates/gc_effects/src/runner_cap_pkg_low/dispatch_resolution/workflow/parity.rs", "pub(super) fn finalize_workflow_parity", "pub(super) fn legacy_finalize", "parity custody")
    source_mutation("crates/gc_effects/src/runner_cap_pkg_low/dispatch_resolution.rs", "execute_workflow(", "legacy_workflow(", "dispatch")
    source_mutation("crates/gc_cli/tests/cli_pkg_engine.rs", "pkg_update_value_matches_between_frontends", "legacy_update_test", "integration")

    controls = 0
    for changed_profile, overrides, name in mutations:
        try:
            validate_all(root, changed_profile, schema, overrides)
        except CheckError:
            controls += 1
        else:
            fail(f"negative control survived: {name}")
    if controls != 17:
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
            "selfhost-pkg-resolution-workflow-authority: ok "
            f"profile={profile['contentIdentitySha256']} controls={controls}"
        )
        return 0
    except CheckError as error:
        print(f"selfhost-pkg-resolution-workflow-authority: {error}", file=sys.stderr)
        return 1


if __name__ == "__main__":
    raise SystemExit(main())
