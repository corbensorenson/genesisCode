#!/usr/bin/env python3
"""Independent custody verifier for the partial self-hosted commit authority."""

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
    result = {}
    for key, value in pairs:
        if key in result:
            fail(f"duplicate JSON key: {key}")
        result[key] = value
    return result


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
    "binding": "core/commit::authority",
    "decisionInventory": [
        "native-commit-object-construction", "native-commit-object-field-closure",
        "native-commit-object-identity-admission", "native-commit-object-inspection-admission",
        "native-commit-author-metadata-construction", "request-bound-result-verdict",
    ],
    "hostMechanisms": [
        "artifact-only-authority-bootstrap-and-bounded-evaluation", "cli-argument-transport",
        "ref-and-store-mechanisms", "patch-application-mechanism",
        "artifact-hash-contradiction-check", "diagnostic-rendering",
    ],
    "hostOracle": {"parityOnly": True, "productionRequired": False, "removalTask": "R4.2.e"},
    "independentVerifier": "scripts/lib/selfhost_commit_authority.py",
    "kind": "genesis/selfhost-commit-authority-v0.1",
    "productionEntrypoints": ["genesis"],
    "requestKind": "genesis/commit-authority-request-v0.1",
    "resultKind": "genesis/commit-authority-result-v0.1",
    "schema": "docs/spec/SELFHOST_COMMIT_AUTHORITY_v0.1.schema.json",
    "sourceModule": "selfhost/commit_authority_v1.gc",
    "spec": "docs/spec/SELFHOST_COMMIT_AUTHORITY_v0.1.md",
    "version": "0.1.0",
}
NONCLAIMS = {
    "bootstrap-fixpoint", "h2-sd-canon-identity", "h2-sd-commit",
    "internal-package-registry-vcs-commit-authority", "r4-2-e-closure",
    "release-qualification", "sh-c-closure", "wasi-commit-cli-authority",
}


def canonical_identity(profile) -> str:
    value = copy.deepcopy(profile)
    value.pop("contentIdentitySha256", None)
    return hashlib.sha256(json.dumps(value, sort_keys=True, separators=(",", ":")).encode()).hexdigest()


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
    if check_identity and profile["contentIdentitySha256"] != canonical_identity(profile):
        fail("profile content identity mismatch")


def read_text(root: Path, relative: str, overrides) -> str:
    return overrides.get(relative, (root / relative).read_text())


def require_all(haystack: str, needles, context: str) -> None:
    for needle in needles:
        if needle not in haystack:
            fail(f"{context} missing {needle!r}")


def static_check(root: Path, profile, overrides=None, artifact_path=None, check_artifact=True) -> None:
    overrides = overrides or {}
    source_relative = profile["sourceModule"]
    source_path = root / source_relative
    if source_path.is_symlink() or not source_path.is_file() or root.resolve() not in source_path.resolve().parents:
        fail("authority source is missing, escaping, or symlinked")
    source = read_text(root, source_relative, overrides)
    if source_identity(source_relative, source.encode()) != profile["sourceSha256"]:
        fail("authority source identity mismatch")
    require_all(source, [
        f"(def {profile['binding']}", profile["requestKind"], profile["resultKind"],
        "(quote :make)", "(quote :validate)", "selfhost/commit::commit?",
        "selfhost/store-authority::exact-map? request", "selfhost/store-authority::hash?",
        "selfhost/commit::allowed-fields-loop?", "constructed commit failed canonical validation",
        ":request-h (selfhost/hash::hash-term request)", "commit must use the closed canonical v1 schema",
    ], "GenesisCode commit authority")
    if "core/effect::" in source or "core/host::" in source:
        fail("commit authority contains an ambient effect or host operation")

    manifest_path = "selfhost/toolchain_manifest.gc"
    manifest = read_text(root, manifest_path, overrides)
    if manifest.count(f'"{source_relative}"') != 1 or manifest.count(profile["binding"]) != 1:
        fail("toolchain manifest custody drift")

    bridge_path = "crates/gc_cli_driver/src/commit_authority.rs"
    bridge = read_text(root, bridge_path, overrides)
    require_all(bridge, [
        f'const BINDING: &str = "{profile["binding"]}"',
        f'const REQUEST_KIND: &str = "{profile["requestKind"]}"',
        f'const RESULT_KIND: &str = "{profile["resultKind"]}"',
        "load_selfhost_toolchain(cli, &mut context, &mut environment)?",
        "hex32(gc_coreform::hash_term(&request))", "decode_result(value, &request_hash, command)",
        "value.to_plain_term()", "result field set mismatch", "successful result artifact must be a map",
        "strict_decoder_rejects_open_and_unbound_results", "strict_decoder_accepts_runtime_map_results",
    ], "Rust commit authority bridge")
    for default in ("unwrap_or_default()", "unwrap_or(true)", "unwrap_or(Term::Map"):
        if default in bridge:
            fail(f"commit authority bridge contains success-capable default {default!r}")

    cmd_path = "crates/gc_cli_driver/src/cmd_commit.rs"
    cmd = read_text(root, cmd_path, overrides)
    require_all(cmd, [
        "commit_authority::make(", 'commit_authority::validate(cli, artifact, "commit/show")',
        'commit_authority::validate(cli, artifact, "commit/new base ref")',
        "commit_make_payload(", "mk_store_put_program(&artifact)",
    ], "native commit route")
    for residual in (
        "fn build_commit_artifact", "gc_vcs::Commit::from_term", "invalid --target-id: empty value",
        "invalid --message: empty value", "to_ascii_lowercase()",
    ):
        if residual in cmd:
            fail(f"native commit route retains host semantic residual {residual!r}")
    if cmd.index("commit_authority::make(") > cmd.index("mk_store_put_program(&artifact)"):
        fail("commit artifact is stored before authority construction")

    tests_path = "crates/gc_cli/tests/cli_commit.rs"
    tests = read_text(root, tests_path, overrides)
    require_all(tests, [
        "commit_new_and_show_roundtrip_with_ref_base_and_patch_file",
        "exercise self-hosted construction", "commit_show_rejects_open_commit_objects",
        'contains("core/vcs/bad-commit")',
    ], "native commit authority tests")

    ledger = load_json(root / "docs/spec/SEMANTIC_OWNERSHIP_LEDGER_v0.1.json")
    rows = [row for row in ledger.get("semanticDecisions", []) if row.get("id") == "SD-COMMIT"]
    if len(rows) != 1:
        fail("SD-COMMIT ledger row missing or duplicated")
    row = rows[0]
    limitations = " ".join(row.get("limitations", []))
    if (row.get("currentLevel") != "H0" or source_relative not in row.get("producingImplementationPaths", [])
            or source_relative not in row.get("productionAuthorityPaths", [])
            or bridge_path not in row.get("productionAuthorityPaths", [])
            or profile["spec"] not in row.get("specAuthorityPaths", [])
            or profile["independentVerifier"] not in row.get("verifierPaths", [])
            or "internal package/registry/VCS" not in limitations):
        fail("SD-COMMIT partial H0 custody drift")

    spec = read_text(root, profile["spec"], overrides)
    require_all(spec, [
        "This slice remains H0", "sole producer of canonical v1 commit construction",
        "Package, registry, and internal VCS paths", "cannot substitute an artifact",
        "does not close `SD-COMMIT`", "permanent source/route mutations",
    ], "commit authority specification")

    if check_artifact:
        artifact = artifact_path or (root / profile["artifact"])
        data = artifact.read_bytes()
        if source_relative.encode() not in data or profile["binding"].encode() not in data:
            fail("commit authority source or binding absent from admitted artifact")


def mutation_controls(root: Path, profile) -> int:
    names = [
        profile["sourceModule"], "selfhost/toolchain_manifest.gc",
        "crates/gc_cli_driver/src/commit_authority.rs", "crates/gc_cli_driver/src/cmd_commit.rs",
        "crates/gc_cli/tests/cli_commit.rs",
    ]
    paths = {name: (root / name).read_text() for name in names}
    source = paths[profile["sourceModule"]]
    manifest = paths["selfhost/toolchain_manifest.gc"]
    bridge = paths["crates/gc_cli_driver/src/commit_authority.rs"]
    cmd = paths["crates/gc_cli_driver/src/cmd_commit.rs"]
    tests = paths["crates/gc_cli/tests/cli_commit.rs"]
    mutations = [
        ({profile["sourceModule"]: source.replace("(quote :make)", "(quote :removed)", 1)}, "make operation"),
        ({profile["sourceModule"]: source.replace("(quote :validate)", "(quote :removed)", 1)}, "validate operation"),
        ({profile["sourceModule"]: source.replace("selfhost/store-authority::hash?", "selfhost/store-authority::str?")}, "identity admission"),
        ({profile["sourceModule"]: source.replace("selfhost/commit::allowed-fields-loop?", "selfhost/commit::removed-fields-loop?")}, "field closure"),
        ({profile["sourceModule"]: source.replace(":request-h (selfhost/hash::hash-term request)", ":request-h nil", 1)}, "request binding"),
        ({"selfhost/toolchain_manifest.gc": manifest.replace(f'    "{profile["sourceModule"]}"\n', "", 1)}, "module custody"),
        ({"selfhost/toolchain_manifest.gc": manifest.replace(f"    {profile['binding']}\n", "", 1)}, "binding custody"),
        ({"crates/gc_cli_driver/src/commit_authority.rs": bridge.replace("value.to_plain_term()", "value.as_data().cloned()", 1)}, "runtime collection decoder"),
        ({"crates/gc_cli_driver/src/commit_authority.rs": bridge.replace("result field set mismatch", "removed field closure", 1)}, "result closure"),
        ({"crates/gc_cli_driver/src/cmd_commit.rs": cmd.replace("commit_authority::make(", "removed_authority::make(", 1)}, "construction route"),
        ({"crates/gc_cli_driver/src/cmd_commit.rs": cmd.replace('commit_authority::validate(cli, artifact, "commit/show")', "Ok(artifact)", 1)}, "inspection route"),
        ({"crates/gc_cli_driver/src/cmd_commit.rs": cmd.replace("Ok((base.to_string(), Vec::new()))", "Ok((base.to_ascii_lowercase(), Vec::new()))", 1)}, "host normalization"),
        ({"crates/gc_cli/tests/cli_commit.rs": tests.replace("commit_show_rejects_open_commit_objects", "removed_open_commit_control", 1)}, "negative control"),
    ]
    passed = 0
    for overrides, name in mutations:
        candidate = copy.deepcopy(profile)
        if profile["sourceModule"] in overrides:
            candidate["sourceSha256"] = source_identity(
                profile["sourceModule"], overrides[profile["sourceModule"]].encode())
        try:
            static_check(root, candidate, overrides, check_artifact=False)
        except CheckError:
            passed += 1
        else:
            fail(f"mutation control survived: {name}")
    return passed


def update(root: Path, profile_path: Path, schema_path: Path) -> None:
    profile = load_json(profile_path)
    schema = load_json(schema_path)
    validate_profile(profile, schema, check_identity=False)
    relative = profile["sourceModule"]
    profile["sourceSha256"] = source_identity(relative, (root / relative).read_bytes())
    profile["contentIdentitySha256"] = canonical_identity(profile)
    profile_path.write_text(json.dumps(profile, indent=2) + "\n")


def main() -> int:
    parser = argparse.ArgumentParser()
    parser.add_argument("--root", default=".")
    parser.add_argument("--profile", required=True)
    parser.add_argument("--schema", required=True)
    parser.add_argument("--artifact")
    parser.add_argument("--self-test", action="store_true")
    parser.add_argument("--update", action="store_true")
    args = parser.parse_args()
    root = Path(args.root).resolve()
    profile_path = (root / args.profile).resolve()
    schema_path = (root / args.schema).resolve()
    if args.update:
        update(root, profile_path, schema_path)
    profile = load_json(profile_path)
    schema = load_json(schema_path)
    validate_profile(profile, schema)
    artifact = Path(args.artifact).resolve() if args.artifact else None
    static_check(root, profile, artifact_path=artifact)
    controls = mutation_controls(root, profile) if args.self_test else 0
    print(json.dumps({
        "artifact": str(artifact or (root / profile["artifact"])),
        "contentIdentitySha256": profile["contentIdentitySha256"],
        "kind": profile["kind"], "mutationControls": controls, "ok": True,
        "sourceSha256": profile["sourceSha256"],
    }, sort_keys=True))
    return 0


if __name__ == "__main__":
    try:
        raise SystemExit(main())
    except CheckError as error:
        print(f"selfhost-commit-authority: {error}", file=sys.stderr)
        raise SystemExit(1)
