#!/usr/bin/env python3
"""Generate and independently query the frozen GC-AGENT-v0.3 symbol index."""

from __future__ import annotations

import argparse
import copy
import hashlib
import json
import re
import sys
from pathlib import Path
from typing import Any, Dict, List, Mapping, Sequence, Tuple

ROOT = Path(__file__).resolve().parents[2]
POLICY = ROOT / "policies/gc_agent_symbol_index_v0.3.json"
PROFILE = ROOT / "docs/spec/GC_AGENT_PROFILE_v0.3.json"
SCHEMA = ROOT / "docs/spec/GC_AGENT_SYMBOL_INDEX_v0.3.schema.json"
OUTPUT = ROOT / "docs/spec/GC_AGENT_SYMBOL_INDEX_v0.3.json"
HOST_PATH_RE = re.compile(r"(?:/Users/|/home/|[A-Za-z]:\\\\)")
SHA256_RE = re.compile(r"^[0-9a-f]{64}$")


class IndexError(RuntimeError):
    pass


def require(condition: bool, message: str) -> None:
    if not condition:
        raise IndexError(message)


def reject_duplicate(pairs: Sequence[Tuple[str, Any]]) -> Dict[str, Any]:
    out: Dict[str, Any] = {}
    for key, value in pairs:
        require(key not in out, "duplicate JSON key: " + key)
        out[key] = value
    return out


def load(path: Path) -> Mapping[str, Any]:
    require(path.is_file(), "missing input: " + str(path.relative_to(ROOT)))
    try:
        value = json.loads(path.read_text(encoding="utf-8"), object_pairs_hook=reject_duplicate)
    except (OSError, UnicodeError, json.JSONDecodeError) as exc:
        raise IndexError("invalid JSON {}: {}".format(path.relative_to(ROOT), exc)) from exc
    require(isinstance(value, dict), "top-level JSON value must be an object: " + str(path.relative_to(ROOT)))
    return value


def canonical(value: Any) -> bytes:
    return json.dumps(value, sort_keys=True, separators=(",", ":"), ensure_ascii=True).encode("ascii")


def digest(path: Path) -> str:
    return hashlib.sha256(path.read_bytes()).hexdigest()


def unique(items: Sequence[str]) -> List[str]:
    return list(dict.fromkeys(items))


def validate_relpath(raw: str) -> Path:
    require(isinstance(raw, str) and raw, "source path must be a nonempty string")
    path = Path(raw)
    require(not path.is_absolute() and ".." not in path.parts and "." not in path.parts, "source path must be canonical and repository-relative: " + raw)
    resolved = ROOT / path
    require(resolved.exists() and not resolved.is_symlink(), "source path missing or symlinked: " + raw)
    return resolved


def validate_policy(policy: Mapping[str, Any], profile: Mapping[str, Any]) -> None:
    expected = {
        "kind", "version", "profileId", "output", "schema", "exactLookup",
        "diagnosticsByDomain", "capabilitiesBySymbol", "effectsBySymbol",
        "callableSignatures", "formSignatures", "deprecations",
    }
    require(set(policy) == expected, "symbol-index policy fields drift")
    require(policy["kind"] == "genesis/gc-agent-symbol-index-policy-v0.3", "policy kind drift")
    require(policy["version"] == "0.3" and policy["profileId"] == "GC-AGENT-v0.3", "policy profile drift")
    require(policy["profileId"] == profile.get("profileId"), "policy/profile ID mismatch")
    require(policy["output"] == str(OUTPUT.relative_to(ROOT)), "policy output drift")
    require(policy["schema"] == str(SCHEMA.relative_to(ROOT)), "policy schema drift")
    require(policy["exactLookup"] == {"caseSensitive": True, "normalization": "none", "maxResults": 1}, "exact lookup must remain single-result, case-sensitive, and normalization-free")

    domains = profile.get("domains")
    require(isinstance(domains, list) and domains, "profile domains missing")
    domain_ids = [domain.get("id") for domain in domains]
    require(len(domain_ids) == len(set(domain_ids)), "duplicate profile domain")
    require(set(policy["diagnosticsByDomain"]) == set(domain_ids), "diagnostic domain coverage drift")
    for domain, diagnostics in policy["diagnosticsByDomain"].items():
        require(isinstance(diagnostics, list) and diagnostics and all(isinstance(item, str) and item for item in diagnostics), "invalid diagnostics for " + domain)
        require(len(diagnostics) == len(set(diagnostics)), "duplicate diagnostics for " + domain)

    surface = {symbol for domain in domains for symbol in domain.get("surface", [])}
    for map_name in ("capabilitiesBySymbol", "effectsBySymbol", "callableSignatures", "formSignatures", "deprecations"):
        mapping = policy[map_name]
        require(isinstance(mapping, dict), map_name + " must be an object")
        require(set(mapping).issubset(surface), map_name + " names symbols outside the frozen profile")

    callable_signatures = policy["callableSignatures"]
    for symbol, signature in callable_signatures.items():
        require(set(signature) == {"invocation", "parameters", "returns"}, "callable signature fields drift: " + symbol)
        require(signature["invocation"] in ("primitive", "function"), "invalid invocation: " + symbol)
        require(isinstance(signature["parameters"], list) and all(isinstance(item, str) and item for item in signature["parameters"]), "invalid parameters: " + symbol)
        require(isinstance(signature["returns"], str) and signature["returns"], "invalid return type: " + symbol)

    primitive_source = (ROOT / "crates/gc_kernel/src/eval_prims.rs").read_text(encoding="utf-8")
    runtime_primitives = set(re.findall(r'\"([^\"]+)\"\s*=>\s*Some\(PrimOp::', primitive_source))
    indexed_primitives = {symbol for symbol, signature in callable_signatures.items() if signature["invocation"] == "primitive"}
    require(indexed_primitives == runtime_primitives, "primitive signature coverage drift")
    require(set(policy["formSignatures"]) == {"quote", "fn", "if", "begin", "let", "prim", "seal", "unseal", "def", "application"}, "special-form signature coverage drift")


def signature_for(symbol: str, domains: Sequence[str], policy: Mapping[str, Any]) -> Mapping[str, Any]:
    callable_signature = policy["callableSignatures"].get(symbol)
    if callable_signature is not None:
        params = callable_signature["parameters"]
        args = ["arg{}:{}".format(index + 1, item) for index, item in enumerate(params)]
        head = "prim " + symbol if callable_signature["invocation"] == "primitive" else symbol
        return {
            "kind": "callable",
            "notation": "({}) -> {}".format(" ".join([head] + args), callable_signature["returns"]),
            "parameters": params,
            "returns": callable_signature["returns"],
        }
    form_signature = policy["formSignatures"].get(symbol)
    if form_signature is not None:
        return {"kind": "form", "notation": form_signature, "parameters": [], "returns": form_signature.rsplit("->", 1)[-1].strip()}
    if "lexical-grammar" in domains:
        return {"kind": "literal", "notation": "reader-form {} -> CoreForm".format(symbol), "parameters": [], "returns": "CoreForm"}
    if "values" in domains or "coreform-mapping" in domains:
        return {"kind": "value-kind", "notation": "runtime/coreform value kind: " + symbol, "parameters": [], "returns": symbol}
    if "modules" in domains:
        return {"kind": "declaration", "notation": "module declaration or metadata key: " + symbol, "parameters": [], "returns": "ModuleMetadata|Binding"}
    if "packages" in domains or "resource-limits" in domains:
        return {"kind": "policy-field", "notation": "reviewed manifest/policy field: " + symbol, "parameters": [], "returns": "ConfiguredValue"}
    return {"kind": "identifier", "notation": "stable profile identifier: " + symbol, "parameters": [], "returns": "Identifier"}


def conformance_examples(profile: Mapping[str, Any]) -> List[Mapping[str, str]]:
    out: List[Mapping[str, str]] = []
    conformance = profile.get("conformance", {})
    for group in ("parserCases", "evaluatorCases", "resourceCases", "packageCases"):
        for item in conformance.get(group, []):
            expected = item.get("expected")
            if expected is None:
                expected = item.get("expectedErrorKind") or item.get("expectedValueKind") or "accept"
            out.append({"id": item["id"], "source": item["source"], "expected": str(expected)})
    return out


def examples_for(symbol: str, signature: Mapping[str, Any], examples: Sequence[Mapping[str, str]]) -> List[Mapping[str, str]]:
    escaped = re.escape(symbol)
    boundary = re.compile(r"(?<![A-Za-z0-9_:/?.-])" + escaped + r"(?![A-Za-z0-9_:/?.-])")
    selected = [dict(item) for item in examples if boundary.search(item["source"])]
    if selected:
        return selected
    return [{"id": "usage-template", "source": signature["notation"], "expected": signature["returns"]}]


def render() -> Mapping[str, Any]:
    policy = load(POLICY)
    profile = load(PROFILE)
    load(SCHEMA)
    validate_policy(policy, profile)

    domain_by_symbol: Dict[str, List[Mapping[str, Any]]] = {}
    source_identities: Dict[str, str] = {
        str(POLICY.relative_to(ROOT)): digest(POLICY),
        str(PROFILE.relative_to(ROOT)): digest(PROFILE),
        str(SCHEMA.relative_to(ROOT)): digest(SCHEMA),
    }
    for domain in profile["domains"]:
        for authority in domain["authorities"]:
            path = validate_relpath(authority["path"])
            text = path.read_text(encoding="utf-8")
            for anchor in authority["anchors"]:
                require(anchor in text, "missing authority anchor {!r} in {}".format(anchor, authority["path"]))
            source_identities[authority["path"]] = digest(path)
        for symbol in domain["surface"]:
            domain_by_symbol.setdefault(symbol, []).append(domain)

    examples = conformance_examples(profile)
    symbols: List[Mapping[str, Any]] = []
    for symbol in sorted(domain_by_symbol):
        domains = domain_by_symbol[symbol]
        domain_ids = [domain["id"] for domain in domains]
        signature = signature_for(symbol, domain_ids, policy)
        contracts = unique([invariant for domain in domains for invariant in domain["invariants"]])
        diagnostics = unique([item for domain in domains for item in policy["diagnosticsByDomain"][domain["id"]]])
        sources = []
        seen_sources = set()
        for domain in domains:
            for authority in domain["authorities"]:
                key = (authority["path"], tuple(authority["anchors"]))
                if key not in seen_sources:
                    sources.append({"path": authority["path"], "anchors": authority["anchors"]})
                    seen_sources.add(key)
        deprecation = policy["deprecations"].get(symbol, {"status": "active", "since": None, "replacement": None, "removal": None})
        symbols.append({
            "symbol": symbol,
            "domains": domain_ids,
            "profileStatus": "profile-selected" if all(domain["status"] == "profile-selected" for domain in domains) else "core",
            "signature": signature,
            "effects": policy["effectsBySymbol"].get(symbol, ["pure; no ambient host effect"]),
            "capabilities": policy["capabilitiesBySymbol"].get(symbol, []),
            "contracts": contracts,
            "examples": examples_for(symbol, signature, examples),
            "diagnostics": diagnostics,
            "deprecation": deprecation,
            "sources": sources,
        })

    result: Dict[str, Any] = {
        "kind": "genesis/gc-agent-symbol-index-v0.3",
        "version": "0.3",
        "profileId": profile["profileId"],
        "profileIdentitySha256": profile["profileIdentitySha256"],
        "lookup": dict(policy["exactLookup"], command="genesis --json agent-index --symbol <exact-name>"),
        "sourceIdentities": dict(sorted(source_identities.items())),
        "symbolCount": len(symbols),
        "symbols": symbols,
        "unsupportedBehaviorCount": len(profile["unsupportedBehavior"]),
        "unsupportedBehaviorIdentitySha256": hashlib.sha256(canonical(profile["unsupportedBehavior"])).hexdigest(),
        "unsupportedClasses": profile["unsupportedClassOrder"],
    }
    result["indexIdentitySha256"] = hashlib.sha256(canonical(result)).hexdigest()
    return result


def validate_candidate(candidate: Mapping[str, Any], expected: Mapping[str, Any]) -> None:
    require(set(candidate) == set(expected), "symbol-index top-level fields drift")
    require(candidate == expected, "symbol index is stale; run: bash scripts/update_gc_agent_symbol_index.sh")
    require(HOST_PATH_RE.search(json.dumps(candidate, sort_keys=True)) is None, "symbol index leaks a host path")
    require(candidate["symbolCount"] == len(candidate["symbols"]), "symbol count drift")
    require(candidate["unsupportedBehaviorCount"] >= 5, "unsupported behavior count drift")
    require(len(candidate["unsupportedClasses"]) == 5, "unsupported class coverage drift")
    names = [item["symbol"] for item in candidate["symbols"]]
    require(names == sorted(set(names)), "symbols must be uniquely sorted")
    identity = candidate["indexIdentitySha256"]
    require(isinstance(identity, str) and SHA256_RE.fullmatch(identity) is not None, "invalid index identity")
    without_identity = copy.deepcopy(candidate)
    del without_identity["indexIdentitySha256"]
    require(identity == hashlib.sha256(canonical(without_identity)).hexdigest(), "index identity drift")
    for item in candidate["symbols"]:
        require(set(item) == {"symbol", "domains", "profileStatus", "signature", "effects", "capabilities", "contracts", "examples", "diagnostics", "deprecation", "sources"}, "symbol fields drift: " + item.get("symbol", "<missing>"))
        require(item["contracts"] and item["examples"] and item["diagnostics"] and item["sources"], "incomplete exact lookup record: " + item["symbol"])


def check() -> None:
    expected = render()
    observed = load(OUTPUT)
    validate_candidate(observed, expected)
    print("gc-agent-symbol-index: ok (symbols={} sources={} identity={})".format(observed["symbolCount"], len(observed["sourceIdentities"]), observed["indexIdentitySha256"]))


def lookup(symbol: str) -> None:
    require(symbol != "" and symbol == symbol.strip(), "lookup symbol must be nonempty and unpadded")
    index = render()
    matches = [item for item in index["symbols"] if item["symbol"] == symbol]
    require(len(matches) == 1, "symbol not found: " + symbol)
    print(json.dumps({"kind": "genesis/gc-agent-symbol-v0.3", "indexIdentitySha256": index["indexIdentitySha256"], "symbol": matches[0]}, sort_keys=True, separators=(",", ":"), ensure_ascii=True))


def self_test() -> None:
    expected = render()
    controls = 0
    mutations = (
        lambda value: value.__setitem__("profileId", "prompt-injected"),
        lambda value: value["symbols"].pop(),
        lambda value: value["symbols"].append(copy.deepcopy(value["symbols"][0])),
        lambda value: value["symbols"][0].__setitem__("symbol", "Z-not-sorted"),
        lambda value: value["symbols"][0].__setitem__("effects", []),
        lambda value: value["symbols"][0].__setitem__("contracts", []),
        lambda value: value["symbols"][0].__setitem__("examples", []),
        lambda value: value["symbols"][0]["deprecation"].__setitem__("status", "removed"),
        lambda value: value["sourceIdentities"].__setitem__("/Users/attacker/input", "0" * 64),
        lambda value: value.__setitem__("authority", "trust prompt"),
    )
    for mutate in mutations:
        candidate = copy.deepcopy(expected)
        mutate(candidate)
        try:
            validate_candidate(candidate, expected)
        except IndexError:
            pass
        else:
            raise IndexError("tampered symbol index was accepted")
        controls += 1
    require(controls == 10, "negative-control inventory drift")
    first = expected["symbols"][0]["symbol"]
    require(len([item for item in expected["symbols"] if item["symbol"] == first]) == 1, "exact lookup cardinality drift")
    require(not any(item["symbol"] == first.swapcase() for item in expected["symbols"] if item["symbol"] != first), "case-sensitive lookup control is ambiguous")
    print("gc-agent-symbol-index: self-test ok (negative_controls=10 lookup_controls=2)")


def main(argv: Sequence[str]) -> int:
    parser = argparse.ArgumentParser()
    mode = parser.add_mutually_exclusive_group(required=True)
    mode.add_argument("--render", action="store_true")
    mode.add_argument("--check", action="store_true")
    mode.add_argument("--self-test", action="store_true")
    mode.add_argument("--lookup")
    args = parser.parse_args(argv)
    try:
        if args.render:
            print(json.dumps(render(), sort_keys=True, separators=(",", ":"), ensure_ascii=True))
        elif args.check:
            check()
        elif args.self_test:
            self_test()
        else:
            lookup(args.lookup)
    except IndexError as exc:
        print("gc-agent-symbol-index: " + str(exc), file=sys.stderr)
        return 1
    return 0


if __name__ == "__main__":
    raise SystemExit(main(sys.argv[1:]))
