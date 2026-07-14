#!/usr/bin/env python3
"""Validate GenesisCode's immutable v1 compatibility reservations."""

from __future__ import annotations

import argparse
from copy import deepcopy
from hashlib import sha256
import json
from pathlib import Path, PurePosixPath
import re
import sys
from typing import Any, Dict, Iterable, Mapping, Optional, Sequence, Tuple


REGISTRY_REL = "genesis.compatibility.json"
SCHEMA_REL = "docs/spec/V1_COMPATIBILITY_REGISTRY_v0.1.schema.json"
VERSION_SPEC_REL = "docs/spec/VERSION_SURFACES_v0.1.md"

EXPECTED_STABLE_IDS = {
    "language-profile": "genesis/compat/v1/language-profile",
    "coreform": "genesis/compat/v1/coreform",
    "value-effect-hash": "genesis/compat/v1/value-effect-hash",
    "effect-log": "genesis/compat/v1/effect-log",
    "evidence": "genesis/compat/v1/evidence",
    "package": "genesis/compat/v1/package",
    "patch": "genesis/compat/v1/patch",
    "bytecode": "genesis/compat/v1/bytecode",
    "snapshot": "genesis/compat/v1/snapshot",
    "bootstrap": "genesis/compat/v1/bootstrap",
}
EXPECTED_CANDIDATES = {
    "language-profile": "genesis/language-profile/v0.2",
    "coreform": "genesis/coreform/v0.2",
    "value-effect-hash": "genesis/value-effect-hash/v0.2",
    "effect-log": "genesis/effect-log/v3",
    "evidence": "genesis/evidence-profile/v0.1",
    "package": "genesis/package-profile/v0.2",
    "patch": "genesis/patch-profile/v0.2",
    "bytecode": None,
    "snapshot": "genesis/vcs-snapshot/v1",
    "bootstrap": "genesis/bootstrap-profile/v0.2",
}
EXPECTED_DEPENDENCIES = {
    "language-profile": [],
    "coreform": ["language-profile"],
    "value-effect-hash": ["coreform"],
    "effect-log": ["coreform", "value-effect-hash"],
    "evidence": ["language-profile"],
    "package": ["coreform", "evidence", "patch", "snapshot"],
    "patch": ["coreform", "snapshot"],
    "bytecode": ["language-profile", "coreform", "value-effect-hash", "effect-log"],
    "snapshot": ["coreform"],
    "bootstrap": ["language-profile", "coreform", "value-effect-hash"],
}
EXPECTED_PROMOTIONS = {
    "P-NORMATIVE-SPEC",
    "P-GOLDEN-CORPUS",
    "P-INDEPENDENT-VERIFIER",
    "P-MIGRATION",
    "P-TIER-PARITY",
    "P-SECURITY",
    "P-R9-FREEZE",
}

TOP_FIELDS = {
    "kind", "version", "namespace", "releaseClaim", "stabilizationTask",
    "policy", "promotionRequirements", "entries",
}
POLICY_FIELDS = {
    "candidateChange", "dependencyRule", "reservation", "retirementRule", "stabilityRule",
}
PROMOTION_FIELDS = {"id", "description"}
ENTRY_FIELDS = {
    "key", "stableId", "state", "candidateId", "compatibilityClass", "dependencies",
    "authorities", "components", "promotionRequirements",
}
AUTHORITY_FIELDS = {"path", "contains"}
COMPONENT_FIELDS = {
    "id", "currentWriter", "acceptedReaders", "missingDiscriminator", "migrationRecords",
}
ID_RE = re.compile(r"^[a-z0-9][a-z0-9./-]*$")
RECORD_RE = re.compile(r"^M-[A-Z0-9-]+$")


class CompatibilityError(ValueError):
    pass


def reject_duplicate_keys(pairs: Sequence[Tuple[str, Any]]) -> Dict[str, Any]:
    result: Dict[str, Any] = {}
    for key, value in pairs:
        if key in result:
            raise CompatibilityError(f"duplicate JSON key: {key}")
        result[key] = value
    return result


def load_json(path: Path) -> Any:
    try:
        return json.loads(
            path.read_text(encoding="utf-8"), object_pairs_hook=reject_duplicate_keys
        )
    except FileNotFoundError as exc:
        raise CompatibilityError(f"missing file: {path}") from exc
    except json.JSONDecodeError as exc:
        raise CompatibilityError(
            f"invalid JSON in {path}:{exc.lineno}:{exc.colno}: {exc.msg}"
        ) from exc


def require_type(value: Any, expected: type, context: str) -> None:
    if not isinstance(value, expected):
        raise CompatibilityError(f"{context} must be {expected.__name__}")


def require_closed(value: Mapping[str, Any], fields: set[str], context: str) -> None:
    actual = set(value)
    if actual != fields:
        missing = sorted(fields - actual)
        unknown = sorted(actual - fields)
        raise CompatibilityError(
            f"{context} field mismatch: missing={missing} unknown={unknown}"
        )


def require_nonempty_string(value: Any, context: str) -> str:
    if not isinstance(value, str) or not value.strip():
        raise CompatibilityError(f"{context} must be a non-empty string")
    return value


def require_unique(values: Iterable[str], context: str) -> list[str]:
    out = list(values)
    if len(out) != len(set(out)):
        raise CompatibilityError(f"{context} contains duplicates")
    return out


def validate_relative_path(raw: Any, context: str) -> str:
    path = require_nonempty_string(raw, context)
    pure = PurePosixPath(path)
    if pure.is_absolute() or ".." in pure.parts or "." in pure.parts or "\\" in path:
        raise CompatibilityError(f"{context} must be a normalized repository-relative path")
    if path.startswith(("/Users/", "/home/", "/private/", "/tmp/")):
        raise CompatibilityError(f"{context} leaks a host path")
    return path


def validate_schema_marker(root: Path) -> None:
    schema = load_json(root / SCHEMA_REL)
    require_type(schema, dict, "schema")
    if schema.get("$id") != "https://genesiscode.dev/schemas/v1-compatibility-registry-v0.1.json":
        raise CompatibilityError("compatibility schema $id drift")
    if schema.get("additionalProperties") is not False:
        raise CompatibilityError("compatibility schema root must be closed")
    if schema.get("properties", {}).get("namespace", {}).get("const") != "genesis/compat/v1":
        raise CompatibilityError("compatibility schema namespace drift")
    entries = schema.get("properties", {}).get("entries", {})
    if entries.get("minItems") != 10 or entries.get("maxItems") != 10:
        raise CompatibilityError("compatibility schema must require exactly ten entries")


def validate_authority(authority: Any, root: Path, context: str, check_sources: bool) -> None:
    require_type(authority, dict, context)
    require_closed(authority, AUTHORITY_FIELDS, context)
    rel = validate_relative_path(authority["path"], f"{context}.path")
    needle = require_nonempty_string(authority["contains"], f"{context}.contains")
    if check_sources:
        path = root / rel
        try:
            source = path.read_text(encoding="utf-8")
        except FileNotFoundError as exc:
            raise CompatibilityError(f"{context} source is missing: {rel}") from exc
        if needle not in source:
            raise CompatibilityError(f"{context} source-current drift: {needle!r} absent from {rel}")


def validate_component(component: Any, context: str, migration_records: set[str]) -> None:
    require_type(component, dict, context)
    require_closed(component, COMPONENT_FIELDS, context)
    require_nonempty_string(component["id"], f"{context}.id")
    writer = require_nonempty_string(component["currentWriter"], f"{context}.currentWriter")
    readers = component["acceptedReaders"]
    require_type(readers, list, f"{context}.acceptedReaders")
    readers = require_unique(
        [require_nonempty_string(v, f"{context}.acceptedReaders") for v in readers],
        f"{context}.acceptedReaders",
    )
    if not readers or writer not in readers:
        raise CompatibilityError(f"{context} current writer must be an accepted reader")
    require_nonempty_string(component["missingDiscriminator"], f"{context}.missingDiscriminator")
    records = component["migrationRecords"]
    require_type(records, list, f"{context}.migrationRecords")
    records = require_unique(
        [require_nonempty_string(v, f"{context}.migrationRecords") for v in records],
        f"{context}.migrationRecords",
    )
    unknown = sorted(set(records) - migration_records)
    if unknown:
        raise CompatibilityError(f"{context} references undocumented migrations: {unknown}")
    for record in records:
        if not RECORD_RE.fullmatch(record):
            raise CompatibilityError(f"{context} has malformed migration record {record}")
    if len(readers) > 1 and not records:
        raise CompatibilityError(f"{context} accepts multiple identities without migration records")


def validate_dag(entries: Mapping[str, Mapping[str, Any]]) -> None:
    state: Dict[str, int] = {}

    def visit(key: str, chain: list[str]) -> None:
        mark = state.get(key, 0)
        if mark == 1:
            raise CompatibilityError(f"dependency cycle: {' -> '.join(chain + [key])}")
        if mark == 2:
            return
        state[key] = 1
        for dependency in entries[key]["dependencies"]:
            if dependency not in entries:
                raise CompatibilityError(f"{key} has unknown dependency {dependency}")
            if dependency == key:
                raise CompatibilityError(f"{key} depends on itself")
            visit(dependency, chain + [key])
        state[key] = 2

    for key in entries:
        visit(key, [])


def validate(data: Any, root: Path, *, check_sources: bool = True) -> None:
    require_type(data, dict, "registry")
    require_closed(data, TOP_FIELDS, "registry")
    expected_scalars = {
        "kind": "genesis/v1-compatibility-registry-v0.1",
        "version": "0.1",
        "namespace": "genesis/compat/v1",
        "releaseClaim": "reserved-not-stable",
        "stabilizationTask": "R9.1.a",
    }
    for field, expected in expected_scalars.items():
        if data[field] != expected:
            raise CompatibilityError(f"registry.{field} must be {expected!r}")

    require_type(data["policy"], dict, "registry.policy")
    require_closed(data["policy"], POLICY_FIELDS, "registry.policy")
    for field in POLICY_FIELDS:
        require_nonempty_string(data["policy"][field], f"registry.policy.{field}")

    promotions = data["promotionRequirements"]
    require_type(promotions, list, "registry.promotionRequirements")
    promotion_ids = []
    for index, requirement in enumerate(promotions):
        context = f"registry.promotionRequirements[{index}]"
        require_type(requirement, dict, context)
        require_closed(requirement, PROMOTION_FIELDS, context)
        promotion_ids.append(require_nonempty_string(requirement["id"], f"{context}.id"))
        require_nonempty_string(requirement["description"], f"{context}.description")
    require_unique(promotion_ids, "registry promotion requirement IDs")
    if set(promotion_ids) != EXPECTED_PROMOTIONS:
        raise CompatibilityError("registry promotion requirement set drift")

    version_spec = (root / VERSION_SPEC_REL).read_text(encoding="utf-8")
    migration_records = set(re.findall(r"^### (M-[A-Z0-9-]+)$", version_spec, re.MULTILINE))

    raw_entries = data["entries"]
    require_type(raw_entries, list, "registry.entries")
    if len(raw_entries) != 10:
        raise CompatibilityError("registry must contain exactly ten compatibility entries")
    entries: Dict[str, Mapping[str, Any]] = {}
    stable_ids = []
    candidate_ids = []
    for index, entry in enumerate(raw_entries):
        context = f"registry.entries[{index}]"
        require_type(entry, dict, context)
        require_closed(entry, ENTRY_FIELDS, context)
        key = require_nonempty_string(entry["key"], f"{context}.key")
        if key in entries:
            raise CompatibilityError(f"duplicate compatibility entry key: {key}")
        entries[key] = entry
        stable_id = require_nonempty_string(entry["stableId"], f"{context}.stableId")
        stable_ids.append(stable_id)
        if stable_id != EXPECTED_STABLE_IDS.get(key):
            raise CompatibilityError(f"{key} stable ID reservation drift")
        if not ID_RE.fullmatch(stable_id):
            raise CompatibilityError(f"{key} stable ID is malformed")
        state = entry["state"]
        if state not in {"unbound", "candidate", "stable"}:
            raise CompatibilityError(f"{key} has invalid state {state!r}")
        expected_state = "unbound" if key == "bytecode" else "candidate"
        if state != expected_state:
            raise CompatibilityError(
                f"{key} must remain {expected_state} while releaseClaim is reserved-not-stable"
            )
        candidate_id = entry["candidateId"]
        if candidate_id != EXPECTED_CANDIDATES.get(key):
            raise CompatibilityError(f"{key} candidate/source-current drift")
        if candidate_id is not None:
            require_nonempty_string(candidate_id, f"{context}.candidateId")
            if candidate_id.startswith("genesis/compat/v1/"):
                raise CompatibilityError(f"{key} candidate masquerades as a stable v1 identity")
            if candidate_id in stable_ids:
                raise CompatibilityError(f"{key} candidate reuses a stable identity")
            candidate_ids.append(candidate_id)
        require_nonempty_string(entry["compatibilityClass"], f"{context}.compatibilityClass")

        dependencies = entry["dependencies"]
        require_type(dependencies, list, f"{context}.dependencies")
        dependencies = require_unique(
            [require_nonempty_string(v, f"{context}.dependencies") for v in dependencies],
            f"{context}.dependencies",
        )
        if dependencies != EXPECTED_DEPENDENCIES.get(key):
            raise CompatibilityError(f"{key} dependency contract drift")

        authorities = entry["authorities"]
        components = entry["components"]
        require_type(authorities, list, f"{context}.authorities")
        require_type(components, list, f"{context}.components")
        if state == "unbound":
            if candidate_id is not None or authorities or components:
                raise CompatibilityError(f"unbound {key} must not claim a candidate, authority, or component")
        elif not authorities or not components:
            raise CompatibilityError(f"bound {key} requires authorities and components")
        for authority_index, authority in enumerate(authorities):
            validate_authority(
                authority, root, f"{context}.authorities[{authority_index}]", check_sources
            )
        if candidate_id is not None and not any(
            candidate_id in authority["contains"] for authority in authorities
        ):
            raise CompatibilityError(f"{key} candidate ID lacks a direct source authority")
        component_ids = []
        for component_index, component in enumerate(components):
            validate_component(
                component,
                f"{context}.components[{component_index}]",
                migration_records,
            )
            component_ids.append(component["id"])
        require_unique(component_ids, f"{context} component IDs")

        required = entry["promotionRequirements"]
        require_type(required, list, f"{context}.promotionRequirements")
        required = require_unique(
            [require_nonempty_string(v, f"{context}.promotionRequirements") for v in required],
            f"{context}.promotionRequirements",
        )
        if set(required) != EXPECTED_PROMOTIONS:
            raise CompatibilityError(f"{key} does not require the complete promotion set")

    if set(entries) != set(EXPECTED_STABLE_IDS):
        raise CompatibilityError("compatibility entry set drift")
    require_unique(stable_ids, "stable IDs")
    require_unique(candidate_ids, "candidate IDs")
    validate_dag(entries)
    if check_sources:
        validate_schema_marker(root)


def canonical_identity(data: Any) -> str:
    payload = json.dumps(data, sort_keys=True, separators=(",", ":"), ensure_ascii=True)
    return sha256(payload.encode("utf-8")).hexdigest()


def expect_reject(name: str, data: Any, root: Path) -> None:
    try:
        validate(data, root, check_sources=True)
    except CompatibilityError:
        return
    raise CompatibilityError(f"negative control was accepted: {name}")


def run_self_test(data: Any, root: Path) -> int:
    vectors = []

    mutated = deepcopy(data)
    mutated["unexpected"] = True
    vectors.append(("unknown-top-field", mutated))

    mutated = deepcopy(data)
    mutated["entries"][-1] = deepcopy(mutated["entries"][0])
    vectors.append(("duplicate-entry", mutated))

    mutated = deepcopy(data)
    mutated["entries"][0]["stableId"] = "genesis/compat/v1/reassigned"
    vectors.append(("stable-id-reassignment", mutated))

    mutated = deepcopy(data)
    mutated["entries"][1]["dependencies"] = ["language-profile", "unknown"]
    vectors.append(("unknown-dependency", mutated))

    mutated = deepcopy(data)
    mutated["entries"][0]["dependencies"] = ["coreform"]
    vectors.append(("dependency-cycle", mutated))

    mutated = deepcopy(data)
    mutated["entries"][0]["candidateId"] = mutated["entries"][0]["stableId"]
    vectors.append(("candidate-stable-confusion", mutated))

    mutated = deepcopy(data)
    bytecode = next(entry for entry in mutated["entries"] if entry["key"] == "bytecode")
    bytecode["components"] = [{
        "id": "fictional", "currentWriter": "1", "acceptedReaders": ["1"],
        "missingDiscriminator": "reject", "migrationRecords": [],
    }]
    vectors.append(("false-bytecode-binding", mutated))

    mutated = deepcopy(data)
    mutated["entries"][0]["promotionRequirements"].pop()
    vectors.append(("incomplete-promotion", mutated))

    mutated = deepcopy(data)
    mutated["entries"][0]["authorities"][0]["path"] = "/Users/example/source.rs"
    vectors.append(("host-path", mutated))

    mutated = deepcopy(data)
    mutated["entries"][0]["authorities"][0]["contains"] = "not present in source"
    vectors.append(("source-authority-drift", mutated))

    mutated = deepcopy(data)
    mutated["entries"][0]["components"][0]["unknown"] = True
    vectors.append(("unknown-component-field", mutated))

    mutated = deepcopy(data)
    mutated["entries"][0]["state"] = "stable"
    vectors.append(("premature-stability-claim", mutated))

    for name, vector in vectors:
        expect_reject(name, vector, root)

    try:
        json.loads('{"kind":"a","kind":"b"}', object_pairs_hook=reject_duplicate_keys)
    except CompatibilityError:
        pass
    else:
        raise CompatibilityError("duplicate JSON key negative control was accepted")

    print(f"v1-compatibility: self-test ok (negative_controls={len(vectors) + 1})")
    return len(vectors) + 1


def main(argv: Optional[Sequence[str]] = None) -> int:
    parser = argparse.ArgumentParser()
    parser.add_argument("root", nargs="?", type=Path, default=Path(__file__).resolve().parents[2])
    parser.add_argument("--self-test", action="store_true")
    args = parser.parse_args(argv)
    root = args.root.resolve()
    try:
        data = load_json(root / REGISTRY_REL)
        validate(data, root)
        if args.self_test:
            run_self_test(data, root)
        else:
            print(
                "v1-compatibility: ok "
                f"(entries=10 reserved=10 candidate=9 unbound=1 "
                f"identity={canonical_identity(data)})"
            )
        return 0
    except (CompatibilityError, OSError) as exc:
        print(f"v1-compatibility: {exc}", file=sys.stderr)
        return 1


if __name__ == "__main__":
    raise SystemExit(main())
