#!/usr/bin/env python3
"""Independent custody verifier for package scaffold workflow authority."""

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
    "selfhost/pkg_scaffold_core_v1.gc",
    "selfhost/pkg_scaffold_render_v1.gc",
    "selfhost/pkg_scaffold_authority_v1.gc",
]
FIELDS = {
    "artifact", "auditDate", "binding", "contentIdentitySha256", "decisionInventory",
    "hostMechanisms", "hostOracle", "independentVerifier", "kind", "nonclaims",
    "productionEntrypoints", "requestKind", "resultKind", "schema", "sourceModules",
    "sourceSha256", "spec", "version",
}
CONSTANTS = {
    "artifact": "selfhost/toolchain.gc",
    "binding": "core/pkg::scaffold-authority",
    "decisionInventory": [
        "ascii-identifier-normalization",
        "closed-six-archetype-admission",
        "runtime-backend-alias-default-and-release-selection",
        "primary-target-and-mobile-secondary-target-selection",
        "canonical-six-dynamic-document-rendering",
        "identity-pinned-four-static-template-admission",
        "fixed-ten-file-order-and-body-identities",
        "aggregate-scaffold-identity-and-exact-report",
    ],
    "hostMechanisms": [
        "artifact-only-bounded-authority-evaluation",
        "four-static-capability-template-byte-observations",
        "strict-request-bound-result-and-cross-document-decoding",
        "whole-plan-path-conflict-type-and-symlink-preflight",
        "exact-byte-temporary-file-persistence-and-atomic-per-file-rename",
    ],
    "hostOracle": {"parityOnly": True, "productionRequired": False, "removalTask": "R4.2.e"},
    "independentVerifier": "scripts/lib/selfhost_pkg_scaffold_authority.py",
    "kind": "genesis/selfhost-pkg-scaffold-authority-v0.1",
    "productionEntrypoints": ["genesis"],
    "requestKind": "genesis/pkg-scaffold-authority-request-v0.1",
    "resultKind": "genesis/pkg-scaffold-authority-result-v0.1",
    "schema": "docs/spec/SELFHOST_PKG_SCAFFOLD_AUTHORITY_v0.1.schema.json",
    "sourceModules": SOURCE_MODULES,
    "spec": "docs/spec/SELFHOST_PKG_SCAFFOLD_AUTHORITY_v0.1.md",
    "version": "0.1.0",
}
NONCLAIMS = {
    "bootstrap-fixpoint", "crash-atomic-multi-file-commit",
    "filesystem-or-path-policy-authority", "generic-toml-authority",
    "h2-workspace-closure", "r4-2-e-closure", "release-qualification",
    "sh-c-closure", "static-capability-template-generation", "wasi-scaffold-support",
    "workspace-init-migration-environment-task-or-manifest-authority",
}
STATIC_HASHES = {
    "c59cc9fc2d22e351df9f1ca0993f5287747dd04424dd2ab29dc9c40b5feeaebe",
    "263a3a57675d9f02d3b7f3e63e567556e26e9a8826c9c2f18a3de2608707bc1f",
    "facc334d775e73441b8861a5119af13be4b4f53fa548e039d99de28a7a78a388",
    "df2d9cd700e2e45ee809db493119db52882baf94af8fc1b6f343d3de97d491c7",
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


def require_markers(subject: str, markers, label: str) -> None:
    for marker in markers:
        if marker not in subject:
            fail(f"{label} missing marker: {marker}")


def validate_sources(root: Path, profile, overrides=None) -> None:
    overrides = overrides or {}
    modules = "\n".join(text(root, path, overrides) for path in SOURCE_MODULES)
    manifest = text(root, "selfhost/toolchain_manifest.gc", overrides)
    artifact = text(root, profile["artifact"], overrides)
    adapter = text(root, "crates/gc_cli_driver/src/pkg_scaffold.rs", overrides)
    route = text(root, "crates/gc_cli_driver/src/cmd_pkg/local_workspace_ops.rs", overrides)
    parity = text(root, "crates/gc_cli_driver/src/pkg_scaffold/parity.rs", overrides)
    tests = text(root, "crates/gc_cli/tests/cli_pkg_scaffold.rs", overrides)
    ledger = parse_json(
        text(root, "docs/spec/SEMANTIC_OWNERSHIP_LEDGER_v0.1.json", overrides), "ledger"
    )

    require_markers(modules, [
        "(def core/pkg::scaffold-authority", profile["requestKind"], profile["resultKind"],
        "selfhost/pkg-scaffold::normalize-loop", "selfhost/pkg-scaffold::toml-string",
        "selfhost/pkg-scaffold::archetype?", "selfhost/pkg-scaffold::default-backend",
        "selfhost/pkg-scaffold::release-backend", "selfhost/pkg-scaffold::primary-target",
        "selfhost/pkg-scaffold::workspace-body", "selfhost/pkg-scaffold::lock-body",
        "selfhost/pkg-scaffold::scaffold-h", "static scaffold template identity mismatch",
    ], "GenesisCode scaffold authority")
    for digest in STATIC_HASHES:
        if digest not in modules:
            fail(f"GenesisCode static-template identity missing: {digest}")
    for source in SOURCE_MODULES:
        if source not in manifest:
            fail(f"toolchain manifest missing scaffold module: {source}")
    if profile["binding"] not in manifest:
        fail("toolchain manifest missing scaffold binding")
    for marker in (profile["binding"], *SOURCE_MODULES):
        if marker not in artifact:
            fail(f"published artifact missing scaffold marker: {marker}")

    require_markers(adapter, [
        "const AUTHORITY_BINDING", ".get(AUTHORITY_BINDING)",
        "decode_authorized_scaffold(", "require_exact_fields(",
        "WorkspaceConfig::from_toml_str", "GenesisLock::from_toml_str",
        "scaffold workspace defaults contradict request",
        "scaffold lock registry contradicts request", "preflight_scaffold(",
        "preflight_directory_chain(", "file_type().is_symlink()",
        "scaffold destination is not a regular file", "atomic_write_text(",
        "remove_file(&candidate)", "remove_file(&tmp)",
    ], "strict scaffold adapter")
    if adapter.count("file_type().is_symlink()") != 2:
        fail("scaffold symlink boundary inventory drift")
    production = adapter[adapter.index("pub(crate) fn handle_scaffold("):adapter.index("fn decode_authorized_scaffold(")]
    plan_at = production.find("decode_authorized_scaffold(")
    preflight_at = production.find("preflight_scaffold(")
    write_at = production.find("write_scaffold_file(")
    if min(plan_at, preflight_at, write_at) < 0 or not plan_at < preflight_at < write_at:
        fail("scaffold authority/preflight/write causal order drift")
    if "handle_scaffold_parity" in production:
        fail("native scaffold fallback reachable in production")
    require_markers(route, ["pkg_scaffold::handle_scaffold("], "scaffold route")
    require_markers(parity, [
        "pub(super) fn handle_scaffold_parity(",
        "retained_oracle_sample_has_stable_file_identities",
        "aaf0e92bbba88301783207edfe8d637cf3bc9429e8f3b6ff3042b6507c36ca1f",
    ], "retained scaffold oracle")
    for digest in STATIC_HASHES:
        if digest not in parity:
            fail(f"retained oracle static identity missing: {digest}")
    require_markers(adapter, [
        "#[cfg(any(test, feature = \"parity-harness\"))]",
        "#[path = \"pkg_scaffold/parity.rs\"]",
    ], "compile-time parity custody")
    require_markers(tests, [
        "gcpm_scaffold_covers_closed_archetype_decision_matrix",
        "gcpm_scaffold_round_trips_escaped_toml_metadata",
        "gcpm_scaffold_rejects_invalid_backend_without_mutation",
        "gcpm_scaffold_preflights_late_collision_before_any_write",
        "gcpm_scaffold_rejects_parent_symlink_without_external_write",
    ], "scaffold integration evidence")

    rows = ledger.get("semanticDecisions")
    if not isinstance(rows, list):
        fail("ledger semanticDecisions missing")
    row = next((item for item in rows if item.get("id") == "SD-PACKAGE-WORKSPACE"), None)
    if not isinstance(row, dict):
        fail("ledger workspace decision missing")
    joined = json.dumps(row, sort_keys=True)
    require_markers(joined, [
        "genesis/selfhost-pkg-scaffold-authority-v0.1", profile["spec"],
        profile["independentVerifier"], *SOURCE_MODULES,
        "crates/gc_cli_driver/src/pkg_scaffold.rs",
        "crates/gc_cli/tests/cli_pkg_scaffold.rs",
        "Workspace init, migration, environment, task, and manifest decisions remain host-authoritative",
    ], "workspace ownership ledger")


def validate_all(root: Path, profile, schema, overrides=None) -> None:
    overrides = overrides or {}
    validate_profile(profile, schema)
    if source_identity(root, overrides) != profile["sourceSha256"]:
        fail("profile source identity mismatch")
    validate_sources(root, profile, overrides)


def self_test(root: Path, profile, schema) -> int:
    paths = SOURCE_MODULES + [
        "selfhost/toolchain_manifest.gc", profile["artifact"],
        "crates/gc_cli_driver/src/pkg_scaffold.rs",
        "crates/gc_cli_driver/src/cmd_pkg/local_workspace_ops.rs",
        "crates/gc_cli_driver/src/pkg_scaffold/parity.rs",
        "crates/gc_cli/tests/cli_pkg_scaffold.rs",
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
        ("binding", "core/pkg::legacy-scaffold"),
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

    source_mutation(SOURCE_MODULES[-1], "(def core/pkg::scaffold-authority", "(def core/pkg::legacy-scaffold", "source")
    source_mutation(SOURCE_MODULES[-1], sorted(STATIC_HASHES)[0], "0" * 64, "static identity")
    source_mutation("selfhost/toolchain_manifest.gc", profile["binding"], "core/pkg::missing-scaffold", "manifest")
    source_mutation("crates/gc_cli_driver/src/pkg_scaffold.rs", ".get(AUTHORITY_BINDING)", ".get(\"native\")", "loader")
    source_mutation("crates/gc_cli_driver/src/pkg_scaffold.rs", "preflight_scaffold(args.root", "write_scaffold_file(args.root", "preflight")
    source_mutation("crates/gc_cli_driver/src/pkg_scaffold.rs", "file_type().is_symlink()", "file_type().is_file()", "symlink")
    source_mutation("crates/gc_cli_driver/src/pkg_scaffold.rs", "WorkspaceConfig::from_toml_str", "WorkspaceConfig::empty", "cross document")
    source_mutation("crates/gc_cli_driver/src/pkg_scaffold/parity.rs", "pub(super) fn handle_scaffold_parity(", "pub(super) fn handle_scaffold(", "parity")
    source_mutation("crates/gc_cli/tests/cli_pkg_scaffold.rs", "gcpm_scaffold_preflights_late_collision_before_any_write", "legacy_collision_test", "integration")
    source_mutation("docs/spec/SEMANTIC_OWNERSHIP_LEDGER_v0.1.json", "genesis/selfhost-pkg-scaffold-authority-v0.1", "native-scaffold", "ledger")

    controls = 0
    for changed_profile, overrides, name in mutations:
        try:
            validate_all(root, changed_profile, schema, overrides)
        except CheckError:
            controls += 1
        else:
            fail(f"negative control survived: {name}")
    if controls != 16:
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
            "selfhost-pkg-scaffold-authority: ok "
            f"profile={profile['contentIdentitySha256']} controls={controls}"
        )
        return 0
    except CheckError as error:
        print(f"selfhost-pkg-scaffold-authority: {error}", file=sys.stderr)
        return 1


if __name__ == "__main__":
    raise SystemExit(main())
