#!/usr/bin/env python3
"""Independently verify the GenesisCode frontend production authority profile."""

from __future__ import annotations

import argparse
import copy
import hashlib
import json
import os
import re
import subprocess
import tempfile
from pathlib import Path
from typing import Any, Callable


class ContractError(ValueError):
    pass


ROOT_FIELDS = {
    "artifact",
    "binding",
    "contentIdentitySha256",
    "frontendProfile",
    "kind",
    "malformedCases",
    "moduleHashDomainHex",
    "nonclaims",
    "productionEntrypoints",
    "spanUnit",
    "validCases",
    "version",
}
VALID_FIELDS = {"canonicalSource", "id", "moduleHashHex", "source"}
MALFORMED_FIELDS = {
    "byteOffset",
    "errorCode",
    "exitCode",
    "id",
    "protocolCode",
    "source",
}
EXPECTED_SCALARS = {
    "artifact": "selfhost/toolchain.gc",
    "binding": "core/cli::frontend-module",
    "frontendProfile": "genesis/coreform-canon-hash-v0.2",
    "kind": "genesis/selfhost-frontend-authority-v0.1",
    "moduleHashDomainHex": "474376302e32006d6f64756c6500",
    "spanUnit": "utf8-byte",
    "version": "0.1.0",
}
EXPECTED_ENTRYPOINTS = ["genesis", "genesis_wasi"]
EXPECTED_NONCLAIMS = [
    "bootstrap-fixpoint",
    "exhaustive-language-conformance",
    "h2-ledger-promotion",
    "independent-full-parser",
]
HEX64 = re.compile(r"^[0-9a-f]{64}$")
CASE_ID = re.compile(r"^[a-z0-9][a-z0-9-]*$")
PARSE_DATA_FIELDS = {
    "canonical",
    "canonical_source",
    "engine",
    "file",
    "frontend_profile",
    "module_hash_hex",
    "selfhost_artifact",
    "source_bytes",
    "source_span",
    "span_unit",
}
FMT_DATA_FIELDS = {
    "changed",
    "check",
    "engine",
    "file",
    "frontend_profile",
    "module_hash_hex",
    "selfhost_artifact",
    "source_span",
    "span_unit",
}


def fail(message: str) -> None:
    raise ContractError(message)


def no_duplicate_pairs(pairs: list[tuple[str, Any]]) -> dict[str, Any]:
    result: dict[str, Any] = {}
    for key, value in pairs:
        if key in result:
            fail(f"duplicate JSON key: {key}")
        result[key] = value
    return result


def load_json(path: Path) -> Any:
    try:
        return json.loads(path.read_text(encoding="utf-8"), object_pairs_hook=no_duplicate_pairs)
    except (OSError, json.JSONDecodeError) as error:
        fail(f"cannot read {path}: {error}")


def canonical_identity(value: dict[str, Any]) -> str:
    payload = {key: item for key, item in value.items() if key != "contentIdentitySha256"}
    encoded = json.dumps(
        payload, sort_keys=True, separators=(",", ":"), ensure_ascii=True
    ).encode("utf-8")
    return hashlib.sha256(encoded).hexdigest()


def require_closed(value: Any, fields: set[str], label: str) -> dict[str, Any]:
    if not isinstance(value, dict):
        fail(f"{label} must be an object")
    missing = sorted(fields - set(value))
    unknown = sorted(set(value) - fields)
    if missing or unknown:
        fail(f"{label} field drift: missing={missing}, unknown={unknown}")
    return value


def require_string(value: Any, label: str) -> str:
    if not isinstance(value, str):
        fail(f"{label} must be a string")
    return value


def require_int(value: Any, label: str) -> int:
    if isinstance(value, bool) or not isinstance(value, int):
        fail(f"{label} must be an integer")
    return value


def blake3_hex(data: bytes) -> str:
    try:
        import blake3  # type: ignore[import-not-found]
    except ImportError as error:
        fail(f"independent Python blake3 module is required: {error}")
    return blake3.blake3(data).hexdigest()


def module_hash(profile: dict[str, Any], canonical_source: str) -> str:
    domain = bytes.fromhex(require_string(profile["moduleHashDomainHex"], "module hash domain"))
    return blake3_hex(domain + canonical_source.encode("utf-8"))


def validate_schema(schema: Any) -> None:
    root = require_closed(
        schema,
        {"$defs", "$id", "$schema", "additionalProperties", "properties", "required", "type"},
        "schema",
    )
    if root["$schema"] != "https://json-schema.org/draft/2020-12/schema":
        fail("schema draft drift")
    if root["$id"] != "https://genesiscode.dev/schemas/selfhost-frontend-authority-v0.1.json":
        fail("schema ID drift")
    if root["type"] != "object" or root["additionalProperties"] is not False:
        fail("schema root must be a closed object")
    if set(root["required"]) != ROOT_FIELDS or set(root["properties"]) != ROOT_FIELDS:
        fail("schema root field inventory drift")
    definitions = root["$defs"]
    if not isinstance(definitions, dict) or set(definitions) != {"malformedCase", "validCase"}:
        fail("schema definition inventory drift")
    for name, fields in (("validCase", VALID_FIELDS), ("malformedCase", MALFORMED_FIELDS)):
        definition = definitions[name]
        if not isinstance(definition, dict):
            fail(f"schema {name} definition must be an object")
        if definition.get("type") != "object" or definition.get("additionalProperties") is not False:
            fail(f"schema {name} must be a closed object")
        if set(definition.get("required", [])) != fields:
            fail(f"schema {name} required fields drift")
        if set(definition.get("properties", {})) != fields:
            fail(f"schema {name} property inventory drift")


def validate_sorted_unique_cases(cases: Any, fields: set[str], label: str) -> list[dict[str, Any]]:
    if not isinstance(cases, list) or not cases:
        fail(f"{label} must be a non-empty array")
    checked: list[dict[str, Any]] = []
    for index, case in enumerate(cases):
        item = require_closed(case, fields, f"{label}[{index}]")
        case_id = require_string(item["id"], f"{label}[{index}].id")
        if not CASE_ID.fullmatch(case_id):
            fail(f"{label}[{index}].id is not canonical")
        checked.append(item)
    ids = [item["id"] for item in checked]
    if ids != sorted(ids) or len(ids) != len(set(ids)):
        fail(f"{label} IDs must be sorted and unique")
    return checked


def validate_profile(
    profile: Any,
    schema: Any,
    *,
    check_identity: bool = True,
    allow_identity_placeholder: bool = False,
) -> dict[str, Any]:
    validate_schema(schema)
    root = require_closed(profile, ROOT_FIELDS, "profile")
    for field, expected in EXPECTED_SCALARS.items():
        if root[field] != expected:
            fail(f"profile {field} drift")
    if root["productionEntrypoints"] != EXPECTED_ENTRYPOINTS:
        fail("production entrypoint inventory or order drift")
    if root["nonclaims"] != EXPECTED_NONCLAIMS:
        fail("nonclaim inventory or order drift")

    valid_cases = validate_sorted_unique_cases(root["validCases"], VALID_FIELDS, "validCases")
    if len(valid_cases) < 3:
        fail("validCases must cover at least three independent vectors")
    malformed_cases = validate_sorted_unique_cases(
        root["malformedCases"], MALFORMED_FIELDS, "malformedCases"
    )

    for case in valid_cases:
        source = require_string(case["source"], f"valid case {case['id']} source")
        canonical = require_string(
            case["canonicalSource"], f"valid case {case['id']} canonicalSource"
        )
        declared_hash = require_string(
            case["moduleHashHex"], f"valid case {case['id']} moduleHashHex"
        )
        if not source or not canonical:
            fail(f"valid case {case['id']} source and canonical output must be non-empty")
        if not HEX64.fullmatch(declared_hash):
            fail(f"valid case {case['id']} module hash is not lowercase hex")
        computed = module_hash(root, canonical)
        if declared_hash != computed:
            fail(f"valid case {case['id']} module hash mismatch: {declared_hash} != {computed}")

    for case in malformed_cases:
        source = require_string(case["source"], f"malformed case {case['id']} source")
        offset = require_int(case["byteOffset"], f"malformed case {case['id']} byteOffset")
        if offset < 0 or offset > len(source.encode("utf-8")):
            fail(f"malformed case {case['id']} byte offset is outside the UTF-8 source")
        if case["errorCode"] != "selfhost/error" or case["exitCode"] != 10:
            fail(f"malformed case {case['id']} error contract drift")
        protocol_code = require_string(
            case["protocolCode"], f"malformed case {case['id']} protocolCode"
        )
        if not protocol_code.startswith("core/parse/"):
            fail(f"malformed case {case['id']} protocol code drift")

    identity = root["contentIdentitySha256"]
    if allow_identity_placeholder and identity == "TO_BE_REFRESHED":
        pass
    elif not isinstance(identity, str) or not HEX64.fullmatch(identity):
        fail("profile contentIdentitySha256 must be lowercase hex")
    if check_identity and identity != canonical_identity(root):
        fail("profile content identity mismatch")
    return root


def extract_braced_blocks(source: str, marker: str) -> list[str]:
    blocks: list[str] = []
    offset = 0
    while True:
        found = source.find(marker, offset)
        if found < 0:
            return blocks
        start = source.find("{", found + len(marker))
        if start < 0:
            fail(f"source marker has no braced body: {marker}")
        depth = 0
        for index in range(start, len(source)):
            char = source[index]
            if char == "{":
                depth += 1
            elif char == "}":
                depth -= 1
                if depth == 0:
                    blocks.append(source[start : index + 1])
                    offset = index + 1
                    break
        else:
            fail(f"source marker has an unterminated body: {marker}")


def read_source_contract(root: Path) -> dict[str, str]:
    paths = [
        "selfhost/cli_coreform_v1.gc",
        "selfhost/toolchain_manifest.gc",
        "crates/gc_cli_driver/src/selfhost_bridge.rs",
        "crates/gc_cli_driver/src/cmd_core.rs",
        "crates/gc_cli_driver/src/cmd_source.rs",
        "crates/gc_obligations/src/obligations/frontend_module_ops.rs",
        "crates/gc_obligations/src/obligations/manifest_hashing.rs",
    ]
    sources: dict[str, str] = {}
    for relative in paths:
        path = root / relative
        try:
            sources[relative] = path.read_text(encoding="utf-8")
        except OSError as error:
            fail(f"cannot read authority source {relative}: {error}")
    return sources


def validate_source_contract(profile: dict[str, Any], sources: dict[str, str]) -> None:
    binding = profile["binding"]
    gc_source = sources["selfhost/cli_coreform_v1.gc"]
    manifest = sources["selfhost/toolchain_manifest.gc"]
    bridge = sources["crates/gc_cli_driver/src/selfhost_bridge.rs"]
    core = sources["crates/gc_cli_driver/src/cmd_core.rs"]
    source_cli = sources["crates/gc_cli_driver/src/cmd_source.rs"]
    obligation_bridge = sources[
        "crates/gc_obligations/src/obligations/frontend_module_ops.rs"
    ]
    manifest_hashing = sources[
        "crates/gc_obligations/src/obligations/manifest_hashing.rs"
    ]

    required_gc_markers = [
        f"(def {binding}",
        ':kind "genesis/frontend-module-v0.1"',
        ':profile "genesis/coreform-canon-hash-v0.2"',
        ":span-unit (quote :utf8-byte)",
        ":canonical-source canonical",
        ":module-h module-h",
    ]
    for marker in required_gc_markers:
        if marker not in gc_source:
            fail(f"GenesisCode frontend authority marker missing: {marker}")
    if manifest.count(binding) != 1:
        fail("toolchain manifest must require the frontend authority binding exactly once")

    for label, source in (("CLI bridge", bridge), ("obligation bridge", obligation_bridge)):
        if f'get("{binding}")' not in source:
            fail(f"{label} does not require the production frontend binding")
        for forbidden in (
            'get("selfhost/parse::parse-module")',
            'get("selfhost/hash::hash-module")',
        ):
            if forbidden in source:
                fail(f"{label} retains a forbidden frontend semantic fallback: {forbidden}")

    if "selfhost_frontend_module(ctx, env, src)?.forms" not in bridge:
        fail("CLI parse/canonicalize compatibility helper does not delegate to atomic authority")
    if "selfhost_frontend_module(ctx, env, src)?.forms" not in obligation_bridge:
        fail("obligation parse helper does not delegate to atomic authority")
    if core.count("frontend.module_hash") < 2:
        fail("run/replay do not retain GenesisCode-produced module identity")
    if "forms: frontend.forms" not in manifest_hashing or "hash: frontend.module_hash" not in manifest_hashing:
        fail("package manifest hashing does not retain atomic GenesisCode frontend facts")

    production_arms = extract_braced_blocks(core, "FmtEngine::Selfhost =>")
    production_arms += extract_braced_blocks(source_cli, "FmtEngine::Selfhost =>")
    if len(production_arms) < 5:
        fail("production frontend arm inventory is incomplete")
    for block in production_arms:
        for producer in ("hash_module", "parse_module", "canonicalize_module"):
            if re.search(rf"(?<![A-Za-z0-9_]){producer}\(", block):
                fail("production selfhost arm reaches a Rust frontend semantic producer")


def parse_json_output(raw: bytes, label: str) -> dict[str, Any]:
    try:
        value = json.loads(raw.decode("utf-8"), object_pairs_hook=no_duplicate_pairs)
    except (UnicodeDecodeError, json.JSONDecodeError) as error:
        fail(f"{label} emitted invalid UTF-8 JSON: {error}")
    if not isinstance(value, dict):
        fail(f"{label} JSON envelope must be an object")
    return value


def expected_position(source: str, byte_offset: int) -> tuple[int, int]:
    prefix = source.encode("utf-8")[:byte_offset]
    try:
        text = prefix.decode("utf-8")
    except UnicodeDecodeError:
        fail("diagnostic byte offset splits a UTF-8 scalar")
    line = text.count("\n") + 1
    column = len(text.rsplit("\n", 1)[-1]) + 1
    return line, column


def validate_artifact_identity(data: dict[str, Any], artifact: Path, artifact_hash: str) -> None:
    identity = require_closed(
        data.get("selfhost_artifact"), {"hash", "path", "source"}, "selfhost_artifact"
    )
    if identity["hash"] != artifact_hash or identity["source"] != "explicit":
        fail("production output does not retain the independently checked artifact identity")
    reported_path = Path(require_string(identity["path"], "selfhost_artifact.path"))
    if reported_path.resolve() != artifact.resolve():
        fail("production output reports the wrong selfhost artifact path")


def validate_valid_runtime_data(
    profile: dict[str, Any],
    case: dict[str, Any],
    data_value: Any,
    source_path: Path,
    artifact: Path,
    artifact_hash: str,
) -> dict[str, Any]:
    data = require_closed(data_value, PARSE_DATA_FIELDS, f"parse data for {case['id']}")
    source = case["source"]
    source_bytes = len(source.encode("utf-8"))
    expected = {
        "canonical": source.replace("\r\n", "\n")
        == case["canonicalSource"].replace("\r\n", "\n"),
        "canonical_source": case["canonicalSource"],
        "engine": "selfhost",
        "frontend_profile": profile["frontendProfile"],
        "module_hash_hex": case["moduleHashHex"],
        "source_bytes": source_bytes,
        "source_span": {"start_byte": 0, "end_byte": source_bytes},
        "span_unit": profile["spanUnit"],
    }
    for field, expected_value in expected.items():
        if data[field] != expected_value:
            fail(
                f"valid case {case['id']} field {field} mismatch: "
                f"{data[field]!r} != {expected_value!r}"
            )
    if Path(require_string(data["file"], "parse data file")).resolve() != source_path.resolve():
        fail(f"valid case {case['id']} reports the wrong source path")
    if module_hash(profile, data["canonical_source"]) != data["module_hash_hex"]:
        fail(f"valid case {case['id']} returned canonical bytes and hash disagree")
    validate_artifact_identity(data, artifact, artifact_hash)
    return {
        key: value
        for key, value in data.items()
        if key not in {"file", "selfhost_artifact"}
    } | {
        "artifact_hash": data["selfhost_artifact"]["hash"],
        "artifact_source": data["selfhost_artifact"]["source"],
    }


def validate_fmt_runtime_data(
    profile: dict[str, Any],
    case: dict[str, Any],
    data_value: Any,
    source_path: Path,
    artifact: Path,
    artifact_hash: str,
) -> dict[str, Any]:
    data = require_closed(data_value, FMT_DATA_FIELDS, f"fmt data for {case['id']}")
    source_bytes = len(case["source"].encode("utf-8"))
    changed = case["source"].replace("\r\n", "\n") != case["canonicalSource"].replace(
        "\r\n", "\n"
    )
    expected = {
        "changed": changed,
        "check": False,
        "engine": "selfhost",
        "frontend_profile": profile["frontendProfile"],
        "module_hash_hex": case["moduleHashHex"],
        "source_span": {"start_byte": 0, "end_byte": source_bytes},
        "span_unit": profile["spanUnit"],
    }
    for field, expected_value in expected.items():
        if data[field] != expected_value:
            fail(
                f"fmt case {case['id']} field {field} mismatch: "
                f"{data[field]!r} != {expected_value!r}"
            )
    if Path(require_string(data["file"], "fmt data file")).resolve() != source_path.resolve():
        fail(f"fmt case {case['id']} reports the wrong source path")
    try:
        formatted = source_path.read_bytes()
    except OSError as error:
        fail(f"fmt case {case['id']} did not leave readable output: {error}")
    if formatted != case["canonicalSource"].encode("utf-8"):
        fail(f"fmt case {case['id']} did not write the frozen canonical bytes")
    if module_hash(profile, formatted.decode("utf-8")) != data["module_hash_hex"]:
        fail(f"fmt case {case['id']} written canonical bytes and hash disagree")
    validate_artifact_identity(data, artifact, artifact_hash)
    return {
        key: value
        for key, value in data.items()
        if key not in {"file", "selfhost_artifact"}
    } | {
        "canonical_source": formatted.decode("utf-8"),
        "artifact_hash": data["selfhost_artifact"]["hash"],
        "artifact_source": data["selfhost_artifact"]["source"],
    }


def validate_malformed_runtime(
    case: dict[str, Any], envelope: dict[str, Any], source_path: Path, returncode: int
) -> dict[str, Any]:
    if returncode != case["exitCode"]:
        fail(f"malformed case {case['id']} exit code mismatch: {returncode}")
    if envelope.get("ok") is not False or envelope.get("kind") != "genesis/error-v0.2":
        fail(f"malformed case {case['id']} did not emit the error envelope")
    error = envelope.get("error")
    if not isinstance(error, dict) or error.get("code") != case["errorCode"]:
        fail(f"malformed case {case['id']} error code mismatch")
    context = error.get("context")
    if not isinstance(context, dict):
        fail(f"malformed case {case['id']} missing structured failure context")
    facts = context.get("facts")
    if not isinstance(facts, dict):
        fail(f"malformed case {case['id']} missing diagnostic facts")
    if facts.get("protocol_code") != case["protocolCode"]:
        fail(f"malformed case {case['id']} protocol code mismatch")
    if facts.get("byte_offset") != case["byteOffset"]:
        fail(f"malformed case {case['id']} byte offset mismatch")
    line, column = expected_position(case["source"], case["byteOffset"])
    expected_span = {
        "source": source_path.name,
        "startLine": line,
        "startColumn": column,
        "endLine": line,
        "endColumn": column,
    }
    if context.get("primary_span") != expected_span:
        fail(f"malformed case {case['id']} primary span mismatch")
    return {
        "error_code": error["code"],
        "protocol_code": facts["protocol_code"],
        "byte_offset": facts["byte_offset"],
        "primary_span": {
            key: value for key, value in expected_span.items() if key != "source"
        },
    }


def run_entrypoint(
    binary: Path,
    root: Path,
    artifact: Path,
    source_path: Path,
    command: str,
    timeout_seconds: int,
) -> subprocess.CompletedProcess[bytes]:
    if not binary.is_file() or not os.access(binary, os.X_OK):
        fail(f"production entrypoint is not executable: {binary}")
    command = [
        str(binary),
        "--json",
        "--selfhost-only",
        "--selfhost-artifact",
        str(artifact),
        command,
        str(source_path),
    ]
    try:
        return subprocess.run(
            command,
            cwd=root,
            stdout=subprocess.PIPE,
            stderr=subprocess.PIPE,
            timeout=timeout_seconds,
            check=False,
        )
    except (OSError, subprocess.TimeoutExpired) as error:
        fail(f"cannot execute {binary.name}: {error}")


def validate_runtime(
    profile: dict[str, Any],
    root: Path,
    genesis_bin: Path,
    genesis_wasi_bin: Path,
    timeout_seconds: int,
) -> int:
    artifact = (root / profile["artifact"]).resolve()
    try:
        artifact_hash = blake3_hex(artifact.read_bytes())
    except OSError as error:
        fail(f"cannot read production artifact {artifact}: {error}")
    binaries = {"genesis": genesis_bin.resolve(), "genesis_wasi": genesis_wasi_bin.resolve()}
    observations: dict[str, dict[str, dict[str, Any]]] = {name: {} for name in binaries}

    with tempfile.TemporaryDirectory(prefix="genesis-frontend-authority-") as temp:
        temp_root = Path(temp)
        for case in profile["validCases"]:
            for name, binary in binaries.items():
                parse_path = temp_root / f"{name}-{case['id']}-parse.gc"
                parse_path.write_bytes(case["source"].encode("utf-8"))
                completed = run_entrypoint(
                    binary, root, artifact, parse_path, "parse", timeout_seconds
                )
                envelope = parse_json_output(completed.stdout, f"{name}:{case['id']}")
                if completed.returncode != 0 or envelope.get("ok") is not True:
                    fail(
                        f"{name}:{case['id']} failed: rc={completed.returncode} "
                        f"stderr={completed.stderr.decode('utf-8', errors='replace')!r}"
                    )
                observations[name][f"parse:{case['id']}"] = validate_valid_runtime_data(
                    profile,
                    case,
                    envelope.get("data"),
                    parse_path,
                    artifact,
                    artifact_hash,
                )
                fmt_path = temp_root / f"{name}-{case['id']}-fmt.gc"
                fmt_path.write_bytes(case["source"].encode("utf-8"))
                completed = run_entrypoint(
                    binary, root, artifact, fmt_path, "fmt", timeout_seconds
                )
                envelope = parse_json_output(completed.stdout, f"{name}:fmt:{case['id']}")
                if completed.returncode != 0 or envelope.get("ok") is not True:
                    fail(
                        f"{name}:fmt:{case['id']} failed: rc={completed.returncode} "
                        f"stderr={completed.stderr.decode('utf-8', errors='replace')!r}"
                    )
                observations[name][f"fmt:{case['id']}"] = validate_fmt_runtime_data(
                    profile,
                    case,
                    envelope.get("data"),
                    fmt_path,
                    artifact,
                    artifact_hash,
                )

        for case in profile["malformedCases"]:
            for name, binary in binaries.items():
                for command in ("parse", "fmt"):
                    source_path = temp_root / f"{name}-{case['id']}-{command}.gc"
                    source_path.write_bytes(case["source"].encode("utf-8"))
                    completed = run_entrypoint(
                        binary, root, artifact, source_path, command, timeout_seconds
                    )
                    envelope = parse_json_output(
                        completed.stdout, f"{name}:{command}:{case['id']}"
                    )
                    observations[name][f"{command}:{case['id']}"] = (
                        validate_malformed_runtime(
                            case, envelope, source_path, completed.returncode
                        )
                    )

    if observations["genesis"] != observations["genesis_wasi"]:
        fail("native and WASI production frontend facts disagree")
    return sum(len(cases) for cases in observations.values())


def expect_failure(label: str, action: Callable[[], None]) -> None:
    try:
        action()
    except ContractError:
        return
    fail(f"negative control unexpectedly passed: {label}")


def run_self_tests(profile: dict[str, Any], schema: dict[str, Any], sources: dict[str, str]) -> int:
    controls = 0

    def profile_control(label: str, mutate: Callable[[dict[str, Any]], None]) -> None:
        nonlocal controls
        candidate = copy.deepcopy(profile)
        mutate(candidate)
        candidate["contentIdentitySha256"] = canonical_identity(candidate)
        expect_failure(label, lambda: validate_profile(candidate, schema))
        controls += 1

    profile_control("unknown-profile-field", lambda value: value.__setitem__("unknown", True))
    profile_control(
        "canonical-hash-tamper",
        lambda value: value["validCases"][0].__setitem__("moduleHashHex", "0" * 64),
    )
    profile_control(
        "malformed-offset-out-of-range",
        lambda value: value["malformedCases"][0].__setitem__("byteOffset", 10_000),
    )

    missing_binding = copy.deepcopy(sources)
    missing_binding["selfhost/toolchain_manifest.gc"] = missing_binding[
        "selfhost/toolchain_manifest.gc"
    ].replace(profile["binding"], "", 1)
    expect_failure("missing-production-binding", lambda: validate_source_contract(profile, missing_binding))
    controls += 1

    restored_fallback = copy.deepcopy(sources)
    restored_fallback["crates/gc_cli_driver/src/selfhost_bridge.rs"] += (
        '\nfn restored_fallback(env: &Env) { let _ = env.get("selfhost/parse::parse-module"); }\n'
    )
    expect_failure("restored-rust-fallback", lambda: validate_source_contract(profile, restored_fallback))
    controls += 1

    case = profile["validCases"][0]
    source_path = Path(f"/tmp/{case['id']}.gc")
    artifact = Path("/tmp/toolchain.gc")
    artifact_hash = "1" * 64
    valid_data = {
        "canonical": False,
        "canonical_source": case["canonicalSource"],
        "engine": "selfhost",
        "file": str(source_path),
        "frontend_profile": profile["frontendProfile"],
        "module_hash_hex": case["moduleHashHex"],
        "selfhost_artifact": {
            "hash": artifact_hash,
            "path": str(artifact),
            "source": "explicit",
        },
        "source_bytes": len(case["source"].encode("utf-8")),
        "source_span": {"start_byte": 0, "end_byte": len(case["source"].encode("utf-8"))},
        "span_unit": profile["spanUnit"],
    }
    for label, field, replacement in (
        ("returned-canonical-tamper", "canonical_source", case["canonicalSource"] + "\n"),
        ("returned-hash-tamper", "module_hash_hex", "2" * 64),
        ("returned-span-tamper", "source_span", {"start_byte": 0, "end_byte": 1}),
        ("returned-profile-tamper", "frontend_profile", "genesis/unknown"),
    ):
        candidate = copy.deepcopy(valid_data)
        candidate[field] = replacement
        expect_failure(
            label,
            lambda candidate=candidate: validate_valid_runtime_data(
                profile, case, candidate, source_path, artifact, artifact_hash
            ),
        )
        controls += 1
    return controls


def refresh_identity(path: Path, profile: dict[str, Any], schema: dict[str, Any]) -> None:
    validate_profile(
        profile,
        schema,
        check_identity=False,
        allow_identity_placeholder=True,
    )
    profile["contentIdentitySha256"] = canonical_identity(profile)
    validate_profile(profile, schema)
    path.write_text(
        json.dumps(profile, indent=2, sort_keys=True, ensure_ascii=True) + "\n",
        encoding="utf-8",
    )


def main() -> int:
    parser = argparse.ArgumentParser()
    default_root = Path(__file__).resolve().parents[2]
    parser.add_argument("--root", type=Path, default=default_root)
    parser.add_argument(
        "--profile", type=Path, default=Path("policies/selfhost_frontend_authority_v0.1.json")
    )
    parser.add_argument(
        "--schema",
        type=Path,
        default=Path("docs/spec/SELFHOST_FRONTEND_AUTHORITY_v0.1.schema.json"),
    )
    parser.add_argument("--refresh-identity", action="store_true")
    parser.add_argument("--self-test", action="store_true")
    parser.add_argument("--runtime", action="store_true")
    parser.add_argument("--genesis-bin", type=Path)
    parser.add_argument("--genesis-wasi-bin", type=Path)
    parser.add_argument("--timeout-seconds", type=int, default=60)
    args = parser.parse_args()

    root = args.root.resolve()
    profile_path = args.profile if args.profile.is_absolute() else root / args.profile
    schema_path = args.schema if args.schema.is_absolute() else root / args.schema
    profile = load_json(profile_path)
    schema = load_json(schema_path)
    if args.refresh_identity:
        refresh_identity(profile_path, profile, schema)
        print(f"selfhost-frontend-authority: refreshed {profile_path.relative_to(root)}")
        return 0

    checked = validate_profile(profile, schema)
    sources = read_source_contract(root)
    validate_source_contract(checked, sources)
    controls = run_self_tests(checked, schema, sources) if args.self_test else 0
    observations = 0
    if args.runtime:
        if args.genesis_bin is None or args.genesis_wasi_bin is None:
            fail("--runtime requires --genesis-bin and --genesis-wasi-bin")
        if args.timeout_seconds <= 0:
            fail("--timeout-seconds must be positive")
        observations = validate_runtime(
            checked,
            root,
            args.genesis_bin,
            args.genesis_wasi_bin,
            args.timeout_seconds,
        )
    print(
        "selfhost-frontend-authority: ok "
        f"valid={len(checked['validCases'])} malformed={len(checked['malformedCases'])} "
        f"controls={controls} runtime-observations={observations}"
    )
    return 0


if __name__ == "__main__":
    try:
        raise SystemExit(main())
    except ContractError as error:
        print(f"selfhost-frontend-authority: {error}", file=os.sys.stderr)
        raise SystemExit(1)
