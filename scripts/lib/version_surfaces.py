#!/usr/bin/env python3
"""Validate GenesisCode's version-surface registry against live authorities."""

import json
import re
import copy
from pathlib import Path


REGISTRY = "genesis.version-surfaces.json"
REQUIRED_SURFACES = {
    "product-release",
    "package-manifest",
    "workspace-config",
    "genesis-lock",
    "effect-log",
    "gpk-bundle",
    "canonical-hash-profile",
    "compiled-module-blob",
    "selfhost-compiled-cache",
    "selfhost-toolchain-artifact",
}
SURFACE_KEYS = {
    "id",
    "kind",
    "discriminator",
    "current_writer",
    "accepted_readers",
    "wire_identity",
    "authorities",
    "specs",
    "migrations",
}
SURFACE_REQUIRED = SURFACE_KEYS - {"wire_identity"}
MIGRATION_KEYS = {
    "id",
    "surface",
    "from",
    "to",
    "reader",
    "writer",
    "semantic_delta",
    "user_action",
    "tests",
    "retirement",
}


def fail(message):
    raise SystemExit("version-surfaces: " + message)


def read(root, rel):
    path = root / rel
    if not path.is_file():
        fail("missing authority: " + rel)
    return path.read_text(encoding="utf-8")


def require(text, needle, rel):
    if needle not in text:
        fail("{} missing {!r}".format(rel, needle))


def require_absent(text, needle, rel):
    if needle in text:
        fail("{} retains forbidden {!r}".format(rel, needle))


def string_list(value, context, allow_empty=False):
    if not isinstance(value, list) or (not value and not allow_empty):
        fail(context + " must be a " + ("list" if allow_empty else "non-empty list"))
    if any(not isinstance(item, str) or not item for item in value):
        fail(context + " entries must be non-empty strings")
    if len(value) != len(set(value)):
        fail(context + " entries must be unique")


def validate_registry(root, supplied_data=None):
    if supplied_data is None:
        registry_path = root / REGISTRY
        try:
            data = json.loads(registry_path.read_text(encoding="utf-8"))
        except (OSError, ValueError) as error:
            fail("cannot load {}: {}".format(REGISTRY, error))
    else:
        data = supplied_data

    if set(data) != {"schema", "release_train", "policy", "surfaces", "migrations"}:
        fail("registry top-level keys drifted")
    if data["schema"] != "genesis/version-surfaces-v0.1":
        fail("unsupported registry schema")
    if not re.fullmatch(r"[0-9]+\.[0-9]+\.[0-9]+", data["release_train"]):
        fail("release_train must be semver-shaped")
    expected_policy = {
        "current_writer_only": True,
        "future_versions": "reject",
        "missing_discriminator": "reject-except-named-migration",
        "legacy_reader_rule": "every non-current accepted value requires a migration record and regression test",
    }
    if data["policy"] != expected_policy:
        fail("compatibility policy drifted")

    if not isinstance(data["surfaces"], list) or not data["surfaces"]:
        fail("surfaces must be a non-empty list")
    surfaces = {}
    for surface in data["surfaces"]:
        if not isinstance(surface, dict):
            fail("surface entries must be objects")
        if not SURFACE_REQUIRED.issubset(surface) or not set(surface).issubset(SURFACE_KEYS):
            fail("surface has missing or unknown keys: {!r}".format(surface.get("id")))
        sid = surface["id"]
        if not isinstance(sid, str) or not re.fullmatch(r"[a-z][a-z0-9-]*", sid):
            fail("invalid surface id: {!r}".format(sid))
        if sid in surfaces:
            fail("duplicate surface id: " + sid)
        for field in ("kind", "discriminator", "current_writer"):
            if not isinstance(surface[field], str) or not surface[field]:
                fail("{}.{} must be a non-empty string".format(sid, field))
        for field in ("accepted_readers", "authorities", "specs"):
            string_list(surface[field], "{}.{}".format(sid, field))
        string_list(surface["migrations"], "{}.migrations".format(sid), allow_empty=True)
        if surface["current_writer"] not in surface["accepted_readers"]:
            fail(sid + " current writer is not accepted by its reader")
        for rel in surface["authorities"] + surface["specs"]:
            if not (root / rel).is_file():
                fail("{} references missing path {}".format(sid, rel))
        surfaces[sid] = surface
    if set(surfaces) != REQUIRED_SURFACES:
        fail("surface inventory drift: expected {}, got {}".format(
            sorted(REQUIRED_SURFACES), sorted(surfaces)))

    if not isinstance(data["migrations"], list):
        fail("migrations must be a list")
    migrations = {}
    for migration in data["migrations"]:
        if not isinstance(migration, dict) or set(migration) != MIGRATION_KEYS:
            fail("migration has missing or unknown keys")
        mid = migration["id"]
        if not isinstance(mid, str) or not re.fullmatch(r"M-[A-Z0-9-]+", mid):
            fail("invalid migration id: {!r}".format(mid))
        if mid in migrations:
            fail("duplicate migration id: " + mid)
        if migration["surface"] not in surfaces:
            fail(mid + " references unknown surface")
        for field in MIGRATION_KEYS - {"tests"}:
            if not isinstance(migration[field], str) or not migration[field]:
                fail("{}.{} must be a non-empty string".format(mid, field))
        string_list(migration["tests"], mid + ".tests")
        for rel in migration["tests"]:
            if not (root / rel).is_file():
                fail("{} references missing test path {}".format(mid, rel))
        migrations[mid] = migration

    for sid, surface in surfaces.items():
        declared = set(surface["migrations"])
        actual = {mid for mid, item in migrations.items() if item["surface"] == sid}
        if declared != actual:
            fail("{} migration links drifted".format(sid))
        legacy_values = set(surface["accepted_readers"]) - {surface["current_writer"]}
        migrated_values = {migrations[mid]["from"] for mid in declared}
        if legacy_values != migrated_values:
            fail("{} legacy readers do not exactly match migration sources".format(sid))
        for mid in declared:
            if migrations[mid]["to"] != surface["current_writer"]:
                fail(mid + " does not target the current writer")

    return data, surfaces, migrations


def run_negative_controls(root, data):
    controls = []

    def reject(label, mutate):
        candidate = copy.deepcopy(data)
        mutate(candidate)
        try:
            validate_registry(root, candidate)
        except SystemExit as error:
            if not str(error).startswith("version-surfaces: "):
                raise
            controls.append(label)
            return
        fail("negative control unexpectedly passed: " + label)

    reject("unknown-top-level-field", lambda item: item.update({"future": True}))
    reject("unknown-surface-field", lambda item: item["surfaces"][0].update({"future": True}))
    reject(
        "unregistered-legacy-reader",
        lambda item: item["surfaces"][0]["accepted_readers"].append("0.1.0"),
    )
    reject(
        "migration-target-drift",
        lambda item: item["migrations"][0].update({"to": "2"}),
    )
    reject(
        "duplicate-surface-id",
        lambda item: item["surfaces"].append(copy.deepcopy(item["surfaces"][0])),
    )
    reject(
        "writer-not-readable",
        lambda item: item["surfaces"][0].update({"accepted_readers": ["0.1.0"]}),
    )
    return controls


def validate_authorities(root, data):
    release = data["release_train"]
    cargo = read(root, "Cargo.toml")
    require(cargo, '[workspace.package]\nversion = "{}"'.format(release), "Cargo.toml")
    for manifest in sorted((root / "crates").glob("*/Cargo.toml")):
        text = manifest.read_text(encoding="utf-8")
        rel = str(manifest.relative_to(root))
        for line in ("version.workspace = true", "edition.workspace = true", "license.workspace = true", "publish.workspace = true"):
            require(text, line, rel)

    cli_args = read(root, "crates/gc_cli_driver/src/cli_args.rs")
    require(cli_args, '#[command(name = "genesis", version)]', "crates/gc_cli_driver/src/cli_args.rs")
    require(read(root, "crates/gc_cli/Cargo.toml"), 'default-run = "genesis"', "crates/gc_cli/Cargo.toml")
    require(read(root, "crates/gc_wasi_cli/Cargo.toml"), 'default-run = "genesis_wasi"', "crates/gc_wasi_cli/Cargo.toml")

    core = read(root, "crates/gc_coreform/src/term.rs")
    require(core, 'HASH_PROFILE_ID: &str = "genesis/hash-profile/gcv0.2-blake3"', "crates/gc_coreform/src/term.rs")
    require(core, 'HASH_DOMAIN_PREFIX: &[u8] = b"GCv0.2\\0"', "crates/gc_coreform/src/term.rs")
    bare_prefix = re.compile(r'b"GCv0\.2\\0"')
    owners = []
    for path in (root / "crates").glob("*/src/**/*.rs"):
        if bare_prefix.search(path.read_text(encoding="utf-8")):
            owners.append(str(path.relative_to(root)))
    if owners != ["crates/gc_coreform/src/term.rs"]:
        fail("bare canonical hash prefix must have one authority, got {}".format(sorted(owners)))

    log = read(root, "crates/gc_effects/src/log.rs")
    require(log, "GCLOG_LEGACY_VERSION: u64 = 2", "crates/gc_effects/src/log.rs")
    require(log, "GCLOG_CURRENT_VERSION: u64 = 3", "crates/gc_effects/src/log.rs")
    require(log, '"missing gclog :version"', "crates/gc_effects/src/log.rs")
    require_absent(log, 'unwrap_or(2)', "crates/gc_effects/src/log.rs")
    runner = read(root, "crates/gc_effects/src/runner.rs")
    require(runner, "version: GCLOG_CURRENT_VERSION", "crates/gc_effects/src/runner.rs")

    manifest = read(root, "crates/gc_pkg/src/manifest.rs")
    require(manifest, "PACKAGE_MANIFEST_SCHEMA_VERSION: u64 = 1", "crates/gc_pkg/src/manifest.rs")
    require(manifest, "unsupported package manifest schema", "crates/gc_pkg/src/manifest.rs")
    scaffold = read(root, "crates/gc_cli_driver/src/pkg_scaffold.rs")
    require(scaffold, 'r#"schema = 1', "crates/gc_cli_driver/src/pkg_scaffold.rs")
    package_files = sorted((root / "examples").rglob("package.toml"))
    package_files += sorted((root / "tests").rglob("package.toml"))
    if not package_files:
        fail("no maintained package.toml files found")
    for path in package_files:
        rel = str(path.relative_to(root))
        top_level = []
        for line in path.read_text(encoding="utf-8").splitlines():
            if line.lstrip().startswith("["):
                break
            top_level.append(line)
        schema_lines = [
            line
            for line in top_level
            if re.fullmatch(r"\s*schema\s*=\s*1\s*(?:#.*)?", line)
        ]
        if len(schema_lines) != 1:
            fail(rel + " must contain exactly one explicit top-level schema = 1")

    workspace = read(root, "crates/gc_pkg/src/workspace.rs")
    require(workspace, "GENESIS_WORKSPACE_VERSION: u64 = 1", "crates/gc_pkg/src/workspace.rs")
    require(workspace, 'msg: "missing version".to_string()', "crates/gc_pkg/src/workspace.rs")
    require_absent(workspace, "wt.version.unwrap_or", "crates/gc_pkg/src/workspace.rs")
    lock = read(root, "crates/gc_pkg/src/lock.rs")
    require(lock, "GENESIS_LOCK_LEGACY_VERSION: u64 = 1", "crates/gc_pkg/src/lock.rs")
    require(lock, "GENESIS_LOCK_CURRENT_VERSION: u64 = 2", "crates/gc_pkg/src/lock.rs")
    require_absent(lock, "lt.version.unwrap_or", "crates/gc_pkg/src/lock.rs")

    gpk = read(root, "crates/gc_vcs/src/gpk.rs")
    require(gpk, "GPK_LEGACY_VERSION: u32 = 1", "crates/gc_vcs/src/gpk.rs")
    require(gpk, "GPK_CURRENT_VERSION: u32 = 2", "crates/gc_vcs/src/gpk.rs")
    gpk_ops = read(root, "crates/gc_effects/src/runner_cap_gc_gpk_low/gpk_ops.rs")
    require(gpk_ops, "let bundle_version = gc_vcs::GPK_CURRENT_VERSION;", "crates/gc_effects/src/runner_cap_gc_gpk_low/gpk_ops.rs")
    require_absent(gpk_ops, "if embed_refnames.is_empty() { 1 } else { 2 }", "crates/gc_effects/src/runner_cap_gc_gpk_low/gpk_ops.rs")

    compiled = read(root, "crates/gc_kernel/src/compiled.rs")
    require(compiled, 'COMPILED_MODULE_BLOB_MAGIC: &[u8] = b"GCKM5\\0"', "crates/gc_kernel/src/compiled.rs")
    selfhost = read(root, "crates/gc_prelude/src/selfhost_coreform_v1.rs")
    require(selfhost, 'SELFHOST_COMPILED_CACHE_FILE_MAGIC: &[u8] = b"GCSHC1\\0"', "crates/gc_prelude/src/selfhost_coreform_v1.rs")
    kind = "genesis/selfhost-toolchain-artifact-v0.2"
    require(selfhost, kind, "crates/gc_prelude/src/selfhost_coreform_v1.rs")
    require(read(root, "selfhost/toolchain.gc"), ':generated-by "genesis {}"'.format(release), "selfhost/toolchain.gc")
    require(read(root, "selfhost/toolchain.gc"), kind, "selfhost/toolchain.gc")

    spec = read(root, "docs/spec/VERSION_SURFACES_v0.1.md")
    for migration in data["migrations"]:
        require(spec, migration["id"], "docs/spec/VERSION_SURFACES_v0.1.md")
    gclog_spec = read(root, "docs/spec/GCLOG.md")
    require(gclog_spec, "parser accepts legacy `2`", "docs/spec/GCLOG.md")
    require(gclog_spec, "genesis {}".format(release), "docs/spec/GCLOG.md")
    require_absent(gclog_spec, "genesis 0.1.0", "docs/spec/GCLOG.md")


def main(root):
    data, surfaces, migrations = validate_registry(root)
    controls = run_negative_controls(root, data)
    validate_authorities(root, data)
    print("version-surfaces: ok (surfaces={} migrations={} negative_controls={} release={})".format(
        len(surfaces), len(migrations), len(controls), data["release_train"]))


if __name__ == "__main__":
    import sys

    if len(sys.argv) != 2:
        raise SystemExit("usage: version_surfaces.py ROOT")
    main(Path(sys.argv[1]).resolve())
