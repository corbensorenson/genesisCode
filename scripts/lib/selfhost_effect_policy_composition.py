#!/usr/bin/env python3
"""Independent verifier for the partial R4.2.d effect-policy composition slice."""

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
    "decisionInventory",
    "hostOracle",
    "independentVerifier",
    "kind",
    "maxPolicyOperations",
    "nonclaims",
    "productionEntrypoints",
    "requestKind",
    "residualDecisionInventory",
    "resultKind",
    "runtimeEvidence",
    "schema",
    "sourceModule",
    "sourceSha256",
    "spec",
    "version",
}

DECISIONS = [
    "baseline-operation-admission",
    "canonical-log-cap-descriptor",
    "per-operation-allow-precedence",
]

RESIDUALS = {
    "candidate-operation-inventory",
    "crypto-and-signing-policy",
    "database-policy",
    "device-and-graphics-policy",
    "effect-execution-and-hard-cancellation",
    "ffi-plugin-and-model-policy",
    "filesystem-policy",
    "global-log-store-refs-policy",
    "network-policy",
    "path-and-secret-resolution",
    "process-policy",
    "replay-execution-and-validation",
    "runtime-and-task-resource-policy",
    "toml-syntax-and-type-decoding",
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
        "binding": "core/effects::policy-authority",
        "decisionInventory": DECISIONS,
        "hostOracle": {"required": True, "removalTask": "R4.2.d"},
        "independentVerifier": "scripts/lib/selfhost_effect_policy_composition.py",
        "kind": "genesis/selfhost-effect-policy-composition-v0.1",
        "maxPolicyOperations": 4096,
        "productionEntrypoints": ["genesis", "genesis_wasi"],
        "requestKind": "genesis/effect-policy-authority-request-v0.1",
        "resultKind": "genesis/effect-policy-authority-result-v0.1",
        "runtimeEvidence": {
            "allocationLimit": 20_000_000,
            "stepLimit": 20_000_000,
            "timeoutSeconds": 30,
        },
        "schema": "docs/spec/SELFHOST_EFFECT_POLICY_COMPOSITION_v0.1.schema.json",
        "sourceModule": "selfhost/effect_policy_authority_v1.gc",
        "spec": "docs/spec/SELFHOST_EFFECT_POLICY_COMPOSITION_v0.1.md",
        "version": "0.1.0",
    }
    for key, expected in constants.items():
        if profile.get(key) != expected:
            fail(f"profile {key} drift")
    if set(profile.get("residualDecisionInventory", [])) != RESIDUALS:
        fail("residual decision inventory drift")
    if set(profile.get("nonclaims", [])) != {
        "bootstrap-fixpoint",
        "effect-policy-h2",
        "host-oracle-removal",
        "r4-2-d-closure",
        "release-qualification",
        "replay-authority",
        "sh-c-closure",
    }:
        fail("nonclaim inventory drift")
    for key in ("contentIdentitySha256", "sourceSha256"):
        if not re.fullmatch(r"[0-9a-f]{64}", str(profile.get(key, ""))):
            fail(f"invalid {key}")
    if check_identity and profile["contentIdentitySha256"] != identity(profile):
        fail("profile content identity mismatch")


def source_files(root: Path):
    for crate in ("gc_cli_driver", "gc_obligations"):
        yield from (root / "crates" / crate / "src").rglob("*.rs")


def static_check(root: Path, profile):
    source_path = root / profile["sourceModule"]
    if source_path.is_symlink() or not source_path.is_file() or root not in source_path.resolve().parents:
        fail("effect-policy source is missing, escaping, or symlinked")
    source_hash = hashlib.sha256(source_path.read_bytes()).hexdigest()
    if source_hash != profile["sourceSha256"]:
        fail("effect-policy source identity mismatch")

    manifest = (root / "selfhost/toolchain_manifest.gc").read_text()
    if manifest.count(f'"{profile["sourceModule"]}"') != 1:
        fail("effect-policy source manifest custody drift")
    if manifest.count(profile["binding"]) != 1:
        fail("effect-policy binding manifest custody drift")

    authority = (root / "crates/gc_effects/src/policy_authority.rs").read_text()
    required_authority = [
        "const MAX_POLICY_OPS: usize = 4_096;",
        "const POLICY_AUTHORITY_STEP_LIMIT: u64 = 20_000_000;",
        "const POLICY_AUTHORITY_ALLOC_LIMIT: u64 = 20_000_000;",
        profile["requestKind"],
        profile["resultKind"],
        'get("core/effects::policy-authority")',
        "let request_hash = hash_term(&request);",
        "contradicts independently reconstructed policy composition",
        "op_policy.authorized_cap = Some(cap);",
    ]
    for token in required_authority:
        if token not in authority:
            fail(f"missing effect-policy boundary token: {token}")

    policy = (root / "crates/gc_effects/src/policy.rs").read_text()
    if "pub fn load_with_selfhost_authority(" not in policy:
        fail("self-host policy loader is missing")
    runner = (root / "crates/gc_effects/src/runner_response_budget.rs").read_text()
    if "policy.authorized_cap(op)" not in runner:
        fail("effect log does not consume the authorized capability descriptor")

    driver = (root / "crates/gc_cli_driver/src/lib.rs").read_text()
    if driver.count("CapsPolicy::load(path)") != 1 or driver.count("load_with_selfhost_authority(") != 1:
        fail("CLI policy authority loader inventory drift")
    if '#[cfg(feature = "parity-harness")]\n        gc_obligations::CoreformFrontend::Rust => CapsPolicy::load(path)' not in driver:
        fail("CLI Rust compatibility loader is not compile-time gated")
    if 'Rust effect-policy authority is not compiled into production' not in driver:
        fail("CLI production Rust route does not fail closed")

    call_sites = 0
    direct_loads = []
    for path in source_files(root):
        text = path.read_text()
        call_sites += text.count("load_caps_policy(cli,")
        if "CapsPolicy::load(" in text:
            direct_loads.append(path.relative_to(root).as_posix())
    if call_sites != 10:
        fail(f"production self-host policy call-site inventory drift: {call_sites}")
    if sorted(direct_loads) != [
        "crates/gc_cli_driver/src/lib.rs",
        "crates/gc_obligations/src/obligation_authority_preflight.rs",
    ]:
        fail(f"unexpected production direct policy loader: {direct_loads}")

    preflight = (root / "crates/gc_obligations/src/obligation_authority_preflight.rs").read_text()
    if preflight.count("load_with_selfhost_authority(") != 1 or "CoreformFrontend::Rust => CapsPolicy::load(path)" not in preflight:
        fail("preflight effect-policy authority routing drift")
    task_profile = (root / "crates/gc_cli_driver/src/pkg_runtime_profile.rs").read_text()
    if 'CapsPolicy::from_toml_str("allow = [\\"core/task::spawn\\", \\"core/task::await\\"]")' not in task_profile:
        fail("declared internal task-policy residual disappeared without migration")

    tests = (root / "crates/gc_effects/src/policy_tests.rs").read_text()
    for name in (
        "selfhost_authority_composes_admission_and_canonical_caps",
        "selfhost_authority_rejects_unbounded_operation_inventories_before_evaluation",
    ):
        if tests.count(f"fn {name}()") != 1:
            fail(f"missing focused authority control: {name}")

    ledger = load_json(root / "docs/spec/SEMANTIC_OWNERSHIP_LEDGER_v0.1.json")
    rows = [row for row in ledger.get("semanticDecisions", []) if row.get("id") == "SD-EFFECT-POLICY"]
    if len(rows) != 1 or rows[0].get("currentLevel") is not None or rows[0].get("fallbackReachability") != "host-authoritative":
        fail("partial effect-policy slice was promoted beyond its evidence")
    return {
        "callSites": call_sites,
        "decisions": len(DECISIONS),
        "residualDecisions": len(RESIDUALS),
        "sourceSha256": source_hash,
    }


def mutation_controls(profile, schema):
    edits = [
        ("binding", lambda item: item.__setitem__("binding", "core/cli::policy-authority")),
        ("decision", lambda item: item["decisionInventory"].pop()),
        ("oracle", lambda item: item["hostOracle"].__setitem__("required", False)),
        ("limit", lambda item: item.__setitem__("maxPolicyOperations", 0)),
        ("nonclaim", lambda item: item["nonclaims"].pop()),
        ("residual", lambda item: item["residualDecisionInventory"].pop()),
        ("request", lambda item: item.__setitem__("requestKind", "unknown")),
        ("runtime", lambda item: item["runtimeEvidence"].__setitem__("stepLimit", 0)),
        ("source", lambda item: item.__setitem__("sourceModule", "selfhost/unknown.gc")),
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
    stale["auditDate"] = "2026-08-13"
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
    args = parser.parse_args()
    root = args.root.resolve()
    profile_path = root / args.profile
    profile = load_json(profile_path)
    schema = load_json(root / args.schema)
    if args.refresh_identity:
        profile["sourceSha256"] = hashlib.sha256((root / profile["sourceModule"]).read_bytes()).hexdigest()
        profile["contentIdentitySha256"] = identity(profile)
        profile_path.write_text(json.dumps(profile, indent=2) + "\n")
        print(f"selfhost-effect-policy-composition: refreshed {args.profile}")
        return
    validate(profile, schema)
    static = static_check(root, profile)
    controls = mutation_controls(profile, schema) if args.self_test else 0
    print(json.dumps({
        "kind": "genesis/selfhost-effect-policy-composition-check-v0.1",
        "mutationControls": controls,
        "ok": True,
        "profileIdentitySha256": identity(profile),
        "static": static,
    }, sort_keys=True, separators=(",", ":")))


if __name__ == "__main__":
    try:
        main()
    except CheckError as error:
        print(f"selfhost-effect-policy-composition: {error}", file=sys.stderr)
        raise SystemExit(1)
