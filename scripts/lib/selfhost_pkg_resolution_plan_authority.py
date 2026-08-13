#!/usr/bin/env python3
"""Independent custody verifier for self-hosted package resolution planning."""

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
    "binding": "core/pkg::resolution-plan-authority",
    "decisionInventory": [
        "selector-form-admission-and-normalization",
        "selector-kind-and-strategy-inference",
        "declared-strategy-agreement",
        "tag-policy-presence-admission",
        "semver-selection-policy-normalization",
        "existing-lock-update-admission",
        "request-bound-plan-verdict",
    ],
    "hostMechanisms": [
        "artifact-only-shared-context-bootstrap-and-bounded-evaluation",
        "typed-request-and-strict-result-transport",
        "semver-grammar-version-comparison-and-ref-observation",
        "artifact-commit-registry-network-and-lock-transport",
        "sealed-diagnostic-rendering",
    ],
    "hostOracle": {"parityOnly": True, "productionRequired": False, "removalTask": "R4.2.e"},
    "independentVerifier": "scripts/lib/selfhost_pkg_resolution_plan_authority.py",
    "kind": "genesis/selfhost-pkg-resolution-plan-authority-v0.1",
    "productionEntrypoints": ["genesis", "genesis_wasi"],
    "requestKind": "genesis/pkg-resolution-plan-request-v0.1",
    "resultKind": "genesis/pkg-resolution-plan-result-v0.1",
    "schema": "docs/spec/SELFHOST_PKG_RESOLUTION_PLAN_AUTHORITY_v0.1.schema.json",
    "sourceModule": "selfhost/pkg_resolution_identity_authority_v1.gc",
    "spec": "docs/spec/SELFHOST_PKG_RESOLUTION_PLAN_AUTHORITY_v0.1.md",
    "version": "0.1.0",
}
NONCLAIMS = {
    "bootstrap-fixpoint", "complete-graph-solving", "generic-lock-codec-authority",
    "h2-package-resolution", "r4-2-e-closure", "registry-transport-authority",
    "release-qualification", "sh-c-closure", "workspace-authority",
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


def source_text(root: Path, relative: str, overrides) -> str:
    if relative in overrides:
        return overrides[relative]
    try:
        return (root / relative).read_text()
    except OSError as error:
        fail(f"cannot read {relative}: {error}")


def validate_sources(root: Path, profile, overrides=None) -> None:
    overrides = overrides or {}
    module = source_text(root, profile["sourceModule"], overrides)
    manifest = source_text(root, "selfhost/toolchain_manifest.gc", overrides)
    artifact = source_text(root, profile["artifact"], overrides)
    adapter = source_text(root, "crates/gc_effects/src/pkg_resolution_identity_authority.rs", overrides)
    plan_adapter = source_text(root, "crates/gc_effects/src/pkg_resolution_plan_authority.rs", overrides)
    planner = source_text(root, "crates/gc_effects/src/runner_vcs_pkg_helpers/pkg_resolution.rs", overrides)
    validation = source_text(
        root, "crates/gc_effects/src/runner_vcs_pkg_helpers/pkg_resolution/lock_validation.rs", overrides
    )
    dispatch = source_text(
        root, "crates/gc_effects/src/runner_cap_pkg_low/dispatch_resolution.rs", overrides
    )
    install = source_text(
        root, "crates/gc_effects/src/runner_cap_pkg_low/dispatch_resolution/install_verify.rs", overrides
    )
    runner = source_text(root, "crates/gc_effects/src/runner.rs", overrides)
    ledger = load_json(root / "docs/spec/SEMANTIC_OWNERSHIP_LEDGER_v0.1.json")

    for marker in (
        "(def core/pkg::resolution-plan-authority", profile["requestKind"], profile["resultKind"],
        "selfhost/pkg-resolution-plan::plan", "selfhost/pkg-resolution-plan::hash64?",
        "selfhost/pkg-resolution-plan::semver-policy", "selfhost/pkg-resolution-plan::should-resolve?",
        "selfhost/pkg-resolution-id::exact-map? request", "selfhost/hash::hash-term request",
    ):
        if marker not in module:
            fail(f"GenesisCode plan authority missing marker: {marker}")
    if f'    "{profile["sourceModule"]}"' not in manifest or profile["binding"] not in manifest:
        fail("toolchain manifest does not custody plan authority module and required binding")
    if f':path "{profile["sourceModule"]}"' not in artifact or profile["binding"] not in artifact:
        fail("published artifact does not contain plan authority module and binding")

    for marker in (
        "plan_authority: Value", "let plan_authority = environment", ".get(PLAN_BINDING)",
        'mod plan;', "PkgResolutionPlanError", "result field set mismatch",
    ):
        if marker not in adapter:
            fail(f"Rust shared authority adapter missing marker: {marker}")
    for marker in (
        "pub(crate) fn plan(",
        "decode_plan_result", "request-h",
        "semver selector and :semver-policy disagree", "PkgResolutionPlanError::Rejected",
        "result :code is outside the closed rejection inventory",
    ):
        if marker not in plan_adapter:
            fail(f"Rust plan adapter missing marker: {marker}")
    if adapter.count("load_selfhost_coreform_toolchain_v1_with_mode(") != 1:
        fail("plan and identity bindings must share exactly one artifact bootstrap context")

    parity_marker = '\n#[cfg(any(test, feature = "parity-oracle"))]\nfn plan_requirement_parity'
    if parity_marker not in planner:
        fail("test-only Rust planning oracle boundary missing")
    production = planner.split(parity_marker, 1)[0]
    for marker in (
        ".plan(req, has_existing)", "requires the artifact-loaded GenesisCode planning authority",
        "pub(crate) fn resolve_requirement(", "plan: PkgResolutionPlan", "match plan.selector",
    ):
        if marker not in planner:
            fail(f"production planning route missing marker: {marker}")
    for forbidden in (
        "pub(crate) fn parse_selector(", "fn semver_selection_policy(",
        "let inferred_strategy = gc_pkg::infer_strategy",
    ):
        if forbidden in production:
            fail(f"production route retains Rust planning oracle: {forbidden}")

    for text, markers, label in (
        (dispatch, ("plan_requirement(", "if !plan.should_resolve", "resolve_requirement("), "lock/update"),
        (validation, ("plan_requirement_for_strict_validation(", "match plan.selector"), "strict validation"),
        (install, ("plan_requirement(", "resolve_requirement("), "install hydration"),
    ):
        for marker in markers:
            if marker not in text:
                fail(f"{label} plan wiring missing marker: {marker}")
    if "let should_update = req.update_policy" in dispatch:
        fail("update dispatcher retains duplicated Rust update admission")
    for marker in (
        ".map(PkgResolutionIdentityAuthority::load)", '"core/pkg-low::lock"',
        '"core/pkg-low::update"', '"core/pkg-low::install"',
    ):
        if marker not in runner:
            fail(f"runner plan authority wiring missing marker: {marker}")

    row = next((item for item in ledger.get("semanticDecisions", [])
                if item.get("id") == "SD-PACKAGE-RESOLUTION"), None)
    if not row or row.get("currentLevel") != "H0":
        fail("SD-PACKAGE-RESOLUTION must remain truthful H0")
    for path in (
        profile["sourceModule"],
        "crates/gc_effects/src/pkg_resolution_identity_authority.rs",
        "crates/gc_effects/src/pkg_resolution_plan_authority.rs",
    ):
        if path not in row.get("productionAuthorityPaths", []):
            fail(f"semantic ledger missing plan production authority path: {path}")
    if profile["spec"] not in row.get("specAuthorityPaths", []):
        fail("semantic ledger missing plan authority specification")
    limitations = "\n".join(row.get("limitations", [])).lower()
    for marker in ("selector", "update", "semver", "graph", "remain"):
        if marker not in limitations:
            fail(f"semantic ledger lacks partial plan claim/residual marker: {marker}")
    if source_identity(profile["sourceModule"], module.encode()) != profile["sourceSha256"]:
        fail("plan authority source identity mismatch")


def validate_all(root, profile, schema, overrides=None, check_identity=True) -> None:
    validate_profile(profile, schema, check_identity)
    validate_sources(root, profile, overrides)


def self_test(root: Path, profile, schema) -> int:
    paths = [
        profile["sourceModule"], "selfhost/toolchain_manifest.gc", profile["artifact"],
        "crates/gc_effects/src/pkg_resolution_identity_authority.rs",
        "crates/gc_effects/src/pkg_resolution_plan_authority.rs",
        "crates/gc_effects/src/runner_vcs_pkg_helpers/pkg_resolution.rs",
        "crates/gc_effects/src/runner_vcs_pkg_helpers/pkg_resolution/lock_validation.rs",
        "crates/gc_effects/src/runner_cap_pkg_low/dispatch_resolution.rs",
        "crates/gc_effects/src/runner_cap_pkg_low/dispatch_resolution/install_verify.rs",
        "crates/gc_effects/src/runner.rs",
    ]
    sources = {path: source_text(root, path, {}) for path in paths}
    mutations = []

    def profile_mutation(name, value):
        changed = copy.deepcopy(profile)
        changed[name] = value
        changed["contentIdentitySha256"] = canonical_identity(changed)
        mutations.append((changed, {}, name))

    profile_mutation("binding", "core/pkg::legacy-resolution-plan")
    profile_mutation("decisionInventory", profile["decisionInventory"][:-1])
    profile_mutation("hostMechanisms", profile["hostMechanisms"][:-1])
    profile_mutation("hostOracle", {"parityOnly": False, "productionRequired": True, "removalTask": "R4.2.e"})
    profile_mutation("productionEntrypoints", ["genesis"])
    profile_mutation("nonclaims", profile["nonclaims"][:-1])
    profile_mutation("sourceSha256", "f" * 64)

    def source_mutation(path, old, new, name):
        if old not in sources[path]:
            fail(f"self-test marker absent for {name}")
        mutations.append((profile, {path: sources[path].replace(old, new, 1)}, name))

    source_mutation(profile["sourceModule"], "(def core/pkg::resolution-plan-authority", "(def core/pkg::legacy-plan", "source")
    source_mutation("selfhost/toolchain_manifest.gc", profile["binding"], "core/pkg::missing-plan", "manifest")
    if profile["binding"] not in sources[profile["artifact"]]:
        fail("self-test marker absent for artifact")
    mutations.append((
        profile,
        {profile["artifact"]: sources[profile["artifact"]].replace(
            profile["binding"], "core/pkg::missing-plan"
        )},
        "artifact",
    ))
    source_mutation("crates/gc_effects/src/pkg_resolution_plan_authority.rs", "semver selector and :semver-policy disagree", "semver accepted", "decoder")
    source_mutation("crates/gc_effects/src/runner_vcs_pkg_helpers/pkg_resolution.rs", ".plan(req, has_existing)", ".legacy_plan(req)", "production plan")
    source_mutation("crates/gc_effects/src/runner_vcs_pkg_helpers/pkg_resolution.rs", "match plan.selector", "match parse_selector_parity(&req.selector).unwrap()", "selector execution")
    source_mutation("crates/gc_effects/src/runner_cap_pkg_low/dispatch_resolution.rs", "if !plan.should_resolve", "if false", "update decision")
    source_mutation("crates/gc_effects/src/runner_vcs_pkg_helpers/pkg_resolution/lock_validation.rs", "match plan.selector", "match parse_selector_parity(&req.selector).unwrap()", "strict validation")
    source_mutation("crates/gc_effects/src/runner_cap_pkg_low/dispatch_resolution/install_verify.rs", "plan_requirement(", "legacy_plan(", "install")
    source_mutation("crates/gc_effects/src/runner.rs", ".map(PkgResolutionIdentityAuthority::load)", ".map(PkgLockReadAuthority::load)", "runner load")

    controls = 0
    for changed_profile, overrides, name in mutations:
        try:
            validate_all(root, changed_profile, schema, overrides, check_identity=True)
        except CheckError:
            controls += 1
        else:
            fail(f"negative control survived: {name}")
    if controls != 17:
        fail(f"negative control inventory drift: {controls}")
    return controls


def write_identities(path: Path, profile, root: Path) -> None:
    source_path = root / profile["sourceModule"]
    profile["sourceSha256"] = source_identity(profile["sourceModule"], source_path.read_bytes())
    profile["contentIdentitySha256"] = canonical_identity(profile)
    path.write_text(json.dumps(profile, indent=2) + "\n")


def main(argv=None) -> int:
    parser = argparse.ArgumentParser()
    parser.add_argument("--root", type=Path, required=True)
    parser.add_argument("--profile", type=Path, required=True)
    parser.add_argument("--schema", type=Path, required=True)
    parser.add_argument("--artifact", type=Path)
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
        if args.artifact and args.artifact.resolve() != (root / profile["artifact"]).resolve():
            fail("artifact argument does not match profile")
        controls = self_test(root, profile, schema) if args.self_test else 0
        print(
            "selfhost-pkg-resolution-plan-authority: ok "
            f"profile={profile['contentIdentitySha256']} controls={controls}"
        )
        return 0
    except CheckError as error:
        print(f"selfhost-pkg-resolution-plan-authority: {error}", file=sys.stderr)
        return 1


if __name__ == "__main__":
    raise SystemExit(main())
