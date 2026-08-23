#!/usr/bin/env python3
"""Independent custody verifier for package workspace-new workflow authority."""

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


SOURCE_MODULES = ["selfhost/pkg_workspace_new_authority_v1.gc"]
FIELDS = {
    "artifact", "auditDate", "binding", "contentIdentitySha256", "decisionInventory",
    "hostMechanisms", "hostOracle", "independentVerifier", "kind", "nonclaims",
    "productionEntrypoints", "requestKind", "resultKind", "schema", "sourceModules",
    "sourceSha256", "spec", "version",
}
CONSTANTS = {
    "artifact": "selfhost/toolchain.gc",
    "binding": "core/pkg::workspace-new-authority",
    "decisionInventory": [
        "bounded-member-spec-parsing-and-default-root-member",
        "legacy-exact-toml-string-escaping",
        "canonical-workspace-default-and-closed-profile-rendering",
        "canonical-empty-v2-lock-rendering",
        "closed-active-runtime-backend-observation-admission",
        "fixed-two-file-order-and-body-identities",
        "exact-workspace-new-report",
    ],
    "hostMechanisms": [
        "artifact-only-bounded-authority-evaluation",
        "active-runtime-backend-profile-observation",
        "strict-request-bound-result-and-cross-document-decoding",
        "two-destination-ancestor-type-and-symlink-preflight",
        "exact-byte-temporary-file-persistence-and-atomic-per-file-rename",
    ],
    "hostOracle": {"parityOnly": True, "productionRequired": False, "removalTask": "R4.2.e"},
    "independentVerifier": "scripts/lib/selfhost_pkg_workspace_new_authority.py",
    "kind": "genesis/selfhost-pkg-workspace-new-authority-v0.1",
    "productionEntrypoints": ["genesis"],
    "requestKind": "genesis/pkg-workspace-new-authority-request-v0.1",
    "resultKind": "genesis/pkg-workspace-new-authority-result-v0.1",
    "schema": "docs/spec/SELFHOST_PKG_WORKSPACE_NEW_AUTHORITY_v0.1.schema.json",
    "sourceModules": SOURCE_MODULES,
    "spec": "docs/spec/SELFHOST_PKG_WORKSPACE_NEW_AUTHORITY_v0.1.md",
    "version": "0.1.0",
}
NONCLAIMS = {
    "bootstrap-fixpoint", "crash-atomic-two-file-commit",
    "filesystem-or-path-policy-authority", "generic-toml-or-path-authority",
    "h2-workspace-closure", "r4-2-e-closure", "release-qualification",
    "sh-c-closure", "wasi-workspace-new-support",
    "workspace-remove-migrate-environment-task-manifest-or-scaffold-authority",
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
    module = text(root, SOURCE_MODULES[0], overrides)
    manifest = text(root, "selfhost/toolchain_manifest.gc", overrides)
    artifact = text(root, profile["artifact"], overrides)
    adapter = text(root, "crates/gc_cli_driver/src/pkg_workspace_new.rs", overrides)
    shared_writer = text(root, "crates/gc_cli_driver/src/pkg_scaffold.rs", overrides)
    route = text(root, "crates/gc_cli_driver/src/cmd_pkg/local_workspace_ops.rs", overrides)
    custody = text(root, "crates/gc_cli_driver/src/pkg_workspace_ops.rs", overrides)
    tests = text(root, "crates/gc_cli/tests/cli_pkg_workspace.rs", overrides)
    ledger = parse_json(
        text(root, "docs/spec/SEMANTIC_OWNERSHIP_LEDGER_v0.1.json", overrides), "ledger"
    )

    require_markers(module, [
        "(def core/pkg::workspace-new-authority", profile["requestKind"], profile["resultKind"],
        "selfhost/pkg-workspace-new::member", "selfhost/pkg-workspace-new::members-loop",
        "selfhost/pkg-workspace-new::toml-control-fragment",
        "selfhost/pkg-workspace-new::c1-control?", "b\"0123456789ABCDEF\"",
        "selfhost/pkg-workspace-new::workspace-body", "selfhost/pkg-workspace-new::lock-body",
        "selfhost/pkg-scaffold::hash-body", "core/pkg/bad-workspace-new",
    ], "GenesisCode workspace-new authority")
    for source in SOURCE_MODULES:
        if source not in manifest:
            fail(f"toolchain manifest missing workspace-new module: {source}")
    if profile["binding"] not in manifest:
        fail("toolchain manifest missing workspace-new binding")
    for marker in (profile["binding"], *SOURCE_MODULES):
        if marker not in artifact:
            fail(f"published artifact missing workspace-new marker: {marker}")

    require_markers(adapter, [
        "const AUTHORITY_BINDING", ".get(AUTHORITY_BINDING)", "decode_authorized(",
        "require_exact_fields(", "blake3::hash(body.as_bytes())",
        "WorkspaceConfig::from_toml_str", "GenesisLock::from_toml_str",
        "workspace-new profile inventory is not closed",
        "workspace-new lock registry contradicts request", "preflight_paths(",
        "preflight_directory_chain(parent)", "file_type().is_symlink()",
        "workspace-new destination is not a regular file", "atomic_write_text(lock",
        "atomic_write_text(workspace_file",
    ], "strict workspace-new adapter")
    if adapter.count("file_type().is_symlink()") != 1:
        fail("workspace-new symlink boundary inventory drift")
    production = adapter[adapter.index("pub(super) fn handle_new("):adapter.index("fn decode_authorized(")]
    decode_at = production.find("decode_authorized(")
    preflight_at = production.find("preflight_paths(")
    lock_write_at = production.find("atomic_write_text(lock")
    workspace_write_at = production.find("atomic_write_text(workspace_file")
    if (min(decode_at, preflight_at, lock_write_at, workspace_write_at) < 0
            or not decode_at < preflight_at < lock_write_at < workspace_write_at):
        fail("workspace-new authority/preflight/write causal order drift")
    if "handle_new_parity" in production or "WorkspaceConfig::empty" in production:
        fail("native workspace-new fallback reachable in production")
    require_markers(shared_writer, [
        "pub(crate) fn preflight_directory_chain(", "pub(crate) fn atomic_write_text(",
        "remove_file(&candidate)", "remove_file(&tmp)",
    ], "shared exact-byte writer")
    require_markers(route, ["pkg_workspace_ops::handle_new(", "cli,"], "workspace-new route")
    require_markers(custody, [
        "#[cfg(any(test, feature = \"parity-harness\"))]", "fn handle_new_parity(",
        "workspace_new_retained_oracle_has_stable_identities",
        "649135d46f7c7e78cc52326dbe915f42a4c521232ff2fdb24c219992654ab5c2",
        "1913610e4cae447230fcaa5ca6a32449f0a25d76d9beec7796d2b1f18a4d85ea",
    ], "retained workspace-new oracle")
    require_markers(tests, [
        "gcpm_new_preserves_member_order_and_closed_profiles",
        "gcpm_new_toml_escapes_dynamic_values_losslessly",
        "gcpm_new_rejects_malformed_member_without_writes",
        "gcpm_new_rejects_identical_destinations_without_writes",
        "gcpm_new_preflights_workspace_symlink_before_lock_write",
    ], "workspace-new integration evidence")

    rows = ledger.get("semanticDecisions")
    if not isinstance(rows, list):
        fail("ledger semanticDecisions missing")
    row = next((item for item in rows if item.get("id") == "SD-PACKAGE-WORKSPACE"), None)
    if not isinstance(row, dict):
        fail("ledger workspace decision missing")
    joined = json.dumps(row, sort_keys=True)
    require_markers(joined, [
        profile["kind"], profile["spec"], profile["independentVerifier"], *SOURCE_MODULES,
        "crates/gc_cli_driver/src/pkg_workspace_new.rs",
        "crates/gc_cli/tests/cli_pkg_workspace.rs",
        "Manifest decisions remain host-authoritative; generic TOML and path authority, filesystem policy, cross-root crash-atomic commit and recovery, backend bridge binary semantics, and WASI workspace command support remain unproven, so SD-PACKAGE-WORKSPACE remains H0",
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
        "crates/gc_cli_driver/src/pkg_workspace_new.rs",
        "crates/gc_cli_driver/src/pkg_scaffold.rs",
        "crates/gc_cli_driver/src/cmd_pkg/local_workspace_ops.rs",
        "crates/gc_cli_driver/src/pkg_workspace_ops.rs",
        "crates/gc_cli/tests/cli_pkg_workspace.rs",
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
        ("binding", "core/pkg::legacy-workspace-new"),
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

    source_mutation(SOURCE_MODULES[0], "(def core/pkg::workspace-new-authority", "(def core/pkg::legacy-workspace-new", "source")
    source_mutation(SOURCE_MODULES[0], "b\"0123456789ABCDEF\"", "b\"0123456789abcdef\"", "escape alphabet")
    source_mutation("selfhost/toolchain_manifest.gc", profile["binding"], "core/pkg::missing-workspace-new", "manifest")
    mutations.append((profile, {
        profile["artifact"]: sources[profile["artifact"]].replace(
            profile["binding"], "core/pkg::missing-workspace-new"
        )
    }, "artifact"))
    source_mutation("crates/gc_cli_driver/src/pkg_workspace_new.rs", ".get(AUTHORITY_BINDING)", ".get(\"native\")", "loader")
    source_mutation("crates/gc_cli_driver/src/pkg_workspace_new.rs", "decode_authorized(", "decode_unchecked(", "decoder")
    source_mutation("crates/gc_cli_driver/src/pkg_workspace_new.rs", "preflight_paths(lock, workspace_file)?", "Ok(())?", "preflight")
    source_mutation("crates/gc_cli_driver/src/pkg_workspace_new.rs", "file_type().is_symlink()", "file_type().is_file()", "symlink")
    source_mutation("crates/gc_cli_driver/src/pkg_workspace_new.rs", "WorkspaceConfig::from_toml_str", "WorkspaceConfig::empty", "cross document")
    source_mutation("crates/gc_cli_driver/src/pkg_scaffold.rs", "pub(crate) fn atomic_write_text(", "fn native_write_text(", "writer")
    source_mutation("crates/gc_cli_driver/src/pkg_workspace_ops.rs", "fn handle_new_parity(", "fn handle_new(", "parity")
    source_mutation("crates/gc_cli/tests/cli_pkg_workspace.rs", "gcpm_new_preflights_workspace_symlink_before_lock_write", "legacy_symlink_test", "integration")
    source_mutation("docs/spec/SEMANTIC_OWNERSHIP_LEDGER_v0.1.json", profile["kind"], "native-workspace-new", "ledger")

    controls = 0
    for changed_profile, overrides, name in mutations:
        try:
            validate_all(root, changed_profile, schema, overrides)
        except CheckError:
            controls += 1
        else:
            fail(f"negative control survived: {name}")
    if controls != 19:
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
            "selfhost-pkg-workspace-new-authority: ok "
            f"profile={profile['contentIdentitySha256']} controls={controls}"
        )
        return 0
    except CheckError as error:
        print(f"selfhost-pkg-workspace-new-authority: {error}", file=sys.stderr)
        return 1


if __name__ == "__main__":
    raise SystemExit(main())
