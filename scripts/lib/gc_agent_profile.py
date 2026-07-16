#!/usr/bin/env python3
"""Resolve and verify the frozen GC-AGENT-v0.3 training profile."""

from __future__ import annotations

import argparse
import copy
from hashlib import sha256
import json
from pathlib import Path
import re
import sys
from typing import Any, Dict, Mapping, Sequence, Tuple


ROOT = Path(__file__).resolve().parents[2]
POLICY_PATH = ROOT / "policies/gc_agent_profile_v0.3.json"
SCHEMA_PATH = ROOT / "docs/spec/GC_AGENT_PROFILE_v0.3.schema.json"
DOMAIN_ORDER = (
    "lexical-grammar",
    "coreform-mapping",
    "evaluation",
    "values",
    "contracts",
    "modules",
    "effects",
    "packages",
    "errors",
    "resource-limits",
    "compatibility-identifiers",
)
RUNTIME_VALUES = (
    "Data",
    "Int",
    "Vector",
    "Map",
    "Closure",
    "CompiledClosure",
    "SealToken",
    "Sealed",
    "NativeFn",
    "Contract",
    "EffectProgram",
    "EffectRequest",
)
SPECIAL_FORMS = (
    "quote",
    "fn",
    "if",
    "begin",
    "let",
    "prim",
    "seal",
    "unseal",
    "def",
    "application",
)
UNSUPPORTED_CLASS_ORDER = (
    "experimental-syntax",
    "host-only-operation",
    "unavailable-target",
    "nondeterministic-facility",
    "out-of-profile-capability",
)
REQUIRED_UNSUPPORTED_IDS = {
    "experimental-syntax": "U-SYNTAX-EXTENSIONS",
    "host-only-operation": "U-IMPLICIT-EFFECTS",
    "unavailable-target": "U-UNAVAILABLE-TARGETS",
    "nondeterministic-facility": "U-NONDETERMINISTIC-REPLAY",
    "out-of-profile-capability": "U-INDEX-AS-AUTHORITY",
}
UNSUPPORTED_ENFORCEMENT = {
    "reject",
    "explicit-effect-only",
    "deny-by-default",
    "profile-negotiation-required",
    "bounded-opt-in-only",
    "claim-prohibited",
}
HOST_PATH_RE = re.compile(r"(?:/Users/|/home/|/private/|[A-Za-z]:\\\\)")


class AgentProfileError(ValueError):
    pass


def reject_duplicate_keys(pairs: Sequence[Tuple[str, Any]]) -> Dict[str, Any]:
    result: Dict[str, Any] = {}
    for key, value in pairs:
        if key in result:
            raise AgentProfileError("duplicate JSON key: " + key)
        result[key] = value
    return result


def load_json(path: Path) -> Any:
    try:
        return json.loads(
            path.read_text(encoding="utf-8"), object_pairs_hook=reject_duplicate_keys
        )
    except FileNotFoundError as exc:
        raise AgentProfileError("missing input: " + display(path)) from exc
    except json.JSONDecodeError as exc:
        raise AgentProfileError(
            "invalid JSON in {}:{}:{}: {}".format(
                display(path), exc.lineno, exc.colno, exc.msg
            )
        ) from exc


def display(path: Path) -> str:
    try:
        return path.resolve().relative_to(ROOT).as_posix()
    except ValueError:
        return path.as_posix()


def require(condition: bool, message: str) -> None:
    if not condition:
        raise AgentProfileError(message)


def relative_file(rel: str, label: str) -> Path:
    require(isinstance(rel, str) and rel, label + " must be a path")
    path = Path(rel)
    require(not path.is_absolute() and ".." not in path.parts, label + " must be relative")
    resolved = ROOT / path
    require(resolved.is_file() and not resolved.is_symlink(), label + " is missing")
    return resolved


def canonical_bytes(value: Any) -> bytes:
    return json.dumps(value, sort_keys=True, separators=(",", ":"), ensure_ascii=True).encode("utf-8")


def file_digest(path: Path) -> str:
    return sha256(path.read_bytes()).hexdigest()


def domain_map(policy: Mapping[str, Any]) -> Mapping[str, Mapping[str, Any]]:
    return {item["id"]: item for item in policy["domains"]}


def rust_primitives() -> Sequence[str]:
    names = set()
    paths = [ROOT / "crates/gc_kernel/src/eval_prims.rs"]
    paths.extend(sorted((ROOT / "crates/gc_kernel/src/eval_prims").glob("*.rs")))
    arm = re.compile(r'^\s*"([^" ]+)"\s*=>', re.MULTILINE)
    for path in paths:
        names.update(arm.findall(path.read_text(encoding="utf-8")))
    require(names, "no kernel primitive match arms were discovered")
    return sorted(names)


def validate_schema() -> None:
    schema = load_json(SCHEMA_PATH)
    require(schema.get("$schema") == "https://json-schema.org/draft/2020-12/schema", "agent profile schema draft drift")
    require(schema.get("$id") == "https://genesiscode.dev/schemas/gc-agent-profile-v0.3.json", "agent profile schema id drift")
    require(schema.get("type") == "object" and schema.get("additionalProperties") is False, "agent profile schema root is open")
    order_schema = schema.get("properties", {}).get("unsupportedClassOrder", {})
    require(
        tuple(order_schema.get("items", {}).get("enum", [])) == UNSUPPORTED_CLASS_ORDER,
        "unsupported class schema order drift",
    )
    behavior_schema = schema.get("properties", {}).get("unsupportedBehavior", {})
    required_classes = []
    for rule in behavior_schema.get("allOf", []):
        required_class = (
            rule.get("contains", {})
            .get("properties", {})
            .get("roadmapClass", {})
            .get("const")
        )
        if required_class is not None:
            required_classes.append(required_class)
    require(tuple(required_classes) == UNSUPPORTED_CLASS_ORDER, "unsupported class containment schema drift")

    def walk(value: Any, location: str) -> None:
        if isinstance(value, dict):
            if value.get("type") == "object":
                require("additionalProperties" in value, location + " object schema is open")
                if value["additionalProperties"] is not False:
                    require("propertyNames" in value, location + " dynamic map has no key contract")
            for key, child in value.items():
                walk(child, location + "/" + key)
        elif isinstance(value, list):
            for index, child in enumerate(value):
                walk(child, location + "/" + str(index))

    walk(schema, display(SCHEMA_PATH))


def validate_policy(policy: Mapping[str, Any]) -> None:
    expected_fields = {
        "compatibility",
        "conformance",
        "domainOrder",
        "domains",
        "kind",
        "output",
        "profileId",
        "stability",
        "unsupportedBehavior",
        "unsupportedClassOrder",
        "version",
    }
    require(set(policy) == expected_fields, "agent profile policy fields drift")
    require(policy["kind"] == "genesis/gc-agent-profile-policy-v0.3", "agent profile policy kind drift")
    require(policy["version"] == "0.3" and policy["profileId"] == "GC-AGENT-v0.3", "agent profile identity drift")
    require(policy["stability"] == "training-frozen-pre-v1", "agent profile stability drift")
    require(tuple(policy["unsupportedClassOrder"]) == UNSUPPORTED_CLASS_ORDER, "unsupported class order drift")
    require(policy["output"] == "docs/spec/GC_AGENT_PROFILE_v0.3.json", "agent profile output drift")
    require(tuple(policy["domainOrder"]) == DOMAIN_ORDER, "agent profile domain order drift")
    domains = policy["domains"]
    require(isinstance(domains, list) and len(domains) == len(DOMAIN_ORDER), "agent profile must contain exactly eleven domains")
    require(tuple(item["id"] for item in domains) == DOMAIN_ORDER, "agent profile domain inventory drift")
    require(len(set(policy["compatibility"])) == len(policy["compatibility"]), "duplicate compatibility identity")

    source_paths = {display(POLICY_PATH), display(SCHEMA_PATH)}
    for domain in domains:
        require(domain["surface"] and len(domain["surface"]) == len(set(domain["surface"])), domain["id"] + " surface must be unique")
        require(domain["invariants"] and domain["limitations"], domain["id"] + " must state invariants and limitations")
        for authority in domain["authorities"]:
            path = relative_file(authority["path"], domain["id"] + " authority")
            source = path.read_text(encoding="utf-8")
            for anchor in authority["anchors"]:
                require(anchor in source, "{} authority anchor drift: {!r}".format(authority["path"], anchor))
            source_paths.add(authority["path"])
        for check in domain["conformanceChecks"]:
            relative_file(check, domain["id"] + " conformance check")
            source_paths.add(check)

    checks = policy["conformance"]["requiredChecks"]
    require(checks == sorted(checks) and len(checks) == len(set(checks)), "required checks must be sorted and unique")
    for check in checks:
        relative_file(check, "required check")
        source_paths.add(check)

    values = domain_map(policy)["values"]["surface"]
    require(values == list(RUNTIME_VALUES) + list(rust_primitives()), "runtime value or primitive inventory drift")
    require(domain_map(policy)["evaluation"]["surface"] == list(SPECIAL_FORMS), "special-form inventory drift")

    ids = []
    for group in ("parserCases", "evaluatorCases", "resourceCases", "packageCases"):
        cases = policy["conformance"][group]
        require(cases, group + " must not be empty")
        ids.extend(item["id"] for item in cases)
    require(len(ids) == len(set(ids)), "conformance case IDs must be globally unique")

    unsupported_ids = [item["id"] for item in policy["unsupportedBehavior"]]
    require(len(unsupported_ids) == len(set(unsupported_ids)), "unsupported behavior IDs must be unique")
    expected_unsupported_fields = {
        "behavior",
        "category",
        "enforcement",
        "id",
        "rationale",
        "roadmapClass",
        "roadmapTask",
        "safeAlternative",
        "status",
    }
    by_class: Dict[str, list[str]] = {}
    roadmap = (ROOT / "ROADMAP.md").read_text(encoding="utf-8")
    for item in policy["unsupportedBehavior"]:
        require(set(item) == expected_unsupported_fields, item.get("id", "unsupported behavior") + " fields drift")
        require(item["status"] == "unsupported", item["id"] + " status must fail closed")
        require(item["enforcement"] in UNSUPPORTED_ENFORCEMENT, item["id"] + " enforcement drift")
        require(item["roadmapClass"] in set(UNSUPPORTED_CLASS_ORDER) | {"additional-safety-boundary"}, item["id"] + " class drift")
        require(all(isinstance(item[key], str) and item[key].strip() for key in ("behavior", "category", "rationale", "safeAlternative")), item["id"] + " must be actionable")
        by_class.setdefault(item["roadmapClass"], []).append(item["id"])
        task = item["roadmapTask"]
        if task is not None:
            require("**{} ".format(task) in roadmap, item["id"] + " references an unknown roadmap task")
    for class_name, item_id in REQUIRED_UNSUPPORTED_IDS.items():
        require(by_class.get(class_name) == [item_id], class_name + " must have exactly one canonical unsupported record")

    versions = load_json(ROOT / "genesis.version-surfaces.json")
    compatibility = load_json(ROOT / "genesis.compatibility.json")
    compat = policy["compatibility"]
    require(compat["releaseTrain"] == versions["release_train"], "release train compatibility drift")
    require(compat["v1ReleaseClaim"] == compatibility["releaseClaim"], "v1 release claim drift")
    surface_by_id = {item["id"]: item for item in versions["surfaces"]}
    require(compat["effectLog"] == "genesis/effect-log/v" + surface_by_id["effect-log"]["current_writer"], "effect log profile drift")
    require(compat["packageManifest"] == surface_by_id["package-manifest"]["current_writer"], "package manifest profile drift")
    require(compat["genesisLock"] == surface_by_id["genesis-lock"]["current_writer"], "lock profile drift")
    require(compat["gpk"] == surface_by_id["gpk-bundle"]["current_writer"], "GPK profile drift")

    term_source = (ROOT / "crates/gc_coreform/src/term.rs").read_text(encoding="utf-8")
    value_source = (ROOT / "crates/gc_kernel/src/value.rs").read_text(encoding="utf-8")
    for key, constant in (
        ("languageProfile", "LANGUAGE_PROFILE_ID"),
        ("coreformProfile", "COREFORM_PROFILE_ID"),
        ("hashProfile", "HASH_PROFILE_ID"),
    ):
        require('{}: &str = "{}"'.format(constant, compat[key]) in term_source, key + " source identity drift")
    require('VALUE_EFFECT_HASH_PROFILE_ID: &str = "{}"'.format(compat["valueEffectHashProfile"]) in value_source, "value/effect hash profile drift")

    rendered = json.dumps(policy, sort_keys=True, ensure_ascii=True)
    require(HOST_PATH_RE.search(rendered) is None, "agent profile policy leaks an absolute host path")


def source_identities(policy: Mapping[str, Any]) -> Mapping[str, str]:
    paths = {display(POLICY_PATH), display(SCHEMA_PATH)}
    paths.update(("genesis.compatibility.json", "genesis.version-surfaces.json"))
    for domain in policy["domains"]:
        paths.update(item["path"] for item in domain["authorities"])
        paths.update(domain["conformanceChecks"])
    paths.update(policy["conformance"]["requiredChecks"])
    return {rel: file_digest(relative_file(rel, "profile source")) for rel in sorted(paths)}


def render_profile() -> Mapping[str, Any]:
    policy = load_json(POLICY_PATH)
    validate_policy(policy)
    profile = copy.deepcopy(policy)
    del profile["output"]
    profile["kind"] = "genesis/gc-agent-profile-v0.3"
    profile["sourceIdentities"] = source_identities(policy)
    profile["profileIdentitySha256"] = sha256(canonical_bytes(profile)).hexdigest()
    return profile


def render_json() -> str:
    return json.dumps(render_profile(), indent=2, sort_keys=True, ensure_ascii=True) + "\n"


def validate_candidate(candidate: Any) -> None:
    expected = render_profile()
    require(candidate == expected, "GC-AGENT-v0.3 contains stale or unsupported surface claims")
    material = dict(candidate)
    identity = material.pop("profileIdentitySha256", None)
    require(isinstance(identity, str) and sha256(canonical_bytes(material)).hexdigest() == identity, "GC-AGENT-v0.3 content identity mismatch")


def check() -> None:
    validate_schema()
    policy = load_json(POLICY_PATH)
    validate_policy(policy)
    output = relative_file(policy["output"], "agent profile output")
    require(output.read_text(encoding="utf-8") == render_json(), "GC-AGENT-v0.3 is stale; run: bash scripts/update_agent_authoring_bundle.sh profile")
    candidate = load_json(output)
    validate_candidate(candidate)
    print(
        "gc-agent-profile: ok (domains={} primitives={} parser_cases={} evaluator_cases={} resource_cases={} package_cases={} unsupported={} identity={})".format(
            len(candidate["domains"]),
            len(rust_primitives()),
            len(candidate["conformance"]["parserCases"]),
            len(candidate["conformance"]["evaluatorCases"]),
            len(candidate["conformance"]["resourceCases"]),
            len(candidate["conformance"]["packageCases"]),
            len(candidate["unsupportedBehavior"]),
            candidate["profileIdentitySha256"],
        )
    )


def self_test() -> None:
    expected = render_profile()
    controls = []

    def reject(label: str, mutate: Any) -> None:
        candidate = copy.deepcopy(expected)
        mutate(candidate)
        try:
            validate_candidate(candidate)
        except AgentProfileError:
            controls.append(label)
        else:
            raise AgentProfileError("self-test accepted " + label)

    reject("profile-id", lambda d: d.__setitem__("profileId", "GC-AGENT-v1"))
    reject("domain-omission", lambda d: d["domains"].pop())
    reject("domain-order", lambda d: d["domains"].reverse())
    reject("surface-broadening", lambda d: d["domains"][0]["surface"].append("reader/eval"))
    reject("limitation-erasure", lambda d: d["domains"][0].__setitem__("limitations", []))
    reject("primitive-omission", lambda d: next(x for x in d["domains"] if x["id"] == "values")["surface"].pop())
    reject("compatibility-promotion", lambda d: d["compatibility"].__setitem__("v1ReleaseClaim", "stable"))
    reject("bytecode-promotion", lambda d: d["unsupportedBehavior"].__setitem__(0, d["unsupportedBehavior"][-1]))
    reject("unsupported-class-omission", lambda d: d["unsupportedClassOrder"].pop())
    reject("unsupported-safe-alternative-erasure", lambda d: d["unsupportedBehavior"][0].__setitem__("safeAlternative", ""))
    reject("unsupported-enforcement-weakening", lambda d: d["unsupportedBehavior"][0].__setitem__("enforcement", "allow"))
    reject("parser-case-omission", lambda d: d["conformance"]["parserCases"].pop())
    reject("negative-case-flip", lambda d: d["conformance"]["parserCases"][-1].__setitem__("expected", "accept"))
    reject("resource-limit-removal", lambda d: d["conformance"]["resourceCases"].pop())
    reject("package-future-accept", lambda d: d["conformance"]["packageCases"][1].__setitem__("expected", "accept"))
    reject("source-tamper", lambda d: d["sourceIdentities"].__setitem__(next(iter(d["sourceIdentities"])), "0" * 64))
    reject("unknown-field", lambda d: d.__setitem__("trustMe", True))
    reject("host-path", lambda d: d["domains"][0]["limitations"].__setitem__(0, "/Users/example/private"))
    reject("identity-tamper", lambda d: d.__setitem__("profileIdentitySha256", "0" * 64))
    try:
        json.loads('{"kind":"a","kind":"b"}', object_pairs_hook=reject_duplicate_keys)
    except AgentProfileError:
        controls.append("duplicate-key")
    else:
        raise AgentProfileError("self-test accepted duplicate-key")
    require(len(controls) == 20, "agent profile self-test inventory drift")
    print("gc-agent-profile: self-test ok (negative_controls={})".format(len(controls)))


def main(argv: Sequence[str]) -> int:
    parser = argparse.ArgumentParser()
    mode = parser.add_mutually_exclusive_group(required=True)
    mode.add_argument("--render", action="store_true")
    mode.add_argument("--check", action="store_true")
    mode.add_argument("--self-test", action="store_true")
    args = parser.parse_args(argv)
    try:
        if args.render:
            sys.stdout.write(render_json())
        elif args.check:
            check()
        else:
            self_test()
    except AgentProfileError as exc:
        print("gc-agent-profile: " + str(exc), file=sys.stderr)
        return 1
    return 0


if __name__ == "__main__":
    raise SystemExit(main(sys.argv[1:]))
