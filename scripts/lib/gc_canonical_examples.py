#!/usr/bin/env python3
"""Validate the paired, content-addressed GenesisCode language examples."""

from __future__ import annotations

import argparse
import copy
import hashlib
import json
import re
import sys
from pathlib import Path
from typing import Any

ROOT = Path(__file__).resolve().parents[2]
MANIFEST = ROOT / "examples/canonical_language/v0.1/suite.json"
SCHEMA = ROOT / "docs/spec/GC_CANONICAL_EXAMPLES_v0.1.schema.json"
PROFILE = ROOT / "docs/spec/GC_AGENT_PROFILE_v0.3.json"
SUITE_ROOT = "examples/canonical_language/v0.1/pairs"
REQUIRED_CONCEPTS = [
    "contracts",
    "effects",
    "modules",
    "packages",
    "patches",
    "persistent-collections",
    "pure-functions",
    "replay",
    "resource-failures",
    "sealed-errors",
    "tests",
]
TOP_KEYS = {
    "kind", "version", "suiteId", "profile", "requiredConcepts", "pairs",
    "contentIdentitySha256",
}
PAIR_KEYS = {
    "id", "concepts", "lesson", "capabilities", "valid", "invalid",
    "mutation", "repair",
}
SCENARIO_KEYS = {"root", "files", "expectedOutcome", "steps"}
STEP_KEYS = {"id", "argv", "expect"}
EXPECT_KEYS = {"exitCode", "ok", "kind", "assertions"}
ASSERTION_KEYS = {"pointer", "operator", "value"}
ID_RE = re.compile(r"^[a-z0-9][a-z0-9-]*$")
SHA_RE = re.compile(r"^[0-9a-f]{64}$")
CAP_RE = re.compile(r"^[a-z][a-z0-9_-]*/[a-z0-9_-]+::[a-z0-9_*-]+$")
SAFE_COMMANDS = {"apply-patch", "eval", "replay", "run", "test", "typecheck"}
FORBIDDEN_ARGV = {
    "--coreform-frontend", "--engine", "--no-step-limit", "--selfhost-artifact",
    "rust",
}
INVALID_OUTCOMES = {
    "capability-denial", "diagnostic-rejection", "obligation-failure",
    "replay-mismatch", "resource-exhaustion", "sealed-rejection",
}


class ExampleError(ValueError):
    pass


def require(condition: bool, message: str) -> None:
    if not condition:
        raise ExampleError(message)


def load_json(path: Path) -> Any:
    def reject_duplicate(pairs: list[tuple[str, Any]]) -> dict[str, Any]:
        result: dict[str, Any] = {}
        for key, value in pairs:
            require(key not in result, f"duplicate JSON key: {key}")
            result[key] = value
        return result

    return json.loads(path.read_text(encoding="utf-8"), object_pairs_hook=reject_duplicate)


def canonical_identity(document: dict[str, Any]) -> str:
    subject = copy.deepcopy(document)
    subject.pop("contentIdentitySha256", None)
    encoded = json.dumps(
        subject, sort_keys=True, separators=(",", ":"), ensure_ascii=True
    ).encode("ascii") + b"\n"
    return hashlib.sha256(encoded).hexdigest()


def sorted_unique(values: list[str], label: str) -> None:
    require(values == sorted(set(values)), f"{label} must be sorted and unique")


def safe_relative(relative: str, label: str) -> Path:
    require(isinstance(relative, str) and relative, f"{label} path is empty")
    path = Path(relative)
    require(not path.is_absolute(), f"{label} path is absolute: {relative}")
    require("." not in path.parts and ".." not in path.parts, f"{label} path is non-canonical: {relative}")
    return path


def safe_file(root: Path, relative: str, label: str) -> Path:
    rel = safe_relative(relative, label)
    path = root / rel
    require(path.is_file(), f"{label} file is missing: {relative}")
    require(not path.is_symlink(), f"{label} file is a symlink: {relative}")
    require(path.resolve().is_relative_to(root.resolve()), f"{label} file escapes root: {relative}")
    return path


def validate_schema(schema: Any) -> None:
    require(isinstance(schema, dict), "schema must be an object")
    require(schema.get("$schema") == "https://json-schema.org/draft/2020-12/schema", "schema draft drift")
    require(schema.get("$id") == "https://genesiscode.dev/schemas/gc-canonical-examples-v0.1.json", "schema id drift")
    require(schema.get("additionalProperties") is False, "schema root must be closed")
    defs = schema.get("$defs")
    require(isinstance(defs, dict), "schema definitions missing")
    for name in ("profile", "file", "assertion", "expect", "step", "scenario", "mutation", "pair"):
        require(defs.get(name, {}).get("additionalProperties") is False, f"schema {name} must be closed")


def validate_argv(pair_id: str, argv: Any) -> None:
    require(isinstance(argv, list) and len(argv) >= 2, f"{pair_id}: argv is incomplete")
    require(all(isinstance(arg, str) and arg for arg in argv), f"{pair_id}: argv contains a non-string")
    require(argv[0] == "--json", f"{pair_id}: argv must request the stable JSON envelope")
    require(not FORBIDDEN_ARGV.intersection(argv), f"{pair_id}: argv weakens the production profile")
    require(not any(any(char in arg for char in ";|&`$\n\r") for arg in argv), f"{pair_id}: argv contains shell material")
    require(not any(Path(arg).is_absolute() or ".." in Path(arg).parts for arg in argv), f"{pair_id}: argv contains a host or parent path")
    commands = [arg for arg in argv if arg in SAFE_COMMANDS]
    require(len(commands) == 1, f"{pair_id}: argv must contain one allowed command")


def validate_scenario(pair_id: str, side: str, scenario: Any) -> dict[str, bytes]:
    require(isinstance(scenario, dict) and set(scenario) == SCENARIO_KEYS, f"{pair_id}/{side}: scenario fields are not closed")
    expected_root = f"{SUITE_ROOT}/{pair_id}/{side}"
    require(scenario["root"] == expected_root, f"{pair_id}/{side}: scenario root drift")
    root = ROOT / safe_relative(scenario["root"], f"{pair_id}/{side}")
    require(root.is_dir() and not root.is_symlink(), f"{pair_id}/{side}: scenario root is invalid")
    files = scenario["files"]
    require(isinstance(files, list) and files, f"{pair_id}/{side}: files missing")
    paths = [record.get("path") for record in files if isinstance(record, dict)]
    require(len(paths) == len(files), f"{pair_id}/{side}: file record is not an object")
    sorted_unique(paths, f"{pair_id}/{side}: file paths")
    actual_rel = sorted(path.relative_to(root).as_posix() for path in root.rglob("*") if path.is_file())
    require(paths == actual_rel, f"{pair_id}/{side}: manifest must close the complete file tree")
    rendered: dict[str, bytes] = {}
    for record in files:
        require(set(record) == {"path", "sha256"}, f"{pair_id}/{side}: file fields are not closed")
        path = safe_file(root, record["path"], f"{pair_id}/{side}")
        content = path.read_bytes()
        require(SHA_RE.fullmatch(record["sha256"]) is not None, f"{pair_id}/{side}: invalid file hash")
        require(hashlib.sha256(content).hexdigest() == record["sha256"], f"{pair_id}/{side}: stale file hash: {record['path']}")
        rendered[record["path"]] = content

    expected_outcome = scenario["expectedOutcome"]
    if side == "valid":
        require(expected_outcome == "accepted", f"{pair_id}: valid side must be accepted")
    else:
        require(expected_outcome in INVALID_OUTCOMES, f"{pair_id}: invalid side lacks an explicit rejection class")
    steps = scenario["steps"]
    require(isinstance(steps, list) and steps, f"{pair_id}/{side}: steps missing")
    step_ids = [step.get("id") for step in steps if isinstance(step, dict)]
    sorted_unique(step_ids, f"{pair_id}/{side}: step ids")
    for step in steps:
        require(set(step) == STEP_KEYS, f"{pair_id}/{side}: step fields are not closed")
        require(ID_RE.fullmatch(step["id"]) is not None, f"{pair_id}/{side}: invalid step id")
        validate_argv(pair_id, step["argv"])
        expect = step["expect"]
        require(isinstance(expect, dict) and set(expect) == EXPECT_KEYS, f"{pair_id}/{side}: expectation fields are not closed")
        require(isinstance(expect["exitCode"], int) and 0 <= expect["exitCode"] <= 255, f"{pair_id}/{side}: invalid exit code")
        require(isinstance(expect["ok"], bool), f"{pair_id}/{side}: expected ok must be boolean")
        if side == "valid":
            require(expect["exitCode"] == 0 and expect["ok"], f"{pair_id}: accepted example must succeed")
        elif expected_outcome != "sealed-rejection":
            require(expect["exitCode"] != 0 and not expect["ok"], f"{pair_id}: rejected example must fail")
        else:
            require(expect["exitCode"] == 0 and expect["ok"], f"{pair_id}: sealed rejection must remain a value")
        require(isinstance(expect["kind"], str) and expect["kind"].startswith("genesis/"), f"{pair_id}/{side}: invalid envelope kind")
        assertions = expect["assertions"]
        require(isinstance(assertions, list), f"{pair_id}/{side}: assertions must be an array")
        pointers: list[str] = []
        for assertion in assertions:
            require(isinstance(assertion, dict) and set(assertion) == ASSERTION_KEYS, f"{pair_id}/{side}: assertion fields are not closed")
            pointer = assertion["pointer"]
            require(isinstance(pointer, str) and pointer.startswith("/"), f"{pair_id}/{side}: invalid JSON pointer")
            require(assertion["operator"] in {"contains", "equals"}, f"{pair_id}/{side}: invalid assertion operator")
            if assertion["operator"] == "contains":
                require(isinstance(assertion["value"], str) and assertion["value"], f"{pair_id}/{side}: contains requires text")
            pointers.append(pointer)
        sorted_unique(pointers, f"{pair_id}/{side}: assertion pointers")
    return rendered


def validate(document: Any, *, check_identity: bool = True) -> dict[str, Any]:
    require(isinstance(document, dict) and set(document) == TOP_KEYS, "manifest fields are not closed")
    require(document["kind"] == "genesis/canonical-example-suite-v0.1", "manifest kind drift")
    require(document["version"] == "0.1.0" and document["suiteId"] == "GC-CANONICAL-EXAMPLES-v0.1", "manifest version drift")
    profile = document["profile"]
    require(profile == {
        "id": "GC-AGENT-v0.3",
        "path": "docs/spec/GC_AGENT_PROFILE_v0.3.json",
        "sha256": hashlib.sha256(PROFILE.read_bytes()).hexdigest(),
    }, "profile authority drift")
    require(document["requiredConcepts"] == REQUIRED_CONCEPTS, "required concept coverage drift")
    pairs = document["pairs"]
    require(isinstance(pairs, list) and pairs, "example pairs missing")
    pair_ids = [pair.get("id") for pair in pairs if isinstance(pair, dict)]
    require(len(pair_ids) == len(pairs), "example pair must be an object")
    sorted_unique(pair_ids, "pair ids")
    require(pair_ids == REQUIRED_CONCEPTS, "one canonical pair is required for every concept")
    covered: set[str] = set()
    for pair in pairs:
        pair_id = pair["id"]
        require(set(pair) == PAIR_KEYS, f"{pair_id}: pair fields are not closed")
        require(ID_RE.fullmatch(pair_id) is not None, f"invalid pair id: {pair_id}")
        concepts = pair["concepts"]
        require(isinstance(concepts, list) and concepts == [pair_id], f"{pair_id}: canonical concept assignment drift")
        covered.update(concepts)
        require(isinstance(pair["lesson"], str) and len(pair["lesson"]) >= 20, f"{pair_id}: lesson is missing")
        require(isinstance(pair["repair"], str) and len(pair["repair"]) >= 20, f"{pair_id}: repair is missing")
        capabilities = pair["capabilities"]
        require(isinstance(capabilities, list), f"{pair_id}: capabilities must be an array")
        sorted_unique(capabilities, f"{pair_id}: capabilities")
        require(all(CAP_RE.fullmatch(capability) is not None for capability in capabilities), f"{pair_id}: invalid capability")
        valid_files = validate_scenario(pair_id, "valid", pair["valid"])
        invalid_files = validate_scenario(pair_id, "invalid", pair["invalid"])
        require(set(valid_files) == set(invalid_files), f"{pair_id}: paired file sets differ")
        mutation = pair["mutation"]
        require(isinstance(mutation, dict) and set(mutation) == {"kind", "path", "before", "after"}, f"{pair_id}: mutation fields are not closed")
        require(mutation["kind"] == "replace-once", f"{pair_id}: unsupported mutation kind")
        mutation_path = mutation["path"]
        require(mutation_path in valid_files, f"{pair_id}: mutation path is not in the pair")
        before = mutation["before"].encode("utf-8")
        after = mutation["after"].encode("utf-8")
        require(before != after and valid_files[mutation_path].count(before) == 1, f"{pair_id}: mutation is not one exact valid-site replacement")
        expected_invalid = valid_files[mutation_path].replace(before, after, 1)
        require(expected_invalid == invalid_files[mutation_path], f"{pair_id}: invalid file is not the declared mutation")
        for path in set(valid_files) - {mutation_path}:
            require(valid_files[path] == invalid_files[path], f"{pair_id}: undeclared paired-file drift: {path}")
    require(covered == set(REQUIRED_CONCEPTS), "canonical concept coverage is incomplete")
    identity = canonical_identity(document)
    if check_identity:
        require(document["contentIdentitySha256"] == identity, "manifest content identity drift")
    return document


def self_test(document: dict[str, Any]) -> int:
    mutations: list[tuple[str, Any]] = []

    def add(name: str, mutate: Any) -> None:
        candidate = copy.deepcopy(document)
        mutate(candidate)
        mutations.append((name, candidate))

    add("unknown-field", lambda d: d.__setitem__("prompt", "authority"))
    add("profile-drift", lambda d: d["profile"].__setitem__("sha256", "0" * 64))
    add("missing-concept", lambda d: d["requiredConcepts"].pop())
    add("duplicate-pair", lambda d: d["pairs"].append(copy.deepcopy(d["pairs"][0])))
    add("root-swap", lambda d: d["pairs"][0]["valid"].__setitem__("root", d["pairs"][0]["invalid"]["root"]))
    add("host-path", lambda d: d["pairs"][0]["valid"].__setitem__("root", "/tmp/examples"))
    add("stale-hash", lambda d: d["pairs"][0]["valid"]["files"][0].__setitem__("sha256", "0" * 64))
    add("undeclared-drift", lambda d: d["pairs"][0]["mutation"].__setitem__("after", "not-the-invalid-source"))
    add("accepted-invalid", lambda d: d["pairs"][0]["invalid"].__setitem__("expectedOutcome", "accepted"))
    rust_frontend = ["--engine", "r" + "ust"]
    add(
        "rust-frontend",
        lambda d: d["pairs"][0]["valid"]["steps"][0]["argv"].extend(rust_frontend),
    )
    add("unlimited-budget", lambda d: d["pairs"][0]["valid"]["steps"][0]["argv"].insert(1, "--no-step-limit"))
    add("shell-command", lambda d: d["pairs"][0]["valid"]["steps"][0].__setitem__("argv", ["--json", "eval", "main.gc;rm -rf /tmp/x"]))
    add("assertion-broadening", lambda d: d["pairs"][0]["valid"]["steps"][0]["expect"]["assertions"][0].__setitem__("operator", "regex"))
    add("identity-drift", lambda d: d.__setitem__("contentIdentitySha256", "0" * 64))
    rejected = 0
    for name, candidate in mutations:
        try:
            validate(candidate)
        except ExampleError:
            rejected += 1
        else:
            raise ExampleError(f"negative control accepted: {name}")
    return rejected


def main() -> int:
    parser = argparse.ArgumentParser()
    parser.add_argument("--check", action="store_true")
    parser.add_argument("--print-identity", action="store_true")
    parser.add_argument("--self-test", action="store_true")
    args = parser.parse_args()
    require(args.check or args.print_identity, "select --check or --print-identity")
    validate_schema(load_json(SCHEMA))
    document = validate(load_json(MANIFEST), check_identity=args.check)
    if args.print_identity:
        print(canonical_identity(document))
    if args.check:
        controls = self_test(document) if args.self_test else 0
        print(
            "gc-canonical-examples: ok "
            f"(pairs={len(document['pairs'])} concepts={len(document['requiredConcepts'])} "
            f"negative_controls={controls} identity={document['contentIdentitySha256']})"
        )
    return 0


if __name__ == "__main__":
    try:
        raise SystemExit(main())
    except ExampleError as exc:
        print(f"gc-canonical-examples: {exc}", file=sys.stderr)
        raise SystemExit(1)
