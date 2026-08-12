#!/usr/bin/env python3
"""Independent verifier for the partial R4.2.d obligation authority."""

import argparse
import copy
import hashlib
import json
import os
import re
import subprocess
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
    "contentIdentitySha256",
    "hostFacts",
    "independentVerifier",
    "kind",
    "migratedObligations",
    "nonclaims",
    "productionEntrypoints",
    "residualObligations",
    "resultKind",
    "runtimeEvidence",
    "schema",
    "sourceModule",
    "sourceSha256",
    "spec",
    "version",
}

MIGRATED = ["core/obligation::budgets", "core/obligation::unit-tests"]
RESIDUAL = [
    "core/obligation::ai-style",
    "core/obligation::capabilities-declared",
    "core/obligation::concurrency-replay",
    "core/obligation::coverage",
    "core/obligation::coverage-decision",
    "core/obligation::coverage-mcdc",
    "core/obligation::determinism",
    "core/obligation::gfx-api-stability",
    "core/obligation::gfx-frame-budgets",
    "core/obligation::gfx-golden-images",
    "core/obligation::lint",
    "core/obligation::property-tests",
    "core/obligation::replayable-tests",
    "core/obligation::stage1-validation",
    "core/obligation::translation-validation",
    "core/obligation::typecheck",
    "core/obligation::typecheck-strict",
    "core/obligation::preflight",
]


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
        "binding": "core/cli::obligation-authority",
        "hostFacts": [
            "actual-value-hash",
            "effect-entry-count",
            "effect-log-artifact",
            "effect-log-byte-count",
            "expected-value-hash",
            "sealed-error-status",
            "step-count",
            "test-identity",
        ],
        "independentVerifier": "scripts/lib/selfhost_obligation_authority.py",
        "kind": "genesis/selfhost-obligation-authority-v0.1",
        "migratedObligations": MIGRATED,
        "productionEntrypoints": ["genesis", "genesis_wasi"],
        "residualObligations": RESIDUAL,
        "resultKind": "genesis/obligation-authority-result-v0.1",
        "runtimeEvidence": {
            "allocationLimit": 5_000_000,
            "stepLimit": 5_000_000,
            "timeoutSeconds": 60,
        },
        "schema": "docs/spec/SELFHOST_OBLIGATION_AUTHORITY_v0.1.schema.json",
        "sourceModule": "selfhost/obligation_authority_v1.gc",
        "spec": "docs/spec/SELFHOST_OBLIGATION_AUTHORITY_v0.1.md",
        "version": "0.1.0",
    }
    for key, expected in constants.items():
        if profile.get(key) != expected:
            fail(f"profile {key} drift")
    expected_nonclaims = {
        "bootstrap-fixpoint",
        "effect-policy-authority",
        "evidence-verification-authority",
        "r4-2-d-closure",
        "release-qualification",
        "replay-authority",
        "sd-obligation-h2",
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


def validate_bridge(bridge: str) -> None:
    required = [
        'env.get("core/cli::obligation-authority")',
        "evaluate_obligation_with_authority(",
        "unit_test_observations(",
        "budget_observations(",
        "validate_unit_report(",
        "validate_budget_report(",
        "if frontend_is_rust(frontend)",
        "resolved_authority_frontend = default_coreform_frontend();",
        "rust_frontend_selection_does_not_replace_selfhost_obligation_authority",
    ]
    for token in required:
        if token not in bridge:
            fail(f"missing obligation authority boundary: {token}")


def static_check(root: Path, profile):
    source_path = root / profile["sourceModule"]
    if source_path.is_symlink() or not source_path.is_file() or root not in source_path.resolve().parents:
        fail("obligation authority source is missing, escaping, or symlinked")
    source_hash = hashlib.sha256(source_path.read_bytes()).hexdigest()
    if source_hash != profile["sourceSha256"]:
        fail("obligation authority source identity mismatch")
    manifest = (root / "selfhost/toolchain_manifest.gc").read_text()
    if manifest.count(f'"{profile["sourceModule"]}"') != 1:
        fail("obligation authority source manifest custody drift")
    if manifest.count(profile["binding"]) != 1:
        fail("obligation authority binding manifest custody drift")

    bridge = (root / "crates/gc_obligations/src/obligation_authority.rs").read_text()
    validate_bridge(bridge)
    types_api = (root / "crates/gc_obligations/src/obligations/types_api.rs").read_text()
    if types_api.count("obligation_unit_tests(&store, &manifest, &test_runs, &frontend, limits)") != 1:
        fail("unit-test production authority call-site drift")
    if types_api.count("obligation_budgets(&store, &manifest, &test_runs, &frontend, limits)") != 1:
        fail("budget production authority call-site drift")
    unit_host = (root / "crates/gc_obligations/src/obligation_exec.rs").read_text()
    budget_host = (root / "crates/gc_obligations/src/obligation_exec_budgets.rs").read_text()
    test_host = (root / "crates/gc_obligations/src/obligations/test_exec.rs").read_text()
    stage_host = (root / "crates/gc_obligations/src/obligation_stage.rs").read_text()
    forbidden = [
        "t.steps >",
        "t.effect_entries >",
        "t.effect_log_bytes >",
        '" exceeded max_steps_per_test: "',
        "fv_hash == expected_hash",
        "tr.ok",
    ]
    combined = unit_host + budget_host + test_host + stage_host
    for token in forbidden:
        if token in combined:
            fail(f"reachable host obligation decision restored: {token}")
    if unit_host.count("ObligationAuthorityOperation::UnitTests") != 1:
        fail("unit-test authority dispatch drift")
    if budget_host.count("ObligationAuthorityOperation::Budgets") != 1:
        fail("budget authority dispatch drift")
    if stage_host.count("ObligationAuthorityOperation::UnitTests") != 1:
        fail("translation validation unit-test authority dispatch drift")
    for package in ("gc_cli", "gc_wasi_cli"):
        tree = cargo_tree(root, package)
        if 'gc_obligations feature "parity-oracle"' in tree:
            fail(f"{package} production graph activates obligation parity oracle")
    return {"migrated": len(MIGRATED), "residual": len(RESIDUAL), "sourceSha256": source_hash}


def run_case(binary: Path, artifact: Path, root: Path, fixture: str, profile):
    limits = profile["runtimeEvidence"]
    result = subprocess.run(
        [
            str(binary),
            "test",
            "--pkg",
            str(root / fixture / "package.toml"),
            "--selfhost-artifact",
            str(artifact),
            "--step-limit",
            str(limits["stepLimit"]),
            "--max-alloc-units",
            str(limits["allocationLimit"]),
            "--json",
        ],
        cwd=root,
        text=True,
        capture_output=True,
        timeout=limits["timeoutSeconds"],
        env={**os.environ, "GENESIS_OBLIGATION_CACHE_DISABLE": "1"},
    )
    try:
        envelope = json.loads(result.stdout)
    except json.JSONDecodeError as error:
        fail(f"invalid JSON from {binary.name}/{fixture}: {error}: {result.stderr.strip()}")
    facts = []
    for item in envelope.get("data", {}).get("obligations", []):
        if item.get("name") in MIGRATED:
            facts.append((item.get("name"), item.get("ok"), item.get("errors")))
    return result.returncode, envelope.get("ok"), facts


def runtime_check(root: Path, profile, binaries):
    artifact = (root / profile["artifact"]).resolve()
    fixtures = [
        ("tests/spec/pkg_basic", 0, True),
        ("tests/spec/pkg_fail_unit", 30, False),
        ("tests/spec/pkg_fail_budgets", 30, False),
    ]
    all_observations = []
    for binary in binaries:
        binary = binary.resolve()
        if not binary.is_file() or not os.access(binary, os.X_OK):
            fail(f"runtime entrypoint is not executable: {binary}")
        observations = []
        for fixture, expected_exit, expected_ok in fixtures:
            observed = run_case(binary, artifact, root, fixture, profile)
            if observed[0] != expected_exit or observed[1] is not expected_ok:
                fail(f"{binary.name}/{fixture} disposition drift: {observed[:2]}")
            observations.append(observed)
        all_observations.append(observations)
    if any(observations != all_observations[0] for observations in all_observations[1:]):
        fail("native/WASI obligation authority observations differ")
    return all_observations[0]


def self_test(root: Path, profile, schema):
    mutations = []
    for label, mutate in [
        ("binding", lambda p: p.__setitem__("binding", "core/cli::wrong")),
        ("migrated", lambda p: p.__setitem__("migratedObligations", MIGRATED[:1])),
        ("residual", lambda p: p.__setitem__("residualObligations", RESIDUAL[:-1])),
        ("host-facts", lambda p: p.__setitem__("hostFacts", p["hostFacts"][:-1])),
        ("source", lambda p: p.__setitem__("sourceSha256", "0" * 64)),
        ("nonclaim", lambda p: p.__setitem__("nonclaims", p["nonclaims"][:-1])),
    ]:
        candidate = copy.deepcopy(profile)
        mutate(candidate)
        candidate["contentIdentitySha256"] = identity(candidate)
        try:
            validate(candidate, schema)
            if label == "source":
                static_check(root, candidate)
        except CheckError:
            mutations.append(label)
        else:
            fail(f"mutation was not rejected: {label}")
    bridge = (root / "crates/gc_obligations/src/obligation_authority.rs").read_text()
    redirected = "resolved_authority_frontend = default_coreform_frontend();"
    try:
        validate_bridge(bridge.replace(redirected, "", 1))
    except CheckError:
        mutations.append("rust-frontend-redirection")
    else:
        fail("mutation was not rejected: rust-frontend-redirection")
    return mutations


def main(argv=None):
    parser = argparse.ArgumentParser()
    parser.add_argument("--root", type=Path, default=Path.cwd())
    parser.add_argument("--profile", type=Path, required=True)
    parser.add_argument("--schema", type=Path, required=True)
    parser.add_argument("--self-test", action="store_true")
    parser.add_argument("--refresh-identity", action="store_true")
    parser.add_argument("--runtime", action="store_true")
    parser.add_argument("--binary", action="append", type=Path, default=[])
    args = parser.parse_args(argv)
    root = args.root.resolve()
    profile_path = args.profile if args.profile.is_absolute() else root / args.profile
    schema_path = args.schema if args.schema.is_absolute() else root / args.schema
    profile = load_json(profile_path)
    schema = load_json(schema_path)
    if args.refresh_identity:
        source = root / profile["sourceModule"]
        profile["sourceSha256"] = hashlib.sha256(source.read_bytes()).hexdigest()
        profile["contentIdentitySha256"] = identity(profile)
        profile_path.write_text(json.dumps(profile, indent=2, sort_keys=True) + "\n")
    validate(profile, schema)
    report = {"static": static_check(root, profile)}
    if args.self_test:
        report["mutationsRejected"] = self_test(root, profile, schema)
    if args.runtime:
        if not args.binary:
            fail("--runtime requires at least one --binary")
        report["runtime"] = runtime_check(root, profile, args.binary)
    print(json.dumps(report, sort_keys=True))


if __name__ == "__main__":
    try:
        main()
    except CheckError as error:
        print(f"selfhost-obligation-authority: {error}", file=sys.stderr)
        raise SystemExit(1)
