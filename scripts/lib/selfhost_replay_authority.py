#!/usr/bin/env python3
"""Independent verifier for H2 GenesisCode effect replay authority."""

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


FIELDS = {
    "artifact",
    "auditDate",
    "binding",
    "contentIdentitySha256",
    "decisionInventory",
    "hostMechanisms",
    "hostOracle",
    "independentVerifier",
    "kind",
    "nonclaims",
    "productionEntrypoints",
    "requestKind",
    "resultKind",
    "runtimeEvidence",
    "schema",
    "sourceModule",
    "sourceSha256",
    "spec",
    "version",
}

DECISIONS = [
    "program-hash-identity",
    "ordered-entry-presence-and-exhaustion",
    "entry-index",
    "operation-identity",
    "payload-hash-identity",
    "continuation-hash-identity",
    "request-hash-identity",
    "decision-cap-consistency",
    "response-load-admission",
    "response-hash-identity",
    "schedule-step-identity",
    "task-id-identity",
    "parent-task-identity",
    "await-edge-identity",
]

HOST_MECHANISMS = [
    "canonical-hash-observation",
    "continuation-application-after-acceptance",
    "effect-program-step-and-seal-validation",
    "log-structure-decoding",
    "response-artifact-loading-and-codec",
]

NONCLAIMS = {
    "bootstrap-fixpoint",
    "h3-h4-closure",
    "r4-2-d-closure",
    "release-qualification",
    "sh-c-closure",
    "signing-and-evidence-authority",
}

CONSTANTS = {
    "artifact": "selfhost/toolchain.gc",
    "binding": "core/effects::replay-authority",
    "decisionInventory": DECISIONS,
    "hostMechanisms": HOST_MECHANISMS,
    "hostOracle": {"removalTask": "R4.2.d", "required": False},
    "independentVerifier": "scripts/lib/selfhost_replay_authority.py",
    "kind": "genesis/selfhost-replay-authority-v0.1",
    "productionEntrypoints": ["genesis", "genesis_wasi"],
    "requestKind": "genesis/effect-replay-authority-request-v0.1",
    "resultKind": "genesis/effect-replay-authority-result-v0.1",
    "runtimeEvidence": {"allocationLimit": 32_000_000, "stepLimit": 20_000_000},
    "schema": "docs/spec/SELFHOST_REPLAY_AUTHORITY_v0.1.schema.json",
    "sourceModule": "selfhost/effect_replay_authority_v1.gc",
    "spec": "docs/spec/SELFHOST_REPLAY_AUTHORITY_v0.1.md",
    "version": "0.1.0",
}


def profile_identity(profile) -> str:
    value = copy.deepcopy(profile)
    value.pop("contentIdentitySha256", None)
    canonical = json.dumps(value, sort_keys=True, separators=(",", ":")).encode()
    return hashlib.sha256(canonical).hexdigest()


def source_identity(relative: str, data: bytes) -> str:
    digest = hashlib.sha256()
    digest.update(relative.encode())
    digest.update(b"\0")
    digest.update(data)
    digest.update(b"\0")
    return digest.hexdigest()


def validate(profile, schema, check_identity=True) -> None:
    if (
        schema.get("$schema") != "https://json-schema.org/draft/2020-12/schema"
        or schema.get("type") != "object"
        or schema.get("additionalProperties") is not False
        or set(schema.get("required", [])) != FIELDS
        or set(schema.get("properties", {})) != FIELDS
    ):
        fail("schema closure drift")
    if set(profile) != FIELDS:
        fail("profile field drift")
    for name, expected in CONSTANTS.items():
        if profile.get(name) != expected:
            fail(f"profile {name} drift")
        if name not in {"hostOracle", "runtimeEvidence"}:
            schema_value = schema["properties"].get(name, {}).get("const")
            if schema_value is not None and schema_value != expected:
                fail(f"schema {name} drift")
    if set(profile.get("nonclaims", [])) != NONCLAIMS:
        fail("profile nonclaim inventory drift")
    for name in ("contentIdentitySha256", "sourceSha256"):
        if not re.fullmatch(r"[0-9a-f]{64}", str(profile.get(name, ""))):
            fail(f"profile {name} is invalid")
    if check_identity and profile["contentIdentitySha256"] != profile_identity(profile):
        fail("profile content identity mismatch")


def read_text(root: Path, relative: str, overrides) -> str:
    if relative in overrides:
        return overrides[relative]
    return (root / relative).read_text()


def function_slice(source: str, start: str, end: str) -> str:
    begin = source.find(start)
    finish = source.find(end, begin + len(start))
    if begin < 0 or finish < 0:
        fail(f"cannot isolate production function between {start!r} and {end!r}")
    return source[begin:finish]


def static_check(root: Path, profile, overrides=None, check_artifact=True) -> None:
    overrides = overrides or {}
    source_relative = profile["sourceModule"]
    source_path = root / source_relative
    if source_path.is_symlink() or not source_path.is_file() or root.resolve() not in source_path.resolve().parents:
        fail("replay authority source is missing, escaping, or symlinked")
    source = read_text(root, source_relative, overrides)
    if source_identity(source_relative, source.encode()) != profile["sourceSha256"]:
        fail("replay authority source identity mismatch")

    manifest = read_text(root, "selfhost/toolchain_manifest.gc", overrides)
    if manifest.count(f'"{source_relative}"') != 1 or manifest.count(profile["binding"]) != 1:
        fail("replay authority manifest custody drift")

    compact = re.sub(r"\s+", " ", source)
    required_source_markers = [
        "selfhost/replay::header-request-valid?",
        "selfhost/replay::pure-request-valid?",
        "selfhost/replay::perform-request-valid?",
        "selfhost/replay::expected-schedule",
        "selfhost/replay::cap-error",
        "quote :load-error",
        "quote :unavailable",
        '"replay/program-hash-mismatch"',
        '"replay/remaining-entries"',
        '"replay/mismatch"',
        profile["requestKind"],
        profile["resultKind"],
    ]
    for marker in required_source_markers:
        if marker not in compact:
            fail(f"replay authority source marker missing: {marker}")
    if compact.count("selfhost/replay::exact-map?") < 5:
        fail("replay authority does not close every request/result observation map")

    bridge = read_text(root, "crates/gc_effects/src/replay_authority.rs", overrides)
    for marker in [
        "REPLAY_AUTHORITY_STEP_LIMIT: u64 = 20_000_000",
        "REPLAY_AUTHORITY_ALLOC_LIMIT: u64 = 32_000_000",
        "load_selfhost_coreform_toolchain_v1_with_mode",
        "result field set mismatch",
        "actual == &hash_hex(request_hash)",
        "accepted result must carry nil :code and :message",
        "rejected result must carry nonempty :code",
        "returned sealed ERROR",
        "context.reset_counters()",
    ]:
        if marker not in bridge:
            fail(f"replay host boundary marker missing: {marker}")

    runner = read_text(root, "crates/gc_effects/src/runner.rs", overrides)
    production_runner = read_text(
        root, "crates/gc_effects/src/runner_replay_authority.rs", overrides
    )
    production = function_slice(
        production_runner, "pub fn replay_with_selfhost_authority(", "fn require_accept("
    )
    for marker in [
        "ReplayAuthority::load",
        "authority.header",
        "authority.pure",
        "authority.missing_entry",
        "authority.response_load_error",
        "authority.entry",
        "require_accept",
    ]:
        if marker not in production:
            fail(f"production replay authority route missing: {marker}")
    for forbidden in [
        "replay_validate_decision_cap",
        "task_schedule_event_for",
        "entry.i !=",
        "entry.op !=",
        "entry.payload_h !=",
        "entry.cont_h !=",
        "entry.req_h !=",
        "entry.resp_h !=",
    ]:
        if forbidden in production:
            fail(f"production replay retains Rust semantic oracle: {forbidden}")
    if '#[cfg(any(test, feature = "parity-oracle"))]\npub fn replay(' not in runner:
        fail("legacy replay oracle is not parity-only")
    if '#[cfg(any(test, feature = "parity-oracle"))]\npub fn replay_with_store(' not in runner:
        fail("legacy replay-with-store oracle is not parity-only")

    effects_manifest = read_text(root, "crates/gc_effects/Cargo.toml", overrides)
    cli_manifest = read_text(root, "crates/gc_cli_driver/Cargo.toml", overrides)
    if "parity-oracle = []" not in effects_manifest:
        fail("effects parity oracle feature is missing")
    if '"gc_effects/parity-oracle"' not in cli_manifest:
        fail("CLI parity harness does not explicitly activate the legacy replay oracle")

    obligation_manifest = read_text(root, "crates/gc_obligations/Cargo.toml", overrides)
    obligation_route = read_text(root, "crates/gc_obligations/src/obligation_exec.rs", overrides)
    if 'replay-parity-oracle = ["gc_effects/parity-oracle"]' not in obligation_manifest:
        fail("obligation parity harness does not explicitly activate the legacy replay oracle")
    for marker in [
        "CoreformFrontend::Selfhost(config)",
        "gc_effects::replay_with_selfhost_authority(",
        '#[cfg(feature = "replay-parity-oracle")]\n            {\n                gc_effects::replay_with_store',
        "Rust replay oracle is disabled outside the parity harness",
    ]:
        if marker not in obligation_route:
            fail(f"obligation replay route marker missing: {marker}")
    for relative in [
        "crates/gc_obligations/src/obligation_exec_coverage.rs",
        "crates/gc_obligations/src/obligation_exec_replay.rs",
    ]:
        source_text = read_text(root, relative, overrides)
        if "replay_effect_program(" not in source_text or "replay_with_store(" in source_text:
            fail(f"obligation production replay route drift: {relative}")

    cli = read_text(root, "crates/gc_cli_driver/src/cmd_core.rs", overrides)
    replay_command = function_slice(cli, "pub(super) fn cmd_replay(", "\n}")
    if replay_command.count("gc_effects::replay_with_selfhost_authority(") != 1:
        fail("production CLI replay authority route drift")
    if replay_command.count("gc_effects::replay_with_store(") != 1:
        fail("parity CLI replay oracle route drift")
    route_start = replay_command.find("let replayed = match engine")
    if route_start < 0:
        fail("CLI replay execution match is missing")
    execution_route = replay_command[route_start:]
    rust_route = execution_route.find("FmtEngine::Rust =>")
    legacy_call = execution_route.find("gc_effects::replay_with_store(")
    selfhost_route = execution_route.find("FmtEngine::Selfhost =>")
    authority_call = execution_route.find("gc_effects::replay_with_selfhost_authority(")
    if not (0 <= rust_route < legacy_call < selfhost_route < authority_call):
        fail("CLI replay engine routing order drift")
    if '#[cfg(feature = "parity-harness")]\n        FmtEngine::Rust =>' not in execution_route:
        fail("legacy CLI replay route is not parity-harness-only")

    tests = read_text(root, "crates/gc_effects/src/tests.rs", overrides)
    mutation_labels = set(
        re.findall(r'mutations\.push\(\("([a-z-]+)"', tests)
    )
    expected_mutations = {
        "program-hash", "index", "op", "payload-h", "cont-h", "req-h",
        "decision", "cap", "resp-h", "schedule-step", "task-id", "parent-task",
        "await-edge", "missing-entry", "remaining-entry", "response-load",
    }
    if mutation_labels != expected_mutations:
        fail("replay adversarial mutation inventory drift")

    ledger = json.loads(
        read_text(root, "docs/spec/SEMANTIC_OWNERSHIP_LEDGER_v0.1.json", overrides),
        object_pairs_hook=unique_object,
    )
    rows = [row for row in ledger.get("semanticDecisions", []) if row.get("id") == "SD-REPLAY"]
    if len(rows) != 1:
        fail("SD-REPLAY ledger row missing or duplicated")
    row = rows[0]
    if row.get("currentLevel") != "H2" or row.get("fallbackReachability") != "none-proven":
        fail("SD-REPLAY ledger authority claim drift")
    for relative in [source_relative, profile["artifact"]]:
        if relative not in row.get("productionAuthorityPaths", []):
            fail(f"SD-REPLAY ledger production authority omits {relative}")
    if profile["independentVerifier"] not in row.get("verifierPaths", []):
        fail("SD-REPLAY ledger verifier custody drift")

    if check_artifact:
        artifact = read_text(root, profile["artifact"], overrides)
        if artifact.count(source_relative) != 1:
            fail("replay authority artifact source custody drift")
        for marker in (profile["requestKind"], profile["resultKind"]):
            if marker not in artifact:
                fail(f"replay authority artifact marker missing: {marker}")


def self_test(root: Path, profile, schema) -> None:
    controls = 0

    def reject_profile(mutator):
        nonlocal controls
        candidate = copy.deepcopy(profile)
        mutator(candidate)
        try:
            validate(candidate, schema)
        except CheckError:
            controls += 1
            return
        fail("profile mutation was accepted")

    reject_profile(lambda value: value.__setitem__("binding", "core/effects::legacy"))
    reject_profile(lambda value: value["decisionInventory"].pop())
    reject_profile(lambda value: value["hostMechanisms"].append("semantic-oracle"))
    reject_profile(lambda value: value["hostOracle"].__setitem__("required", True))
    reject_profile(lambda value: value["nonclaims"].remove("h3-h4-closure"))
    reject_profile(lambda value: value["runtimeEvidence"].__setitem__("stepLimit", 0))
    reject_profile(lambda value: value.__setitem__("kind", "wrong"))
    reject_profile(lambda value: value.__setitem__("contentIdentitySha256", "0" * 64))

    bad_schema = copy.deepcopy(schema)
    bad_schema["additionalProperties"] = True
    try:
        validate(profile, bad_schema)
    except CheckError:
        controls += 1
    else:
        fail("open schema mutation was accepted")

    static_mutations = [
        ("selfhost/toolchain_manifest.gc", lambda text: text.replace(profile["binding"], "legacy", 1)),
        ("crates/gc_effects/src/replay_authority.rs", lambda text: text.replace("result field set mismatch", "open result", 1)),
        ("crates/gc_effects/src/runner_replay_authority.rs", lambda text: text.replace("ReplayAuthority::load", "task_schedule_event_for", 1)),
        ("crates/gc_effects/Cargo.toml", lambda text: text.replace("parity-oracle = []", "", 1)),
        ("crates/gc_cli_driver/Cargo.toml", lambda text: text.replace('"gc_effects/parity-oracle",', "", 1)),
        ("crates/gc_cli_driver/src/cmd_core.rs", lambda text: text.replace("gc_effects::replay_with_selfhost_authority(", "gc_effects::replay_with_store(", 1)),
        ("crates/gc_obligations/src/obligation_exec.rs", lambda text: text.replace("gc_effects::replay_with_selfhost_authority(", "gc_effects::replay_with_store(", 1)),
    ]
    for relative, mutate in static_mutations:
        overrides = {relative: mutate((root / relative).read_text())}
        try:
            static_check(root, profile, overrides, check_artifact=False)
        except CheckError:
            controls += 1
        else:
            fail(f"static mutation was accepted: {relative}")

    if controls != 16:
        fail(f"negative control inventory drift: {controls}")
    print(f"selfhost-replay-authority-self-test: ok (negative_controls={controls})")


def main() -> int:
    parser = argparse.ArgumentParser()
    parser.add_argument("--root", type=Path, default=Path("."))
    parser.add_argument("--profile", type=Path, required=True)
    parser.add_argument("--schema", type=Path, required=True)
    parser.add_argument("--self-test", action="store_true")
    args = parser.parse_args()
    root = args.root.resolve()
    try:
        profile = load_json(root / args.profile)
        schema = load_json(root / args.schema)
        validate(profile, schema)
        static_check(root, profile)
        if args.self_test:
            self_test(root, profile, schema)
        print(
            "selfhost-replay-authority: ok "
            f"(decisions={len(DECISIONS)} host_oracle=none level=H2)"
        )
        return 0
    except (CheckError, OSError, json.JSONDecodeError) as error:
        print(f"selfhost-replay-authority: {error}", file=sys.stderr)
        return 1


if __name__ == "__main__":
    raise SystemExit(main())
