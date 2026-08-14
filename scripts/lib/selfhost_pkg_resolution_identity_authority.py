#!/usr/bin/env python3
"""Independent custody verifier for self-hosted package resolution identities."""

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
    "binding": "core/pkg::resolution-identity-authority",
    "decisionInventory": [
        "closed-requirement-identity-map", "deterministic-coreform-term-print",
        "newline-delimited-identity-bytes", "raw-blake3-requirement-fingerprint",
        "closed-enum-and-optional-field-admission", "request-bound-result-verdict",
    ],
    "hostMechanisms": [
        "artifact-only-authority-bootstrap-and-bounded-evaluation",
        "typed-requirement-term-transport", "strict-result-contradiction-checking",
        "effect-log-and-diagnostic-rendering",
    ],
    "hostOracle": {"parityOnly": True, "productionRequired": False, "removalTask": "R4.2.e"},
    "independentVerifier": "scripts/lib/selfhost_pkg_resolution_identity_authority.py",
    "kind": "genesis/selfhost-pkg-resolution-identity-authority-v0.1",
    "productionEntrypoints": ["genesis", "genesis_wasi"],
    "requestKind": "genesis/pkg-resolution-identity-request-v0.1",
    "resultKind": "genesis/pkg-resolution-identity-result-v0.1",
    "schema": "docs/spec/SELFHOST_PKG_RESOLUTION_IDENTITY_AUTHORITY_v0.1.schema.json",
    "sourceModule": "selfhost/pkg_resolution_identity_authority_v1.gc",
    "spec": "docs/spec/SELFHOST_PKG_RESOLUTION_IDENTITY_AUTHORITY_v0.1.md",
    "version": "0.1.0",
}
NONCLAIMS = {
    "bootstrap-fixpoint", "graph-resolution-authority", "h2-package-resolution",
    "r4-2-e-closure", "registry-authority", "release-qualification",
    "selector-resolution-authority", "sh-c-closure", "workspace-authority",
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
    validation = source_text(
        root, "crates/gc_effects/src/runner_vcs_pkg_helpers/pkg_resolution/lock_validation.rs", overrides
    )
    resolution = source_text(
        root, "crates/gc_effects/src/runner_cap_pkg_low/dispatch_resolution.rs", overrides
    )
    workflow = source_text(
        root,
        "crates/gc_effects/src/runner_cap_pkg_low/dispatch_resolution/workflow.rs",
        overrides,
    )
    install = source_text(
        root, "crates/gc_effects/src/runner_cap_pkg_low/dispatch_resolution/install_verify.rs", overrides
    )
    runner = source_text(root, "crates/gc_effects/src/runner.rs", overrides)
    ledger = load_json(root / "docs/spec/SEMANTIC_OWNERSHIP_LEDGER_v0.1.json")

    for marker in (
        "(def core/pkg::resolution-identity-authority", profile["requestKind"],
        profile["resultKind"], "selfhost/pkg-resolution-id::fingerprint",
        "selfhost/pkg-resolution-id::exact-map? request",
        "selfhost/printer::print-term identity", "core/crypto::blake3",
        "selfhost/hash::hash-term request",
    ):
        if marker not in module:
            fail(f"GenesisCode identity authority missing marker: {marker}")
    if f'    "{profile["sourceModule"]}"' not in manifest or profile["binding"] not in manifest:
        fail("toolchain manifest does not custody identity authority module and binding")
    if f':path "{profile["sourceModule"]}"' not in artifact or profile["binding"] not in artifact:
        fail("published artifact does not contain identity authority module and binding")
    for marker in (
        "pub(crate) struct PkgResolutionIdentityAuthority", "SelfhostBootstrapMode::ArtifactOnly",
        "const STEP_LIMIT: u64 = 20_000_000", "const ALLOC_LIMIT: u64 = 40_000_000",
        "max_bytes_len: Some(4 * 1024 * 1024)", "max_vec_len: Some(65_536)",
        "decode_identity_result", "result field set mismatch", "request-h",
        "result :fingerprint must be lowercase BLAKE3 hex64",
    ):
        if marker not in adapter:
            fail(f"Rust identity adapter missing marker: {marker}")

    parity_marker = '\n#[cfg(any(test, feature = "parity-oracle"))]\nfn compute_requirement_fingerprint_parity'
    if parity_marker not in validation:
        fail("test-only Rust fingerprint oracle boundary missing")
    production = validation.split(parity_marker, 1)[0]
    for marker in (
        ".fingerprint(req, snapshot, commit)", "core/pkg/authority-error",
        "requires the artifact-loaded GenesisCode identity authority",
    ):
        if marker not in production:
            fail(f"production fingerprint route missing marker: {marker}")
    if "blake3::hash" in production or "print_term(&Term::Map(m))" in production:
        fail("production fingerprint route retains Rust identity oracle")
    for marker in (
        ".map(PkgResolutionIdentityAuthority::load)", '"core/pkg-low::lock"',
        '"core/pkg-low::update"', '"core/pkg-low::install"',
        "pkg_resolution_identity_authority.as_mut()",
    ):
        if marker not in runner:
            fail(f"runner identity authority wiring missing marker: {marker}")
    if (resolution.count("identity_authority.as_deref_mut()") != 4
            or resolution.count("execute_workflow(") != 2
            or resolution.count("finalize_workflow(") != 2):
        fail("lock/update workflow authority forwarding inventory drift")
    if (workflow.count("plan_requirement(") != 2
            or workflow.count("resolve_requirement(") != 2
            or workflow.count("validate_locked_entries_strict(") != 1):
        fail("workflow identity and validation forwarding inventory drift")
    install_plan = install.find("plan_requirement(Some(authority)")
    install_resolve = install.find("resolve_requirement(")
    if (install.count("identity_authority.as_deref_mut()") != 1
            or install.count("plan_requirement(Some(authority)") != 1
            or install.count("Some(authority),") < 2
            or min(install_plan, install_resolve) < 0
            or not install_plan < install_resolve):
        fail("install hydration does not forward identity authority")

    row = next((item for item in ledger.get("semanticDecisions", [])
                if item.get("id") == "SD-PACKAGE-RESOLUTION"), None)
    if not row or row.get("currentLevel") != "H0":
        fail("SD-PACKAGE-RESOLUTION must remain truthful H0")
    for path in (profile["sourceModule"], "crates/gc_effects/src/pkg_resolution_identity_authority.rs"):
        if path not in row.get("productionAuthorityPaths", []):
            fail(f"semantic ledger missing production authority path: {path}")
    if profile["spec"] not in row.get("specAuthorityPaths", []):
        fail("semantic ledger missing identity authority specification")
    limitations = "\n".join(row.get("limitations", [])).lower()
    if "fingerprint" not in limitations or "graph" not in limitations or "remain" not in limitations:
        fail("semantic ledger lacks partial identity claim and residual graph limitation")
    if source_identity(profile["sourceModule"], module.encode()) != profile["sourceSha256"]:
        fail("identity authority source identity mismatch")


def validate_all(root, profile, schema, overrides=None, check_identity=True) -> None:
    validate_profile(profile, schema, check_identity)
    validate_sources(root, profile, overrides)


def self_test(root: Path, profile, schema) -> int:
    paths = [
        profile["sourceModule"], "selfhost/toolchain_manifest.gc", profile["artifact"],
        "crates/gc_effects/src/pkg_resolution_identity_authority.rs",
        "crates/gc_effects/src/runner_vcs_pkg_helpers/pkg_resolution/lock_validation.rs",
        "crates/gc_effects/src/runner_cap_pkg_low/dispatch_resolution.rs",
        "crates/gc_effects/src/runner_cap_pkg_low/dispatch_resolution/workflow.rs",
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

    profile_mutation("binding", "core/pkg::legacy-resolution-identity")
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

    source_mutation(profile["sourceModule"], "selfhost/printer::print-term identity", "legacy-print identity", "source")
    source_mutation("selfhost/toolchain_manifest.gc", profile["sourceModule"], "selfhost/missing.gc", "manifest")
    source_mutation(profile["artifact"], f':path "{profile["sourceModule"]}"', ':path "selfhost/missing.gc"', "artifact")
    source_mutation("crates/gc_effects/src/pkg_resolution_identity_authority.rs", "result field set mismatch", "result accepted", "decoder")
    source_mutation("crates/gc_effects/src/runner_vcs_pkg_helpers/pkg_resolution/lock_validation.rs", ".fingerprint(req, snapshot, commit)", ".legacy_fingerprint(req)", "production route")
    source_mutation("crates/gc_effects/src/runner_cap_pkg_low/dispatch_resolution.rs", "identity_authority.as_deref_mut()", "None", "resolution forwarding")
    source_mutation("crates/gc_effects/src/runner_cap_pkg_low/dispatch_resolution/workflow.rs", "plan_requirement(", "legacy_requirement_plan(", "workflow forwarding")
    source_mutation("crates/gc_effects/src/runner_cap_pkg_low/dispatch_resolution/install_verify.rs", "plan_requirement(Some(authority)", "plan_requirement(None", "install forwarding")
    source_mutation("crates/gc_effects/src/runner.rs", ".map(PkgResolutionIdentityAuthority::load)", ".map(PkgLockReadAuthority::load)", "runner load")

    controls = 0
    for changed_profile, overrides, name in mutations:
        try:
            validate_all(root, changed_profile, schema, overrides, check_identity=True)
        except CheckError:
            controls += 1
        else:
            fail(f"negative control survived: {name}")
    if controls != 16:
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
            "selfhost-pkg-resolution-identity-authority: ok "
            f"profile={profile['contentIdentitySha256']} controls={controls}"
        )
        return 0
    except CheckError as error:
        print(f"selfhost-pkg-resolution-identity-authority: {error}", file=sys.stderr)
        return 1


if __name__ == "__main__":
    raise SystemExit(main())
