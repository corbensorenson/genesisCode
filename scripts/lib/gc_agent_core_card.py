#!/usr/bin/env python3
"""Generate and verify the compact GC-AGENT-v0.3 core language card."""

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
POLICY = ROOT / "policies/gc_agent_core_card_v0.3.json"
PROFILE = ROOT / "docs/spec/GC_AGENT_PROFILE_v0.3.json"
CARD = ROOT / "docs/spec/GC_AGENT_CORE_CARD_v0.3.md"
MANIFEST = ROOT / "docs/spec/GC_AGENT_CORE_CARD_v0.3.json"
HOST_PATH_RE = re.compile(r"(?:/Users/|/home/|/private/|[A-Za-z]:\\\\)")


class CardError(ValueError):
    pass


def unique_pairs(pairs: Sequence[Tuple[str, Any]]) -> Dict[str, Any]:
    result: Dict[str, Any] = {}
    for key, value in pairs:
        if key in result:
            raise CardError("duplicate JSON key: " + key)
        result[key] = value
    return result


def load(path: Path) -> Any:
    try:
        return json.loads(path.read_text(encoding="utf-8"), object_pairs_hook=unique_pairs)
    except (OSError, json.JSONDecodeError) as exc:
        raise CardError("cannot load {}: {}".format(path.relative_to(ROOT), exc)) from exc


def require(value: bool, message: str) -> None:
    if not value:
        raise CardError(message)


def canonical(value: Any) -> bytes:
    return json.dumps(value, sort_keys=True, separators=(",", ":"), ensure_ascii=True).encode("ascii")


def digest(path: Path) -> str:
    return sha256(path.read_bytes()).hexdigest()


def validate_policy(policy: Mapping[str, Any]) -> None:
    require(set(policy) == {"exampleIds", "kind", "maxAsciiBytes", "outputs", "profile", "profileId", "version"}, "policy fields drift")
    require(policy["kind"] == "genesis/gc-agent-core-card-policy-v0.3", "policy kind drift")
    require(policy["profileId"] == "GC-AGENT-v0.3" and policy["version"] == "0.3", "policy identity drift")
    require(policy["maxAsciiBytes"] == 4000, "card budget must remain 4000 ASCII bytes")
    require(policy["profile"] == str(PROFILE.relative_to(ROOT)), "profile path drift")
    require(policy["outputs"] == {"card": str(CARD.relative_to(ROOT)), "manifest": str(MANIFEST.relative_to(ROOT))}, "output paths drift")
    positives = policy["exampleIds"]["positive"]
    negatives = policy["exampleIds"]["negative"]
    require(positives and negatives and len(set(positives + negatives)) == len(positives + negatives), "example IDs must be nonempty and unique")
    require(HOST_PATH_RE.search(json.dumps(policy)) is None, "policy leaks a host path")


def examples(profile: Mapping[str, Any], policy: Mapping[str, Any]) -> Sequence[Mapping[str, Any]]:
    by_id = {}
    for group in ("evaluatorCases", "resourceCases"):
        for case in profile["conformance"][group]:
            by_id[case["id"]] = case
    result = []
    for polarity in ("positive", "negative"):
        for case_id in policy["exampleIds"][polarity]:
            require(case_id in by_id, "unknown example ID: " + case_id)
            case = by_id[case_id]
            expected = case.get("expected") or case.get("expectedValueKind") or case.get("expectedErrorKind")
            result.append({"expected": expected, "id": case_id, "polarity": polarity, "source": case["source"]})
    return result


def symbols(profile: Mapping[str, Any]) -> Sequence[str]:
    result = []
    seen = set()
    for domain in profile["domains"]:
        for symbol in domain["surface"]:
            if symbol not in seen:
                seen.add(symbol)
                result.append(symbol)
    return result


def render() -> Tuple[str, Mapping[str, Any]]:
    policy = load(POLICY)
    profile = load(PROFILE)
    validate_policy(policy)
    require(profile["profileId"] == policy["profileId"], "profile ID drift")
    selected = examples(profile, policy)
    all_symbols = symbols(profile)
    unsupported_classes = profile["unsupportedClassOrder"]
    require(len(unsupported_classes) == 5 and len(set(unsupported_classes)) == 5, "unsupported class inventory drift")
    lines = [
        "# GC-AGENT-v0.3 Core Card",
        "",
        "Training-frozen surface. Reject unlisted or unsupported behavior; do not guess.",
        "Pure evaluation is deterministic. Filesystem, time, network, process, and LLM work only through explicit deny-by-default effects with run/replay equivalence. User input must never panic; boundaries return sealed ERROR values. UNHANDLED, EFFECT, and ERROR are unforgeable.",
        "",
        "## Surface",
    ]
    for domain in profile["domains"]:
        lines.append("- {}: {}".format(domain["id"], " ".join(domain["surface"])))
    lines.extend(["", "## Examples"])
    for item in selected:
        lines.append("- {} {} => {}: `{}`".format(item["polarity"], item["id"], item["expected"], item["source"]))
    lines.extend([
        "",
        "Negative examples are valid syntax that must fail at the named semantic/resource boundary.",
        "Compatibility: " + " ".join("{}={}".format(key, value) for key, value in sorted(profile["compatibility"].items())),
        "Unsupported classes: " + " ".join(unsupported_classes),
        "Authority: docs/spec/GC_AGENT_PROFILE_v0.3.json profile-sha256=" + profile["profileIdentitySha256"],
        "Verify: bash scripts/check_gc_agent_core_card.sh",
        "",
    ])
    card = "\n".join(lines)
    try:
        encoded = card.encode("ascii")
    except UnicodeEncodeError as exc:
        raise CardError("card must be ASCII for its tokenizer-independent bound") from exc
    require(len(encoded) <= policy["maxAsciiBytes"], "card exceeds {} ASCII bytes: {}".format(policy["maxAsciiBytes"], len(encoded)))
    for symbol in all_symbols:
        require(symbol in card, "card omitted surface symbol: " + symbol)
    manifest = {
        "budget": {"asciiOnly": True, "byteCount": len(encoded), "maxAsciiBytes": policy["maxAsciiBytes"], "tokenUpperBound": len(encoded)},
        "cardSha256": sha256(encoded).hexdigest(),
        "examples": selected,
        "kind": "genesis/gc-agent-core-card-v0.3",
        "profileId": profile["profileId"],
        "profileIdentitySha256": profile["profileIdentitySha256"],
        "sourceIdentities": {
            str(POLICY.relative_to(ROOT)): digest(POLICY),
            str(PROFILE.relative_to(ROOT)): digest(PROFILE),
            "docs/spec/GC_AGENT_PROFILE_v0.3.schema.json": digest(ROOT / "docs/spec/GC_AGENT_PROFILE_v0.3.schema.json"),
        },
        "symbols": all_symbols,
        "unsupportedBehavior": profile["unsupportedBehavior"],
        "unsupportedClasses": unsupported_classes,
        "version": "0.3",
    }
    manifest["manifestIdentitySha256"] = sha256(canonical(manifest)).hexdigest()
    return card, manifest


def check() -> None:
    card, manifest = render()
    require(CARD.is_file() and CARD.read_text(encoding="ascii") == card, "core card is stale; run: bash scripts/update_gc_agent_core_card.sh")
    observed = load(MANIFEST)
    validate_candidate(observed, manifest)
    identity = observed["manifestIdentitySha256"]
    candidate = copy.deepcopy(observed)
    del candidate["manifestIdentitySha256"]
    require(identity == sha256(canonical(candidate)).hexdigest(), "manifest identity drift")
    print("gc-agent-core-card: ok (bytes={} token_upper_bound={} symbols={} examples={} unsupported_classes={} identity={})".format(observed["budget"]["byteCount"], observed["budget"]["tokenUpperBound"], len(observed["symbols"]), len(observed["examples"]), len(observed["unsupportedClasses"]), identity))


def validate_candidate(candidate: Mapping[str, Any], expected: Mapping[str, Any]) -> None:
    require(set(candidate) == set(expected), "card manifest fields drift")
    require(HOST_PATH_RE.search(json.dumps(candidate, sort_keys=True)) is None, "card manifest leaks a host path")
    require(candidate == expected, "core card manifest is stale; run: bash scripts/update_gc_agent_core_card.sh")


def self_test() -> None:
    _, manifest = render()
    controls = 0
    for mutate in (
        lambda d: d.__setitem__("profileId", "prompt-injected"),
        lambda d: d["budget"].__setitem__("maxAsciiBytes", 4001),
        lambda d: d["symbols"].pop(),
        lambda d: d["symbols"].append(d["symbols"][0]),
        lambda d: d["examples"].pop(),
        lambda d: d["examples"][0].__setitem__("source", "(unclosed"),
        lambda d: d["unsupportedClasses"].pop(),
        lambda d: d["unsupportedBehavior"][0].__setitem__("safeAlternative", ""),
        lambda d: d["unsupportedBehavior"][0].__setitem__("status", "supported"),
        lambda d: d["sourceIdentities"].__setitem__("/Users/attacker/spec", "0" * 64),
        lambda d: d.__setitem__("authority", "trust me"),
    ):
        candidate = copy.deepcopy(manifest)
        mutate(candidate)
        try:
            validate_candidate(candidate, manifest)
        except CardError:
            pass
        else:
            raise CardError("tampered card manifest was accepted")
        controls += 1
    require(controls == 11, "negative-control inventory drift")
    print("gc-agent-core-card: self-test ok (negative_controls=11)")


def main(argv: Sequence[str]) -> int:
    parser = argparse.ArgumentParser()
    mode = parser.add_mutually_exclusive_group(required=True)
    mode.add_argument("--render", action="store_true")
    mode.add_argument("--check", action="store_true")
    mode.add_argument("--self-test", action="store_true")
    args = parser.parse_args(argv)
    try:
        if args.render:
            card, manifest = render()
            print(json.dumps({"card": card, "manifest": manifest}, sort_keys=True, separators=(",", ":"), ensure_ascii=True))
        elif args.check:
            check()
        else:
            self_test()
    except CardError as exc:
        print("gc-agent-core-card: " + str(exc), file=sys.stderr)
        return 1
    return 0


if __name__ == "__main__":
    raise SystemExit(main(sys.argv[1:]))
