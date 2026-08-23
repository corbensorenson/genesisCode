#!/usr/bin/env python3
"""Independently verify the package-manifest authority cutover."""

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
        return parse_json(path.read_text(encoding="utf-8"), str(path))
    except OSError as error:
        fail(f"cannot read {path}: {error}")


SOURCE_MODULES = [
    "selfhost/pkg_package_manifest_core_v1.gc",
    "selfhost/pkg_package_manifest_config_v1.gc",
    "selfhost/pkg_package_manifest_authority_v1.gc",
]
FIELDS = {
    "artifact", "auditDate", "binding", "contentIdentitySha256", "decisionInventory",
    "hostMechanisms", "hostOracle", "independentVerifier", "kind", "nonclaims",
    "productionEntrypoints", "requestKind", "resultKind", "schema", "sourceModules",
    "sourceSha256", "spec", "version",
}
CONSTANTS = {
    "artifact": "selfhost/toolchain.gc",
    "binding": "core/pkg::package-manifest-authority",
    "decisionInventory": [
        "documented-pre-schema-and-exact-schema-one-admission",
        "required-field-and-legacy-default-normalization",
        "module-dependency-and-capability-policy-portable-path-admission",
        "module-dependency-hash-and-obligation-suite-type-normalization",
        "limits-budgets-property-and-graphics-configuration-normalization",
        "bounded-closed-normalized-package-manifest-construction",
        "request-and-source-bound-result-verdict",
    ],
    "hostMechanisms": [
        "bounded-file-read-and-utf8-validation",
        "generic-toml-syntax-decoding-and-neutral-term-transport",
        "artifact-only-bounded-authority-evaluation",
        "strict-request-and-source-bound-result-decoding",
        "typed-package-manifest-materialization",
        "package-relative-path-resolution-source-loading-and-effect-dispatch",
    ],
    "hostOracle": {
        "productionRequired": False,
        "productionResidualPaths": [],
        "reachability": "test-or-parity-only",
        "removalTask": "R4.2.e",
    },
    "independentVerifier": "scripts/lib/selfhost_pkg_package_manifest_authority.py",
    "kind": "genesis/selfhost-pkg-package-manifest-authority-v0.1",
    "productionEntrypoints": ["genesis", "gc_effects", "gc_obligations", "gc_patches"],
    "requestKind": "genesis/pkg-package-manifest-authority-request-v0.1",
    "resultKind": "genesis/pkg-package-manifest-authority-result-v0.1",
    "schema": "docs/spec/SELFHOST_PKG_PACKAGE_MANIFEST_AUTHORITY_v0.1.schema.json",
    "sourceModules": SOURCE_MODULES,
    "spec": "docs/spec/PACKAGE_TOML.md",
    "version": "0.1.0",
}
NONCLAIMS = {
    "bootstrap-fixpoint",
    "generic-toml-syntax-codec",
    "h2-package-resolution-closure",
    "package-filesystem-or-source-loading-authority",
    "package-graph-lock-registry-or-vcs-closure",
    "r4-2-e-closure",
    "release-qualification",
    "sh-c-closure",
    "wasi-package-command-support",
}
CLAIMED_CALLERS = {
    "crates/gc_cli_driver/src/agent_session/storage.rs": "pkg_manifest_authority::load(cli,",
    "crates/gc_cli_driver/src/cmd_security_ops.rs": "pkg_manifest_authority::load(cli,",
    "crates/gc_cli_driver/src/cmd_security_signing.rs": "pkg_manifest_authority::load(cli,",
    "crates/gc_cli_driver/src/pkg_abi.rs": "pkg_manifest_authority::load_with_frontend(",
    "crates/gc_cli_driver/src/pkg_assurance_ops.rs": "pkg_manifest_authority::load(cli,",
    "crates/gc_cli_driver/src/pkg_assurance_pack_ops.rs": "pkg_manifest_authority::load(cli,",
    "crates/gc_cli_driver/src/pkg_self_opt.rs": "pkg_manifest_authority::load_with_frontend(",
    "crates/gc_cli_driver/src/pkg_workspace_migrate.rs": "pkg_manifest_authority::load(cli,",
    "crates/gc_cli_driver/src/pkg_workspace_ops_build.rs": "pkg_manifest_authority::load_with_frontend(",
    "crates/gc_cli_driver/src/semantic_workspace_analysis.rs": "pkg_manifest_authority::load_with_frontend(",
    "crates/gc_obligations/src/obligation_eval_helpers.rs": "load_package_manifest_with_frontend(",
    "crates/gc_obligations/src/obligations/manifest_hashing.rs": "load_package_manifest_with_frontend(",
    "crates/gc_obligations/src/obligations/types_api.rs": "load_package_manifest_with_frontend(",
    "crates/gc_obligations/src/verify.rs": "load_package_manifest_with_frontend(",
    "crates/gc_patches/src/patch_apply.rs": "load_package_manifest_with_frontend(",
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
        return (root / relative).read_text(encoding="utf-8")
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
    host_schema = schema["properties"].get("hostOracle", {}).get("properties", {})
    if (
        host_schema.get("productionRequired", {}).get("const") is not False
        or host_schema.get("productionResidualPaths", {}).get("maxItems") != 0
        or host_schema.get("reachability", {}).get("const") != "test-or-parity-only"
        or host_schema.get("removalTask", {}).get("const") != "R4.2.e"
    ):
        fail("schema host-oracle closure drift")
    entrypoint_schema = schema["properties"].get("productionEntrypoints", {})
    if (
        set(entrypoint_schema.get("items", {}).get("enum", []))
        != set(CONSTANTS["productionEntrypoints"])
        or entrypoint_schema.get("minItems") != len(CONSTANTS["productionEntrypoints"])
    ):
        fail("schema production-entrypoint closure drift")
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


def direct_load_inventory(root: Path, overrides) -> dict[str, int]:
    inventory = {}
    for base in (
        "crates/gc_cli_driver/src",
        "crates/gc_obligations/src",
        "crates/gc_patches/src",
        "crates/gc_effects/src",
    ):
        for path in sorted((root / base).rglob("*.rs")):
            relative = path.relative_to(root).as_posix()
            subject = text(root, relative, overrides)
            # The unqualified suffix also matches fully qualified gc_pkg calls.
            count = subject.count("PackageManifest::load(")
            if count:
                inventory[relative] = count
    return inventory


def validate_sources(root: Path, profile, overrides=None) -> None:
    overrides = overrides or {}
    core = text(root, SOURCE_MODULES[0], overrides)
    config = text(root, SOURCE_MODULES[1], overrides)
    authority = text(root, SOURCE_MODULES[2], overrides)
    manifest = text(root, "selfhost/toolchain_manifest.gc", overrides)
    artifact = text(root, profile["artifact"], overrides)
    transport = text(root, "crates/gc_pkg/src/manifest_authority.rs", overrides)
    adapter = text(root, "crates/gc_obligations/src/package_manifest_authority.rs", overrides)
    cli_adapter = text(root, "crates/gc_cli_driver/src/pkg_manifest_authority.rs", overrides)
    effects_adapter = text(
        root, "crates/gc_effects/src/pkg_package_manifest_authority.rs", overrides
    )
    effects_runner = text(root, "crates/gc_effects/src/runner.rs", overrides)
    effects_module = text(
        root, "crates/gc_effects/src/runner_cap_pkg_low/module_semantics.rs", overrides
    )
    editor_tasks = text(root, "crates/gc_effects/src/runner_editor_tasks.rs", overrides)
    editor_workflows = text(
        root, "crates/gc_effects/src/runner_editor_task_workflows.rs", overrides
    )
    tests = text(root, "crates/gc_obligations/src/tests/package_manifest_authority.rs", overrides)
    spec = text(root, profile["spec"], overrides)
    ledger = parse_json(
        text(root, "docs/spec/SEMANTIC_OWNERSHIP_LEDGER_v0.1.json", overrides), "ledger"
    )

    require_markers(core, [
        "(def selfhost/pkg-package-manifest::portable-path",
        "selfhost/typecheck::validate-portable-module-path",
        "(def selfhost/pkg-package-manifest::module",
        "(def selfhost/pkg-package-manifest::dependency",
        "(def selfhost/pkg-package-manifest::entry-vector",
        '"package manifest vector is too large"',
    ], "GenesisCode package-manifest core")
    require_markers(config, [
        "(def selfhost/pkg-package-manifest::limits",
        "(def selfhost/pkg-package-manifest::budgets",
        "(def selfhost/pkg-package-manifest::property",
        "(def selfhost/pkg-package-manifest::gfx",
        "(def selfhost/pkg-package-manifest::normalize",
        "(if (core/list::is-nil? schema-raw) 1 schema-raw)",
        '"unsupported package manifest schema (expected 1)"',
    ], "GenesisCode package-manifest configuration")
    require_markers(authority, [
        "(def core/pkg::package-manifest-authority", profile["requestKind"],
        profile["resultKind"], "[:document :kind :source-h :v]",
        '"core/pkg/bad-package-manifest"', "selfhost/hash::hash-term",
        "selfhost/pkg-lock-ops::lower-hex64?",
    ], "GenesisCode package-manifest authority")
    for marker in (profile["binding"], *SOURCE_MODULES):
        if marker not in manifest:
            fail(f"toolchain manifest missing package-manifest marker: {marker}")
        if marker not in artifact:
            fail(f"published artifact missing package-manifest marker: {marker}")

    require_markers(transport, [
        "SOURCE_LIMIT: u64 = 16 * 1024 * 1024",
        "COLLECTION_LIMIT: usize = 4096",
        "STRING_LIMIT: usize = 16 * 1024 * 1024",
        "pub fn read_package_manifest_transport(",
        ".take(SOURCE_LIMIT + 1)", "toml::from_str::<toml::Value>",
        "fn toml_to_term(", "pub fn decode_authorized_package_manifest(",
        'require_string(path, result, ":source-h", source_hash)',
        "exact_fields(", "fn decode_modules(", "fn decode_dependencies(",
    ], "bounded package-manifest transport and decoder")
    for forbidden in (
        "PackageManifest::load(", "validate_manifest_paths(",
        "validate_rel_path_str(", "toml::from_str::<PackageManifest>",
    ):
        if forbidden in transport:
            fail(f"native package-manifest semantic oracle reachable in transport: {forbidden}")
    if transport.count("exact_fields(") != 9:
        fail("package-manifest exact-field check inventory drift")

    require_markers(adapter, [
        "enforce_frontend_allowed(frontend, \"package-manifest authority\")",
        "if rust_frontend_compat_enabled()", "return PackageManifest::load(path)",
        "read_package_manifest_transport(path)",
        "load_selfhost_coreform_toolchain_v1_with_mode(",
        ".get(PACKAGE_MANIFEST_AUTHORITY_BINDING)",
        "Value::data(request)", ".to_plain_term()",
        "decode_authorized_package_manifest(",
    ], "artifact-only obligation package-manifest adapter")
    require_markers(cli_adapter, [
        "resolved_coreform_frontend(cli)",
        "gc_obligations::load_package_manifest_with_frontend(path, frontend)",
    ], "CLI package-manifest adapter")
    require_markers(effects_adapter, [
        "pub(crate) struct PkgPackageManifestAuthority",
        "pub(crate) fn required_for_request",
        "load_selfhost_coreform_toolchain_v1_with_mode(",
        ".get(PACKAGE_MANIFEST_AUTHORITY_BINDING)",
        "read_package_manifest_transport(path)",
        "package_manifest_authority_request(",
        "decode_authorized_package_manifest(",
        "returned sealed ERROR",
    ], "effects package-manifest adapter")
    if "PackageManifest::load(" in effects_adapter:
        fail("native package-manifest fallback reachable in effects adapter")
    require_markers(effects_runner, [
        "PkgPackageManifestAuthority::required_for_request",
        ".and_then(PkgPackageManifestAuthority::load)",
        "package-manifest consumers require the artifact-loaded GenesisCode authority",
    ], "effects package-manifest authority loader")
    if effects_module.count(".load_manifest(&pkg_path)") != 2:
        fail("package-low package-manifest authority custody drift")
    if editor_tasks.count(".load_manifest(&pkg_pathbuf)") != 1:
        fail("editor package-analysis manifest authority custody drift")
    if editor_workflows.count(".load_manifest(path)") != 1:
        fail("editor workspace-index manifest authority custody drift")
    for path, marker in CLAIMED_CALLERS.items():
        require_markers(text(root, path, overrides), [marker], f"manifest custody {path}")

    expected_direct = {
        "crates/gc_cli_driver/src/pkg_workspace_ops.rs": 1,
        "crates/gc_obligations/src/obligation_authority_tests.rs": 19,
        "crates/gc_obligations/src/obligations/frontend_module_ops.rs": 1,
        "crates/gc_obligations/src/package_manifest_authority.rs": 1,
    }
    actual_direct = direct_load_inventory(root, overrides)
    # Files under src/tests are test-only and do not participate in production reachability.
    actual_direct = {
        path: count for path, count in actual_direct.items()
        if "/src/tests/" not in path
    }
    if actual_direct != expected_direct:
        fail(f"direct native package-manifest reachability drift: {actual_direct}")
    parity = text(root, "crates/gc_cli_driver/src/pkg_workspace_ops.rs", overrides)
    require_markers(parity, [
        '#[cfg(any(test, feature = "parity-harness"))]',
        "fn handle_migrate_parity(", "PackageManifest::load(pkg)",
    ], "CLI retained package-manifest oracle")
    frontend_tests = text(
        root, "crates/gc_obligations/src/obligations/frontend_module_ops.rs", overrides
    )
    if frontend_tests.rfind("#[cfg(test)]", 0, frontend_tests.index("PackageManifest::load(")) < 0:
        fail("obligations direct manifest fixture is not test-guarded")

    require_markers(tests, [
        "package_manifest_authority_normalizes_complete_manifest",
        "package_manifest_authority_preserves_legacy_defaults",
        "package_manifest_authority_preserves_schema_repair_contract",
        "package_manifest_authority_rejects_nonportable_paths",
        "package_manifest_authority_rejects_invalid_types_without_source_access",
        '"../escape.gc"', '"src//main.gc"', '"C:/main.gc"', '"src\\\\main.gc"',
    ], "package-manifest authority tests")
    require_markers(spec, [
        "Artifact-loaded `core/pkg::package-manifest-authority` exclusively decides",
        "No production\n  caller may invoke that native semantic oracle",
        "Unknown TOML keys are ignored",
        "Unicode-NFC module file path",
    ], "package-manifest normative specification")

    rows = ledger.get("semanticDecisions")
    if not isinstance(rows, list):
        fail("ledger semanticDecisions missing")
    row = next((item for item in rows if item.get("id") == "SD-PACKAGE-RESOLUTION"), None)
    if not isinstance(row, dict):
        fail("ledger package-resolution decision missing")
    if row.get("currentLevel") != "H0":
        fail("package-resolution ledger must remain H0")
    joined = json.dumps(row, sort_keys=True)
    require_markers(joined, [
        profile["kind"], profile["spec"], profile["independentVerifier"], *SOURCE_MODULES,
        "crates/gc_pkg/src/manifest_authority.rs",
        "crates/gc_obligations/src/package_manifest_authority.rs",
        "crates/gc_effects/src/pkg_package_manifest_authority.rs",
        "GenesisCode exclusively owns structural package-manifest admission",
        "no production route retains the native package-manifest semantic oracle",
    ], "package-manifest ownership ledger")


def validate_all(root: Path, profile, schema, overrides=None) -> None:
    overrides = overrides or {}
    validate_profile(profile, schema)
    if source_identity(root, overrides) != profile["sourceSha256"]:
        fail("profile source identity mismatch")
    validate_sources(root, profile, overrides)


def self_test(root: Path, profile, schema) -> int:
    paths = SOURCE_MODULES + [
        "selfhost/toolchain_manifest.gc", profile["artifact"],
        "crates/gc_pkg/src/manifest_authority.rs",
        "crates/gc_obligations/src/package_manifest_authority.rs",
        "crates/gc_cli_driver/src/pkg_manifest_authority.rs",
        "crates/gc_effects/src/pkg_package_manifest_authority.rs",
        "crates/gc_effects/src/runner.rs",
        "crates/gc_effects/src/runner_cap_pkg_low/module_semantics.rs",
        "crates/gc_effects/src/runner_editor_tasks.rs",
        "crates/gc_effects/src/runner_editor_task_workflows.rs",
        "crates/gc_cli_driver/src/pkg_workspace_ops.rs",
        "crates/gc_obligations/src/obligations/frontend_module_ops.rs",
        "crates/gc_obligations/src/obligation_authority_tests.rs",
        "crates/gc_obligations/src/tests/package_manifest_authority.rs",
        "docs/spec/PACKAGE_TOML.md",
        "docs/spec/SEMANTIC_OWNERSHIP_LEDGER_v0.1.json",
        *CLAIMED_CALLERS,
    ]
    sources = {path: text(root, path, {}) for path in dict.fromkeys(paths)}
    mutations = []

    def profile_mutation(name, value):
        changed = copy.deepcopy(profile)
        changed[name] = value
        changed["contentIdentitySha256"] = canonical_identity(changed)
        mutations.append((changed, {}, name))

    for name, value in (
        ("binding", "core/pkg::legacy-package-manifest"),
        ("decisionInventory", profile["decisionInventory"][:-1]),
        ("hostMechanisms", profile["hostMechanisms"][:-1]),
        ("hostOracle", {**profile["hostOracle"], "productionRequired": True}),
        ("nonclaims", profile["nonclaims"][:-1]),
        ("sourceSha256", "f" * 64),
    ):
        profile_mutation(name, value)

    def source_mutation(path, old, new, name):
        if old not in sources[path]:
            fail(f"self-test marker absent for {name}")
        mutations.append((profile, {path: sources[path].replace(old, new, 1)}, name))

    core, config, authority = SOURCE_MODULES
    source_mutation(core, "(def selfhost/pkg-package-manifest::portable-path", "(def selfhost/pkg-package-manifest::legacy-path", "portable path")
    source_mutation(core, "selfhost/typecheck::validate-portable-module-path", "selfhost/pkg-lock-read::str?", "portable path validator")
    source_mutation(core, '"package manifest vector is too large"', '"unbounded package manifest vector"', "collection bound")
    source_mutation(config, "(def selfhost/pkg-package-manifest::limits", "(def selfhost/pkg-package-manifest::legacy-limits", "limits")
    source_mutation(config, "(def selfhost/pkg-package-manifest::gfx", "(def selfhost/pkg-package-manifest::legacy-gfx", "graphics")
    source_mutation(config, "(if (core/list::is-nil? schema-raw) 1 schema-raw)", "schema-raw", "legacy schema")
    source_mutation(config, '"unsupported package manifest schema (expected 1)"', '"package manifest schema invalid"', "schema repair contract")
    source_mutation(authority, "(def core/pkg::package-manifest-authority", "(def core/pkg::legacy-package-manifest", "source binding")
    source_mutation(authority, profile["requestKind"], "genesis/legacy-package-manifest-request", "request kind")
    source_mutation(authority, "selfhost/pkg-lock-ops::lower-hex64?", "selfhost/pkg-lock-read::str?", "source hash admission")
    source_mutation("selfhost/toolchain_manifest.gc", profile["binding"], "core/pkg::missing-package-manifest", "manifest binding")
    mutations.append((profile, {
        profile["artifact"]: sources[profile["artifact"]].replace(
            profile["binding"], "core/pkg::missing-package-manifest"
        )
    }, "artifact binding"))
    transport = "crates/gc_pkg/src/manifest_authority.rs"
    source_mutation(transport, "toml::from_str::<toml::Value>", "toml::from_str::<PackageManifest>", "neutral TOML transport")
    source_mutation(transport, ".take(SOURCE_LIMIT + 1)", ".read_to_end", "source bound")
    source_mutation(transport, "exact_fields(", "accept_open_fields(", "result closure")
    source_mutation(transport, 'require_string(path, result, ":source-h", source_hash)', 'required_string(path, result, ":source-h")', "source binding")
    source_mutation(transport, "COLLECTION_LIMIT: usize = 4096", "COLLECTION_LIMIT: usize = usize::MAX", "decoder collection bound")
    adapter = "crates/gc_obligations/src/package_manifest_authority.rs"
    source_mutation(adapter, "if rust_frontend_compat_enabled()", "if true", "parity fallback guard")
    source_mutation(adapter, ".get(PACKAGE_MANIFEST_AUTHORITY_BINDING)", ".get(\"legacy-manifest\")", "artifact authority lookup")
    source_mutation(adapter, ".to_plain_term()", ".as_data()", "sealed result rejection")
    cli_adapter = "crates/gc_cli_driver/src/pkg_manifest_authority.rs"
    source_mutation(cli_adapter, "gc_obligations::load_package_manifest_with_frontend(path, frontend)", "PackageManifest::load(path)", "CLI custody")
    caller = "crates/gc_cli_driver/src/pkg_workspace_ops_build.rs"
    source_mutation(caller, "pkg_manifest_authority::load_with_frontend(", "PackageManifest::load(", "build custody")
    caller = "crates/gc_obligations/src/verify.rs"
    source_mutation(caller, "load_package_manifest_with_frontend(", "PackageManifest::load(", "verification custody")
    caller = "crates/gc_patches/src/patch_apply.rs"
    source_mutation(caller, "load_package_manifest_with_frontend(", "PackageManifest::load(", "patch custody")
    effects = "crates/gc_effects/src/runner_editor_tasks.rs"
    source_mutation(effects, ".load_manifest(&pkg_pathbuf)", "PackageManifest::load(&pkg_pathbuf)", "effects native fallback")
    tests = "crates/gc_obligations/src/tests/package_manifest_authority.rs"
    source_mutation(tests, "package_manifest_authority_rejects_nonportable_paths", "legacy_nonportable_paths", "path negative control")
    source_mutation(tests, "package_manifest_authority_preserves_schema_repair_contract", "legacy_schema_repair_test", "schema repair control")
    source_mutation(tests, "package_manifest_authority_rejects_invalid_types_without_source_access", "legacy_type_test", "pre-access negative control")
    spec = "docs/spec/PACKAGE_TOML.md"
    source_mutation(spec, "Unknown TOML keys are ignored", "Unknown TOML keys are semantic", "unknown field contract")
    ledger = "docs/spec/SEMANTIC_OWNERSHIP_LEDGER_v0.1.json"
    source_mutation(ledger, profile["kind"], "native-package-manifest", "ledger authority")
    source_mutation(ledger, "no production route retains the native package-manifest semantic oracle", "effects routes may use the native package-manifest semantic oracle", "ledger residual")

    controls = 0
    for changed_profile, overrides, name in mutations:
        try:
            validate_all(root, changed_profile, schema, overrides)
        except CheckError:
            controls += 1
        else:
            fail(f"negative control survived: {name}")
    if controls != 37:
        fail(f"negative control inventory drift: {controls}")
    return controls


def write_identities(path: Path, profile, root: Path) -> None:
    profile["sourceSha256"] = source_identity(root, {})
    profile["contentIdentitySha256"] = canonical_identity(profile)
    path.write_text(json.dumps(profile, indent=2) + "\n", encoding="utf-8")


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
            "selfhost-pkg-package-manifest-authority: ok "
            f"profile={profile['contentIdentitySha256']} controls={controls}"
        )
    except CheckError as error:
        print(f"selfhost-pkg-package-manifest-authority: {error}", file=sys.stderr)
        return 1
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
