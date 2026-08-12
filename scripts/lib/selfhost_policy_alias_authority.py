#!/usr/bin/env python3
"""Independent verifier for the R4.2.d policy-alias authority slice."""

import argparse
import copy
import hashlib
import json
import os
import re
import subprocess
import sys
import tempfile
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


def load_json(path: Path):
    try:
        value = json.loads(path.read_text(), object_pairs_hook=unique_object)
    except (OSError, json.JSONDecodeError) as error:
        fail(f"cannot read {path}: {error}")
    if not isinstance(value, dict):
        fail(f"JSON root is not an object: {path}")
    return value


def identity(profile) -> str:
    value = copy.deepcopy(profile)
    value.pop("contentIdentitySha256", None)
    canonical = json.dumps(value, sort_keys=True, separators=(",", ":")).encode()
    return hashlib.sha256(canonical).hexdigest()


FIELDS = {
    "artifact",
    "auditDate",
    "binding",
    "compatibilityOracle",
    "contentIdentitySha256",
    "decisionInventory",
    "errorCodes",
    "independentVerifier",
    "kind",
    "nonclaims",
    "productionEntrypoints",
    "resultKind",
    "runtimeEvidence",
    "schema",
    "sourceModule",
    "sourceSha256",
    "spec",
    "version",
}


def validate(profile, schema, check_identity=True):
    if (
        schema.get("type") != "object"
        or schema.get("additionalProperties") is not False
        or set(schema.get("required", [])) != FIELDS
        or set(schema.get("properties", {})) != FIELDS
    ):
        fail("schema closure drift")
    if set(profile) != FIELDS:
        fail("profile field drift")
    constants = {
        "artifact": "selfhost/toolchain.gc",
        "binding": "core/cli::policy-authority",
        "decisionInventory": [
            "alias-normalization",
            "default-mutation",
            "default-resolution",
            "direct-hash-normalization",
            "selector-resolution",
        ],
        "errorCodes": ["policy/parse", "policy/resolve", "policy/set-default"],
        "independentVerifier": "scripts/lib/selfhost_policy_alias_authority.py",
        "kind": "genesis/selfhost-policy-alias-authority-v0.1",
        "productionEntrypoints": ["genesis", "genesis_wasi"],
        "resultKind": "genesis/policy-authority-result-v0.1",
        "runtimeEvidence": {
            "allocationLimit": 5_000_000,
            "stepLimit": 5_000_000,
            "timeoutSeconds": 30,
        },
        "schema": "docs/spec/SELFHOST_POLICY_ALIAS_AUTHORITY_v0.1.schema.json",
        "sourceModule": "selfhost/policy_authority_v1.gc",
        "spec": "docs/spec/SELFHOST_POLICY_ALIAS_AUTHORITY_v0.1.md",
        "version": "0.1.0",
    }
    for key, expected in constants.items():
        if profile.get(key) != expected:
            fail(f"profile {key} drift")
    if profile.get("compatibilityOracle") != {
        "feature": "parity-harness",
        "sunsetReviewDate": "2026-11-11",
    }:
        fail("compatibility oracle custody drift")
    expected_nonclaims = {
        "bootstrap-fixpoint",
        "effect-policy-authority",
        "evidence-verification-authority",
        "obligation-authority",
        "r4-2-d-closure",
        "release-qualification",
        "replay-authority",
        "signing-authority",
    }
    if set(profile.get("nonclaims", [])) != expected_nonclaims:
        fail("nonclaim inventory drift")
    for key in ("contentIdentitySha256", "sourceSha256"):
        if not re.fullmatch(r"[0-9a-f]{64}", str(profile.get(key, ""))):
            fail(f"invalid {key}")
    if check_identity and profile["contentIdentitySha256"] != identity(profile):
        fail("profile content identity mismatch")


def cargo_tree(root: Path, package: str) -> str:
    result = subprocess.run(
        ["cargo", "tree", "-p", package, "-e", "features", "--locked", "--offline"],
        cwd=root,
        text=True,
        capture_output=True,
    )
    if result.returncode:
        fail(f"cargo tree failed for {package}: {result.stderr.strip()}")
    return result.stdout


def static_check(root: Path, profile):
    source_path = root / profile["sourceModule"]
    if source_path.is_symlink() or not source_path.is_file() or root not in source_path.resolve().parents:
        fail("policy authority source is missing, escaping, or symlinked")
    source_hash = hashlib.sha256(source_path.read_bytes()).hexdigest()
    if source_hash != profile["sourceSha256"]:
        fail("policy authority source identity mismatch")
    manifest = (root / "selfhost/toolchain_manifest.gc").read_text()
    if manifest.count(f'"{profile["sourceModule"]}"') != 1:
        fail("policy authority source manifest custody drift")
    if manifest.count(profile["binding"]) != 1:
        fail("policy authority binding manifest custody drift")

    rust = (root / "crates/gc_cli_driver/src/policy_config.rs").read_text()
    required = [
        '#[cfg(feature = "parity-harness")]\nfn normalize_policies_config',
        '#[cfg(feature = "parity-harness")]\nfn resolve_policy_selector',
        '#[cfg(feature = "parity-harness")]\nfn rust_policy_authority',
        '"Rust policy oracle is not compiled into production"',
        '"core/cli::policy-authority"',
        "decode_policy_authority(report, operation)",
    ]
    for token in required:
        if token not in rust:
            fail(f"missing policy authority boundary: {token}")
    command = (root / "crates/gc_cli_driver/src/cmd_policy.rs").read_text()
    if command.count("authoritative_policy_decision(") != 3:
        fail("policy command authority call-site inventory drift")
    for package in ("gc_cli", "gc_wasi_cli"):
        tree = cargo_tree(root, package)
        if 'gc_cli_driver feature "parity-harness"' in tree or "gc_cli_driver_parity" in tree:
            fail(f"{package} production graph reaches policy parity authority")
    mains = (root / "crates/gc_cli/src/main.rs").read_text()
    mains += (root / "crates/gc_wasi_cli/src/main.rs").read_text()
    if "gc_cli_driver_parity" in mains or mains.count("gc_cli_driver::run") != 2:
        fail("production entrypoint custody drift")
    return {"authorityCalls": 3, "sourceSha256": source_hash}


def run_json(command, timeout):
    result = subprocess.run(command, text=True, capture_output=True, timeout=timeout)
    try:
        envelope = json.loads(result.stdout)
    except json.JSONDecodeError as error:
        fail(f"invalid JSON from {command[0]}: {error}: {result.stderr.strip()}")
    return result, envelope


def base_command(binary: Path, artifact: Path, profile):
    limits = profile["runtimeEvidence"]
    return [
        str(binary),
        "--json",
        "--selfhost-only",
        "--selfhost-artifact",
        str(artifact),
        "--coreform-frontend",
        "selfhost",
        "--step-limit",
        str(limits["stepLimit"]),
        "--max-alloc-units",
        str(limits["allocationLimit"]),
    ]


def runtime_check(root: Path, profile, binaries):
    artifact = (root / profile["artifact"]).resolve()
    timeout = profile["runtimeEvidence"]["timeoutSeconds"]
    upper = "D" * 64
    lower = "d" * 64
    observations = []
    with tempfile.TemporaryDirectory(prefix="genesis-policy-authority-") as temp:
        for binary in binaries:
            binary = binary.resolve()
            if not binary.is_file() or not os.access(binary, os.X_OK):
                fail(f"runtime entrypoint is not executable: {binary}")
            policy = Path(temp) / f"{binary.name}.toml"
            policy.write_text(
                f'version = 1\ndefault = "\u3000stable\u3000"\n\n[aliases]\n'
                f'"\u00a0stable\u00a0" = "\u2009{upper}\u2009"\n'
            )
            base = base_command(binary, artifact, profile)
            listed, list_envelope = run_json(
                base + ["policy", "list", "--policies", str(policy)], timeout
            )
            data = list_envelope.get("data", {})
            expected_aliases = [{"hash": lower, "name": "stable"}]
            if (
                listed.returncode != 0
                or list_envelope.get("ok") is not True
                or data.get("default") != "stable"
                or data.get("default_resolved") != lower
                or data.get("aliases") != expected_aliases
            ):
                fail(f"{binary.name} canonical policy list drift")

            selected, select_envelope = run_json(
                base
                + [
                    "policy",
                    "set-default",
                    f"\u2028{upper}\u2029",
                    "--policies",
                    str(policy),
                ],
                timeout,
            )
            if (
                selected.returncode != 0
                or select_envelope.get("ok") is not True
                or select_envelope.get("data", {}).get("default") != lower
            ):
                fail(f"{binary.name} direct-hash selection drift")

            before_denial = policy.read_bytes()
            denied, deny_envelope = run_json(
                base
                + [
                    "policy",
                    "set-default",
                    "missing",
                    "--policies",
                    str(policy),
                ],
                timeout,
            )
            if (
                denied.returncode != 50
                or deny_envelope.get("ok") is not False
                or deny_envelope.get("error", {}).get("code") != "policy/set-default"
                or policy.read_bytes() != before_denial
            ):
                fail(f"{binary.name} verification denial or non-mutation drift")

            empty, empty_envelope = run_json(
                base
                + ["policy", "set-default", " ", "--policies", str(policy)],
                timeout,
            )
            if (
                empty.returncode != 10
                or empty_envelope.get("ok") is not False
                or empty_envelope.get("error", {}).get("code") != "policy/parse"
                or policy.read_bytes() != before_denial
            ):
                fail(f"{binary.name} empty-selector taxonomy or non-mutation drift")

            recursive, recursive_envelope = run_json(
                base
                + ["policy", "set-default", "default", "--policies", str(policy)],
                timeout,
            )
            if (
                recursive.returncode != 50
                or recursive_envelope.get("ok") is not False
                or recursive_envelope.get("error", {}).get("code") != "policy/set-default"
                or policy.read_bytes() != before_denial
            ):
                fail(f"{binary.name} self-reference denial or non-mutation drift")

            invalid = Path(temp) / f"{binary.name}-invalid.toml"
            invalid.write_text('version = 1\n\n[aliases]\nbad = "not-a-hash"\n')
            before_parse = invalid.read_bytes()
            parsed, parse_envelope = run_json(
                base + ["policy", "list", "--policies", str(invalid)], timeout
            )
            if (
                parsed.returncode != 10
                or parse_envelope.get("ok") is not False
                or parse_envelope.get("error", {}).get("code") != "policy/parse"
                or invalid.read_bytes() != before_parse
            ):
                fail(f"{binary.name} parse denial or non-mutation drift")
            observations.append(
                {
                    "entrypoint": binary.name,
                    "canonicalHash": lower,
                    "emptyExit": empty.returncode,
                    "parseExit": parsed.returncode,
                    "selfReferenceExit": recursive.returncode,
                    "verifyExit": denied.returncode,
                }
            )
    comparable = [{key: value for key, value in item.items() if key != "entrypoint"} for item in observations]
    if comparable[0] != comparable[1]:
        fail("native/WASI policy authority divergence")
    return observations


def mutation_controls(profile, schema):
    edits = [
        ("binding", lambda item: item.__setitem__("binding", "core/cli::policy-list-request")),
        ("decision", lambda item: item["decisionInventory"].pop()),
        ("errors", lambda item: item["errorCodes"].pop()),
        ("oracle", lambda item: item["compatibilityOracle"].__setitem__("feature", "default")),
        ("entrypoint", lambda item: item.__setitem__("productionEntrypoints", ["genesis"])),
        ("source", lambda item: item.__setitem__("sourceModule", "selfhost/unknown.gc")),
        ("limits", lambda item: item["runtimeEvidence"].__setitem__("stepLimit", 0)),
        ("nonclaim", lambda item: item["nonclaims"].pop()),
        ("unknown", lambda item: item.__setitem__("unexpected", True)),
    ]
    rejected = 0
    for label, edit in edits:
        candidate = copy.deepcopy(profile)
        edit(candidate)
        candidate["contentIdentitySha256"] = identity(candidate)
        try:
            validate(candidate, schema)
        except CheckError:
            rejected += 1
            continue
        fail(f"self-test accepted authority mutation: {label}")
    stale = copy.deepcopy(profile)
    stale["auditDate"] = "2026-08-12"
    try:
        validate(stale, schema)
    except CheckError:
        return rejected + 1
    fail("self-test accepted stale profile identity")


def main():
    parser = argparse.ArgumentParser()
    parser.add_argument("--root", type=Path, default=Path.cwd())
    parser.add_argument("--profile", type=Path, required=True)
    parser.add_argument("--schema", type=Path, required=True)
    parser.add_argument("--refresh-identity", action="store_true")
    parser.add_argument("--self-test", action="store_true")
    parser.add_argument("--runtime", action="store_true")
    parser.add_argument("--genesis-bin", type=Path)
    parser.add_argument("--genesis-wasi-bin", type=Path)
    args = parser.parse_args()
    root = args.root.resolve()
    profile_path = root / args.profile
    profile = load_json(profile_path)
    schema = load_json(root / args.schema)
    if args.refresh_identity:
        source = root / profile["sourceModule"]
        profile["sourceSha256"] = hashlib.sha256(source.read_bytes()).hexdigest()
        profile["contentIdentitySha256"] = identity(profile)
        profile_path.write_text(json.dumps(profile, indent=2) + "\n")
        print(f"selfhost-policy-alias-authority: refreshed {args.profile}")
        return
    validate(profile, schema)
    static = static_check(root, profile)
    controls = mutation_controls(profile, schema) if args.self_test else 0
    runtime = None
    if args.runtime:
        if not args.genesis_bin or not args.genesis_wasi_bin:
            fail("runtime mode requires native and WASI binaries")
        runtime = runtime_check(root, profile, [args.genesis_bin, args.genesis_wasi_bin])
    print(
        json.dumps(
            {
                "kind": "genesis/selfhost-policy-alias-authority-check-v0.1",
                "mutationControls": controls,
                "ok": True,
                "profileIdentitySha256": identity(profile),
                "runtime": runtime,
                "static": static,
            },
            sort_keys=True,
            separators=(",", ":"),
        )
    )


if __name__ == "__main__":
    try:
        main()
    except CheckError as error:
        print(f"selfhost-policy-alias-authority: {error}", file=sys.stderr)
        raise SystemExit(1)
