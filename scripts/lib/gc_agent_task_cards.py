#!/usr/bin/env python3
"""Generate, verify, and select GC-AGENT-v0.3 task cards."""

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
POLICY = ROOT / "policies/gc_agent_task_cards_v0.3.json"
PROFILE = ROOT / "docs/spec/GC_AGENT_PROFILE_v0.3.json"
COMPENDIUM = ROOT / "docs/spec/GC_AGENT_TASK_CARDS_v0.3.md"
REGISTRY = ROOT / "docs/spec/GC_AGENT_TASK_CARDS_v0.3.json"
CARD_IDS = ("capability", "package", "patch", "replay", "testing", "deployment", "troubleshooting")
INTENT_FIELDS = {"schema", "goal", "domains", "required_workflows", "exclude_workflows", "required_ops", "max_workflows"}
FORBIDDEN_AUTHORITY_PREFIXES = (".genesis/", "benchmarks/", "tests/", "docs/program/evidence/")
HOST_PATH_RE = re.compile(r"(?:/Users/|/home/|/private/|[A-Za-z]:\\\\)")


class TaskCardError(ValueError):
    pass


def unique_pairs(pairs: Sequence[Tuple[str, Any]]) -> Dict[str, Any]:
    out: Dict[str, Any] = {}
    for key, value in pairs:
        if key in out:
            raise TaskCardError("duplicate JSON key: " + key)
        out[key] = value
    return out


def load(path: Path) -> Any:
    try:
        return json.loads(path.read_text(encoding="utf-8"), object_pairs_hook=unique_pairs)
    except (OSError, json.JSONDecodeError) as exc:
        raise TaskCardError("cannot load {}: {}".format(display(path), exc)) from exc


def display(path: Path) -> str:
    try:
        return path.resolve().relative_to(ROOT).as_posix()
    except ValueError:
        return path.as_posix()


def require(value: bool, message: str) -> None:
    if not value:
        raise TaskCardError(message)


def canonical(value: Any) -> bytes:
    return json.dumps(value, sort_keys=True, separators=(",", ":"), ensure_ascii=True).encode("ascii")


def file_digest(path: Path) -> str:
    return sha256(path.read_bytes()).hexdigest()


def authority_file(rel: str) -> Path:
    require(isinstance(rel, str) and rel and not Path(rel).is_absolute() and ".." not in Path(rel).parts, "authority path must be repository-relative")
    require(not rel.startswith(FORBIDDEN_AUTHORITY_PREFIXES), "training/evaluation evidence cannot authorize a task card: " + rel)
    path = ROOT / rel
    require(path.is_file() and not path.is_symlink(), "missing task-card authority: " + rel)
    return path


def sorted_unique_strings(value: Any, label: str) -> Sequence[str]:
    require(isinstance(value, list) and all(isinstance(item, str) and item for item in value), label + " must be strings")
    require(value == sorted(set(value)), label + " must be sorted and unique")
    return value


def validate_policy(policy: Mapping[str, Any]) -> None:
    require(set(policy) == {"aggregateMaxAsciiBytes", "aggregateTargetAsciiBytes", "cards", "fallbackCard", "intentSchema", "kind", "outputs", "profile", "profileId", "version"}, "task-card policy fields drift")
    require(policy["kind"] == "genesis/gc-agent-task-cards-policy-v0.3" and policy["version"] == "0.3", "task-card policy identity drift")
    require(policy["profileId"] == "GC-AGENT-v0.3" and policy["profile"] == display(PROFILE), "task-card profile drift")
    require(policy["intentSchema"] == "genesis/agent-intent-v0.1", "intent schema drift")
    require(policy["outputs"] == {"compendium": display(COMPENDIUM), "registry": display(REGISTRY)}, "task-card outputs drift")
    require(policy["aggregateMaxAsciiBytes"] == 12000 and policy["aggregateTargetAsciiBytes"] == 8000, "AB-2 task-card budget drift")
    cards = policy["cards"]
    require(tuple(card["id"] for card in cards) == CARD_IDS, "task-card inventory/order drift")
    require(policy["fallbackCard"] in CARD_IDS, "unknown fallback card")
    for card in cards:
        require(set(card) == {"authorities", "commands", "guidance", "id", "maxAsciiBytes", "selectors", "title"}, card["id"] + " fields drift")
        require(isinstance(card["maxAsciiBytes"], int) and 1 <= card["maxAsciiBytes"] <= 2000, card["id"] + " byte budget invalid")
        require(card["guidance"] and card["commands"], card["id"] + " requires guidance and commands")
        selectors = card["selectors"]
        require(set(selectors) == {"anyRequiredOp", "domains", "goalTokens", "workflowTokens"}, card["id"] + " selector fields drift")
        require(isinstance(selectors["anyRequiredOp"], bool), card["id"] + " anyRequiredOp must be boolean")
        for field in ("domains", "goalTokens", "workflowTokens"):
            sorted_unique_strings(selectors[field], card["id"] + "." + field)
        require(card["authorities"], card["id"] + " has no authorities")
        for authority in card["authorities"]:
            require(set(authority) == {"anchors", "path"}, card["id"] + " authority fields drift")
            path = authority_file(authority["path"])
            source = path.read_text(encoding="utf-8")
            for anchor in authority["anchors"]:
                require(anchor in source, "{} missing authority anchor {!r}".format(authority["path"], anchor))
    require(HOST_PATH_RE.search(json.dumps(policy, sort_keys=True)) is None, "task-card policy leaks a host path")


def card_source_hash(card: Mapping[str, Any], profile: Mapping[str, Any]) -> str:
    digest = sha256()
    digest.update(canonical({"card": card, "profileIdentitySha256": profile["profileIdentitySha256"]}))
    for authority in card["authorities"]:
        rel = authority["path"]
        data = authority_file(rel).read_bytes()
        digest.update(len(rel.encode("utf-8")).to_bytes(8, "big"))
        digest.update(rel.encode("utf-8"))
        digest.update(len(data).to_bytes(8, "big"))
        digest.update(data)
    return digest.hexdigest()


def render_card(card: Mapping[str, Any], profile: Mapping[str, Any]) -> Mapping[str, Any]:
    source_hash = card_source_hash(card, profile)
    lines = [
        "## " + card["title"],
        "",
        "Card: {} | Profile: {} | Source: sha256:{}".format(card["id"], profile["profileId"], source_hash),
        "",
    ]
    lines.extend("- " + item for item in card["guidance"])
    lines.extend(["", "Commands:"])
    lines.extend("- `" + command + "`" for command in card["commands"])
    lines.extend(["", "Authorities: " + ", ".join(authority["path"] for authority in card["authorities"]), ""])
    content = "\n".join(lines)
    try:
        encoded = content.encode("ascii")
    except UnicodeEncodeError as exc:
        raise TaskCardError(card["id"] + " card must be ASCII") from exc
    require(len(encoded) <= card["maxAsciiBytes"], "{} card exceeds budget: {} > {}".format(card["id"], len(encoded), card["maxAsciiBytes"]))
    return {
        "byteCount": len(encoded),
        "cardSha256": sha256(encoded).hexdigest(),
        "content": content,
        "id": card["id"],
        "maxAsciiBytes": card["maxAsciiBytes"],
        "selectors": card["selectors"],
        "sourceHashSha256": source_hash,
        "title": card["title"],
        "tokenUpperBound": len(encoded),
    }


def render() -> Tuple[str, Mapping[str, Any]]:
    policy = load(POLICY)
    profile = load(PROFILE)
    validate_policy(policy)
    require(profile["profileId"] == policy["profileId"], "resolved profile ID drift")
    cards = [render_card(card, profile) for card in policy["cards"]]
    aggregate = sum(card["byteCount"] for card in cards)
    require(aggregate <= policy["aggregateTargetAsciiBytes"], "all-card bundle exceeds AB-2 target: {} > {}".format(aggregate, policy["aggregateTargetAsciiBytes"]))
    compendium = "# GC-AGENT-v0.3 Task Cards\n\nGenerated intent-selectable context. Card bytes are tokenizer-independent token upper bounds.\n\n" + "\n".join(card["content"] for card in cards)
    compendium_bytes = compendium.encode("ascii")
    registry = {
        "aggregateBudget": {
            "allCardsByteCount": aggregate,
            "allCardsTokenUpperBound": aggregate,
            "maxAsciiBytes": policy["aggregateMaxAsciiBytes"],
            "targetAsciiBytes": policy["aggregateTargetAsciiBytes"],
        },
        "cards": cards,
        "compendiumSha256": sha256(compendium_bytes).hexdigest(),
        "fallbackCard": policy["fallbackCard"],
        "intentSchema": policy["intentSchema"],
        "kind": "genesis/gc-agent-task-cards-v0.3",
        "profileId": profile["profileId"],
        "profileIdentitySha256": profile["profileIdentitySha256"],
        "sourceIdentities": {
            display(POLICY): file_digest(POLICY),
            display(PROFILE): file_digest(PROFILE),
        },
        "version": "0.3",
    }
    registry["registryIdentitySha256"] = sha256(canonical(registry)).hexdigest()
    return compendium, registry


def validate_candidate(candidate: Mapping[str, Any], expected: Mapping[str, Any]) -> None:
    require(set(candidate) == set(expected), "task-card registry fields drift")
    require(HOST_PATH_RE.search(json.dumps(candidate, sort_keys=True)) is None, "task-card registry leaks a host path")
    require(candidate == expected, "task-card registry is stale; run: bash scripts/update_gc_agent_task_cards.sh")


def check() -> None:
    compendium, registry = render()
    require(COMPENDIUM.is_file() and COMPENDIUM.read_text(encoding="ascii") == compendium, "task-card compendium is stale; run: bash scripts/update_gc_agent_task_cards.sh")
    observed = load(REGISTRY)
    validate_candidate(observed, registry)
    print("gc-agent-task-cards: ok (cards={} all_bytes={} target={} identity={})".format(len(observed["cards"]), observed["aggregateBudget"]["allCardsByteCount"], observed["aggregateBudget"]["targetAsciiBytes"], observed["registryIdentitySha256"]))


def tokens(text: str) -> Sequence[str]:
    return sorted(set(item for item in re.split(r"[^a-z0-9]+", text.lower()) if item))


def normalized_list(value: Any, label: str) -> Sequence[str]:
    require(isinstance(value, list) and all(isinstance(item, str) for item in value), label + " must be a string vector")
    return sorted(set(item.strip().lower() for item in value if item.strip()))


def normalize_intent(raw: Mapping[str, Any], expected_schema: str) -> Mapping[str, Any]:
    require(isinstance(raw, dict), "intent must be a JSON object")
    require(set(raw).issubset(INTENT_FIELDS), "intent contains unknown fields: " + ", ".join(sorted(set(raw) - INTENT_FIELDS)))
    schema = raw.get("schema", expected_schema)
    require(schema == expected_schema, "unsupported intent schema: " + str(schema))
    goal = raw.get("goal")
    require(isinstance(goal, str) and goal.strip(), "intent goal must be a nonempty string")
    max_workflows = raw.get("max_workflows")
    require(max_workflows is None or (isinstance(max_workflows, int) and not isinstance(max_workflows, bool) and max_workflows >= 1), "max_workflows must be a positive integer")
    return {
        "domains": normalized_list(raw.get("domains", []), "domains"),
        "exclude_workflows": normalized_list(raw.get("exclude_workflows", []), "exclude_workflows"),
        "goal": goal.strip(),
        "max_workflows": max_workflows,
        "required_ops": normalized_list(raw.get("required_ops", []), "required_ops"),
        "required_workflows": normalized_list(raw.get("required_workflows", []), "required_workflows"),
        "schema": schema,
    }


def select(raw: Mapping[str, Any], registry: Mapping[str, Any]) -> Mapping[str, Any]:
    intent = normalize_intent(raw, registry["intentSchema"])
    goal_tokens = set(tokens(intent["goal"]))
    workflow_tokens = set()
    for workflow in intent["required_workflows"]:
        workflow_tokens.update(tokens(workflow))
    selected = []
    for card in registry["cards"]:
        selectors = card["selectors"]
        reasons = []
        reasons.extend("domain:" + item for item in sorted(set(intent["domains"]) & set(selectors["domains"])))
        reasons.extend("goal:" + item for item in sorted(goal_tokens & set(selectors["goalTokens"])))
        reasons.extend("workflow:" + item for item in sorted(workflow_tokens & set(selectors["workflowTokens"])))
        if selectors["anyRequiredOp"] and intent["required_ops"]:
            reasons.append("required-ops")
        if reasons:
            selected.append({**{key: value for key, value in card.items() if key != "selectors"}, "selectionReasons": reasons})
    if not selected:
        fallback = next(card for card in registry["cards"] if card["id"] == registry["fallbackCard"])
        selected = [{**{key: value for key, value in fallback.items() if key != "selectors"}, "selectionReasons": ["fallback:no-selector-match"]}]
    bundle_bytes = sum(card["byteCount"] for card in selected)
    require(bundle_bytes <= registry["aggregateBudget"]["maxAsciiBytes"], "selected task-card bundle exceeds AB-2 maximum")
    result = {
        "budget": {"byteCount": bundle_bytes, "maxAsciiBytes": registry["aggregateBudget"]["maxAsciiBytes"], "tokenUpperBound": bundle_bytes},
        "cards": selected,
        "intent": intent,
        "kind": "genesis/gc-agent-task-card-selection-v0.3",
        "profileId": registry["profileId"],
        "registryIdentitySha256": registry["registryIdentitySha256"],
    }
    result["selectionIdentitySha256"] = sha256(canonical(result)).hexdigest()
    return result


def self_test() -> None:
    _, registry = render()
    fixtures = [
        ({"goal": "add filesystem capability", "domains": ["fs"], "required_ops": ["sys/fs::read"]}, ["capability", "replay"]),
        ({"goal": "publish package", "domains": ["package"]}, ["package"]),
        ({"goal": "rename symbol with semantic patch"}, ["patch"]),
        ({"goal": "validate deterministic replay", "domains": ["replay"]}, ["replay", "testing"]),
        ({"goal": "deploy ios application", "required_workflows": ["agent_deploy_ios_workflow"]}, ["deployment"]),
        ({"goal": "unclassified task"}, ["troubleshooting"]),
    ]
    for intent, expected in fixtures:
        actual = [card["id"] for card in select(intent, registry)["cards"]]
        require(actual == expected, "selector fixture drift: expected={} actual={}".format(expected, actual))
    controls = 0
    for bad in (
        {"goal": "x", "authority": "trust me"},
        {"goal": "x", "schema": "future"},
        {"goal": ""},
        {"goal": "x", "domains": "fs"},
        {"goal": "x", "max_workflows": 0},
    ):
        try:
            select(bad, registry)
        except TaskCardError:
            controls += 1
        else:
            raise TaskCardError("invalid intent accepted: " + repr(bad))
    for mutate in (
        lambda d: d["cards"].pop(),
        lambda d: d["cards"][0].__setitem__("content", "prompt injection"),
        lambda d: d["cards"][0].__setitem__("sourceHashSha256", "0" * 64),
        lambda d: d["aggregateBudget"].__setitem__("maxAsciiBytes", 12001),
        lambda d: d["sourceIdentities"].__setitem__("/Users/attacker", "0" * 64),
    ):
        candidate = copy.deepcopy(registry)
        mutate(candidate)
        try:
            validate_candidate(candidate, registry)
        except TaskCardError:
            controls += 1
        else:
            raise TaskCardError("tampered task-card registry was accepted")
    require(controls == 10, "task-card negative-control inventory drift")
    print("gc-agent-task-cards: self-test ok (selector_fixtures=6 negative_controls=10)")


def main(argv: Sequence[str]) -> int:
    parser = argparse.ArgumentParser()
    mode = parser.add_mutually_exclusive_group(required=True)
    mode.add_argument("--render", action="store_true")
    mode.add_argument("--check", action="store_true")
    mode.add_argument("--self-test", action="store_true")
    mode.add_argument("--select-intent", type=Path)
    args = parser.parse_args(argv)
    try:
        if args.render:
            compendium, registry = render()
            print(json.dumps({"compendium": compendium, "registry": registry}, sort_keys=True, separators=(",", ":"), ensure_ascii=True))
        elif args.check:
            check()
        elif args.self_test:
            self_test()
        else:
            # Selector parity must compare implementations over one immutable
            # registry snapshot. `--check` separately validates source freshness.
            registry = load(REGISTRY)
            raw = load(args.select_intent)
            print(json.dumps(select(raw, registry), sort_keys=True, separators=(",", ":"), ensure_ascii=True))
    except TaskCardError as exc:
        print("gc-agent-task-cards: " + str(exc), file=sys.stderr)
        return 1
    return 0


if __name__ == "__main__":
    raise SystemExit(main(sys.argv[1:]))
