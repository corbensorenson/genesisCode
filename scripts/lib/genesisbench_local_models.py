#!/usr/bin/env python3
"""Capture and verify score-blind GenesisBench local-model selection evidence."""

from __future__ import annotations

import argparse
import copy
import hashlib
import json
import os
import re
import stat
import sys
import tempfile
from pathlib import Path, PurePosixPath
from typing import Any


ROOT = Path(__file__).resolve().parents[2]
PLAN_PATH = ROOT / "benchmarks/genesisbench/v0.1/local-models/preselection.json"
INVENTORY_PATH = ROOT / "benchmarks/genesisbench/v0.1/local-models/inventory.json"
SCHEMA_PATH = ROOT / "docs/spec/GENESISBENCH_LOCAL_MODELS_v0.1.schema.json"
PLAN_KIND = "genesis/genesisbench-local-model-preselection-v0.1"
INVENTORY_KIND = "genesis/genesisbench-local-model-inventory-v0.1"
SHA_RE = re.compile(r"^[0-9a-f]{64}$")
ID_RE = re.compile(r"^[A-Za-z0-9][A-Za-z0-9._/@+-]{0,191}$")
HOST_PATH_RE = re.compile(r"(?:/Users/|/home/|[A-Za-z]:\\\\Users\\\\)")
ROLES = {"smallest-adapter-viable", "strongest-host-fit"}
STATUSES = {"selected", "not-selected"}


class LocalModelError(ValueError):
    pass


def require(condition: bool, message: str) -> None:
    if not condition:
        raise LocalModelError(message)


def reject_pairs(rows: list[tuple[str, Any]]) -> dict[str, Any]:
    result: dict[str, Any] = {}
    for key, value in rows:
        require(key not in result, f"duplicate JSON key: {key}")
        result[key] = value
    return result


def load_json(path: Path) -> Any:
    try:
        return json.loads(path.read_text(encoding="utf-8"), object_pairs_hook=reject_pairs)
    except (OSError, UnicodeDecodeError, json.JSONDecodeError) as exc:
        raise LocalModelError(f"cannot load JSON {path}: {exc}") from exc


def canonical_bytes(value: Any) -> bytes:
    return (json.dumps(value, sort_keys=True, separators=(",", ":"), ensure_ascii=True) + "\n").encode("ascii")


def sha256_bytes(value: bytes) -> str:
    return hashlib.sha256(value).hexdigest()


def sha256_file(path: Path) -> str:
    digest = hashlib.sha256()
    with path.open("rb") as stream:
        while chunk := stream.read(1024 * 1024):
            digest.update(chunk)
    return digest.hexdigest()


def object_identity(value: dict[str, Any], field: str = "contentIdentitySha256") -> str:
    material = copy.deepcopy(value)
    material.pop(field, None)
    return sha256_bytes(canonical_bytes(material))


def identified(value: dict[str, Any], field: str = "contentIdentitySha256") -> dict[str, Any]:
    result = copy.deepcopy(value)
    result[field] = object_identity(result, field)
    return result


def closed(value: Any, fields: set[str], label: str) -> dict[str, Any]:
    require(isinstance(value, dict) and set(value) == fields, f"{label} fields are not closed")
    return value


def safe_id(value: Any, label: str) -> str:
    require(isinstance(value, str) and ID_RE.fullmatch(value) is not None, f"invalid {label}")
    return value


def safe_relative(value: Any, label: str) -> PurePosixPath:
    require(isinstance(value, str) and value and len(value) <= 512, f"invalid {label}")
    require("\\" not in value and not value.startswith("/") and not HOST_PATH_RE.search(value), f"unsafe {label}")
    path = PurePosixPath(value)
    require(all(part not in {"", ".", ".."} for part in path.parts), f"unsafe {label}")
    return path


def regular_payload(path: Path, label: str) -> Path:
    try:
        resolved = path.resolve(strict=True)
    except (OSError, RuntimeError) as exc:
        raise LocalModelError(f"cannot resolve {label}") from exc
    require(resolved.is_file(), f"{label} is not a regular file")
    require(stat.S_ISREG(resolved.stat().st_mode), f"{label} is not regular")
    return resolved


def validate_identity(value: dict[str, Any], field: str = "contentIdentitySha256") -> None:
    require(SHA_RE.fullmatch(value.get(field, "")) is not None, f"invalid {field}")
    require(value[field] == object_identity(value, field), f"{field} drift")


def validate_schema() -> None:
    schema = load_json(SCHEMA_PATH)
    require(schema.get("$schema") == "https://json-schema.org/draft/2020-12/schema", "local model schema draft drift")
    require(schema.get("$id") == "https://genesiscode.dev/schemas/genesisbench-local-models-v0.1.json", "local model schema id drift")

    def walk(value: Any, label: str) -> None:
        if isinstance(value, dict):
            if value.get("type") == "object":
                require(value.get("additionalProperties") is False, f"open schema object: {label}")
                require(set(value.get("required", [])) == set(value.get("properties", {})), f"partially optional schema object: {label}")
            for key, child in value.items():
                walk(child, f"{label}/{key}")
        elif isinstance(value, list):
            for index, child in enumerate(value):
                walk(child, f"{label}/{index}")

    walk(schema, "schema")


def validate_evidence(row: Any, label: str) -> dict[str, Any]:
    row = closed(row, {"bytes", "path", "role", "sha256", "sourceRepository", "sourceRevision", "sourceUrl"}, label)
    relative = safe_relative(row["path"], f"{label} path")
    safe_id(row["role"], f"{label} role")
    safe_id(row["sourceRepository"], f"{label} repository")
    safe_id(row["sourceRevision"], f"{label} revision")
    require(isinstance(row["sourceUrl"], str) and row["sourceUrl"].startswith("https://huggingface.co/"), f"invalid {label} URL")
    require(isinstance(row["bytes"], int) and 0 < row["bytes"] <= 2 * 1024 * 1024, f"invalid {label} bytes")
    require(SHA_RE.fullmatch(row["sha256"] or "") is not None, f"invalid {label} digest")
    path = ROOT.joinpath(*relative.parts)
    require(path.is_file() and not path.is_symlink(), f"missing retained {label}")
    require(path.stat().st_size == row["bytes"] and sha256_file(path) == row["sha256"], f"retained {label} drift")
    return row


def validate_plan(document: Any) -> dict[str, Any]:
    doc = closed(document, {
        "candidates", "contentIdentitySha256", "host", "kind", "policy", "purpose", "version",
    }, "preselection")
    require(doc["kind"] == PLAN_KIND and doc["version"] == "0.1.0", "preselection kind/version drift")
    require(isinstance(doc["purpose"], str) and doc["purpose"], "preselection purpose is empty")
    require(not HOST_PATH_RE.search(canonical_bytes(doc).decode("ascii")), "host path leaked into preselection")
    policy = closed(doc["policy"], {
        "adapter", "qualityScoresObservedBeforeSelection", "requiredRoles", "serverFallbackAllowed",
        "trustRemoteCodeAllowed", "weightsDownloadedOrMutatedDuringSelection",
    }, "preselection policy")
    require(policy == {
        "adapter": "offline-mlx-responses-v0.1",
        "qualityScoresObservedBeforeSelection": False,
        "requiredRoles": sorted(ROLES),
        "serverFallbackAllowed": False,
        "trustRemoteCodeAllowed": False,
        "weightsDownloadedOrMutatedDuringSelection": False,
    }, "preselection policy drift")
    host = closed(doc["host"], {"architecture", "hardwareClass", "memoryBytes", "operatingSystem"}, "host")
    for field in ("architecture", "hardwareClass", "operatingSystem"):
        safe_id(host[field], f"host {field}")
    require(isinstance(host["memoryBytes"], int) and host["memoryBytes"] > 0, "invalid host memory")
    require(isinstance(doc["candidates"], list) and len(doc["candidates"]) >= 2, "candidate inventory is too small")
    ids: list[str] = []
    roles: list[str] = []
    for index, raw in enumerate(doc["candidates"]):
        candidate = closed(raw, {
            "adapterCompatible", "evidence", "format", "id", "license", "parameterClass", "repository",
            "revision", "selection", "trustRemoteCode",
        }, f"candidate[{index}]")
        ids.append(safe_id(candidate["id"], "candidate id"))
        safe_id(candidate["repository"], "candidate repository")
        safe_id(candidate["revision"], "candidate revision")
        safe_id(candidate["format"], "candidate format")
        safe_id(candidate["parameterClass"], "candidate parameter class")
        require(type(candidate["adapterCompatible"]) is bool and type(candidate["trustRemoteCode"]) is bool, "invalid candidate booleans")
        license_record = closed(candidate["license"], {"benchmarkUseCompatible", "id"}, "candidate license")
        safe_id(license_record["id"], "license id")
        require(type(license_record["benchmarkUseCompatible"]) is bool, "invalid license compatibility")
        evidence = candidate["evidence"]
        require(isinstance(evidence, list) and len(evidence) == 2, "candidate must retain card and license evidence")
        for evidence_index, row in enumerate(evidence):
            validate_evidence(row, f"candidate[{index}].evidence[{evidence_index}]")
        require([row["role"] for row in evidence] == ["base-license", "quantized-model-card"], "candidate evidence role/order drift")
        selection = closed(candidate["selection"], {"reasonCodes", "role", "status"}, "candidate selection")
        require(selection["status"] in STATUSES, "invalid selection status")
        require(isinstance(selection["reasonCodes"], list) and selection["reasonCodes"] == sorted(set(selection["reasonCodes"])) and selection["reasonCodes"], "invalid selection reasons")
        for reason in selection["reasonCodes"]:
            safe_id(reason, "selection reason")
        if selection["status"] == "selected":
            require(selection["role"] in ROLES, "selected candidate has invalid role")
            require(candidate["adapterCompatible"] and not candidate["trustRemoteCode"] and license_record["benchmarkUseCompatible"], "ineligible candidate selected")
            roles.append(selection["role"])
        else:
            require(selection["role"] is None, "unselected candidate has a role")
    require(ids == sorted(set(ids)), "candidate ids must be sorted and unique")
    require(sorted(roles) == sorted(ROLES) and len(roles) == len(ROLES), "selected role closure drift")
    validate_identity(doc)
    return doc


def artifact_rows(root: Path) -> list[dict[str, Any]]:
    require(root.is_dir() and not root.is_symlink(), "model snapshot root is unavailable")
    rows: list[dict[str, Any]] = []
    observed: list[str] = []
    for path in sorted(root.iterdir(), key=lambda item: item.name):
        require(path.name not in {".", ".."} and "/" not in path.name and "\\" not in path.name, "unsafe model file name")
        require(not path.is_dir(), "nested model snapshot topology is unsupported")
        payload = regular_payload(path, f"model file {path.name}")
        observed.append(path.name)
        rows.append({"bytes": payload.stat().st_size, "path": path.name, "sha256": sha256_file(payload)})
    require(observed == sorted(set(observed)) and rows, "model snapshot is empty or duplicated")
    return rows


def parse_model_roots(values: list[str], expected: set[str]) -> dict[str, Path]:
    result: dict[str, Path] = {}
    for value in values:
        identifier, separator, raw_path = value.partition("=")
        require(separator == "=" and identifier in expected and identifier not in result and raw_path, "invalid --model-root binding")
        result[identifier] = Path(raw_path).expanduser().resolve()
    require(set(result) == expected, "model-root bindings are incomplete")
    return result


def capture_inventory(plan: dict[str, Any], roots: dict[str, Path]) -> dict[str, Any]:
    candidates = []
    for candidate in plan["candidates"]:
        rows = artifact_rows(roots[candidate["id"]])
        candidates.append({
            "artifactIdentitySha256": sha256_bytes(canonical_bytes(rows)),
            "bytes": sum(row["bytes"] for row in rows),
            "fileCount": len(rows),
            "files": rows,
            "id": candidate["id"],
            "revision": candidate["revision"],
        })
    return identified({
        "candidates": candidates,
        "contentIdentitySha256": "",
        "kind": INVENTORY_KIND,
        "preselectionIdentitySha256": plan["contentIdentitySha256"],
        "version": "0.1.0",
    })


def validate_inventory(document: Any, plan: dict[str, Any]) -> dict[str, Any]:
    doc = closed(document, {"candidates", "contentIdentitySha256", "kind", "preselectionIdentitySha256", "version"}, "inventory")
    require(doc["kind"] == INVENTORY_KIND and doc["version"] == "0.1.0", "inventory kind/version drift")
    require(doc["preselectionIdentitySha256"] == plan["contentIdentitySha256"], "inventory preselection binding drift")
    expected = [(candidate["id"], candidate["revision"]) for candidate in plan["candidates"]]
    observed: list[tuple[str, str]] = []
    require(isinstance(doc["candidates"], list), "inventory candidates must be an array")
    for index, raw in enumerate(doc["candidates"]):
        candidate = closed(raw, {"artifactIdentitySha256", "bytes", "fileCount", "files", "id", "revision"}, f"inventory candidate[{index}]")
        observed.append((safe_id(candidate["id"], "inventory candidate id"), safe_id(candidate["revision"], "inventory revision")))
        require(isinstance(candidate["files"], list) and candidate["files"], "inventory files are empty")
        paths: list[str] = []
        total = 0
        for file_index, row in enumerate(candidate["files"]):
            row = closed(row, {"bytes", "path", "sha256"}, f"inventory file[{file_index}]")
            path = safe_relative(row["path"], "inventory file path").as_posix()
            require("/" not in path, "inventory model file must be top-level")
            require(isinstance(row["bytes"], int) and row["bytes"] >= 0, "invalid inventory byte count")
            require(SHA_RE.fullmatch(row["sha256"] or "") is not None, "invalid inventory file digest")
            paths.append(path); total += row["bytes"]
        require(paths == sorted(set(paths)), "inventory paths must be sorted and unique")
        require(candidate["fileCount"] == len(candidate["files"]) and candidate["bytes"] == total, "inventory aggregate drift")
        require(candidate["artifactIdentitySha256"] == sha256_bytes(canonical_bytes(candidate["files"])), "artifact identity drift")
    require(observed == expected, "inventory candidate order/binding drift")
    validate_identity(doc)
    return doc


def verify_local(inventory: dict[str, Any], roots: dict[str, Path]) -> None:
    for candidate in inventory["candidates"]:
        require(artifact_rows(roots[candidate["id"]]) == candidate["files"], f"local model artifact drift: {candidate['id']}")


def refresh_inventory_identities(inventory: dict[str, Any]) -> None:
    for candidate in inventory["candidates"]:
        candidate["fileCount"] = len(candidate["files"])
        candidate["bytes"] = sum(row["bytes"] for row in candidate["files"])
        candidate["artifactIdentitySha256"] = sha256_bytes(canonical_bytes(candidate["files"]))
    inventory["contentIdentitySha256"] = object_identity(inventory)


def self_test(plan: dict[str, Any], inventory: dict[str, Any]) -> int:
    controls = 0
    def plan_semantic(mutate: Any) -> Any:
        def apply(document: dict[str, Any]) -> None:
            mutate(document)
            document["contentIdentitySha256"] = object_identity(document)
        return apply

    def inventory_semantic(mutate: Any) -> Any:
        def apply(document: dict[str, Any]) -> None:
            mutate(document)
            refresh_inventory_identities(document)
        return apply

    for label, source, mutate, validator in (
        ("plan-identity", plan, lambda d: d.__setitem__("contentIdentitySha256", "0" * 64), validate_plan),
        ("selection-role", plan, plan_semantic(lambda d: d["candidates"][0]["selection"].__setitem__("role", "strongest-host-fit")), validate_plan),
        ("license", plan, plan_semantic(lambda d: d["candidates"][-1]["license"].__setitem__("benchmarkUseCompatible", False)), validate_plan),
        ("trust", plan, plan_semantic(lambda d: d["candidates"][-1].__setitem__("trustRemoteCode", True)), validate_plan),
        ("inventory-identity", inventory, lambda d: d.__setitem__("contentIdentitySha256", "0" * 64), lambda d: validate_inventory(d, plan)),
        ("file-digest-format", inventory, inventory_semantic(lambda d: d["candidates"][0]["files"][0].__setitem__("sha256", "invalid")), lambda d: validate_inventory(d, plan)),
        ("file-extra", inventory, inventory_semantic(lambda d: d["candidates"][0]["files"].append(copy.deepcopy(d["candidates"][0]["files"][0]))), lambda d: validate_inventory(d, plan)),
        ("candidate-omission", inventory, inventory_semantic(lambda d: d["candidates"].pop()), lambda d: validate_inventory(d, plan)),
    ):
        candidate = copy.deepcopy(source); mutate(candidate)
        try:
            validator(candidate)
        except LocalModelError:
            controls += 1
        else:
            raise LocalModelError(f"negative control accepted: {label}")
    with tempfile.TemporaryDirectory(prefix="genesisbench-local-model-") as raw:
        root = Path(raw); (root / "config.json").write_bytes(b"{}\n"); (root / "weights.bin").write_bytes(b"weights")
        rows = artifact_rows(root)
        (root / "weights.bin").write_bytes(b"tamper")
        require(artifact_rows(root) != rows, "local byte tamper was not observed"); controls += 1
        (root / "nested").mkdir()
        try:
            artifact_rows(root)
        except LocalModelError:
            controls += 1
        else:
            raise LocalModelError("nested model topology accepted")
    return controls


def main() -> int:
    parser = argparse.ArgumentParser(description=__doc__)
    mode = parser.add_mutually_exclusive_group(required=True)
    mode.add_argument("--check", action="store_true")
    mode.add_argument("--capture", action="store_true")
    mode.add_argument("--verify-local", action="store_true")
    parser.add_argument("--plan", type=Path, default=PLAN_PATH)
    parser.add_argument("--inventory", type=Path, default=INVENTORY_PATH)
    parser.add_argument("--model-root", action="append", default=[])
    parser.add_argument("--self-test", action="store_true")
    args = parser.parse_args()
    validate_schema()
    plan = validate_plan(load_json(args.plan))
    expected_ids = {candidate["id"] for candidate in plan["candidates"]}
    if args.capture:
        require(not args.inventory.exists(), "inventory output already exists")
        inventory = capture_inventory(plan, parse_model_roots(args.model_root, expected_ids))
        args.inventory.parent.mkdir(parents=True, exist_ok=True)
        args.inventory.write_text(json.dumps(inventory, indent=2, sort_keys=True) + "\n", encoding="ascii")
        print(f"genesisbench-local-models: captured {inventory['contentIdentitySha256']}")
        return 0
    inventory = validate_inventory(load_json(args.inventory), plan)
    if args.verify_local:
        verify_local(inventory, parse_model_roots(args.model_root, expected_ids))
    else:
        require(not args.model_root, "model roots require --capture or --verify-local")
    controls = self_test(plan, inventory) if args.self_test else 0
    print(
        "genesisbench-local-models: ok "
        f"candidates={len(inventory['candidates'])} controls={controls} identity={inventory['contentIdentitySha256']}"
    )
    return 0


if __name__ == "__main__":
    try:
        raise SystemExit(main())
    except (LocalModelError, OSError, UnicodeError) as exc:
        print(f"genesisbench-local-models: {exc}", file=sys.stderr)
        raise SystemExit(1)
