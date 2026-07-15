#!/usr/bin/env python3
"""Validate the closed, content-addressed GenesisCode agent corpus manifest."""

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
MANIFEST = ROOT / "docs/spec/GC_AGENT_CORPUS_v0.1.json"
SCHEMA = ROOT / "docs/spec/GC_AGENT_CORPUS_v0.1.schema.json"
SHA_RE = re.compile(r"^[0-9a-f]{64}$")
ID_RE = re.compile(r"^[a-z0-9][a-z0-9-]*$")
TOP_KEYS = {"kind", "version", "corpusId", "profile", "sourcePolicy", "licensePolicy", "splitPolicy", "entries", "contentIdentitySha256"}
ENTRY_KEYS = {"id", "role", "artifacts", "generator", "profileId", "capabilities", "tests", "difficulty", "oracleExposure"}


class CorpusError(ValueError):
    pass


def require(condition: bool, message: str) -> None:
    if not condition:
        raise CorpusError(message)


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
    encoded = (json.dumps(subject, sort_keys=True, separators=(",", ":"), ensure_ascii=True) + "\n").encode()
    return hashlib.sha256(encoded).hexdigest()


def safe_file(relative: str) -> Path:
    require(relative and not relative.startswith("/"), f"absolute corpus path: {relative}")
    parts = Path(relative).parts
    require(".." not in parts and "." not in parts, f"non-canonical corpus path: {relative}")
    path = ROOT / relative
    require(path.is_file(), f"missing corpus artifact: {relative}")
    require(not path.is_symlink(), f"symlink corpus artifact: {relative}")
    resolved = path.resolve()
    require(resolved.is_relative_to(ROOT.resolve()), f"escaped corpus artifact: {relative}")
    return path


def sorted_unique(values: list[str], label: str) -> None:
    require(values == sorted(set(values)), f"{label} must be sorted and unique")


def validate_schema_marker(schema: Any) -> None:
    require(isinstance(schema, dict), "schema must be an object")
    require(schema.get("$schema") == "https://json-schema.org/draft/2020-12/schema", "schema draft drift")
    require(schema.get("$id") == "https://genesiscode.dev/schemas/gc-agent-corpus-v0.1.json", "schema id drift")
    require(schema.get("additionalProperties") is False, "schema root must be closed")
    defs = schema.get("$defs")
    require(isinstance(defs, dict), "schema definitions missing")
    for name in ("profile", "sourcePolicy", "licensePolicy", "splitPolicy", "artifact", "generator", "difficulty", "test", "entry"):
        require(defs.get(name, {}).get("additionalProperties") is False, f"schema {name} must be closed")


def validate(document: Any, *, check_identity: bool = True) -> dict[str, Any]:
    require(isinstance(document, dict), "manifest must be an object")
    require(set(document) == TOP_KEYS, "manifest fields are not closed")
    require(document["kind"] == "genesis/agent-corpus-manifest-v0.1", "manifest kind drift")
    require(document["version"] == "0.1.0" and document["corpusId"] == "GC-AGENT-CORPUS-v0.1", "manifest version drift")

    profile = document["profile"]
    require(set(profile) == {"id", "path", "sha256"}, "profile fields are not closed")
    require(profile["id"] == "GC-AGENT-v0.3" and profile["path"] == "docs/spec/GC_AGENT_PROFILE_v0.3.json", "profile authority drift")
    profile_path = safe_file(profile["path"])
    require(hashlib.sha256(profile_path.read_bytes()).hexdigest() == profile["sha256"], "profile hash drift")

    source_policy = document["sourcePolicy"]
    require(source_policy == {
        "repository": "https://github.com/corbensorenson/genesisCode",
        "revisionBinding": "artifact-sha256-plus-manifest-identity",
        "provenanceKinds": ["deterministically-generated", "genesiscode-repository-authored"],
    }, "source provenance policy drift")

    licenses = document["licensePolicy"]
    require(licenses == {"allowedSpdx": ["Apache-2.0", "MIT"], "repositoryDefault": "Apache-2.0 OR MIT"}, "license policy drift")
    splits = document["splitPolicy"]
    require(splits == {"roles": ["train", "dev", "public-test", "held-out"], "heldOutContentMayBeDistributed": False, "oracleSeparationRequired": True}, "split policy drift")

    entries = document["entries"]
    require(isinstance(entries, list) and entries, "corpus entries missing")
    ids = [entry.get("id") for entry in entries if isinstance(entry, dict)]
    require(len(ids) == len(entries), "corpus entry must be an object")
    sorted_unique(ids, "entry ids")
    roles: set[str] = set()
    artifact_paths: set[str] = set()

    for entry in entries:
        entry_id = entry["id"]
        require(ID_RE.fullmatch(entry_id) is not None, f"invalid entry id: {entry_id}")
        require(set(entry) == ENTRY_KEYS, f"{entry_id}: entry fields are not closed")
        role = entry["role"]
        require(role in splits["roles"], f"{entry_id}: unknown role")
        roles.add(role)
        require(entry["profileId"] == profile["id"], f"{entry_id}: profile mismatch")

        artifacts = entry["artifacts"]
        require(isinstance(artifacts, list) and artifacts, f"{entry_id}: artifacts missing")
        paths = [artifact.get("path") for artifact in artifacts if isinstance(artifact, dict)]
        require(len(paths) == len(artifacts), f"{entry_id}: artifact must be an object")
        sorted_unique(paths, f"{entry_id}: artifact paths")
        byte_count = 0
        for artifact in artifacts:
            require(set(artifact) == {"path", "sha256", "spdx", "provenance"}, f"{entry_id}: artifact fields are not closed")
            relative = artifact["path"]
            require(relative not in artifact_paths, f"artifact assigned to multiple entries: {relative}")
            artifact_paths.add(relative)
            path = safe_file(relative)
            digest = hashlib.sha256(path.read_bytes()).hexdigest()
            require(SHA_RE.fullmatch(artifact["sha256"]) is not None and digest == artifact["sha256"], f"{entry_id}: stale artifact hash: {relative}")
            require(artifact["spdx"] in licenses["allowedSpdx"], f"{entry_id}: unapproved license")
            require(artifact["provenance"] == "genesiscode-repository-authored", f"{entry_id}: unknown provenance")
            byte_count += path.stat().st_size

        generator = entry["generator"]
        require(set(generator) == {"kind", "identity", "path", "sha256"}, f"{entry_id}: generator fields are not closed")
        require(re.fullmatch(r"[a-z0-9][a-z0-9._/-]*", generator["identity"]) is not None, f"{entry_id}: invalid generator identity")
        if generator["kind"] == "repository-authored":
            require(generator["path"] is None and generator["sha256"] is None, f"{entry_id}: authored entry cannot claim a generator")
        else:
            require(generator["kind"] == "deterministic-generator", f"{entry_id}: unknown generator kind")
            generator_path = safe_file(generator["path"])
            require(hashlib.sha256(generator_path.read_bytes()).hexdigest() == generator["sha256"], f"{entry_id}: generator hash drift")

        capabilities = entry["capabilities"]
        require(isinstance(capabilities, list), f"{entry_id}: capabilities must be an array")
        sorted_unique(capabilities, f"{entry_id}: capabilities")
        tests = entry["tests"]
        require(isinstance(tests, list) and tests, f"{entry_id}: tests missing")
        test_ids = [test.get("id") for test in tests if isinstance(test, dict)]
        sorted_unique(test_ids, f"{entry_id}: test ids")
        for test in tests:
            require(set(test) == {"id", "argv"}, f"{entry_id}: test fields are not closed")
            require(ID_RE.fullmatch(test["id"]) is not None, f"{entry_id}: invalid test id")
            argv = test["argv"]
            require(
                isinstance(argv, list)
                and len(argv) == 2
                and argv[0] == "bash"
                and isinstance(argv[1], str)
                and re.fullmatch(r"scripts/check_[A-Za-z0-9_]+\.sh", argv[1]) is not None,
                f"{entry_id}: test argv must name one repository check without shell arguments",
            )
            safe_file(argv[1])

        difficulty = entry["difficulty"]
        require(set(difficulty) == {"level", "contextBytes", "concepts"}, f"{entry_id}: difficulty fields are not closed")
        require(difficulty["level"] in {"introductory", "intermediate", "advanced", "adversarial"}, f"{entry_id}: invalid difficulty")
        require(difficulty["contextBytes"] == byte_count, f"{entry_id}: context byte count drift")
        concepts = difficulty["concepts"]
        require(isinstance(concepts, list) and concepts, f"{entry_id}: concepts missing")
        sorted_unique(concepts, f"{entry_id}: concepts")
        exposure = entry["oracleExposure"]
        require(exposure in {"public", "commitment-only"}, f"{entry_id}: invalid oracle exposure")
        if role == "held-out":
            require(exposure == "commitment-only", f"{entry_id}: held-out oracle leakage")
        else:
            require(exposure == "public", f"{entry_id}: public split must be inspectable")

    require({"train", "dev", "public-test"}.issubset(roles), "manifest lacks train/dev/public-test coverage")
    require("held-out" not in roles, "held-out payloads must remain outside the distributed corpus")
    identity = canonical_identity(document)
    if check_identity:
        require(document["contentIdentitySha256"] == identity, "manifest content identity drift")
    return document


def self_test(document: dict[str, Any]) -> int:
    mutations: list[tuple[str, Any]] = []
    def add(name: str, mutate: Any) -> None:
        candidate = copy.deepcopy(document); mutate(candidate); mutations.append((name, candidate))
    add("unknown-field", lambda d: d.__setitem__("prompt", "authority"))
    add("stale-profile", lambda d: d["profile"].__setitem__("sha256", "0" * 64))
    add("host-path", lambda d: d["entries"][0]["artifacts"][0].__setitem__("path", "/tmp/secret"))
    add("stale-artifact", lambda d: d["entries"][0]["artifacts"][0].__setitem__("sha256", "0" * 64))
    add("license-broadening", lambda d: d["licensePolicy"]["allowedSpdx"].append("UNKNOWN"))
    add("provenance-rebinding", lambda d: d["sourcePolicy"].__setitem__("revisionBinding", "mutable-branch"))
    add("split-broadening", lambda d: d["splitPolicy"].__setitem__("heldOutContentMayBeDistributed", True))
    add("oracle-leak", lambda d: (d["entries"][0].__setitem__("role", "held-out"), d["entries"][0].__setitem__("oracleExposure", "public")))
    add("profile-mismatch", lambda d: d["entries"][0].__setitem__("profileId", "prompt-selected"))
    add("generator-claim", lambda d: d["entries"][0]["generator"].__setitem__("path", "README.md"))
    add("test-shell", lambda d: d["entries"][0]["tests"][0].__setitem__("argv", ["bash", "-c", "true"]))
    add("context-drift", lambda d: d["entries"][0]["difficulty"].__setitem__("contextBytes", 1))
    add("identity-drift", lambda d: d.__setitem__("contentIdentitySha256", "0" * 64))
    rejected = 0
    for name, candidate in mutations:
        try: validate(candidate)
        except CorpusError: rejected += 1
        else: raise CorpusError(f"negative control accepted: {name}")
    return rejected


def main() -> int:
    parser = argparse.ArgumentParser()
    parser.add_argument("--check", action="store_true")
    parser.add_argument("--print-identity", action="store_true")
    parser.add_argument("--self-test", action="store_true")
    args = parser.parse_args()
    require(args.check or args.print_identity, "select --check or --print-identity")
    validate_schema_marker(load_json(SCHEMA))
    document = validate(load_json(MANIFEST), check_identity=args.check)
    if args.print_identity: print(canonical_identity(document))
    if args.check:
        controls = self_test(document) if args.self_test else 0
        print(f"gc-agent-corpus: ok (entries={len(document['entries'])} negative_controls={controls} identity={document['contentIdentitySha256']})")
    return 0


if __name__ == "__main__":
    try: raise SystemExit(main())
    except CorpusError as exc:
        print(f"gc-agent-corpus: {exc}", file=sys.stderr)
        raise SystemExit(1)
