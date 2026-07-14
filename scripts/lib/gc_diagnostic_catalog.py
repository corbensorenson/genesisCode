#!/usr/bin/env python3
"""Generate and validate the closed GenesisCode CLI diagnostic catalog."""

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
POLICY = ROOT / "policies/gc_diagnostic_catalog_v0.1.json"
SCHEMA = ROOT / "docs/spec/GC_DIAGNOSTIC_CATALOG_v0.1.schema.json"
REPAIR_SCHEMA = ROOT / "docs/spec/GC_DIAGNOSTIC_REPAIR_PLAN_v0.1.schema.json"
OUTPUT = ROOT / "docs/spec/GC_DIAGNOSTIC_CATALOG_v0.1.json"
CODE_RE = re.compile(r"^[a-z][a-z0-9-]*/[a-z][a-z0-9-]*$")
SHA256_RE = re.compile(r"^[0-9a-f]{64}$")
CALL_RE = re.compile(
    r'(?:cli_err(?:_anyhow|_with_context)?\(\s*[^,\n]+,\s*|session_error\(\s*)"([^"]+)"',
    re.MULTILINE,
)
FIELD_RE = re.compile(r'\bcode:\s*"([^"]+)"')
HOST_PATH_RE = re.compile(r"(?:/Users/|/home/|[A-Za-z]:\\\\)")


class CatalogError(RuntimeError):
    pass


def require(condition: bool, message: str) -> None:
    if not condition:
        raise CatalogError(message)


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
        raise CatalogError("invalid JSON {}: {}".format(path.relative_to(ROOT), exc)) from exc
    require(isinstance(value, dict), "top-level JSON must be an object: " + str(path.relative_to(ROOT)))
    return value


def canonical(value: Any) -> bytes:
    return json.dumps(value, sort_keys=True, separators=(",", ":"), ensure_ascii=True).encode("ascii")


def digest(path: Path) -> str:
    return hashlib.sha256(path.read_bytes()).hexdigest()


def source_files(policy: Mapping[str, Any]) -> List[Path]:
    files: List[Path] = []
    for raw in policy["sourceRoots"]:
        require(isinstance(raw, str) and raw and not Path(raw).is_absolute(), "source root must be repository-relative")
        root = ROOT / raw
        require(root.is_dir() and not root.is_symlink(), "source root missing or symlinked: " + raw)
        files.extend(sorted(path for path in root.rglob("*.rs") if path.is_file() and not path.is_symlink()))
    require(files, "diagnostic source inventory is empty")
    return sorted(set(files))


def discover_codes(files: Sequence[Path]) -> Tuple[Dict[str, List[str]], Dict[str, str]]:
    authorities: Dict[str, List[str]] = {}
    source_text: Dict[str, str] = {}
    for path in files:
        rel = str(path.relative_to(ROOT))
        text = path.read_text(encoding="utf-8")
        source_text[rel] = text
        candidates = list(CALL_RE.finditer(text)) + list(FIELD_RE.finditer(text))
        for match in candidates:
            code = match.group(1)
            if not CODE_RE.fullmatch(code):
                continue
            line = text.count("\n", 0, match.start(1)) + 1
            authorities.setdefault(code, []).append("{}:{}".format(rel, line))
    return authorities, source_text


def validate_policy(policy: Mapping[str, Any], source_text: Mapping[str, str]) -> Dict[str, Mapping[str, Any]]:
    expected = {
        "kind", "version", "schema", "repairPlanSchema", "output", "sourceRoots",
        "exactLookup", "repairGuardrails", "extraCodes", "commonParameters", "phases",
    }
    require(set(policy) == expected, "diagnostic policy fields drift")
    require(policy["kind"] == "genesis/diagnostic-catalog-policy-v0.1", "policy kind drift")
    require(policy["version"] == "0.1.0", "policy version drift")
    require(policy["schema"] == str(SCHEMA.relative_to(ROOT)), "policy schema drift")
    require(policy["repairPlanSchema"] == str(REPAIR_SCHEMA.relative_to(ROOT)), "repair-plan schema drift")
    require(policy["output"] == str(OUTPUT.relative_to(ROOT)), "policy output drift")
    require(policy["exactLookup"] == {"caseSensitive": True, "normalization": "none", "maxResults": 1}, "exact lookup must be bounded and unnormalized")
    require(
        policy["repairGuardrails"] == {
            "schema": "genesis/diagnostic-repair-guardrails-v0.1",
            "promptAuthority": "none",
            "policyBroadening": "separate-reviewed-diff",
            "obligationSuppression": "forbidden",
            "automaticActionPolicy": "catalog-eligible-only",
        },
        "repair guardrails drift",
    )

    parameters = policy["commonParameters"]
    require(isinstance(parameters, list) and parameters, "common parameters missing")
    parameter_names = []
    for parameter in parameters:
        require(set(parameter) == {"name", "type", "required", "description"}, "common parameter fields drift")
        require(isinstance(parameter["name"], str) and parameter["name"], "parameter name missing")
        require(parameter["type"] in {"string", "integer", "number", "boolean", "object", "array", "null"}, "parameter type invalid")
        require(isinstance(parameter["required"], bool) and isinstance(parameter["description"], str) and parameter["description"], "parameter contract invalid")
        parameter_names.append(parameter["name"])
    require(parameter_names == sorted(set(parameter_names)), "common parameters must be uniquely sorted")

    phase_by_family: Dict[str, Mapping[str, Any]] = {}
    phase_ids: List[str] = []
    for phase in policy["phases"]:
        require(set(phase) == {"id", "families", "likelyCauses", "safeRepairActions", "documentation"}, "phase fields drift")
        phase_id = phase["id"]
        require(isinstance(phase_id, str) and re.fullmatch(r"[a-z][a-z0-9-]*", phase_id) is not None, "invalid phase ID")
        phase_ids.append(phase_id)
        families = phase["families"]
        require(isinstance(families, list) and families == sorted(set(families)), "phase families must be nonempty, unique, and sorted: " + phase_id)
        for family in families:
            require(family not in phase_by_family, "diagnostic family assigned to multiple phases: " + family)
            phase_by_family[family] = phase
        require(isinstance(phase["likelyCauses"], list) and phase["likelyCauses"] and all(isinstance(item, str) and item for item in phase["likelyCauses"]), "phase likely causes missing: " + phase_id)
        repairs = phase["safeRepairActions"]
        require(isinstance(repairs, list) and repairs, "phase repair actions missing: " + phase_id)
        for repair in repairs:
            require(set(repair).issubset({"id", "description", "policyEffect", "command"}) and {"id", "description", "policyEffect"}.issubset(repair), "repair fields drift: " + phase_id)
            require(repair["policyEffect"] in {"none", "review-required"}, "repair policy effect invalid: " + phase_id)
        docs = phase["documentation"]
        require(isinstance(docs, list) and docs, "phase documentation missing: " + phase_id)
        for doc in docs:
            require(set(doc) == {"path", "anchor"}, "documentation fields drift: " + phase_id)
            path = ROOT / doc["path"]
            require(path.is_file() and not path.is_symlink(), "documentation path missing or symlinked: " + doc["path"])
            require(doc["anchor"] in path.read_text(encoding="utf-8"), "documentation anchor missing: {}#{}".format(doc["path"], doc["anchor"]))
    require(phase_ids == list(dict.fromkeys(phase_ids)), "duplicate phase ID")

    extras = policy["extraCodes"]
    require(isinstance(extras, list) and extras == sorted(set(extras)), "extra codes must be uniquely sorted")
    joined_source = "\n".join(source_text.values())
    for code in extras:
        require(CODE_RE.fullmatch(code) is not None, "invalid extra diagnostic code: " + str(code))
        require(code in joined_source or code == "diagnostic/catalog-miss", "extra diagnostic code has no production authority: " + code)
    return phase_by_family


def enrich_repair(repair: Mapping[str, Any], phase_id: str) -> Mapping[str, Any]:
    repair_id = repair["id"]
    policy_review = repair["policyEffect"] == "review-required"
    if policy_review:
        kind = "policy-review"
    elif repair_id == "canonicalize-input":
        kind = "source-patch"
    elif repair_id == "regenerate-replay-log":
        kind = "effect-command"
    elif "command" in repair:
        kind = "verify-command"
    else:
        kind = "inspect"
    requires_review = policy_review or kind in {"source-patch", "effect-command"}
    obligation_effect = (
        "rerun-required"
        if phase_id in {"canonicalization", "obligation", "package", "parse", "runtime-stage", "semantic-edit"}
        else "preserve"
    )
    preconditions = [
        "The diagnostic ID and catalog identity still match the current failure.",
        "The target content identity and active policy are unchanged since diagnosis.",
    ]
    if policy_review:
        preconditions.append("Any capability broadening is represented by a separate explicit policy diff.")
    postconditions = [
        "The original failing command is rerun against the repaired content.",
        "All declared obligations remain enabled and are rerun when required.",
    ]
    return {
        **repair,
        "kind": kind,
        "obligationEffect": obligation_effect,
        "automaticEligible": not requires_review,
        "requiresReview": requires_review,
        "preconditions": preconditions,
        "postconditions": postconditions,
    }


def render() -> Mapping[str, Any]:
    policy = load(POLICY)
    load(SCHEMA)
    load(REPAIR_SCHEMA)
    files = source_files(policy)
    authorities, source_text = discover_codes(files)
    phase_by_family = validate_policy(policy, source_text)

    for code in policy["extraCodes"]:
        if code not in authorities:
            locations = []
            for rel, text in source_text.items():
                offset = text.find('"{}"'.format(code))
                if offset >= 0:
                    locations.append("{}:{}".format(rel, text.count("\n", 0, offset) + 1))
            authorities[code] = locations or ["crates/gc_cli_driver/src/diagnostics.rs:1"]

    diagnostics: List[Mapping[str, Any]] = []
    for code in sorted(authorities):
        family = code.split("/", 1)[0]
        require(family in phase_by_family, "uncataloged diagnostic family `{}` for code `{}`".format(family, code))
        phase = phase_by_family[family]
        diagnostics.append({
            "id": "genesis/diagnostic/v1/" + code,
            "code": code,
            "version": 1,
            "severity": "error",
            "phase": phase["id"],
            "primarySpan": {"field": "primary_span", "required": False, "availability": "nullable-until-localized"},
            "relatedSpans": {"field": "related_spans", "required": True, "availability": "zero-or-more"},
            "parameters": policy["commonParameters"],
            "likelyCauses": phase["likelyCauses"],
            "safeRepairActions": [enrich_repair(repair, phase["id"]) for repair in phase["safeRepairActions"]],
            "documentation": phase["documentation"],
            "sourceAuthorities": sorted(set(authorities[code])),
        })

    source_identities = {
        str(POLICY.relative_to(ROOT)): digest(POLICY),
        str(SCHEMA.relative_to(ROOT)): digest(SCHEMA),
        str(REPAIR_SCHEMA.relative_to(ROOT)): digest(REPAIR_SCHEMA),
    }
    for path in files:
        source_identities[str(path.relative_to(ROOT))] = digest(path)
    for phase in policy["phases"]:
        for doc in phase["documentation"]:
            source_identities[doc["path"]] = digest(ROOT / doc["path"])

    result: Dict[str, Any] = {
        "kind": "genesis/diagnostic-catalog-v0.1",
        "version": "0.1.0",
        "lookup": dict(policy["exactLookup"], command="genesis --json agent-index --diagnostic <exact-code>"),
        "repairPlanSchema": policy["repairPlanSchema"],
        "repairGuardrails": policy["repairGuardrails"],
        "sourceIdentities": dict(sorted(source_identities.items())),
        "spanSchema": {
            "fields": ["source", "startLine", "startColumn", "endLine", "endColumn"],
            "required": ["source", "startLine", "startColumn", "endLine", "endColumn"],
        },
        "diagnosticCount": len(diagnostics),
        "diagnostics": diagnostics,
    }
    result["catalogIdentitySha256"] = hashlib.sha256(canonical(result)).hexdigest()
    return result


def validate_candidate(candidate: Mapping[str, Any], expected: Mapping[str, Any]) -> None:
    require(candidate == expected, "diagnostic catalog is stale; run: bash scripts/update_gc_diagnostic_catalog.sh")
    require(HOST_PATH_RE.search(json.dumps(candidate, sort_keys=True)) is None, "diagnostic catalog leaks a host path")
    require(candidate["diagnosticCount"] == len(candidate["diagnostics"]), "diagnostic count drift")
    codes = [item["code"] for item in candidate["diagnostics"]]
    ids = [item["id"] for item in candidate["diagnostics"]]
    require(codes == sorted(set(codes)), "diagnostic codes must be uniquely sorted")
    require(len(ids) == len(set(ids)), "diagnostic IDs must be unique")
    require("diagnostic/catalog-miss" in codes and "error/unknown" in codes, "closed-catalog fallback diagnostics missing")
    identity = candidate["catalogIdentitySha256"]
    require(isinstance(identity, str) and SHA256_RE.fullmatch(identity) is not None, "catalog identity invalid")
    without_identity = copy.deepcopy(candidate)
    del without_identity["catalogIdentitySha256"]
    require(identity == hashlib.sha256(canonical(without_identity)).hexdigest(), "catalog identity drift")
    for item in candidate["diagnostics"]:
        require(CODE_RE.fullmatch(item["code"]) is not None, "invalid diagnostic code in catalog")
        require(item["id"] == "genesis/diagnostic/v1/" + item["code"], "diagnostic ID/code drift")
        require(item["severity"] == "error" and item["version"] == 1, "diagnostic version/severity drift")
        require(item["parameters"] and item["likelyCauses"] and item["safeRepairActions"] and item["documentation"] and item["sourceAuthorities"], "incomplete diagnostic: " + item["code"])
        for repair in item["safeRepairActions"]:
            require(repair["obligationEffect"] != "suppress", "repair suppresses an obligation")
            require(not (repair["policyEffect"] == "review-required" and repair["automaticEligible"]), "policy-changing repair is automatic")
            require(not (repair["requiresReview"] and repair["automaticEligible"]), "review-gated repair is automatic")
            require(len(repair["preconditions"]) >= 2 and len(repair["postconditions"]) >= 2, "repair conditions incomplete")


def check() -> None:
    expected = render()
    observed = load(OUTPUT)
    validate_candidate(observed, expected)
    print("gc-diagnostic-catalog: ok (diagnostics={} sources={} identity={})".format(observed["diagnosticCount"], len(observed["sourceIdentities"]), observed["catalogIdentitySha256"]))


def lookup(code: str) -> None:
    require(CODE_RE.fullmatch(code) is not None, "lookup code must be exact and unpadded")
    catalog = render()
    matches = [item for item in catalog["diagnostics"] if item["code"] == code]
    require(len(matches) == 1, "diagnostic not found: " + code)
    print(json.dumps({"kind": "genesis/diagnostic-v0.1", "catalogIdentitySha256": catalog["catalogIdentitySha256"], "diagnostic": matches[0]}, sort_keys=True, separators=(",", ":"), ensure_ascii=True))


def self_test() -> None:
    expected = render()
    mutations = (
        lambda value: value.__setitem__("version", "prompt-injected"),
        lambda value: value["diagnostics"].pop(),
        lambda value: value["diagnostics"].append(copy.deepcopy(value["diagnostics"][0])),
        lambda value: value["diagnostics"][0].__setitem__("id", "unstable"),
        lambda value: value["diagnostics"][0].__setitem__("phase", ""),
        lambda value: value["diagnostics"][0].__setitem__("parameters", []),
        lambda value: value["diagnostics"][0].__setitem__("likelyCauses", []),
        lambda value: value["diagnostics"][0].__setitem__("safeRepairActions", []),
        lambda value: value["diagnostics"][0].__setitem__("documentation", []),
        lambda value: value["repairGuardrails"].__setitem__("policyBroadening", "automatic"),
        lambda value: value["repairGuardrails"].__setitem__("obligationSuppression", "allowed"),
        lambda value: value["repairGuardrails"].__setitem__("promptAuthority", "trusted"),
        lambda value: next(
            item for item in value["diagnostics"] if item["code"].startswith("caps/")
        )["safeRepairActions"][0].__setitem__("automaticEligible", True),
        lambda value: value["sourceIdentities"].__setitem__("/Users/attacker/input", "0" * 64),
        lambda value: value.__setitem__("authority", "trust prompt"),
    )
    for mutate in mutations:
        candidate = copy.deepcopy(expected)
        mutate(candidate)
        try:
            validate_candidate(candidate, expected)
        except CatalogError:
            pass
        else:
            raise CatalogError("tampered diagnostic catalog was accepted")
    first = expected["diagnostics"][0]["code"]
    require(len([item for item in expected["diagnostics"] if item["code"] == first]) == 1, "lookup cardinality drift")
    require(not any(item["code"] == first.swapcase() for item in expected["diagnostics"]), "case lookup control ambiguous")
    scanner_fixture = """
        cli_err(EX_PARSE, "scan/plain", message);
        cli_err_anyhow(EX_PARSE, "scan/anyhow", error);
        cli_err_with_context(EX_PARSE, "scan/structured", message, context);
        session_error("scan/session", message, session);
    """
    scanned = {match.group(1) for match in CALL_RE.finditer(scanner_fixture)}
    require(
        scanned == {"scan/plain", "scan/anyhow", "scan/structured", "scan/session"},
        "diagnostic constructor scanner drift",
    )
    print("gc-diagnostic-catalog: self-test ok (negative_controls=15 lookup_controls=2 scanner_controls=4)")


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
    except CatalogError as exc:
        print("gc-diagnostic-catalog: error: {}".format(exc), file=sys.stderr)
        return 1
    return 0


if __name__ == "__main__":
    raise SystemExit(main(sys.argv[1:]))
