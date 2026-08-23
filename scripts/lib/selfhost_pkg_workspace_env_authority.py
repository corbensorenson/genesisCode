#!/usr/bin/env python3
"""Independently verify gcpm workspace-environment semantic authority."""

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
    value = {}
    for key, item in pairs:
        if key in value:
            fail(f"duplicate JSON key: {key}")
        value[key] = item
    return value


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
    "selfhost/pkg_workspace_env_core_v1.gc",
    "selfhost/pkg_workspace_env_finalize_v1.gc",
    "selfhost/pkg_workspace_env_authority_v1.gc",
]
FIELDS = {
    "artifact", "auditDate", "binding", "contentIdentitySha256", "decisionInventory",
    "finalizeRequestKind", "hostMechanisms", "hostOracle", "independentVerifier",
    "kind", "nonclaims", "planRequestKind", "productionEntrypoints", "resultKind",
    "schema", "sourceModules", "sourceSha256", "spec", "version",
}
CONSTANTS = {
    "artifact": "selfhost/toolchain.gc",
    "binding": "core/pkg::workspace-env-authority",
    "decisionInventory": [
        "closed-workspace-lock-member-and-dependency-projection",
        "effective-policy-registry-toolchain-and-capability-selection",
        "canonical-environment-profile-provenance-and-wasi-descriptors",
        "complete-authority-bearing-environment-identity",
        "backend-bridge-and-effective-capability-binding",
        "ordered-environment-and-external-write-plan",
        "immutable-environment-root-and-public-result",
        "request-bound-two-phase-observation-validation",
    ],
    "finalizeRequestKind": "genesis/pkg-workspace-env-finalize-authority-request-v0.1",
    "hostMechanisms": [
        "bounded-workspace-lock-package-capability-and-toolchain-observation",
        "active-backend-store-and-path-prefix-observation",
        "artifact-only-bounded-two-phase-evaluation",
        "strict-request-bound-plan-result-and-body-decoding",
        "safe-path-joining-regular-file-checks-and-cryptographic-rechecks",
        "backend-launcher-planning-and-atomic-file-persistence",
        "filesystem-preflight-and-atomic-environment-directory-publication",
    ],
    "hostOracle": {
        "productionRequired": False,
        "reachability": "none-proven",
        "removalTask": "R4.2.e",
    },
    "independentVerifier": "scripts/lib/selfhost_pkg_workspace_env_authority.py",
    "kind": "genesis/selfhost-pkg-workspace-env-authority-v0.1",
    "planRequestKind": "genesis/pkg-workspace-env-plan-authority-request-v0.1",
    "productionEntrypoints": ["genesis"],
    "resultKind": "genesis/pkg-workspace-env-authority-result-v0.1",
    "schema": "docs/spec/SELFHOST_PKG_WORKSPACE_ENV_AUTHORITY_v0.1.schema.json",
    "sourceModules": SOURCE_MODULES,
    "spec": "docs/spec/SELFHOST_PKG_WORKSPACE_NEW_AUTHORITY_v0.1.md",
    "version": "0.1.0",
}
NONCLAIMS = {
    "backend-bridge-binary-semantic-authority",
    "bootstrap-fixpoint",
    "cross-root-crash-atomic-transaction-or-recovery",
    "filesystem-or-generic-path-policy-authority",
    "generic-toml-authority",
    "h2-workspace-closure",
    "manifest-authority",
    "r4-2-e-closure",
    "release-qualification",
    "sh-c-closure",
    "wasi-workspace-env-support",
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


def validate_profile(profile, schema) -> None:
    if set(profile) != FIELDS:
        fail("profile field closure drift")
    if (
        schema.get("$schema") != "https://json-schema.org/draft/2020-12/schema"
        or schema.get("type") != "object"
        or schema.get("additionalProperties") is not False
        or set(schema.get("required", [])) != FIELDS
        or set(schema.get("properties", {})) != FIELDS
    ):
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
    if canonical_identity(profile) != profile["contentIdentitySha256"]:
        fail("profile content identity mismatch")


def require_markers(subject: str, markers, label: str) -> None:
    for marker in markers:
        if marker not in subject:
            fail(f"{label} missing marker: {marker}")


def validate_sources(root: Path, profile, overrides=None) -> None:
    overrides = overrides or {}
    core = text(root, SOURCE_MODULES[0], overrides)
    finalizer = text(root, SOURCE_MODULES[1], overrides)
    authority = text(root, SOURCE_MODULES[2], overrides)
    manifest = text(root, "selfhost/toolchain_manifest.gc", overrides)
    artifact = text(root, profile["artifact"], overrides)
    adapter = text(root, "crates/gc_cli_driver/src/pkg_workspace_env_authority.rs", overrides)
    decoder = text(
        root, "crates/gc_cli_driver/src/pkg_workspace_env_authority/decode.rs", overrides
    )
    route = text(root, "crates/gc_cli_driver/src/pkg_workspace_ops_env.rs", overrides)
    materializer = text(
        root, "crates/gc_cli_driver/src/pkg_workspace_env_materialize.rs", overrides
    )
    backend = text(root, "crates/gc_cli_driver/src/pkg_workspace_ops_backend.rs", overrides)
    tests = text(root, "crates/gc_cli/tests/cli_pkg_workspace.rs", overrides)
    spec = text(root, profile["spec"], overrides)
    ledger = parse_json(
        text(root, "docs/spec/SEMANTIC_OWNERSHIP_LEDGER_v0.1.json", overrides), "ledger"
    )

    require_markers(core, [
        "(def selfhost/pkg-workspace-env::request-valid?",
        "(def selfhost/pkg-workspace-env::make-plan",
        "core/pkg::workspace-env-select-authority",
        "(def selfhost/pkg-workspace-env::observations-valid?",
        ":effective-policy", ":effective-registry", ":effective-toolchain",
        ":members-body", ":deps-body",
    ], "GenesisCode workspace-env core")
    require_markers(finalizer, [
        "(def selfhost/pkg-workspace-env::make-final", ":caps-policy-h",
        ":effective-policy", ":toolchain-path", ":backend-effective-caps-h",
        ":backend-bridge-sha256", '"caps-policy.backend.effective.toml"',
        '"wasi-http-bridge.gc"',
    ], "GenesisCode workspace-env finalizer")
    require_markers(authority, [
        "(def core/pkg::workspace-env-authority",
        profile["planRequestKind"], profile["finalizeRequestKind"], profile["resultKind"],
        "selfhost/pkg-workspace-env::make-plan",
        "selfhost/pkg-workspace-env::observations-valid?",
        "selfhost/pkg-workspace-env::make-final",
        "workspace environment plan substitution detected",
    ], "GenesisCode workspace-env authority")
    if authority.index("selfhost/pkg-workspace-env::make-plan") >= authority.index(
        "selfhost/pkg-workspace-env::observations-valid?"
    ):
        fail("workspace-env plan no longer precedes observation finalization")
    for marker in (profile["binding"], *SOURCE_MODULES):
        if marker not in manifest:
            fail(f"toolchain manifest missing workspace-env marker: {marker}")
        if marker not in artifact:
            fail(f"published artifact missing workspace-env marker: {marker}")

    require_markers(adapter, [
        'const AUTHORITY_BINDING: &str = "core/pkg::workspace-env-authority"',
        "pub(super) fn authorize<F>(", "crate::load_selfhost_toolchain",
        ".get(AUTHORITY_BINDING)", "decode_envelope(", "decode_plan(",
        "decode_environment(", "use decode::*;", "hash_term(&plan_request)",
        "validate_authorized_bodies(",
        'join("caps-policy.backend.effective.toml")',
    ], "strict workspace-env adapter")
    require_markers(decoder, [
        "pub(super) fn require_exact_fields(", "pub(super) fn require_string(",
        "pub(super) fn require_lower_hex64(", "pub(super) fn blake3_hex(",
        "pub(super) fn hex32(",
    ], "workspace-env strict decoder")
    if adapter.count("require_exact_fields(") != 6:
        fail("workspace-env exact-field check inventory drift")
    if adapter.count("validate_authorized_bodies(") != 2:
        fail("workspace-env body-validation call inventory drift")
    require_markers(route, [
        "pkg_workspace_env_select::load_workspace(workspace_file, profile)",
        "pkg_workspace_env_authority::authorize(cli, plan_request, out_dir",
        "plan_backend_env_bundle(workspace_file)",
        "pkg_workspace_env_materialize::commit(&authorized, backend_plan.as_ref())",
        "read_required_file(&caps_path, \"caps policy\")",
    ], "workspace-env production custody")
    for forbidden in (
        "RuntimeBackendContract", "canonical_env_body", "canonical_profile_body",
        "canonical_provenance_body", "render_workspace_toml", "build_wasi_http_bridge_config",
    ):
        if forbidden in route:
            fail(f"native workspace-env semantic oracle reachable: {forbidden}")
    if (root / "crates/gc_cli_driver/src/pkg_workspace_ops_manifest_helpers.rs").exists():
        fail("retired native workspace-env manifest helper remains reachable")

    require_markers(materializer, [
        "preflight(authorized, backend)?;", "materialize_backend_bridge(plan)?;",
        "materialize_external(authorized)?;", "materialize_environment_root(authorized)",
        "existing workspace environment file inventory mismatch",
        "immutable environment artifact differs", "preflight_mutable_file(",
        "std::fs::rename(&staging, &authorized.env_root)",
    ], "workspace-env materialization mechanism")
    ordered = [
        materializer.index("preflight(authorized, backend)?;"),
        materializer.index("materialize_backend_bridge(plan)?;"),
        materializer.index("materialize_external(authorized)?;"),
        materializer.index("materialize_environment_root(authorized)"),
    ]
    if ordered != sorted(ordered):
        fail("workspace-env preflight/write/publication order drift")
    require_markers(backend, [
        "pub(crate) fn plan_backend_env_bundle(",
        "pub(crate) fn materialize_backend_bridge(", "write_text_if_changed(",
    ], "backend plan/mechanism separation")
    if backend.index("pub(crate) fn plan_backend_env_bundle(") >= backend.index(
        "pub(crate) fn materialize_backend_bridge("
    ):
        fail("backend planning/materialization declaration order drift")

    require_markers(tests, [
        "gcpm_env_materializes_deterministic_profile_record",
        "gcpm_env_identity_binds_capability_policy_bytes",
        "gcpm_env_corrupt_immutable_root_rejects_before_external_write",
        "gcpm_env_symlinked_capability_policy_rejects_before_materialization",
        "gcpm_env_selection_invalid_selected_backend_rejects_before_materialization",
        "gcpm_env_backend_profile_materializes_effective_caps_with_bridge_digest",
        "assert_ne!(first_h, second_h)", 'b"sentinel\\n"',
    ], "workspace-env integration evidence")
    require_markers(spec, [
        "## Workspace environment authority contract", profile["planRequestKind"],
        profile["finalizeRequestKind"], "cross-root crash transaction",
    ], "workspace-env normative contract")

    rows = ledger.get("semanticDecisions")
    if not isinstance(rows, list):
        fail("ledger semanticDecisions missing")
    row = next((item for item in rows if item.get("id") == "SD-PACKAGE-WORKSPACE"), None)
    if not isinstance(row, dict):
        fail("ledger workspace decision missing")
    joined = json.dumps(row, sort_keys=True)
    require_markers(joined, [
        profile["kind"], profile["spec"], profile["independentVerifier"], *SOURCE_MODULES,
        "crates/gc_cli_driver/src/pkg_workspace_env_authority.rs",
        "crates/gc_cli_driver/src/pkg_workspace_env_authority/decode.rs",
        "crates/gc_cli_driver/src/pkg_workspace_env_materialize.rs",
        "GenesisCode exclusively owns gcpm env workspace, lock, member, dependency, descriptor, identity, and ordered write-plan decisions",
        "Filesystem preflight, atomic file writes, and atomic immutable environment-root publication remain host mechanisms",
    ], "workspace-env ownership ledger")


def validate_all(root: Path, profile, schema, overrides=None) -> None:
    overrides = overrides or {}
    validate_profile(profile, schema)
    if source_identity(root, overrides) != profile["sourceSha256"]:
        fail("profile source identity mismatch")
    validate_sources(root, profile, overrides)


def self_test(root: Path, profile, schema) -> int:
    paths = SOURCE_MODULES + [
        "selfhost/toolchain_manifest.gc", profile["artifact"],
        "crates/gc_cli_driver/src/pkg_workspace_env_authority.rs",
        "crates/gc_cli_driver/src/pkg_workspace_env_authority/decode.rs",
        "crates/gc_cli_driver/src/pkg_workspace_ops_env.rs",
        "crates/gc_cli_driver/src/pkg_workspace_env_materialize.rs",
        "crates/gc_cli_driver/src/pkg_workspace_ops_backend.rs",
        "crates/gc_cli/tests/cli_pkg_workspace.rs", profile["spec"],
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
        ("binding", "core/pkg::legacy-workspace-env"),
        ("decisionInventory", profile["decisionInventory"][:-1]),
        ("hostMechanisms", profile["hostMechanisms"][:-1]),
        ("hostOracle", {**profile["hostOracle"], "reachability": "production"}),
        ("nonclaims", profile["nonclaims"][:-1]),
        ("sourceSha256", "f" * 64),
    ):
        profile_mutation(name, value)

    def source_mutation(path, old, new, name):
        if old not in sources[path]:
            fail(f"self-test marker absent for {name}")
        mutations.append((profile, {path: sources[path].replace(old, new, 1)}, name))

    core, finalizer, authority = SOURCE_MODULES
    source_mutation(core, "(def selfhost/pkg-workspace-env::make-plan", "(def selfhost/pkg-workspace-env::legacy-plan", "plan")
    source_mutation(finalizer, ":caps-policy-h", ":legacy-caps-h", "caps identity")
    source_mutation(finalizer, ":effective-policy", ":legacy-policy", "policy identity")
    source_mutation(finalizer, ":backend-bridge-sha256", ":legacy-bridge-h", "bridge identity")
    source_mutation(finalizer, "(def selfhost/pkg-workspace-env::make-final", "(def selfhost/pkg-workspace-env::legacy-final", "finalize")
    source_mutation(authority, "(def core/pkg::workspace-env-authority", "(def core/pkg::legacy-env", "binding")
    source_mutation(authority, profile["finalizeRequestKind"], "genesis/legacy-finalize", "finalize kind")
    source_mutation(authority, "workspace environment plan substitution detected", "workspace environment accepted substitution", "substitution")
    source_mutation("selfhost/toolchain_manifest.gc", profile["binding"], "core/pkg::missing-env", "manifest")
    mutations.append((profile, {profile["artifact"]: sources[profile["artifact"]].replace(profile["binding"], "core/pkg::missing-env")}, "artifact"))
    adapter = "crates/gc_cli_driver/src/pkg_workspace_env_authority.rs"
    source_mutation(adapter, "crate::load_selfhost_toolchain", "crate::load_legacy_toolchain", "loader")
    source_mutation(adapter, "require_exact_fields(", "accept_open_fields(", "closure")
    source_mutation(adapter, "validate_authorized_bodies(", "trust_authorized_bodies(", "body validation")
    source_mutation(adapter, 'join("caps-policy.backend.effective.toml")', 'join("caps-policy.toml")', "effective path")
    decoder = "crates/gc_cli_driver/src/pkg_workspace_env_authority/decode.rs"
    source_mutation(decoder, "pub(super) fn require_exact_fields(", "pub(super) fn accept_open_fields(", "decoder closure")
    route = "crates/gc_cli_driver/src/pkg_workspace_ops_env.rs"
    source_mutation(route, "pkg_workspace_env_authority::authorize(cli, plan_request, out_dir", "legacy_env_authority(plan_request", "production custody")
    source_mutation(route, "read_required_file(&caps_path, \"caps policy\")", "std::fs::read(&caps_path)", "regular input")
    materializer = "crates/gc_cli_driver/src/pkg_workspace_env_materialize.rs"
    source_mutation(materializer, "preflight(authorized, backend)?;", "// preflight removed", "preflight")
    source_mutation(materializer, "immutable environment artifact differs", "overwrite immutable environment artifact", "immutability")
    source_mutation(materializer, "std::fs::rename(&staging, &authorized.env_root)", "std::fs::create_dir_all(&authorized.env_root)", "atomic root")
    backend = "crates/gc_cli_driver/src/pkg_workspace_ops_backend.rs"
    source_mutation(backend, "pub(crate) fn plan_backend_env_bundle(", "pub(crate) fn materialize_backend_env_bundle(", "backend planning")
    tests = "crates/gc_cli/tests/cli_pkg_workspace.rs"
    source_mutation(tests, "gcpm_env_identity_binds_capability_policy_bytes", "legacy_identity_test", "identity control")
    source_mutation(tests, "gcpm_env_corrupt_immutable_root_rejects_before_external_write", "legacy_preflight_test", "preflight control")
    source_mutation(tests, "gcpm_env_symlinked_capability_policy_rejects_before_materialization", "legacy_symlink_test", "symlink control")
    spec = profile["spec"]
    source_mutation(spec, "## Workspace environment authority contract", "## Legacy environment contract", "spec")
    ledger = "docs/spec/SEMANTIC_OWNERSHIP_LEDGER_v0.1.json"
    source_mutation(ledger, profile["kind"], "native-workspace-env", "ledger authority")

    controls = 0
    for changed_profile, overrides, name in mutations:
        try:
            validate_all(root, changed_profile, schema, overrides)
        except CheckError:
            controls += 1
        else:
            fail(f"negative control survived: {name}")
    if controls != len(mutations):
        fail(f"negative control inventory drift: {controls}/{len(mutations)}")
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
            "selfhost-pkg-workspace-env-authority: ok "
            f"profile={profile['contentIdentitySha256']} controls={controls}"
        )
    except CheckError as error:
        print(f"selfhost-pkg-workspace-env-authority: {error}", file=sys.stderr)
        return 1
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
